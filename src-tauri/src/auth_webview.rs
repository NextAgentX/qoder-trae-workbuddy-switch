//! **内嵌授权窗**（公共设置 `auth_in_app`）—— 账号授权不跳出系统浏览器。
//!
//! ## 为什么可以内嵌
//!
//! 三个平台（WorkBuddy / Trae / Qoder）的登录都是**同一个形状**：
//! 后端先算出一个授权页 URL（`verificationUri`），用户在页面里登录并同意，
//! 令牌再由**后端轮询**取回并入库（`oauth::login_poll_for` 等）。也就是说：
//!
//! - 授权页只是一个普通 HTTPS 页面，不需要浏览器扩展、不需要自定义 scheme 回调；
//! - 「授权完成」的判据在**后端轮询**里，不在浏览器回调里。
//!
//! ⇒ 把授权页从系统浏览器搬到一个 webview 窗口，链路其余部分**一个字都不用改**。
//! Trae 的 `127.0.0.1:<port>` 回调同理：webview 里跳到回环地址，本地监听照样收到。
//!
//! ## ★★ 一次登录 = 一个窗口 = 一份独立的 webview 数据目录
//!
//! 这是本模块**最容易做错**的一处，也是 v2026.11.91845 的真实缺陷：最初按
//! 「平台 + 区域」复用一个窗口（`auth-web-qoder-cn`），结果是 ——
//!
//! 1. 同一平台登第二个账号时，第二次 `open` 直接 `navigate` **顶掉**第一个窗口里的
//!    授权页（用户正在操作的页面被换掉）；
//! 2. 更要命的是 cookie store 是**进程级共享**的：第一个账号登录后，第二次打开的
//!    授权页直接是「已登录」状态，用户点授权拿到的还是**上一个账号**。
//!
//! 授权窗承载的语义正是「**要登哪个账号还没定**」，所以它不能按账号分家，只能
//! **按「这一次登录」分家**：每次 `open` 都新建窗口 + 新建数据目录（Windows
//! `data_directory`；其它平台 `incognito`），close 时销毁，目录由
//! [`prune_profiles`] 定期清理。
//!
//! 代价（有意）：同一个窗口不再复用登录态，每次登录都要重新输一次账号密码。
//! 这换来的是「登 A 就是 A、登 B 就是 B」，以及多账号可以**并行**登录。
//!
//! ## 三条硬边界
//!
//! 1. ★★ **这只是默认路径，不是硬约束**。窗口建不起来 / 页面在内嵌环境里打不开，
//!    调用方必须能退回系统浏览器 —— 否则一次建窗失败就等于「登不了账号」。
//! 2. ★ **URL 必须过形状校验**：只允许 `https`（与 `http` 回环，给本地联调）。
//!    `file:` / `data:` / `javascript:` 一律拒绝 —— 这个命令的参数来自渲染层，
//!    不能让它把任意本地文件或脚本塞进一个带 Tauri IPC 能力的窗口。
//! 3. ★ **`close` 只认本模块建的 label**（`auth-web-*` + 安全字符集）：
//!    不能让渲染层传一个任意 label 去销毁主窗口。
//!
//! ## 与 `qoder_web` 登录窗的分工（别合并）
//!
//! | 窗口 | 用途 | 关窗时 |
//! |:---|:---|:---|
//! | 本模块 | 完成 **OAuth 授权**（结果走后端轮询） | 直接销毁，无副作用 |
//! | `qoder_web` | 采集**网页会话 Cookie**（逐包明细） | 必须先收割再销毁 |
//!
//! 合并会让「关窗要不要等收割」这个条件渗进授权路径，把简单的事弄成时序问题。

// ★ 只被 Windows 的 `profiles_root` / `prune_profiles` 使用 ⇒ 必须同 cfg 限定，
//   否则 mac / linux 构建报 `unused import`，而 CI 的 rust-cache 默认带 `-D warnings`
//   （v2026.11.91845 已经这样挂过一次，别再犯）。
#[cfg(target_os = "windows")]
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::{json, Value};
use tauri::{AppHandle, Manager, Runtime, Url, WebviewUrl, WebviewWindowBuilder};

#[cfg(target_os = "windows")]
use buddy_switch_core::modules::runtime_log;

/// 窗口 label 前缀；完整形态 `auth-web-{key slug}-{序号}`。
const LABEL_PREFIX: &str = "auth-web";

/// 授权窗尺寸：授权页多为「手机版」纵向布局（含扫码 / 验证码输入），窄而高最合适。
const WINDOW_WIDTH: f64 = 520.0;
const WINDOW_HEIGHT: f64 = 760.0;

