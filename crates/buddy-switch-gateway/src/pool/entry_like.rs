//! 选号内核所需的账号视图 —— **跨产品线契约**。
//!
//! ## 为什么需要它
//!
//! [`crate::pool::pick`] 的选号算法（四因子加权 → 成本分层 → 短名单 → 防惊群 → LRU）
//! 本身与产品线无关，但历史上它直接操作 [`crate::pool::entry::PoolEntry`] ——
//! 那是 WorkBuddy 专用的条目，带 region 分区、实测成本账本、熔断退避等字段。
//!
//! Trae 侧因此只能另写一套极简选号（[`crate::trae::pool`] 的「到期最近优先」），
//! 于是「同一个概念两套实现」，能力无法互通。
//!
//! 本 trait 把「选号到底需要什么」抽成契约：算法只依赖这里的方法，
//! 各产品线提供自己的条目实现。WorkBuddy 用 [`PoolEntry`]（字段齐全），
//! Trae 用 [`crate::trae::pool::TraePoolEntry`]（缺的走中性降级）。
//!
//! ## 时间单位契约
//!
//! **本 trait 全部时间入参一律是「毫秒」**，与 [`crate::pool::entry::PoolEntry`] 一致。
//! 若某产品线的内部状态用的是秒（Trae 的冷却与积分到期时间就是秒），
//! **必须由该产品线的实现内部换算**，不得把秒直接当成毫秒传进算法 ——
//! 混用不会报错，只会让冷却判定与紧迫度计算静默错位。
//!
//! ## 中性降级的硬约束
//!
//! 实现方**必须**让「拿不到的数据」表现得像「没有证据」，而不是像「证据指向坏结果」：
//!
//! - 无成功率记录 ⇒ `success_ema` / `error_ema` 都返回 `0.0`
//!   （算法据此给中性权重 `1.5`；若返回 `error_ema = 1.0`，该账号会被永久压制）。
//! - 无成本观测 ⇒ [`CostTier::Unknown`]、`cost_per_1k` 返回 `0.0`
//!   （`Unknown` 在分层中排在 `Free` 之后、`Paid` 之前，保留被观测的机会）。
//! - 无在途统计 ⇒ `in_flight` 返回 `0`（不触发在途限流）。
//!
//! 违反这条约束不会报错，只会让某个账号**静默地永远选不上**。
//!
//! [`PoolEntry`]: crate::pool::entry::PoolEntry

use crate::pool::entry::CostTier;
use crate::pool::RealmTag;

/// 选号算法所需的账号视图。
///
/// 只暴露 [`crate::pool::pick`] 真正读取的字段与行为；实现方不得在方法里做
/// 与选号无关的副作用（算法会在同一把锁窗口内对全部候选调用它们）。
pub trait PoolEntryLike {
    /// 账号 uid（池键）。
    fn uid(&self) -> &str;

    /// 可用积分总量。
    fn credits(&self) -> i64;

    /// 「此刻被认为快过期」的积分**绝对值**。
    ///
    /// 算法内部会用它除以 [`PoolEntryLike::credits`] 得到紧迫占比，再乘
    /// [`crate::pool::PickPolicy::expiring_weight`]。
    ///
    /// ★ 需要 `now_ms` 是因为「紧迫」本身随时间变化：WorkBuddy 直接返回余额刷新时
    /// 算出的字段（与时刻无关），而 Trae 没有「分批到期」概念，必须按**剩余时间**
    /// 现算 —— 见 [`crate::trae::pool::TraePoolEntry`] 的实现说明。
    ///
    /// 返回 `0` 表示「不紧迫」，会让该项权重归零（而不是给负分）。
    ///
    /// ★ 与 [`PoolEntryLike::expiry_urgency`] 的分工：本方法表达「**已经**快过期的
    /// 那部分积分有多少」（需要产品线能分批给出绝对值），后者表达「**还有多久**到期」
    /// （只需一个 `[0,1]` 的紧迫度）。两者互补，不要用一个去模拟另一个 ——
    /// 实测过：把「剩余时间」硬塞进本方法会被 `credits` 归一化抹平，紧迫度差异
    /// 被积分差异淹没，语义**静默丢失**。
    fn expiring_credits(&self, now_ms: i64) -> i64;

    /// 到期紧迫度 ∈ `[0,1]`：`0` = 不紧迫（无到期信息 / 远未到期），`1` = 即将到期。
    ///
    /// 默认 `0.0`（不启用）。只有具备「积分到期时间」概念的产品线才需要覆写它，
    /// 并在自己的 [`crate::pool::PickPolicy`] 里把
    /// [`expiry_urgency_weight`](crate::pool::PickPolicy::expiry_urgency_weight)
    /// 设为非零值 —— 否则覆写了也不会生效。
    ///
    /// 实现方必须自己保证单调性（剩余时间越短，返回值越大），算法不做校验。
    fn expiry_urgency(&self, _now_ms: i64) -> f64 {
        0.0
    }

    /// 成功率 EMA（α=0.1）。无记录返回 `0.0`。
    fn success_ema(&self) -> f64;

    /// 错误率 EMA（α=0.1）。无记录返回 `0.0`。
    fn error_ema(&self) -> f64;

