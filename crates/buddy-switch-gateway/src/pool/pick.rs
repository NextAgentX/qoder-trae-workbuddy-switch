//! 选号算法：四因子加权 + 成本分层硬过滤 + Top-N 短名单 + 防惊群 + LRU 兜底。
//!
//! 移植自参考实现 `internal/pool/pick.go`。
//!
//! **为什么不只是「按权重排序取第一名」**：单点最优会让同一账号被连续打死（惊群），
//! 也会让「快过期积分优先消耗」与「闲置补偿」这些目标互相压制。因此实际策略是
//! 「分层 → 排序 → 取短名单 → 在短名单内按权重抽签」，把确定性排序转化为有界随机。
//!
//! 实现约束：候选快照在**同一把锁窗口内**一次性拷贝出所需字段，之后不再持有对池的
//! 借用——否则「选出结果后回写 `last_used`」会与候选列表的借用冲突。

use std::collections::{BTreeMap, HashSet};

use crate::pool::entry::CostTier;
use crate::pool::entry_like::PoolEntryLike;
use crate::pool::RealmTag;
use crate::rng::Pcg32;

/// 成本分层与短名单共用的策略参数。
#[derive(Debug, Clone, Copy)]
pub struct PickPolicy {
    /// 快过期积分占比的权重（参考实现 `expiringWeight`）。
    pub expiring_weight: f64,
    /// 「到期紧迫度」**独立因子**的权重（`0.0` = 不启用，WorkBuddy 默认如此）。
    ///
    /// ## 为什么要有这个因子（而不是复用 `expiring_weight`）
    ///
    /// `expiring_weight` 的输入是 `credits_expiring / credits` —— 一个**比值**，
    /// 只有当产品线能给出「快过期积分的绝对值」时才有意义（WorkBuddy 的余额刷新
    /// 正是如此）。
    ///
    /// Trae 没有「分批到期」概念，只有「这批积分在某个时刻统一过期」。若把它的
    /// 紧迫度硬塞进 `credits_expiring`，会被 `credits` 归一化抹平 ——
    /// 实测过：积分 999、一年后到期的账号会压过积分 10、一分钟后就到期的账号，
    /// 等于把「先用掉快过期的额度」这条核心语义**静默丢掉**。
    ///
    /// 因此独立成一个因子：取值范围 `[0,1]`（0 = 无到期信息或远未到期），
    /// 由 [`PoolEntryLike::expiry_urgency`] 提供。权重为 `0.0` 时该项恒为 0，
    /// 引入前后行为逐字一致。
    ///
    /// ★ 对 Trae 侧该权重需**大于积分项上限（10.0）**，否则紧迫度差异会被积分差异
    /// 淹没；见 [`crate::trae::pool`] 的取值说明。
    pub expiry_urgency_weight: f64,
    /// 闲置补偿：每小时增加的权重。
    pub idle_weight_per_hour: f64,
    /// 闲置补偿权重上限。
    pub idle_weight_max: f64,
    /// 防惊群间隔：此毫秒数内刚被选中的账号不进抽签池。
    pub min_pick_gap_ms: i64,
    /// 成本账本有效期。
    pub model_cost_ttl_ms: i64,
    /// 单账号在途上限（0 = 不限）。
    pub max_in_flight: i64,
    /// 国际版账号在途上限（0 = 沿用 `max_in_flight`）。
    pub max_in_flight_global: i64,
    /// 是否**跳过加权抽签**，直接取排序后的第一名（严格择优）。
    ///
    /// - `false`（默认）：短名单内加权抽签，把确定性排序转化为**有界随机**，
    ///   避免同一账号被连续打死。WorkBuddy 的多账号长会话场景需要它。
    /// - `true`：权重最高者恒胜。适用于「账号少、用户要求行为可预期」的场景。
    ///
    /// ★ 为什么必须有这个开关：加权抽签天然带随机性，而「先用掉快过期的额度」
    /// 这类语义要求**确定性** —— 用户指定或按到期时间择优时，不接受「大约 60% 的概率
    /// 选到它」。若只靠把权重调大来逼近确定性，既脆弱又需要指数级放大，
    /// 不如把「要不要随机」这件事显式暴露成策略。
    pub deterministic: bool,
}

/// 短名单长度。
pub const SHORTLIST_SIZE: usize = 5;
/// 权重比较的浮点容差（判定「等权重」用）。
const WEIGHT_EPSILON: f64 = 1e-9;
/// 加权抽签的定点缩放。
const DRAW_SCALE: f64 = 1_000_000.0;

/// 选号策略 —— **只影响「在合格候选里挑谁」**，不影响任何硬过滤。
///
/// 与 `AccountStrategy` 一一对应（`Current` → [`Self::Weighted`]、`MaxCredits` →
/// [`Self::MaxCredits`]、`Random` → [`Self::Random`]、`Sequential` → [`Self::Sequential`]；
/// `Pinned` 走「偏好」而不是这里）。
///
/// ★ 为什么是一个枚举而不是若干 bool：这几种策略**互斥**。做成 bool 会出现
/// 「既按积分择优又随机」这种语义上不可能成立的组合，而 UI 上很容易点出来。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PickMode {
    /// 六因子权重 → 成本分层 → Top-N 短名单 → 加权抽签（默认）。
    #[default]
    Weighted,
    /// 候选内**积分最高者**（`AccountStrategy::MaxCredits`）。
    MaxCredits,
    /// 候选内**均匀抽签**（`AccountStrategy::Random`）：不看权重，用于把额度平均分摊。
    ///
    /// 与 `Weighted` 的区别是「有偏 / 无偏」：加权抽签仍然偏爱积分多、闲置久的号，
    /// 长期看会把某几个号抽干。用户明确要「平均分摊」时只有均匀抽签能满足。
    Random,
    /// **顺序**（issue #102）：粘住当前账号，只有它不可用时才换「顺序上的下一个」。
    ///
    /// ## 为什么不是「轮流」（round-robin）
    ///
    /// 用户的诉求是**提高上游前缀缓存的命中率**：同一会话的上下文反复落在同一个账号上，
    /// 上游侧才有可复用的 KV 前缀。轮流会在每轮请求上换号 —— 缓存命中率归零，
    /// 与「随机」的后果完全一样，只是变得可预测。
    ///
    /// 因此本档的语义是「**粘住最近使用的那个号**」：只要它仍然健康、没被限流、
    /// 没被排除、没占满在途，就一直用它；它一旦不可用，才换下一个。
    ///
    /// ## 「顺序」从哪来
    ///
    /// 候选按 `used_seq` **降序**取第一名（`used_seq` 是选号序号，单调递增、与系统时钟
    /// 无关 ⇒ 不受时间回拨影响）；从未用过的号 `used_seq = 0`，并列时按 uid 升序。
    /// 于是：冷启动取 uid 最小的那个并一直粘着它；它被限流/冷却后，落回「次近使用」的号，
    /// 全都没用过时就是 uid 序的下一个。
    ///
    /// ★ 不依赖账号库的物理顺序：池是 `BTreeMap<uid, _>`，插入序根本不存在 ——
    /// 「按列表顺序」只能是 uid 序，写清楚比含糊地叫「顺序」重要。
    ///
    /// ★ **不受 `min_pick_gap_ms`（防惊群）约束**：那条规则的作用是「别让同一个号被连续打死」，
    /// 而本档的目的恰恰是「让它连续被用」。两者互斥，故本档在分层之后直接返回，
    /// 不进入短名单与防惊群那一段。
    Sequential,
}

/// 一次选号的**输入**（把 6 个同类型的参数收成一个结构体）。
///
/// 收成结构体不是为了少打字，而是因为 `realm` / `model` / `tried` / `excluded` 全是
/// `&HashSet<String>` / `Option<RealmTag>` 这类**同形状**参数 —— 位置传参一旦顺序写反，
/// 编译器不会报错（都是引用），只会静默换错语义。
pub struct PickInput<'a> {
    /// 归属域过滤（`None` = 不限）。
    pub realm: Option<RealmTag>,
    /// 本次请求的模型（决定模型级冷却是否命中）。
    pub model: &'a str,
    /// 本轮已试过的 uid（换号重试时由调用方累加）。
    pub tried: &'a HashSet<String>,
    /// 用户显式排除的 uid（**硬约束**：自动选号、兜底、偏好一律跳过）。
    pub excluded: &'a HashSet<String>,
    /// ★ 本密钥的**允许名单**（issue #76）：`None` = 全部账号（不过滤）；
    /// `Some(set)` = 只有集合内的 uid 可被选中。与 `excluded` **同层**（都是硬约束），
    /// 但来源不同：`excluded` 是用户对账号池的全局排除，`allowed` 是**这一把 Key**
    /// 的授权范围。分组已在调用方展开成 uid（池条目不认识分组，见 `relay::select_account`）。
    pub allowed: Option<&'a HashSet<String>>,
    /// 选号策略。
    pub mode: PickMode,
    /// 抽签种子（生产用时间戳+序号，测试用固定值以保证可复现）。
    pub seed: u64,
}