/// 数据目录的保鲜期：超过它的自动清理（Windows 每次登录一份，不清理会单调增长）。
///
/// ★ 同 `PathBuf`：只有 Windows 分支建目录，常量也必须跟着 cfg，否则 mac / linux
///   报 `dead_code`（同样被 `-D warnings` 打成错误）。
#[cfg(target_os = "windows")]
const PROFILE_TTL: std::time::Duration = std::time::Duration::from_secs(7 * 24 * 60 * 60);

/// 窗口序号：保证同一平台连续登录两次拿到两个**不同**的 label（也就有两份数据目录）。
static SEQ: AtomicU64 = AtomicU64::new(1);

/// 把调用方给的 `key` 压成可进 label 的片段（label 是窗口的唯一身份，不能带空格）。
fn slug_for(key: &str) -> String {
    let mut slug: String = key
        .trim()
        .to_ascii_lowercase()
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '-'
            }
        })
        .take(48)
        .collect();
    if slug.is_empty() {
        slug.push_str("default");
    }
    slug
}

/// 本次登录要用的新 label —— **永不复用**（见文件头「一次登录 = 一个窗口」）。
fn next_label(key: &str) -> String {
    format!(
        "{LABEL_PREFIX}-{}-{}",
        slug_for(key),
        SEQ.fetch_add(1, Ordering::SeqCst)
    )
}

/// `close` 的安全边界：只接受本模块生成的 label（`auth-web-{slug}-{序号}`）。
///
/// 守的是「渲染层拿一个任意 label 把主窗口销毁」——所以必须同时校验**前缀**、
/// **字符集**（`..` / `/` / `\` 之类会把 label 变成路径或别的窗口）与**形状**
/// （末尾必须是自增序号，光有前缀不够 —— `auth-web-` 这种半截值也要拒）。
fn is_own_label(label: &str) -> bool {
    let Some(rest) = label.strip_prefix(&format!("{LABEL_PREFIX}-")) else {
        return false;
    };
    if rest.is_empty() || label.len() > 96 {
        return false;
    }
    if !label
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
    {
        return false;
    }
    match rest.rsplit_once('-') {
        Some((slug, seq)) => {
            !slug.is_empty() && !seq.is_empty() && seq.chars().all(|ch| ch.is_ascii_digit())
        }
        None => false,
    }
}

/// 授权页 URL 的形状校验（见文件头第 2 条）。
fn validated_url(raw: &str) -> Result<Url, String> {
    let url: Url = raw
        .trim()
        .parse()
        .map_err(|error| format!("授权地址非法: {error}"))?;
    let host = url.host_str().unwrap_or_default();
    let allowed = match url.scheme() {
        "https" => !host.is_empty(),
        // 回环留给本地联调 / 自建反代（Trae 的回调也跑在 127.0.0.1）。
        "http" => matches!(host, "localhost" | "127.0.0.1" | "[::1]" | "::1"),
        _ => false,
    };
    if !allowed {
        return Err(format!(
            "拒绝在应用内打开该地址（只允许 https 或本机回环 http）: {}://{host}",
            url.scheme()
        ));
    }
    Ok(url)
}

/// 本模块的数据目录根（Windows 每个窗口在其下一份）。
#[cfg(target_os = "windows")]
fn profiles_root<R: Runtime>(app: &AppHandle<R>) -> Option<PathBuf> {
    app.path()
        .app_data_dir()
        .ok()
        .map(|dir| dir.join("auth-web-profiles"))
}

/// 清理过期的授权窗数据目录（每次开窗顺手跑一次，IO 很轻）。
///
/// ★ 为什么不做「关窗即删」：Windows 上 WebView2 的用户数据目录在窗口销毁后仍会被
/// 其子进程短暂持有，`remove_dir_all` 大概率失败 —— 失败要么吞掉（等于没删），要么
/// 报错（用户看到莫名其妙的错误）。按时间清理更可靠。
#[cfg(target_os = "windows")]
fn prune_profiles(root: &std::path::Path) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let stale = entry
            .metadata()
            .and_then(|meta| meta.modified())
            .map(|modified| {
                modified
                    .elapsed()
                    .map(|age| age > PROFILE_TTL)
                    .unwrap_or(false)
            })
            .unwrap_or(false);
        if stale {
            // 失败无所谓：下一次开窗还会来清。
            let _ = std::fs::remove_dir_all(&path);
        }
    }
}

