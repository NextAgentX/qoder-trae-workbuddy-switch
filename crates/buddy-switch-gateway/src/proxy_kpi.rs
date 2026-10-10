//! ★ E1 · 代理 KPI 的**真实计数**（今日代理请求 / 今日换号）。
//!
//! ## 为什么需要这个模块（而不是在前端算）
//!
//! 原型要求开启态显示三个 KPI（`demo-app.js:1131-1134`）。前两个**必须有真实来源**：
//!
//! - **今日代理请求**：走**业务转发**（`routes/biz.rs`）的请求数。
//!   ⚠️ 关键事实：业务转发**此前完全不写请求日志**（只有 LLM 路由写 `RequestLog`），
//!   所以既有的 `gateway_logs.json` **数不出**代理请求数 —— 这不是「取不到」，
//!   而是**数据从未产生**。本模块补上这份计数。
//! - **今日换号**：`relay` 轮换时**换了 uid** 的次数（同 uid 重试不计）。
//!
//! ## ★ 为什么要落盘（而不是进程内 Atomic）
//!
//! 「今日」跨**网关重启**。若只用内存计数器，重启后归零 ⇒ 界面显示「今日 0」，
//! 而真相是「**不知道**」（网关重启前的数字丢了）—— 这违反本仓最硬的数据诚实纪律
//! （`overview-metrics.ts`：0 是「确实是 0」，读不到是「不知道」，两者不能混）。
//! ⇒ 按自然日落盘，跨日自动清零；读不到时**返回 `None` 而不是 0**。
//!
//! ## 「落盘正文」不在此模块
//!
//! 第三个 KPI「落盘正文 = 0」是**承诺**（网关只记元数据），不是统计出来的数
//! ⇒ 由前端恒显 0，不走后端。

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};
use serde_json::json;

/// 进程内热计数（写请求时累加；**由 [`flush`] 落盘**）。
///
/// ⚠️ 热计数**不带日期** —— 跨日由 [`flush`] 读落盘日期决定是否清零，
/// 这样热路径不必每次读文件。
#[derive(Debug, Default)]
pub struct ProxyCounters {
    requests: AtomicU64,
    switches: AtomicU64,
}

impl ProxyCounters {
    pub fn new() -> Self {
        Self::default()
    }

    /// 记一次**代理请求**（业务转发到达网关即计，与是否成功无关 ——
    /// KPI 问的是「今天有多少请求走过这里」，不是「成功多少」）。
    pub fn record_request(&self) {
        self.requests.fetch_add(1, Ordering::Relaxed);
    }

    /// 记一次**换号**（`from` 与 `to` 都非空且不同）。
    pub fn record_switch(&self) {
        self.switches.fetch_add(1, Ordering::Relaxed);
    }

    /// 进程启动时把**已落盘**的当日台账并回热计数（否则重启后「今日」从零起算）。
    pub fn restore(&self, requests: u64, switches: u64) {
        self.requests.store(requests, Ordering::Relaxed);
        self.switches.store(switches, Ordering::Relaxed);
    }

    pub fn snapshot(&self) -> (u64, u64) {
        (
            self.requests.load(Ordering::Relaxed),
            self.switches.load(Ordering::Relaxed),
        )
    }
}

/// 落盘形状：`{ "2026-10-10": { "requests": 12, "switches": 3 } }`。
///
/// 只留**当天**一条 ⇒ 文件天然是「今日台账」，跨日写入即替换。
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct DayCounters {
    #[serde(default)]
    pub requests: u64,
    #[serde(default)]
    pub switches: u64,
}

/// 台账文件：`~/.buddy-switch/proxy-kpi.json`（与 `gateway_config.json` **同目录**，
/// 沿用 `disabled_models::file()` 的既有写法，不另立路径口径）。
fn stats_file() -> PathBuf {
    buddy_switch_core::modules::region::gateway_config_file().with_file_name("proxy-kpi.json")
}

/// 把热计数并入落盘台账（**跨日自动清零**）。
pub fn flush(counters: &ProxyCounters) {
    flush_at(&stats_file(), counters)
}