/// 候选快照（不持有对池的借用）。
#[derive(Debug, Clone)]
struct Candidate {
    uid: String,
    weight: f64,
    credits: i64,
    cost_per_1k: f64,
    used_seq: u64,
    last_used_ms: i64,
    tier: u8,
}

/// 计算四因子权重。
///
/// ```text
/// w = 1
///   + credits/max_credits * 10                     （积分比例，归一化到候选集最大值）
///   + credits_expiring/credits * expiring_weight   （快过期占比，越紧迫越高）
///   + expiry_urgency * expiry_urgency_weight       （到期紧迫度，产品线特有）
///   + idle_weight                                  （闲置补偿，0.5/小时，封顶 5.0）
///   + success_ratio * 3                            （成功率 EMA 比率；无记录取 1.5）
/// ```
///
/// 归一化口径：**积分除以分层过滤前的候选集最大值**（截断与抽签共用同一基准），
/// 因此「积分最多的账号」恒得满配 10 分。候选积分全为 0 时该项跳过，
/// 而不是退化为均匀随机——否则零积分账号会获得与有余额账号相同的权重。
///
/// ★ 泛型化说明：本函数只通过 [`PoolEntryLike`] 读取条目，因此两条产品线共用同一套
/// 权重语义。产品线特有的「紧迫度」表达方式由各自的 [`PoolEntryLike::credits_expiring`]
/// 实现负责（见该方法的契约）。
pub fn weight_of<E: PoolEntryLike>(
    entry: &E,
    max_credits: i64,
    now_ms: i64,
    policy: &PickPolicy,
) -> f64 {
    // 账号级版本：成功/错误率都取**账号维度** EMA（与改造前逐字相同）。
    weight_with_ema(
        entry,
        max_credits,
        now_ms,
        policy,
        entry.success_ema().max(0.0),
        entry.error_ema().max(0.0),
    )
}

/// **分模型**版本的权重（O6）：该模型有足够样本就用它的 EMA，否则回落到账号级。
///
/// ## 为什么需要它
///
/// 账号级 EMA 会把「在这个模型上老失败」摊到**所有**模型头上：一个号在 A 模型上
/// 连吃 429，它在 B 模型上的权重也被一起拖下去 —— 可它其实完全能服务 B。
/// 那是**错杀**：用户看到的是「明明换个模型就好，网关却一直不用这个号」。
///
/// 回落到账号级（而不是「没样本就当中性」）是有意的：新号 / 新模型上，
/// 账号的整体表现仍然是唯一可用的证据。
pub fn weight_of_model<E: PoolEntryLike>(
    entry: &E,
    max_credits: i64,
    now_ms: i64,
    policy: &PickPolicy,
    model: &str,
) -> f64 {
    let (success, error) = entry
        .model_ema(model)
        .unwrap_or((entry.success_ema().max(0.0), entry.error_ema().max(0.0)));
    weight_with_ema(
        entry,
        max_credits,
        now_ms,
        policy,
        success.max(0.0),
        error.max(0.0),
    )
}

/// 权重计算的**唯一实现**（两个公开入口只差「取哪一份 EMA」）。
fn weight_with_ema<E: PoolEntryLike>(
    entry: &E,
    max_credits: i64,
    now_ms: i64,
    policy: &PickPolicy,
    success: f64,
    error: f64,
) -> f64 {
    let mut weight = 1.0;

    let credits = entry.credits();
    let credits_expiring = entry.expiring_credits(now_ms);

    if max_credits > 0 {
        weight += (credits.max(0) as f64 / max_credits as f64) * 10.0;
    }

    if credits > 0 && credits_expiring > 0 {
        let ratio = (credits_expiring as f64 / credits as f64).clamp(0.0, 1.0);
        weight += ratio * policy.expiring_weight;
    }

    // 到期紧迫度独立因子：承载「还有多久过期」这类产品线特有语义。
    // 权重为 0 时该项恒为 0，因此对不启用它的产品线零影响。
    weight += entry.expiry_urgency(now_ms).clamp(0.0, 1.0) * policy.expiry_urgency_weight;

    weight += idle_weight(entry.last_used_ms(), now_ms, policy);

    let success = success.max(0.0);
    let error = error.max(0.0);
    let total = success + error;
    weight += if total > 0.0 {
        (success / total) * 3.0
    } else {
        // 无历史记录：给中性值，既不被惩罚也不被优待。
        1.5
    };

    weight
}

/// 闲置补偿权重：从未使用按上限给（鼓励启用新号）；否则随时长线性增长并封顶。
fn idle_weight(last_used_ms: i64, now_ms: i64, policy: &PickPolicy) -> f64 {
    if last_used_ms <= 0 {
        return policy.idle_weight_max.max(0.0);
    }
    let idle_ms = now_ms.saturating_sub(last_used_ms).max(0);
    let hours = idle_ms as f64 / 3_600_000.0;
    (hours * policy.idle_weight_per_hour).clamp(0.0, policy.idle_weight_max.max(0.0))
}

/// 「指定账号」下拉里的**策略哨兵值**（占位 uid）。
///
/// ## 为什么把策略编码进「偏好值」，而不是给两条产品线各加一个配置字段
///
/// Trae / Qoder 的配置里已经有 `preferredUid`（一个字符串，配合前端同一张池卡的
/// 下拉框）。再加一个 `pickMode` 字段意味着：两份配置结构、两个端点参数、两个前端状态
/// —— 而它们表达的其实是**同一个控件的一个取值**。
///
/// 前端早就有这套约定（`AUTO_OPTION = "__auto__"` / `MAX_CREDITS_OPTION = "__max_credits__"`），
/// 这里只是把它变成**后端也认**的语义（否则用户选「按积分择优」在 Trae 侧会被当成一个
/// 不存在的 uid，静默回落到加权 —— 又一个「假开关」）。
///
/// 四个值与前端常量**逐字相同**，改任何一处都必须同步（`parse_preferred` 的用例钉住）。
pub const PREFERRED_AUTO: &str = "__auto__";
/// 「按积分择优」（候选内积分最高者）。
pub const PREFERRED_MAX_CREDITS: &str = "__max_credits__";
/// 「随机」（候选内均匀抽签）。
pub const PREFERRED_RANDOM: &str = "__random__";
/// 「顺序」（粘住当前账号，不可用才换下一个；issue #102）。
pub const PREFERRED_SEQUENTIAL: &str = "__sequential__";

/// 把「偏好值」解析成 `(选号策略, 真正的偏好 uid)`。
///
/// - 哨兵值 ⇒ 对应策略 + **无偏好**（策略本身就是「不指定某个号」）；
/// - 普通非空串 ⇒ 加权 + 该 uid 作偏好（仍是偏好而非约束，见 `pick_with_preference`）；
/// - 空 / `None` ⇒ 加权 + 无偏好（默认行为，与改造前逐字相同）。
pub fn parse_preferred(value: Option<&str>) -> (PickMode, Option<&str>) {
    match value.map(str::trim) {
        Some(PREFERRED_MAX_CREDITS) => (PickMode::MaxCredits, None),
        Some(PREFERRED_RANDOM) => (PickMode::Random, None),
        Some(PREFERRED_SEQUENTIAL) => (PickMode::Sequential, None),
        Some(PREFERRED_AUTO) | None => (PickMode::Weighted, None),
        Some("") => (PickMode::Weighted, None),
        Some(uid) => (PickMode::Weighted, Some(uid)),
    }
}

/// 空的排除集（「不排除任何账号」的默认值）。
/// 用一个静态单例而不是到处 `&HashSet::new()`：后者要在每个调用点持有临时变量，
/// 容易写成 `&HashSet::new()` 的悬垂借用；且「没有排除」是最常见的默认值。
pub fn no_exclusions() -> &'static HashSet<String> {
    static EMPTY: std::sync::OnceLock<HashSet<String>> = std::sync::OnceLock::new();
    EMPTY.get_or_init(HashSet::new)
}

