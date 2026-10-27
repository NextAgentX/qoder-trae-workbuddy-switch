//! 账号池：多账号共享、选号、失败治理与状态持久化。
//!
//! 移植自参考实现 `internal/pool/`。与既有 [`crate::account_strategy`] 的关系：
//! 策略模块解决「**用哪个账号**」（current / pinned / max_credits 这类单点决策），
//! 本模块解决「**多账号如何协同**」——失败换号、冷却熔断、在途限流、状态落盘。
//! 二者互补：策略先给出首选账号，池负责在其不可用时接管。
//!
//! 治理语义（优先级从高到低）：
//! 1. **禁用**（终态，需刷新成功或人工复活）；
//! 2. **账号级冷却**（`until`）——硬冷却至次日 04:00（余额不足）/ 软冷却退避（限流）；
//! 3. **熔断**（`breaker_until`）——连续失败达阈值后指数退避；
//! 4. **模型级冷却**（`model_cooldowns`）——只封锁触发限流的那个模型。

pub mod entry;
pub mod entry_like;
pub mod exclusions;
pub mod gate;
pub mod pick;

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

// ★ issue #108 §P0-B：会话失效标记**从 core 复用，不在这里另写一份** ——
// Trae 侧此前各自没有这份表，于是「真正的会话作废」与「access token 过期」
// 只能共用同一个判据（裸 401）。两侧同源是「改一处、两边生效」的前提。
use buddy_switch_core::modules::upstream::SESSION_DEAD_MARKERS;
use chrono::{NaiveDate, TimeZone};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::sync::RwLock;

pub use entry::{CoolKind, CostTier, ModelCooldown, ModelCost, PoolEntry};
pub use entry_like::PoolEntryLike;
pub use gate::{GateBlock, GatePhase, ModelGate, ModelGates};
pub use pick::{
    no_exclusions, parse_preferred, pick, pick_with_preference, weight_of, weight_of_model,
    PickInput, PickMode, PickPolicy, PREFERRED_AUTO, PREFERRED_MAX_CREDITS, PREFERRED_RANDOM,
    SHORTLIST_SIZE,
};

/// 池内账号的归属域标记（与 core 的 `Region` 解耦，避免循环依赖）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RealmTag {
    /// 国内版。
    Cn,
    /// 国际版。
    Global,
}

/// 域标记 → 池视图里的字符串键（`None` 为 `unknown`）。
pub fn realm_key(realm: Option<RealmTag>) -> &'static str {
    match realm {
        Some(RealmTag::Cn) => "cn",
        Some(RealmTag::Global) => "global",
        None => "unknown",
    }
}

/// 池状态摘要（**两侧共用的五态计数**）。
///
/// WorkBuddy 与 Trae 的管理面「账号池」卡读同一形状，因此**字段名即契约** ——
/// 改动必须同步两侧（前端卡片与后端两侧各一份测试都盯着它）。
///
/// 五态互斥且穷尽，判定顺序见 [`PoolEntry::status_of`]。
///
/// ⚠️ `expired` 在 WorkBuddy 侧**恒为 0**：池条目不存积分有效期（只有「快过期的
/// 积分**数量**」），没有判据 ⇒ 不臆造数字。前端据此**不渲染**该格（见共用卡的 `tiles`），
/// 避免出现一个永远为 0 的假格子。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PoolSummary {
    pub total: usize,
    pub available: usize,
    pub cooling: usize,
    pub disabled: usize,
    pub expired: usize,
    pub zero_credits: usize,
    /// **用户显式排除**的账号数（2026-10-17 新增，互斥第六态）。
    ///
    /// ## 为什么它是「态」而不是「正交标记」
    ///
    /// 正交标记（如 `model_cooling` / `in_flight_full`）不改变「可用与否」；
    /// 而排除**直接决定能不能被选中**（硬约束）。若把它做成正交计数、仍留在 `available` 里，
    /// 卡片就会说「4 可用」，而实际只有 3 个能被选中 —— **数字与事实不符**。
    /// 因此它必须从 `available` 里扣掉，既然扣掉了就已经是互斥态。
    ///
    /// 不变量：`available + cooling + disabled + expired + zero_credits + excluded == total`。
    pub excluded: usize,
    /// **状态可用、但在途已满**的账号数（**正交计数，已计入** `available`，2026-10-17 新增）。
    ///
    /// 为什么单列而不做第七个互斥态：账号本身是好的（凭据、余额、冷却、排除都没问题），
    /// 只是**这一瞬间**并发占满，几秒后自己就没了 —— 把它从 `available` 扣掉会让
    /// 「可用数」随并发抖动，还会破坏上面的六态不变量。
    ///
    /// 但缺了它，又会出现和 `excluded` 一模一样的自相矛盾：「4 个号、0 禁用、0 冷却、
    /// 0 零余额、0 被排除，却 0 个可用」。判据与选号**同源**（`pick::in_flight_full`）。
    ///
    /// 不变量：`in_flight_full <= available`。
    pub in_flight_full: usize,
    /// **状态可用、但有模型级冷却**的账号数（**正交计数，已计入** `available`，issue #88 新增）。
    ///
    /// 与 `in_flight_full` 同一档：账号本身没坏（凭据、余额、账号级冷却都没问题），
    /// 只是「某个模型」被上游 429 罚了 —— 换个模型就能用。因此**不能**从 `available`
    /// 里扣掉（扣掉会让人把「换个模型就好」误判成「这个号废了」，也与
    /// [`PoolEntry::status_of`] 不读 `model_cooldowns` 的领域建模冲突）。
    ///
    /// 但缺了它，池摘要就**一个数字都不反映模型级冷却** —— 探测判出限流、池卡全绿，
    /// 用户只能怀疑「探测没生效」（issue #88 的 2.6）。Qoder 侧早有这一格，属实现不对称。
    ///
    /// 不变量：`model_cooling <= available`。
    pub model_cooling: usize,
    pub total_credits: f64,
}

/// 一条**用户排除**记录：`uid → (归属 scope, 备注)`。
///
/// scope 只用于「按作用域重建」（`wb:cn` / `trae:work` / `qoder:cn` …）与写盘，
/// 选号只按 uid 判。uid 在账号库里本身就是唯一的，跨作用域重名不存在。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PoolExclusion {
    /// 归属作用域键（`wb:cn` / `trae:work` / …）。
    pub scope: String,
    /// 备注（为什么排除）。空串 = 未填。
    pub note: String,
}

/// 池治理配置。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct PoolConfig {
    /// 单账号在途上限（0 = 不限）。
    pub max_in_flight: i64,
    /// 国际版账号在途上限（<=0 归一为 2，**无「不限」语义**）。
    pub max_in_flight_global: i64,
    /// 连续失败熔断阈值（<=0 归一为 3）。
    pub breaker_threshold: i32,
    /// 熔断基准退避（毫秒，默认 30m）。
    pub breaker_cooldown_ms: i64,
    /// 熔断退避封顶（毫秒，默认 6h）。
    pub breaker_cooldown_max_ms: i64,
    /// 闲置补偿：每小时权重。
    pub idle_weight_per_hour: f64,
    /// 闲置补偿权重上限。
    pub idle_weight_max: f64,
    /// 429 软冷却基准时长（毫秒，默认 600s）。
    pub soft_rate_ms: i64,
    /// 软冷却封顶（毫秒，默认 2h）。
    pub soft_rate_max_ms: i64,
    /// 快过期积分权重。
    pub expiring_weight: f64,
    /// 「到期紧迫度」独立因子的权重。
    ///
    /// 默认 `0.0` = 不启用 —— WorkBuddy 的紧迫度已由 `expiring_weight` 表达，
    /// 引入该字段前后它的选号结果逐字一致。需要「按积分到期时间择优」的产品线
    /// （如 Trae）在自己的策略里覆写，见 [`crate::pool::PickPolicy::expiry_urgency_weight`]。
    pub expiry_urgency_weight: f64,
    /// 是否走严格择优（跳过加权抽签）。
    ///
    /// 默认 `false`（有界随机，WorkBuddy 既有语义）。见
    /// [`crate::pool::PickPolicy::deterministic`]。
    pub deterministic: bool,
    /// 防惊群间隔（毫秒，默认 100）。
    pub min_pick_gap_ms: i64,
    /// 成本账本有效期（毫秒，默认 6h）。
    pub model_cost_ttl_ms: i64,
    /// 会话失效连续次数阈值（默认 3）。
    pub session_dead_threshold: i32,
    /// 余额刷新新鲜度阈值（毫秒，默认 30 分钟；`<= 0` 归一为该默认值）。
    ///
    /// 由后台余额刷新循环使用：池条目 `credits_refreshed_ms == 0`（从未取过）一定刷新，
    /// 否则 `now - credits_refreshed_ms >= 本值` 才刷新。见 [`crate::credits_refresh`]。
    pub credits_refresh_interval_ms: i64,
    /// 模型级冷却的**下限**（毫秒，默认 60s）。
    ///
    /// ## 为什么必须有下限（2026-10-17 · 本 bug 的第一根因）
    ///
    /// 上游 `6004` 的响应体通常**不带**结构化墙钟，而文案里的 `reset at <T>` 实测是
    /// **配额窗口的重置时刻**、不是解锁时刻（`client_limits` 实测 6/6 判错）。
    /// 旧实现两条路都错：没墙钟 ⇒ `now + 1ms`（等于不冷却，同一个号每次请求都被优先选中）；
    /// 采信文案 ⇒ 冷 2 小时（早就恢复了却一直不用它）。
    ///
    /// 现在的口径：**只信结构化字段**，没有就给这个下限；由「该模型下一次成功」提前清掉
    /// （见 [`PoolEntry::note_success`]）。成功是最强的反证。
    pub min_model_cooldown_ms: i64,
    /// 账号级 429 的**上限**（毫秒，默认 15 分钟）。
    ///
    /// 与模型级同一理由：配额窗口 ≠ 解锁时刻。给 2 小时会让一个十几分钟就恢复的号
    /// 长时间不可用（实测过）；给太短又会把真限流当没发生。
    pub rate_window_reset_max_ms: i64,
    /// 探测回填的**兜底冷却时长**（毫秒，默认 10 分钟）—— issue #88。
    ///
    /// 上游没给任何可用墙钟时用它（见 [`PROBE_FALLBACK_COOLDOWN_MS`]）。
    /// **刻意与 [`Self::min_model_cooldown_ms`]（真实流量）分开配置**：探测是用户
    /// 主动发起的一次性动作，冷多久应当可以单独调，不能顺手改到真实流量那条链路。
    /// `<= 0` 归一为 [`PROBE_FALLBACK_COOLDOWN_MS`]。
    pub probe_fallback_ms: i64,
    /// 在途满时的**排队等待**上限（毫秒，默认 2000；`0` = 不排队、立刻换号）。
    ///
    /// 为什么要有它：`acquire` 失败就换号会把「此刻并发占满」误报成「这个号不可用」——
    /// 客户端并行发 tool calls 时，用户看到的是「网关拒绝服务」，而账号其实只是忙。
    pub acquire_timeout_ms: i64,
    /// 模型闸门：同一模型在多少个账号上失败即判定为**模型级问题**（默认 2）。
    ///
    /// 低于 2 没有意义（单个账号失败只是那个号的问题）；高于 3 会让前几次请求白撞。
    pub gate_threshold: i32,
    /// 模型闸门的观察窗口（毫秒，默认 10 分钟）。窗口外的失败不累计。
    pub gate_window_ms: i64,
    /// 模型闸门的基准暂停时长（毫秒，默认 60s）；触发次数每多一轮翻倍。
    pub gate_base_ms: i64,
    /// 模型闸门的暂停上限（毫秒，默认 15 分钟）。
    pub gate_max_ms: i64,
}

impl Default for PoolConfig {
    fn default() -> Self {
        Self {
            max_in_flight: 3,
            max_in_flight_global: 2,
            breaker_threshold: 3,
            breaker_cooldown_ms: 30 * 60 * 1000,
            breaker_cooldown_max_ms: 6 * 60 * 60 * 1000,
            idle_weight_per_hour: 0.5,
            idle_weight_max: 5.0,
            soft_rate_ms: 600 * 1000,
            soft_rate_max_ms: entry::DEFAULT_SOFT_RATE_MAX_MS,
            expiring_weight: 8.0,
            expiry_urgency_weight: 0.0,
            deterministic: false,
            min_pick_gap_ms: 100,
            model_cost_ttl_ms: 6 * 60 * 60 * 1000,
            session_dead_threshold: 3,
            credits_refresh_interval_ms: 30 * 60 * 1000,
            min_model_cooldown_ms: MIN_MODEL_COOLDOWN_MS,
            rate_window_reset_max_ms: 15 * 60 * 1000,
            probe_fallback_ms: PROBE_FALLBACK_COOLDOWN_MS,
            acquire_timeout_ms: 2000,
            gate_threshold: 2,
            gate_window_ms: 10 * 60 * 1000,
            gate_base_ms: 60 * 1000,
            gate_max_ms: 15 * 60 * 1000,
        }
    }
}

impl PoolConfig {
    /// 归一化非法值（对照参考实现 `normalize`）。
    ///
    /// 注意 `max_in_flight_global` 与 `max_in_flight` 的 0 语义**不同**：
    /// 前者 0 会被改写为 2（因为「0 = 不限」对国际版是危险的默认），
    /// 后者 0 保留「不限」语义。
    pub fn normalized(mut self) -> Self {
        if self.max_in_flight < 0 {
            self.max_in_flight = 0;
        }
        if self.max_in_flight_global <= 0 {
            self.max_in_flight_global = 2;
        }
        if self.breaker_threshold <= 0 {
            self.breaker_threshold = 3;
        }
        if self.breaker_cooldown_ms <= 0 {
            self.breaker_cooldown_ms = 30 * 60 * 1000;
        }
        if self.breaker_cooldown_max_ms <= 0 {
            self.breaker_cooldown_max_ms = 6 * 60 * 60 * 1000;
        }
        if self.idle_weight_per_hour <= 0.0 {
            self.idle_weight_per_hour = 0.5;
        }
        if self.idle_weight_max <= 0.0 {
            self.idle_weight_max = 5.0;
        }
        if self.soft_rate_ms <= 0 {
            self.soft_rate_ms = 600 * 1000;
        }
        if self.soft_rate_max_ms <= 0 {
            self.soft_rate_max_ms = entry::DEFAULT_SOFT_RATE_MAX_MS;
        }
        if self.expiring_weight <= 0.0 {
            self.expiring_weight = 8.0;
        }
        if self.min_pick_gap_ms < 0 {
            self.min_pick_gap_ms = 100;
        }
        if self.model_cost_ttl_ms <= 0 {
            self.model_cost_ttl_ms = 6 * 60 * 60 * 1000;
        }
        if self.session_dead_threshold <= 0 {
            self.session_dead_threshold = 3;
        }
        if self.credits_refresh_interval_ms <= 0 {
            self.credits_refresh_interval_ms = 30 * 60 * 1000;
        }
        // ★ 下限不能被配成 0：那等于回到「冷 1ms = 没冷却」的旧缺陷。
        if self.min_model_cooldown_ms <= 0 {
            self.min_model_cooldown_ms = MIN_MODEL_COOLDOWN_MS;
        }
        if self.rate_window_reset_max_ms <= 0 {
            self.rate_window_reset_max_ms = 15 * 60 * 1000;
        }
        // ★ 兜底时长同样不能配成 0：那会让探测回填退化成「冷 1ms = 没冷却」（issue #88）。
        if self.probe_fallback_ms <= 0 {
            self.probe_fallback_ms = PROBE_FALLBACK_COOLDOWN_MS;
        }
        // `acquire_timeout_ms` 允许 0（= 不排队），负值归 0。
        if self.acquire_timeout_ms < 0 {
            self.acquire_timeout_ms = 0;
        }
        if self.gate_threshold < 2 {
            self.gate_threshold = 2;
        }
        if self.gate_window_ms <= 0 {
            self.gate_window_ms = 10 * 60 * 1000;
        }
        if self.gate_base_ms <= 0 {
            self.gate_base_ms = 60 * 1000;
        }
        if self.gate_max_ms < self.gate_base_ms {
            self.gate_max_ms = 15 * 60 * 1000;
        }
        self
    }
}

/// 上游失败事件（由 status + body 分类得到）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpstreamEvent {
    /// 402 或余额不足标记 → 硬冷却至次日 04:00。
    HardCredit,
    /// 429 限流。
    SoftRate {
        /// 上游声明的重置时刻（毫秒）。
        reset_at_ms: Option<i64>,
        /// `true` 表示该限流只针对触发调用的模型（6004 / `IsModelRateLimit`）。
        model_scoped: bool,
    },
    /// 模型被上游**封禁**（`11102`）。
    ///
    /// 与 [`Self::SoftRate`] 的模型级限流**语义不同**：限流的解冻时刻由上游墙钟决定
    /// （封顶 `soft_rate_max`），封禁则按「这是第几次被罚」逐次加倍退避
    /// （6h → 12h → 24h，封顶 24h）。混用会让被封禁的模型过早重试。
    ModelBlocked,
    /// 404 → 固定浅冷却。
    NotFound,
    /// 5xx → 计入熔断。
    Server,
    /// 会话失效（12153）。
    SessionDead,
    /// 上游 401 / 403（**无** `12153` 这类会话失效标记）：凭据被拒（issue #100）。
    ///
    /// ## 为什么不能归到 [`Self::Client`]
    ///
    /// 调用方自己的 Key 早就过了网关的鉴权，能走到上游说明「这一把 Key 是好的」——
    /// 那么上游回 401/403 只可能是**该账号的凭据**（过期 / 被作废 / 签名不符）。
    /// 旧实现把它当成「调用方错误」，后果是一整串：
    ///
    /// 1. `should_rotate` 返回 `false` ⇒ **一个请求都不换号**，池里明明有健康账号，
    ///    客户端却直接吃到 401（用户原话：「之前旧的登录状态直接报 401」）；
    /// 2. `apply_upstream_error` 不冷却、不计数 ⇒ 池卡继续把死号显示成「可路由」，
    ///    下一次请求还会选中它；
    /// 3. `probe_state_of` 落 `UpstreamError` ⇒ 探测结论也说不清是「凭据坏了」。
    ///
    /// ## 为什么**不**直接当 [`Self::SessionDead`]
    ///
    /// 那个分支是「三振**永久**禁用」（`disabled` 是终态，只有人工「恢复」能解），
    /// 而 401/403 的绝大多数成因是**access token 过期**——保活刷新一轮就好。
    /// 把可自愈的问题打成终态，用户看到的是「号没坏却再也用不了」。
    /// ⇒ 本档只做**短冷却 + 换号**，让保活刷新后自然恢复；真正被作废的会话
    /// 会带 `12153`，那条路仍然走三振禁用。
    AuthRejected,
    /// 其它 4xx → 仅记错，不做治理动作。
    Client,
}

/// 探测回填的**冷却下限**（毫秒）：上游给的墙钟离现在太近（甚至已过期）时兜住它。
///
/// ## ★ 它不是「兜底时长」（issue #88）
///
/// 旧实现把「下限」当成了「兜底」—— `reset_at_ms` 为 `None` 时直接 `now + 60s`。
/// 而 WB 的 `6004` 一个结构化数值键都不给 ⇒ 每条回填都恰好只冷 60 秒，用户根本来不及看见。
/// 现在「不知道冷多久」走 [`PROBE_FALLBACK_COOLDOWN_MS`]（10 分钟，可配
/// [`PoolConfig::probe_fallback_ms`]），本常量只负责「不能更短」。
///
/// ## ★ 为什么必须有下限（2026-10-16 评审抓出的真缺陷）
///
/// `cooldown_model(.., reset_at_ms = 0, ..)` 会落到 `now + 1ms` —— 那等于**没有冷却**
/// （下一毫秒就又去撞同一堵墙），而调用方却会以为「已置冷却」。
///
/// ## 为什么下限放在**本模块**而不是调用方
///
/// 放在调用方 ⇒ 换个调用方又漏。这里是回填的唯一入口，放这儿才有约束力。
pub const MIN_PROBE_COOLDOWN_MS: i64 = 60_000;

/// 探测回填的**兜底冷却时长**（毫秒，默认 10 分钟）—— 上游完全没给墙钟时用它。
///
/// ## ★★ 为什么必须与 [`MIN_PROBE_COOLDOWN_MS`] 分开（issue #88）
///
/// 旧实现把两者合成一个 60 秒常量：`reset_at_ms` 为 `None` 时直接 `now + 60s`。
/// 而 WB 的 `6004` 响应体**一个结构化数值键都没有**（`resetTime` / `resetAt` … 全缺），
/// 于是**每一条**探测回填的冷却都恰好只有 60 秒。用户的操作节奏（跑完探测 → 切到
/// 账号池分区 → 找到那个号）几乎必然超过它 ⇒ 结论在任何出口上都**不可观测**，
/// 表现为「探测判出限流了，可账号池还是全绿」。
///
/// 60 秒作为「下限」是对的（防 `now + 1ms` 等于没冷却），但作为「兜底时长」太短。
/// 两者是两条不同语义：下限约束「不能更短」，兜底决定「不知道时冷多久」。
///
/// 10 分钟的依据：探测是**用户主动发起**的一次性动作，冷却过长会误伤一个其实
/// 已经恢复的模型；过短则用户来不及看见。10 分钟 ≈ 用户排查一轮的时间量级，
/// 且首次成功请求会立刻清掉它（[`PoolEntry::note_success`] 的反证优先）。
pub const PROBE_FALLBACK_COOLDOWN_MS: i64 = 10 * 60 * 1000;

/// 探测回填的**目标冷却截止时刻**（唯一计算点）。
///
/// 三条规则，顺序不可换：
/// 1. 上游给了**未来**墙钟 ⇒ 用它，但**不得短于** [`MIN_PROBE_COOLDOWN_MS`]
///    （差 3 秒的墙钟等于没冷却，下一秒又去撞同一堵墙）；
/// 2. 没给 / 已过期 ⇒ `now + config.probe_fallback_ms`（默认 10 分钟，**不是 60 秒**）；
/// 3. 兜底值本身也被 [`MIN_PROBE_COOLDOWN_MS`] 兜住，防有人把它配成 0。
///
/// ★ 与真实流量那条链路的 [`MIN_MODEL_COOLDOWN_MS`] **无关**：两条语义独立，
/// 共用一个会让「调探测」误伤真实流量（issue #88 的硬约束）。
fn probe_cooldown_until(now_ms: i64, reset_at_ms: Option<i64>, config: &PoolConfig) -> i64 {
    let floor = now_ms.saturating_add(MIN_PROBE_COOLDOWN_MS);
    match reset_at_ms.filter(|value| *value > now_ms) {
        Some(value) => value.max(floor),
        None => now_ms.saturating_add(config.probe_fallback_ms.max(MIN_PROBE_COOLDOWN_MS)),
    }
}