/// 打开**一个新的**授权窗并载入授权页。
///
/// 返回 `{ opened, url, label }` —— 调用方必须保存 `label`，并在登录出结果（或用户取消）
/// 时用它调 [`close`]；否则窗口会一直留在桌面上。
pub fn open<R: Runtime>(app: &AppHandle<R>, key: String, url: String) -> Result<Value, String> {
    let parsed = validated_url(&url)?;
    let label = next_label(&key);

    let mut builder = WebviewWindowBuilder::new(app, &label, WebviewUrl::External(parsed.clone()))
        .title("账号授权登录")
        .inner_size(WINDOW_WIDTH, WINDOW_HEIGHT)
        .min_inner_size(420.0, 560.0)
        .center();

    #[cfg(target_os = "windows")]
    {
        // 每个窗口一份 WebView2 user data folder（wry 以它为 `WebContext` 的 key，
        // cookie / 缓存完全分家）。拿不到目录就只能退回共享 profile —— 那正是
        // 「多账号互相覆盖」的旧症状，所以失败要留痕。
        match profiles_root(app) {
            Some(root) => {
                let profile = root.join(&label);
                if let Err(error) = std::fs::create_dir_all(&profile) {
                    runtime_log::warn(
                        "auth-web",
                        format!("建 profile 目录失败，多账号隔离将失效: {error}"),
                    );
                } else {
                    builder = builder.data_directory(profile);
                }
                prune_profiles(&root);
            }
            None => runtime_log::warn(
                "auth-web",
                "取 appDataDir 失败，多账号隔离将失效（授权窗共享 profile）",
            ),
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        // `data_directory` 只在 Windows 生效；其它平台用 incognito 拿每窗口一份的
        // nonPersistent / ephemeral 数据存储（wry 语义）。窗口关掉即丢 Cookie ——
        // 本链路结果走后端轮询，丢 Cookie 无影响。
        builder = builder.incognito(true);
    }

    let window = builder
        .build()
        .map_err(|error| format!("打开授权窗失败: {error}"))?;
    let _ = window.show();
    let _ = window.set_focus();
    Ok(json!({
        "opened": true,
        "url": parsed.to_string(),
        "label": label,
    }))
}

/// 关闭某个授权窗（按 [`open`] 返回的 `label`；幂等）。
///
/// 只有 `auth-web-*` 的 label 会被受理（见文件头第 3 条）。
pub fn close<R: Runtime>(app: &AppHandle<R>, label: String) -> Result<Value, String> {
    if !is_own_label(&label) {
        return Err("拒绝关闭非授权窗".to_string());
    }
    let closed = match app.get_webview_window(&label) {
        Some(window) => {
            // `destroy()` 不触发 `CloseRequested`，直接销毁（本窗没有拦截钩子）。
            let _ = window.destroy();
            true
        }
        None => false,
    };
    Ok(json!({ "closed": closed, "label": label }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_are_unique_per_open_and_keyed_to_the_platform() {
        let a = next_label("qoder-cn");
        let b = next_label("qoder-cn");
        assert_ne!(a, b, "同一平台连续登录两次必须拿到两个窗口");
        assert!(a.starts_with("auth-web-qoder-cn-"), "label 形状: {a}");
        assert_eq!(slug_for("WB-CN"), slug_for("wb-cn"), "大小写归一");
        assert_ne!(slug_for("wb-cn"), slug_for("wb-global"));
        // 非法字符不得进 label（空格 / 斜杠会把窗口身份弄乱）
        let label = next_label("a b/c:d");
        assert!(
            is_own_label(&label),
            "生成的 label 必须能通过自身校验: {label}"
        );
        // 空 key 有兜底（否则会建出一个名为 `auth-web--1` 的窗口）
        assert!(next_label("   ").starts_with("auth-web-default-"));
    }

    /// `close` 的安全边界：不能让渲染层用一个任意 label 销毁主窗口。
    #[test]
    fn close_guard_rejects_foreign_labels() {
        assert!(is_own_label("auth-web-qoder-cn-3"));
        assert!(!is_own_label("main"));
        assert!(!is_own_label("auth-web-../evil"));
        assert!(!is_own_label("auth-web-a/b"));
        assert!(!is_own_label("auth-web-"));
        assert!(!is_own_label(&format!("auth-web-{}", "x".repeat(200))));
    }

    #[test]
    fn url_guard_allows_https_and_loopback_only() {
        assert!(validated_url("https://qoder.com.cn/oauth/authorize?x=1").is_ok());
        assert!(validated_url("http://127.0.0.1:5173/callback").is_ok());
        assert!(validated_url("http://localhost:8080/x").is_ok());

        // ★ 可证伪点：把 scheme 白名单放宽 ⇒ 后四条变红。
        assert!(validated_url("file:///C:/Windows/win.ini").is_err());
        assert!(validated_url("javascript:alert(1)").is_err());
        assert!(validated_url("data:text/html,<script>x</script>").is_err());
        assert!(validated_url("http://evil.example.com/auth").is_err());
        assert!(validated_url("not a url").is_err());
        assert!(validated_url("").is_err());
    }
}
