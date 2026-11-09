//! Qoder **网页端登录窗** + 会话 Cookie 收割（issue #111 阶段一）。
//!
//! ## 为什么需要这个窗口
//!
//! 逐资源包明细只存在于网页端私有接口，且**只认浏览器登录后的会话 Cookie**。
//! 本仓 Qoder 的登录链路是「设备码 + 系统浏览器」，全程不经内嵌 webview，
//! 因此进程从不接触 cookie store —— 这是「拿不到逐包」的真正原因（不是 httpOnly）。
//!
//! 这里补的就是那条缺失的链路：一个独立的 webview 窗口载
//! `{website}/account/usage`，用户登录一次后，用 `Webview::cookies_for_url()` 收割。
//!
//! ## 三条硬约束（都踩过坑，别省）
//!
//! 1. ★★ **收割必须是 async command + 独立线程**。`Webview::cookies()` 在 Windows 上
//!    于**同步 command 或事件处理器**里会死锁（Tauri 文档 Known issues / wry#583）。
//!    本模块把 `cookies_for_url()` 放进 `spawn_blocking`，不要「顺手」改成同步调用。
//! 2. ★★ **Cookie 明文不出这个模块**。它等价于完整身份凭据：只拼成 `Cookie:` 头交给
//!    core 的请求函数，返回值里只有**名称与条数**，不落日志、不进错误消息。
//! 3. ★ **只对桌面宿主成立**。Docker / npm CLI / 裸二进制没有 Tauri、没有 `Webview`
//!    ⇒ 对应命令在 `buddy-switch-server` 侧返回结构化「仅桌面端可用」，
//!    **不得**返回空包列表冒充「该账号无包」。
//!
//! ## 读取时机（#111 复核补的两处，别删）
//!
//! 收割**只在窗口活着时**做得到（`cookies_for_url` 需要 `Webview` 句柄）。原来的实现
//! 把「什么时候读」留给用户点按钮，结果：登录完关窗 ⇒ 窗口没了 ⇒ 缓存从未被写过 ⇒
//! 渲染路径永远 `no_session`。现在有两个读取时机，都不依赖用户额外操作：
//!
//! 1. **窗口关之前**：[`install_close_harvest`] 拦下 `CloseRequested`，先收割再销毁；
//! 2. **窗口开着时**：[`window_cookies`] 供前端轮询（只回名称 / 条数 / 指纹，不打上游）。
//!
//! 两者都把 Cookie **暂存进进程内存**（[`stage_cookies`]），于是**不需要 `AppHandle`
//! 的渲染路径**（[`credit_packs`]）也能用上刚登录的那份会话 —— 这是原来那个闭环依赖
//! （缓存为空 ⇒ 走不到写缓存的那一行）的解药。
//!
//! ## 多账号（#155，别退回单窗口 / 单文件）
//!
//! 用户会同时挂多个 Qoder 账号，每个都要看自己的逐包。三处**都**必须按账号分家，缺一处
//! 就会「登录 B 顶掉 A」：
//!
//! 1. 窗口：label 由 [`login_label`] 按区域 + uid 派生，且每个窗口一份独立 webview 数据
//!    目录（Windows `data_directory` / 其他平台 `incognito`）—— 否则 cookie store 共享；
//! 2. 暂存：`HashMap<区域+uid, …>`，收割与认领都按 uid 取放；
//! 3. 落盘：`web_session.{区域}.{uid}.json`（见 `buddy_switch_core::…::web_session`）。

use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::sync::Mutex;

use serde_json::{json, Value};
use tauri::{
    AppHandle, Manager, Runtime, Url, WebviewUrl, WebviewWindow, WebviewWindowBuilder, WindowEvent,
};

use buddy_switch_core::modules::product::{require_qoder_region, resolve_scope_param};
use buddy_switch_core::modules::qoder::region::{self, QoderRegion};
use buddy_switch_core::modules::qoder::web_proxy;
use buddy_switch_core::modules::qoder::web_session;
use buddy_switch_core::modules::qoder::web_usage;
use buddy_switch_core::modules::runtime_log;

/// 登录窗 label 的前缀；实际 label 由 [`login_label`] 按**区域 + 账号**派生。
///
/// ★ 为什么必须每账号一个窗口（#155）：cookie store 是**进程级共享**的，单个窗口意味着
/// 登录第二个账号就顶掉第一个 —— 多账号用户的逐包明细永远只剩最后一个。窗口分家还不够，
/// 每个窗口的 webview 数据目录也必须分家（见 [`ensure_login_window`]），否则两个窗口
/// 共用同一份 cookie 罐，照样互相顶。
pub const LOGIN_LABEL_PREFIX: &str = "qoder-web-login";

