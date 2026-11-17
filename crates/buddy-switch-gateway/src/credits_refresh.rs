//! 账号池余额刷新：把账号库里的**真实积分余额**喂给 [`crate::pool::Pool`]。
//!
//! ## 为什么需要它
//!
//! 选号权重 [`crate::pool::pick::weight_of`] 里权重最大的两项是「积分比例」与
//! 「快过期占比」，二者的数据源都是池条目上的 `credits` / `credits_expiring`。
//! 但在本模块出现之前，**生产路径上没有任何地方写过这两个字段**——唯一写池的是
//! `relay.rs` 的 `pool.sync_accounts(...)`，而它只写 uid / realm / nickname。结果是
//! 池里的 `credits` 恒为 0，`max_credits` 也为 0，`weight_of` 的积分分支退化为死代码，
//! 对外宣称的「四因子加权」在生产上只剩「闲置补偿 + 成功率 + 成本分层」。
//!
//! ## 硬约束：**绝不在请求路径上同步拉余额**
//!
//! 余额是**慢变数据**，而每个请求多一次上游往返会直接拉高端到端延迟。因此刷新
//! **只能**由后台周期循环触发（见 `buddy-switch-server` 的独立刷新循环）。后来者最容易
//! 「顺手」把一次取数塞进 `relay` / `pick` 的请求路径，那会违背本模块存在的全部意义。
//!
//! ## 设计：可注入的取数缝
//!
//! 「取余额」被抽成 [`CreditFetcher`]，生产用 [`CoreCreditFetcher`]，单测用假取数器，
//! 与既有的 `CatIo` / `UsageSink` 是同一套思路——单测**绝不发真实网络请求**。

use std::future::Future;
use std::pin::Pin;

use serde_json::{json, Value};

use buddy_switch_core::modules::account;
use buddy_switch_core::modules::credits;
use buddy_switch_core::modules::region::Region;

use crate::pool::{Pool, RealmTag};
use crate::state::GatewayState;
use crate::timeutil;

/// 余额刷新新鲜度阈值的缺省值（30 分钟）。
///
/// 与 [`crate::pool::PoolConfig::credits_refresh_interval_ms`] 的缺省保持一致；
/// 配置里的非正值在刷新时归一到本值（配置本身也会在 [`crate::pool::PoolConfig::normalized`]
/// 里归一，这里是第二道防线，保证刷新器独立可用时也安全）。
pub const DEFAULT_REFRESH_INTERVAL_MS: i64 = 30 * 60 * 1000;

/// 池为空时的探测周期（毫秒）。
///
/// ## 为什么必须有这条（2026-09-30 实测缺陷）
///
/// 池的账号同步**只发生在**「有请求经过」（`relay`）或「管理面读状态」（`fill_pool` 之前）
/// 时 —— 因此**进程刚启动时池通常是空的**（`state.json` 只在治理状态有改动时才写盘）。
///
/// 而刷新循环是「启动即刷一次，然后等一个周期」。池空 ⇒ 本轮 `refresh_targets` 为空 ⇒
/// 什么都没刷，却照样等满 **30 分钟**。实测：19:54 起服务，20:24 才出现第一条余额读数 ——
/// 用户看到的是「刚导入账号，页面上积分一直是『—』」。
///
/// 探测成本 = 一次读锁 + 遍历空表，可忽略；换来「账号一进池就被刷新」。
pub const EMPTY_POOL_PROBE_MS: u64 = 15_000;

/// 下一轮余额刷新的等待时长（毫秒）——**两侧宿主共用**，别各写一份判据。
///
/// - **池为空** ⇒ 短探测：本轮必然空转，但账号随时可能被同步进来。
/// - **池非空** ⇒ 按池配置的正常周期。注意「池非空但取数失败」也走这条 ——
///   失败时上游多半在闹脾气，短周期重试只会把上游打得更狠。
pub fn next_refresh_wait_ms(pool: &Pool) -> u64 {
    if pool.is_empty() {
        EMPTY_POOL_PROBE_MS
    } else {
        pool.config().credits_refresh_interval_ms.max(1) as u64
    }
}

