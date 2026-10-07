//! 桌面悬浮窗（**独立 `WebviewWindow`**）的基础设施：懒创建 / 生命周期 / 位置记忆。
//!
//! ## 为什么是独立窗口，而不是主窗口里的浮层
//!
//! `PLAN §3.16` 把悬浮窗定为「本应用里**唯一**一个常驻桌面、脱离主窗口的 UI」。
//! 主页面里的 `position:fixed` 浮层做不到「脱离主窗口」（关掉主窗口它也没了），
//! 所以这里用 Tauri 的独立窗口（label = [`COMPANION_LABEL`]）。
//!
//! ## 为什么**懒创建**、不写进 `tauri.conf.json` 的 `windows[]`
//!
//! 写进配置会让它在**启动时无条件创建**，与「默认是否显示由用户偏好决定」冲突；
//! 而且 `transparent` / `always_on_top` / `skip_taskbar` 这些属性是悬浮窗专属，
//! 放在主窗口配置里并不适用。这里只在**首次需要显示时**创建（[`ensure`]），
//! 之后复用同一个窗口实例。
//!
//! ## 为什么位置记忆在 Rust 侧
//!
//! 窗口 `decorations(false)`，拖动由 `data-tauri-drag-region` 触发**原生**移动 ——
//! 最终坐标只有 Rust 侧知道（[`WindowEvent::Moved`]），前端拿不到「窗口落在哪」。
//! 因此位置的读 / 写都在这里，落盘目录与 `modules::config` 同源（`~/.buddy-switch/`）。
//!
//! ## 为什么不在这里起后台循环
//!
//! 悬浮窗的数据刷新走**前端 webview 内**的轮询 + 既有事件（P9 设计 §1.3），
//! 不新增任何 Rust 周期循环 —— 于是**不触碰** `lib.rs` 的 `BackgroundTask` 注册表，
//! 把「任务未登记 ⇒ 永不执行」这类风险归零。

use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::{
    AppHandle, Emitter, Manager, PhysicalPosition, Runtime, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder, Window, WindowEvent,
};

use buddy_switch_core::modules::config;

/// 悬浮窗的窗口 label；也是 `capabilities/companion.json` 里登记的 label。
///
/// ⚠️ 二者必须一致：新窗口若没有对应的 capability，拖拽 / 事件监听会**静默失败**
/// （不报错，只是不生效）—— 这是最难查的一类问题。单测
/// [`tests::capability_registers_companion_window`] 钉住这层耦合。
pub const COMPANION_LABEL: &str = "companion";

/// 可见性事件：主窗口侧栏与托盘勾选态靠它同步（沿用 `main-window-visible` 的既有模式）。
pub const VISIBILITY_EVENT: &str = "companion-visibility";

/// 窗口宽度（与原型 `demo.css` 的 `width:292px` 对齐）。
const WINDOW_WIDTH: f64 = 292.0;
/// 窗口高度（原型未固定高度，这里给一个起点；内容变化由前端自适应）。
const WINDOW_HEIGHT: f64 = 240.0;

/// 默认位置距屏幕右下角的边距（与原型 `right:18px; bottom:18px` 对齐）。
const EDGE_MARGIN: i32 = 18;

/// 位置记忆文件名（落在 [`config::store_dir`]，即 `~/.buddy-switch/`）。
const POSITION_FILE: &str = "companion_window.json";

/// 上次已落盘的位置，避免拖动过程中每一像素都写盘。
static LAST_PERSISTED: Mutex<Option<(i32, i32)>> = Mutex::new(None);

/// 落盘的位置（物理坐标）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
struct SavedPosition {
    x: i32,
    y: i32,
}

/// 显示器矩形（**物理坐标**，纯数据）。
///
/// 抽成纯结构是为了让「落屏校验」成为**不依赖 Tauri 运行时**的可单测逻辑 ——
/// 越界判定（[`resolve_position`]）是整个模块里最容易写错、也最值得守住的一段。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct MonitorRect {
    x: i32,
    y: i32,
    width: i32,
    height: i32,
}