/// 某账号登录窗的 label。
///
/// 含 `uid` 的稳定指纹（[`web_session::uid_file_token`]）：窗口 label 与 Cookie 槽位用
/// **同一套**推导，两边不可能对不上。
pub fn login_label(region: QoderRegion, uid: &str) -> String {
    format!(
        "{LOGIN_LABEL_PREFIX}-{}-{}",
        region.as_str(),
        web_session::uid_file_token(uid)
    )
}

/// 登录窗尺寸（网页端用量页是桌面版布局，窄了会挤成两行）。
const WINDOW_WIDTH: f64 = 1024.0;
const WINDOW_HEIGHT: f64 = 780.0;

/// uid 的归一形式（与 [`buddy_switch_core::modules::qoder::web_session::WebSession::matches_uid`]
/// 一致：trim + 小写）。暂存键、窗口 label、Cookie 槽位都靠它对齐。
fn uid_key(uid: &str) -> String {
    uid.trim().to_ascii_lowercase()
}

/// uid 必填校验（网页会话的**归属**就是它，缺了等于不知道给谁存）。
fn require_uid(uid: &str) -> Result<(), String> {
    if uid_key(uid).is_empty() {
        return Err("缺少 uid".to_string());
    }
    Ok(())
}

/// 确保**该账号**的登录窗存在并导航到该区域的用量页。
///
/// ## 多账号隔离（#155）
///
/// 同一个 `Webview` 进程里，cookie store 默认是共享的：给第二个账号开窗，两个窗口读到的
/// 是同一份 Cookie ⇒ 谁最后登录谁赢。隔离手段按平台分：
///
/// - **Windows**：`data_directory` 给每个账号一份 WebView2 user data folder（wry 以它作
///   `WebContext` 的 key，profile 完全分家）；
/// - **macOS / Linux**：`data_directory` 不支持（tauri 文档明确），改用 `incognito`——
///   wry 在 macOS 用 `nonPersistentDataStore`、Linux 用 ephemeral `WebContext`，同样是
///   每窗口一份。窗口关闭即丢 Cookie 无所谓：收割后就落盘了。
///
/// ⚠️ 两条不能混用：`incognito` 会**忽略** `data_directory`（wry 语义），所以只能二选一。
fn ensure_login_window<R: Runtime>(
    app: &AppHandle<R>,
    region: QoderRegion,
    uid: &str,
) -> Result<WebviewWindow<R>, String> {
    let url = web_usage::usage_page_url(region);
    let parsed: Url = url
        .parse()
        .map_err(|error| format!("登录页地址非法: {error}"))?;
    let label = login_label(region, uid);

    if let Some(window) = app.get_webview_window(&label) {
        let _ = window.show();
        let _ = window.set_focus();
        return Ok(window);
    }

    let mut builder = WebviewWindowBuilder::new(app, &label, WebviewUrl::External(parsed))
        .title(format!(
            "登录 Qoder 网页端（{} · 读取逐包明细）",
            region.as_str()
        ))
        .inner_size(WINDOW_WIDTH, WINDOW_HEIGHT)
        .min_inner_size(720.0, 560.0)
        .center();

    #[cfg(target_os = "windows")]
    {
        // 每个账号一份 WebView2 user data folder。建目录失败就只能退回共享 profile
        // （多账号会互相顶），所以记一行运行时日志，别让它静默降级。
        match app.path().app_data_dir() {
            Ok(dir) => {
                let profile = dir.join("qoder-web-profiles").join(format!(
                    "{}-{}",
                    region.as_str(),
                    web_session::uid_file_token(uid)
                ));
                if let Err(error) = std::fs::create_dir_all(&profile) {
                    runtime_log::warn(
                        "qoder-web",
                        format!("建 profile 目录失败，多账号隔离将失效: {error}"),
                    );
                } else {
                    // ★ 每次开窗记一行「这个窗口用的是哪份 profile」：多账号互相顶的
                    //   报障第一手证据就是这里两个窗口指向同一个目录（或没有这一行）。
                    runtime_log::info(
                        "qoder-web",
                        format!(
                            "登录窗 {label} · 区域={} · uid={uid} · profile={}",
                            region.as_str(),
                            profile.display()
                        ),
                    );
                    builder = builder.data_directory(profile);
                }
            }
            Err(error) => runtime_log::warn(
                "qoder-web",
                format!("取 appDataDir 失败，多账号隔离将失效: {error}"),
            ),
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        // `data_directory` 只在 Windows 生效；其它平台靠 incognito 拿每窗口一份的
        // nonPersistent / ephemeral 数据存储（wry 语义）。
        runtime_log::info(
            "qoder-web",
            format!(
                "登录窗 {label} · 区域={} · uid={uid} · 数据存储=incognito",
                region.as_str()
            ),
        );
        builder = builder.incognito(true);
    }

    let window = builder
        .build()
        .map_err(|error| format!("打开 Qoder 登录窗失败: {error}"))?;
    // ★ 关窗前兜底收割（#111 复核 P1）：没有它，「登录完直接关窗」= 白登录。
    install_close_harvest(&window, region, uid);
    Ok(window)
}

/// 读**某个账号**登录窗里现有的 Cookie **名称与条数**（本地命令，**不打上游**）。
///
/// 前端在登录窗开着时轮询它：`fingerprint` 变了 ⇒ 用户在窗口里完成了登录 ⇒ 再去探一次
/// 逐包。为什么不让前端每 3 秒直接调 probe：probe 一旦拿到非空 Cookie 就会真打上游，
/// 未登录状态下的匿名 Cookie 换来 401，而 401 又会立刻清缓存 —— 40 次轮询等于对上游
/// 打一串注定失败的请求，还每次抹掉刚写的缓存（#111 复核 §5 P0 的 ⚠️）。
///
/// 副作用（有意）：**顺手暂存**这份 Cookie。于是渲染路径（[`credit_packs`]，没有
/// `AppHandle`）也能用上它，不必等 probe 落盘。
pub async fn window_cookies<R: Runtime>(
    app: &AppHandle<R>,
    scope: String,
    uid: String,
) -> Result<Value, String> {
    let scope = resolve_scope_param(Some(&scope))?;
    let region = require_qoder_region(scope)?;
    require_uid(&uid)?;
    let Some(window) = app.get_webview_window(&login_label(region, &uid)) else {
        return Ok(json!({
            "windowOpen": false,
            "cookieCount": 0,
            "cookieNames": Vec::<String>::new(),
            "fingerprint": "",
        }));
    };
    let Some((header, names)) = harvest(window, region).await else {
        return Ok(json!({
            "windowOpen": true,
            "cookieCount": 0,
            "cookieNames": Vec::<String>::new(),
            "fingerprint": "",
        }));
    };
    stage_cookies(region, &uid, &header, &names);
    Ok(json!({
        "windowOpen": true,
        // ⚠️ 只给名称、条数与指纹，**不给值**。
        "cookieCount": names.len(),
        "cookieNames": names,
        "fingerprint": cookie_fingerprint(&header),
    }))
}

/// 把该区域的会话 Cookie 拼成 `Cookie:` 头。
///
/// 只保留域名属于该区域的条目：cookie store 是**进程级共享**的，若不筛，
/// 会把别的站点的 cookie 一起发给 Qoder（既无意义，也是不必要的凭据外泄面）。
fn cookie_header_for<R: Runtime>(
    window: &WebviewWindow<R>,
    region: QoderRegion,
) -> Result<(String, Vec<String>), String> {
    let spec = region::realm_spec(region);
    let target: Url = web_usage::credits_url(region)
        .parse()
        .map_err(|error| format!("逐包接口地址非法: {error}"))?;
    let cookies = window
        .cookies_for_url(target)
        .map_err(|error| format!("读取网页会话 Cookie 失败: {error}"))?;

    let mut pairs: Vec<String> = Vec::new();
    let mut names: Vec<String> = Vec::new();
    for cookie in cookies {
        let domain = cookie.domain().unwrap_or_default();
        // `domain` 可能带前导点（`.qoder.com.cn`）；用后缀匹配覆盖两种形态。
        if !domain.is_empty() && !domain.trim_start_matches('.').ends_with(spec.domain) {
            continue;
        }
        let name = cookie.name().to_string();
        if names.contains(&name) {
            continue; // 同名多 path 只取第一条，避免头部重复
        }
        names.push(name.clone());
        pairs.push(format!("{name}={}", cookie.value()));
    }
    Ok((pairs.join("; "), names))
}

/// 关窗前暂存的会话 Cookie 的保鲜期（毫秒）。
///
/// 只覆盖「用户刚登录 → 立刻去点逐包」这一小段时间；10 分钟足够，长了反而会让一份
/// 早已作废的 Cookie 一直冒充「刚登录的会话」。
const STAGED_TTL_MS: i64 = 10 * 60 * 1000;

/// 进程内暂存的一份会话 Cookie。
///
/// ★ 为什么要有它：落盘需要 `uid`（归属校验），而**窗口关闭事件里不知道用户在登哪个
/// 账号**。所以关窗前只能先暂存，等随后的 probe（带 uid）来认领。`Debug` 手写，
/// 永不出原文。
///
/// ★ 按账号分槽（#155）：单槽时两个窗口并行登录会互相覆盖 —— A 收割的 Cookie 被 B 顶掉，
/// 随后 A 的 probe 拿 B 的会话去认领，必然是「归属不符」。
struct StagedCookies {
    region: QoderRegion,
    header: String,
    names: Vec<String>,
    staged_at_ms: i64,
}

impl std::fmt::Debug for StagedCookies {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StagedCookies")
            .field("region", &self.region)
            .field("cookies", &self.names.len())
            .field("staged_at_ms", &self.staged_at_ms)
            .finish()
    }
}