/// 取余额的可注入缝。
///
/// 实现者按 `(region, uid)` 取回该账号的**真实**余额；返回体是 `credits.rs::credit_result`
/// 形态的 JSON（成功含 `ok:true` / `totalRemaining` / `resources`；失败含 `ok:false`）。
/// 账号对象由实现者自行解析（生产用 `account::find_account_for`），因此单测的假取数器
/// 无需任何文件系统或网络依赖。
pub trait CreditFetcher {
    /// 取单账号余额。
    fn fetch<'a>(
        &'a self,
        region: Region,
        uid: &'a str,
    ) -> Pin<Box<dyn Future<Output = Value> + Send + 'a>>;
}

/// 生产取数器：解析账号对象后调用 core 的真实余额接口。
pub struct CoreCreditFetcher;

impl CreditFetcher for CoreCreditFetcher {
    fn fetch<'a>(
        &'a self,
        region: Region,
        uid: &'a str,
    ) -> Pin<Box<dyn Future<Output = Value> + Send + 'a>> {
        Box::pin(async move {
            match account::find_account_for(region, uid) {
                Some(account) => credits::get_credit_expiry_for(region, &account).await,
                // 账号库已无该 uid：返回「未知」，绝不写假数据。
                None => json!({"ok": false, "error": "account not found"}),
            }
        })
    }
}

/// 刷新一次账号池余额；返回本次**真正刷新**（拿到有效读数并写入）的账号数。
///
/// 仅由后台周期循环调用（首次启动后先跑一次）。
///
/// ## ★ 为什么不能像原来那样「先取写锁、再 await 取数」
///
/// 改造前这里是：
///
/// ```ignore
/// let mut pool = state.pool.write().await;
/// refresh_with(&mut pool, &fetcher, now_ms, interval_ms).await
/// ```
///
/// `refresh_with` 内部要对每个待刷账号发一次**网络**请求 ⇒ **全局写锁被跨整个取数过程持有**。
/// 后果不是死锁而是**排队**：启动时所有账号都「从未取过余额」⇒ 第一批 relay 请求要等
/// 完整一轮取数（账号数 × 单次上游耗时）才能选到号；之后每 30 分钟再来一次。
/// 这与本模块开头那条硬约束（「绝不在请求路径上同步拉余额」）是**同一件事的另一面**：
/// 取数虽然不在请求路径上，但它**把请求路径堵住了**。
///
/// 现在分三段：读锁算目标 → **无锁**取数 → 短暂写锁写回。
pub async fn refresh_once(state: &GatewayState) -> usize {
    refresh_pool(&state.pool, &CoreCreditFetcher, timeutil::now_ms()).await
}

/// 刷新入口（按池句柄）：**三段式**，取数期间不持锁。
///
/// 与 [`refresh_with`] 共用 [`refresh_targets`] / [`apply_refresh`] / [`parse_credits`]，
/// 因此「测的语义」与「跑的语义」是**同一份代码**；两者只差锁的持有时机。
pub async fn refresh_pool<F: CreditFetcher>(
    pool: &std::sync::Arc<tokio::sync::RwLock<Pool>>,
    fetcher: &F,
    now_ms: i64,
) -> usize {
    // ① 读锁：只算出目标清单，随即释放（不跨 await）。
    let targets = {
        let guard = pool.read().await;
        refresh_targets(&guard, now_ms, guard.config().credits_refresh_interval_ms)
    };
    if targets.is_empty() {
        return 0;
    }

    // ② **不持锁**：逐个取数（网络）。
    let mut refreshed = 0usize;
    for (uid, region) in targets {
        let result = fetcher.fetch(region, &uid).await;
        // 未知（失败/回退）→ 绝不写 0，也绝不标记已刷新（下一轮重试）。
        let Some((credits, expiring)) = parse_credits(&result) else {
            continue;
        };
        // ③ 短暂写锁：只做内存写回。
        apply_refresh(&mut *pool.write().await, &uid, credits, expiring, now_ms);
        refreshed += 1;
    }
    refreshed
}