/// **真实流量**的模型级冷却下限（毫秒）。与 [`MIN_PROBE_COOLDOWN_MS`] 同值、**分常量**声明。
///
/// ## 为什么需要它（2026-10-17 · issue「限流后一直失败」的第一根因）
///
/// [`Pool::apply_upstream_error`] 的模型级分支此前是 `reset_at.unwrap_or(0)` 直传
/// [`PoolEntry::cooldown_model`]，而后者对「墙钟 <= now」一律落 `now + 1ms` ——
/// 等于**没有冷却**。上游 `6004` 恰恰通常不给结构化墙钟，于是：
///
/// - 那个号在「这个模型」上从来没被真正排除；
/// - 每个请求的第一跳都重新选中它、拿一次 429；
/// - `max_rotate` 的重试预算全烧在同一堵墙上，用户看到「池里别的号明明可用却一直失败」。
///
/// 两条常量**刻意分开**：探测回填与真实流量是两条独立语义，共用一个会让
/// 「调探测」误伤真实流量（本仓在 `qoder::pool` 的同名常量旁边已写明这一纪律）。
pub const MIN_MODEL_COOLDOWN_MS: i64 = 60_000;

/// 在途租约的有效期（毫秒，默认 10 分钟）。
///
/// acquire 时把条目的「最晚到期时刻」推到这个点；读侧超过它就视为**全部租约已过期**。
/// 选 10 分钟的依据：lease 在「拿到响应头」时就释放（流式场景的既有偏差），
/// 正常请求的占用量级是秒；10 分钟足够覆盖慢上游，又能让泄漏在十分钟内自愈。
pub const IN_FLIGHT_LEASE_MS: i64 = 10 * 60 * 1000;

/// 上游 401/403（凭据被拒）的冷却时长（毫秒，issue #100）。
///
/// 取 60 秒而不是「三振禁用」：401/403 的绝大多数成因是 **access token 过期**，
/// 保活刷新一轮就好。把可自愈的问题打成终态（`disabled` 只能人工恢复）会让用户
/// 看到「号没坏却再也用不了」。真正被作废的会话会带 `12153`，那条路仍然走三振禁用。
///
/// 也不能取 0：不冷却的话，本轮请求换号成功后，下一轮又会选中这个死号，
/// 每次都要白吃一次 401。
pub const AUTH_REJECT_COOLDOWN_MS: i64 = 60_000;

/// 探测结果的**采纳**语义 —— [`Pool::adopt_probe_result`] 的输入。
///
/// ## 为什么不直接复用 [`UpstreamEvent`]
///
/// 两个理由：
/// 1. 探测有一个 [`UpstreamEvent`] 里**没有**的成功态（`Available`）——它要做的是
///    **清掉**该模型的冷却，而不是置冷却；
/// 2. [`Pool::apply_upstream_error`] 的语义是「**真实流量**吃了这一击」，它会记一次错、
///    推进连续失败计数与 `soft_streak` 退避、并可能触发熔断。探测是**我们主动打的**，
///    被 429 很正常 —— 走那条路会让探测次数混进退避指数，冷却**越探越长**。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProbeOutcome {
    /// 探到可用 ⇒ **清掉**该模型的冷却（用户口径：探到可用就该恢复）。
    Available,
    /// 模型级限流（`6004` / `IsModelRateLimit`）⇒ 只冷该模型，看上游墙钟。
    ModelRateLimited {
        /// 上游声明的重置时刻（毫秒）；无声明为 `None` ⇒ 退化为瞬时冷却。
        reset_at_ms: Option<i64>,
    },
    /// 模型被封禁（`11102`）⇒ 按该模型**既有**的封禁次数退避（探测不 +1）。
    ModelBlocked,
    /// 账号级限流（429 且无模型标记）⇒ 账号级软冷却。
    AccountRateLimited {
        /// 上游声明的重置时刻（毫秒）。
        reset_at_ms: Option<i64>,
    },
    /// 其它档（积分不足 / 登录失效 / 网络错误…）⇒ **不动冷却**。
    Other,
}

/// 余额不足标记（ASCII 小写 + 原样中文）。
const HARD_CREDIT_MARKERS: &[&str] = &[
    "insufficient credit",
    "no credit",
    "credit exhausted",
    "credits exhausted",
    "out of credit",
    "quota exceeded",
    "quota exhaust",
    "payment required",
    "credit not enough",
    "not enough credit",
    "积分不足",
    "额度不足",
    "余额不足",
    "积分用完",
    "额度用尽",
    "没有积分",
];

/// 「余额不足」硬冷却的原因文案（**单一事实来源**）。
///
/// [`Pool::apply_upstream_error`] 在 `HardCredit` 时写入它，余额刷新器在自动解冻时
/// 依此判定「该硬冷却是否源于余额不足」。两处共用同一常量，避免文案漂移导致
/// 解冻判定失效（例如把「余额不足」改成「积分不足」却忘了同步判定）。
pub const HARD_CREDIT_COOLDOWN_REASON: &str = "余额不足";

/// 自动解冻后写入的原因文案（供 `/status` 与日志观察）。
pub const AUTO_THAW_REASON: &str = "余额已恢复，自动解冻";

/// 模型级限流标记（命中表示「只封锁该模型」，按上游墙钟解冻）。
///
/// 刻意**不含** `11102`——那是「模型被封禁」，退避语义不同（按被罚次数逐次加倍），
/// 由 [`MODEL_BLOCK_MARKER`] 单独识别。把两者混为一谈会让封禁账号过早重试。
const MODEL_RATE_MARKERS: &[&str] = &["IsModelRateLimit", "6004"];

/// 模型封禁标记（`11102`）。
const MODEL_BLOCK_MARKER: &str = "11102";

/// 从响应体里尝试解析上游声明的重置时刻。
///
/// 两条分支，**结构化优先**：
///
/// 1. [`parse_reset_at_ms_from_json`] —— 上游各端点字段名不统一，按候选键顺序做**宽容解析**，
///    命中第一个可解析的数值即返回；数值按量级判断单位（`> 1e12` 视为毫秒，否则视为秒）。
/// 2. [`parse_reset_at_ms_from_text`] —— **文案兜底**。真实上游把解冻时刻写在 `msg` 文本里
///    （`… reset at 2026-09-16 16:34:24 UTC+8 …` / `… 将在 2026-09-17 14:18:44 UTC+8 重置 …`），
///    一个 JSON 数值键都没有 ⇒ 只走分支 1 会恒 `None`（2026-09-28 修复）。
///
/// 解析失败一律 `None`：**不得**把「未知」伪装成「已恢复」（调用方据 `None` 决定是否退避）。
///
/// ## ★★ 这个组合出口**只用于诊断/展示**，不用于冷却（2026-10-17）
///
/// 文案里的时刻实测是**配额窗口重置**、不是解锁时刻（`client_limits` 实测 6/6 判错）。
/// 冷却必须走 [`parse_reset_at_ms_from_json`]；否则一次 429 会把号冷掉 2 小时，
/// 而它十几分钟就恢复了 —— 症状是「号明明能用、网关却一直不用它」。
pub fn parse_reset_at_ms(body: &str) -> Option<i64> {
    parse_reset_at_ms_from_json(body).or_else(|| parse_reset_at_ms_from_text(body))
}

/// 结构化形态：JSON 里的数值键。
///
/// ★ **唯一可信的墙钟来源**（`pub` 是为了让 `apply_upstream_error` / 探测回填只认它）。
/// 文案兜底（[`parse_reset_at_ms_from_text`]）**不进冷却**：实测那是配额窗口、不是解锁时刻。
pub fn parse_reset_at_ms_from_json(body: &str) -> Option<i64> {
    const CANDIDATE_KEYS: &[&str] = &[
        "\"resetTime\"",
        "\"reset_time\"",
        "\"resetAt\"",
        "\"reset_at\"",
        "\"expireTime\"",
        "\"expire_time\"",
    ];
    let value: Value = serde_json::from_str(body).ok()?;
    let mut stack: Vec<&Value> = vec![&value];
    while let Some(current) = stack.pop() {
        match current {
            Value::Object(map) => {
                for (key, value) in map {
                    let quoted = format!("\"{key}\"");
                    if CANDIDATE_KEYS.iter().any(|candidate| *candidate == quoted) {
                        if let Some(number) = value.as_f64() {
                            if number > 0.0 {
                                return Some(if number > 1.0e12 {
                                    number as i64
                                } else {
                                    (number * 1000.0) as i64
                                });
                            }
                        }
                    }
                    stack.push(value);
                }
            }
            Value::Array(items) => stack.extend(items.iter()),
            _ => {}
        }
    }
    None
}

/// 文案兜底：从 `msg` 文本里抽出「`<时间戳>` + `UTC+8`」形态的重置时刻。
///
/// ## 为什么只锚时间戳形态，不锚周边文案
///
/// 实测（2026-09-28，本机客户端落盘数据）存在**两套真实模板**：
///
/// | 区域 | 真实原文 |
/// |---|---|
/// | 国际版 | `429 usage exceeds frequency limit, … your usage will reset at 2026-09-16 16:34:24 UTC+8, …` |
/// | 国内版 | `429 您的使用量已超出频率限制，将在 2026-09-17 14:18:44 UTC+8 重置，…` |
///
/// ⇒ 若锚「将在 … 重置」（参考实现 `softRateResetPattern` 的写法），**国际版永不命中**；
/// 若锚英文短语，**国内版永不命中**。两者都是**中文/英文文案**，上游随时可能再改。
/// 因此这里**只锚稳定形态**：`YYYY-MM-DD HH:MM:SS` 紧跟 `UTC+8`。
///
/// ## 时区
///
/// `UTC+8` 是**文案自述**，因此时间戳本身就是 CST 墙上时间 —— 直接按 CST 解释，
/// 复用 [`crate::timeutil::cst_offset`]（唯一时区来源，**不在此另写时区逻辑**）。
fn parse_reset_at_ms_from_text(body: &str) -> Option<i64> {
    // 上游 429 是高频路径，正则只编译一次。
    static RESET_TEXT: OnceLock<Regex> = OnceLock::new();
    let pattern = RESET_TEXT.get_or_init(|| {
        // 允许 `T` 或空格分隔（ISO 风格也覆盖），并要求 UTC+8 标记紧跟在时刻之后。
        Regex::new(r"(\d{4})-(\d{2})-(\d{2})[ T](\d{2}):(\d{2}):(\d{2})\s*UTC\+8")
            .expect("重置时刻正则是常量，必须可编译")
    });
    let caps = pattern.captures(body)?;
    let part = |index: usize| caps[index].parse::<u32>().ok();
    let (year, month, day) = (part(1)?, part(2)?, part(3)?);
    let (hour, minute, second) = (part(4)?, part(5)?, part(6)?);
    // 越界日期/时刻由 chrono 拒绝（`None`），不 panic —— 输入是不可信的上游文本。
    let naive =
        NaiveDate::from_ymd_opt(year as i32, month, day)?.and_hms_opt(hour, minute, second)?;
    crate::timeutil::cst_offset()
        .from_local_datetime(&naive)
        .single()
        .map(|dt| dt.timestamp_millis())
}

/// 按 status + body 分类上游失败（判定顺序固定，与参考实现一致）。
pub fn classify_event(status: u16, body: &str) -> UpstreamEvent {
    if status == 402 {
        return UpstreamEvent::HardCredit;
    }
    let lowered = body.to_lowercase();
    if HARD_CREDIT_MARKERS
        .iter()
        .any(|marker| lowered.contains(&marker.to_lowercase()))
    {
        return UpstreamEvent::HardCredit;
    }
    if SESSION_DEAD_MARKERS
        .iter()
        .any(|marker| body.contains(marker))
    {
        return UpstreamEvent::SessionDead;
    }
    // 模型封禁先于 429 判定：封禁可能以 400/403 返回，把它归到限流分支会走错退避公式。
    if body.contains(MODEL_BLOCK_MARKER) {
        return UpstreamEvent::ModelBlocked;
    }
    if status == 429 {
        let model_scoped = MODEL_RATE_MARKERS
            .iter()
            .any(|marker| body.contains(marker));
        return UpstreamEvent::SoftRate {
            // ★ 只信**结构化**墙钟。文案里的 `reset at` 是配额窗口（实测 6/6 判错），
            //   拿它当冷却时长会把号冷掉几小时。
            //   真实流量的兜底见 `PoolConfig::min_model_cooldown_ms`（下限）与
            //   `PoolConfig::rate_window_reset_max_ms`（账号级上限）；
            //   探测回填的兜底是另一条链，见 `probe_cooldown_until` /
            //   `PoolConfig::probe_fallback_ms`。
            reset_at_ms: parse_reset_at_ms_from_json(body),
            model_scoped,
        };
    }
    if status == 404 {
        return UpstreamEvent::NotFound;
    }
    // ★ issue #100：上游 401/403 = **凭据被拒**，不是「调用方错误」。
    //   归到 `Client` 会导致「不换号 + 不冷却 + 池卡仍显示可路由」三连
    //   （见 `UpstreamEvent::AuthRejected` 的文档）。
    if status == 401 || status == 403 {
        return UpstreamEvent::AuthRejected;
    }
    if status >= 500 {
        return UpstreamEvent::Server;
    }
    UpstreamEvent::Client
}

/// 持久化的单账号状态（**不含** `fails` / `in_flight` / `model_cost`）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
struct PersistedEntry {
    realm: Option<RealmTag>,
    nickname: String,
    credits: i64,
    credits_expiring: i64,
    credits_refreshed_ms: i64,
    disabled: bool,
    reason: String,
    until_ms: i64,
    cool_kind: Option<CoolKind>,
    soft_streak: i32,
    session_dead_fails: i32,
    success_count: i64,
    err_total: i64,
    success_ema: f64,
    error_ema: f64,
    last_success_ms: i64,
    last_err_ms: i64,
    breaker_until_ms: i64,
    retry_count: i32,
    model_cooldowns: HashMap<String, ModelCooldown>,
}

impl Default for PersistedEntry {
    fn default() -> Self {
        Self {
            realm: None,
            nickname: String::new(),
            credits: 0,
            credits_expiring: 0,
            credits_refreshed_ms: 0,
            disabled: false,
            reason: String::new(),
            until_ms: 0,
            cool_kind: None,
            soft_streak: 0,
            session_dead_fails: 0,
            success_count: 0,
            err_total: 0,
            success_ema: 0.0,
            error_ema: 0.0,
            last_success_ms: 0,
            last_err_ms: 0,
            breaker_until_ms: 0,
            retry_count: 0,
            model_cooldowns: HashMap::new(),
        }
    }
}

/// `state.json` 顶层结构。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
#[derive(Default)]
struct PersistedState {
    saved_at_ms: i64,
    accounts: BTreeMap<String, PersistedEntry>,
}

/// 在途额度**租约**（RAII）：`Drop` 归还额度，future 被取消也不会泄漏。
///
/// # 为什么不是 `acquire()` / `release()` 手写配对（PR #41）
///
/// 原写法把两者隔着一整个上游往返：
///
/// ```ignore
/// let ok = pool.write().await.acquire(&uid);   // 占
/// let result = upstream.chat(...).await;        // ← 这里可能永不返回
/// pool.write().await.release(&uid);             // 放（被取消 ⇒ 永不执行）
/// ```
///
/// handler future 被 drop（客户端断开、axum 在连接关闭时取消任务、超时）时，
/// 最后一行**根本不会执行** ⇒ `in_flight` 只增不减 ⇒ 选号把整池过滤掉 ⇒
/// 所有模型 401 `no_credential`，而 `/status` 上「0 禁用、0 冷却、0 零余额」。
/// `in_flight` 不落库，所以旧版本只能靠重启进程清掉。
///
/// 绑到变量上让编译器保证「离开作用域必归还」，正常返回、提前 `return`、
/// `continue`、panic、future 被 drop 全都覆盖。
///
/// ★ 与 `PoolEntry::in_flight_until_ms` 的关系：那个是**兜底自愈**（10 分钟后
///   把泄漏的额度整体视为已释放），本租约是**即时归还**。两者并存，不互相替代 ——
///   自愈能救「本机制之外的泄漏」，本机制能免掉「卡 10 分钟」。
pub struct InFlightLease {
    pool: Arc<RwLock<Pool>>,
    uid: String,
}

impl InFlightLease {
    /// 申请在途额度；占满（或 uid 为空 / 账号不在池里）返回 `None`。
    ///
    /// ★ 返回值**必须绑定到变量**（`let _lease = ...`）。写成 `let _ = ...` 会当场
    ///   drop ⇒ 额度瞬间归还，等于没占（且不会有任何报错）。
    pub async fn acquire(pool: Arc<RwLock<Pool>>, uid: &str) -> Option<Self> {
        if uid.is_empty() || !pool.write().await.acquire(uid) {
            return None;
        }
        Some(Self {
            pool,
            uid: uid.to_string(),
        })
    }
}

impl Drop for InFlightLease {
    fn drop(&mut self) {
        let pool = Arc::clone(&self.pool);
        let uid = std::mem::take(&mut self.uid);
        // `Drop` 不能 `await`，交给运行时释放。取不到 handle ⇒ 运行时已关闭
        // （进程正在退出），此时 `in_flight` 本就不落库、重启即清，不释放也无害。
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn(async move {
                pool.write().await.release(&uid);
            });
        }
    }
}

/// 账号池。
pub struct Pool {
    entries: BTreeMap<String, PoolEntry>,
    /// 用户显式排除的账号（uid → 归属 scope + 备注）。**硬约束**：选号、兜底、偏好全跳过。
    excluded: BTreeMap<String, PoolExclusion>,
    /// **模型级闸门**（跨请求）：同一模型在多个账号上失败即整体暂停，到期半开。
    ///
    /// 与 `PoolEntry::model_cooldowns` 的分工：那是「某个号不能再用这个模型」，
    /// 这是「这个模型此刻整体不可用」。前者解决换号，后者解决**不要换号**（换也没用）。
    gates: ModelGates,
    config: PoolConfig,
    pick_seq: u64,
    dirty: bool,
}

impl Pool {
    /// 新建空池。
    pub fn new(config: PoolConfig) -> Self {
        Self {
            entries: BTreeMap::new(),
            excluded: BTreeMap::new(),
            gates: ModelGates::default(),
            config: config.normalized(),
            pick_seq: 0,
            dirty: false,
        }
    }

    /// 当前配置。
    pub fn config(&self) -> &PoolConfig {
        &self.config
    }

    /// 账号数量。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 是否为空池。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 只读遍历。
    pub fn entries(&self) -> impl Iterator<Item = &PoolEntry> {
        self.entries.values()
    }

    /// 按 uid 取条目。
    pub fn get(&self, uid: &str) -> Option<&PoolEntry> {
        self.entries.get(uid)
    }

    /// 是否有待落盘的改动。
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    fn policy(&self) -> PickPolicy {
        PickPolicy {
            expiring_weight: self.config.expiring_weight,
            expiry_urgency_weight: self.config.expiry_urgency_weight,
            idle_weight_per_hour: self.config.idle_weight_per_hour,
            idle_weight_max: self.config.idle_weight_max,
            min_pick_gap_ms: self.config.min_pick_gap_ms,
            model_cost_ttl_ms: self.config.model_cost_ttl_ms,
            max_in_flight: self.config.max_in_flight,
            max_in_flight_global: self.config.max_in_flight_global,
            deterministic: self.config.deterministic,
        }
    }

    /// 新增或更新账号（保留既有治理状态）。
    pub fn upsert(&mut self, uid: &str, realm: Option<RealmTag>, nickname: &str) {
        if uid.is_empty() {
            return;
        }
        match self.entries.get_mut(uid) {
            Some(entry) => {
                entry.realm = realm.or(entry.realm);
                if !nickname.is_empty() {
                    entry.nickname = nickname.to_string();
                }
            }
            None => {
                let mut entry = PoolEntry::new(uid);
                entry.realm = realm;
                entry.nickname = nickname.to_string();
                self.entries.insert(uid.to_string(), entry);
                self.dirty = true;
            }
        }
    }

    /// 用账号库快照对齐池：新增缺失账号，**不删除**已有账号
    /// （删除会让冷却/熔断历史丢失，重登同一账号即「洗白」）。
    pub fn sync_accounts(&mut self, accounts: &[Value], realm: Option<RealmTag>) {
        for account in accounts {
            let Some(uid) = account
                .get("uid")
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
            else {
                continue;
            };
            let nickname = account
                .get("nickname")
                .and_then(Value::as_str)
                .unwrap_or("");
            self.upsert(uid, realm, nickname);
        }
    }

    /// 用某作用域的最新排除表对齐池内那一部分（**只替换该 scope 的条目**）。
    ///
    /// ## 为什么按 scope 替换而不是整体覆盖
    ///
    /// WB 池一份同时装 CN 与 Global（`realm` 只是条目属性），而排除表是按作用域存盘的
    /// （`wb:cn` / `wb:global`）。若整体覆盖，同步 CN 时会把 Global 的排除项一起清掉 ——
    /// 症状是「刚排除的国际版账号，刷新一下又回来了」（且不报错）。
    pub fn sync_exclusions(&mut self, scope: &str, items: &BTreeMap<String, String>) {
        self.excluded.retain(|_, entry| entry.scope != scope);
        for (uid, note) in items {
            self.excluded.insert(
                uid.clone(),
                PoolExclusion {
                    scope: scope.to_string(),
                    note: note.clone(),
                },
            );
        }
    }

    /// 当前被排除的 uid 集合（选号用；条目很少，克隆成本可忽略）。
    pub fn excluded_keys(&self) -> HashSet<String> {
        self.excluded.keys().cloned().collect()
    }

    /// 该 uid 是否被用户排除。
    pub fn is_excluded(&self, uid: &str) -> bool {
        self.excluded.contains_key(uid)
    }

    /// 该 uid 的排除备注（`None` = 未排除）。
    pub fn exclusion_of(&self, uid: &str) -> Option<&PoolExclusion> {
        self.excluded.get(uid)
    }

    /// 全部排除项（供管理面/端点回显）。
    pub fn exclusions(&self) -> &BTreeMap<String, PoolExclusion> {
        &self.excluded
    }

    /// 写入积分明细（`credits` 与「快过期」子集）。
    pub fn set_credits(&mut self, uid: &str, credits: i64, expiring: i64) {
        let Some(entry) = self.entries.get_mut(uid) else {
            return;
        };
        entry.credits = credits.max(0);
        entry.credits_expiring = expiring.clamp(0, entry.credits);
        self.dirty = true;
    }

    /// 记录最近一次余额刷新时刻（毫秒）。由后台余额刷新循环在成功写入后调用。
    pub fn mark_credits_refreshed(&mut self, uid: &str, now_ms: i64) {
        if let Some(entry) = self.entries.get_mut(uid) {
            entry.credits_refreshed_ms = now_ms;
            self.dirty = true;
        }
    }

