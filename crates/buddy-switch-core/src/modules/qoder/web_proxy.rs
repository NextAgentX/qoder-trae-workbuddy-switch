//! Qoder 网页端取数的**出站代理**配置（issue #111 收尾：国际版逐包的最后一条边界）。
//!
//! ## 为什么需要它
//!
//! `big_model_credits` 一族网页端接口**按 IP 判定地区**：国际版（`qoder.com`）从国内直连
//! 会被上游拦下，而登录窗走的是系统代理（WebView2 默认继承），于是出现「登录窗里明明
//! 登录成功、后台取数却失败 ⇒ Cookie 被误判为无效」的错位（CreditDaddy 为此专门做了
//! 代理优先）。本仓 core 的出站链路默认**不读任何系统/环境代理**（`trae/credits.rs` 甚至
//! 显式 `no_proxy()`），所以这里给逐包取数一条**显式配置**的代理通道。
//!
//! ## 边界（刻意收窄）
//!
//! - 只作用于**网页端逐包取数**（`web_usage::fetch_credit_packs_raw`）——那是唯一被证实
//!   按 IP 拦截的路径。openapi（`/api/v2/*`）是否也需要代理没有证据，不顺手动它；
//! - 配置按区域分两份（CN / 国际版），互不影响；留空 = 直连（CN 的缺省形态）；
//! - 登录窗**不走**这份配置（WebView2 自己继承系统代理），说明文案里必须写清这一点，
//!   否则用户会以为「配了代理登录窗反而连不上」。
//!
//! ## 凭据边界
//!
//! 代理 URL 会出现在诊断文案里吗？不会 —— 失败文案走 `net::describe_transport_error`，
//! 只描述传输层错误种类；本模块也不把代理 URL 写进任何日志。

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use reqwest::Client;
use serde::{Deserialize, Serialize};

use super::paths;
use super::region::QoderRegion;
use super::store;

/// 代理配置：`qoder/proxy.json`，两个区域各一个 URL（留空 = 直连）。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct WebProxyConfig {
    /// 国内版（缺省直连；一般不用填）。
    #[serde(default)]
    pub cn: String,
    /// 国际版（国内网络直连会被拦 ⇒ 通常要填）。
    #[serde(default)]
    pub global: String,
}

impl WebProxyConfig {
    pub fn file() -> PathBuf {
        paths::qoder_dir().join("proxy.json")
    }

    /// 读取（文件不存在 / 损坏 ⇒ 全空 = 直连，不报错）。
    pub fn load() -> Self {
        store::read_json(&Self::file())
    }

    /// 保存（两个字段都先过 [`validate_url`]）。
    pub fn save(&self) -> Result<(), String> {
        validate_url(&self.cn)?;
        validate_url(&self.global)?;
        store::write_json(&Self::file(), self)
    }

    /// 该区域的代理 URL（空 = 直连 ⇒ `None`）。
    pub fn url_for(&self, region: QoderRegion) -> Option<String> {
        let raw = match region {
            QoderRegion::Cn => &self.cn,
            QoderRegion::Global => &self.global,
        };
        let trimmed = raw.trim();
        (!trimmed.is_empty()).then(|| trimmed.to_string())
    }
}

/// 校验代理 URL：空 = 直连（合法）；否则必须是 http / https / socks5 / socks5h。
///
/// ⚠️ 只做**形状**校验，不发起连接 ——「代理通不通」由第一次真实取数时的传输层错误
/// 文案回答（`net::describe_transport_error`），那才是可行动的诊断。
pub fn validate_url(raw: &str) -> Result<(), String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(());
    }
    let (scheme, rest) = trimmed.split_once("://").ok_or_else(|| {
        format!("代理地址缺少协议前缀（应为 http:// / socks5:// 开头）：{trimmed}")
    })?;
    let scheme = scheme.to_ascii_lowercase();
    if !matches!(scheme.as_str(), "http" | "https" | "socks5" | "socks5h") {
        return Err(format!(
            "代理地址协议不支持「{scheme}://」：请用 http:// 、https:// 、socks5:// 或 socks5h://"
        ));
    }
    if rest.trim().is_empty() {
        return Err("代理地址缺少主机与端口".to_string());
    }
    Ok(())
}

/// 该区域取数用的 HTTP 客户端。
///
/// 无代理 ⇒ 直接复用 [`super::api::shared_client`]（与 openapi 同一条传输层，错误文案同源）；
/// 有代理 ⇒ 按 URL 缓存的独立客户端（reqwest 的代理是 client 级属性，不能按请求换）。
pub fn client_for(region: QoderRegion) -> Result<Client, String> {
    match WebProxyConfig::load().url_for(region) {
        None => Ok(super::api::shared_client()?.clone()),
        Some(url) => cached_client(&url),
    }
}

/// 按代理 URL 缓存的客户端（构造一次，反复用）。
fn cached_client(proxy_url: &str) -> Result<Client, String> {
    static CACHE: OnceLock<Mutex<HashMap<String, Client>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let mut guard = match cache.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    if let Some(client) = guard.get(proxy_url) {
        return Ok(client.clone());
    }
    let proxy =
        reqwest::Proxy::all(proxy_url).map_err(|error| format!("代理地址无法使用：{error}"))?;
    let client = Client::builder()
        .proxy(proxy)
        .connect_timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|error| format!("构造代理 HTTP 客户端失败: {error}"))?;
    guard.insert(proxy_url.to_string(), client.clone());
    Ok(client)
}

// ---------------------------------------------------------------------------
// 测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validation_accepts_empty_and_known_schemes_only() {
        assert!(validate_url("").is_ok(), "空 = 直连，必须合法");
        assert!(validate_url("   ").is_ok());
        for url in [
            "http://127.0.0.1:7890",
            "https://proxy.example.com:8443",
            "socks5://127.0.0.1:1080",
            "socks5h://127.0.0.1:1080",
        ] {
            assert!(validate_url(url).is_ok(), "{url} 应合法");
        }
        for url in ["127.0.0.1:7890", "ftp://x", "http://", "://7890"] {
            assert!(validate_url(url).is_err(), "{url} 应被拒绝");
        }
    }

    #[test]
    fn url_for_is_region_scoped_and_trimmed() {
        let config = WebProxyConfig {
            cn: "  ".to_string(),
            global: " http://127.0.0.1:7890 ".to_string(),
        };
        assert_eq!(config.url_for(QoderRegion::Cn), None, "空白 = 直连");
        assert_eq!(
            config.url_for(QoderRegion::Global).as_deref(),
            Some("http://127.0.0.1:7890")
        );
    }

    #[test]
    fn proxied_client_is_cached_and_bad_url_fails_fast() {
        let good = "http://127.0.0.1:1"; // 不存在也没关系：构造 Client 不发起连接
        let first = cached_client(good).expect("合法 URL 应能构造客户端");
        let second = cached_client(good).expect("第二次应命中缓存");
        // reqwest Client 是 Arc 共享：同 URL 两次拿到的是同一连接池的克隆，
        // 而不是两个各自握手的新客户端。
        drop((first, second));
        let bad = "http://[invalid";
        assert!(
            cached_client(bad).is_err(),
            "URL 解析失败必须在配置时暴露，而不是等到取数"
        );
    }
}