impl MonitorRect {
    fn right(&self) -> i32 {
        self.x + self.width
    }

    fn bottom(&self) -> i32 {
        self.y + self.height
    }

    /// 左上角是否落在本显示器内（**半开区间**：右 / 下边界不算在内）。
    ///
    /// 用半开区间是为了让「恰好贴在右边界外一像素」的越界坐标一定被判为需要回落。
    fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.x && x < self.right() && y >= self.y && y < self.bottom()
    }

    /// 本显示器**右下角**、留出边距后，窗口左上角应有的坐标。
    fn bottom_right_anchor(&self, width: i32, height: i32, margin: i32) -> (i32, i32) {
        (self.right() - width - margin, self.bottom() - height - margin)
    }
}

/// 落屏校验 + 默认位置解析（**纯逻辑**，单测覆盖）。
///
/// 规则：
/// - 记忆位置若仍落在**任一**显示器内 ⇒ 用它；
/// - 否则（外接屏被拔掉 / 分辨率变化导致坐标越界）⇒ 回落**主显示器右下角**。
///
/// 这正是 P9 设计 R4 的缓解：窗口不会「落在已断开的显示器上」，让用户以为打不开
/// （实为看不见）。
fn resolve_position(
    stored: Option<(i32, i32)>,
    monitors: &[MonitorRect],
    primary: MonitorRect,
    width: i32,
    height: i32,
    margin: i32,
) -> (i32, i32) {
    if let Some((x, y)) = stored {
        if monitors.iter().any(|monitor| monitor.contains(x, y)) {
            return (x, y);
        }
    }
    primary.bottom_right_anchor(width, height, margin)
}

/// 位置记忆文件路径（与 `modules::config` 同源）。
fn position_file() -> std::path::PathBuf {
    config::store_dir().join(POSITION_FILE)
}

/// 读取记忆位置；文件缺失 / 损坏一律视为「无记忆」（回落默认右下角）。
fn load_position() -> Option<(i32, i32)> {
    let text = std::fs::read_to_string(position_file()).ok()?;
    let saved: SavedPosition = serde_json::from_str(&text).ok()?;
    Some((saved.x, saved.y))
}

/// 持久化窗口位置（物理坐标）；位置没变则跳过写盘。
fn save_position(x: i32, y: i32) {
    {
        let mut last = LAST_PERSISTED
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if *last == Some((x, y)) {
            return;
        }
        *last = Some((x, y));
    }
    let _ = std::fs::create_dir_all(config::store_dir());
    let content = serde_json::to_string_pretty(&SavedPosition { x, y }).unwrap_or_default();
    let _ = config::atomic_write(&position_file(), &content);
}

/// 把 Tauri 的显示器列表转成纯数据矩形（物理坐标），并挑出主显示器。
fn monitor_rects<R: Runtime>(app: &AppHandle<R>) -> (Vec<MonitorRect>, MonitorRect) {
    let to_rect = |monitor: &tauri::window::Monitor| MonitorRect {
        x: monitor.position().x,
        y: monitor.position().y,
        width: monitor.size().width as i32,
        height: monitor.size().height as i32,
    };

    let monitors: Vec<MonitorRect> = app
        .available_monitors()
        .unwrap_or_default()
        .iter()
        .map(to_rect)
        .collect();

    // 主显示器优先；取不到就退到第一块屏；再取不到给一个保守默认值。
    let primary = app
        .primary_monitor()
        .ok()
        .flatten()
        .as_ref()
        .map(to_rect)
        .or_else(|| monitors.first().copied())
        .unwrap_or(MonitorRect {
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
        });

    (monitors, primary)
}