impl<'a> PickInput<'a> {
    /// 测试与「无排除 / 默认策略」调用点的便捷构造。
    pub fn new(
        realm: Option<RealmTag>,
        model: &'a str,
        tried: &'a HashSet<String>,
        seed: u64,
    ) -> Self {
        Self {
            realm,
            model,
            tried,
            excluded: no_exclusions(),
            // 便捷构造 = 「无排除、无白名单」的最小输入（测试与简单场景用）。
            allowed: None,
            mode: PickMode::Weighted,
            seed,
        }
    }
}

/// 该账号是否已占满在途额度。
/// 单条目的可用性判据 —— **唯一来源**。
///
/// [`pick`] 的候选过滤走这里；日后若要接「会话粘性」，粘性的可用性检查**也必须**走这里。
///
/// ★ 为什么要有这个名字：粘性要回答「上次那个账号现在还能不能用」。若它另写一套判据
/// 且比 `pick` 宽松，就会把请求钉到一个 `pick` 认为不可用的账号上
/// （症状：**明明有别的号可用却一直失败**）。两处判据一旦分家，这种缺陷不会报错、
/// 只会表现成「偶发失败」。抽成一个函数是唯一可靠的做法。
fn entry_usable<E: PoolEntryLike>(
    entry: &E,
    policy: &PickPolicy,
    now_ms: i64,
    model: &str,
) -> bool {
    entry.healthy_for_request(now_ms, model) && !in_flight_full(entry, policy, now_ms)
}

/// 白名单判定（`None` = 全部账号 ⇒ 放行）。
///
/// ★ 与 [`selectable`] 的 `excluded` 判据**并排**：两条都是硬约束，
///   任一不过就整体不可选 —— 否则「自动选号跳过了、偏好/兜底没跳过」这类缺陷
///   一定会出现（且不报错）。
fn in_allowed(input: &PickInput<'_>, uid: &str) -> bool {
    match input.allowed {
        None => true,
        Some(allowed) => allowed.contains(uid),
    }
}

/// 「此刻能不能被选中」= 健康 ∧ 未占满在途 ∧ **不在排除名单里** ∧ **在允许名单里**。
///
/// 排除与「健康」正交：前者是**用户意图**（不想让网关用它），后者是账号状态。
/// 但两者对选号的后果相同 —— 都不能选。因此必须收敛到**同一个谓词**上，
/// 否则「自动选号跳过了、偏好/兜底没跳过」这类缺陷一定会出现（且不报错）。
fn selectable<E: PoolEntryLike>(
    entry: &E,
    policy: &PickPolicy,
    now_ms: i64,
    model: &str,
    excluded: &HashSet<String>,
    allowed: Option<&HashSet<String>>,
) -> bool {
    !excluded.contains(entry.uid())
        && allowed.map(|set| set.contains(entry.uid())).unwrap_or(true)
        && entry_usable(entry, policy, now_ms, model)
}

/// 指定 uid 此刻是否可用于该请求（**与 [`pick`] 同一套判据**，见 [`selectable`]）。
///
/// 供**会话粘性**与**指定账号偏好**使用：两者都要先问「那个账号还能不能用」，
/// 能用就复用、不能用就换号。
pub fn is_usable<E: PoolEntryLike>(
    entries: &BTreeMap<String, E>,
    policy: &PickPolicy,
    uid: &str,
    now_ms: i64,
    model: &str,
    excluded: &HashSet<String>,
    allowed: Option<&HashSet<String>>,
) -> bool {
    entries
        .get(uid)
        .map(|entry| selectable(entry, policy, now_ms, model, excluded, allowed))
        .unwrap_or(false)
}

/// `pub(crate)`：`Pool::summary` 统计 `PoolSummary::in_flight_full` 复用**同一判据**
/// —— 卡片说「可用」而选号一个都不给，就是从这里开始分家的。
pub(crate) fn in_flight_full<E: PoolEntryLike>(
    entry: &E,
    policy: &PickPolicy,
    now_ms: i64,
) -> bool {
    let limit = match entry.realm() {
        Some(RealmTag::Global) if policy.max_in_flight_global > 0 => policy.max_in_flight_global,
        _ => policy.max_in_flight,
    };
    // 0 = 不限（计数仍累加，但永不拒绝）。
    //
    // ★ 用 `in_flight_now`（**自愈版**）而不是裸计数：acquire/release 是手写配对，
    //   请求被 drop 时会泄漏；裸计数会让账号被永久判成「在途已满」而再也选不中，
    //   界面上却仍显示「可用」—— 正是「账号看着正常、请求一直失败」的一种。
    limit > 0 && entry.in_flight_now(now_ms) >= limit
}

/// 从池中选出一个账号；返回被选中账号的 uid。
///
/// 输入全部收在 [`PickInput`] 里（域 / 模型 / 已试 / 排除 / 策略 / 种子）。
///
/// ★ 泛型化：算法只通过 [`PoolEntryLike`] 读取条目，因此 WorkBuddy / Trae / Qoder
/// 共用同一套选号语义。需要「优先某个账号」请用 [`pick_with_preference`]。
pub fn pick<E: PoolEntryLike>(
    entries: &mut BTreeMap<String, E>,
    pick_seq: &mut u64,
    policy: &PickPolicy,
    now_ms: i64,
    input: &PickInput<'_>,
) -> Option<String> {
    // 惰性清理过期的模型级冷却（与选号共用同一次写锁窗口）。
    for entry in entries.values_mut() {
        entry.prune_model_cooldowns(now_ms);
    }

    let candidate_uids: Vec<String> = entries
        .values()
        .filter(|entry| {
            !input.tried.contains(entry.uid())
                && selectable(
                    *entry,
                    policy,
                    now_ms,
                    input.model,
                    input.excluded,
                    input.allowed,
                )
                && input
                    .realm
                    .map(|wanted| entry.realm() == Some(wanted))
                    .unwrap_or(true)
        })
        .map(|entry| entry.uid().to_string())
        .collect();

    if candidate_uids.is_empty() {
        return pick_earliest_expiry(entries, policy, now_ms, input);
    }

    // 归一化基准：分层过滤**之前**的候选集最大值。
    let max_credits = candidate_uids
        .iter()
        .filter_map(|uid| entries.get(uid))
        .map(|entry| entry.credits())
        .max()
        .unwrap_or(0);

    let mut candidates: Vec<Candidate> = candidate_uids
        .iter()
        .filter_map(|uid| entries.get(uid))
        .map(|entry| Candidate {
            uid: entry.uid().to_string(),
            weight: weight_of_model(entry, max_credits, now_ms, policy, input.model),
            credits: entry.credits(),
            cost_per_1k: entry.cost_per_1k(now_ms, input.model, policy.model_cost_ttl_ms),
            used_seq: entry.used_seq(),
            last_used_ms: entry.last_used_ms(),
            tier: entry
                .cost_tier(now_ms, input.model, policy.model_cost_ttl_ms)
                .rank(),
        })
        .collect();

    // 成本分层硬过滤：免费 > 无观测 > 收费。
    // 设计意图：让「未知号」保留被观测的机会（否则一旦有号实测收费，
    // 所有无观测号会被永久排除，账本永远学不到新数据）。
    let best_tier = candidates
        .iter()
        .map(|candidate| candidate.tier)
        .min()
        .unwrap_or(CostTier::Unknown.rank());
    candidates.retain(|candidate| candidate.tier == best_tier);

    // ★★ 策略分流**必须发生在成本分层之后**：
    //    否则「随机」会随机到收费号上 —— 那不是「平均分摊」，是「随机花积分」。
    match input.mode {
        PickMode::Random => {
            // 均匀抽签：先按 uid 排序（`BTreeMap` 的遍历序天然有序，但 `retain` 之后
            // 仍要显式排序才不依赖实现细节），再取一个下标 ⇒ 同 seed 同结果。
            candidates.sort_by(|left, right| left.uid.cmp(&right.uid));
            let mut rng = Pcg32::new(input.seed, candidates.len() as u64 + 1);
            let index = rng.below(candidates.len() as u64) as usize;
            let uid = candidates[index].uid.clone();
            return Some(mark_selected(entries, uid, now_ms, pick_seq));
        }
        PickMode::MaxCredits => {
            // 积分降序；并列时用权重、再并列用 uid，保证可复现。
            candidates.sort_by(|left, right| {
                right
                    .credits
                    .cmp(&left.credits)
                    .then(
                        right
                            .weight
                            .partial_cmp(&left.weight)
                            .unwrap_or(std::cmp::Ordering::Equal),
                    )
                    .then(left.uid.cmp(&right.uid))
            });
            let uid = candidates.first()?.uid.clone();
            return Some(mark_selected(entries, uid, now_ms, pick_seq));
        }
        PickMode::Sequential => {
            // 「顺序」= **粘住最近被选过的那个号**：`used_seq` 降序取第一，并列按 uid 升序。
            //
            // ★ 为什么用 `used_seq` 而不是 `last_used_ms`：后者是墙钟，系统时间回拨 /
            //   跨时区同步会把它弄反；`used_seq` 是单调递增的选号序号，永远可靠。
            // ★ 这里**不洗牌、不抽签、不看防惊群** —— 三者都会把「粘住」变成「轮流」，
            //   而轮流对上游前缀缓存毫无帮助（见 `PickMode::Sequential` 的文档）。
            candidates.sort_by(|left, right| {
                right
                    .used_seq
                    .cmp(&left.used_seq)
                    .then(left.uid.cmp(&right.uid))
            });
            let uid = candidates.first()?.uid.clone();
            return Some(mark_selected(entries, uid, now_ms, pick_seq));
        }
        PickMode::Weighted => {}
    }

    // 等权重洗牌：仅当候选超过短名单长度且存在权重并列时执行。
    // 作用：让并列组的 Top-5 不总是同一批 uid（否则短名单会退化成 uid 序前 5 名）。
    let mut rng = Pcg32::new(input.seed, candidates.len() as u64 + 1);
    if candidates.len() > SHORTLIST_SIZE {
        let reference = candidates[0].weight;
        let has_tie = candidates
            .iter()
            .any(|candidate| (candidate.weight - reference).abs() < WEIGHT_EPSILON);
        if has_tie {
            rng.shuffle(&mut candidates);
        }
    }

    // 稳定排序：单价升序 → 权重降序。并列项保持上一步的（可能已打乱的）相对顺序；
    // 未洗牌时并列项保持 BTreeMap 的 uid 升序，因此测试可复现。
    candidates.sort_by(|left, right| {
        left.cost_per_1k
            .partial_cmp(&right.cost_per_1k)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(
                right
                    .weight
                    .partial_cmp(&left.weight)
                    .unwrap_or(std::cmp::Ordering::Equal),
            )
    });

    let shortlist_len = candidates.len().min(SHORTLIST_SIZE);
    let shortlist = &candidates[..shortlist_len];

    // 防惊群：短名单内剔除「刚被选过」的账号。被挤出的号**不回填**——
    // 回填会让短名单失去「限流」作用。
    let eligible: Vec<&Candidate> = shortlist
        .iter()
        .filter(|candidate| {
            candidate.last_used_ms <= 0
                || now_ms.saturating_sub(candidate.last_used_ms) >= policy.min_pick_gap_ms
        })
        .collect();

    let chosen_uid = if eligible.is_empty() {
        // LRU 兜底：短名单全被防惊群剔除时，取**全局** used_seq 最小者
        // （与时钟精度无关，因此不受系统时间回拨影响）。
        candidates
            .iter()
            .min_by_key(|candidate| candidate.used_seq)
            .map(|candidate| candidate.uid.clone())
    } else if policy.deterministic {
        // 严格择优：`eligible` 保持排序（单价升序 → 权重降序），首元素即最优。
        eligible.first().map(|candidate| candidate.uid.clone())
    } else {
        draw_weighted(&eligible, &mut rng).map(|candidate| candidate.uid.clone())
    };

    chosen_uid.map(|uid| mark_selected(entries, uid, now_ms, pick_seq))
}

