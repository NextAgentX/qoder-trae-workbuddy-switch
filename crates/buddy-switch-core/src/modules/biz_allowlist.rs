//! P12 · 业务请求转发的**路径白名单**（本地路由路线）。
//!
//! ## 为什么需要白名单
//!
//! 「本地路由」下，客户端把 **base URL** 指向本机网关（明文 `http`），
//! 于是**网关会收到本该发给上游的业务请求**。网关**只应转发已知的业务路径**，
//! 其余一律**明确拒绝**（404）—— **不静默、不猜测**。
//!
//! ## ★ 与 cc-switch「最小侵入」同精神
//!
//! cc-switch 只替换目标工具配置里的**关键字段**（endpoint/key/model），其余原样保留。
//! 本白名单同源：**只转发列出的路径前缀**，其它路径**不碰**。
//!
//! ## ★ 绝不劫持（D29 红线，不变）
//!
//! 本模块**只做「路径是否在册」的判定** —— **没有任何** hosts / DNS / 透明代理写路径。
//! 网关是**客户端自愿指向**的本地服务，不是系统级拦截。
//!
//! ## 已知范围（实测）
//!
//! 前缀取自本仓既有的业务 API 常量（`config.rs`）：
//! `CHECKIN_API_PREFIX` = `/v2/billing/meter`、`TRAVEL_API_PREFIX` = `/activity/growth/buddy/travel`，
//! 外加 WorkBuddy 业务域基址下的 `/v2/plugin`（`WORKBUDDY_API_PREFIX`）。

/// 允许转发的**业务路径前缀**（**顺序即 UI 展示顺序**）。
///
/// ⚠️ 加新前缀 = 在**本表**加一行 + 在面板白名单表里同步（面板从后端读，故只改这里）。
pub const BIZ_PATH_PREFIXES: &[&str] = &[
    // 签到 / 积分计量（`config.rs` 的 `CHECKIN_API_PREFIX`）。
    "/v2/billing/",
    // 业务插件接口（`config.rs` 的 `WORKBUDDY_API_PREFIX`）。
    "/v2/plugin/",
    // 自动旅行（`config.rs` 的 `TRAVEL_API_PREFIX` 的域内前缀）。
    "/activity/",
];

/// 该路径是否在**业务白名单**内（**fail-closed**：不在册即 `false`）。
///
/// ⚠️ 判据是**前缀匹配**且**要求路径以 `/` 开头** —— 空串、相对路径、
/// 带 `..` 的穿越尝试一律 `false`（**不解析、不归一化**，避免把自己绕进去）。
pub fn is_allowed_path(path: &str) -> bool {
    if !path.starts_with('/') {
        return false;
    }
    // 拒绝任何穿越尝试（`..` / `//`）—— 白名单是"确知安全"的清单，不该靠归一化兜底。
    if path.contains("..") || path.contains("//") {
        return false;
    }
    BIZ_PATH_PREFIXES.iter().any(|prefix| path.starts_with(prefix))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_known_business_paths() {
        assert!(is_allowed_path("/v2/billing/meter/daily-checkin"));
        assert!(is_allowed_path("/v2/plugin/user/credits"));
        assert!(is_allowed_path("/activity/growth/buddy/travel/status"));
    }

    #[test]
    fn rejects_unknown_paths_fail_closed() {
        assert!(!is_allowed_path("/v2/chat/completions"), "LLM 路径不走业务白名单");
        assert!(!is_allowed_path("/admin/secret"));
        assert!(!is_allowed_path(""), "空路径必须拒绝");
        assert!(!is_allowed_path("v2/billing/meter"), "相对路径必须拒绝");
    }

    #[test]
    fn rejects_traversal_attempts() {
        assert!(!is_allowed_path("/v2/billing/../../etc/passwd"));
        assert!(!is_allowed_path("/v2//billing/meter"), "双斜杠必须拒绝（不靠归一化兜底）");
    }
}