/// 暂存槽的键：区域 + uid（uid 归一为小写，与 `matches_uid` 的语义一致）。
fn staged_key(region: QoderRegion, uid: &str) -> String {
    format!("{}:{}", region.as_str(), uid_key(uid))
}

fn staged_slot() -> &'static Mutex<HashMap<String, StagedCookies>> {
    static SLOT: std::sync::OnceLock<Mutex<HashMap<String, StagedCookies>>> =
        std::sync::OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(HashMap::new()))
}

/// 暂存一份刚收割到的 Cookie（空串直接忽略；同账号后写覆盖先写）。
fn stage_cookies(region: QoderRegion, uid: &str, header: &str, names: &[String]) {
    if header.trim().is_empty() || uid_key(uid).is_empty() {
        return;
    }
    let mut guard = match staged_slot().lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    guard.insert(
        staged_key(region, uid),
        StagedCookies {
            region,
            header: header.to_string(),
            names: names.to_vec(),
            staged_at_ms: web_usage_now_ms(),
        },
    );
}

/// 取暂存（按区域 + 账号、判保鲜期）。**不清除** —— 认领方拿到 uid 校验通过后才落盘。
fn staged_cookies(region: QoderRegion, uid: &str) -> Option<(String, Vec<String>, i64)> {
    let mut guard = match staged_slot().lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    let key = staged_key(region, uid);
    let staged = guard.get(&key)?;
    if web_usage_now_ms().saturating_sub(staged.staged_at_ms) >= STAGED_TTL_MS {
        guard.remove(&key);
        return None;
    }
    Some((
        staged.header.clone(),
        staged.names.clone(),
        staged.staged_at_ms,
    ))
}

