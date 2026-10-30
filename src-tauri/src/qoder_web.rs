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

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use serde_json::{json, Value};
use tauri::{
    AppHandle, Manager, Runtime, Url, WebviewUrl, WebviewWindow, WebviewWindowBuilder, WindowEvent,
};

use buddy_switch_core::modules::product::{require_qoder_region, resolve_scope_param};
use buddy_switch_core::modules::qoder::region::{self, QoderRegion};
use buddy_switch_core::modules::qoder::web_session;
use buddy_switch_core::modules::qoder::web_usage;

/// 登录窗的 label。
///
/// 一个窗口足够：两区域名不同，cookie store 按域隔离，不会互相污染。
/// 换区域时**导航**而不是新开窗口 —— 用户不该看到两个 Qoder 登录窗。
pub const LOGIN_LABEL: &str = "qoder-web-login";

/// 登录窗尺寸（网页端用量页是桌面版布局，窄了会挤成两行）。
const WINDOW_WIDTH: f64 = 1024.0;
const WINDOW_HEIGHT: f64 = 780.0;

/// 确保登录窗存在并导航到该区域的用量页。
fn ensure_login_window<R: Runtime>(
    app: &AppHandle<R>,
    region: QoderRegion,
) -> Result<WebviewWindow<R>, String> {
    let url = web_usage::usage_page_url(region);
    let parsed: Url = url
        .parse()
        .map_err(|error| format!("登录页地址非法: {error}"))?;

    if let Some(window) = app.get_webview_window(LOGIN_LABEL) {
        // 已存在：**只在区域变了时才导航**。
        //
        // ⚠️ 无条件 `navigate` 会把用户正在填的登录表单刷掉 —— 前端每次点击都会走这里，
        //    「点两下按钮就把输了一半的账号密码清空」是必然被报的 bug。用 host 判定区域
        //    是否真的换了（两区域的 website 域名不同：qoder.com.cn / qoder.com）。
        let same_region = window
            .url()
            .map(|current| current.host_str() == parsed.host_str())
            .unwrap_or(false);
        if !same_region {
            let _ = window.navigate(parsed);
        }
        let _ = window.show();
        let _ = window.set_focus();
        return Ok(window);
    }

    let window = WebviewWindowBuilder::new(app, LOGIN_LABEL, WebviewUrl::External(parsed))
        .title("登录 Qoder 网页端（用于读取逐包明细）")
        .inner_size(WINDOW_WIDTH, WINDOW_HEIGHT)
        .min_inner_size(720.0, 560.0)
        .center()
        .build()
        .map_err(|error| format!("打开 Qoder 登录窗失败: {error}"))?;
    // ★ 关窗前兜底收割（#111 复核 P1）：没有它，「登录完直接关窗」= 白登录。
    CLOSE_HARVESTING.store(false, Ordering::SeqCst);
    install_close_harvest(&window, region);
    Ok(window)
}

/// 读登录窗里现有的 Cookie **名称与条数**（本地命令，**不打上游**）。
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
) -> Result<Value, String> {
    let scope = resolve_scope_param(Some(&scope))?;
    let region = require_qoder_region(scope)?;
    let Some(window) = app.get_webview_window(LOGIN_LABEL) else {
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
    stage_cookies(region, &header, &names);
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

fn staged_slot() -> &'static Mutex<Option<StagedCookies>> {
    static SLOT: std::sync::OnceLock<Mutex<Option<StagedCookies>>> = std::sync::OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(None))
}

/// 暂存一份刚收割到的 Cookie（空串直接忽略；同一区域后写覆盖先写）。
fn stage_cookies(region: QoderRegion, header: &str, names: &[String]) {
    if header.trim().is_empty() {
        return;
    }
    let mut guard = match staged_slot().lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    *guard = Some(StagedCookies {
        region,
        header: header.to_string(),
        names: names.to_vec(),
        staged_at_ms: web_usage_now_ms(),
    });
}

/// 取暂存（按区域、判保鲜期）。**不清除** —— 认领方拿到 uid 校验通过后才落盘。
fn staged_cookies(region: QoderRegion) -> Option<(String, Vec<String>, i64)> {
    let guard = match staged_slot().lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    let staged = guard.as_ref()?;
    if staged.region != region {
        return None;
    }
    if web_usage_now_ms().saturating_sub(staged.staged_at_ms) >= STAGED_TTL_MS {
        return None;
    }
    Some((
        staged.header.clone(),
        staged.names.clone(),
        staged.staged_at_ms,
    ))
}