/// 本轮需要刷新的账号（**只读**池）：从未取过的一律刷新，否则超过阈值才刷新。
fn refresh_targets(pool: &Pool, now_ms: i64, interval_ms: i64) -> Vec<(String, Region)> {
    let interval = normalized_interval(interval_ms);
    pool.entries()
        .filter(|entry| needs_refresh(entry.credits_refreshed_ms, now_ms, interval))
        .filter_map(|entry| {
            entry
                .realm
                .map(|realm| (entry.uid.clone(), region_of(realm)))
        })
        .collect()
}

/// 把一次**有效**读数写回池（只碰内存，不做 IO）。
fn apply_refresh(pool: &mut Pool, uid: &str, credits: i64, expiring: i64, now_ms: i64) {
    pool.set_credits(uid, credits, expiring);
    pool.mark_credits_refreshed(uid, now_ms);
    // 余额恢复后自动解冻「因余额不足」的硬冷却账号（内部有 0 余额/未知/其它原因三重反面约束）。
    pool.thaw_hard_credit_if_recovered(uid, credits);
}

/// 刷新核心：对「从未取过余额」或「读数已过期」的账号取真实余额并写入池。
///
/// 规则：
/// - 判定 `credits_refreshed_ms == 0`（从未取过）或 `now - credits_refreshed_ms >= interval`
///   才刷新；否则跳过（不发起取数）。
/// - 池条目的 `realm` 为 `None` 时**跳过**——无从确定调用哪个域。
/// - 拿不到有效读数（接口失败 / 回退路径 / 无 `resources`）→ **不写**、不标记、不写 0。
///
/// 该签名（`&mut Pool` + 注入 `fetcher`）是单测的注入点；生产走 [`refresh_pool`]。
///
/// 两者共用 [`refresh_targets`] / [`apply_refresh`] / [`parse_credits`]，**只差锁的持有时机** ——
/// 因此这里测到的语义就是生产语义，不存在「测的路径不是跑的路径」。
pub async fn refresh_with<F: CreditFetcher>(
    pool: &mut Pool,
    fetcher: &F,
    now_ms: i64,
    interval_ms: i64,
) -> usize {
    let targets = refresh_targets(pool, now_ms, interval_ms);

    let mut refreshed = 0usize;
    for (uid, region) in targets {
        let result = fetcher.fetch(region, &uid).await;
        // 未知（失败/回退）→ 绝不写 0，也绝不标记已刷新（下一轮重试）。
        let Some((credits, expiring)) = parse_credits(&result) else {
            continue;
        };
        apply_refresh(pool, &uid, credits, expiring, now_ms);
        refreshed += 1;
    }
    refreshed
}

/// 归一刷新间隔：非正 → 缺省 30 分钟。
fn normalized_interval(interval_ms: i64) -> i64 {
    if interval_ms > 0 {
        interval_ms
    } else {
        DEFAULT_REFRESH_INTERVAL_MS
    }
}

/// 是否需要刷新：从未取过（0）**一定**刷新；否则超过阈值才刷新。
fn needs_refresh(credits_refreshed_ms: i64, now_ms: i64, interval_ms: i64) -> bool {
    credits_refreshed_ms <= 0 || now_ms.saturating_sub(credits_refreshed_ms) >= interval_ms
}

/// 池域标记 → core 的 [`Region`]。
fn region_of(realm: RealmTag) -> Region {
    match realm {
        RealmTag::Cn => Region::Cn,
        RealmTag::Global => Region::Global,
    }
}