/// 丢弃**该账号**的暂存（认领完成 / 判定为别人的会话之后）。
///
/// ★ 不能清整张表：同区域的另一个账号可能正等着它的 probe 来认领。
fn clear_staged(region: QoderRegion, uid: &str) {
    let mut guard = match staged_slot().lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    guard.remove(&staged_key(region, uid));
}

/// Cookie 集合的指纹：只用于判断「集合变没变」（前端据此决定要不要探一次上游）。
///
/// ★ 刻意**不可逆、不含明文**：`DefaultHasher` 的输出不能还原成 Cookie，前端拿到它
/// 也只能做相等比较。它不是凭据，但也没必要让前端看见任何可还原的东西。
fn cookie_fingerprint(header: &str) -> String {
    let mut hasher = DefaultHasher::new();
    header.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

/// 从窗口收割 Cookie（**异步 + 独立线程**；见文件头第 1 条硬约束）。
async fn harvest<R: Runtime>(
    window: WebviewWindow<R>,
    region: QoderRegion,
) -> Option<(String, Vec<String>)> {
    let pair = tauri::async_runtime::spawn_blocking(move || cookie_header_for(&window, region))
        .await
        .ok()?
        .ok()?;
    let (header, names) = pair;
    if header.trim().is_empty() {
        return None;
    }
    Some((header, names))
}

/// 「正在关窗前收割」的标记集合：连点两次 × 时同一个窗口不重复起两个收割任务。
///
/// ★ 按**窗口 label** 记（#155）：多账号会有多个登录窗，用一个全局 `AtomicBool` 的话
/// 关 A 窗时正在收割，B 窗的关闭请求会被无限期 `prevent_close`（看起来「× 按不动」）。
fn harvesting_slots() -> &'static Mutex<HashSet<String>> {
    static SLOTS: std::sync::OnceLock<Mutex<HashSet<String>>> = std::sync::OnceLock::new();
    SLOTS.get_or_init(|| Mutex::new(HashSet::new()))
}

fn try_begin_harvest(label: &str) -> bool {
    match harvesting_slots().lock() {
        Ok(mut guard) => guard.insert(label.to_string()),
        Err(poisoned) => poisoned.into_inner().insert(label.to_string()),
    }
}

fn end_harvest(label: &str) {
    let mut guard = match harvesting_slots().lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    guard.remove(label);
}

/// 给**某账号**的登录窗装「关窗前先收割」的钩子。
///
/// ★ 为什么必须拦：用户登录完**直接关窗**是最自然的操作。窗口一销毁，本进程就再没有
/// 任何能调用 `cookies_for_url` 的句柄，那份会话就白登录了（#111 复核 §2.5）。
///
/// ★ 为什么是 `prevent_close` + 异步再关：`cookies()` 在**事件处理器里同步调用会在
/// Windows 上死锁**（wry#583）。所以先拦住，起任务收割，再用 `destroy()` 关 ——
/// `destroy()` 不触发 `CloseRequested`，不会自己把自己拦住。
fn install_close_harvest<R: Runtime>(window: &WebviewWindow<R>, region: QoderRegion, uid: &str) {
    let handle = window.clone();
    let label = window.label().to_string();
    let uid = uid.to_string();
    window.on_window_event(move |event| {
        let WindowEvent::CloseRequested { api, .. } = event else {
            return;
        };
        if !try_begin_harvest(&label) {
            // 已在收割：继续拦（否则这次会抢在收割完成前把窗口销毁）。
            api.prevent_close();
            return;
        }
        api.prevent_close();
        let handle = handle.clone();
        let uid = uid.clone();
        let label = label.clone();
        tauri::async_runtime::spawn(async move {
            if let Some((header, names)) = harvest(handle.clone(), region).await {
                stage_cookies(region, &uid, &header, &names);
            }
            end_harvest(&label);
            let _ = handle.destroy();
        });
    });
}

