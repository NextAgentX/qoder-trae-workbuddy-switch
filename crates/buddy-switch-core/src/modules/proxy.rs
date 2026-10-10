//! P12 · C 路（base URL 重定向）的**本地状态** —— **不含任何转发 / 中间人逻辑**。
//!
//! ## 为什么只有「状态」、没有「改写」
//!
//! `P12-ARCH.md` 的 T02 定的是**引导式（方案 A）**：应用**只显示**要设的 env 与命令，
//! **由用户手动设置** —— 因为 `ACC_PRODUCT_CONFIG*` 是**启动期 env**，应用**无法在不碰
//! 用户环境的前提下「悄悄改」**（那也违反「绝不劫持」的精神，D29 红线）。
//!
//! ⇒ 本模块只负责**记一个本地标记**（「用户说他已按引导设过了」），
//!   **不做任何系统级改动**、**不碰 hosts / DNS / 透明代理**。
//!
//! ⚠️ **应用不校验**：标记只代表「用户点了按钮」，**不代表代理真的生效**。
//!   前端文案必须如实（见 `P12-ARCH.md` T04 补充 §①）。

use serde_json::{json, Value};

use super::config::store_dir;

/// 状态文件名（放在 `store_dir()` 下，与其它状态同源）。
const PROXY_STATE_FILE: &str = "proxy-state.json";

/// 网关本机基址（C 路让客户端的 `endpoint` 指向它）。
///
/// ⚠️ 与「端口分离」约定一致（WB **57891** / Trae **7864**）；C 路本期只做 WB 侧。
pub const PROXY_GATEWAY_BASE: &str = "http://127.0.0.1:57891";

/// C 路覆盖的域（**白名单**；兜底行是「其它全部域名 → 直连」）。
pub const PROXY_DOMAINS: &[&str] = &[
    "www.workbuddy.cn",
    "www.codebuddy.cn",
    "copilot.tencent.com",
];

/// 要引导用户设置的**客户端设置键**。
///
/// ★ **2026-10-07 实测发现**：WB 从 **`~/.workbuddy/settings.json` 的 `http.proxy`** 读代理
/// （代码有 `proxySettings:getSettings/saveSettings` IPC，读 VSCode 风格的 `http.proxy`）。
/// **改系统代理 / 注入 `HTTPS_PROXY` env 都无效**（两次实测零流量）——
/// env 那段代码只是「**读出来给 UI 显示**」，不是「用它设代理」。
///
/// ⚠️ **但「直接写这个键」仍待验证**（注入过但未观察到流量）⇒ 本期只**引导**，**不自动写用户设置文件**。
pub const PROXY_SETTING_KEY: &str = "http.proxy";

/// 该键的推荐值（指向本应用网关）。
pub const PROXY_SETTING_VALUE: &str = PROXY_GATEWAY_BASE;

fn state_file() -> std::path::PathBuf {
    store_dir().join(PROXY_STATE_FILE)
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// 读本地标记（**读不到 = 关闭**，fail-closed）。
///
/// ⚠️ 抽成「接受路径」的内部函数，**公开 API 只负责传 `state_file()`** ——
/// 这样单测可以直接传临时路径，**不必改进程级 env**（那既不可靠、又会串测试）。
fn read_enabled_at(path: &std::path::Path) -> bool {
    let Ok(text) = std::fs::read_to_string(path) else {
        return false;
    };
    serde_json::from_str::<Value>(&text)
        .ok()
        .and_then(|v| v.get("enabled").and_then(Value::as_bool))
        .unwrap_or(false)
}

/// 写本地标记。`enabled = false` 时**删除文件**（一键还原**不留残留**）。
fn write_enabled_at(path: &std::path::Path, enabled: bool) -> Result<(), String> {
    if !enabled {
        if path.exists() {
            std::fs::remove_file(path).map_err(|e| format!("清除代理状态失败: {e}"))?;
        }
        return Ok(());
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("创建目录失败: {e}"))?;
    }
    let body = json!({
        "enabled": true,
        // ⚠️ 只记「用户声称已设置」这一事实；**不记录、也无法验证**代理是否真的生效。
        "setAt": now_ms(),
    });
    std::fs::write(path, body.to_string()).map_err(|e| format!("写入代理状态失败: {e}"))
}

pub fn is_enabled() -> bool {
    read_enabled_at(&state_file())
}

pub fn set_enabled(enabled: bool) -> Result<(), String> {
    write_enabled_at(&state_file(), enabled)
}

/// 给前端的**只读视图**（含引导信息；`snake_case`，与其它 view 一致）。
pub fn status_view() -> Value {
    json!({
        "enabled": is_enabled(),
        // ⚠️ **如实**：应用**不校验**代理是否生效，只反映本地标记。
        "verified": false,
        // C 路的拦截方式 = **base URL 重定向**（**不是「中间人」**，不许含糊）。
        "mode": "c-redirect",
        "gatewayBase": PROXY_GATEWAY_BASE,
        "domains": PROXY_DOMAINS,
        // ★ P12 T03：网关**实际转发**的业务路径前缀 —— 读的是**与网关 404 判定同一份**白名单
        //   （`super::biz_allowlist`）⇒ **两侧不会漂移**（本仓在"两份实现"上吃过多次亏）。
        "bizPaths": super::biz_allowlist::BIZ_PATH_PREFIXES,
        // ★ 引导用户设的是**客户端设置键**（不是 env）—— 见 `PROXY_SETTING_KEY` 的注释。
        "setting": {
            "key": PROXY_SETTING_KEY,
            "value": PROXY_SETTING_VALUE,
        },
        // ★ E1：开启态 KPI（`P12-ARCH.md` §3.2 已定契约）。
        //   ⚠️ 计数由**网关进程**写进 `proxy-kpi.json`（`buddy_switch_gateway::proxy_kpi`），
        //   core 只**读**那份台账 ⇒ 不引入 core→gateway 的运行时依赖。
        //   「落盘正文恒 0」是**承诺**（只记元数据），不是统计值。
        "stats": super::proxy_kpi_view::kpi_view(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 每个用例一个**独立临时目录**（不改进程 env ⇒ 不串测试、不污染真实状态）。
    fn temp_state_path(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("bs-proxy-test-{tag}-{}", now_ms()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join(PROXY_STATE_FILE)
    }

    #[test]
    fn defaults_to_disabled_and_fails_closed() {
        let path = temp_state_path("default");
        assert!(!read_enabled_at(&path), "读不到状态时必须视为**关闭**（fail-closed）");
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn enable_then_disable_leaves_no_residue() {
        let path = temp_state_path("roundtrip");
        write_enabled_at(&path, true).unwrap();
        assert!(read_enabled_at(&path));
        write_enabled_at(&path, false).unwrap();
        assert!(!read_enabled_at(&path));
        assert!(!path.exists(), "一键还原后**不得留残留文件**");
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn status_view_is_honest_about_not_verifying() {
        let v = status_view();
        assert_eq!(v["verified"], json!(false), "应用**不校验**，不许声称已验证");
        assert_eq!(v["mode"], json!("c-redirect"), "拦截方式是 base URL 重定向");
        assert!(!v["domains"].as_array().unwrap().is_empty());
        assert_eq!(v["setting"]["key"], json!(PROXY_SETTING_KEY));
    }
}
