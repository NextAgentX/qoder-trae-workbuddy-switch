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
//! ## 三条硬边界
//!
//! 1. ★★ **这只是默认路径，不是硬约束**。窗口建不起来 / 页面在内嵌环境里打不开，
//!    调用方必须能退回系统浏览器 —— 否则一次建窗失败就等于「登不了账号」。
//! 2. ★ **URL 必须过形状校验**：只允许 `https`（与 `http` 回环，给本地联调）。
//!    `file:` / `data:` / `javascript:` 一律拒绝 —— 这个命令的参数来自渲染层，
//!    不能让它把任意本地文件或脚本塞进一个带 Tauri IPC 能力的窗口。
//! 3. ★ **窗口按 `key` 分家**：`key` 由调用方给（`wb-cn` / `trae-…` / `qoder-…`），
//!    多平台并行登录时互不导航覆盖。同一 `key` 再次发起 ⇒ 导航到新地址而不是堆新窗口。
//!
//! ## 与 `qoder_web` 登录窗的分工（别合并）
//!
//! | 窗口 | 用途 | 关窗时 |
//! |:---|:---|:---|
//! | 本模块 | 完成 **OAuth 授权**（结果走后端轮询） | 直接销毁，无副作用 |
//! | `qoder_web` | 采集**网页会话 Cookie**（逐包明细） | 必须先收割再销毁 |
//!
//! 合并会让「关窗要不要等收割」这个条件渗进授权路径，把简单的事弄成时序问题。

use serde_json::{json, Value};
use tauri::{AppHandle, Manager, Runtime, Url, WebviewUrl, WebviewWindowBuilder};

/// 窗口 label 前缀；实际 label = `auth-web-{key}`。
const LABEL_PREFIX: &str = "auth-web";

/// 授权窗尺寸：授权页多为「手机版」纵向布局（含扫码 / 验证码输入），窄而高最合适。
const WINDOW_WIDTH: f64 = 520.0;
const WINDOW_HEIGHT: f64 = 760.0;

/// 把调用方给的 `key` 压成可进 label 的片段（label 是窗口的唯一身份，不能带空格）。
fn label_for(key: &str) -> String {
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
        .take(64)
        .collect();
    if slug.is_empty() {
        slug.push_str("default");
    }
    format!("{LABEL_PREFIX}-{slug}")
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

/// 打开（或复用）`key` 对应的授权窗并载入授权页。
///
/// 返回 `{ opened, reused, url, label }`；`reused = true` 表示窗口已存在、这次只是
/// 导航过去（同一账号点了两次「登录」时不该堆出两个窗）。
pub fn open<R: Runtime>(app: &AppHandle<R>, key: String, url: String) -> Result<Value, String> {
    let parsed = validated_url(&url)?;
    let label = label_for(&key);

    if let Some(window) = app.get_webview_window(&label) {
        // ★ 必须导航：同一账号重新发起登录时，授权地址里的 state / PKCE challenge 都变了，
        //   沿用旧页面会让后端轮询与页面授权对不上（症状是「授权完了却一直转圈」）。
        let _ = window.navigate(parsed.clone());
        let _ = window.show();
        let _ = window.set_focus();
        return Ok(json!({
            "opened": true,
            "reused": true,
            "url": parsed.to_string(),
            "label": label,
        }));
    }

    let window = WebviewWindowBuilder::new(app, &label, WebviewUrl::External(parsed.clone()))
        .title("账号授权登录")
        .inner_size(WINDOW_WIDTH, WINDOW_HEIGHT)
        .min_inner_size(420.0, 560.0)
        .center()
        .build()
        .map_err(|error| format!("打开授权窗失败: {error}"))?;
    let _ = window.show();
    let _ = window.set_focus();
    Ok(json!({
        "opened": true,
        "reused": false,
        "url": parsed.to_string(),
        "label": label,
    }))
}

/// 关闭 `key` 对应的授权窗（没开 ⇒ 幂等成功）。
///
/// 前端在轮询拿到终态后调用：令牌已入库，窗口留着只会让用户以为「还在等」。
pub fn close<R: Runtime>(app: &AppHandle<R>, key: String) -> Result<Value, String> {
    let label = label_for(&key);
    let closed = match app.get_webview_window(&label) {
        Some(window) => {
            // `close()` 触发 `CloseRequested`；本窗口没装拦截钩子，直接销毁。
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
    fn label_is_stable_and_keyed() {
        assert_eq!(label_for("WB-CN"), label_for("wb-cn"));
        assert_ne!(label_for("wb-cn"), label_for("wb-global"));
        assert!(label_for("qoder-cn").starts_with(LABEL_PREFIX));
        // 非法字符不得进 label（空格 / 斜杠会把窗口身份弄乱）
        let label = label_for("a b/c:d");
        assert!(label
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_'));
        // 空 key 有兜底（否则会建出一个名为 `auth-web-` 的窗口）
        assert_eq!(label_for("   "), format!("{LABEL_PREFIX}-default"));
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