    /// 余额恢复后自动解冻**因余额不足**的硬冷却账号。
    ///
    /// 背景：上游 402 → `entry.cooldown_until_tomorrow_4am(now, "余额不足")` →
    /// `CoolKind::Hard`；而 [`pick::pick_earliest_expiry`] **排除仍处于硬冷却的账号**，
    /// 于是该账号会一直闲置到次日 04:00——**即使中途已经充值**。
    ///
    /// 触发（全部满足）：
    /// - `credits > 0`（本次刷新确实拿到正余额）；
    /// - `cool_kind == Some(CoolKind::Hard)`；
    /// - `reason` 含 [`HARD_CREDIT_COOLDOWN_REASON`]（确系余额原因）；
    /// - 非禁用（禁用是独立终态，需刷新成功或人工复活）。
    ///
    /// 动作：调用 [`PoolEntry::clear_cooling`] 清**冷却域**并写入 [`AUTO_THAW_REASON`]，
    /// **绝不动**熔断域（`breaker_until_ms` / `retry_count`）。
    ///
    /// 三条反面约束（同样重要）：
    /// - 余额**未知**（刷新失败 / 接口无数据）→ 调用方不会进入本方法（读数根本不产生）；
    /// - 余额为 **0** → 直接返回 `false`（否则立刻再撞 402，形成抖动）；
    /// - 因**其它原因**（限流 / 404 / 熔断）的冷却 → `cool_kind` 或 `reason` 不匹配 → `false`。
    ///
    /// 返回是否解冻。
    pub fn thaw_hard_credit_if_recovered(&mut self, uid: &str, credits: i64) -> bool {
        if credits <= 0 {
            return false;
        }
        let Some(entry) = self.entries.get_mut(uid) else {
            return false;
        };
        if entry.disabled
            || entry.cool_kind != Some(CoolKind::Hard)
            || !entry.reason.contains(HARD_CREDIT_COOLDOWN_REASON)
        {
            return false;
        }
        entry.clear_cooling();
        entry.reason = AUTO_THAW_REASON.to_string();
        self.dirty = true;
        true
    }

    /// **模型闸门裁决**：`None` = 放行；`Some(block)` = 本次请求不该发出去。
    ///
    /// 三个分支（对应 `GatePhase`）：
    /// - 无闸门 ⇒ 放行；
    /// - 暂停中 ⇒ 拒绝（带剩余时长）；
    /// - 半开 ⇒ **只放一个探针**，其余请求按「仍在暂停」拒绝（`remain_ms = 0`
    ///   表示「票已被别的请求领走」，与「还在暂停」区分开）。
    ///
    /// 放在 `Pool` 上而不是 `relay` 里：闸门状态与池的写锁同一把，
    /// 两处各拿一次锁会给「探针票」留下竞态窗口（两个请求同时领到票）。
    pub fn admit_model(&mut self, model: &str, now_ms: i64) -> Option<GateBlock> {
        let config = self.config.clone();
        if let Some(block) = self.gates.blocked(model, now_ms, &config) {
            return Some(block);
        }
        if !self.gates.is_gated(model) {
            return None;
        }
        // 走到这里 = 刚到期转入半开（或已在半开）。
        if self.gates.try_probe(model, now_ms) {
            None
        } else {
            Some(GateBlock {
                model: model.to_string(),
                remain_ms: 0,
                failures: 0,
            })
        }
    }

    /// 记一次**模型级**失败（`6004` / `11102`）。返回是否因此新开了闸门。
    ///
    /// 只该由中继在 `apply_upstream_error` 之后调用，且**仅当**事件归因到模型 ——
    /// 账号级失败计进来会让闸门频繁误触发（一个号额度耗尽就把整个模型停掉）。
    pub fn note_model_failure(&mut self, model: &str, now_ms: i64) -> bool {
        let config = self.config.clone();
        self.gates.record_failure(model, now_ms, &config)
    }

    /// 该模型成功一次 ⇒ 立刻关闸（成功是最强的反证）。
    pub fn note_model_success(&mut self, model: &str) {
        self.gates.record_success(model);
    }

    /// 当前所有闸门（管理面/诊断用）。
    pub fn model_gates(&self) -> Vec<&ModelGate> {
        self.gates.snapshot()
    }

    #[allow(clippy::too_many_arguments)]
    // 入参都是「这一次选号」的输入，拆结构体会把同一批字段再抄一遍。
    /// 选号；`preferred` 为**偏好 uid**（可用则优先，不可用则回落，绝不拒绝服务）。
    ///
    /// 偏好有两个来源，优先级见 `relay::select_account`：显式固定
    /// （[`crate::AccountStrategy::Pinned`]）**高于**会话粘性。
    /// 算法见 [`pick::pick_with_preference`]。
    ///
    /// `excluded` 是**用户显式排除**的 uid（硬约束：自动选号、兜底、偏好都跳过）；
    /// `allowed` 是**本密钥的允许名单**（issue #76；`None` = 全部账号）——
    /// 与 `excluded` 同层，来源不同（一个是全局排除，一个是这一把 Key 的授权范围）；
    /// `mode` 只决定「在合格候选里挑谁」（加权 / 积分择优 / 随机）。
    pub fn pick_account(
        &mut self,
        now_ms: i64,
        realm: Option<RealmTag>,
        model: &str,
        tried: &HashSet<String>,
        mode: PickMode,
        seed: u64,
        preferred: Option<&str>,
        allowed: Option<&HashSet<String>>,
    ) -> Option<String> {
        let policy = self.policy();
        // 与模型冷却同一处惰性清理：窗口外的失败记录与已关闸的条目不该无限增长。
        self.gates.prune(now_ms, self.config.gate_window_ms);
        let excluded = self.excluded_keys();
        let input = PickInput {
            realm,
            model,
            tried,
            excluded: &excluded,
            // ★ issue #76：本密钥的允许名单（`None` = 全部账号）。
            //   分组由调用方展开成 uid 后再传进来 —— 池条目不认识「分组」这个概念。
            allowed,
            mode,
            seed,
        };
        pick_with_preference(
            &mut self.entries,
            &mut self.pick_seq,
            &policy,
            now_ms,
            &input,
            preferred,
        )
    }

    /// **人工解除禁用**（清禁用 + 清冷却域，**保留熔断历史**）。
    ///
    /// 返回「本次是否真的解除了一个禁用」；`false` = 该 uid 不在池里，或本来就没被禁用
    /// （幂等：重复点不会报错，也不会重复清状态）。
    ///
    /// # 为什么必须有这个出口（2026-10-14 实测：一个**没有任何出口**的终态）
    ///
    /// [`PoolEntry::disabled`] 是终态，而且**落盘持久化**（[`restore_entry`] 会读回）。
    /// 但代码里写的两条恢复路径**都不存在**：
    ///
    /// - [`Self::thaw_hard_credit_if_recovered`] 第一句就是 `if entry.disabled { return false }`
    ///   ⇒ 余额刷新**救不了**禁用账号；
    /// - [`PoolEntry::revive`] 在本模块之外**零调用点**（只有单测用）。
    ///
    /// ⇒ 一个被三振（`12153` 会话失效连续 3 次）禁用的账号**永久不可用**，用户在界面上
    /// 找不到任何出口。这正是 issue #26「切换过多个账号、桌面端和 Buddy Switch 都试过
    /// 解决不了」的另一半原因。
    ///
    /// # 为什么不做「重新登录 ⇒ 自动复活」（**已验证不可行，别再试**）
    ///
    /// 唯一看似可行的信号是「账号库里那条记录变了」。实测两条都不成立：
    ///
    /// - `id` / `createdAt` **刻意被保留**（`account::upsert_collected_account`：重新导入同一
    ///   uid 时把已有的 `id` / `createdAt` 原样写回）⇒ 重新登录**不换** id，id 变化无法作信号；
    /// - `access_token` 确实会变，但**后台保活刷新也会改它**（`refresh::refresh_account_token_for`
    ///   只写 token、保留 id）⇒ 用 token 当指纹等于「每次保活都把所有被禁用的号洗白」，
    ///   三振机制直接被取消。
    ///
    /// 结论：账号库里**没有任何字段**能区分「用户重新登录」与「保活刷新」⇒ 自动复活必然误判。
    /// 故只提供**显式**出口，与网关 API Key 的「恢复」（吊销 ↔ 恢复）同构。
    ///
    /// # 为什么**还要**清熔断截止（而不是沿用 [`PoolEntry::revive`] 的「不动熔断域」）
    ///
    /// `PoolEntry::healthy` 是 `!disabled && now >= until_ms && now >= breaker_until_ms`
    /// —— **熔断截止同样挡选号**。三振禁用时 `note_error` 已顺手压上一次熔断
    /// （默认 30 分钟，退避封顶 6 小时）⇒ 若只清 `disabled`，用户点了「恢复」后
    /// 账号**最长 6 小时仍选不中**，而界面不会告诉他为什么 —— 按钮点了没用，等于坏了。
    ///
    /// 语义上这也说得通：熔断是**自动**治理（防机器反复撞同一堵墙），人工复活是**人的
    /// 显式判断**（「我知道我在做什么」）。两者冲突时后者优先。
    ///
    /// 保留 `retry_count`（退避指数）：它**不参与可用性判定**，只让这个号下次熔断
    /// 退避更久 —— 既给了出口，也记住了教训。若这个号其实还是坏的，它会在 3 次失败后
    /// **再次**被禁用，且下次熔断退避更长（不会变成「点一次试三次」的无限循环）。
    pub fn revive(&mut self, uid: &str) -> bool {
        let Some(entry) = self.entries.get_mut(uid) else {
            return false;
        };
        if !entry.disabled {
            return false;
        }
        entry.revive();
        entry.breaker_until_ms = 0;
        self.dirty = true;
        true
    }

    /// 指定 uid 此刻是否可用于该请求。
    ///
    /// 供**会话粘性**使用：粘性先问「上次那个账号还能不能用」，能用就复用、不能用就解绑换号。
    /// 粘性是**优化而非约束**，所以这里必须能明确判出「不可用」，绝不能因为粘性而拒绝服务。
    ///
    /// ★ 判据与 [`Self::pick_account`] **同源**（共用 `pick::selectable`）——
    /// 若这里另写一套且更宽松，粘性就会把请求钉到 `pick` 认为不可用的账号上
    /// （症状：明明有别的号可用却一直失败）。
    pub fn is_usable_for(&self, uid: &str, now_ms: i64, model: &str) -> bool {
        let policy = self.policy();
        let excluded = self.excluded_keys();
        // ★ issue #76：`is_usable_for` 问的是「账号本身还能不能用」——
        //   **不带** Key 白名单（那是「这一把 Key 允不允许用它」，两个问题）。
        //   白名单只在选号入口（`pick_account`）生效，这里若也判，
        //   「复活」按钮会把白名单外的账号判成不可恢复（语义混淆）。
        pick::is_usable(&self.entries, &policy, uid, now_ms, model, &excluded, None)
    }

    /// 申请在途额度；返回是否成功（失败表示该号已占满）。
    ///
    /// 每次成功都把 `in_flight_until_ms` 推到 `now + IN_FLIGHT_LEASE_MS`；
    /// 若发现上一批租约已整体过期，先把裸计数清零（**泄漏自愈**）。
    ///
    /// ★ `pub(crate)`（2026-10-17）：业务代码**只能**通过 [`InFlightLease::acquire`] 占用。
    /// 裸调它 + 手写 `release` 就是 PR #41 之前那个泄漏 bug 的形状，而注释拦不住人，
    /// 可见性才拦得住。
    pub(crate) fn acquire(&mut self, uid: &str) -> bool {
        let now = crate::timeutil::now_ms();
        let policy = self.policy();
        let Some(entry) = self.entries.get_mut(uid) else {
            return false;
        };
        let limit = match entry.realm {
            Some(RealmTag::Global) if policy.max_in_flight_global > 0 => {
                policy.max_in_flight_global
            }
            _ => policy.max_in_flight,
        };
        let live = entry.in_flight_now(now);
        if limit > 0 && live >= limit {
            return false;
        }
        // 过期租约一律回收：此时 `in_flight_now` 已经是 0，裸计数还停在旧值上。
        if entry.in_flight != live {
            entry.in_flight = live;
        }
        entry.in_flight += 1;
        entry.in_flight_until_ms = now.saturating_add(IN_FLIGHT_LEASE_MS);
        true
    }

    /// 释放在途额度（幂等；重复释放不会把计数压成负数）。
    ///
    /// ★ **业务代码不要直接调它**：`acquire` / `release` 手写配对在被取消的 future 里会漏
    /// （release 那行不执行），这正是 PR #41 修掉的 bug。请用
    /// [`InFlightLease::acquire`]，把归还交给 `Drop`。本方法只在租约内部与测试里用
    /// —— 因此它是 `pub(crate)`：跳出本 crate 就调不到。
    pub(crate) fn release(&mut self, uid: &str) {
        if let Some(entry) = self.entries.get_mut(uid) {
            if entry.in_flight > 0 {
                entry.in_flight -= 1;
            }
        }
    }

    /// 成功回调：清熔断域、记账本、按实际上报扣减余额。
    pub fn note_success(&mut self, uid: &str, model: &str, credit: f64, tokens: i64, now_ms: i64) {
        let Some(entry) = self.entries.get_mut(uid) else {
            return;
        };
        entry.note_success(now_ms, model);
        if entry.record_cost(now_ms, model, credit, tokens) {
            entry.deduct_credits(credit);
        }
        self.dirty = true;
    }

    /// **仅**写入实测成本账本（不重复累计成功次数）。
    ///
    /// 流式响应的 usage 要到流末尾才拿到，而「请求成功」在响应头到达时就已确认，
    /// 后者已调用 [`Self::note_success`]。若此处再调一次，`success_count` 与成功率
    /// EMA 会被同一请求计两次，四因子里的「成功率」权重随之失真。
    pub fn record_ledger(&mut self, uid: &str, model: &str, credit: f64, tokens: i64, now_ms: i64) {
        let Some(entry) = self.entries.get_mut(uid) else {
            return;
        };
        if entry.record_cost(now_ms, model, credit, tokens) {
            entry.deduct_credits(credit);
            self.dirty = true;
        }
    }

    /// 失败回调：按事件类型施加冷却 / 熔断 / 禁用。
    ///
    /// 返回是否因本次事件而**禁用**了账号（调用方据此跳过后续重试）。
    pub fn apply_upstream_error(
        &mut self,
        uid: &str,
        model: &str,
        event: &UpstreamEvent,
        now_ms: i64,
    ) -> bool {
        let config = self.config.clone();
        let Some(entry) = self.entries.get_mut(uid) else {
            return false;
        };
        self.dirty = true;
        // ★ 调用方错误（其它 4xx）与账号侧错误**分家**。
        //
        // 下面 `UpstreamEvent::Client` 分支**不做任何治理动作** —— 判定就是「不是这个账号的问题」。
        // 既然不处置，就不该再扣它的健康度：`error_ema` 的用途是选号权重，而调用方的坏请求
        // 不构成「这个号靠不住」的证据。混在一起会把健康账号的 `error_ema` 拖到 0.9
        // （实测：某账号 88.8% 的错误率全部来自这类不治理的 4xx，但它其实一直可用）。
        // 这类错误只累计 `err_total` 供诊断，不推进 EMA / `fails`。
        if matches!(event, UpstreamEvent::Client) {
            entry.note_caller_error(now_ms);
        } else {
            entry.note_error(now_ms);
        }

        match event {
            UpstreamEvent::HardCredit => {
                entry.cooldown_until_tomorrow_4am(now_ms, HARD_CREDIT_COOLDOWN_REASON);
                true
            }
            UpstreamEvent::SoftRate {
                reset_at_ms,
                model_scoped,
            } => {
                if *model_scoped && !model.is_empty() {
                    // 模型级限流：只冷却该模型，切模型立即可用。
                    //
                    // ★★ 冷却时长口径（2026-10-17，本 bug 的第一根因）：
                    //   只信**结构化**墙钟；没有（或已过期）⇒ 用 `min_model_cooldown_ms`（60s）。
                    //   旧实现这里落了 `now + 1ms`（= 不冷却），于是同一个号**每个请求**都被
                    //   重新选中、每次都 429，重试预算全烧在同一堵墙上 —— 用户看到的就是
                    //   「池里别的号明明可用，请求却一直失败」。
                    let reset_at = reset_at_ms
                        .filter(|value| {
                            *value > now_ms.saturating_add(config.min_model_cooldown_ms)
                        })
                        .unwrap_or_else(|| now_ms.saturating_add(config.min_model_cooldown_ms));
                    entry.cooldown_model(
                        now_ms,
                        model,
                        reset_at,
                        // ★ 封顶与账号级**同一个值**（15 分钟）：两处不同会让「同一个错误
                        //   在模型维度被放大 4 倍」（QA 复核指出的不对称）。真限流会由后续
                        //   429 继续延长，不需要一次押 2 小时。
                        config.rate_window_reset_max_ms,
                        "6004 model rate limit",
                    );
                    // O6：这是**模型维度**的失败 ⇒ 记进该模型的健康度，
                    // 账号级 EMA 不动（否则这个号在别的模型上也被一起降权）。
                    entry.note_model_result(model, false);
                } else {
                    match reset_at_ms {
                        Some(reset_at) => entry.cooldown_soft_until(
                            now_ms,
                            *reset_at,
                            // ★ 账号级同样不采信「配额窗口」长墙钟：封顶 15 分钟。
                            //   真限流会由后续的 429 继续延长，不需要一次押 2 小时。
                            config.rate_window_reset_max_ms,
                            "429 rate limit",
                        ),
                        None => {
                            entry.cooldown_soft_backoff(
                                now_ms,
                                config.soft_rate_ms,
                                config.rate_window_reset_max_ms,
                                "429 rate limit",
                            );
                        }
                    }
                }
                false
            }
            UpstreamEvent::ModelBlocked => {
                if model.is_empty() {
                    // 无模型名可归因：退化为账号级浅冷却。完全不加处置会让下一次选号
                    // 立刻再撞同一堵墙，比保守冷却更浪费额度。
                    entry.cooldown_fixed(now_ms, 60_000, "11102 model blocked (no model)");
                } else {
                    entry.block_model_backoff(now_ms, model, "11102 model blocked");
                    // O6：模型被封禁是**模型维度**的失败，同样只记该模型。
                    entry.note_model_result(model, false);
                }
                false
            }
            UpstreamEvent::NotFound => {
                // 404 固定浅冷却，不参与退避计数。
                entry.cooldown_fixed(now_ms, 60_000, "upstream 404");
                false
            }
            UpstreamEvent::Server => {
                entry.maybe_trip_breaker(
                    now_ms,
                    config.breaker_threshold,
                    config.breaker_cooldown_ms,
                    config.breaker_cooldown_max_ms,
                );
                false
            }
            UpstreamEvent::SessionDead => {
                entry.note_session_dead(config.session_dead_threshold, "12153 session dead")
            }
            UpstreamEvent::AuthRejected => {
                // 401/403：**短冷却 + 换号**，不计数、不禁用（见 `AuthRejected` 的文档）。
                // 冷却时长与 404 同档：一分钟足够让本轮请求换到别的号，
                // 又短到「保活刷新出新的 access token 后」不必人工干预就能自己恢复。
                entry.cooldown_fixed(
                    now_ms,
                    AUTH_REJECT_COOLDOWN_MS,
                    "upstream 401/403 (credential rejected)",
                );
                false
            }
            UpstreamEvent::Client => false,
        }
    }

    /// ★ **采纳一次探测结果**（唯一入口，见 [`crate::model_probe`]）。
    ///
    /// ## 为什么不能复用 [`Pool::apply_upstream_error`]
    ///
    /// 那个出口的语义是「**真实流量**吃下这一击」：它记一次错（`note_error`）、推进
    /// `fails` / `soft_streak` / `retry_count` 这些**退避计数**、并可能触发熔断。
    /// 而探测是**我们主动打的**，被 429 本来就是常态 —— 走那条路会让「探测次数」混进
    /// 退避指数，冷却**越探越长**（自我实现的限流）。
    ///
    /// ⇒ 本入口**只置/清冷却**：`fails` / `soft_streak` / `retry_count` /
    /// `success_ema` / `error_ema` / `session_dead_fails` **逐字段不变**（有单测钉住）。
    ///
    /// ## 三条安全约束（2026-10-16 安全审计）
    ///
    /// - **`realm` 必校验**：池以 `uid` 为键、`realm` 只是属性 ⇒ 不校验就可能**跨作用域**
    ///   写错条目（`cn` 的探测写进 `global` 的号）。`realm` 为 `None` 时**不做**这个校验
    ///   （调用方确实不知道归属）；条目 `realm` 为 `None`（未知）时也不拦。
    /// - **冷却有下限**（[`MIN_PROBE_COOLDOWN_MS`]）：上游没给墙钟 / 墙钟已过期时补齐，
    ///   否则 `cooldown_model(.., 0, ..)` 只冷 1ms = 等于没冷却。
    /// - **会置 `dirty`**：与 [`Pool::apply_upstream_error`] 同款 —— 否则周期落盘的
    ///   `flush_if_dirty` 不会写盘，回填**静默丢失**。
    ///
    /// 返回**是否真的改变了冷却**（幂等：同一结论重复采纳不会重复计数；
    /// 既有的冷却更晚时不会被缩短）。
    pub fn adopt_probe_result(
        &mut self,
        uid: &str,
        realm: Option<RealmTag>,
        model: &str,
        outcome: &ProbeOutcome,
        now_ms: i64,
    ) -> bool {
        let config = self.config.clone().normalized();
        let changed = {
            let Some(entry) = self.entries.get_mut(uid) else {
                return false;
            };
            // 跨作用域防御（见上方文档）。
            if realm.is_some() && entry.realm.is_some() && entry.realm != realm {
                return false;
            }
            if entry.disabled {
                return false;
            }
            match outcome {
                ProbeOutcome::Available => {
                    if model.is_empty() {
                        false
                    } else {
                        entry.model_cooldowns.remove(model).is_some()
                    }
                }
                ProbeOutcome::ModelRateLimited { reset_at_ms } => {
                    if model.is_empty() {
                        return false;
                    }
                    let before = entry
                        .model_cooldowns
                        .get(model)
                        .map(|cool| cool.until_ms)
                        .unwrap_or(0);
                    // ★ 下限 + 兜底：未知 / 已过期的墙钟一律走 `probe_fallback_ms`
                    //   （默认 10 分钟），不再是写死的 60 秒（issue #88）。
                    let reset_at = probe_cooldown_until(now_ms, *reset_at_ms, &config);
                    entry.cooldown_model(
                        now_ms,
                        model,
                        reset_at,
                        config.soft_rate_max_ms,
                        "6004 model rate limit (probe)",
                    );
                    entry
                        .model_cooldowns
                        .get(model)
                        .map(|cool| cool.until_ms)
                        .unwrap_or(0)
                        > before
                }
                ProbeOutcome::ModelBlocked => {
                    if model.is_empty() {
                        return false;
                    }
                    let before = entry
                        .model_cooldowns
                        .get(model)
                        .map(|cool| cool.until_ms)
                        .unwrap_or(0);
                    // ★ 不推进 `hits`（那是「真实流量被罚了几次」的历史）。
                    entry.block_model_backoff_without_hits(
                        now_ms,
                        model,
                        "11102 model blocked (probe)",
                    );
                    entry
                        .model_cooldowns
                        .get(model)
                        .map(|cool| cool.until_ms)
                        .unwrap_or(0)
                        > before
                }
                ProbeOutcome::AccountRateLimited { reset_at_ms } => {
                    let before = entry.until_ms;
                    let reset_at = probe_cooldown_until(now_ms, *reset_at_ms, &config);
                    entry.cooldown_soft_until(
                        now_ms,
                        reset_at,
                        config.soft_rate_max_ms,
                        "429 rate limit (probe)",
                    );
                    entry.until_ms > before
                }
                ProbeOutcome::Other => false,
            }
        };
        if changed {
            self.dirty = true;
        }
        changed
    }