    /// 最近被选中时刻（毫秒；`0` = 从未）。
    fn last_used_ms(&self) -> i64;

    /// 选中单调序号（LRU 权威依据）。
    fn used_seq(&self) -> u64;

    /// 当前在途请求数。无统计返回 `0`。
    fn in_flight(&self) -> i64;

    /// **该模型维度**的 `(success_ema, error_ema)`；`None` = 证据不足（回落到账号级）。
    ///
    /// 默认 `None` ⇒ 该产品线不做分模型评价（行为与改造前逐字相同）。
    /// WorkBuddy 侧覆写成「读 `model_health`」，避免「在这个模型上老失败」把账号
    /// 在**其它**模型上的权重也拖下去（错杀）。
    fn model_ema(&self, _model: &str) -> Option<(f64, f64)> {
        None
    }

    /// **自愈版**在途读数（默认 = [`Self::in_flight`]，即「不做租约自愈」）。
    ///
    /// 为什么给默认实现：Qoder / Trae 池不做并发租约（恒 0），不必为它们写空实现；
    /// WorkBuddy 池有真实租约，覆写成「到期即失效」。
    fn in_flight_now(&self, _now_ms: i64) -> i64 {
        self.in_flight()
    }

    /// 归属域标记；`None` 表示未知，不参与 realm 过滤。
    fn realm(&self) -> Option<RealmTag>;

    /// 是否已禁用（终态，需人工或刷新成功复活）。
    fn is_disabled(&self) -> bool;

    /// 冷却 / 熔断的**解冻时刻**（毫秒；`0` = 无）。
    ///
    /// ★ 这是「什么时候能再用」，**不是**「积分什么时候过期」。
    /// 全冷却兜底（[`crate::pool::pick`] 的 `pick_earliest_expiry`）据此挑最早解冻者。
    fn cool_expiry_ms(&self) -> i64;

    /// 是否正处于**硬冷却**中（余额不足 ⇒ 强试必然再失败且浪费一次上游调用）。
    ///
    /// 全冷却兜底会排除这类账号；软冷却不排除（值得再试一次）。
    fn is_hard_cooled(&self, now_ms: i64) -> bool;

    /// **该模型维度**的冷却解冻时刻（毫秒；`0` = 该模型没有冷却）。
    ///
    /// ★ issue #106 §2.3：全冷却兜底（[`crate::pool::pick`] 的 `pick_earliest_expiry`）
    /// 原本只看账号级冷却（[`Self::cool_expiry_ms`]）。于是「池里唯一的问题就是**这个
    /// 模型**被限流」时兜底一个号都给不出来，落到 `last == None` ⇒ 对外 401 ——
    /// 与闸门短路那条并列的第二条「19ms 空账号 401」路径。
    ///
    /// 默认 `0`（该产品线不做模型级冷却）。WorkBuddy 覆写成读 `model_cooldowns`。
    fn model_cool_expiry_ms(&self, _model: &str) -> i64 {
        0
    }

    /// 该账号此刻是否可用于这个模型（账号级可用 + 模型级冷却）。
    ///
    /// ★ 这是可用性的**唯一判据**：候选过滤与偏好短路都必须走它。
    /// 两处若分家，偏好就会把请求钉到一个算法认为不可用的账号上 ——
    /// 症状是「明明有别的号可用却一直失败」，且**不报错**。
    fn healthy_for_request(&self, now_ms: i64, model: &str) -> bool;

    /// **账号级**健康 —— 刻意**不看**模型级冷却（issue #134）。
    ///
    /// 用途只有一个：选号失败时判断「是不是**全部**账号都只是被这个模型卡住」。
    /// 那时必须把「该模型的冷却」从判据里摘掉再问一遍「这个号本来能不能用」，
    /// 否则问出来的永远是「不能用」，判据恒成立。
    ///
    /// 默认实现走 [`Self::healthy_for_request`] 的空模型分支：三个产品线的实现里
    /// `model` 为空都等价于「只看账号维度」（WB 的 `healthy` / Qoder 不查
    /// `model_cooldowns[""]` / Trae 本就忽略模型）。
    fn account_healthy(&self, now_ms: i64) -> bool {
        self.healthy_for_request(now_ms, "")
    }

    /// 惰性清理已过期的模型级冷却。无模型级冷却的实现可为空操作。
    fn prune_model_cooldowns(&mut self, now_ms: i64);

    /// 当前有效实测单价（无观测 / 观测过期返回 `0.0`）。
    fn cost_per_1k(&self, now_ms: i64, model: &str, ttl_ms: i64) -> f64;

    /// 实测成本分层（硬过滤用）。无观测返回 [`CostTier::Unknown`]。
    fn cost_tier(&self, now_ms: i64, model: &str, ttl_ms: i64) -> CostTier;

    /// 选号写回：记录「被正式选中」的时刻与序号。
    ///
    /// 同时推进 `used_seq`（LRU 权威依据）。
    fn mark_picked(&mut self, now_ms: i64, seq: u64);

    /// 兜底强试写回：只更新「最近使用时刻」，**不推进** `used_seq`。
    ///
    /// 用于全冷却兜底 —— 那只是「试一下」，不应影响 LRU 语义。
    fn mark_probed(&mut self, now_ms: i64);
}