/// 打开（或前置）**某账号**的 Qoder 网页端登录窗。
///
/// 前端入口：账号卡 / 抽屉额度区的「登录 Qoder 网页端以获取逐包明细」按钮。
///
/// ★ 必须带 `uid`：窗口按账号分家（[`login_label`]），cookie 槽位也按账号分家 ——
/// 只按区域开窗的话，登第二个账号会顶掉第一个（#155）。
///
/// ⚠️ 本模块只提供实现；`#[tauri::command]` 的薄包装在 `commands.rs`
/// （`scripts/check-api-contract.cjs` 只认 `commands::<name>` 形态的登记，写在别处会被判「未登记」）。
pub async fn login_open<R: Runtime>(
    app: &AppHandle<R>,
    scope: String,
    uid: String,
) -> Result<Value, String> {
    let scope = resolve_scope_param(Some(&scope))?;
    let region = require_qoder_region(scope)?;
    require_uid(&uid)?;
    let window = ensure_login_window(app, region, &uid)?;
    let _ = window.show();
    let _ = window.set_focus();
    Ok(json!({
        "opened": true,
        "url": web_usage::usage_page_url(region),
        // 多账号：同一时刻可能开着多个窗，前端按 label 区分（也是「谁在登」的唯一凭据）。
        "label": login_label(region, &uid),
    }))
}

/// 关闭**某账号**的登录窗（已关闭 ⇒ 幂等成功）。
///
/// ★ 只关这一个：多账号场景下「退出 A 的登录窗」不能把 B 的窗一起关掉。
pub async fn login_close<R: Runtime>(
    app: &AppHandle<R>,
    scope: String,
    uid: String,
) -> Result<Value, String> {
    let scope = resolve_scope_param(Some(&scope))?;
    let region = require_qoder_region(scope)?;
    require_uid(&uid)?;
    let closed = match app.get_webview_window(&login_label(region, &uid)) {
        Some(window) => {
            window
                .close()
                .map_err(|error| format!("关闭 Qoder 登录窗失败: {error}"))?;
            true
        }
        None => false,
    };
    Ok(json!({ "closed": closed }))
}