    /// 刷新成功后清零会话失效计数（避免跨轮累计误禁用）。
    pub fn clear_session_dead(&mut self, uid: &str) {
        if let Some(entry) = self.entries.get_mut(uid) {
            entry.session_dead_fails = 0;
        }
    }

    /// 五态计数摘要（`realm = None` 为全池）。
    ///
    /// 五态互斥且穷尽，判定见 [`PoolEntry::status_of`]。
    pub fn summary(&self, now_ms: i64, realm: Option<RealmTag>) -> PoolSummary {
        self.summary_filtered(now_ms, realm, None)
    }

    /// **名单口径**的池快照（issue #76）：只统计 `allowed` 里的账号。
    ///
    /// 白名单生效时**必须**用这个而不是 [`Self::summary`]：全池口径会告诉用户
    /// 「有 12 个号但 0 个可用」，而他这把 Key 只允许用其中 2 个 —— 数字对不上，
    /// 也没法定位到底卡在哪。统计规则与 `summary` **逐条同源**（六态互斥 +
    /// 排除优先 + `in_flight_full` 同源判据），只多一道 uid 过滤，所以不会出现
    /// 「卡片说 2 个可用、名单口径说 0 个」这种新裂缝。
    pub fn summary_of(
        &self,
        allowed: &HashSet<String>,
        now_ms: i64,
        realm: Option<RealmTag>,
    ) -> PoolSummary {
        self.summary_filtered(now_ms, realm, Some(allowed))
    }

    /// [`Self::summary`] / [`Self::summary_of`] 的唯一实现。
    fn summary_filtered(
        &self,
        now_ms: i64,
        realm: Option<RealmTag>,
        only: Option<&HashSet<String>>,
    ) -> PoolSummary {
        let mut summary = PoolSummary {
            total: 0,
            ..PoolSummary::default()
        };
        for entry in self.entries.values() {
            if realm.is_some() && entry.realm != realm {
                continue;
            }
            // ★ 名单过滤放在**计数之前**：`total` 必须是名单内的总数，
            //   否则 `available + ... == total` 这条不变量在名单口径下不成立。
            if let Some(only) = only {
                if !only.contains(&entry.uid) {
                    continue;
                }
            }
            summary.total += 1;
            // ★ 排除优先于一切状态：它决定「能不能被选中」，因此必须先从 `available`
            //   里扣掉，否则「4 可用」与实际可选中数不符（见 `PoolSummary::excluded`）。
            if self.excluded.contains_key(&entry.uid) {
                summary.excluded += 1;
            } else {
                match entry.status_of(now_ms) {
                    "disabled" => summary.disabled += 1,
                    "cooling" => summary.cooling += 1,
                    // `expired` 本侧无判据（见 `PoolEntry::status_of`），恒为 0。
                    "no_credits" => summary.zero_credits += 1,
                    _ => {
                        summary.available += 1;
                        // ★ 与选号**同源**判据（`pick::in_flight_full`：自愈计数 + realm 上限），
                        //   否则「卡片说可用、pick 却一个都不给」又会成为一类新报障。
                        //   只在可用分支里数：禁用/冷却/零积分/被排除的号即便在途也不能选中。
                        if pick::in_flight_full(entry, &self.policy(), now_ms) {
                            summary.in_flight_full += 1;
                        }
                        // ★ 模型级冷却是**正交**信号（issue #88）：账号仍可用，只是某几个
                        //   模型被罚。与 `in_flight_full` 同一档 —— 只数「状态可用」的号，
                        //   否则会与 `cooling` / `disabled` 重复计数。
                        if entry
                            .model_cooldowns
                            .values()
                            .any(|cool| cool.until_ms > now_ms)
                        {
                            summary.model_cooling += 1;
                        }
                    }
                }
            }
            // 余额未知（从未刷新）不计入总额：把未知当 0 会让「总积分」看起来凭空变少。
            if entry.credits_refreshed_ms > 0 {
                summary.total_credits += entry.credits as f64;
            }
        }
        summary.total_credits = (summary.total_credits * 100.0).round() / 100.0;
        summary
    }

    /// 逐账号状态（**camelCase，与 Trae 侧 `status_list` 同形**）。
    ///
    /// 管理面「账号池」卡两侧共用，故键名即契约：`uid` / `name` / `realm` /
    /// `status` / `credits` / `cooling` / `cooldownUntil` / `cooldownReason` / `disabled`
    /// / `modelCoolings`。
    ///
    /// ★ `modelCoolings`（`model → until_ms`）是**模型级冷却**对前端的唯一出口：前端「限流探测」
    /// 矩阵的被动底色 = 池冷却 + 客户端台账。**只收 `until_ms > now` 的**（过期的不外发，
    /// 见下）。其余两个平台是各自的 `status_list`，不产出该字段（前端按下表缺省处理）。
    ///
    /// ★ `cooldownUntil` / `cooldownReason` **只在真的处于冷却 / 禁用时**才给 —— 与 Trae 侧
    /// `account_view` 的 `active_cooldown.filter(|e| e.until > now && …)` 同义。
    /// 冷却靠**墙钟**到期，而 `reason` 没有任何人在到期时清掉 ⇒ 若照搬字段，
    /// 一条两天前的 `429 rate limit` 会被当成**当前**原因复述出去，
    /// 前端就渲染出「徽标=可用，旁边却写着 429」这种自相矛盾的行
    /// （症状：用户报「这个号在网关里是冷却的，可我在客户端明明能用」）。
    ///
    /// ★★ 判据是 `cooling || disabled`，**不能只判 `cooling`**：`reason` 是**双语义字段**
    /// （冷却原因与禁用原因共用它，见 [`PoolEntry::disable`] 的注释）。本函数**没有**
    /// `disabled_reason` 这个独立出口（`snapshot` 才有），只判 `cooling` 会把禁用原因一起藏掉。
    pub fn status_list(&self, now_ms: i64) -> Vec<Value> {
        self.entries
            .values()
            .map(|entry| {
                let status = entry.status_of(now_ms);
                // ★ 排除是**互斥第六态**：一个被排除的号可能同时「可用」，但它不能被选中，
                //   所以状态必须是 `excluded` 而不是 `available` + 一个正交标记。
                let exclusion = self.excluded.get(&entry.uid);
                let status = if exclusion.is_some() {
                    "excluded"
                } else {
                    status
                };
                let cooling = status == "cooling";
                let reason_visible = cooling || entry.disabled;
                let cooldown_until = entry.expiry_ms();
                // ★ 模型级冷却（`PoolEntry::model_cooldowns`）——**只收仍有效**的
                // （`until_ms > now_ms`）。与 `cooldownUntil` 同理：冷却靠**墙钟**到期，
                // 而没有任何人在到期时清字段 ⇒ 照搬会把早已解除的限流当成「此刻仍冷」，
                // 前端据此画出的「被动底色」就是假数据（用户会去傻等一个早已解除的锁）。
                //
                // 这是「**经网关真实流量**打出来的模型级冷却」对前端的唯一出口：
                // 前端「限流探测」矩阵的被动底色 = 池冷却 + 客户端台账（`client_limits`）。
                //
                // 键名 `modelCoolings` 与同对象其它字段一致（**camelCase**）：本对象与 Trae 侧
                // `status_list` **共用同一张前端卡**，键名必须是 camelCase 那套
                // （见 `status_list_uses_the_shape_shared_with_trae` 的「不得出现 snake_case」）。
                let model_coolings: BTreeMap<&str, i64> = entry
                    .model_cooldowns
                    .iter()
                    .filter(|(_, cooldown)| cooldown.until_ms > now_ms)
                    .map(|(model, cooldown)| (model.as_str(), cooldown.until_ms))
                    .collect();
                json!({
                    "uid": entry.uid,
                    "name": entry.nickname,
                    "realm": realm_key(entry.realm),
                    "status": status,
                    // 余额未知 ⇒ `null`（Trae 侧同义），前端显示为「—」。
                    "credits": (entry.credits_refreshed_ms > 0).then_some(entry.credits as f64),
                    "cooling": cooling,
                    "cooldownUntil": (cooling && cooldown_until > 0).then_some(cooldown_until),
                    "cooldownReason": reason_visible
                        .then(|| entry.reason.clone())
                        .filter(|reason| !reason.is_empty()),
                    "disabled": entry.disabled,
                    // 用户显式排除（硬约束：选号 / 兜底 / 偏好都跳过）。`excludedNote` 给 UI 展示原因。
                    "excluded": exclusion.is_some(),
                    "excludedNote": exclusion
                        .map(|item| item.note.clone())
                        .filter(|note| !note.is_empty()),
                    // 模型 → 解冻时刻（ms）。空表 = 该账号此刻没有模型级冷却。
                    "modelCoolings": model_coolings,
                })
            })
            .collect()
    }

    /// 诊断：逐账号「此刻为什么（不）能路由」，供管理面排查「没有可用账号」。
    pub fn diagnose(&self, now_ms: i64) -> Vec<String> {
        self.entries
            .values()
            .map(|entry| {
                let tail = entry
                    .uid
                    .get(entry.uid.len().saturating_sub(8)..)
                    .unwrap_or("");
                let credits = if entry.credits_refreshed_ms > 0 {
                    entry.credits.to_string()
                } else {
                    "未知".to_string()
                };
                format!(
                    "{}({tail}:{},积分={credits})",
                    entry.nickname,
                    // ★ 顺序：排除 > 禁用 > 冷却 > 零积分。
                    //   排除是**用户的显式意图**，最该先说明 —— 否则用户会以为「是这个号坏了」，
                    //   去客户端反复重登（而真正要做的只是取消排除）。
                    match self.excluded.get(&entry.uid) {
                        Some(item) if item.note.is_empty() => "已排除（用户设置）".to_string(),
                        Some(item) => format!("已排除（{}）", item.note),
                        None => entry.rejection(now_ms),
                    }
                )
            })
            .collect()
    }

    /// 池状态快照（`/status` 响应体的 `accounts` 部分）。
    pub fn snapshot(&self, now_ms: i64) -> Value {
        let accounts: Vec<Value> = self
            .entries
            .values()
            .map(|entry| {
                // ★ 冷却三件套（`cool_kind` / `until_ms` / `reason`）**只在真的冷却中**才给。
                // 它们靠墙钟到期，而没有任何人在到期时清字段 ⇒ 照搬会把两天前的
                // `429 rate limit` 当成**当前**原因发出去（与 `status_list` 同一处缺陷，
                // 见那里的注释；这个出口走网关 `/status`，同样有外部读者）。
                let cooling = now_ms < entry.until_ms;
                let mut item = json!({
                    "uid": entry.uid,
                    "realm": realm_key(entry.realm),
                    "nickname": entry.nickname,
                    "credits": entry.credits,
                    "cooling": cooling,
                    "disabled": entry.disabled,
                    // 自愈读数：泄漏的租约到期后不再算「占用」（与选号同一判据）。
                    "in_flight": entry.in_flight_now(now_ms),
                    "breaker_fails": entry.fails,
                });
                if cooling {
                    if let Some(kind) = entry.cool_kind {
                        item["cool_kind"] = json!(match kind {
                            CoolKind::Hard => "hard",
                            CoolKind::Soft => "soft",
                        });
                    }
                    if entry.until_ms > 0 {
                        item["until_ms"] = json!(entry.until_ms);
                    }
                    if !entry.reason.is_empty() {
                        item["reason"] = json!(entry.reason);
                    }
                }
                let remaining = entry.cool_remaining_ms(now_ms);
                if remaining > 0 {
                    item["cool_remaining_sec"] = json!(remaining / 1000);
                }
                if entry.soft_streak > 0 {
                    item["soft_streak"] = json!(entry.soft_streak);
                }
                if entry.disabled {
                    item["disabled_reason"] = json!(entry.reason);
                }
                if entry.success_count > 0 {
                    item["success_count"] = json!(entry.success_count);
                }
                if entry.err_total > 0 {
                    item["err_total"] = json!(entry.err_total);
                }
                if entry.last_success_ms > 0 {
                    item["last_success_ms"] = json!(entry.last_success_ms);
                }
                if entry.last_err_ms > 0 {
                    item["last_err_ms"] = json!(entry.last_err_ms);
                }
                if entry.breaker_until_ms > 0 {
                    item["breaker_until_ms"] = json!(entry.breaker_until_ms);
                }
                if !entry.model_cooldowns.is_empty() {
                    let mut limited: Vec<(&String, &ModelCooldown)> =
                        entry.model_cooldowns.iter().collect();
                    limited.sort_by(|left, right| left.0.cmp(right.0));
                    item["rate_limited_models"] = json!(limited
                        .into_iter()
                        .map(|(model, cooldown)| json!({
                            "model": model,
                            "until_ms": cooldown.until_ms,
                            "reset_at_ms": cooldown.reset_at_ms,
                            "reason": cooldown.reason,
                        }))
                        .collect::<Vec<Value>>());
                }
                item
            })
            .collect();

        let policy = self.policy();
        let mut healthy = 0;
        let mut cooling = 0;
        let mut disabled = 0;
        let mut in_flight_full = 0;
        let mut realm_totals: BTreeMap<&str, [i64; 4]> = BTreeMap::new();
        for entry in self.entries.values() {
            let is_healthy = entry.healthy(now_ms);
            let is_cooling = now_ms < entry.until_ms || now_ms < entry.breaker_until_ms;
            // ★ 「满没满」必须与选号**同一个判据**（`pick::in_flight_full`：自愈计数 +
            //   realm 上限）。此处曾自己写一遍 `in_flight_now() >= max_in_flight`，
            //   漏了 `max_in_flight_global` —— 国际版上限更严时，`/status` 会说「没占满」
            //   而选号已经挑不出号，两边各说一套。
            //
            //   与 `PoolSummary::in_flight_full` 的差别只剩**统计范围**（有意如此）：
            //   这里数全部条目（诊断明细，含禁用/冷却），摘要只数「可用」分支。
            let full = pick::in_flight_full(entry, &policy, now_ms);
            if is_healthy {
                healthy += 1;
            }
            if is_cooling {
                cooling += 1;
            }
            if entry.disabled {
                disabled += 1;
            }
            if full {
                in_flight_full += 1;
            }
            let bucket = realm_totals.entry(realm_key(entry.realm)).or_insert([0; 4]);
            bucket[0] += 1;
            if is_healthy {
                bucket[1] += 1;
            }
            if is_cooling {
                bucket[2] += 1;
            }
            if entry.disabled {
                bucket[3] += 1;
            }
        }

        json!({
            "accounts": accounts,
            "total": self.entries.len(),
            "healthy": healthy,
            "cooling": cooling,
            "disabled": disabled,
            "in_flight_full": in_flight_full,
            "realm_totals": realm_totals
                .into_iter()
                .map(|(realm, counts)| {
                    (realm.to_string(), json!({
                        "total": counts[0],
                        "healthy": counts[1],
                        "cooling": counts[2],
                        "disabled": counts[3],
                    }))
                })
                .collect::<serde_json::Map<String, Value>>(),
        })
    }

    /// 原子落盘：先写 `<path>.tmp` 再 rename，避免半截文件被当成有效状态读取。
    pub fn save(&mut self, path: &Path) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        let state = PersistedState {
            saved_at_ms: crate::timeutil::now_ms(),
            accounts: self
                .entries
                .iter()
                .map(|(uid, entry)| (uid.clone(), persist_of(entry)))
                .collect(),
        };
        let text = serde_json::to_string_pretty(&state).map_err(|error| error.to_string())?;

        let tmp: PathBuf = path.with_extension("json.tmp");
        std::fs::write(&tmp, text.as_bytes()).map_err(|error| error.to_string())?;
        restrict_permissions(&tmp);
        std::fs::rename(&tmp, path).map_err(|error| error.to_string())?;

        self.dirty = false;
        Ok(())
    }

    /// 仅在有改动时落盘（后台定时器调用）。
    pub fn flush_if_dirty(&mut self, path: &Path) -> Result<bool, String> {
        if !self.dirty {
            return Ok(false);
        }
        self.save(path)?;
        Ok(true)
    }

    /// 从 `state.json` 恢复，并执行**择优恢复**（见 [`restore_entry`]）。
    ///
    /// 文件缺失或损坏 → 保持空池（不 panic）。
    pub fn load(&mut self, path: &Path, now_ms: i64) {
        let Ok(text) = std::fs::read_to_string(path) else {
            return;
        };
        let Ok(state) = serde_json::from_str::<PersistedState>(&text) else {
            return;
        };
        for (uid, persisted) in state.accounts {
            let entry = self
                .entries
                .entry(uid.clone())
                .or_insert_with(|| PoolEntry::new(uid));
            restore_entry(entry, persisted, now_ms);
        }
        self.dirty = false;
    }
}

fn persist_of(entry: &PoolEntry) -> PersistedEntry {
    PersistedEntry {
        realm: entry.realm,
        nickname: entry.nickname.clone(),
        credits: entry.credits,
        credits_expiring: entry.credits_expiring,
        credits_refreshed_ms: entry.credits_refreshed_ms,
        disabled: entry.disabled,
        reason: entry.reason.clone(),
        until_ms: entry.until_ms,
        cool_kind: entry.cool_kind,
        soft_streak: entry.soft_streak,
        session_dead_fails: entry.session_dead_fails,
        success_count: entry.success_count,
        err_total: entry.err_total,
        success_ema: entry.success_ema,
        error_ema: entry.error_ema,
        last_success_ms: entry.last_success_ms,
        last_err_ms: entry.last_err_ms,
        breaker_until_ms: entry.breaker_until_ms,
        retry_count: entry.retry_count,
        model_cooldowns: entry.model_cooldowns.clone(),
    }
}

/// 恢复单条状态（对照参考实现 `applyAccountsLocked`）。
///
/// 关键规则：
/// - `credits_expiring` 钳到 `[0, credits]`（脏数据防御）；
/// - 两个 EMA 都是 0 但计数非 0 → 由计数反推，避免统计口径丢失；
/// - **熔断截止只在未来时恢复**（同时恢复 `retry_count`，否则归零）——
///   已过期的熔断不应让退避指数继续放大；
/// - **模型级冷却只保留未过期的条目**，全空则清空。
fn restore_entry(entry: &mut PoolEntry, persisted: PersistedEntry, now_ms: i64) {
    entry.realm = persisted.realm.or(entry.realm);
    if !persisted.nickname.is_empty() {
        entry.nickname = persisted.nickname;
    }
    entry.credits = persisted.credits.max(0);
    entry.credits_expiring = persisted.credits_expiring.clamp(0, entry.credits);
    entry.credits_refreshed_ms = persisted.credits_refreshed_ms;
    entry.disabled = persisted.disabled;
    entry.reason = persisted.reason;
    entry.until_ms = persisted.until_ms;
    entry.cool_kind = persisted.cool_kind;
    entry.soft_streak = persisted.soft_streak;
    entry.session_dead_fails = persisted.session_dead_fails;
    entry.success_count = persisted.success_count;
    entry.err_total = persisted.err_total;
    entry.last_success_ms = persisted.last_success_ms;
    entry.last_err_ms = persisted.last_err_ms;

    let both_zero = persisted.success_ema == 0.0 && persisted.error_ema == 0.0;
    let total = persisted.success_count + persisted.err_total;
    if both_zero && total > 0 {
        entry.success_ema = persisted.success_count as f64 / total as f64;
        entry.error_ema = persisted.err_total as f64 / total as f64;
    } else {
        entry.success_ema = persisted.success_ema;
        entry.error_ema = persisted.error_ema;
    }

    if persisted.breaker_until_ms > now_ms {
        entry.breaker_until_ms = persisted.breaker_until_ms;
        entry.retry_count = persisted.retry_count;
    } else {
        entry.breaker_until_ms = 0;
        entry.retry_count = 0;
    }

    entry.model_cooldowns = persisted
        .model_cooldowns
        .into_iter()
        .filter(|(_, cooldown)| cooldown.until_ms > now_ms)
        .collect();

    // 进程内状态：重启即清零，避免「重启前占着在途」永久阻塞。
    entry.fails = 0;
    entry.in_flight = 0;
    entry.in_flight_until_ms = 0;
}

#[cfg(unix)]
fn restrict_permissions(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
}