/// 记一次「被选中」：推进 `pick_seq` 并写回 `last_used` / `used_seq`。
///
/// 三条出口（加权 / 积分择优 / 随机）共用它 —— 否则「随机模式下 LRU 语义不生效」
/// 这类缺陷会藏在一个分支里，而三个分支的测试都得各写一遍。
fn mark_selected<E: PoolEntryLike>(
    entries: &mut BTreeMap<String, E>,
    uid: String,
    now_ms: i64,
    pick_seq: &mut u64,
) -> String {
    *pick_seq += 1;
    if let Some(entry) = entries.get_mut(&uid) {
        entry.mark_picked(now_ms, *pick_seq);
    }
    uid
}

/// 带「指定账号」偏好的选号：**偏好可用则优先，不可用则回落通用选号**。
///
/// ## 为什么偏好走「短路」而不是「加权」
///
/// 用户的诉求是「先消耗指定账号的积分」，语义是**确定性优先**而非「更倾向」。
/// 若把偏好做成权重加成，它会被积分比例项（最高 10 分）压制 —— 指定一个积分较少的
/// 账号后，请求仍会大量落到积分多的账号上，功能等于没做。
///
/// ## 两条硬约束
///
/// 1. **偏好不是约束**：指定账号不可用（冷却 / 禁用 / 零积分 / 该模型被限流 /
///    在途占满 / 已在本轮试过）时**必须回落**到通用选号，绝不能因此拒绝服务。
///    这与会话粘性的取向一致 —— 粘性同样是优化而非约束。
/// 2. **可用性判据必须与 [`pick`] 同源**：走 [`is_usable`]（即 [`entry_usable`]）。
///    若这里另写一套更宽松的判据，就会把请求钉到一个 `pick` 认为不可用的账号上，
///    症状是「明明有别的号可用却一直失败」且**不报错**。
///
/// 命中偏好时同样推进 `pick_seq` 与 `used_seq`（它确实被使用了，应参与 LRU 语义）。
///
/// ★ `excluded` 也在这里生效：**指定一个被排除的账号同样不可用**（硬排除语义），
/// 会直接回落到通用选号 —— 而不是「排除只对自动选号生效」这种两套规则。
pub fn pick_with_preference<E: PoolEntryLike>(
    entries: &mut BTreeMap<String, E>,
    pick_seq: &mut u64,
    policy: &PickPolicy,
    now_ms: i64,
    input: &PickInput<'_>,
    preferred_uid: Option<&str>,
) -> Option<String> {
    if let Some(uid) = preferred_uid {
        let preferred_usable = !input.tried.contains(uid)
            && is_usable(
                entries,
                policy,
                uid,
                now_ms,
                input.model,
                input.excluded,
                input.allowed,
            )
            && input
                .realm
                .map(|wanted| entries.get(uid).and_then(|entry| entry.realm()) == Some(wanted))
                .unwrap_or(true);
        if preferred_usable {
            return Some(mark_selected(entries, uid.to_string(), now_ms, pick_seq));
        }
    }
    pick(entries, pick_seq, policy, now_ms, input)
}

/// 加权抽签：`wi = round(w * 1e6)`，下界 1；前缀累积首个命中者胜出。
fn draw_weighted<'a>(items: &[&'a Candidate], rng: &mut Pcg32) -> Option<&'a Candidate> {
    if items.is_empty() {
        return None;
    }
    let weights: Vec<i64> = items
        .iter()
        .map(|item| {
            let scaled = item.weight * DRAW_SCALE;
            if !scaled.is_finite() || scaled < 1.0 {
                1
            } else {
                scaled.round() as i64
            }
        })
        .collect();
    let total: i64 = weights.iter().sum();
    if total <= 0 {
        return items.first().copied();
    }

    let mut ticket = rng.below(total as u64) as i64;
    for (index, weight) in weights.iter().enumerate() {
        if ticket < *weight {
            return Some(items[index]);
        }
        ticket -= weight;
    }
    items.last().copied()
}

/// 全冷却兜底：池内无健康候选时，挑「最早解冻」的账号强试一次。
///
/// 约束：非 `tried`、realm 匹配、非禁用、**未被排除**、**排除仍在有效硬冷却中的账号**
/// （硬冷却意味着余额不足，强试必然再失败且浪费一次上游调用）。
/// 不推进 `used_seq`（它只是「试一下」，不应影响 LRU 语义）。
///
/// 「解冻时刻」取**账号级与模型级的较大者**（[`effective_cool_expiry_ms`]）：
/// 只被模型级冷落的账号同样值得一次半开试探，否则「只有这个模型被限流」会让
/// 整个池直接放弃（issue #106 §2.3）。
fn effective_cool_expiry_ms<E: PoolEntryLike>(entry: &E, model: &str) -> i64 {
    entry
        .cool_expiry_ms()
        .max(entry.model_cool_expiry_ms(model))
}