/// 从 `credit_result` 返回体解析 `(剩余总额, 快过期子集)`。
///
/// 口径（对照 `credits.rs::credit_result`）：
/// - 剩余总额取返回对象的 `totalRemaining`；
/// - 快过期子集**自行聚合**：`resources[]` 中所有 `expiringSoon == true` 的资源的
///   `remaining` 之和。⚠️ `expiringSoon` 是**布尔标记**而非额度，把它当数量求和是错的；
/// - 只有当 `ok == true`、`totalRemaining` 与 `resources` 数组**同时存在**时才认为读数有效，
///   否则返回 `None`（调用方据此**不写**，避免把失败回退写成 0）。
fn parse_credits(result: &Value) -> Option<(i64, i64)> {
    if result.get("ok").and_then(Value::as_bool) != Some(true) {
        return None;
    }
    let total = result.get("totalRemaining").and_then(Value::as_f64)?;
    let resources = result.get("resources").and_then(Value::as_array)?;
    let expiring_soon: f64 = resources
        .iter()
        .filter(|resource| resource.get("expiringSoon").and_then(Value::as_bool) == Some(true))
        .filter_map(|resource| resource.get("remaining").and_then(Value::as_f64))
        .sum();
    Some((to_credits(total), to_credits(expiring_soon)))
}