/// 收割会话 Cookie 并探一次逐包接口（**阶段零-2 取证**）。
///
/// 返回体刻意包含 `raw`：本阶段的唯一目的是拿到真实响应形状来设计解析器
/// （`shape` 只给键名与计数，`raw` 给完整样本）。⚠️ 两者都**不含 Cookie**。
///
/// ## 会话来源（`source` 字段）
///
/// 1. `window` —— 登录窗里现读的 Cookie（用户刚登录）；
/// 2. `staged` —— 窗口刚被关掉，但关窗前已把 Cookie 暂存在内存里（[`stage_cookies`]）；
/// 3. `stored` —— 窗口里没有、暂存也没有，但本地缓存里有一份**新鲜的、uid 匹配**的会话
///    （用户之前登录过，这次只是没开窗）。少这一步的话，每次取数都要重新登录一次。
///
/// 上游 401/403 ⇒ 本地缓存立刻作废（那份 Cookie 已经死了，留着只会反复失败）。
pub async fn credit_packs_probe<R: Runtime>(
    app: &AppHandle<R>,
    scope: String,
    uid: String,
) -> Result<Value, String> {
    let scope = resolve_scope_param(Some(&scope))?;
    let region = require_qoder_region(scope)?;
    require_uid(&uid)?;

    // ① 该账号的登录窗（可能没开 —— 没开不等于失败，下面还有暂存与本地缓存）。
    let mut names: Vec<String> = Vec::new();
    let mut header = String::new();
    let mut source = "none";
    if let Some(window) = app.get_webview_window(&login_label(region, &uid)) {
        // ★★ 必须离开当前线程读 cookie：Windows 上 `cookies()` 在同步上下文会死锁
        //    （Tauri Known issues / wry#583）。`harvest` 内部即 `spawn_blocking`。
        if let Some((header_from_window, window_names)) = harvest(window, region).await {
            // ★ 顺手暂存：探针万一因网络抖动失败，这份会话也不会随窗口一起消失。
            stage_cookies(region, &uid, &header_from_window, &window_names);
            header = header_from_window;
            names = window_names;
            source = "window";
        }
    }

    // ② 关窗前暂存（用户刚登录完就把窗口关了 —— 最自然的操作序列）。
    if header.is_empty() {
        if let Some((staged_header, staged_names, _)) = staged_cookies(region, &uid) {
            header = staged_header;
            names = staged_names;
            source = "staged";
        }
    }

    // ③ 本地缓存（保鲜 + uid 归属双重校验；**只读该账号自己的槽位**）。
    if header.is_empty() {
        let now = web_usage_now_ms();
        if let Some(session) = web_session::load(region, &uid) {
            if session.is_fresh(now) {
                header = session.cookie;
                names = session.cookie_names;
                source = "stored";
            }
        }
    }

    if header.is_empty() {
        return Ok(json!({
            "hasSession": false,
            "source": source,
            "cookieCount": 0,
            "cookieNames": Vec::<String>::new(),
            "message": "还没有可用的网页会话：请先点击「登录 Qoder 网页端」并在窗口中完成登录。",
        }));
    }

    let raw = match web_usage::fetch_credit_packs_raw(region, &header).await {
        Ok(raw) => raw,
        Err(error) => {
            // ★ 只有「上游明确拒绝」才清缓存；网络抖动不清（否则一次抖动就掉登录态）。
            if error.unauthorized {
                let _ = web_session::clear(region, &uid);
            }
            return Err(error.message);
        }
    };
    let matches = web_usage::owner_matches(&raw, &uid);
    if matches {
        // 归属对得上 ⇒ 这份会话值得留下来，下次不必重新登录。
        let now = web_usage_now_ms();
        let _ = web_session::save(
            region,
            &uid,
            &web_session::WebSession {
                uid: uid.trim().to_string(),
                cookie: header,
                cookie_names: names.clone(),
                saved_at_ms: now,
                verified_at_ms: now,
            },
        );
        // 已落盘 ⇒ 内存里那份暂存没有用处了（留着只会让下次读到一份更旧的副本）。
        clear_staged(region, &uid);
    } else {
        // ★ 归属不符 ⇒ 用户在这个窗口里登了**另一个**账号。
        //    只丢这份暂存（它确实是别人的），**不动**已落盘的会话：落盘那份是当时
        //    通过 `owner_matches` 验证过的，不能因为一次登错窗口就把它删掉（#155）。
        clear_staged(region, &uid);
    }
    Ok(json!({
        "hasSession": true,
        "source": source,
        // ⚠️ 只给名称与条数，**不给值**。
        "cookieCount": names.len(),
        "cookieNames": names,
        "uidMatches": matches,
        "shape": web_usage::shape_of(&raw),
        // 解析结果（S3）：真样本锚定的字段映射，前端 S6 直接用它渲染。
        "packs": web_usage::parse_credit_packs(&raw),
        "raw": raw,
    }))
}

/// 读网页会话取逐包明细（S6 的数据源；不弹登录窗、不需要 `AppHandle`）。
///
/// 与 [`credit_packs_probe`] 的分工：
/// - probe 是**认领**用的（能开窗、会返回 `raw`，成功后把会话落盘）；
/// - 本函数是**渲染**用的：只读「已落盘会话 → 关窗前暂存」，不返回 `raw`，
///   失败也返回**结构化原因**而不是 `Err`。
///
/// ★ 为什么要读暂存（#111 复核 §2.2/§2.3 的断点）：本函数拿不到 `AppHandle`，物理上
/// 读不到登录窗里的 Cookie。原来它只读缓存文件，而缓存文件**只在 probe 里写** ——
/// 于是「用户登录 → 关窗 → 刷新」永远读到 `no_session`（闭环依赖）。现在关窗前收割与
/// 前端轮询都会把 Cookie 暂存进内存，本函数就能用上刚登录的那份；拿到 uid 校验通过后
/// 顺便落盘，下一次连暂存都不需要。
///
/// ★ 为什么失败走 `Ok({available:false, reason})` 而不是 `Err`：前端要按原因给**不同引导**
/// （没登录 → 「登录网页端」；归属不符 → 「换个账号」；上游抖动 → 「稍后重试」），
/// 用 `Err` 一律变成「出错了」，用户不知道该做什么。
pub async fn credit_packs(scope: String, uid: String) -> Result<Value, String> {
    let scope = resolve_scope_param(Some(&scope))?;
    let region = require_qoder_region(scope)?;
    require_uid(&uid)?;
    let now = web_usage_now_ms();
    // ★ 只读该账号自己的槽位（#155）：别的账号的会话既不能用，也不该被这次读取牵连。
    let stored = web_session::load(region, &uid);
    let had_stored = stored.is_some();

    // ① 已落盘的会话（保鲜）；② 否则关窗前 / 轮询时暂存的会话。
    let candidate: Option<(String, Vec<String>, i64, bool)> =
        match stored.filter(|s| s.is_fresh(now)) {
            Some(session) => Some((
                session.cookie,
                session.cookie_names,
                session.saved_at_ms,
                true,
            )),
            None => staged_cookies(region, &uid)
                .map(|(header, names, staged_at)| (header, names, staged_at, false)),
        };
    let Some((header, names, saved_at_ms, from_store)) = candidate else {
        // 有缓存但过期（且没有更新的暂存）⇒ 引导重新登录，而不是当成「没登录过」。
        return Ok(unavailable(if had_stored {
            "session_expired"
        } else {
            "no_session"
        }));
    };

    let raw = match web_usage::fetch_credit_packs_raw(region, &header).await {
        Ok(raw) => raw,
        Err(error) => {
            if error.unauthorized {
                let _ = web_session::clear(region, &uid);
                clear_staged(region, &uid);
                return Ok(unavailable("session_expired"));
            }
            // 上游抖动 / 网络问题**不清缓存**（否则一次抖动就掉登录态）。
            let mut payload = unavailable("upstream");
            payload["message"] = json!(error.message);
            return Ok(payload);
        }
    };
    if !web_usage::owner_matches(&raw, &uid) {
        // ★ 归属不符 ⇒ 这份 Cookie 属于别的账号：只清该账号的槽位与暂存（fail-closed）。
        let _ = web_session::clear(region, &uid);
        clear_staged(region, &uid);
        return Ok(unavailable("session_mismatch"));
    }
    // 会话被上游接受 ⇒ 落盘（暂存那份顺便认领，之后不必再依赖内存）。
    let _ = web_session::save(
        region,
        &uid,
        &web_session::WebSession {
            uid: uid.trim().to_string(),
            cookie: header,
            cookie_names: names,
            saved_at_ms: if from_store { saved_at_ms } else { now },
            verified_at_ms: now,
        },
    );
    if !from_store {
        clear_staged(region, &uid);
    }
    let packs = web_usage::parse_credit_packs(&raw);
    Ok(json!({
        "available": true,
        // 汇总（「N 个包共剩 X」这类行用，省得前端再求和）。
        "summary": web_usage::summarize(&packs),
        "packs": packs,
        "verifiedAtMs": now,
    }))
}

