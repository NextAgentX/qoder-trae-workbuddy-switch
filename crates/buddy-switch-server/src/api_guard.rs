//! 管理面的**来源校验**（Host / Origin / Sec-Fetch-Site / Content-Type）。
//!
//! ## 为什么需要它（安全审计 P0-1）
//!
//! `buddy-switch-server` 把**整个管理面**挂在 HTTP 上，而它背后是全部明文凭据：
//! `POST /api/accounts/export` 直接返回 `access_token` / `refresh_token`，
//! `POST /api/accounts/export-to-path` 可写任意绝对路径。
//!
//! 改造前 `api_routes()` **一个中间件都没有**（`HeaderMap` / `Authorization` /
//! `Origin` / `Host` 在整个 `api.rs` 里出现 0 次、`.layer()` 0 次）。默认绑
//! `127.0.0.1` 时，唯一屏障是「外部网页读不到回环端口」—— 而 **DNS rebinding**
//! 正好绕过它：恶意页面把 `attacker.com` 解析到 `127.0.0.1`，浏览器就认为
//! `http://attacker.com:57890` 是同源，可以读回响应。
//!
//! ## 防线为什么是 Host
//!
//! DNS rebinding 下 `Origin` 与 `Host` **是同源的**（都是 `attacker.com`），
//! 所以「Origin 与 Host 同源」这条**挡不住它**；唯一有效的是**要求 Host 本身可信**。
//! 这与本仓 `agent_hook_rx::validate`（issue 前身的安全基线）用的是同一套判据，
//! 那边注释已经写明「Host 校验是 DNS rebinding 的唯一防线」。
//!
//! ## 对既有部署的影响
//!
//! - 浏览器用 `localhost` / `127.0.0.1` 访问 ⇒ Host 是回环 ⇒ 放行（**官方 Docker
//!   用法映射的正是 `127.0.0.1:57890`，不受影响**）。
//! - 用局域网 IP / 域名远程访问 ⇒ 默认 **403**，必须在启动前设置
//!   `BUDDY_SWITCH_ALLOWED_HOSTS=192.168.1.5:57890,my-nas.local:57890`。
//!   这是**刻意**的行为变更：暴露管理面应当是一次显式决定，而不是默认后果。

use std::sync::OnceLock;

use axum::extract::Request;
use axum::http::{header, Method, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use serde_json::json;

/// 额外允许的 `Host`（逗号分隔）环境变量名。
pub const ALLOWED_HOSTS_ENV: &str = "BUDDY_SWITCH_ALLOWED_HOSTS";

/// 运行时策略。启动时定一次，之后只读。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Policy {
    /// 小写化的允许项：可以是 `host:port`（精确）或纯主机名（任意端口）。
    allowed_hosts: Vec<String>,
}

impl Policy {
    /// 该 `Host` 是否被允许（回环之外的第二条通路）。
    fn allows(&self, host: &str) -> bool {
        let host = host.trim().to_ascii_lowercase();
        if host.is_empty() {
            return false;
        }
        let hostname = host_name_of(&host);
        self.allowed_hosts
            .iter()
            .any(|entry| entry == &host || entry == hostname)
    }
}

/// 解析 `BUDDY_SWITCH_ALLOWED_HOSTS`。
///
/// 容错：空项 / 首尾空白 / 大小写都吃掉；`*` **不支持**（一个通配符就把这道防线变成装饰）。
pub fn parse_allowed_hosts(raw: Option<&str>) -> Policy {
    let mut allowed_hosts = Vec::new();
    for entry in raw.unwrap_or_default().split(',') {
        let entry = entry.trim().to_ascii_lowercase();
        if entry.is_empty() || entry == "*" {
            continue;
        }
        if !allowed_hosts.contains(&entry) {
            allowed_hosts.push(entry);
        }
    }
    Policy { allowed_hosts }
}