/// f64 额度 → i64（四舍五入，非有限 / 负值钳 0）。
fn to_credits(value: f64) -> i64 {
    if !value.is_finite() || value <= 0.0 {
        0
    } else {
        (value + 0.5).floor() as i64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    use crate::pool::{CoolKind, PoolConfig, UpstreamEvent};

    const NOW: i64 = 1_700_000_000_000;
    const INTERVAL: i64 = 30 * 60 * 1000;

    /// 假取数器：返回固定响应并记录被调用次数（断言「跳过」用）。
    struct FakeFetcher {
        response: Value,
        calls: Arc<AtomicUsize>,
    }

    impl FakeFetcher {
        fn new(response: Value) -> Self {
            Self {
                response,
                calls: Arc::new(AtomicUsize::new(0)),
            }
        }

        fn calls(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }
    }

    impl CreditFetcher for FakeFetcher {
        fn fetch<'a>(
            &'a self,
            _region: Region,
            _uid: &'a str,
        ) -> Pin<Box<dyn Future<Output = Value> + Send + 'a>> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let response = self.response.clone();
            Box::pin(async move { response })
        }
    }

    fn ok_response(total: f64, resources: Value) -> Value {
        json!({ "ok": true, "totalRemaining": total, "resources": resources })
    }

    fn failed_response() -> Value {
        json!({ "ok": false, "error": "upstream failed" })
    }

    fn cn_pool(uid: &str) -> Pool {
        let mut pool = Pool::new(PoolConfig::default());
        pool.upsert(uid, Some(RealmTag::Cn), uid);
        pool
    }

    /// ★ 池为空时必须**短探测**：否则「刚导入账号 → 打开页面」会看到长达一整个
    /// 周期（默认 30 分钟）的「积分未知」——实测踩过（19:54 起服务，20:24 才有读数）。
    #[test]
    fn empty_pool_probes_short_instead_of_waiting_a_full_interval() {
        let empty = Pool::new(PoolConfig::default());
        assert_eq!(next_refresh_wait_ms(&empty), EMPTY_POOL_PROBE_MS);
        assert!(
            EMPTY_POOL_PROBE_MS < DEFAULT_REFRESH_INTERVAL_MS as u64,
            "探测周期必须远短于正常周期，否则等于没修"
        );

        let pool = cn_pool("u1");
        assert_eq!(
            next_refresh_wait_ms(&pool),
            pool.config().credits_refresh_interval_ms as u64,
            "非空池走配置周期（避免失败时高频打上游）"
        );
    }

    #[tokio::test]
    async fn first_refresh_fetches_and_writes_balance() {
        let mut pool = cn_pool("u1");
        assert_eq!(
            pool.get("u1").unwrap().credits_refreshed_ms,
            0,
            "初始应为「从未取过」"
        );
        let fetcher = FakeFetcher::new(ok_response(200.0, json!([])));

        let count = refresh_with(&mut pool, &fetcher, NOW, INTERVAL).await;

        assert_eq!(count, 1, "从未取过的账号必须被刷新");
        assert_eq!(fetcher.calls(), 1);
        let entry = pool.get("u1").unwrap();
        assert_eq!(entry.credits, 200);
        assert_eq!(entry.credits_expiring, 0);
        assert_eq!(entry.credits_refreshed_ms, NOW, "应记录最近刷新时刻");
    }

    #[tokio::test]
    async fn fresh_entry_is_skipped_within_interval() {
        let mut pool = cn_pool("u1");
        pool.set_credits("u1", 111, 0);
        pool.mark_credits_refreshed("u1", NOW - 60_000); // 1 分钟前刚刷过
        let fetcher = FakeFetcher::new(ok_response(999.0, json!([])));

        let count = refresh_with(&mut pool, &fetcher, NOW, INTERVAL).await;

        assert_eq!(count, 0, "阈值内不得再次刷新");
        assert_eq!(fetcher.calls(), 0, "不得发起取数");
        assert_eq!(pool.get("u1").unwrap().credits, 111, "未刷新不得改余额");
    }

    #[tokio::test]
    async fn stale_entry_is_refreshed_after_interval() {
        let mut pool = cn_pool("u1");
        pool.mark_credits_refreshed("u1", NOW - INTERVAL - 1);
        let fetcher = FakeFetcher::new(ok_response(50.0, json!([])));

        let count = refresh_with(&mut pool, &fetcher, NOW, INTERVAL).await;

        assert_eq!(count, 1, "超过阈值必须刷新");
        assert_eq!(fetcher.calls(), 1);
        assert_eq!(pool.get("u1").unwrap().credits, 50);
        assert_eq!(pool.get("u1").unwrap().credits_refreshed_ms, NOW);
    }

    #[tokio::test]
    async fn zero_interval_falls_back_to_default_threshold() {
        let mut pool = cn_pool("u1");
        pool.mark_credits_refreshed("u1", NOW - 60_000);
        let fetcher = FakeFetcher::new(ok_response(1.0, json!([])));

        // interval<=0 归一为缺省 30 分钟：1 分钟前的读数仍属「新鲜」，应跳过。
        let count = refresh_with(&mut pool, &fetcher, NOW, 0).await;
        assert_eq!(count, 0);
        assert_eq!(fetcher.calls(), 0);
    }

    #[tokio::test]
    async fn positive_balance_thaws_hard_credit_cooling() {
        let mut pool = cn_pool("u1");
        pool.apply_upstream_error("u1", "", &UpstreamEvent::HardCredit, NOW);
        let entry = pool.get("u1").unwrap();
        assert_eq!(entry.cool_kind, Some(CoolKind::Hard), "应先进入硬冷却");
        assert!(entry.until_ms > NOW);

        let fetcher = FakeFetcher::new(ok_response(500.0, json!([])));
        let count = refresh_with(&mut pool, &fetcher, NOW, INTERVAL).await;

        assert_eq!(count, 1);
        let entry = pool.get("u1").unwrap();
        assert_eq!(entry.cool_kind, None, "余额恢复后应自动解冻");
        assert_eq!(entry.until_ms, 0, "冷却截止应清零");
        assert!(
            entry.reason.contains("解冻"),
            "原因应写明自动解冻，实际：{}",
            entry.reason
        );
    }

    #[tokio::test]
    async fn zero_balance_does_not_thaw() {
        let mut pool = cn_pool("u1");
        pool.apply_upstream_error("u1", "", &UpstreamEvent::HardCredit, NOW);
        let until = pool.get("u1").unwrap().until_ms;
        let fetcher = FakeFetcher::new(ok_response(0.0, json!([])));

        let count = refresh_with(&mut pool, &fetcher, NOW, INTERVAL).await;

        assert_eq!(count, 1, "0 也是有效读数（写 0），但不得解冻");
        let entry = pool.get("u1").unwrap();
        assert_eq!(entry.credits, 0);
        assert_eq!(entry.cool_kind, Some(CoolKind::Hard), "0 余额不得解冻");
        assert_eq!(entry.until_ms, until, "冷却截止必须保持不变");
    }

    #[tokio::test]
    async fn unknown_balance_does_not_write_or_thaw() {
        let mut pool = cn_pool("u1");
        pool.set_credits("u1", 77, 0);
        pool.apply_upstream_error("u1", "", &UpstreamEvent::HardCredit, NOW);
        let fetcher = FakeFetcher::new(failed_response());

        let count = refresh_with(&mut pool, &fetcher, NOW, INTERVAL).await;

        assert_eq!(count, 0, "取数失败不计入刷新数");
        let entry = pool.get("u1").unwrap();
        assert_eq!(entry.credits, 77, "未知不得把余额写成 0");
        assert_eq!(
            entry.credits_refreshed_ms, 0,
            "未知不标记已刷新（下轮重试）"
        );
        assert_eq!(entry.cool_kind, Some(CoolKind::Hard), "未知不得解冻");
    }

    #[tokio::test]
    async fn thaw_does_not_touch_breaker_domain() {
        let mut pool = cn_pool("u1");
        for _ in 0..3 {
            pool.apply_upstream_error("u1", "", &UpstreamEvent::Server, NOW);
        }
        let breaker = pool.get("u1").unwrap().breaker_until_ms;
        let retry = pool.get("u1").unwrap().retry_count;
        assert!(breaker > NOW, "应先触发熔断");
        pool.apply_upstream_error("u1", "", &UpstreamEvent::HardCredit, NOW);

        let fetcher = FakeFetcher::new(ok_response(300.0, json!([])));
        refresh_with(&mut pool, &fetcher, NOW, INTERVAL).await;

        let entry = pool.get("u1").unwrap();
        assert_eq!(entry.cool_kind, None, "应先解冻");
        assert_eq!(entry.breaker_until_ms, breaker, "熔断域不得被解冻影响");
        assert_eq!(entry.retry_count, retry, "退避指数不得被重置");
    }

    #[tokio::test]
    async fn expiring_subset_sums_only_remaining_of_expiring_resources() {
        let mut pool = cn_pool("u1");
        let resources = json!([
            { "remaining": 80.0, "expiringSoon": true, "expireAt": 1 },
            { "remaining": 20.0, "expiringSoon": false, "expireAt": 2 },
            { "remaining": 15.0, "expiringSoon": true, "expireAt": 3 },
        ]);
        let fetcher = FakeFetcher::new(ok_response(115.0, resources));

        refresh_with(&mut pool, &fetcher, NOW, INTERVAL).await;

        let entry = pool.get("u1").unwrap();
        assert_eq!(entry.credits, 115);
        assert_eq!(
            entry.credits_expiring, 95,
            "只累加 expiringSoon==true 的 remaining（80+15）；布尔项不得被当额度"
        );
    }

    #[tokio::test]
    async fn entry_without_realm_is_skipped() {
        let mut pool = Pool::new(PoolConfig::default());
        pool.upsert("u1", None, "无名域");
        let fetcher = FakeFetcher::new(ok_response(100.0, json!([])));

        let count = refresh_with(&mut pool, &fetcher, NOW, INTERVAL).await;

        assert_eq!(count, 0, "realm 为 None 无从确定域，必须跳过");
        assert_eq!(fetcher.calls(), 0);
    }

    #[test]
    fn parse_credits_requires_ok_and_resources() {
        assert_eq!(parse_credits(&failed_response()), None, "ok=false → 未知");
        assert_eq!(
            parse_credits(&json!({"ok": true})),
            None,
            "缺 totalRemaining → 未知"
        );
        assert_eq!(
            parse_credits(&json!({"ok": true, "totalRemaining": 5.0})),
            None,
            "缺 resources 数组 → 未知（不把回退当成 0）"
        );
        assert_eq!(
            parse_credits(&json!({"ok": true, "totalRemaining": 5.0, "resources": []})),
            Some((5, 0))
        );
    }

    /// ★ 护栏：**取数期间不得持有池锁**。
    ///
    /// ## 回归背景
    ///
    /// 改造前 `refresh_once` 是「先 `state.pool.write().await` 取全局写锁，再把 guard
    /// 交给 `refresh_with` 逐账号 `await` 网络取数」⇒ **写锁跨整个取数过程持有**。
    /// 后果不是死锁而是**排队**：启动时所有账号都「从未取过余额」，第一批 relay 请求
    /// 必须等完整一轮取数（账号数 × 单次上游耗时）才能选到号。
    ///
    /// ## 判据：**直接观测**，不靠计时
    ///
    /// 注入的 fetcher 在 `fetch` 被调用时（即「取数期间」）对同一把锁 `try_read()`。
    /// 拿不到 ⇒ 此刻锁被占着。计时而断会随机器快慢抖动，这个不会。
    ///
    /// 反面对照：把 `refresh_pool` 改回「先取写锁再 `refresh_with`」，本用例立刻变红。
    #[tokio::test]
    async fn refresh_pool_does_not_hold_the_lock_across_fetches() {
        /// 取数时探测锁：记录「有几次拿不到读锁」。
        struct LockProbeFetcher {
            pool: Arc<tokio::sync::RwLock<Pool>>,
            observed_locked: Arc<AtomicUsize>,
        }

        impl CreditFetcher for LockProbeFetcher {
            fn fetch<'a>(
                &'a self,
                _region: Region,
                _uid: &'a str,
            ) -> Pin<Box<dyn Future<Output = Value> + Send + 'a>> {
                // 只在「锁被写者占着」时 `try_read` 才失败 —— 这正是要观测的。
                if self.pool.try_read().is_err() {
                    self.observed_locked.fetch_add(1, Ordering::SeqCst);
                }
                let response = ok_response(100.0, json!([]));
                Box::pin(async move { response })
            }
        }

        let pool = Arc::new(tokio::sync::RwLock::new(cn_pool("u1")));
        let probe = LockProbeFetcher {
            pool: Arc::clone(&pool),
            observed_locked: Arc::new(AtomicUsize::new(0)),
        };

        let refreshed = refresh_pool(&pool, &probe, NOW).await;

        assert_eq!(refreshed, 1, "有效读数必须写回");
        assert_eq!(
            probe.observed_locked.load(Ordering::SeqCst),
            0,
            "取数期间不得持有池锁：改造前写锁跨整个 await 持有，本断言必红"
        );
        assert_eq!(pool.read().await.get("u1").unwrap().credits, 100);
    }

    /// `refresh_pool` 与 `refresh_with` 必须产出**相同结果**（同一份语义）。
    ///
    /// 前者是生产路径（三段式、不持锁），后者是单测注入点；若有人日后只改一处，
    /// 本用例会红 —— 防「测的路径不是跑的路径」。
    #[tokio::test]
    async fn refresh_pool_and_refresh_with_agree() {
        let handle = Arc::new(tokio::sync::RwLock::new(cn_pool("u1")));
        let direct = {
            let mut pool = cn_pool("u1");
            refresh_with(
                &mut pool,
                &FakeFetcher::new(ok_response(77.0, json!([]))),
                NOW,
                INTERVAL,
            )
            .await;
            (
                pool.get("u1").unwrap().credits,
                pool.get("u1").unwrap().credits_refreshed_ms,
            )
        };

        let via_pool = refresh_pool(
            &handle,
            &FakeFetcher::new(ok_response(77.0, json!([]))),
            NOW,
        )
        .await;

        let guard = handle.read().await;
        assert_eq!(via_pool, 1);
        assert_eq!(
            (
                guard.get("u1").unwrap().credits,
                guard.get("u1").unwrap().credits_refreshed_ms
            ),
            direct,
            "两条路径必须产出完全相同的池状态"
        );
    }
}
