//! ★ E1 · 代理 KPI 台账的**只读视图**（core 侧）。
//!
//! ## 为什么要单独一个模块（而不是让 core 直接依赖 gateway）
//!
//! 计数由**网关进程**在转发路径上写（`buddy_switch_gateway::proxy_kpi`），
//! 而 `proxy::status_view()` 在 **core**。若 core 直接调 gateway 的函数，
//! 就把 `core → gateway` 变成**编译期依赖** —— 而本仓的依赖方向是单向的
//! （gateway 依赖 core，反向会被 `cargo` 直接拒绝，也破坏 `MEMORY-10` 记的
//! 「core 为唯一来源」的分层）。
//!
//! ⇒ 共识只有**一个文件**：`~/.buddy-switch/proxy-kpi.json`
//! （与 `gateway_config.json` 同目录）。core 只**读**，不写。
//!
//! ## 诚实边界
//!
//! 读不到 / 当天没有记录 ⇒ `known: false` 且两个计数为 `null`
//! ⇒ 前端显示「—」。**不给 0**：0 的含义是「今天确实一次都没走过」，
//! 与「台账还不存在」（= 不知道）是两件事（`overview-metrics.ts` 的同一纪律）。
//!
//! `persistedBodies` 恒 `0`：它是**承诺**（网关只记元数据、不落盘请求正文，D29），
//! 不是统计出来的数 ⇒ 无论有没有台账都是 0。

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde_json::{json, Value};

use super::region::gateway_config_file;

/// 台账文件（与 gateway 侧 `proxy_kpi::stats_file()` **同一份** —— 路径必须一致，
/// 否则界面会永远显示「—」而网关在老实写数）。
fn stats_file() -> PathBuf {
    gateway_config_file().with_file_name("proxy-kpi.json")
}

#[derive(Debug, Clone, Copy, Default, serde::Deserialize)]
struct DayCounters {
    #[serde(default)]
    requests: u64,
    #[serde(default)]
    switches: u64,
}

/// `ProxyStatus.stats` 段（`P12-ARCH.md` §3.2 契约）。
pub fn kpi_view() -> Value {
    let today = super::cst::cst_date_str(super::cst::cst_datetime(now_ms()).timestamp_millis());
    let ledger: Option<BTreeMap<String, DayCounters>> = std::fs::read_to_string(stats_file())
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok());

    match ledger.and_then(|map| map.get(&today).copied()) {
        Some(day) => json!({
            "todayRequests": day.requests,
            "switches": day.switches,
            "persistedBodies": 0,
            "known": true,
        }),
        None => json!({
            "todayRequests": Value::Null,
            "switches": Value::Null,
            "persistedBodies": 0,
            "known": false,
        }),
    }
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 读不到台账 ⇒ `known: false` + 计数为 `null`（**不许**回落 0）。
    #[test]
    fn missing_ledger_reads_as_unknown_not_zero() {
        let path = stats_file();
        let backup = std::fs::read_to_string(&path).ok();
        let _ = std::fs::remove_file(&path);

        let view = kpi_view();
        assert_eq!(view["known"], json!(false), "无台账必须说「不知道」");
        assert_eq!(
            view["todayRequests"], json!(null),
            "未知时必须是 null（前端显示「—」），不得回落 0"
        );
        assert_eq!(view["switches"], json!(null));
        // 「落盘正文」是**承诺**，无论有没有数据都是 0。
        assert_eq!(view["persistedBodies"], json!(0));

        if let Some(text) = backup {
            let _ = std::fs::write(&path, text);
        }
    }

    /// 台账里**没有今天**（例如只剩昨天）⇒ 同样是「不知道」，不是 0。
    #[test]
    fn stale_day_is_unknown_not_zero() {
        let path = stats_file();
        let backup = std::fs::read_to_string(&path).ok();
        std::fs::write(
            &path,
            r#"{"2000-01-01":{"requests":999,"switches":9}}"#,
        )
        .unwrap();

        let view = kpi_view();
        assert_eq!(
            view["known"], json!(false),
            "只有旧日期的台账 ⇒ 今天「不知道」，不得把昨天的数当今天"
        );

        if let Some(text) = backup {
            let _ = std::fs::write(&path, text);
        } else {
            let _ = std::fs::remove_file(&path);
        }
    }
}