/// 丢弃暂存（认领完成 / 判定为别人的会话之后）。
fn clear_staged() {
    let mut guard = match staged_slot().lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    *guard = None;
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

/// 「正在关窗前收割」的互斥位：连点两次 × 时不重复起两个收割任务。
static CLOSE_HARVESTING: AtomicBool = AtomicBool::new(false);

/// 给登录窗装「关窗前先收割」的钩子。
///
/// ★ 为什么必须拦：用户登录完**直接关窗**是最自然的操作。窗口一销毁，本进程就再没有
/// 任何能调用 `cookies_for_url` 的句柄，那份会话就白登录了（#111 复核 §2.5）。
///
/// ★ 为什么是 `prevent_close` + 异步再关：`cookies()` 在**事件处理器里同步调用会在
/// Windows 上死锁**（wry#583）。所以先拦住，起任务收割，再用 `destroy()` 关 ——
/// `destroy()` 不触发 `CloseRequested`，不会自己把自己拦住。
fn install_close_harvest<R: Runtime>(window: &WebviewWindow<R>, region: QoderRegion) {
    let handle = window.clone();
    window.on_window_event(move |event| {
        let WindowEvent::CloseRequested { api, .. } = event else {
            return;
        };
        if CLOSE_HARVESTING.swap(true, Ordering::SeqCst) {
            // 已在收割：继续拦（否则这次会抢在收割完成前把窗口销毁）。
            api.prevent_close();
            return;
        }
        api.prevent_close();
        let handle = handle.clone();
        tauri::async_runtime::spawn(async move {
            if let Some((header, names)) = harvest(handle.clone(), region).await {
                stage_cookies(region, &header, &names);
            }
            CLOSE_HARVESTING.store(false, Ordering::SeqCst);
            let _ = handle.destroy();
        });
    });
}

/// 打开（或前置）Qoder 网页端登录窗。
///
/// 前端入口：账号卡 / 抽屉额度区的「登录 Qoder 网页端以获取逐包明细」按钮。
///
/// ⚠️ 本模块只提供实现；`#[tauri::command]` 的薄包装在 `commands.rs`
/// （`scripts/check-api-contract.cjs` 只认 `commands::<name>` 形态的登记，写在别处会被判「未登记」）。
pub async fn login_open<R: Runtime>(app: &AppHandle<R>, scope: String) -> Result<Value, String> {
    let scope = resolve_scope_param(Some(&scope))?;
    let region = require_qoder_region(scope)?;
    let window = ensure_login_window(app, region)?;
    let _ = window.show();
    let _ = window.set_focus();
    Ok(json!({
        "opened": true,
        "url": web_usage::usage_page_url(region),
    }))
}

/// 关闭登录窗（已关闭 ⇒ 幂等成功）。
pub async fn login_close<R: Runtime>(app: &AppHandle<R>) -> Result<Value, String> {
    let closed = match app.get_webview_window(LOGIN_LABEL) {
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
    if uid.trim().is_empty() {
        return Err("缺少 uid".to_string());
    }

    // ① 登录窗（可能没开 —— 没开不等于失败，下面还有暂存与本地缓存）。
    let mut names: Vec<String> = Vec::new();
    let mut header = String::new();
    let mut source = "none";
    if let Some(window) = app.get_webview_window(LOGIN_LABEL) {
        // ★★ 必须离开当前线程读 cookie：Windows 上 `cookies()` 在同步上下文会死锁
        //    （Tauri Known issues / wry#583）。`harvest` 内部即 `spawn_blocking`。
        if let Some((header_from_window, window_names)) = harvest(window, region).await {
            // ★ 顺手暂存：探针万一因网络抖动失败，这份会话也不会随窗口一起消失。
            stage_cookies(region, &header_from_window, &window_names);
            header = header_from_window;
            names = window_names;
            source = "window";
        }
    }

    // ② 关窗前暂存（用户刚登录完就把窗口关了 —— 最自然的操作序列）。
    if header.is_empty() {
        if let Some((staged_header, staged_names, _)) = staged_cookies(region) {
            header = staged_header;
            names = staged_names;
            source = "staged";
        }
    }

    // ③ 本地缓存（保鲜 + uid 归属双重校验）。
    if header.is_empty() {
        let now = web_usage_now_ms();
        if let Some(session) = web_session::load(region) {
            if session.is_fresh(now) && session.matches_uid(&uid) {
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
                let _ = web_session::clear(region);
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
            &web_session::WebSession {
                uid: uid.trim().to_string(),
                cookie: header,
                cookie_names: names.clone(),
                saved_at_ms: now,
                verified_at_ms: now,
            },
        );
        // 已落盘 ⇒ 内存里那份暂存没有用处了（留着只会让下次读到一份更旧的副本）。
        clear_staged();
    } else {
        // ★ 归属不符 ⇒ 这是**别人的**会话，绝不能留在本机、更不能显示给当前账号。
        let _ = web_session::clear(region);
        clear_staged();
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
    if uid.trim().is_empty() {
        return Err("缺少 uid".to_string());
    }
    let now = web_usage_now_ms();
    let stored = web_session::load(region);
    let had_stored = stored.is_some();
    if let Some(session) = stored.as_ref() {
        if !session.matches_uid(&uid) {
            // 缓存里是**别人的**会话 ⇒ 既不能用也不能留。
            let _ = web_session::clear(region);
            return Ok(unavailable("session_mismatch"));
        }
    }

    // ① 已落盘的会话（uid 匹配 + 保鲜）；② 否则关窗前 / 轮询时暂存的会话。
    let candidate: Option<(String, Vec<String>, i64, bool)> =
        match stored.filter(|s| s.is_fresh(now)) {
            Some(session) => Some((
                session.cookie,
                session.cookie_names,
                session.saved_at_ms,
                true,
            )),
            None => staged_cookies(region)
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
                let _ = web_session::clear(region);
                clear_staged();
                return Ok(unavailable("session_expired"));
            }
            // 上游抖动 / 网络问题**不清缓存**（否则一次抖动就掉登录态）。
            let mut payload = unavailable("upstream");
            payload["message"] = json!(error.message);
            return Ok(payload);
        }
    };
    if !web_usage::owner_matches(&raw, &uid) {
        // ★ 归属不符 ⇒ 这份 Cookie 属于别的账号：缓存与暂存都要清掉（fail-closed）。
        let _ = web_session::clear(region);
        clear_staged();
        return Ok(unavailable("session_mismatch"));
    }
    // 会话被上游接受 ⇒ 落盘（暂存那份顺便认领，之后不必再依赖内存）。
    let _ = web_session::save(
        region,
        &web_session::WebSession {
            uid: uid.trim().to_string(),
            cookie: header,
            cookie_names: names,
            saved_at_ms: if from_store { saved_at_ms } else { now },
            verified_at_ms: now,
        },
    );
    if !from_store {
        clear_staged();
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

/// 当前区域有没有可用的网页会话（前端据此决定显示「登录」按钮还是「已登录」）。
///
/// 返回 `hasSession`（新鲜且 uid 匹配）、`expired`（有缓存但过期/换过账号）、
/// `pending`（关窗前收割到、还没被 uid 认领的会话）。
/// 三者**不是**一件事：过期要引导重新登录，匹配要引导清除，pending 只要「读一次」。
pub async fn session_status(scope: String, uid: String) -> Result<Value, String> {
    let scope = resolve_scope_param(Some(&scope))?;
    let region = require_qoder_region(scope)?;
    let now = web_usage_now_ms();
    let stored = web_session::load(region);
    let has_session = stored
        .as_ref()
        .map(|session| session.is_fresh(now) && session.matches_uid(&uid))
        .unwrap_or(false);
    let staged = staged_cookies(region).is_some();
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

/// 清除本地会话缓存（「退出网页登录」；幂等）。
pub async fn session_logout(scope: String) -> Result<Value, String> {
    let scope = resolve_scope_param(Some(&scope))?;
    let region = require_qoder_region(scope)?;
    web_session::clear(region)?;
    // 内存里那份也要清：否则「退出登录」之后逐包还能从暂存读到（用户会认为没退掉）。
    clear_staged();
    Ok(json!({ "cleared": true }))
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
// ② 关窗前暂存必须按区域隔离、且空值不得覆盖已有会话（否则一次匿名收割就把刚登录的抹掉）。

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
    fn staged_cookies_are_region_scoped_and_ignore_empty() {
        clear_staged();
        assert!(staged_cookies(QoderRegion::Cn).is_none());

        stage_cookies(
            QoderRegion::Cn,
            "a=1; b=2",
            &["a".to_string(), "b".to_string()],
        );
        let (header, names, staged_at) = staged_cookies(QoderRegion::Cn).expect("同区域应能读到");
        assert_eq!(header, "a=1; b=2");
        assert_eq!(names, vec!["a".to_string(), "b".to_string()]);
        assert!(staged_at > 0);
        assert!(
            staged_cookies(QoderRegion::Global).is_none(),
            "别的区域不得读到本区域的会话"
        );

        // 空 Cookie（窗口刚开、还没登录）不得覆盖已有暂存。
        stage_cookies(QoderRegion::Cn, "   ", &[]);
        assert!(
            staged_cookies(QoderRegion::Cn).is_some(),
            "空 cookie 不得覆盖已有会话"
        );

        clear_staged();
        assert!(
            staged_cookies(QoderRegion::Cn).is_none(),
            "清除后必须读不到"
        );
    }
}