/// 确保悬浮窗存在（**懒创建**）；已存在则直接返回。
///
/// 位置解析见 [`resolve_position`]：优先记忆位置，越界则回落主显示器右下角。
pub fn ensure<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<WebviewWindow<R>> {
    if let Some(window) = app.get_webview_window(COMPANION_LABEL) {
        return Ok(window);
    }

    let (monitors, primary) = monitor_rects(app);
    let (x, y) = resolve_position(
        load_position(),
        &monitors,
        primary,
        WINDOW_WIDTH as i32,
        WINDOW_HEIGHT as i32,
        EDGE_MARGIN,
    );

    let window = WebviewWindowBuilder::new(
        app,
        COMPANION_LABEL,
        WebviewUrl::App("index.html?window=companion".into()),
    )
    .title("会话状态")
    .inner_size(WINDOW_WIDTH, WINDOW_HEIGHT)
    .decorations(false) // 无系统标题栏 ⇒ 自管焦点
    .transparent(true) // 圆角卡片外观
    .always_on_top(true) // 「常驻桌面」
    .skip_taskbar(true) // 不占任务栏
    .resizable(false)
    .shadow(false) // 与 transparent 配合，避免方角阴影
    .visible(false) // 首次显示由 set_visible 决定
    .build()?;

    // 用**物理坐标**定位：`available_monitors()` 给的是物理坐标，而 builder 的
    // `.position()` 按**逻辑坐标**解释 —— 在 HiDPI 屏上会差一个缩放因子。先建成
    // 隐藏窗口再 `set_position(PhysicalPosition)` 可绕开这层换算（此时未显示，无闪烁）。
    let _ = window.set_position(PhysicalPosition::new(x, y));

    Ok(window)
}

/// 显示 / 隐藏悬浮窗（显示时懒创建），并广播可见性变化。
///
/// ★ 这是可见性的**唯一收敛点**：侧栏图标、托盘菜单、窗口内关闭按钮都经它改变状态，
/// 因此「不许出现第二个看起来能切的控件」这条纪律靠它守住。
pub fn set_visible<R: Runtime>(app: &AppHandle<R>, visible: bool) -> tauri::Result<()> {
    if visible {
        let window = ensure(app)?;
        window.show()?;
        window.set_focus()?;
    } else if let Some(window) = app.get_webview_window(COMPANION_LABEL) {
        window.hide()?;
    }
    let _ = app.emit(VISIBILITY_EVENT, visible);
    Ok(())
}

/// 便捷封装：翻转可见性，返回翻转**之后**的状态。
pub fn toggle<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<bool> {
    let next = !is_visible(app);
    set_visible(app, next)?;
    Ok(next)
}

/// 当前是否可见（窗口尚未创建 ⇒ 视为不可见）。
pub fn is_visible<R: Runtime, M: Manager<R>>(app: &M) -> bool {
    app.get_webview_window(COMPANION_LABEL)
        .and_then(|window| window.is_visible().ok())
        .unwrap_or(false)
}