/// 「拿不到逐包」的统一形状。`reason` 是前端选文案的唯一依据。
fn unavailable(reason: &str) -> Value {
    json!({ "available": false, "reason": reason, "packs": Vec::<Value>::new() })
}

/// 该账号有没有可用的网页会话（前端据此决定显示「登录」按钮还是「已登录」）。
///
/// 返回 `hasSession`（新鲜）、`expired`（有缓存但过期）、`pending`（关窗前收割到、
/// 还没被 uid 认领的会话）。三者**不是**一件事：过期要引导重新登录，pending 只要「读一次」。
///
/// ★ 按 uid 判（#155）：同区域两个账号各有各的槽位，互不影响。
pub async fn session_status(scope: String, uid: String) -> Result<Value, String> {
    let scope = resolve_scope_param(Some(&scope))?;
    let region = require_qoder_region(scope)?;
    require_uid(&uid)?;
    let now = web_usage_now_ms();
    let stored = web_session::load(region, &uid);
    let has_session = stored
        .as_ref()
        .map(|session| session.is_fresh(now))
        .unwrap_or(false);
    let staged = staged_cookies(region, &uid).is_some();
    let payload = serde_json::json!({
        "hasSession": has_session,
        "expired": !has_session && stored.is_some(),
        // ★ 「有会话但还没认领」（刚在登录窗里登录、尚未探过上游）：前端据此决定
        //   要不要给「我已登录，读取逐包」这个按钮 —— 不给的话用户只能干等轮询。
        "pending": !has_session && staged,
        "source": if has_session {
            "stored"
        } else if staged {
            "staged"
        } else {
            "none"
        },
        "cookieNames": stored.as_ref().map(|s| s.cookie_names.clone()).unwrap_or_default(),
        "savedAtMs": stored.as_ref().map(|s| s.saved_at_ms).unwrap_or(0),
        "verifiedAtMs": stored.as_ref().map(|s| s.verified_at_ms).unwrap_or(0),
    });
    Ok(payload)
}

/// 清除**该账号**的本地会话缓存（「退出网页登录」；幂等）。
pub async fn session_logout(scope: String, uid: String) -> Result<Value, String> {
    let scope = resolve_scope_param(Some(&scope))?;
    let region = require_qoder_region(scope)?;
    require_uid(&uid)?;
    web_session::clear(region, &uid)?;
    // 内存里那份也要清：否则「退出登录」之后逐包还能从暂存读到（用户会认为没退掉）。
    clear_staged(region, &uid);
    Ok(json!({ "cleared": true }))
}

/// 读出站代理配置（两个区域各一个 URL；空 = 直连）。
///
/// ★ 这是**纯配置**，不是「仅桌面端可用」的东西：headless 服务端同样要取逐包
/// （有会话文件就能取），代理通道对它是同等需要的能力。
pub fn web_proxy_config() -> Result<Value, String> {
    let config = web_proxy::WebProxyConfig::load();
    serde_json::to_value(config).map_err(|error| format!("序列化代理配置失败: {error}"))
}