fn pick_earliest_expiry<E: PoolEntryLike>(
    entries: &mut BTreeMap<String, E>,
    policy: &PickPolicy,
    now_ms: i64,
    input: &PickInput<'_>,
) -> Option<String> {
    let candidate = entries
        .values()
        .filter(|entry| {
            if input.tried.contains(entry.uid())
                || entry.is_disabled()
                || input.excluded.contains(entry.uid())
                || !in_allowed(input, entry.uid())
            {
                return false;
            }
            if let Some(wanted) = input.realm {
                if entry.realm() != Some(wanted) {
                    return false;
                }
            }
            if in_flight_full(*entry, policy, now_ms) {
                return false;
            }
            // 硬冷却期间余额不足，强试无意义。
            if entry.is_hard_cooled(now_ms) {
                return false;
            }
            // ★ issue #106 §2.3：判据必须包含**该模型**的冷却。
            //
            // 改造前只看 `cool_expiry_ms()`（账号级冷却 + 熔断），完全不含
            // `model_cooldowns` ⇒ 「池里唯一的问题就是这个模型被限流」时兜底一个号
            // 都给不出来，调用方落到 `last == None` ⇒ 对外 401「凭据无效」。
            // 这是与闸门短路并列的第二条 19ms 空账号 401 路径。
            effective_cool_expiry_ms(*entry, input.model) > 0
        })
        .min_by_key(|entry| {
            (
                effective_cool_expiry_ms(*entry, input.model),
                entry.uid().to_string(),
            )
        })
        .map(|entry| entry.uid().to_string())?;

    if let Some(entry) = entries.get_mut(&candidate) {
        entry.mark_probed(now_ms);
    }
    Some(candidate)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pool::entry::PoolEntry;
    use crate::pool::ModelCooldown;

    fn policy() -> PickPolicy {
        PickPolicy {
            expiring_weight: 8.0,
            // WorkBuddy 不启用独立紧迫度因子（它的紧迫度由 `expiring_weight` 表达）。
            expiry_urgency_weight: 0.0,
            idle_weight_per_hour: 0.5,
            idle_weight_max: 5.0,
            min_pick_gap_ms: 100,
            model_cost_ttl_ms: 6 * 60 * 60 * 1000,
            max_in_flight: 3,
            max_in_flight_global: 2,
            // 保持 WorkBuddy 既有的有界随机语义。
            deterministic: false,
        }
    }

    fn pool_of(entries: Vec<PoolEntry>) -> BTreeMap<String, PoolEntry> {
        entries
            .into_iter()
            .map(|entry| (entry.uid.clone(), entry))
            .collect()
    }

    fn base_entry(uid: &str) -> PoolEntry {
        PoolEntry::new(uid)
    }

    const NOW: i64 = 1_000_000_000_000;

    #[allow(clippy::too_many_arguments)]
    // 测试夹具的参数逐项对应一个约束维度。
    /// 测试用简写：默认策略（加权）+ 不排除任何账号。
    ///
    /// `PickInput` 是生产签名的唯一入口；这里保留旧的位置参数形式，是为了让
    /// 二十多个既有选号用例**逐字不动** —— 它们测的是算法，不是参数打包。
    fn pick(
        entries: &mut BTreeMap<String, PoolEntry>,
        pick_seq: &mut u64,
        policy: &PickPolicy,
        now_ms: i64,
        realm: Option<RealmTag>,
        model: &str,
        tried: &HashSet<String>,
        seed: u64,
    ) -> Option<String> {
        super::pick(
            entries,
            pick_seq,
            policy,
            now_ms,
            &PickInput::new(realm, model, tried, seed),
        )
    }

    /// 同上，用于 `is_usable`（不排除任何账号）。
    fn is_usable(
        entries: &BTreeMap<String, PoolEntry>,
        policy: &PickPolicy,
        uid: &str,
        now_ms: i64,
        model: &str,
    ) -> bool {
        super::is_usable(entries, policy, uid, now_ms, model, no_exclusions(), None)
    }

    /// ★ O6：**分模型** EMA —— 账号级 EMA 相同，但「在 A 模型上老失败」不能拖累
    /// 它在 B 模型上的权重（那是错杀：换个模型明明就能用）。
    ///
    /// 变异：把 `weight_of_model` 改回恒用账号级 EMA ⇒ 第二条断言红。
    #[test]
    fn model_scoped_ema_does_not_punish_other_models() {
        let mut flaky = base_entry("flaky");
        let mut clean = base_entry("clean");
        // 两者账号级 EMA 完全相同（中性 0.5 / 0.5）—— 只有**分模型**证据不同。
        for entry in [&mut flaky, &mut clean] {
            entry.success_ema = 0.5;
            entry.error_ema = 0.5;
        }
        for _ in 0..3 {
            flaky.note_model_result("mA", false);
        }
        for _ in 0..3 {
            flaky.note_model_result("mB", true);
        }

        let bad_model = weight_of_model(&flaky, 0, NOW, &policy(), "mA");
        let good_model = weight_of_model(&flaky, 0, NOW, &policy(), "mB");
        let neutral = weight_of_model(&clean, 0, NOW, &policy(), "mB");

        assert!(
            good_model > bad_model,
            "同一账号在「它能用的模型」上权重必须更高：mB={good_model} mA={bad_model}"
        );
        assert!(
            good_model > neutral,
            "有该模型成功证据的号必须高于只有账号级中性证据的号：{good_model} vs {neutral}"
        );

        // 样本不足 ⇒ 回落到账号级（1 次偶发失败不该改变评价）。
        let mut fresh = base_entry("fresh");
        fresh.success_ema = 0.5;
        fresh.error_ema = 0.5;
        fresh.note_model_result("mC", false);
        assert!(
            (weight_of_model(&fresh, 0, NOW, &policy(), "mC")
                - weight_of(&fresh, 0, NOW, &policy()))
            .abs()
                < 1e-9,
            "样本 < MODEL_EMA_MIN_SAMPLES 时必须回落到账号级 EMA"
        );
    }

    /// ★ 偏好值 → 策略的解析（Trae / Qoder 的策略下拉就靠它）。
    ///
    /// 四个哨兵值必须与前端常量**逐字相同**（`AUTO_OPTION` / `MAX_CREDITS_OPTION` /
    /// `RANDOM_OPTION` / `SEQUENTIAL_OPTION`）—— 否则用户选了「按积分择优」，
    /// 后端会把它当成一个不存在的 uid 静默回落加权，那就是**假开关**
    /// （本仓对这类缺陷零容忍）。
    #[test]
    fn parse_preferred_maps_sentinels_to_modes() {
        assert_eq!(parse_preferred(None), (PickMode::Weighted, None));
        assert_eq!(parse_preferred(Some("")), (PickMode::Weighted, None));
        assert_eq!(parse_preferred(Some("  ")), (PickMode::Weighted, None));
        assert_eq!(
            parse_preferred(Some(PREFERRED_AUTO)),
            (PickMode::Weighted, None)
        );
        assert_eq!(
            parse_preferred(Some(PREFERRED_MAX_CREDITS)),
            (PickMode::MaxCredits, None)
        );
        assert_eq!(
            parse_preferred(Some(PREFERRED_RANDOM)),
            (PickMode::Random, None)
        );
        assert_eq!(
            parse_preferred(Some(PREFERRED_SEQUENTIAL)),
            (PickMode::Sequential, None)
        );
        // 普通 uid ⇒ 仍是「偏好」（不是策略）
        assert_eq!(
            parse_preferred(Some("uid-1")),
            (PickMode::Weighted, Some("uid-1"))
        );
        // 前后空白要吃掉（前端偶尔会带）
        assert_eq!(
            parse_preferred(Some(" __random__ ")),
            (PickMode::Random, None)
        );
    }

    /// 带策略的选号（测试用；`PickInput::new` 恒为加权档）。
    // 参数逐项对应一个约束维度（与上面的 `pick` 夹具同形），不为过 lint 而合并。
    #[allow(clippy::too_many_arguments)]
    fn pick_with_mode(
        entries: &mut BTreeMap<String, PoolEntry>,
        pick_seq: &mut u64,
        policy: &PickPolicy,
        now_ms: i64,
        model: &str,
        tried: &HashSet<String>,
        mode: PickMode,
        seed: u64,
    ) -> Option<String> {
        super::pick(
            entries,
            pick_seq,
            policy,
            now_ms,
            &PickInput {
                realm: None,
                model,
                tried,
                excluded: no_exclusions(),
                allowed: None,
                mode,
                seed,
            },
        )
    }

    /// ★ issue #102：顺序档必须**粘住同一个号**，而不是轮流。
    ///
    /// 变异：把 `Sequential` 分支改成「取 `used_seq` 最小者」⇒ 本用例立刻红
    /// （那是轮流，上游前缀缓存命中率归零 —— 正是用户报的问题）。
    ///
    /// 同时钉住「不受防惊群约束」：`policy()` 的 `min_pick_gap_ms = 100`，
    /// 而这里五次选号用**同一个 `now_ms`**。若顺序档走进短名单 + 防惊群那一段，
    /// 第二跳就会被踢出去换号。
    #[test]
    fn sequential_sticks_to_the_same_account() {
        let mut entries = pool_of(vec![base_entry("a"), base_entry("b"), base_entry("c")]);
        let mut seq = 0u64;
        let mut seen = Vec::new();
        for round in 0..5u64 {
            let uid = pick_with_mode(
                &mut entries,
                &mut seq,
                &policy(),
                NOW,
                "m",
                &HashSet::new(),
                PickMode::Sequential,
                round,
            )
            .expect("必须有候选");
            seen.push(uid);
        }
        assert!(
            seen.iter().all(|uid| uid == "a"),
            "顺序档必须一直用同一个号（uid 序第一个）：{seen:?}"
        );
    }

    /// ★ issue #102 的后半句：「账号限流后自动切换下一个」。
    ///
    /// 且**恢复后不抢回** —— 否则每 60 秒抖一次账号，缓存又被清空，等于没修。
    #[test]
    fn sequential_moves_to_next_only_when_current_is_unusable() {
        let mut entries = pool_of(vec![base_entry("a"), base_entry("b")]);
        let mut seq = 0u64;

        let first = pick_with_mode(
            &mut entries,
            &mut seq,
            &policy(),
            NOW,
            "m",
            &HashSet::new(),
            PickMode::Sequential,
            1,
        )
        .expect("必须有候选");
        assert_eq!(first, "a", "冷启动取 uid 序第一个");

        // a 被账号级限流 ⇒ 不可用 ⇒ 换 b。
        entries
            .get_mut("a")
            .expect("a 存在")
            .cooldown_fixed(NOW, 60_000, "429 账号级限流");
        let second = pick_with_mode(
            &mut entries,
            &mut seq,
            &policy(),
            NOW,
            "m",
            &HashSet::new(),
            PickMode::Sequential,
            2,
        )
        .expect("必须有候选");
        assert_eq!(second, "b", "当前号被限流后必须换下一个");

        // a 的冷却到期（60s 后）：**仍然留在 b** —— b 已是当前号。
        let third = pick_with_mode(
            &mut entries,
            &mut seq,
            &policy(),
            NOW + 61_000,
            "m",
            &HashSet::new(),
            PickMode::Sequential,
            3,
        )
        .expect("必须有候选");
        assert_eq!(
            third, "b",
            "冷却恢复后不得把请求从 b 抢回 a —— 那等于每 60 秒清一次缓存"
        );
    }

    /// 顺序档照样吃硬约束：`tried`（本轮已试）与排除名单都不进候选。
    #[test]
    fn sequential_respects_tried_and_exclusions() {
        let mut entries = pool_of(vec![base_entry("a"), base_entry("b"), base_entry("c")]);
        let mut seq = 0u64;
        let mut tried = HashSet::new();
        tried.insert("a".to_string());
        let uid = pick_with_mode(
            &mut entries,
            &mut seq,
            &policy(),
            NOW,
            "m",
            &tried,
            PickMode::Sequential,
            1,
        )
        .expect("必须有候选");
        assert_eq!(uid, "b", "本轮已试过的号不得再被选中");

        let excluded: HashSet<String> = ["b".to_string()].into_iter().collect();
        let input = PickInput {
            realm: None,
            model: "m",
            tried: &HashSet::new(),
            excluded: &excluded,
            allowed: None,
            mode: PickMode::Sequential,
            seed: 2,
        };
        let uid = super::pick(&mut entries, &mut seq, &policy(), NOW, &input).expect("必须有候选");
        assert_eq!(uid, "a", "排除是硬约束：被排除的号即使「最近用过」也不能选");
    }

    #[test]
    fn weight_increases_with_credit_ratio_and_is_bounded() {
        let mut small = base_entry("small");
        small.credits = 100;
        let mut big = base_entry("big");
        big.credits = 1000;

        let small_weight = weight_of(&small, 1000, NOW, &policy());
        let big_weight = weight_of(&big, 1000, NOW, &policy());

        // 基准 1 + 积分项 10 + 闲置项 5（从未使用）+ 成功率项 1.5 = 17.5
        assert!(
            (big_weight - 17.5).abs() < 1e-9,
            "满配应约 17.5，实际 {big_weight}"
        );
        assert!(
            (small_weight - (1.0 + 1.0 + 5.0 + 1.5)).abs() < 1e-9,
            "实际 {small_weight}"
        );
        assert!(big_weight > small_weight);
    }

    #[test]
    fn zero_max_credits_skips_credit_term_instead_of_uniformizing() {
        let mut entry = base_entry("a");
        entry.credits = 0;
        // max=0 时积分项跳过：1 + 闲置 5 + 成功率 1.5 = 7.5
        let weight = weight_of(&entry, 0, NOW, &policy());
        assert!((weight - 7.5).abs() < 1e-9, "实际 {weight}");
    }

    #[test]
    fn expiring_ratio_uses_own_credits_as_denominator() {
        let mut entry = base_entry("a");
        entry.credits = 1000;
        entry.credits_expiring = 500;
        entry.last_used_ms = NOW; // 闲置项归零
                                  // 1 + 10 + 0.5*8 + 1.5 = 16.5
        let weight = weight_of(&entry, 1000, NOW, &policy());
        assert!((weight - 16.5).abs() < 1e-9, "实际 {weight}");

        // 占比超过 100% 时必须钳制（脏数据防御）
        entry.credits_expiring = 5000;
        let clamped = weight_of(&entry, 1000, NOW, &policy());
        assert!(
            (clamped - 20.5).abs() < 1e-9,
            "占比应钳到 1.0，实际 {clamped}"
        );
    }

    #[test]
    fn idle_weight_is_capped_and_never_used_gets_max() {
        let mut entry = base_entry("a");
        entry.last_used_ms = 0;
        let never_used = weight_of(&entry, 0, NOW, &policy());
        entry.last_used_ms = NOW - 100 * 3_600_000; // 闲置 100 小时
        let long_idle = weight_of(&entry, 0, NOW, &policy());
        assert!((never_used - long_idle).abs() < 1e-9, "两者都应取上限 5.0");

        entry.last_used_ms = NOW - 3_600_000; // 闲置 1 小时
        let one_hour = weight_of(&entry, 0, NOW, &policy());
        assert!(
            (one_hour - (1.0 + 0.5 + 1.5)).abs() < 1e-9,
            "1 小时闲置 → 0.5 权重，实际 {one_hour}"
        );
    }

    #[test]
    fn success_ratio_uses_ema_and_defaults_to_neutral() {
        let mut good = base_entry("good");
        good.success_ema = 0.9;
        good.error_ema = 0.1;
        let good_weight = weight_of(&good, 0, NOW, &policy());
        // 1 + 闲置 5 + 0.9*3 = 8.7
        assert!((good_weight - 8.7).abs() < 1e-9, "实际 {good_weight}");

        let neutral = base_entry("neutral");
        let neutral_weight = weight_of(&neutral, 0, NOW, &policy());
        assert!((neutral_weight - 7.5).abs() < 1e-9, "无记录应取 1.5");
        assert!(good_weight > neutral_weight);
    }

    #[test]
    fn in_flight_full_blocks_candidate() {
        let mut entry = base_entry("a");
        entry.in_flight = 3;
        let mut pool = pool_of(vec![entry]);
        let mut seq = 0;
        assert_eq!(
            pick(
                &mut pool,
                &mut seq,
                &policy(),
                NOW,
                None,
                "",
                &HashSet::new(),
                1
            ),
            None,
            "占满在途的账号不得被选中"
        );
    }

    #[test]
    fn zero_max_in_flight_means_unlimited() {
        let mut entry = base_entry("a");
        entry.in_flight = 99;
        let mut pool = pool_of(vec![entry]);
        let mut seq = 0;
        let mut relaxed = policy();
        relaxed.max_in_flight = 0;
        assert!(pick(
            &mut pool,
            &mut seq,
            &relaxed,
            NOW,
            None,
            "",
            &HashSet::new(),
            1
        )
        .is_some());
    }

    #[test]
    fn realm_filter_isolates_cn_and_global() {
        let mut cn = base_entry("cn-1");
        cn.realm = Some(RealmTag::Cn);
        let mut global = base_entry("global-1");
        global.realm = Some(RealmTag::Global);
        let mut pool = pool_of(vec![cn, global]);
        let mut seq = 0;

        assert_eq!(
            pick(
                &mut pool,
                &mut seq,
                &policy(),
                NOW,
                Some(RealmTag::Global),
                "",
                &HashSet::new(),
                1
            ),
            Some("global-1".to_string())
        );
        assert_eq!(
            pick(
                &mut pool,
                &mut seq,
                &policy(),
                NOW,
                Some(RealmTag::Cn),
                "",
                &HashSet::new(),
                2
            ),
            Some("cn-1".to_string())
        );
    }

    #[test]
    fn tried_accounts_are_excluded_across_retries() {
        let mut pool = pool_of(vec![base_entry("a"), base_entry("b")]);
        let mut seq = 0;

        let first = pick(
            &mut pool,
            &mut seq,
            &policy(),
            NOW,
            None,
            "",
            &HashSet::new(),
            1,
        )
        .unwrap();
        let mut tried = HashSet::new();
        tried.insert(first.clone());
        let second = pick(&mut pool, &mut seq, &policy(), NOW, None, "", &tried, 2).unwrap();
        assert_ne!(first, second, "换号重试必须换到另一个账号");

        tried.insert(second);
        assert_eq!(
            pick(&mut pool, &mut seq, &policy(), NOW, None, "", &tried, 3),
            None,
            "全部试过后不得重复尝试"
        );
    }

    #[test]
    fn anti_thundering_herd_prefers_untouched_account() {
        // 两个账号权重相同；刚被选中的那个在 100ms 内不应再被选中。
        let mut pool = pool_of(vec![base_entry("a"), base_entry("b")]);
        let mut seq = 0;

        for _ in 0..30 {
            let chosen = pick(
                &mut pool,
                &mut seq,
                &policy(),
                NOW,
                None,
                "",
                &HashSet::new(),
                7,
            )
            .expect("有候选");
            let next = pick(
                &mut pool,
                &mut seq,
                &policy(),
                NOW,
                None,
                "",
                &HashSet::new(),
                7,
            )
            .expect("有候选");
            assert_ne!(chosen, next, "100ms 内不得重复选中同一账号");
        }
    }

    #[test]
    fn lru_fallback_kicks_in_when_shortlist_all_recently_used() {
        // 单账号池：它刚被选中 → 防惊群会剔除它 → 走 LRU 兜底仍返回它
        let mut pool = pool_of(vec![base_entry("only")]);
        let mut seq = 0;
        let first = pick(
            &mut pool,
            &mut seq,
            &policy(),
            NOW,
            None,
            "",
            &HashSet::new(),
            1,
        );
        assert_eq!(first, Some("only".to_string()));
        let second = pick(
            &mut pool,
            &mut seq,
            &policy(),
            NOW,
            None,
            "",
            &HashSet::new(),
            2,
        );
        assert_eq!(
            second,
            Some("only".to_string()),
            "LRU 兜底必须保证不空手而回"
        );
    }

    #[test]
    fn cost_tier_hard_filters_paid_when_free_available() {
        let mut free = base_entry("free");
        free.record_cost(NOW, "m", 0.0, 1000);
        let mut paid = base_entry("paid");
        paid.record_cost(NOW, "m", 9.0, 1000);
        // 让收费号积分极高——若不做分层过滤，它必然胜出
        paid.credits = 1_000_000;
        free.credits = 0;

        let mut pool = pool_of(vec![free, paid]);
        let mut seq = 0;
        for seed in 0..20 {
            let chosen = pick(
                &mut pool,
                &mut seq,
                &policy(),
                NOW,
                None,
                "m",
                &HashSet::new(),
                seed,
            )
            .expect("有候选");
            assert_eq!(chosen, "free", "实测免费号必须硬胜过积分高的收费号");
        }
    }

    #[test]
    fn unknown_tier_keeps_learning_opportunity() {
        // 一个实测收费、一个无观测 → 分层为 Paid(2) 与 Unknown(1) → 无观测者优先
        let mut paid = base_entry("paid");
        paid.record_cost(NOW, "m", 9.0, 1000);
        paid.credits = 1_000_000;
        let unknown = base_entry("unknown");

        let mut pool = pool_of(vec![paid, unknown]);
        let mut seq = 0;
        let chosen = pick(
            &mut pool,
            &mut seq,
            &policy(),
            NOW,
            None,
            "m",
            &HashSet::new(),
            3,
        )
        .expect("有候选");
        assert_eq!(chosen, "unknown", "无观测号应保留被观测的机会");
    }

    #[test]
    fn expired_cost_observation_falls_back_to_unknown() {
        let mut entry = base_entry("a");
        entry.record_cost(0, "m", 9.0, 1000); // 很久以前
        assert_eq!(
            entry.cost_tier(NOW, "m", policy().model_cost_ttl_ms),
            CostTier::Unknown,
            "超 TTL 的观测视为无观测"
        );
    }

    #[test]
    fn pick_marks_last_used_and_advances_sequence() {
        let mut pool = pool_of(vec![base_entry("a")]);
        let mut seq = 0;
        pick(
            &mut pool,
            &mut seq,
            &policy(),
            NOW,
            None,
            "",
            &HashSet::new(),
            1,
        );
        let entry = pool.get("a").expect("存在");
        assert_eq!(entry.last_used_ms, NOW);
        assert_eq!(entry.used_seq, 1);
        assert_eq!(seq, 1);
    }

    #[test]
    fn pick_is_deterministic_for_same_seed() {
        let build = || pool_of(vec![base_entry("a"), base_entry("b"), base_entry("c")]);
        let mut pool_a = build();
        let mut pool_b = build();
        let mut seq_a = 0;
        let mut seq_b = 0;
        for _ in 0..10 {
            let left = pick(
                &mut pool_a,
                &mut seq_a,
                &policy(),
                NOW,
                None,
                "",
                &HashSet::new(),
                42,
            );
            let right = pick(
                &mut pool_b,
                &mut seq_b,
                &policy(),
                NOW,
                None,
                "",
                &HashSet::new(),
                42,
            );
            assert_eq!(left, right, "同种子必须选出同一账号");
        }
    }

    #[test]
    fn empty_pool_returns_none() {
        let mut pool: BTreeMap<String, PoolEntry> = BTreeMap::new();
        let mut seq = 0;
        assert_eq!(
            pick(
                &mut pool,
                &mut seq,
                &policy(),
                NOW,
                None,
                "",
                &HashSet::new(),
                1
            ),
            None
        );
    }

    #[test]
    fn all_cooling_falls_back_to_earliest_expiry() {
        let mut soon = base_entry("soon");
        soon.cooldown_fixed(NOW, 1000, "404");
        let mut late = base_entry("late");
        late.cooldown_fixed(NOW, 60_000, "429");
        let mut pool = pool_of(vec![soon, late]);
        let mut seq = 0;
        assert_eq!(
            pick(
                &mut pool,
                &mut seq,
                &policy(),
                NOW,
                None,
                "",
                &HashSet::new(),
                1
            ),
            Some("soon".to_string()),
            "全冷却时选最早解冻者"
        );
    }

    #[test]
    fn hard_cooled_accounts_are_excluded_from_expiry_fallback() {
        let mut hard = base_entry("hard");
        hard.cooldown_until_tomorrow_4am(NOW, "余额不足");
        let mut pool = pool_of(vec![hard]);
        let mut seq = 0;
        assert_eq!(
            pick(
                &mut pool,
                &mut seq,
                &policy(),
                NOW,
                None,
                "",
                &HashSet::new(),
                1
            ),
            None,
            "硬冷却（余额不足）不得被兜底强试"
        );
    }

    #[test]
    fn disabled_accounts_never_selected() {
        let mut disabled = base_entry("disabled");
        disabled.disable("12153 session dead");
        let mut pool = pool_of(vec![disabled]);
        let mut seq = 0;
        assert_eq!(
            pick(
                &mut pool,
                &mut seq,
                &policy(),
                NOW,
                None,
                "",
                &HashSet::new(),
                1
            ),
            None
        );
    }

    /// ★ issue #106 §2.3：模型级冷却必须让账号**不可路由**，但**不得**让整个池
    /// 直接放弃 —— 兜底要能对它做一次半开试探。
    ///
    /// 改造前 `pick_earliest_expiry` 的判据是 `cool_expiry_ms() > 0`，只看账号级冷却，
    /// 于是「池里唯一的问题就是这个模型被限流」时一个号都给不出来 ⇒ 调用方落到
    /// `last == None` ⇒ 对外 **401 `no_credential`**（19ms、无上游往返、账号列为空）。
    /// 这正是报障里「9 个号、五项计数全 0，却说 0 个可用」的来源。
    ///
    /// 反向对照（同批钉住）：硬冷却（余额不足）**仍然**不得被兜底强试 ——
    /// 见 `hard_cooled_accounts_are_excluded_from_expiry_fallback`。
    #[test]
    fn model_cooldown_falls_back_to_a_probe_instead_of_giving_up() {
        let mut entry = base_entry("a");
        entry.cooldown_model(NOW, "glm-5.2", 0, 60_000, "6004");
        let mut pool = pool_of(vec![entry]);
        let mut seq = 0;
        assert_eq!(
            pick(
                &mut pool,
                &mut seq,
                &policy(),
                NOW,
                None,
                "glm-5.2",
                &HashSet::new(),
                1
            ),
            Some("a".to_string()),
            "只剩模型级冷却时必须给出一个半开试探号，而不是整池放弃（那会变成 401）"
        );
        // 但它在**择优**阶段仍被视为不可用：冷却未到期时不会因为「健康」被选中。
        // （上面那次是兜底 `pick_earliest_expiry`，走的是 `mark_probed`，不推进 `used_seq`。）
        assert_eq!(pool["a"].used_seq, 0, "兜底强试不得推进 LRU 序号");
        assert_eq!(
            pick(
                &mut pool,
                &mut seq,
                &policy(),
                NOW,
                None,
                "deepseek-v4-flash",
                &HashSet::new(),
                1
            ),
            Some("a".to_string()),
            "其它模型应立即可用"
        );
    }

    #[test]
    fn draw_weighted_respects_weights() {
        let heavy = Candidate {
            uid: "heavy".to_string(),
            weight: 1000.0,
            credits: 0,
            cost_per_1k: 0.0,
            used_seq: 1,
            last_used_ms: 0,
            tier: 0,
        };
        let light = Candidate {
            uid: "light".to_string(),
            weight: 1.0,
            credits: 0,
            cost_per_1k: 0.0,
            used_seq: 2,
            last_used_ms: 0,
            tier: 0,
        };
        let items = vec![&heavy, &light];
        let mut rng = Pcg32::new(1, 1);
        let mut heavy_wins = 0;
        for _ in 0..1000 {
            if draw_weighted(&items, &mut rng).map(|item| item.uid.as_str()) == Some("heavy") {
                heavy_wins += 1;
            }
        }
        assert!(
            heavy_wins > 900,
            "权重 1000:1 应几乎必胜，实际 {heavy_wins}/1000"
        );
    }

    #[test]
    fn draw_weighted_handles_empty_and_zero_weights() {
        let mut rng = Pcg32::new(1, 1);
        assert!(draw_weighted(&[], &mut rng).is_none());

        let zero = Candidate {
            uid: "z".to_string(),
            weight: 0.0,
            credits: 0,
            cost_per_1k: 0.0,
            used_seq: 1,
            last_used_ms: 0,
            tier: 0,
        };
        let items = vec![&zero];
        assert_eq!(
            draw_weighted(&items, &mut rng).map(|item| item.uid.as_str()),
            Some("z"),
            "零权重也应能被选中（下界 1），避免死锁"
        );
    }

    /// ★★ 护栏：`is_usable` 与 `pick` 的可用性判断**必须一致**。
    ///
    /// 会话粘性用 `is_usable` 回答「上次那个账号现在还能不能用」。若两者分家、
    /// 且 `is_usable` 更宽松，粘性就会把请求钉到一个 `pick` 认为不可用的账号上 ——
    /// 症状是「明明有别的号可用却一直失败」，而且**不报错**、只表现为偶发失败。
    ///
    /// 两处现在共用 `entry_usable`；本用例是它的**行为级**印证（改坏任一侧都会红）。
    #[test]
    fn is_usable_agrees_with_pick_on_each_state() {
        let policy = policy();

        let healthy = base_entry("healthy");
        let mut saturated = base_entry("saturated");
        saturated.in_flight = 3; // 与 `in_flight_full_blocks_candidate` 同款：占满在途
        let mut cooling = base_entry("cooling");
        cooling.until_ms = NOW + 60_000;

        let map = pool_of(vec![healthy, saturated, cooling]);

        assert!(
            is_usable(&map, &policy, "healthy", NOW, ""),
            "健康账号应可用"
        );
        assert!(
            !is_usable(&map, &policy, "saturated", NOW, ""),
            "在途占满应不可用"
        );
        assert!(
            !is_usable(&map, &policy, "cooling", NOW, ""),
            "冷却中应不可用"
        );
        assert!(
            !is_usable(&map, &policy, "absent", NOW, ""),
            "不在池里应不可用"
        );

        // 同一组条目下 `pick` 必须只挑到 `healthy` —— 与上面三条否定断言互为印证。
        let mut for_pick = map.clone();
        let mut seq = 0;
        assert_eq!(
            pick(
                &mut for_pick,
                &mut seq,
                &policy,
                NOW,
                None,
                "",
                &HashSet::new(),
                1
            )
            .as_deref(),
            Some("healthy"),
            "pick 只能选到 `is_usable` 为 true 的那个"
        );
    }

    /// 模型级冷却也必须被 `is_usable` 看到 —— 粘性传的是**模型名**，
    /// 若这里漏看模型维度，粘性会把请求钉到一个「该模型正被限流」的账号上。
    #[test]
    fn is_usable_honours_model_scoped_cooldown() {
        let policy = policy();
        let mut blocked = base_entry("blocked");
        blocked.model_cooldowns.insert(
            "m1".to_string(),
            ModelCooldown {
                until_ms: NOW + 60_000,
                reset_at_ms: 0,
                reason: String::new(),
                hits: 0,
            },
        );
        let map = pool_of(vec![blocked]);

        assert!(
            !is_usable(&map, &policy, "blocked", NOW, "m1"),
            "该模型正被冷却 ⇒ 粘性不得钉住它"
        );
        assert!(
            is_usable(&map, &policy, "blocked", NOW, "m2"),
            "换模型立即可用（模型级限流只封锁该模型）"
        );
    }

    /// ★ 已**过期**的模型级冷却不得被当成不可用。
    ///
    /// 这一条专治「两套判据分家」的一个隐蔽入口：`pick` 在过滤前会
    /// `prune_model_cooldowns`（把过期条目**删掉**），而 `is_usable` **不 prune**
    /// （它只读、不持有写锁）。若判据本身不是「自带过期判断」的，两者就会分家 ——
    /// 表现为「粘性认为该账号不可用，而 pick 认为可用」，或反之。
    ///
    /// 现在两者共用 `entry_usable` ⇒ `healthy_for_model` 自带 `now_ms >= until_ms`，
    /// prune 只是清理。本用例把这个前提钉住。
    #[test]
    fn is_usable_treats_expired_model_cooldown_as_available() {
        let policy = policy();
        let mut recovered = base_entry("recovered");
        recovered.model_cooldowns.insert(
            "m1".to_string(),
            ModelCooldown {
                until_ms: NOW - 1, // 刚过期
                reset_at_ms: 0,
                reason: String::new(),
                hits: 1,
            },
        );
        let map = pool_of(vec![recovered.clone()]);

        assert!(
            is_usable(&map, &policy, "recovered", NOW, "m1"),
            "已过期的模型级冷却不得判为不可用（is_usable 不 prune，判据必须自带过期判断）"
        );

        // 与 pick 的行为互为印证：只有这一个候选，必须能选到它。
        let mut for_pick = pool_of(vec![recovered]);
        let mut seq = 0;
        assert_eq!(
            pick(
                &mut for_pick,
                &mut seq,
                &policy,
                NOW,
                None,
                "m1",
                &HashSet::new(),
                1
            )
            .as_deref(),
            Some("recovered")
        );
    }
}