/// 取 `Host` 的主机名部分（去掉 `:port`，正确处理 IPv6 字面量）。
fn host_name_of(host: &str) -> &str {
    let host = host.trim();
    if let Some(rest) = host.strip_prefix('[') {
        // `[::1]` / `[::1]:57890`
        return match rest.find(']') {
            Some(end) => &rest[..end],
            None => rest,
        };
    }
    host.split(':').next().unwrap_or(host)
}

/// `Host` 是否指向本机回环。
///
/// 判据与 `agent_hook_rx` 的同名函数一致（`127.0.0.1` / `localhost` / `::1`）；
/// 刻意**不含** `0.0.0.0` —— 它不是一个可以被解析的「本机地址」，
/// 浏览器把 `0.0.0.0:port` 当 Host 发出来时，那也不是用户应有的访问方式。
fn host_is_loopback(host: &str) -> bool {
    let hostname = host_name_of(host);
    hostname == "127.0.0.1" || hostname.eq_ignore_ascii_case("localhost") || hostname == "::1"
}

/// 请求被拒的原因。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Deny {
    /// 没有 `Host` 头（HTTP/1.0 或伪造）。
    MissingHost,
    /// `Host` 既不是回环、也不在白名单里。
    HostNotAllowed,
    /// `Origin` 与 `Host` 不同源，或 `Sec-Fetch-Site: cross-site`。
    CrossOrigin,
    /// 写请求没有 `application/json`。
    NotJson,
}

impl Deny {
    fn status(self) -> StatusCode {
        match self {
            Deny::NotJson => StatusCode::UNSUPPORTED_MEDIA_TYPE,
            _ => StatusCode::FORBIDDEN,
        }
    }

    /// 给**用户**看的解释（不是给攻击者的）。
    ///
    /// 远程访问被拒时必须说清「怎么放开」，否则用户只会看到 403 然后以为软件坏了。
    fn message(self) -> &'static str {
        match self {
            Deny::MissingHost => "请求缺少 Host 头，管理面拒绝处理。",
            Deny::HostNotAllowed => {
                "管理面只接受本机访问。若确实要远程访问，请用环境变量 \
                 BUDDY_SWITCH_ALLOWED_HOSTS 显式列出允许的主机（逗号分隔，可带端口），\
                 并确保只有可信网络能连到该端口。"
            }
            Deny::CrossOrigin => "跨站请求被拒绝（Origin 与 Host 不同源）。",
            Deny::NotJson => "管理面的写接口只接受 application/json。",
        }
    }
}

/// 四条校验的**唯一**收敛点（纯函数，好单测）。
///
/// 抽成纯函数的原因与 `agent_hook_rx::validate` 相同：安全校验最容易在
/// 「加一条新规则」时被顺手改坏，而它的症状是**静默放开**（不报错、只是不再拦）。
pub fn judge(
    policy: &Policy,
    method: &Method,
    host: Option<&str>,
    origin: Option<&str>,
    sec_fetch_site: Option<&str>,
    content_type: Option<&str>,
) -> Result<(), Deny> {
    let Some(host) = host.filter(|value| !value.trim().is_empty()) else {
        return Err(Deny::MissingHost);
    };
    // ① Host 必须是回环 **或** 显式白名单。这是唯一能挡 DNS rebinding 的一条。
    if !host_is_loopback(host) && !policy.allows(host) {
        return Err(Deny::HostNotAllowed);
    }
    // ② Origin 存在时必须与 Host 同源。对**普通**跨站请求有效（rebinding 由 ① 挡）。
    if let Some(origin) = origin.map(str::trim).filter(|value| !value.is_empty()) {
        let same = origin.eq_ignore_ascii_case(&format!("http://{host}"))
            || origin.eq_ignore_ascii_case(&format!("https://{host}"));
        if !same {
            return Err(Deny::CrossOrigin);
        }
    }
    // ③ 浏览器主动声明跨站 ⇒ 直接拒（纵深，兜住 ② 可能被省略的场合）。
    if sec_fetch_site
        .map(str::trim)
        .is_some_and(|value| value.eq_ignore_ascii_case("cross-site"))
    {
        return Err(Deny::CrossOrigin);
    }
    // ④ 写请求必须是 JSON。全部 226 条 `/api/*` 路由都用 `Json<Value>` 提取，
    //    所以这一条不会误杀；它挡的是「用简单请求（text/plain）绕开预检」的 CSRF 手法。
    if !matches!(*method, Method::GET | Method::HEAD | Method::OPTIONS)
        && !content_type_is_json(content_type)
    {
        return Err(Deny::NotJson);
    }
    Ok(())
}