#[cfg(not(unix))]
fn restrict_permissions(_path: &Path) {
    // Windows 上 0600 无对应语义；凭据不外泄由「只写本机用户目录」保证。
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_700_000_000_000;

    /// 真实上游限流响应原文 —— **逐字取自本机客户端落盘数据**（2026-09-28 复核）。
    ///
    /// ⚠️ 这两个模板**都真实存在**，不是编造的 fixture：
    /// - 国际版（`~/.workbuddy-ai/projects/**/*.jsonl`）：**英文**模板；
    /// - 国内版（`~/.workbuddy/projects/**/*.jsonl`）：**中文**模板。
    ///
    /// ⇒ 解析**只能锚「时间戳 + UTC+8」形态**，不能锚周边文案（中英两套 + 未来还会漂移）。
    /// 详见 `parse_reset_at_ms` 的文本分支。
    const REAL_GLOBAL_BODY: &str = r#"{"code":6004,"message":"429 usage exceeds frequency limit, but don't worry, your usage will reset at 2026-09-16 16:34:24 UTC+8, alternatively, you can switch to the other models to continue using it. (96df0f39955b4b5db7f4ba9d6df1ab90/1d05a3d1-c8c1-4f0d-9c33-1234567890ab)"}"#;
    const REAL_CN_BODY: &str = r#"{"code":6004,"message":"429 您的使用量已超出频率限制，将在 2026-09-17 14:18:44 UTC+8 重置，您也可以切换其他模型继续使用。 (a5684f88f6602e56/082521ff-3f6e-4f55-b75a-55608b2ac493)"}"#;

    fn temp_path(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!("wb-pool-{tag}-{}", uuid::Uuid::new_v4()))
    }

    /// ★★ **WorkBuddy 选号基线被冻结**（P5-ARCH §2.2 / R4 / 验收 10）。
    ///
    /// 两个字段是 **WorkBuddy 的行为基线**，改它们会**静默**改变 WB 的选号结果
    /// （无编译错误、无日志、无测试失败 —— 直到有人发现「选中的账号变了」）：
    ///
    /// - `expiry_urgency_weight = 0.0`：WB 的紧迫度已由 `expiring_weight` 表达，
    ///   独立紧迫度因子**不启用**；
    /// - `deterministic = false`：WB 走**有界随机**（短名单内加权抽签），
    ///   改成 `true` 会退化为严格择优，同样静默改变选号。
    ///
    /// Trae 需要「紧迫度权重 > 积分项上限（10）」与「严格择优」，它**必须**通过自己的
    /// `trae::pool::trae_pick_policy()` **显式覆写** —— **不得**靠改这里的默认值。
    ///
    /// 可证伪（变异验证 ②）：把 `PoolConfig::default()` 的任一字段改掉（或让 `Pool::policy()`
    /// 不再透传它），本用例立即变红。
    #[test]
    fn workbuddy_pick_policy_defaults_are_a_frozen_baseline() {
        let config = PoolConfig::default();
        assert_eq!(
            config.expiry_urgency_weight, 0.0,
            "WorkBuddy 基线：独立紧迫度因子默认必须为 0.0；Trae 须用 trae_pick_policy() 覆写，不得改默认值"
        );
        assert!(
            !config.deterministic,
            "WorkBuddy 基线：默认必须走有界随机；Trae 须用 trae_pick_policy() 覆写，不得改默认值"
        );

        // 默认值必须**原样透传**到实际使用的 `PickPolicy`（否则默认值改了、策略没跟上）。
        let pool = Pool::new(PoolConfig::default());
        let policy = pool.policy();
        assert_eq!(
            policy.expiry_urgency_weight, 0.0,
            "Pool::policy() 未把默认的 expiry_urgency_weight 透传"
        );
        assert!(
            !policy.deterministic,
            "Pool::policy() 未把默认的 deterministic 透传"
        );
    }

    #[test]
    fn config_normalization_clamps_invalid_values() {
        let config = PoolConfig {
            max_in_flight: -1,
            max_in_flight_global: 0,
            breaker_threshold: 0,
            idle_weight_per_hour: -1.0,
            idle_weight_max: 0.0,
            soft_rate_ms: 0,
            soft_rate_max_ms: -5,
            expiring_weight: 0.0,
            min_pick_gap_ms: -10,
            model_cost_ttl_ms: 0,
            session_dead_threshold: -1,
            ..PoolConfig::default()
        }
        .normalized();

        assert_eq!(config.max_in_flight, 0, "负值归 0（=不限）");
        assert_eq!(
            config.max_in_flight_global, 2,
            "0 必须归一到 2，无「不限」语义"
        );
        assert_eq!(config.breaker_threshold, 3);
        assert_eq!(config.idle_weight_per_hour, 0.5);
        assert_eq!(config.idle_weight_max, 5.0);
        assert_eq!(config.soft_rate_ms, 600 * 1000);
        assert_eq!(config.soft_rate_max_ms, entry::DEFAULT_SOFT_RATE_MAX_MS);
        assert_eq!(config.expiring_weight, 8.0);
        assert_eq!(config.min_pick_gap_ms, 100);
        assert_eq!(config.model_cost_ttl_ms, 6 * 60 * 60 * 1000);
        assert_eq!(config.session_dead_threshold, 3);
    }

    #[test]
    fn classify_event_covers_all_paths() {
        assert_eq!(classify_event(402, ""), UpstreamEvent::HardCredit);
        assert_eq!(
            classify_event(200, "余额不足，请充值"),
            UpstreamEvent::HardCredit
        );
        assert_eq!(
            classify_event(200, "Offline user session not found"),
            UpstreamEvent::SessionDead
        );
        assert_eq!(classify_event(404, "not found"), UpstreamEvent::NotFound);
        assert_eq!(classify_event(503, "boom"), UpstreamEvent::Server);
        assert_eq!(classify_event(400, "bad"), UpstreamEvent::Client);
        // ★ issue #100：401/403 = **凭据被拒**，不是「调用方错误」。
        //   变异：删掉 classify_event 里那条 401/403 分支 ⇒ 下面两条红，
        //   且真实后果是「池里有健康号也不换、死号继续显示可路由」。
        assert_eq!(
            classify_event(401, r#"{"error":{"message":"invalid token"}}"#),
            UpstreamEvent::AuthRejected
        );
        assert_eq!(
            classify_event(403, "forbidden"),
            UpstreamEvent::AuthRejected
        );
        // 带 12153 标记的 401 仍走**会话失效**（三振禁用）—— 标记优先于状态码，
        // 因为「会话被作废」与「token 过期」的处置不同（前者刷新救不回来）。
        assert_eq!(
            classify_event(
                401,
                r#"{"code":12153,"msg":"Offline user session not found"}"#
            ),
            UpstreamEvent::SessionDead
        );

        match classify_event(429, "rate limited") {
            UpstreamEvent::SoftRate {
                reset_at_ms,
                model_scoped,
            } => {
                assert!(reset_at_ms.is_none());
                assert!(!model_scoped, "无 6004 标记 → 账号级");
            }
            other => panic!("应为 SoftRate，实际 {other:?}"),
        }
        // ⚠️ fixture 必须**不含** `IsModelRateLimit`：原 fixture 两个标记同时存在，
        // 无论 6004 分支是否工作 `model_scoped` 都为 true ⇒ **无法证伪**（2026-09-24 复核 A3）。
        // 这里改用真实上游原文（英文模板），它本来就不含该词。
        match classify_event(429, REAL_GLOBAL_BODY) {
            UpstreamEvent::SoftRate {
                model_scoped,
                reset_at_ms,
            } => {
                assert!(model_scoped, "真实 body 里的 6004 必须识别为模型级");
                // ★★ 契约变更（2026-10-17）：`classify_event` 不再从**文案**解冷却时刻。
                //
                // 实测：`will reset at <T> UTC+8` 是**配额窗口的重置时刻**，不是解锁时刻
                // （`client_limits` 记过一次 6/6 全错的案例）。拿它当冷却时长会把号冷掉几小时，
                // 而它十几分钟就恢复了 —— 症状是「号明明能用、网关却一直不用它」。
                //
                // 冷却改为「只信结构化字段 + 60s 下限」（`min_model_cooldown_ms`），
                // 并由「该模型下一次成功」提前清掉。文案解析仍保留，供诊断/展示用。
                assert_eq!(
                    reset_at_ms, None,
                    "文案里的 reset at 不得进冷却（它是配额窗口，不是解锁时刻）"
                );
                assert!(
                    parse_reset_at_ms(REAL_GLOBAL_BODY).is_some(),
                    "文本解析能力必须保留（诊断/展示仍要用它）"
                );
            }
            other => panic!("应为 SoftRate，实际 {other:?}"),
        }
    }

    #[test]
    fn parse_reset_at_handles_seconds_milliseconds_and_nesting() {
        // 秒级
        assert_eq!(
            parse_reset_at_ms("{\"resetTime\":1700000000}"),
            Some(1_700_000_000_000)
        );
        // 毫秒级
        assert_eq!(
            parse_reset_at_ms("{\"reset_time\":1700000000000}"),
            Some(1_700_000_000_000)
        );
        // 嵌套
        assert_eq!(
            parse_reset_at_ms("{\"data\":{\"resetAt\":1700000000}}"),
            Some(1_700_000_000_000)
        );
        // 无字段 / 非法 JSON / 非正数
        assert_eq!(parse_reset_at_ms("{}"), None);
        assert_eq!(parse_reset_at_ms("not json"), None);
        assert_eq!(parse_reset_at_ms("{\"resetTime\":0}"), None);
    }

    /// 文本兜底：上游把解冻时刻写在 `msg` 文本里，**JSON 数值键一个都没有**。
    ///
    /// 修前该用例必红（`parse_reset_at_ms` 只认 JSON 数值键 ⇒ 恒 `None`）。
    #[test]
    fn parse_reset_at_ms_reads_real_upstream_text_templates() {
        // 国际版（英文模板）
        assert_eq!(
            parse_reset_at_ms(REAL_GLOBAL_BODY),
            Some(1_789_547_664_000),
            "英文模板 `reset at <ts> UTC+8` 必须解出"
        );
        // 国内版（中文模板）
        assert_eq!(
            parse_reset_at_ms(REAL_CN_BODY),
            Some(1_789_625_924_000),
            "中文模板 `将在 <ts> UTC+8 重置` 必须解出"
        );

        // ⚠️ 时区证伪：上面两个期望值是**按 UTC+8 硬编码**的 epoch 毫秒。
        // 若把基准改成「系统本地时区」，在 CI（Ubuntu，UTC）上会得到相差 8 小时的值
        // ⇒ 本断言变红。开发机在 UTC+8 时无法本地复现该证伪，故以 CI 为准。
    }

    /// 文本兜底**不得**劫持结构化形态，也不得对无时刻的文案瞎猜。
    #[test]
    fn parse_reset_at_ms_text_branch_does_not_overreach() {
        // JSON 数值键优先，且不因文本分支存在而改变结果
        assert_eq!(
            parse_reset_at_ms("{\"resetTime\":1700000000}"),
            Some(1_700_000_000_000)
        );
        // 有日期但**没有** UTC+8 标记 ⇒ 不猜（避免把「未知」伪装成「已恢复」）
        assert_eq!(
            parse_reset_at_ms(r#"{"msg":"429 rate limited at 2026-09-16 16:34:24"}"#),
            None
        );
        // 文案漂移但没有时间戳 ⇒ 仍是 None（R4：解析失败不得静默改判）
        assert_eq!(parse_reset_at_ms(r#"{"msg":"too many requests"}"#), None);
        // 非法日期不得 panic
        assert_eq!(
            parse_reset_at_ms(r#"{"msg":"reset at 2026-13-45 99:99:99 UTC+8"}"#),
            None
        );
    }

    /// ★ issue #100：401/403 只做**浅冷却**，绝不三振禁用。
    ///
    /// 变异：把 `AuthRejected` 分支改成走 `note_session_dead` ⇒ 第二条断言红。
    /// 真实后果是「一个只是 token 过期的号被永久禁用（`disabled` 只能人工恢复）」，
    /// 而保活刷新明明能救它。
    #[test]
    fn auth_rejected_cools_briefly_and_never_disables() {
        let mut pool = Pool::new(PoolConfig::default());
        pool.upsert("u1", Some(RealmTag::Cn), "小明");

        for _ in 0..5 {
            let disabled = pool.apply_upstream_error("u1", "m", &UpstreamEvent::AuthRejected, NOW);
            assert!(!disabled, "401/403 不得因累计次数而禁用账号");
        }
        let entry = pool.get("u1").expect("账号在池里");
        assert!(!entry.disabled, "401/403 不得把账号打成终态");
        assert_eq!(entry.session_dead_fails, 0, "401/403 不计会话失效次数");
        assert!(
            entry.cool_remaining_ms(NOW) > 0,
            "必须浅冷却，否则下一轮又选中这个死号"
        );
        assert!(
            entry.cool_remaining_ms(NOW) <= AUTH_REJECT_COOLDOWN_MS,
            "冷却时长应恰为 AUTH_REJECT_COOLDOWN_MS，实际 {}",
            entry.cool_remaining_ms(NOW)
        );
        // 冷却一过就能再被选中：这是「保活刷新后自愈」的前提。
        assert_eq!(
            entry.cool_remaining_ms(NOW + AUTH_REJECT_COOLDOWN_MS + 1),
            0,
            "冷却到期后必须恢复可用"
        );
    }

    #[test]
    fn upsert_is_idempotent_and_preserves_state() {
        let mut pool = Pool::new(PoolConfig::default());
        pool.upsert("u1", Some(RealmTag::Cn), "小明");
        pool.set_credits("u1", 500, 100);
        pool.apply_upstream_error("u1", "m", &UpstreamEvent::NotFound, NOW);
        let until = pool.get("u1").unwrap().until_ms;

        pool.upsert("u1", Some(RealmTag::Cn), "小明改名");
        assert_eq!(pool.len(), 1, "重复 upsert 不得产生重复账号");
        assert_eq!(pool.get("u1").unwrap().until_ms, until, "治理状态必须保留");
        assert_eq!(pool.get("u1").unwrap().nickname, "小明改名");
    }

    #[test]
    fn sync_accounts_never_removes_existing_entries() {
        let mut pool = Pool::new(PoolConfig::default());
        pool.upsert("keep", Some(RealmTag::Cn), "保留");
        pool.sync_accounts(
            &[json!({"uid": "new", "nickname": "新号"})],
            Some(RealmTag::Cn),
        );
        assert_eq!(pool.len(), 2, "对齐是增量，不得删除既有账号");
        assert!(pool.get("keep").is_some());
        assert!(pool.get("new").is_some());
    }

    #[test]
    fn sync_accounts_skips_missing_or_empty_uid() {
        let mut pool = Pool::new(PoolConfig::default());
        pool.sync_accounts(&[json!({"nickname": "无 uid"}), json!({"uid": ""})], None);
        assert!(pool.is_empty());
    }

    #[test]
    fn set_credits_clamps_expiring_into_range() {
        let mut pool = Pool::new(PoolConfig::default());
        pool.upsert("u1", None, "");
        pool.set_credits("u1", 100, 999);
        let entry = pool.get("u1").unwrap();
        assert_eq!(entry.credits, 100);
        assert_eq!(entry.credits_expiring, 100, "快过期不得超过总量");

        pool.set_credits("u1", -50, -5);
        let entry = pool.get("u1").unwrap();
        assert_eq!(entry.credits, 0);
        assert_eq!(entry.credits_expiring, 0);
    }

    #[test]
    fn acquire_and_release_respect_in_flight_limit() {
        let mut pool = Pool::new(PoolConfig::default());
        pool.upsert("u1", Some(RealmTag::Cn), "");

        assert!(pool.acquire("u1"));
        assert!(pool.acquire("u1"));
        assert!(pool.acquire("u1"));
        assert!(!pool.acquire("u1"), "达到 max_in_flight=3 后应拒绝");

        pool.release("u1");
        assert!(pool.acquire("u1"));

        // 幂等释放不得压成负数
        for _ in 0..10 {
            pool.release("u1");
        }
        assert_eq!(pool.get("u1").unwrap().in_flight, 0);
    }

    /// 在途占满**计入** `available`，另以 `in_flight_full` 单列（PR #41）。
    #[test]
    fn summary_reports_in_flight_full_without_changing_available() {
        let mut pool = Pool::new(PoolConfig {
            max_in_flight: 1,
            ..PoolConfig::default()
        });
        pool.upsert("only", Some(RealmTag::Cn), "唯一");

        // ★ 必须用**真实** now：`acquire` 按真实时钟打 `in_flight_until_ms`，拿固定的
        //   `NOW`（2023 年）去比会立刻判成「租约已过期」，测不到占满。
        let now = crate::timeutil::now_ms();

        let idle = pool.summary(now, Some(RealmTag::Cn));
        assert_eq!(idle.available, 1);
        assert_eq!(idle.in_flight_full, 0, "没占额度时不应计满");

        assert!(pool.acquire("only"));
        let full = pool.summary(now, Some(RealmTag::Cn));
        assert_eq!(full.available, 1, "在途占满仍是「可用」账号，不是第七态");
        assert_eq!(
            full.in_flight_full, 1,
            "占满必须单列，否则文案说不出「有号却跑不了」"
        );

        pool.release("only");
        assert_eq!(pool.summary(now, Some(RealmTag::Cn)).in_flight_full, 0);
    }

    /// `in_flight_full` 与选号**同源**：认 realm 上限（国际版可能更严）。
    ///
    /// 阳性对照：若这里改成只比 `max_in_flight`（3），国际版占 1 个会漏报 ⇒ 本用例必红。
    #[test]
    fn summary_in_flight_full_honors_global_limit() {
        let mut pool = Pool::new(PoolConfig {
            max_in_flight: 3,
            max_in_flight_global: 1,
            ..PoolConfig::default()
        });
        pool.upsert("intl", Some(RealmTag::Global), "国际");

        let now = crate::timeutil::now_ms();
        assert!(pool.acquire("intl"));
        assert_eq!(
            pool.summary(now, Some(RealmTag::Global)).in_flight_full,
            1,
            "global 上限为 1 ⇒ 占 1 个就是满（不是等到 3）"
        );
        pool.release("intl");
        assert_eq!(pool.summary(now, Some(RealmTag::Global)).in_flight_full, 0);
    }

    /// 被排除的账号即便在途占满，也**不得**计入 `in_flight_full`：它已不在 `available` 里，
    /// 再计一次就破坏 `in_flight_full <= available`。
    #[test]
    fn excluded_accounts_never_count_as_in_flight_full() {
        let mut pool = Pool::new(PoolConfig {
            max_in_flight: 1,
            ..PoolConfig::default()
        });
        pool.upsert("u1", Some(RealmTag::Cn), "一号");
        pool.sync_exclusions(
            "wb:cn",
            &BTreeMap::from([("u1".to_string(), "手排".to_string())]),
        );

        let now = crate::timeutil::now_ms();
        assert!(pool.acquire("u1"));
        let summary = pool.summary(now, Some(RealmTag::Cn));
        assert_eq!(summary.available, 0);
        assert_eq!(summary.excluded, 1);
        assert_eq!(
            summary.in_flight_full, 0,
            "排除项不属于「可用但占满」：既已不在 available，就不能再计入"
        );
        assert!(summary.in_flight_full <= summary.available);
    }

    /// future 被取消时，租约仍在 `Drop` 里归还额度（这就是 in_flight 泄漏的根治点）。
    ///
    /// 阳性对照：去掉 `Drop` 里的释放 ⇒ 第二次断言看到 1 ⇒ 本用例必红。
    #[tokio::test]
    async fn in_flight_lease_returns_the_slot_when_its_future_is_cancelled() {
        let mut pool = Pool::new(PoolConfig {
            max_in_flight: 1,
            ..PoolConfig::default()
        });
        pool.upsert("u1", Some(RealmTag::Cn), "一号");
        let pool = Arc::new(RwLock::new(pool));

        // 模拟真实中断：占额度后走进永不完成的 await，再丢弃 future
        // （等价于 axum 在连接关闭时取消 handler）。
        {
            let pending = async {
                let _lease = InFlightLease::acquire(Arc::clone(&pool), "u1").await;
                std::future::pending::<()>().await;
            };
            tokio::select! {
                _ = pending => unreachable!("pending 不应完成"),
                _ = tokio::time::sleep(std::time::Duration::from_millis(10)) => {}
            }
            // ★ 这一行依赖 `#[tokio::test]` 的单线程调度 + 无竞争 `RwLock` 快路径
            //   （spawn 出来的释放任务只有在主 future 让出时才跑）。若将来改成
            //   `flavor = "multi_thread"`，删掉这个断言即可，下一段才是真正的护栏。
            assert_eq!(
                pool.read().await.get("u1").map(|entry| entry.in_flight),
                Some(1),
                "占额度后应先计入 —— 否则后面的断言测不到泄漏"
            );
        }

        // `Drop` 里 spawn 的释放任务需要让出一次调度才跑。
        tokio::task::yield_now().await;
        assert_eq!(
            pool.read().await.get("u1").map(|entry| entry.in_flight),
            Some(0),
            "future 被取消也必须归还额度"
        );
    }

    #[test]
    fn global_realm_uses_stricter_in_flight_limit() {
        let mut pool = Pool::new(PoolConfig::default());
        pool.upsert("g1", Some(RealmTag::Global), "");
        assert!(pool.acquire("g1"));
        assert!(pool.acquire("g1"));
        assert!(!pool.acquire("g1"), "国际版上限为 2");
    }

    /// ★ 在途租约**自愈**：泄漏的计数在租约到期后必须被回收。
    ///
    /// 变异：把 `in_flight_now` 改回裸 `in_flight` ⇒ 第二条断言红（账号永久选不中，
    /// 而界面仍显示「可用」—— 这正是「账号看着正常、请求一直失败」的一种）。
    #[test]
    fn leaked_in_flight_lease_self_heals_after_expiry() {
        let mut pool = Pool::new(PoolConfig::default());
        pool.upsert("u1", None, "");
        let now = crate::timeutil::now_ms();
        {
            let entry = pool.entries.get_mut("u1").unwrap();
            entry.in_flight = 3; // 正好等于 max_in_flight：泄漏场景
            entry.in_flight_until_ms = now - 1; // 租约已过期
        }
        assert_eq!(
            pool.get("u1").unwrap().in_flight_now(now),
            0,
            "过期租约必须读成 0"
        );
        assert!(
            pool.acquire("u1"),
            "过期租约必须自愈，否则账号会被永久判成「在途已满」"
        );
        // 自愈后是 1（刚 acquire 的那一个），不是 4。
        assert_eq!(pool.get("u1").unwrap().in_flight, 1);
    }

    /// ★ 模型闸门：同模型 2 个账号失败即暂停，成功即关闸。
    #[test]
    fn model_gate_pauses_after_two_accounts_and_success_closes_it() {
        let mut pool = Pool::new(PoolConfig::default());
        pool.upsert("u1", None, "");
        pool.upsert("u2", None, "");

        assert!(pool.admit_model("m", NOW).is_none(), "无闸门 ⇒ 放行");
        assert!(
            !pool.note_model_failure("m", NOW),
            "第 1 个账号失败不该开闸"
        );
        assert!(pool.admit_model("m", NOW).is_none());
        assert!(pool.note_model_failure("m", NOW), "第 2 个账号失败必须开闸");

        let block = pool.admit_model("m", NOW).expect("暂停中必须被拒");
        assert!(block.remain_ms > 0, "拒绝信息必须带剩余时长");
        assert_eq!(block.failures, 2);

        // 另一个模型不受影响（闸门按模型分家）
        assert!(pool.admit_model("other", NOW).is_none());

        pool.note_model_success("m");
        assert!(pool.admit_model("m", NOW).is_none(), "成功必须立刻关闸");
    }

    #[test]
    fn acquire_on_unknown_uid_fails() {
        let mut pool = Pool::new(PoolConfig::default());
        assert!(!pool.acquire("ghost"));
    }

    #[test]
    fn apply_error_hard_credit_cools_until_next_4am() {
        let mut pool = Pool::new(PoolConfig::default());
        pool.upsert("u1", None, "");
        pool.apply_upstream_error("u1", "m", &UpstreamEvent::HardCredit, NOW);
        let entry = pool.get("u1").unwrap();
        assert_eq!(entry.cool_kind, Some(CoolKind::Hard));
        assert_eq!(entry.until_ms, crate::timeutil::next_day_4am(NOW));
    }

    #[test]
    fn apply_error_model_scoped_rate_limit_only_blocks_that_model() {
        let mut pool = Pool::new(PoolConfig::default());
        pool.upsert("u1", None, "");
        pool.apply_upstream_error(
            "u1",
            "glm-5.2",
            &UpstreamEvent::SoftRate {
                reset_at_ms: None,
                model_scoped: true,
            },
            NOW,
        );
        let entry = pool.get("u1").unwrap();
        assert_eq!(entry.until_ms, 0, "模型级限流不得写账号级冷却");
        assert!(entry.model_cooldowns.contains_key("glm-5.2"));
    }

    #[test]
    fn apply_error_account_rate_limit_backs_off_exponentially() {
        let mut pool = Pool::new(PoolConfig::default());
        pool.upsert("u1", None, "");
        let event = UpstreamEvent::SoftRate {
            reset_at_ms: None,
            model_scoped: false,
        };

        pool.apply_upstream_error("u1", "", &event, NOW);
        let first = pool.get("u1").unwrap().until_ms - NOW;
        assert_eq!(first, 600_000, "首次退避应为基准 600s");

        // 冷却结束后再次触发 → 翻倍，但封顶 `rate_window_reset_max_ms`（15 分钟）。
        //
        // ★ 封顶从 2 小时收到 15 分钟（2026-10-17）：2 小时的依据是上游文案里的
        //   `reset at`，而它实测是配额窗口而非解锁时刻。真限流会被后续 429 继续延长，
        //   不需要一次押 2 小时 —— 押错的代价是「号恢复了却一直不用它」。
        let later = NOW + first + 1;
        pool.apply_upstream_error("u1", "", &event, later);
        assert_eq!(
            pool.get("u1").unwrap().until_ms - later,
            900_000,
            "第二次退避翻倍后必须被 15 分钟上限截断"
        );
    }

    #[test]
    fn classify_treats_11102_as_model_block_not_rate_limit() {
        assert_eq!(
            classify_event(400, "code=11102"),
            UpstreamEvent::ModelBlocked,
            "11102 是封禁，不能归到限流分支（否则走错退避公式）"
        );
        assert_eq!(
            classify_event(429, "{\"code\":11102}"),
            UpstreamEvent::ModelBlocked,
            "即使伴随 429，11102 的语义仍是封禁"
        );
        // 回归：6004 仍走限流分支
        match classify_event(429, "IsModelRateLimit") {
            UpstreamEvent::SoftRate { model_scoped, .. } => assert!(model_scoped),
            other => panic!("6004 应为 SoftRate，实际 {other:?}"),
        }
    }

    #[test]
    fn apply_error_model_block_escalates_instead_of_using_rate_limit_cap() {
        let hour = 60 * 60 * 1000;
        let mut pool = Pool::new(PoolConfig::default());
        pool.upsert("u1", None, "");

        pool.apply_upstream_error("u1", "glm-5.2", &UpstreamEvent::ModelBlocked, NOW);
        let entry = pool.get("u1").unwrap();
        assert_eq!(entry.until_ms, 0, "模型级封禁不得写账号级冷却");
        assert_eq!(
            entry.model_cooldowns["glm-5.2"].until_ms - NOW,
            6 * hour,
            "首次封禁应退避 6h（而不是 6004 的 2h 封顶）"
        );

        // 再次被罚 → 翻倍到 12h（「逐次加倍」是封禁独有的语义）
        let later = NOW + 7 * hour;
        pool.apply_upstream_error("u1", "glm-5.2", &UpstreamEvent::ModelBlocked, later);
        assert_eq!(
            pool.get("u1").unwrap().model_cooldowns["glm-5.2"].until_ms - later,
            12 * hour
        );
    }

    #[test]
    fn apply_error_model_block_without_model_degrades_to_account_cooldown() {
        let mut pool = Pool::new(PoolConfig::default());
        pool.upsert("u1", None, "");
        pool.apply_upstream_error("u1", "", &UpstreamEvent::ModelBlocked, NOW);
        let entry = pool.get("u1").unwrap();
        assert!(
            entry.model_cooldowns.is_empty(),
            "无模型名时不应产生模型级条目"
        );
        assert_eq!(
            entry.until_ms - NOW,
            60_000,
            "应退化为账号级浅冷却，而不是完全不处置"
        );
    }

    #[test]
    fn apply_error_session_dead_disables_at_threshold() {
        let mut pool = Pool::new(PoolConfig::default());
        pool.upsert("u1", None, "");
        let event = UpstreamEvent::SessionDead;
        assert!(!pool.apply_upstream_error("u1", "", &event, NOW));
        assert!(!pool.apply_upstream_error("u1", "", &event, NOW));
        assert!(
            pool.apply_upstream_error("u1", "", &event, NOW),
            "第三次应禁用"
        );
        assert!(pool.get("u1").unwrap().disabled);
    }

    /// ★ `revive` 是被三振禁用账号的**唯一出口**（改造前一个出口都没有）。
    ///
    /// 断言四件事：① 真能解除禁用；② 冷却域一并清空；③ **熔断历史保留**（复活不是洗白）；
    /// ④ 幂等、且不在池里返回 `false`。
    #[test]
    fn revive_is_the_only_way_out_of_a_disabled_account() {
        let mut pool = Pool::new(PoolConfig::default());
        pool.upsert("u1", None, "");

        // 先压上熔断历史（3 次 Server ⇒ 熔断），再打到禁用。
        for _ in 0..3 {
            pool.apply_upstream_error("u1", "", &UpstreamEvent::Server, NOW);
        }
        for _ in 0..3 {
            pool.apply_upstream_error("u1", "", &UpstreamEvent::SessionDead, NOW);
        }
        assert!(pool.get("u1").unwrap().disabled, "前置：应已禁用");

        let breaker = pool.get("u1").unwrap().breaker_until_ms;
        let retry = pool.get("u1").unwrap().retry_count;
        assert!(
            breaker > NOW,
            "前置：应有未过期的熔断，否则下面的断言是空转"
        );
        assert!(retry > 0, "前置：退避指数应有记录");

        assert!(pool.revive("u1"), "应报告确实解除了禁用");
        let entry = pool.get("u1").unwrap();
        assert!(!entry.disabled, "禁用必须被解除");
        assert_eq!(entry.until_ms, 0, "冷却域应一并清空");
        assert!(entry.reason.is_empty(), "禁用原因应随禁用一并清掉");
        assert_eq!(
            entry.breaker_until_ms, 0,
            "★ 熔断截止必须一并清掉：`healthy()` 也看它，否则按钮点了最长 6h 不生效"
        );
        assert_eq!(entry.retry_count, retry, "退避指数（教训）必须保留，别洗白");

        assert!(
            !pool.revive("u1"),
            "本来就可用 ⇒ 返回 false（幂等，不报错）"
        );
        assert!(!pool.revive("no-such-uid"), "不在池里 ⇒ 返回 false");
    }

    /// ★ 复活的**用户可见后果**：该账号真的重新进入选号。
    ///
    /// 只断言「`disabled` 字段被清」是不够的 —— 选号还有别的判据（冷却 / 熔断 / 在途），
    /// 而 `healthy()` 把**熔断截止**也算进去。这里直接问「能不能选到它」，
    /// 那才是用户点「恢复」想要的结果。
    ///
    /// 阳性对照：把 `PoolEntry::revive` 改成空操作 ⇒ 本用例必红（`disabled` 未清）；
    /// 只清 `disabled` 而不清 `breaker_until_ms` ⇒ 本用例也必红（`is_usable_for` 为 false）。
    ///
    /// ⚠️ **不能只断言 `pick_account`**：候选全不可用时它会回落到
    /// [`pick_earliest_expiry`]（宁可试一个也不报「没有账号」），于是「只是被回落捞出来」
    /// 与「真的可用」在它眼里**长得一样**。要判「真的可用」必须问 `is_usable_for` ——
    /// 它与选号**同一套判据**（`entry_usable`）且没有回落。
    #[test]
    fn revived_account_is_selectable_again() {
        let mut pool = Pool::new(PoolConfig::default());
        pool.upsert("u1", Some(RealmTag::Cn), "");
        // 先压上熔断，再打到禁用 —— 「禁用 + 熔断同时在」正是最容易让「恢复」按钮失效的状态。
        // ⚠️ 只有 `Server` 会触发熔断（`SessionDead` 只累计三振），故这一步不能省。
        for _ in 0..3 {
            pool.apply_upstream_error("u1", "", &UpstreamEvent::Server, NOW);
        }
        for _ in 0..3 {
            pool.apply_upstream_error("u1", "", &UpstreamEvent::SessionDead, NOW);
        }
        assert!(pool.get("u1").unwrap().disabled, "前置：应已禁用");
        assert!(
            pool.get("u1").unwrap().breaker_until_ms > NOW,
            "前置：熔断应仍在"
        );
        assert!(
            !pool.is_usable_for("u1", NOW, ""),
            "前置：被禁用的账号不可用"
        );

        assert!(pool.revive("u1"));

        assert!(
            pool.is_usable_for("u1", NOW, ""),
            "复活后必须**真的可用**（与选号同一套判据，无回落）：禁用 / 冷却 / 熔断都不得再挡"
        );
        assert_eq!(
            pool.pick_account(
                NOW,
                Some(RealmTag::Cn),
                "",
                &HashSet::new(),
                PickMode::Weighted,
                1,
                None,
                None,
            ),
            Some("u1".to_string()),
            "也应当能被选到 —— 这才是「恢复」按钮的意义"
        );
    }

    #[test]
    fn apply_error_server_trips_breaker_at_threshold() {
        let mut pool = Pool::new(PoolConfig::default());
        pool.upsert("u1", None, "");
        for _ in 0..2 {
            pool.apply_upstream_error("u1", "", &UpstreamEvent::Server, NOW);
        }
        assert_eq!(
            pool.get("u1").unwrap().breaker_until_ms,
            0,
            "未达阈值不熔断"
        );
        pool.apply_upstream_error("u1", "", &UpstreamEvent::Server, NOW);
        assert_eq!(
            pool.get("u1").unwrap().breaker_until_ms - NOW,
            30 * 60 * 1000,
            "阈值 3 触发首次 30m 熔断"
        );
    }

    #[test]
    fn note_success_records_ledger_and_deducts_credits() {
        let mut pool = Pool::new(PoolConfig::default());
        pool.upsert("u1", None, "");
        pool.set_credits("u1", 100, 50);
        pool.note_success("u1", "m", 2.0, 1000, NOW);

        let entry = pool.get("u1").unwrap();
        assert_eq!(entry.success_count, 1);
        assert_eq!(entry.credits, 98, "按上报 credit 扣减（2.0 四舍五入）");
        assert_eq!(entry.credits_expiring, 50);
        assert!(entry.model_cost.contains_key("m"));
    }

    #[test]
    fn note_success_skips_ledger_without_token_usage() {
        let mut pool = Pool::new(PoolConfig::default());
        pool.upsert("u1", None, "");
        pool.set_credits("u1", 100, 0);
        pool.note_success("u1", "m", 5.0, 0, NOW);
        let entry = pool.get("u1").unwrap();
        assert!(entry.model_cost.is_empty(), "无 token 数不写账本");
        assert_eq!(entry.credits, 100, "无 token 数不扣减");
    }

    #[test]
    fn clear_session_dead_resets_counter() {
        let mut pool = Pool::new(PoolConfig::default());
        pool.upsert("u1", None, "");
        pool.apply_upstream_error("u1", "", &UpstreamEvent::SessionDead, NOW);
        assert_eq!(pool.get("u1").unwrap().session_dead_fails, 1);
        pool.clear_session_dead("u1");
        assert_eq!(pool.get("u1").unwrap().session_dead_fails, 0);
    }

    #[test]
    fn snapshot_reports_aggregates_and_realm_totals() {
        let mut pool = Pool::new(PoolConfig::default());
        pool.upsert("cn-ok", Some(RealmTag::Cn), "正常");
        pool.upsert("cn-cool", Some(RealmTag::Cn), "冷却");
        pool.upsert("g-dead", Some(RealmTag::Global), "禁用");
        pool.apply_upstream_error("cn-cool", "", &UpstreamEvent::NotFound, NOW);
        for _ in 0..3 {
            pool.apply_upstream_error("g-dead", "", &UpstreamEvent::SessionDead, NOW);
        }

        let snapshot = pool.snapshot(NOW);
        assert_eq!(snapshot["total"], json!(3));
        assert_eq!(snapshot["healthy"], json!(1));
        assert_eq!(snapshot["cooling"], json!(1));
        assert_eq!(snapshot["disabled"], json!(1));
        assert_eq!(snapshot["realm_totals"]["cn"]["total"], json!(2));
        assert_eq!(snapshot["realm_totals"]["cn"]["disabled"], json!(0));
        assert_eq!(snapshot["realm_totals"]["global"]["disabled"], json!(1));

        let accounts = snapshot["accounts"].as_array().expect("数组");
        let dead = accounts
            .iter()
            .find(|item| item["uid"] == json!("g-dead"))
            .expect("存在");
        assert_eq!(dead["disabled_reason"], json!("12153 session dead"));
    }

    /// `snapshot()` 的 `in_flight_full` 必须与选号同源：**认 realm 上限**。
    ///
    /// 阳性对照：此处若自己写 `in_flight_now() >= max_in_flight`（只有 3），
    /// 国际版占 1 个（上限 2）会被判成「没满」 ⇒ 第一条断言必红。这正是修复前的形态：
    /// `/status` 说没占满、选号却已经挑不出号。
    #[test]
    fn snapshot_in_flight_full_honors_the_global_limit() {
        let mut pool = Pool::new(PoolConfig {
            max_in_flight: 3,
            max_in_flight_global: 1,
            ..PoolConfig::default()
        });
        pool.upsert("intl", Some(RealmTag::Global), "国际");
        pool.upsert("cn", Some(RealmTag::Cn), "国内");

        // `acquire` 按**真实**时钟打租约，因此快照也必须给真实 now（用 `NOW` 会判成已过期）。
        let now = crate::timeutil::now_ms();
        assert_eq!(pool.snapshot(now)["in_flight_full"], json!(0));

        assert!(pool.acquire("intl"));
        assert_eq!(
            pool.snapshot(now)["in_flight_full"],
            json!(1),
            "global 上限为 1 ⇒ 占 1 个就该报「满」，而不是等到 3"
        );

        // 国内账号按 3 的上限：占 1 个不算满。
        assert!(pool.acquire("cn"));
        assert_eq!(
            pool.snapshot(now)["in_flight_full"],
            json!(1),
            "国内的 1/3 不该被算成满"
        );
    }

    /// 五态计数：**与 Trae 侧 `summary` 同形状**（管理面共用一张「账号池」卡）。
    #[test]
    fn summary_counts_every_state_once_and_filters_by_realm() {
        let mut pool = Pool::new(PoolConfig::default());
        pool.upsert("cn-ok", Some(RealmTag::Cn), "正常");
        pool.upsert("cn-cool", Some(RealmTag::Cn), "冷却");
        pool.upsert("g-dead", Some(RealmTag::Global), "会话失效");
        pool.upsert("g-zero", Some(RealmTag::Global), "零积分");

        pool.set_credits("cn-ok", 30, 0);
        pool.mark_credits_refreshed("cn-ok", NOW);
        pool.set_credits("cn-cool", 5, 0);
        pool.mark_credits_refreshed("cn-cool", NOW);
        pool.apply_upstream_error(
            "cn-cool",
            "",
            &UpstreamEvent::SoftRate {
                reset_at_ms: None,
                model_scoped: false,
            },
            NOW,
        );
        for _ in 0..3 {
            pool.apply_upstream_error("g-dead", "", &UpstreamEvent::SessionDead, NOW);
        }
        pool.set_credits("g-zero", 0, 0);
        pool.mark_credits_refreshed("g-zero", NOW);

        let cn = pool.summary(NOW, Some(RealmTag::Cn));
        assert_eq!(cn.total, 2);
        assert_eq!(cn.available, 1, "未冷却且有余额的账号可路由");
        assert_eq!(cn.cooling, 1);
        assert_eq!(cn.disabled, 0);
        assert_eq!(cn.zero_credits, 0);
        // ★ 本侧**没有**积分有效期判据 ⇒ 恒 0，绝不臆造（见 `PoolEntry::status_of`）。
        assert_eq!(cn.expired, 0);
        assert_eq!(cn.total_credits, 35.0);

        let global = pool.summary(NOW, Some(RealmTag::Global));
        assert_eq!(global.total, 2);
        assert_eq!(global.disabled, 1);
        assert_eq!(global.zero_credits, 1);
        assert_eq!(global.available, 0);

        // `None` = 全池：两个域之和。
        let all = pool.summary(NOW, None);
        assert_eq!(all.total, 4);
        assert_eq!(all.available, 1);
        assert_eq!(all.cooling, 1);
        assert_eq!(all.disabled, 1);
        assert_eq!(all.zero_credits, 1);
    }

    /// issue #76：**名单口径**只数这把 Key 允许用的号。
    ///
    /// 阳性对照：把 `summary_of` 的过滤去掉（退化成 `summary`）⇒ 第一组
    /// `total` / `available` 断言必红 —— 这正是「白名单用户看到 12 个号都不可用」
    /// 那个误导的根。
    #[test]
    fn summary_of_counts_only_allowlisted_accounts() {
        let mut pool = Pool::new(PoolConfig::default());
        pool.upsert("a", Some(RealmTag::Cn), "甲");
        pool.upsert("b", Some(RealmTag::Cn), "乙");
        pool.upsert("c", Some(RealmTag::Cn), "丙");
        for uid in ["a", "b", "c"] {
            pool.set_credits(uid, 10, 0);
            pool.mark_credits_refreshed(uid, NOW);
        }

        let full = pool.summary(NOW, Some(RealmTag::Cn));
        assert_eq!(full.total, 3, "全池口径不变（管理面卡片仍要看全池）");

        let only: HashSet<String> = ["a".to_string(), "c".to_string()].into_iter().collect();
        let scoped = pool.summary_of(&only, NOW, Some(RealmTag::Cn));
        assert_eq!(scoped.total, 2, "名单外的 b 不得计入 total");
        assert_eq!(scoped.available, 2);
        assert_eq!(scoped.total_credits, 20.0, "总积分也要按名单口径");
        // 六态不变量在名单口径下同样成立（否则文案里的分类数字会对不上）。
        assert_eq!(
            scoped.available
                + scoped.cooling
                + scoped.disabled
                + scoped.expired
                + scoped.zero_credits
                + scoped.excluded,
            scoped.total,
            "名单口径同样要满足六态不变量"
        );

        // 名单里一个都不在本区域池 ⇒ total 0（上层据此说「名单没覆盖本区域」）。
        let missing: HashSet<String> = ["zzz".to_string()].into_iter().collect();
        assert_eq!(pool.summary_of(&missing, NOW, Some(RealmTag::Cn)).total, 0);

        // 名单口径仍要按 realm 过滤：跨区域的 uid 不进这一本账。
        let cross: HashSet<String> = ["a".to_string(), "zzz".to_string()].into_iter().collect();
        assert_eq!(pool.summary_of(&cross, NOW, Some(RealmTag::Cn)).total, 1);
    }

    /// 余额**未知**（从未刷新）不得被算成零积分，也不得计入总积分。
    #[test]
    fn summary_treats_unknown_credits_as_available_not_zero() {
        let mut pool = Pool::new(PoolConfig::default());
        pool.upsert("u1", Some(RealmTag::Cn), "从未刷新");

        let summary = pool.summary(NOW, Some(RealmTag::Cn));
        assert_eq!(summary.available, 1, "余额未知 ⇒ 可路由，不算零积分");
        assert_eq!(summary.zero_credits, 0);
        assert_eq!(summary.total_credits, 0.0, "未知余额不计入总额");
    }

    #[test]
    fn status_list_uses_the_shape_shared_with_trae() {
        let mut pool = Pool::new(PoolConfig::default());
        pool.upsert("u1", Some(RealmTag::Cn), "一号");
        pool.set_credits("u1", 12, 0);
        pool.mark_credits_refreshed("u1", NOW);

        let list = pool.status_list(NOW);
        let object = list[0].as_object().expect("对象");
        assert_eq!(object["uid"], json!("u1"));
        assert_eq!(object["name"], json!("一号"));
        assert_eq!(object["realm"], json!("cn"));
        assert_eq!(object["status"], json!("available"));
        assert_eq!(object["credits"], json!(12.0));
        assert_eq!(object["disabled"], json!(false));
        // 与 Trae 侧同形 ⇒ 前端共用一张卡：键名必须是 camelCase 那套。
        assert!(object.contains_key("cooldownUntil"));
        assert!(object.contains_key("cooldownReason"));
        assert!(
            !object.contains_key("cooldown_until"),
            "不得出现 snake_case"
        );
        assert!(!object.contains_key("nickname"), "展示名统一叫 name");
        // 无冷却 / 无原因 ⇒ `null`，前端据此不显示。
        assert_eq!(object["cooldownUntil"], Value::Null);
        assert_eq!(object["cooldownReason"], Value::Null);
    }

    /// ★ 模型级冷却（`modelCoolings`）必须随 `status_list` **外发**，且**只发仍有效**的。
    ///
    /// 背景：前端「限流探测」矩阵的**被动底色** = 池冷却 + 客户端台账（`client_limits`）。
    /// 池的模型级冷却此前**不在**给前端的池视图里 ⇒ 被真实流量冷掉的模型在矩阵里恒显示「未知」。
    ///
    /// 变异：① 去掉 `until_ms > now_ms` 过滤 ⇒ 第三条断言红（过期冷却被画成「此刻仍冷」）；
    ///       ② 键名改成 snake_case（`model_coolings`）⇒ 末两条断言红，且违反
    ///          `status_list_uses_the_shape_shared_with_trae` 的「不得出现 snake_case」。
    #[test]
    fn status_list_reports_active_model_coolings() {
        let mut pool = Pool::new(PoolConfig::default());
        pool.upsert("u1", Some(RealmTag::Cn), "一号");

        // 无模型冷却 ⇒ 键**恒存在**且为空对象（前端不必判 `undefined`）。
        let empty = pool.status_list(NOW)[0]["modelCoolings"].clone();
        assert!(
            empty.is_object() && empty.as_object().unwrap().is_empty(),
            "无冷却时 modelCoolings 应为空对象：{empty}"
        );

        // 11102 模型封禁 ⇒ **该模型**进冷却（账号级 `until` 不动）。
        pool.apply_upstream_error("u1", "glm-5.2", &UpstreamEvent::ModelBlocked, NOW);
        let until = pool.get("u1").unwrap().model_cooldowns["glm-5.2"].until_ms;
        assert!(
            until > NOW,
            "前提：封禁必须落在未来（否则下面的阳性对照无效）"
        );

        let row = pool.status_list(NOW)[0].clone();
        assert_eq!(
            row["modelCoolings"]["glm-5.2"],
            json!(until),
            "有效模型冷却必须外发（值 = 解冻时刻 ms）"
        );
        let object = row.as_object().expect("对象");
        assert!(object.contains_key("modelCoolings"), "键名必须是 camelCase");
        assert!(
            !object.contains_key("model_coolings"),
            "不得出现 snake_case（与 Trae 侧共用一张前端卡）"
        );

        // 墙钟走过解冻时刻 ⇒ 不再外发（过期残留没人清，照搬就是假「此刻仍冷」）。
        let expired = pool.status_list(until + 1)[0].clone();
        assert!(
            expired["modelCoolings"].as_object().unwrap().is_empty(),
            "过期模型冷却不得外发：{expired}"
        );
    }

    /// 冷却中的账号要带上**解冻时刻与原因**，否则用户不知道要等多久。
    #[test]
    fn status_list_reports_cooldown_until_and_reason() {
        let mut pool = Pool::new(PoolConfig::default());
        pool.upsert("u1", Some(RealmTag::Cn), "一号");
        pool.apply_upstream_error(
            "u1",
            "",
            &UpstreamEvent::SoftRate {
                reset_at_ms: None,
                model_scoped: false,
            },
            NOW,
        );

        let object = pool.status_list(NOW)[0].as_object().expect("对象").clone();
        assert_eq!(object["status"], json!("cooling"));
        assert!(
            object["cooldownUntil"].as_i64().unwrap_or(0) > NOW,
            "有解冻时刻"
        );
        assert!(
            !object["cooldownReason"].as_str().unwrap_or("").is_empty(),
            "有冷却原因"
        );
    }

    /// 冷却**自然到期**后，`cooldownUntil` / `cooldownReason` 必须不再外发。
    ///
    /// 回归：`reason` 没有任何人在墙钟到期时清掉 ⇒ 一条两天前的 `429 rate limit`
    /// 会被当成**当前**原因复述出去，前端渲染出「徽标=可用、旁边写着 429」的行
    /// （用户症状：「这个号在网关里是冷却的，可我在客户端明明能用」）。
    #[test]
    fn status_list_drops_cooldown_fields_once_the_cooldown_expires() {
        let mut pool = Pool::new(PoolConfig::default());
        pool.upsert("u1", Some(RealmTag::Cn), "一号");
        // `reset_at` 落在过去 ⇒ `until = NOW + 1`，即「1ms 后自然到期」。
        pool.apply_upstream_error(
            "u1",
            "",
            &UpstreamEvent::SoftRate {
                reset_at_ms: Some(NOW - 10_000),
                model_scoped: false,
            },
            NOW,
        );

        // 阳性对照：**冷却中**必须照常给出解冻时刻与原因，否则下面的断言可能是「永远绿」。
        let cooling = pool.status_list(NOW)[0].as_object().expect("对象").clone();
        assert_eq!(cooling["status"], json!("cooling"));
        assert!(
            cooling["cooldownUntil"].as_i64().unwrap_or(0) > NOW,
            "冷却中必须有解冻时刻"
        );
        assert!(
            !cooling["cooldownReason"].as_str().unwrap_or("").is_empty(),
            "冷却中必须有原因"
        );

        // 墙钟走过 1ms ⇒ 冷却自然到期；`reason` 字段仍留在条目里，但**不得外发**。
        let expired = pool.status_list(NOW + 2)[0]
            .as_object()
            .expect("对象")
            .clone();
        assert_eq!(expired["status"], json!("available"), "墙钟已过 ⇒ 判可用");
        assert_eq!(
            expired["cooldownUntil"],
            Value::Null,
            "到期后不得再给解冻时刻"
        );
        assert_eq!(
            expired["cooldownReason"],
            Value::Null,
            "到期后不得复述陈旧原因"
        );
    }

    /// ★ 回归护栏：**禁用**账号的原因必须照常给出。
    ///
    /// `reason` 是**双语义字段**（冷却原因 / 禁用原因共用，见 `PoolEntry::disable` 的注释），
    /// 而 `status_list` **没有** `disabled_reason` 这个独立出口（`snapshot` 才有）。
    /// ⇒ 「只按 `cooling` 门控」会把禁用原因**一起藏掉**：徽标写着「已禁用」，
    /// 却不告诉用户为什么，用户只能去猜。判据必须是 `cooling || disabled`。
    #[test]
    fn status_list_keeps_the_reason_for_disabled_accounts() {
        let mut pool = Pool::new(PoolConfig::default());
        pool.upsert("u1", Some(RealmTag::Cn), "会话失效");
        for _ in 0..3 {
            pool.apply_upstream_error("u1", "", &UpstreamEvent::SessionDead, NOW);
        }

        // 先确认前提：这确实是「禁用」而不是「冷却」，否则下面的断言打的不是同一个分支。
        {
            let dead = pool.entries.get("u1").expect("条目");
            assert!(dead.disabled, "连续 3 次 SessionDead 必须禁用");
            assert_eq!(dead.status_of(NOW), "disabled");
        }

        let object = pool.status_list(NOW)[0].as_object().expect("对象").clone();
        assert_eq!(object["status"], json!("disabled"));
        assert_eq!(
            object["cooldownReason"],
            json!("12153 session dead"),
            "禁用原因（与冷却原因共用 `reason`）不得被门控藏掉"
        );
        assert_eq!(
            object["cooldownUntil"],
            Value::Null,
            "禁用是终态，没有「解冻时刻」可给"
        );
    }

    /// 网关 `/status` 出口（`snapshot`）同样不得复述**已到期**的冷却残留。
    #[test]
    fn snapshot_drops_cooldown_fields_once_the_cooldown_expires() {
        let mut pool = Pool::new(PoolConfig::default());
        pool.upsert("u1", Some(RealmTag::Cn), "一号");
        pool.apply_upstream_error(
            "u1",
            "",
            &UpstreamEvent::SoftRate {
                reset_at_ms: Some(NOW - 10_000),
                model_scoped: false,
            },
            NOW,
        );

        let cooling = pool.snapshot(NOW)["accounts"][0].clone();
        assert_eq!(cooling["cooling"], json!(true));
        assert!(
            cooling["reason"].is_string(),
            "冷却中必须给原因（阳性对照）"
        );
        assert!(cooling["until_ms"].as_i64().unwrap_or(0) > NOW);
        assert_eq!(cooling["cool_kind"], json!("soft"));

        let expired = pool.snapshot(NOW + 2)["accounts"][0].clone();
        assert_eq!(expired["cooling"], json!(false));
        assert!(expired["reason"].is_null(), "到期后不得复述陈旧原因");
        assert!(expired["until_ms"].is_null(), "到期后不得给解冻时刻");
        assert!(
            expired["cool_kind"].is_null(),
            "到期后不得给冷却类型（它不是「当前状态」，是残留）"
        );
    }

    /// 调用方错误（其它 4xx）在池这一层：诊断计数照记，**不污染账号健康度**。
    ///
    /// 回归对象：`apply_upstream_error` 曾**无条件** `note_error` —— 而 `Client` 分支
    /// 明确不做任何治理动作。判定「不是账号的问题」却又扣它的分，是自相矛盾的，
    /// 后果是健康账号的 `error_ema` 被别人的坏请求一路拖到 0.9。
    #[test]
    fn client_errors_are_recorded_without_touching_account_health() {
        let mut pool = Pool::new(PoolConfig::default());
        pool.upsert("u1", Some(RealmTag::Cn), "一号");
        let before = {
            let entry = pool.entries.get("u1").expect("条目");
            (
                entry.success_ema,
                entry.error_ema,
                entry.err_total,
                entry.fails,
            )
        };

        let disabled = pool.apply_upstream_error("u1", "", &UpstreamEvent::Client, NOW);
        assert!(!disabled, "调用方错误不做治理动作");
        {
            let entry = pool.entries.get("u1").expect("条目");
            assert_eq!(entry.err_total, before.2 + 1, "错误数照记（诊断用）");
            assert_eq!(entry.success_ema, before.0, "不得动成功率");
            assert_eq!(entry.error_ema, before.1, "不得动错误率");
            assert_eq!(entry.fails, before.3, "不得推进熔断计数");
        }

        // 阳性对照：换账号侧错误（5xx），EMA 必须动 —— 否则上面的断言可能是「永远绿」。
        pool.apply_upstream_error("u1", "", &UpstreamEvent::Server, NOW);
        {
            let entry = pool.entries.get("u1").expect("条目");
            assert!(
                entry.error_ema > before.1,
                "账号侧错误必须推进错误率（阳性对照）"
            );
            assert!(entry.fails > before.3, "账号侧错误必须推进熔断计数");
        }
    }

    /// 余额未知 ⇒ `credits: null`（与 Trae 侧同义），前端显示为「—」。
    #[test]
    fn status_list_marks_unknown_credits_as_null() {
        let mut pool = Pool::new(PoolConfig::default());
        pool.upsert("u1", Some(RealmTag::Cn), "一号");
        let object = pool.status_list(NOW)[0].as_object().expect("对象").clone();
        assert_eq!(object["credits"], Value::Null);
    }

    #[test]
    fn diagnose_reports_reason_for_every_entry() {
        let mut pool = Pool::new(PoolConfig::default());
        pool.upsert("u-ok", Some(RealmTag::Cn), "正常");
        pool.set_credits("u-ok", 9, 0);
        pool.mark_credits_refreshed("u-ok", NOW);
        pool.upsert("u-dead", Some(RealmTag::Cn), "会话失效");
        for _ in 0..3 {
            pool.apply_upstream_error("u-dead", "", &UpstreamEvent::SessionDead, NOW);
        }

        let lines = pool.diagnose(NOW);
        assert_eq!(lines.len(), 2, "每个账号一行，一个都不能漏");
        let ok = lines
            .iter()
            .find(|line| line.contains("正常"))
            .expect("存在");
        assert!(ok.contains("可用"), "{ok}");
        assert!(ok.contains("积分=9"), "{ok}");
        let dead = lines
            .iter()
            .find(|line| line.contains("会话失效"))
            .expect("存在");
        assert!(
            dead.contains("12153 session dead"),
            "复述后端写入的具体原因（状态名不够排查）: {dead}"
        );
    }

    #[test]
    fn snapshot_exposes_rate_limited_models() {
        let mut pool = Pool::new(PoolConfig::default());
        pool.upsert("u1", None, "");
        pool.apply_upstream_error(
            "u1",
            "glm-5.2",
            &UpstreamEvent::SoftRate {
                reset_at_ms: None,
                model_scoped: true,
            },
            NOW,
        );
        let snapshot = pool.snapshot(NOW);
        let limited = &snapshot["accounts"][0]["rate_limited_models"];
        assert_eq!(limited[0]["model"], json!("glm-5.2"));
    }

    #[test]
    fn persistence_round_trip_restores_governance_state() {
        let path = temp_path("roundtrip");

        let mut pool = Pool::new(PoolConfig::default());
        pool.upsert("u1", Some(RealmTag::Cn), "小明");
        pool.set_credits("u1", 900, 300);
        pool.apply_upstream_error("u1", "", &UpstreamEvent::NotFound, NOW);
        pool.note_success("u1", "m", 1.0, 1000, NOW);
        let expected_until = pool.get("u1").unwrap().until_ms;
        pool.save(&path).expect("落盘成功");
        assert!(!pool.is_dirty());

        let mut restored = Pool::new(PoolConfig::default());
        restored.load(&path, NOW);
        let entry = restored.get("u1").expect("恢复成功");
        assert_eq!(entry.credits, 899, "扣减后的余额应持久化");
        assert_eq!(entry.credits_expiring, 300);
        assert_eq!(entry.until_ms, expected_until, "冷却截止应持久化");
        assert_eq!(entry.nickname, "小明");
        assert_eq!(entry.success_count, 1);
        assert!(entry.success_ema > 0.0);
        assert_eq!(entry.fails, 0, "fails 不持久化，恢复即清零");
        assert_eq!(entry.in_flight, 0, "in_flight 不持久化");
        assert!(entry.model_cost.is_empty(), "账本不持久化（重启重学）");

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn save_is_atomic_and_creates_missing_directories() {
        let dir = temp_path("atomic");
        let path = dir.join("nested").join("state.json");
        let mut pool = Pool::new(PoolConfig::default());
        pool.upsert("u1", None, "");
        pool.save(&path).expect("应自动创建父目录");
        assert!(path.exists());
        assert!(
            !path.with_extension("json.tmp").exists(),
            "临时文件必须已被 rename 掉"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_ignores_missing_or_corrupt_files() {
        let mut pool = Pool::new(PoolConfig::default());
        pool.load(&temp_path("missing"), NOW);
        assert!(pool.is_empty(), "缺失文件不应 panic 也不应伪造账号");

        let corrupt = temp_path("corrupt");
        std::fs::write(&corrupt, b"{ not json").expect("写入");
        let mut pool = Pool::new(PoolConfig::default());
        pool.load(&corrupt, NOW);
        assert!(pool.is_empty());
        let _ = std::fs::remove_file(&corrupt);
    }

    #[test]
    fn restore_drops_expired_breaker_and_model_cooldowns() {
        let path = temp_path("expired");
        let mut pool = Pool::new(PoolConfig::default());
        pool.upsert("u1", None, "");
        // 构造：熔断与模型冷却都设在过去
        {
            let entry = pool.entries.get_mut("u1").unwrap();
            entry.breaker_until_ms = NOW - 1000;
            entry.retry_count = 3;
            entry.cooldown_model(NOW - 5000, "m", 0, 1000, "6004");
        }
        pool.save(&path).expect("落盘");

        let mut restored = Pool::new(PoolConfig::default());
        restored.load(&path, NOW);
        let entry = restored.get("u1").unwrap();
        assert_eq!(entry.breaker_until_ms, 0, "过期熔断不得恢复");
        assert_eq!(entry.retry_count, 0, "退避指数必须随之归零");
        assert!(entry.model_cooldowns.is_empty(), "过期模型冷却不得恢复");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn restore_keeps_future_breaker_with_retry_count() {
        let path = temp_path("future");
        let mut pool = Pool::new(PoolConfig::default());
        pool.upsert("u1", None, "");
        {
            let entry = pool.entries.get_mut("u1").unwrap();
            entry.breaker_until_ms = NOW + 60_000;
            entry.retry_count = 2;
        }
        pool.save(&path).expect("落盘");

        let mut restored = Pool::new(PoolConfig::default());
        restored.load(&path, NOW);
        let entry = restored.get("u1").unwrap();
        assert_eq!(entry.breaker_until_ms, NOW + 60_000);
        assert_eq!(entry.retry_count, 2, "未过期熔断必须连退避指数一起恢复");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn restore_derives_ema_from_counts_when_missing() {
        let mut entry = PoolEntry::new("u1");
        let persisted = PersistedEntry {
            success_count: 3,
            err_total: 1,
            success_ema: 0.0,
            error_ema: 0.0,
            ..PersistedEntry::default()
        };
        restore_entry(&mut entry, persisted, NOW);
        assert!((entry.success_ema - 0.75).abs() < 1e-9, "3/(3+1)=0.75");
        assert!((entry.error_ema - 0.25).abs() < 1e-9);
    }

    #[test]
    fn restore_clamps_expiring_and_keeps_explicit_ema() {
        let mut entry = PoolEntry::new("u1");
        let persisted = PersistedEntry {
            credits: 10,
            credits_expiring: 99,
            success_ema: 0.4,
            error_ema: 0.2,
            success_count: 5,
            err_total: 5,
            ..PersistedEntry::default()
        };
        restore_entry(&mut entry, persisted, NOW);
        assert_eq!(entry.credits_expiring, 10, "快过期必须钳到总量");
        assert!(
            (entry.success_ema - 0.4).abs() < 1e-9,
            "已有 EMA 不得被计数覆盖"
        );
    }

    #[test]
    fn flush_if_dirty_only_writes_when_needed() {
        let path = temp_path("flush");
        let mut pool = Pool::new(PoolConfig::default());
        assert!(!pool.flush_if_dirty(&path).unwrap(), "干净时不写盘");
        assert!(!path.exists());

        pool.upsert("u1", None, "");
        assert!(pool.flush_if_dirty(&path).unwrap(), "有改动时写盘");
        assert!(path.exists());
        assert!(!pool.flush_if_dirty(&path).unwrap(), "写完即干净");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn pool_pick_integration_uses_governance_state() {
        let mut pool = Pool::new(PoolConfig::default());
        pool.upsert("a", Some(RealmTag::Cn), "");
        pool.upsert("b", Some(RealmTag::Cn), "");
        pool.set_credits("a", 1000, 0);
        pool.set_credits("b", 10, 0);

        let mut tried = HashSet::new();
        let first = pool
            .pick_account(
                NOW,
                Some(RealmTag::Cn),
                "",
                &tried,
                PickMode::Weighted,
                1,
                None,
                None,
            )
            .expect("有候选");
        tried.insert(first.clone());

        // 失败的号进入冷却后，换号重试应拿到另一个
        pool.apply_upstream_error(&first, "", &UpstreamEvent::NotFound, NOW);
        let second = pool
            .pick_account(
                NOW,
                Some(RealmTag::Cn),
                "",
                &tried,
                PickMode::Weighted,
                2,
                None,
                None,
            )
            .expect("仍有候选");
        assert_ne!(first, second);
    }

    /// ★★ 护栏：`pick_account` 的**偏好**必须生效，且不可用时**回落**。
    ///
    /// 守的是「界面上写着『固定账号』、实际请求却走池选号」那类缺陷 ——
    /// 偏好若没接进池选号，界面与行为不符且**不报错**。
    ///
    /// 三种不可用形态都要覆盖（已试过 / 不在池里 / 无偏好），且**任何一条都不得
    /// 返回 `None`**：偏好是优化而非约束，绝不因为偏好不可用就拒绝服务。
    #[test]
    fn pick_account_honours_preferred_uid_and_falls_back() {
        let mut pool = Pool::new(PoolConfig::default());
        pool.upsert("rich", Some(RealmTag::Cn), "");
        pool.upsert("poor", Some(RealmTag::Cn), "");
        pool.set_credits("rich", 1000, 0);
        pool.set_credits("poor", 10, 0);

        // ① 偏好可用 ⇒ 必须选它，即使积分远少于别人（这是「指定账号」的全部意义）。
        let picked = pool
            .pick_account(
                NOW,
                Some(RealmTag::Cn),
                "",
                &HashSet::new(),
                PickMode::Weighted,
                1,
                Some("poor"),
                None,
            )
            .expect("有候选");
        assert_eq!(picked, "poor", "偏好账号可用时必须优先选中");

        // ② 偏好已被本请求试过 ⇒ 跳过它、回落自动择优（否则会在同一请求里重复撞）。
        let tried: HashSet<String> = ["poor".to_string()].into_iter().collect();
        let picked = pool
            .pick_account(
                NOW,
                Some(RealmTag::Cn),
                "",
                &tried,
                PickMode::Weighted,
                2,
                Some("poor"),
                None,
            )
            .expect("仍有候选");
        assert_ne!(picked, "poor", "已试过的偏好必须被跳过");

        // ③ 偏好不在池里 ⇒ 回落自动择优（绝不返回 None）。
        let picked = pool
            .pick_account(
                NOW,
                Some(RealmTag::Cn),
                "",
                &HashSet::new(),
                PickMode::Weighted,
                3,
                Some("no-such"),
                None,
            )
            .expect("偏好不存在也必须回落到自动择优");
        assert!(
            ["rich", "poor"].contains(&picked.as_str()),
            "回落结果必须来自池内：{picked}"
        );

        // ④ 无偏好 ⇒ 不引入任何行为变化（仍能选出账号）。
        assert!(
            pool.pick_account(
                NOW,
                Some(RealmTag::Cn),
                "",
                &HashSet::new(),
                PickMode::Weighted,
                4,
                None,
                None,
            )
            .is_some(),
            "无偏好时行为不变"
        );
    }

    #[test]
    fn credits_refresh_interval_normalizes_non_positive_to_default() {
        assert_eq!(
            PoolConfig::default().credits_refresh_interval_ms,
            30 * 60 * 1000
        );
        for invalid in [0, -1, -9999] {
            let config = PoolConfig {
                credits_refresh_interval_ms: invalid,
                ..PoolConfig::default()
            }
            .normalized();
            assert_eq!(
                config.credits_refresh_interval_ms,
                30 * 60 * 1000,
                "非正间隔应归一为默认 30 分钟（输入 {invalid}）"
            );
        }
        let kept = PoolConfig {
            credits_refresh_interval_ms: 1234,
            ..PoolConfig::default()
        }
        .normalized();
        assert_eq!(kept.credits_refresh_interval_ms, 1234, "合法值必须保留");
    }

    #[test]
    fn thaw_skips_non_credit_hard_cooldown() {
        let mut pool = Pool::new(PoolConfig::default());
        pool.upsert("u1", Some(RealmTag::Cn), "");
        {
            let entry = pool.entries.get_mut("u1").unwrap();
            entry.cool_kind = Some(CoolKind::Hard);
            entry.until_ms = 9_999_999_999_999;
            entry.reason = "429 rate limit".to_string();
        }
        assert!(
            !pool.thaw_hard_credit_if_recovered("u1", 500),
            "非余额原因的硬冷却不得解冻"
        );
        let entry = pool.get("u1").unwrap();
        assert_eq!(entry.cool_kind, Some(CoolKind::Hard));
        assert_eq!(entry.until_ms, 9_999_999_999_999);
    }

    #[test]
    fn thaw_skips_soft_cooldown_and_zero_balance() {
        // 软冷却（404）：即使余额为正也不得解冻。
        let mut pool = Pool::new(PoolConfig::default());
        pool.upsert("soft", Some(RealmTag::Cn), "");
        pool.apply_upstream_error("soft", "", &UpstreamEvent::NotFound, NOW);
        assert!(
            !pool.thaw_hard_credit_if_recovered("soft", 500),
            "软冷却不得解冻"
        );
        assert!(!pool.thaw_hard_credit_if_recovered("soft", 0));

        // 硬冷却但余额为 0：不得解冻。
        pool.upsert("hard", Some(RealmTag::Cn), "");
        pool.apply_upstream_error("hard", "", &UpstreamEvent::HardCredit, NOW);
        assert!(
            !pool.thaw_hard_credit_if_recovered("hard", 0),
            "0 余额不得解冻"
        );
        assert_eq!(pool.get("hard").unwrap().cool_kind, Some(CoolKind::Hard));

        // 未知 uid 不 panic。
        assert!(!pool.thaw_hard_credit_if_recovered("ghost", 100));
    }

    #[test]
    fn persistence_round_trips_credits_refreshed_ms() {
        let path = temp_path("credits-refreshed");
        let mut pool = Pool::new(PoolConfig::default());
        pool.upsert("u1", Some(RealmTag::Cn), "");
        pool.set_credits("u1", 100, 10);
        pool.mark_credits_refreshed("u1", NOW);
        pool.save(&path).expect("落盘成功");

        let mut restored = Pool::new(PoolConfig::default());
        restored.load(&path, NOW);
        let entry = restored.get("u1").expect("恢复成功");
        assert_eq!(entry.credits_refreshed_ms, NOW, "刷新时刻必须持久化");
        assert_eq!(entry.credits, 100);
        assert_eq!(entry.credits_expiring, 10);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn thaw_requires_hard_cool_kind_even_when_reason_matches() {
        // `reason` 是字符串匹配，将来若有人改文案、或误把软冷却的 reason 也写成「余额不足」，
        // `cool_kind == Some(Hard)` 就是唯一防线。本用例把「软冷却 + reason 含余额不足」钉死：
        // 余额为正也不得解冻。
        let mut pool = Pool::new(PoolConfig::default());
        pool.upsert("u1", Some(RealmTag::Cn), "");
        {
            let entry = pool.entries.get_mut("u1").unwrap();
            entry.cool_kind = Some(CoolKind::Soft);
            entry.until_ms = 9_999_999_999_999;
            // 含 HARD_CREDIT_COOLDOWN_REASON 子串，但冷却类别是软冷却。
            entry.reason = format!("上游提示：{}（soft）", HARD_CREDIT_COOLDOWN_REASON);
        }

        assert!(
            !pool.thaw_hard_credit_if_recovered("u1", 500),
            "软冷却即使 reason 含「余额不足」也不得解冻（唯一防线是 cool_kind==Hard）"
        );
        let entry = pool.get("u1").unwrap();
        assert_eq!(entry.cool_kind, Some(CoolKind::Soft), "冷却类别不得被改动");
        assert_eq!(entry.until_ms, 9_999_999_999_999, "冷却截止不得被清除");
        assert!(
            entry.reason.contains(HARD_CREDIT_COOLDOWN_REASON),
            "reason 不得被改动"
        );
    }

    /// ★★ 采纳探测结果**绝不能推进退避计数** —— 本仓最容易踩的一脚。
    ///
    /// 探测是**我们主动打的**：若让它走 `apply_upstream_error` 的那套（记错 + 推
    /// `soft_streak` / `fails` / `retry_count`），就会「因为探测而被罚」，冷却越探越长。
    ///
    /// 变异（QA 已实测）：在 `adopt_probe_result` 里插一句 `entry.soft_streak += 1`
    /// ⇒ 本用例红（`soft_streak` 会从 0 变 1）。
    #[test]
    fn adopt_probe_result_never_touches_backoff_or_health() {
        let mut pool = Pool::new(PoolConfig::default());
        pool.upsert("u1", Some(RealmTag::Cn), "n1");
        // 先制造一点「历史」，确保下面的「不变」断言不是空转。
        pool.apply_upstream_error("u1", "", &UpstreamEvent::Server, NOW);
        let before = pool.get("u1").unwrap().clone();
        assert!(before.fails > 0, "前置：应已记过失败，否则断言空转");

        assert!(
            pool.adopt_probe_result(
                "u1",
                Some(RealmTag::Cn),
                "glm-5.3",
                &ProbeOutcome::ModelRateLimited { reset_at_ms: None },
                NOW,
            ),
            "模型级限流 ⇒ 应置该模型冷却（返回 true）"
        );

        let after = pool.get("u1").unwrap();
        assert!(
            after.model_cooldowns.contains_key("glm-5.3"),
            "该模型应进冷却"
        );
        assert_eq!(
            after.until_ms, before.until_ms,
            "★ 账号级冷却不得被探测改动"
        );
        assert_eq!(
            after.soft_streak, before.soft_streak,
            "★ soft_streak 不得推进"
        );
        assert_eq!(after.fails, before.fails, "★ 连续失败计数不得被探测推进");
        assert_eq!(
            after.retry_count, before.retry_count,
            "★ 退避指数不得被探测推进"
        );
        assert_eq!(
            after.breaker_until_ms, before.breaker_until_ms,
            "★ 不得因探测触发熔断"
        );
        assert_eq!(after.success_ema, before.success_ema);
        assert_eq!(after.error_ema, before.error_ema);
        assert_eq!(after.session_dead_fails, before.session_dead_fails);
        assert_eq!(
            after.err_total, before.err_total,
            "★ 探测不是流量，不记错次"
        );
        assert_eq!(after.last_err_ms, before.last_err_ms);
    }

    /// ★ `reset_at_ms` 缺失 / 已过期时**必须补齐下限**。
    ///
    /// 这是三方评审（产品官 / QA / 安全官）独立收敛到的同一个真缺陷：
    /// `cooldown_model(.., 0, ..)` 会落 `now + 1ms` = **等于没冷却**，
    /// 而调用方以为「已置冷却」。变异：把 `MIN_PROBE_COOLDOWN_MS` 那段
    /// `unwrap_or(now + MIN)` 改回 `unwrap_or(0)` ⇒ 本用例红。
    #[test]
    fn adopt_probe_model_rate_limit_always_gets_a_floor() {
        let mut pool = Pool::new(PoolConfig::default());
        pool.upsert("u1", Some(RealmTag::Cn), "n1");

        // ① 上游完全没给墙钟。
        assert!(pool.adopt_probe_result(
            "u1",
            Some(RealmTag::Cn),
            "m",
            &ProbeOutcome::ModelRateLimited { reset_at_ms: None },
            NOW,
        ));
        let cooldown = pool.get("u1").unwrap().model_cooldowns["m"].clone();
        assert!(
            cooldown.until_ms >= NOW + MIN_PROBE_COOLDOWN_MS,
            "没给墙钟必须补下限，不能只冷 1ms（实得 {}）",
            cooldown.until_ms - NOW
        );
        // ★ issue #88：没墙钟时必须走**兜底时长**（10 分钟），不是 60 秒下限 ——
        //   60 秒的冷却撑不到用户切到账号池分区，等于「回填了但看不见」。
        //   变异：把 `probe_cooldown_until` 的 `None` 分支改回 `MIN_PROBE_COOLDOWN_MS` ⇒ 红。
        assert_eq!(
            cooldown.until_ms - NOW,
            PROBE_FALLBACK_COOLDOWN_MS,
            "无墙钟应取兜底时长（{PROBE_FALLBACK_COOLDOWN_MS}ms），实得 {}ms",
            cooldown.until_ms - NOW
        );

        // ② 墙钟已过期（上游说「昨天就重置了」，但我们刚吃了个 429）⇒ 同样补下限。
        assert!(pool.adopt_probe_result(
            "u1",
            Some(RealmTag::Cn),
            "m2",
            &ProbeOutcome::ModelRateLimited {
                reset_at_ms: Some(NOW - 86_400_000)
            },
            NOW,
        ));
        let cooldown = pool.get("u1").unwrap().model_cooldowns["m2"].clone();
        assert!(
            cooldown.until_ms >= NOW + MIN_PROBE_COOLDOWN_MS,
            "过期墙钟同样补下限（既有语义：`reset_at` 不是解锁时刻）"
        );

        // ③ 未来墙钟照用（但封顶 `soft_rate_max`）。
        assert!(pool.adopt_probe_result(
            "u1",
            Some(RealmTag::Cn),
            "m3",
            &ProbeOutcome::ModelRateLimited {
                reset_at_ms: Some(NOW + 120_000)
            },
            NOW,
        ));
        assert_eq!(
            pool.get("u1").unwrap().model_cooldowns["m3"].until_ms,
            NOW + 120_000,
            "未封顶的未来墙钟应原样采用"
        );
    }

    /// ★ 兜底时长可配（issue #88 的「回填值不应是一个固定值」）。
    ///
    /// 变异：让 `adopt_probe_result` 忽略 `config.probe_fallback_ms`（直接吃常量）⇒ 红。
    #[test]
    fn adopt_probe_fallback_honours_config() {
        let config = PoolConfig {
            probe_fallback_ms: 20 * 60 * 1000,
            ..PoolConfig::default()
        };
        let mut pool = Pool::new(config);
        pool.upsert("u1", Some(RealmTag::Cn), "n1");
        assert!(pool.adopt_probe_result(
            "u1",
            Some(RealmTag::Cn),
            "m",
            &ProbeOutcome::ModelRateLimited { reset_at_ms: None },
            NOW,
        ));
        assert_eq!(
            pool.get("u1").unwrap().model_cooldowns["m"].until_ms - NOW,
            20 * 60 * 1000,
            "兜底时长必须读配置，不能写死"
        );
    }

    /// ★ `summary().model_cooling` 必须真的数出来（issue #88 · C）。
    ///
    /// 正交计数：账号状态仍是 `available`，只是有模型级冷却 —— 所以它**不能**
    /// 从 `available` 里扣掉，但**必须**有一个非零数字反映它。
    /// 变异：删掉 `summary_filtered` 里那段 `model_cooling` 累加 ⇒ 红。
    #[test]
    fn summary_counts_model_cooling_without_touching_available() {
        let mut pool = Pool::new(PoolConfig::default());
        pool.upsert("u1", Some(RealmTag::Cn), "n1");
        pool.upsert("u2", Some(RealmTag::Cn), "n2");
        assert!(pool.adopt_probe_result(
            "u1",
            Some(RealmTag::Cn),
            "m",
            &ProbeOutcome::ModelRateLimited { reset_at_ms: None },
            NOW,
        ));

        let summary = pool.summary(NOW, Some(RealmTag::Cn));
        assert_eq!(summary.model_cooling, 1, "有模型级冷却的号应被数到");
        assert_eq!(summary.available, 2, "模型冷却不改账号级状态 ⇒ 可用数不变");
        assert_eq!(summary.cooling, 0, "不得混进账号级冷却");
        assert!(
            summary.model_cooling <= summary.available,
            "正交计数不得越过 available"
        );
    }

    /// ★ 模型封禁（11102）：**探测不推进 `hits`**，但仍要真的置上冷却。
    ///
    /// QA 指出 `block_model_backoff_without_hits`（唯一调用方就是这里）当时**零覆盖**。
    /// 变异：把实现换成会 +1 的 `block_model_backoff` ⇒ 本用例红。
    #[test]
    fn adopt_probe_model_blocked_never_bumps_hits() {
        let now = NOW;
        // ① 全新模型：应拿到「第 1 档」冷却（6h），但 hits 记 1 而不是 2。
        let mut pool = Pool::new(PoolConfig::default());
        pool.upsert("u1", Some(RealmTag::Cn), "n1");
        assert!(pool.adopt_probe_result(
            "u1",
            Some(RealmTag::Cn),
            "m",
            &ProbeOutcome::ModelBlocked,
            now,
        ));
        let cooldown = pool.get("u1").unwrap().model_cooldowns["m"].clone();
        assert_eq!(cooldown.hits, 1, "首次探测取第 1 档参数");
        assert_eq!(cooldown.until_ms - now, 6 * 60 * 60 * 1000, "第 1 档 = 6h");
        assert!(cooldown.reason.starts_with("11102"), "原因应标明封禁");

        // ② 同一模型再探一次：**不得**推到 12h（探测不是新的处罚）。
        assert!(
            !pool.adopt_probe_result(
                "u1",
                Some(RealmTag::Cn),
                "m",
                &ProbeOutcome::ModelBlocked,
                now
            ),
            "同一时刻重复采纳 ⇒ 不算变化"
        );
        let cooldown = pool.get("u1").unwrap().model_cooldowns["m"].clone();
        assert_eq!(cooldown.hits, 1, "★ 探测不得推进 hits");
        assert_eq!(
            cooldown.until_ms - now,
            6 * 60 * 60 * 1000,
            "★ 不得被探测推到更长档"
        );

        // ③ 真实流量已经罚过两次（hits=2 ⇒ 12h）⇒ 探测**保留**既有 hits，不动它的档位。
        let mut pool = Pool::new(PoolConfig::default());
        pool.upsert("u1", Some(RealmTag::Cn), "n1");
        pool.apply_upstream_error("u1", "m", &UpstreamEvent::ModelBlocked, now);
        pool.apply_upstream_error("u1", "m", &UpstreamEvent::ModelBlocked, now);
        let real = pool.get("u1").unwrap().model_cooldowns["m"].clone();
        assert_eq!(real.hits, 2, "前置：真实流量罚了两次");

        pool.adopt_probe_result(
            "u1",
            Some(RealmTag::Cn),
            "m",
            &ProbeOutcome::ModelBlocked,
            now,
        );
        let after = pool.get("u1").unwrap().model_cooldowns["m"].clone();
        assert_eq!(
            after.hits, real.hits,
            "★ 探测保留既有 hits，既不 +1 也不清零"
        );
        assert_eq!(
            after.until_ms,
            now + 12 * 60 * 60 * 1000,
            "仍是第 2 档（12h）"
        );

        // ④ 模型级限流**不参与**封禁计数（两条退避语义不同，混用会让封禁档位被污染）。
        pool.adopt_probe_result(
            "u1",
            Some(RealmTag::Cn),
            "m4",
            &ProbeOutcome::ModelRateLimited { reset_at_ms: None },
            now,
        );
        assert_eq!(
            pool.get("u1").unwrap().model_cooldowns["m4"].hits,
            0,
            "限流的 hits 恒为 0（不是封禁）"
        );
    }

    /// ★ 跨作用域防御：池按 `uid` 建键、`realm` 只是属性 ⇒ 不校验就可能写错条目。
    ///
    /// 变异：删掉 `realm` 校验那一段 ⇒ 本用例红。
    #[test]
    fn adopt_probe_result_refuses_cross_realm() {
        let mut pool = Pool::new(PoolConfig::default());
        pool.upsert("u1", Some(RealmTag::Cn), "n1");

        assert!(
            !pool.adopt_probe_result(
                "u1",
                Some(RealmTag::Global),
                "m",
                &ProbeOutcome::ModelRateLimited { reset_at_ms: None },
                NOW,
            ),
            "用 global 的探测去写 cn 的号 ⇒ 必须拒绝"
        );
        assert!(
            pool.get("u1").unwrap().model_cooldowns.is_empty(),
            "被拒的采纳不得留下任何冷却"
        );

        // `realm = None`（调用方确实不知道归属）⇒ 不做该校验，照常采纳。
        assert!(pool.adopt_probe_result(
            "u1",
            None,
            "m",
            &ProbeOutcome::ModelRateLimited { reset_at_ms: None },
            NOW,
        ));
        assert!(pool.get("u1").unwrap().model_cooldowns.contains_key("m"));
    }

    /// ★ 采纳必须置 `dirty` —— 否则周期落盘的 `flush_if_dirty` 不会写盘，回填**静默丢失**。
    ///
    /// 变异：删掉 `if changed { self.dirty = true; }` ⇒ 本用例红。
    #[test]
    fn adopt_probe_result_marks_pool_dirty() {
        let mut pool = Pool::new(PoolConfig::default());
        pool.upsert("u1", Some(RealmTag::Cn), "n1");
        pool.dirty = false;
        assert!(!pool.dirty);

        pool.adopt_probe_result(
            "u1",
            Some(RealmTag::Cn),
            "m",
            &ProbeOutcome::ModelRateLimited { reset_at_ms: None },
            NOW,
        );
        assert!(pool.dirty, "置了冷却 ⇒ 必须置 dirty，否则回填不会落盘");

        // 没有变化时不该无谓置脏。
        pool.dirty = false;
        assert!(!pool.adopt_probe_result("u1", Some(RealmTag::Cn), "m", &ProbeOutcome::Other, NOW));
        assert!(!pool.dirty, "没变化就别置脏");
    }

    /// `ok` ⇒ **清掉**该模型的冷却（用户 2026-10-16 拍板）；其余档**不动冷却**。
    #[test]
    fn adopt_probe_available_clears_that_model_only() {
        let mut pool = Pool::new(PoolConfig::default());
        pool.upsert("u1", Some(RealmTag::Cn), "n1");
        pool.adopt_probe_result(
            "u1",
            Some(RealmTag::Cn),
            "glm-5.3",
            &ProbeOutcome::ModelRateLimited { reset_at_ms: None },
            NOW,
        );
        pool.adopt_probe_result(
            "u1",
            Some(RealmTag::Cn),
            "kimi-k3",
            &ProbeOutcome::ModelRateLimited { reset_at_ms: None },
            NOW,
        );

        assert!(
            pool.adopt_probe_result(
                "u1",
                Some(RealmTag::Cn),
                "glm-5.3",
                &ProbeOutcome::Available,
                NOW
            ),
            "探到可用 ⇒ 应清掉该模型冷却并返回 true"
        );
        let entry = pool.get("u1").unwrap();
        assert!(
            !entry.model_cooldowns.contains_key("glm-5.3"),
            "该模型的冷却应被清掉"
        );
        assert!(
            entry.model_cooldowns.contains_key("kimi-k3"),
            "★ 只清**这一个模型** —— 不能顺手把同账号其它模型也放了"
        );
        assert!(
            !pool.adopt_probe_result(
                "u1",
                Some(RealmTag::Cn),
                "glm-5.3",
                &ProbeOutcome::Available,
                NOW
            ),
            "本来就没有该模型冷却 ⇒ 返回 false（幂等）"
        );

        // `Other` 档（402 积分不足 / 登录失效 / 网络错误…）：**不写任何冷却**。
        let before = pool.get("u1").unwrap().clone();
        assert!(
            !pool.adopt_probe_result("u1", Some(RealmTag::Cn), "hy3", &ProbeOutcome::Other, NOW),
            "Other 档不产生冷却变化"
        );
        let after = pool.get("u1").unwrap();
        assert_eq!(after.until_ms, before.until_ms, "Other 档不得动账号冷却");
        assert_eq!(
            after.model_cooldowns, before.model_cooldowns,
            "Other 档不得动模型冷却"
        );

        // 账号级限流 ⇒ 账号级软冷却。★ 它**会清空模型级表** —— 这是既有语义
        // （`cooldown_soft_until` 内部 `model_cooldowns.clear()`）：账号级冷却压过模型级。
        let before = pool.get("u1").unwrap().until_ms;
        assert!(
            pool.adopt_probe_result(
                "u1",
                Some(RealmTag::Cn),
                "hy3",
                &ProbeOutcome::AccountRateLimited { reset_at_ms: None },
                NOW,
            ),
            "账号级限流应置账号冷却"
        );
        let after = pool.get("u1").unwrap();
        assert!(after.until_ms > before, "账号级限流应置账号冷却");
        assert!(
            after.model_cooldowns.is_empty(),
            "账号级冷却压过模型级 ⇒ 模型表被清空（既有语义，非本改动引入）"
        );
    }

    /// 已禁用的账号不参与回填（`revive` 是唯一出口，探测不能绕开它复活/改写禁用态）。
    #[test]
    fn adopt_probe_result_skips_disabled_accounts() {
        let mut pool = Pool::new(PoolConfig::default());
        pool.upsert("u1", Some(RealmTag::Cn), "n1");
        for _ in 0..3 {
            pool.apply_upstream_error("u1", "", &UpstreamEvent::SessionDead, NOW);
        }
        assert!(pool.get("u1").unwrap().disabled, "前置：应已禁用");

        assert!(
            !pool.adopt_probe_result(
                "u1",
                Some(RealmTag::Cn),
                "glm-5.3",
                &ProbeOutcome::ModelRateLimited { reset_at_ms: None },
                NOW,
            ),
            "已禁用的账号不应被探测回填"
        );
        assert!(pool.get("u1").unwrap().model_cooldowns.is_empty());
        assert!(!pool.adopt_probe_result(
            "no-such-uid",
            Some(RealmTag::Cn),
            "m",
            &ProbeOutcome::Available,
            NOW
        ));
    }

    /// ★ 真实上游响应原文 → **探测格状态**：把「判据 → 格」这条链走在**真数据**上。
    ///
    /// ## 为什么这条放在 pool 的 tests 里
    ///
    /// **权威 fixture 就在上面**（`REAL_GLOBAL_BODY` / `REAL_CN_BODY` 逐字取自本机客户端落盘数据，
    /// 2026-09-28 复核）。复制一份到 `model_probe::tests` 必然漂移 —— 本仓最忌「两处各存一份」。
    ///
    /// ## 它补的是哪一段
    ///
    /// `model_probe::tests` 里手搓的 `UpstreamEvent` 只能证明「**映射**对了」；
    /// 证明不了「**真实响应体**经过 `classify_event` 之后，再经探测映射，落到的还是那个格」。
    /// 这条把两者**串起来**跑在真实上游原文上 —— 是「无出站 mock 挂载点」条件下最接近 e2e 的一条。
    ///
    /// 变异：把 `probe_state_of` 里 `SoftRate { model_scoped: true }` 改成 `RateLimitedAccount`
    /// ⇒ 前两条断言红。
    #[test]
    fn real_bodies_map_to_the_right_probe_cell() {
        use crate::model_probe::{probe_state_of, ProbeState};

        // ① 真实 429 原文（国内版 / 国际版模板，模型级）：都带 `6004` + 重置墙钟。
        assert_eq!(
            probe_state_of(&classify_event(429, REAL_CN_BODY)),
            ProbeState::RateLimitedModel,
            "真实国内版 429 原文必须落「模型限流」格"
        );
        assert_eq!(
            probe_state_of(&classify_event(429, REAL_GLOBAL_BODY)),
            ProbeState::RateLimitedModel,
            "真实国际版 429 原文必须落「模型限流」格"
        );

        // ② 402 / 余额不足标记：**必须落 `NoCredits`，绝不能算「限流」**
        //（把没积分报成限流，用户会去傻等解锁 —— `client_limits` 已踩过这个坑）。
        // 用常量拼 body 而不是写字面量：换了标记词本用例仍会跟着走，不会空转。
        let no_credits = format!(r#"{{"code":402,"message":"{}"}}"#, HARD_CREDIT_MARKERS[0]);
        assert_eq!(
            probe_state_of(&classify_event(402, &no_credits)),
            ProbeState::NoCredits,
            "402 必须落「积分不足」，不是「限流」"
        );

        // ③ 模型封禁（标记取自常量，防文案漂移）。
        let blocked = format!(r#"{{"code":403,"message":"{}"}}"#, MODEL_BLOCK_MARKER);
        assert_eq!(
            probe_state_of(&classify_event(403, &blocked)),
            ProbeState::ModelBlocked
        );

        // ④ ★ 两条 429 **必须分档**：账号级（响应体里没有任何模型级标记）不能与模型级混。
        let account_level = r#"{"code":429,"message":"too many requests"}"#;
        assert_eq!(
            probe_state_of(&classify_event(429, account_level)),
            ProbeState::RateLimitedAccount
        );
        assert_ne!(
            probe_state_of(&classify_event(429, REAL_CN_BODY)),
            probe_state_of(&classify_event(429, account_level)),
            "★ 模型级与账号级必须是两个不同的格（判反了会让「换模型可用」的用户白等）"
        );

        // ⑤ 其余档：404 无此模型 / 5xx 上游异常 / 4xx 客户端错误。
        assert_eq!(
            probe_state_of(&classify_event(404, r#"{"error":"model not found"}"#)),
            ProbeState::ModelUnavailable
        );
        assert_eq!(
            probe_state_of(&classify_event(500, "boom")),
            ProbeState::UpstreamError
        );
        assert_eq!(
            probe_state_of(&classify_event(400, "bad request")),
            ProbeState::UpstreamError
        );
    }
}