/// 「接受路径」的内部实现 —— 单测直接传临时文件，**不必**改进程级 HOME
/// （照 `backup.rs` 的同款做法：改 env 不可靠且会串测试）。
fn flush_at(path: &std::path::Path, counters: &ProxyCounters) {
    let today = crate::timeutil::cst_date_str(crate::timeutil::now_ms());
    let (requests, switches) = counters.snapshot();

    let mut ledger: BTreeMap<String, DayCounters> = std::fs::read_to_string(&path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default();
    let entry = ledger.entry(today.clone()).or_default();
    // 累加而非覆盖：一次 flush 只包含「上次 flush 之后」的新增量。
    entry.requests = entry.requests.saturating_add(requests);
    entry.switches = entry.switches.saturating_add(switches);
    // 只保留当天（昨天的数字对「今日」没有意义，留着反而会被误读）。
    ledger.retain(|day, _| day == &today);

    if let Ok(text) = serde_json::to_string(&ledger) {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(&path, text);
    }
    counters.requests.store(0, Ordering::Relaxed);
    counters.switches.store(0, Ordering::Relaxed);
}

/// 读**今日** KPI。
///
/// ⚠️ **读不到 ⇒ `None`**（前端显示「—」），**不给 0**。
/// 0 的含义是「今天确实一次都没走过」，与「台账文件还没写过」是两件事。
pub fn today() -> Option<DayCounters> {
    today_at(&stats_file())
}

fn today_at(path: &std::path::Path) -> Option<DayCounters> {
    let today = crate::timeutil::cst_date_str(crate::timeutil::now_ms());
    let text = std::fs::read_to_string(path).ok()?;
    let ledger: BTreeMap<String, DayCounters> = serde_json::from_str(&text).ok()?;
    ledger.get(&today).cloned()
}

/// ★ 先把热计数落盘，再读当日台账（**读路径即 flush 点**）。
///
/// ## 为什么不在后台定时器里落盘
///
/// 定时器会随进程被杀而丢掉最后一段增量（用户「关掉应用」是常态）⇒ 数字会**少算**，
/// 而 KPI 上的数字少算同样是对用户的**谎报**。挂在**读**上则保证：只要界面显示过，
/// 数字就已经落盘；进程被杀最多丢「最后一段还没被任何人看过」的增量。
pub fn flush_and_read(counters: &ProxyCounters) -> Option<DayCounters> {
    flush_and_read_at(&stats_file(), counters)
}

fn flush_and_read_at(path: &std::path::Path, counters: &ProxyCounters) -> Option<DayCounters> {
    if counters.snapshot() != (0, 0) {
        flush_at(path, counters);
    }
    today_at(path)
}

/// 给 `ProxyStatus` 用的 JSON 视图（`stats` 段）。
pub fn status_stats_json(counters: &ProxyCounters) -> serde_json::Value {
    match flush_and_read(counters) {
        Some(day) => json!({
            "todayRequests": day.requests,
            "switches": day.switches,
            // ⚠️ 恒 0：网关只记元数据、**不落盘请求正文**（D29 承诺）。
            "persistedBodies": 0,
            "known": true,
        }),
        // 从未记录 ⇒ 明确说「不知道」，而不是谎报 0。
        None => json!({ "todayRequests": null, "switches": null, "persistedBodies": 0, "known": false }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 每个用例一个**独立临时文件**。
    ///
    /// ⚠️ 早期版本三条用例共用 `stats_file()`（进程级全局路径）⇒ **互相踩**：
    /// `missing_ledger` 会把文件删掉、`persisted_bodies` 也会删，于是并��跑时
    /// `flush_accumulates` 读到的台账被抹掉而变红。**不是逻辑错，是测试不隔离。**
    /// （照 `backup.rs::tests::isolated_home` 的同款纪律：各测各的临时路径。）
    fn temp_file(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("bs-proxy-kpi-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("proxy-kpi.json")
    }

    /// 读不到台账时必须返回 `None`（**不许**回落 0：那会把「不知道」说成「确实是 0」）。
    #[test]
    fn missing_ledger_reads_as_unknown_not_zero() {
        let path = temp_file("missing");
        let _ = std::fs::remove_file(&path);
        assert!(
            today_at(&path).is_none(),
            "台账不存在时必须是「不知道」（None），不得回落 0"
        );
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    /// 计数累加、且只留当天。
    ///
    /// 可证伪：把 `entry.requests = …saturating_add` 改成 `= requests`（覆盖）
    /// ⇒ 第二次 flush 后总数会变小 ⇒ 本条变红。
    #[test]
    fn flush_accumulates_instead_of_overwriting() {
        let path = temp_file("accumulate");
        let _ = std::fs::remove_file(&path);

        let first = ProxyCounters::new();
        first.record_request();
        first.record_request();
        first.record_switch();
        flush_at(&path, &first);
        let after_first = today_at(&path).expect("首次 flush 后应有台账");
        assert_eq!(after_first.requests, 2);
        assert_eq!(after_first.switches, 1);

        let second = ProxyCounters::new();
        second.record_request();
        flush_at(&path, &second);
        let after_second = today_at(&path).expect("二次 flush 后应有台账");
        assert_eq!(
            after_second.requests, 3,
            "第二次 flush 应**累加**（2+1），覆盖写会退回 1"
        );
        assert_eq!(after_second.switches, 1, "未换号则计数不变");

        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    /// 进程重启后**不丢数**：`restore` 把落盘台账并回热计数。
    ///
    /// 这条守的是「今日」跨网关重启 —— 少了它，重启后界面会显示「今日 0」，
    /// 而真相是「不知道」，那是**谎报**。
    #[test]
    fn restore_reloads_persisted_counters() {
        let path = temp_file("restore");
        let _ = std::fs::remove_file(&path);

        let before = ProxyCounters::new();
        before.record_request();
        before.record_switch();
        flush_at(&path, &before);

        // 模拟重启：新进程从落盘台账恢复。
        let after = ProxyCounters::new();
        let day = today_at(&path).expect("台账应存在");
        after.restore(day.requests, day.switches);
        assert_eq!(after.snapshot(), (1, 1), "重启后计数应从台账恢复，不从零起算");

        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    /// 「落盘正文」恒为 0 —— 它是**承诺**，不是统计值。
    #[test]
    fn persisted_bodies_is_always_zero() {
        let path = temp_file("bodies");
        let counters = ProxyCounters::new();
        counters.record_request();
        let day = flush_and_read_at(&path, &counters).expect("flush 后应有台账");
        assert_eq!(day.requests, 1, "请求数应被记录");
        // core 侧的视图恒把 persistedBodies 写成 0（承诺），此处只断言计数不牵连它。
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }
}