fn content_type_is_json(content_type: Option<&str>) -> bool {
    let Some(content_type) = content_type else {
        return false;
    };
    let mime = content_type.split(';').next().unwrap_or_default().trim();
    mime.eq_ignore_ascii_case("application/json")
}

static POLICY: OnceLock<Policy> = OnceLock::new();

/// 启动时装载策略（`main` 调用一次；测试可直接用 [`judge`]）。
pub fn init(raw: Option<&str>) -> &'static Policy {
    POLICY.get_or_init(|| parse_allowed_hosts(raw))
}

fn policy() -> &'static Policy {
    POLICY.get_or_init(|| parse_allowed_hosts(std::env::var(ALLOWED_HOSTS_ENV).ok().as_deref()))
}

/// axum 中间件：把 [`judge`] 套在管理面路由上。
pub async fn guard(request: Request, next: Next) -> Response {
    let headers = request.headers();
    let header_str = |name: header::HeaderName| -> Option<&str> {
        headers.get(name).and_then(|value| value.to_str().ok())
    };
    let verdict = judge(
        policy(),
        request.method(),
        header_str(header::HOST),
        header_str(header::ORIGIN),
        headers
            .get("sec-fetch-site")
            .and_then(|value| value.to_str().ok()),
        header_str(header::CONTENT_TYPE),
    );
    if let Err(deny) = verdict {
        return (
            deny.status(),
            axum::Json(json!({
                "ok": false,
                "error": deny.message(),
                "reason": match deny {
                    Deny::MissingHost => "missing_host",
                    Deny::HostNotAllowed => "host_not_allowed",
                    Deny::CrossOrigin => "cross_origin",
                    Deny::NotJson => "not_json",
                },
            })),
        )
            .into_response();
    }
    next.run(request).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy_with(raw: Option<&str>) -> Policy {
        parse_allowed_hosts(raw)
    }

    fn ok(policy: &Policy, method: Method, host: Option<&str>) -> Result<(), Deny> {
        judge(policy, &method, host, None, None, None)
    }

    /// ★★ 安全审计 P0-1：**DNS rebinding 必须被 Host 校验挡住**。
    ///
    /// rebinding 时 `Host` 与 `Origin` **同源**（都是 `attacker.com`）⇒ 只靠
    /// 「Origin 同源」永远不会拦下它，唯一有效的是 Host 必须可信。
    #[test]
    fn dns_rebinding_is_blocked_by_host_not_by_origin() {
        let policy = policy_with(None);
        // 攻击者的 Host，且 Origin 与之同源（这就是 rebinding 的形态）。
        assert_eq!(
            judge(
                &policy,
                &Method::POST,
                Some("attacker.com:57890"),
                Some("http://attacker.com:57890"),
                Some("same-origin"),
                Some("application/json"),
            ),
            Err(Deny::HostNotAllowed),
            "同源的 Origin 不是防线，Host 才是"
        );
        // 对照：同样的 Origin 形式，但 Host 是回环 ⇒ 放行。
        assert!(judge(
            &policy,
            &Method::POST,
            Some("127.0.0.1:57890"),
            Some("http://127.0.0.1:57890"),
            Some("same-origin"),
            Some("application/json"),
        )
        .is_ok());
    }

    #[test]
    fn loopback_hosts_are_accepted_in_every_common_spelling() {
        let policy = policy_with(None);
        for host in [
            "127.0.0.1:57890",
            "localhost:57890",
            "LOCALHOST:57890",
            "[::1]:57890",
            "127.0.0.1",
        ] {
            assert!(
                ok(&policy, Method::GET, Some(host)).is_ok(),
                "{host} 应当被接受"
            );
        }
        // 反例：`0.0.0.0` 不是「本机地址」，浏览器也不该用它访问。
        assert_eq!(
            ok(&policy, Method::GET, Some("0.0.0.0:57890")),
            Err(Deny::HostNotAllowed)
        );
        assert_eq!(
            ok(&policy, Method::GET, Some("192.168.1.5:57890")),
            Err(Deny::HostNotAllowed)
        );
        assert_eq!(ok(&policy, Method::GET, None), Err(Deny::MissingHost));
    }

    /// 远程部署的逃生舱：必须是**显式**白名单，且 `*` 不被当成通配符。
    #[test]
    fn allowed_hosts_is_an_explicit_list_never_a_wildcard() {
        let policy = policy_with(Some(" 192.168.1.5:57890 , nas.local ,, "));
        assert!(ok(&policy, Method::GET, Some("192.168.1.5:57890")).is_ok());
        assert!(ok(&policy, Method::GET, Some("nas.local:57890")).is_ok());
        // 未列出的仍然拒。
        assert_eq!(
            ok(&policy, Method::GET, Some("192.168.1.6:57890")),
            Err(Deny::HostNotAllowed)
        );
        // `*` 被丢弃 ⇒ 不会变成「放行一切」。
        let wildcard = policy_with(Some("*"));
        assert_eq!(
            ok(&wildcard, Method::GET, Some("evil.example:57890")),
            Err(Deny::HostNotAllowed),
            "通配符必须被忽略，否则这道防线等于没有"
        );
    }

    #[test]
    fn cross_site_markers_are_rejected() {
        let policy = policy_with(None);
        let host = Some("127.0.0.1:57890");
        // Origin 不同源。
        assert_eq!(
            judge(
                &policy,
                &Method::POST,
                host,
                Some("http://evil.example"),
                None,
                Some("application/json"),
            ),
            Err(Deny::CrossOrigin)
        );
        // 浏览器自报 cross-site（Origin 可能被省略）。
        assert_eq!(
            judge(
                &policy,
                &Method::POST,
                host,
                None,
                Some("cross-site"),
                Some("application/json"),
            ),
            Err(Deny::CrossOrigin)
        );
        // 对照：same-origin 且无 Origin 头 ⇒ 放行（同源 GET/POST 的常态）。
        assert!(judge(
            &policy,
            &Method::POST,
            host,
            None,
            Some("same-origin"),
            Some("application/json"),
        )
        .is_ok());
    }

    /// 写请求必须 JSON —— 挡的是「`text/plain` 简单请求绕开 CORS 预检」的 CSRF。
    #[test]
    fn write_requests_must_be_json_but_reads_are_not_checked() {
        let policy = policy_with(None);
        let host = Some("127.0.0.1:57890");
        assert_eq!(
            judge(&policy, &Method::POST, host, None, None, Some("text/plain")),
            Err(Deny::NotJson)
        );
        assert_eq!(
            judge(&policy, &Method::POST, host, None, None, None),
            Err(Deny::NotJson)
        );
        // `charset` 后缀不影响判定。
        assert!(judge(
            &policy,
            &Method::POST,
            host,
            None,
            None,
            Some("application/json; charset=utf-8"),
        )
        .is_ok());
        // GET 不检查 Content-Type。
        assert!(judge(&policy, &Method::GET, host, None, None, None).is_ok());
    }

    #[test]
    fn deny_reasons_map_to_expected_status_codes() {
        assert_eq!(Deny::HostNotAllowed.status(), StatusCode::FORBIDDEN);
        assert_eq!(Deny::CrossOrigin.status(), StatusCode::FORBIDDEN);
        assert_eq!(Deny::MissingHost.status(), StatusCode::FORBIDDEN);
        assert_eq!(Deny::NotJson.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);
        // 远程被拒时文案必须告诉用户怎么放开（否则只会以为软件坏了）。
        assert!(Deny::HostNotAllowed.message().contains(ALLOWED_HOSTS_ENV));
    }
}