/// 保存出站代理配置（两个字段都先过形状校验，见 [`web_proxy::validate_url`]）。
pub fn web_proxy_config_save(config: web_proxy::WebProxyConfig) -> Result<Value, String> {
    config.save()?;
    Ok(json!({
        "ok": true,
        "config": { "cn": config.cn, "global": config.global },
    }))
}

/// 当前毫秒。包一层是为了测试里能替换（当前没有单元测试直接依赖它，
/// 但把时间源收在一处，别让 `Instant::now()` 散落）。
fn web_usage_now_ms() -> i64 {
    buddy_switch_core::modules::config::now_ms()
}

// ---------------------------------------------------------------------------
// 测试
// ---------------------------------------------------------------------------
//
// 本模块的绝大多数逻辑依赖 `Webview`（真机才能跑），能单测的只有这两条**纯函数级**
// 约束。它们各自钉住一个曾经会静默失效的点：
// ① 指纹只用于「变没变」，**绝不能**把 Cookie 值带给前端；
// ② 关窗前暂存必须按「区域 + 账号」隔离、且空值不得覆盖已有会话（否则一次匿名收割就把刚登录的抹掉）。

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fingerprint_detects_change_and_never_leaks_values() {
        let a = cookie_fingerprint("qoder_session=secret-1; visitor=abc");
        let b = cookie_fingerprint("qoder_session=secret-1; visitor=abc");
        let c = cookie_fingerprint("qoder_session=secret-2; visitor=abc");

        assert_eq!(a, b, "同一集合必须同指纹（否则轮询会误判为「变了」）");
        assert_ne!(a, c, "会话变了指纹必须变（否则永远发现不了登录完成）");
        assert_eq!(a.len(), 16, "指纹固定 16 位十六进制");
        assert!(a.chars().all(|ch| ch.is_ascii_hexdigit()));
        assert!(
            !a.contains("secret"),
            "指纹不得包含 cookie 值（它会被送到前端）: {a}"
        );
    }

    #[test]
    fn staged_cookies_are_account_scoped_and_ignore_empty() {
        clear_staged(QoderRegion::Cn, "u1");
        clear_staged(QoderRegion::Cn, "u2");
        assert!(staged_cookies(QoderRegion::Cn, "u1").is_none());

        stage_cookies(
            QoderRegion::Cn,
            "u1",
            "a=1; b=2",
            &["a".to_string(), "b".to_string()],
        );
        let (header, names, staged_at) =
            staged_cookies(QoderRegion::Cn, "u1").expect("同账号应能读到");
        assert_eq!(header, "a=1; b=2");
        assert_eq!(names, vec!["a".to_string(), "b".to_string()]);
        assert!(staged_at > 0);
        assert!(
            staged_cookies(QoderRegion::Global, "u1").is_none(),
            "别的区域不得读到本区域的会话"
        );
        assert!(
            staged_cookies(QoderRegion::Cn, "u2").is_none(),
            "别的账号不得读到本账号的会话"
        );

        // 空 Cookie（窗口刚开、还没登录）不得覆盖已有暂存。
        stage_cookies(QoderRegion::Cn, "u1", "   ", &[]);
        assert!(
            staged_cookies(QoderRegion::Cn, "u1").is_some(),
            "空 cookie 不得覆盖已有会话"
        );

        // ★ 多账号并行：清 u1 不得牵连 u2（否则先认领完 A，B 的会话就丢了）。
        stage_cookies(QoderRegion::Cn, "u2", "c=3", &["c".to_string()]);
        clear_staged(QoderRegion::Cn, "u1");
        assert!(
            staged_cookies(QoderRegion::Cn, "u1").is_none(),
            "清除后必须读不到"
        );
        assert!(
            staged_cookies(QoderRegion::Cn, "u2").is_some(),
            "清 A 的暂存不得连带清掉 B"
        );
        clear_staged(QoderRegion::Cn, "u2");
    }

    #[test]
    fn login_label_is_per_account_and_per_region() {
        let a = login_label(QoderRegion::Cn, "uid-a");
        assert_eq!(a, login_label(QoderRegion::Cn, "UID-A "), "大小写/空白归一");
        assert_ne!(
            a,
            login_label(QoderRegion::Cn, "uid-b"),
            "不同账号必须不同窗口"
        );
        assert_ne!(
            a,
            login_label(QoderRegion::Global, "uid-a"),
            "不同区域必须不同窗口"
        );
        assert!(a.starts_with(LOGIN_LABEL_PREFIX));
        // 同一账号的窗口 label 与会话文件名用同一套归一 ⇒ 永远对得上
        assert!(a.ends_with(&web_session::uid_file_token("uid-a")));
    }
}