/// 悬浮窗的窗口事件：位置记忆（[`WindowEvent::Moved`]）+ 关闭 = 隐藏。
///
/// 与 `tray.rs::on_window_event`（只管 `main`）互补：两者都从 `lib.rs` 的
/// `on_window_event` 转发进来，各按自己的 label 短路。
pub fn on_window_event<R: Runtime>(window: &Window<R>, event: &WindowEvent) {
    if window.label() != COMPANION_LABEL {
        return;
    }
    match event {
        WindowEvent::Moved(position) => save_position(position.x, position.y),
        WindowEvent::CloseRequested { api, .. } => {
            // 关闭 = 隐藏（**不销毁**）：可从侧栏左下角 / 托盘再次打开。
            // 与主窗口 `tray.rs` 同一套「关闭 = 隐藏」纪律。
            api.prevent_close();
            let _ = window.hide();
            let _ = window.app_handle().emit(VISIBILITY_EVENT, false);
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 主显示器（1920x1080 @ 原点）。
    const PRIMARY: MonitorRect = MonitorRect {
        x: 0,
        y: 0,
        width: 1920,
        height: 1080,
    };

    const WIDTH: i32 = WINDOW_WIDTH as i32;
    const HEIGHT: i32 = WINDOW_HEIGHT as i32;

    /// 记忆位置仍在屏内 ⇒ **原样保留**（不打扰用户摆好的位置）。
    #[test]
    fn keeps_stored_position_when_still_on_a_monitor() {
        assert_eq!(
            resolve_position(Some((300, 200)), &[PRIMARY], PRIMARY, WIDTH, HEIGHT, EDGE_MARGIN),
            (300, 200)
        );
    }

    /// ★ 核心护栏（R4）：外接屏被拔掉后，记忆位置落在屏幕外 ⇒ **回落主显示器右下角**。
    ///
    /// 可证伪性：把 [`resolve_position`] 里的落屏校验去掉（无条件用 `stored`）⇒ 变红。
    #[test]
    fn falls_back_to_primary_bottom_right_when_stored_position_is_offscreen() {
        let expected = PRIMARY.bottom_right_anchor(WIDTH, HEIGHT, EDGE_MARGIN);
        assert_eq!(expected, (1920 - WIDTH - 18, 1080 - HEIGHT - 18));
        assert_eq!(
            resolve_position(
                Some((2500, 300)), // 落在 1920 之外：外接屏已断开
                &[PRIMARY],
                PRIMARY,
                WIDTH,
                HEIGHT,
                EDGE_MARGIN,
            ),
            expected
        );
    }

    /// 副屏在主屏**左侧**（负坐标）—— 只要仍落在任一显示器内就必须保留，
    /// 不能因为「负坐标」就误判为越界。
    #[test]
    fn keeps_stored_position_when_it_lands_on_a_secondary_monitor() {
        let secondary = MonitorRect {
            x: -1280,
            y: 0,
            width: 1280,
            height: 1024,
        };
        assert_eq!(
            resolve_position(
                Some((-500, 400)),
                &[PRIMARY, secondary],
                PRIMARY,
                WIDTH,
                HEIGHT,
                EDGE_MARGIN,
            ),
            (-500, 400)
        );
    }

    /// 没有记忆位置 ⇒ 默认主显示器右下角。
    #[test]
    fn missing_stored_position_uses_default_bottom_right() {
        assert_eq!(
            resolve_position(None, &[PRIMARY], PRIMARY, WIDTH, HEIGHT, EDGE_MARGIN),
            PRIMARY.bottom_right_anchor(WIDTH, HEIGHT, EDGE_MARGIN)
        );
    }

    /// 边界判定用**半开区间**：右下边界不算在内。
    #[test]
    fn monitor_contains_uses_half_open_bounds() {
        assert!(PRIMARY.contains(0, 0));
        assert!(PRIMARY.contains(1919, 1079));
        assert!(!PRIMARY.contains(1920, 0), "右边界外一像素必须判为越界");
        assert!(!PRIMARY.contains(0, 1080), "下边界外一像素必须判为越界");
        assert!(!PRIMARY.contains(-1, 0), "主屏左边界外必须判为越界");
    }

    /// ★ 护栏：`capabilities/companion.json` **必须**登记 companion 窗口的 label。
    ///
    /// 回归背景（P9 设计 R2）：新窗口若没有对应的 capability，拖拽 / 事件监听会
    /// **静默失败**（不报错，只是不生效）—— 这是最难查的一类问题。
    ///
    /// 可证伪性：把 label 从 capability 的 `windows` 里删掉（T05 变异④）⇒ 本用例变红。
    #[test]
    fn capability_registers_companion_window() {
        let raw = include_str!("../capabilities/companion.json");
        let value: serde_json::Value =
            serde_json::from_str(raw).expect("capabilities/companion.json 必须是合法 JSON");

        let windows = value
            .get("windows")
            .and_then(|value| value.as_array())
            .expect("capability 必须有 windows 数组");
        assert!(
            windows
                .iter()
                .any(|label| label.as_str() == Some(COMPANION_LABEL)),
            "capabilities/companion.json 的 windows 必须包含 {COMPANION_LABEL}，\
             否则新窗口没有 core 权限，拖拽 / 事件监听会静默失败"
        );

        let permissions = value
            .get("permissions")
            .and_then(|value| value.as_array())
            .expect("capability 必须有 permissions 数组");
        assert!(
            permissions
                .iter()
                .any(|perm| perm.as_str() == Some("core:window:allow-start-dragging")),
            "companion 依赖 data-tauri-drag-region 触发原生拖动，\
             必须授权 core:window:allow-start-dragging"
        );
    }
}
