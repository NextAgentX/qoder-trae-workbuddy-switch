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
pub mod pick;

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use chrono::{NaiveDate, TimeZone};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub use entry::{CoolKind, CostTier, ModelCooldown, ModelCost, PoolEntry};
pub use entry_like::PoolEntryLike;
pub use pick::{pick, pick_with_preference, weight_of, PickPolicy, SHORTLIST_SIZE};

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
    pub total_credits: f64,
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
    /// 其它 4xx → 仅记错，不做治理动作。
    Client,
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

/// 会话失效标记。
const SESSION_DEAD_MARKERS: &[&str] = &["Offline user session not found", "12153"];

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
pub fn parse_reset_at_ms(body: &str) -> Option<i64> {
    parse_reset_at_ms_from_json(body).or_else(|| parse_reset_at_ms_from_text(body))
}

/// 结构化形态：JSON 里的数值键。
fn parse_reset_at_ms_from_json(body: &str) -> Option<i64> {
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
    let naive = NaiveDate::from_ymd_opt(year as i32, month, day)?.and_hms_opt(hour, minute, second)?;
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
            reset_at_ms: parse_reset_at_ms(body),
            model_scoped,
        };
    }
    if status == 404 {
        return UpstreamEvent::NotFound;
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
struct PersistedState {
    saved_at_ms: i64,
    accounts: BTreeMap<String, PersistedEntry>,
}

impl Default for PersistedState {
    fn default() -> Self {
        Self {
            saved_at_ms: 0,
            accounts: BTreeMap::new(),
        }
    }
}

/// 账号池。
pub struct Pool {
    entries: BTreeMap<String, PoolEntry>,
    config: PoolConfig,
    pick_seq: u64,
    dirty: bool,
}

impl Pool {
    /// 新建空池。
    pub fn new(config: PoolConfig) -> Self {
        Self {
            entries: BTreeMap::new(),
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

    /// 选号；`preferred` 为**偏好 uid**（可用则优先，不可用则回落，绝不拒绝服务）。
    ///
    /// 偏好有两个来源，优先级见 `relay::select_account`：显式固定
    /// （[`crate::AccountStrategy::Pinned`]）**高于**会话粘性。
    /// 算法见 [`pick::pick_with_preference`]。
    pub fn pick_account(
        &mut self,
        now_ms: i64,
        realm: Option<RealmTag>,
        model: &str,
        tried: &HashSet<String>,
        seed: u64,
        preferred: Option<&str>,
    ) -> Option<String> {
        let policy = self.policy();
        pick_with_preference(
            &mut self.entries,
            &mut self.pick_seq,
            &policy,
            now_ms,
            realm,
            model,
            tried,
            seed,
            preferred,
        )
    }

    /// 指定 uid 此刻是否可用于该请求。
    ///
    /// 供**会话粘性**使用：粘性先问「上次那个账号还能不能用」，能用就复用、不能用就解绑换号。
    /// 粘性是**优化而非约束**，所以这里必须能明确判出「不可用」，绝不能因为粘性而拒绝服务。
    ///
    /// ★ 判据与 [`Self::pick_account`] **同源**（共用 `pick::entry_usable`）——
    /// 若这里另写一套且更宽松，粘性就会把请求钉到 `pick` 认为不可用的账号上
    /// （症状：明明有别的号可用却一直失败）。
    pub fn is_usable_for(&self, uid: &str, now_ms: i64, model: &str) -> bool {
        let policy = self.policy();
        pick::is_usable(&self.entries, &policy, uid, now_ms, model)
    }

    /// 申请在途额度；返回是否成功（失败表示该号已占满）。
    pub fn acquire(&mut self, uid: &str) -> bool {
        let policy = self.policy();
        let Some(entry) = self.entries.get_mut(uid) else {
            return false;
        };
        let limit = match entry.realm {
            Some(RealmTag::Global) if policy.max_in_flight_global > 0 => policy.max_in_flight_global,
            _ => policy.max_in_flight,
        };
        if limit > 0 && entry.in_flight >= limit {
            return false;
        }
        entry.in_flight += 1;
        true
    }

    /// 释放在途额度（幂等；重复释放不会把计数压成负数）。
    pub fn release(&mut self, uid: &str) {
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
        entry.note_success(now_ms);
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
        entry.note_error(now_ms);
        self.dirty = true;

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
                    entry.cooldown_model(
                        now_ms,
                        model,
                        reset_at_ms.unwrap_or(0),
                        config.soft_rate_max_ms,
                        "6004 model rate limit",
                    );
                } else {
                    match reset_at_ms {
                        Some(reset_at) => entry.cooldown_soft_until(
                            now_ms,
                            *reset_at,
                            config.soft_rate_max_ms,
                            "429 rate limit",
                        ),
                        None => {
                            entry.cooldown_soft_backoff(
                                now_ms,
                                config.soft_rate_ms,
                                config.soft_rate_max_ms,
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
            UpstreamEvent::Client => false,
        }
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
        let mut summary = PoolSummary {
            total: 0,
            ..PoolSummary::default()
        };
        for entry in self.entries.values() {
            if realm.is_some() && entry.realm != realm {
                continue;
            }
            summary.total += 1;
            match entry.status_of(now_ms) {
                "disabled" => summary.disabled += 1,
                "cooling" => summary.cooling += 1,
                // `expired` 本侧无判据（见 `PoolEntry::status_of`），恒为 0。
                "no_credits" => summary.zero_credits += 1,
                _ => summary.available += 1,
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
    /// `status` / `credits` / `cooling` / `cooldownUntil` / `cooldownReason` / `disabled`。
    pub fn status_list(&self, now_ms: i64) -> Vec<Value> {
        self.entries
            .values()
            .map(|entry| {
                let status = entry.status_of(now_ms);
                let cooldown_until = entry.expiry_ms();
                json!({
                    "uid": entry.uid,
                    "name": entry.nickname,
                    "realm": realm_key(entry.realm),
                    "status": status,
                    // 余额未知 ⇒ `null`（Trae 侧同义），前端显示为「—」。
                    "credits": (entry.credits_refreshed_ms > 0).then_some(entry.credits as f64),
                    "cooling": status == "cooling",
                    "cooldownUntil": (cooldown_until > 0).then_some(cooldown_until),
                    "cooldownReason": (!entry.reason.is_empty()).then(|| entry.reason.clone()),
                    "disabled": entry.disabled,
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
                    entry.rejection(now_ms)
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
                let mut item = json!({
                    "uid": entry.uid,
                    "realm": realm_key(entry.realm),
                    "nickname": entry.nickname,
                    "credits": entry.credits,
                    "cooling": now_ms < entry.until_ms,
                    "disabled": entry.disabled,
                    "in_flight": entry.in_flight,
                    "breaker_fails": entry.fails,
                });
                if let Some(kind) = entry.cool_kind {
                    item["cool_kind"] = json!(match kind {
                        CoolKind::Hard => "hard",
                        CoolKind::Soft => "soft",
                    });
                }
                let remaining = entry.cool_remaining_ms(now_ms);
                if remaining > 0 {
                    item["cool_remaining_sec"] = json!(remaining / 1000);
                }
                if entry.until_ms > 0 {
                    item["until_ms"] = json!(entry.until_ms);
                }
                if !entry.reason.is_empty() {
                    item["reason"] = json!(entry.reason);
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

        let mut healthy = 0;
        let mut cooling = 0;
        let mut disabled = 0;
        let mut in_flight_full = 0;
        let mut realm_totals: BTreeMap<&str, [i64; 4]> = BTreeMap::new();
        for entry in self.entries.values() {
            let is_healthy = entry.healthy(now_ms);
            let is_cooling = now_ms < entry.until_ms || now_ms < entry.breaker_until_ms;
            let full = self.policy().max_in_flight > 0
                && entry.in_flight >= self.policy().max_in_flight;
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
            let bucket = realm_totals
                .entry(realm_key(entry.realm))
                .or_insert([0; 4]);
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
        assert_eq!(config.max_in_flight_global, 2, "0 必须归一到 2，无「不限」语义");
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
                assert_eq!(
                    reset_at_ms,
                    Some(1_789_547_664_000),
                    "解冻时刻必须从 msg 文本解出（UTC+8）"
                );
            }
            other => panic!("应为 SoftRate，实际 {other:?}"),
        }
    }

    #[test]
    fn parse_reset_at_handles_seconds_milliseconds_and_nesting() {
        // 秒级
        assert_eq!(parse_reset_at_ms("{\"resetTime\":1700000000}"), Some(1_700_000_000_000));
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
            &vec![json!({"uid": "new", "nickname": "新号"})],
            Some(RealmTag::Cn),
        );
        assert_eq!(pool.len(), 2, "对齐是增量，不得删除既有账号");
        assert!(pool.get("keep").is_some());
        assert!(pool.get("new").is_some());
    }

    #[test]
    fn sync_accounts_skips_missing_or_empty_uid() {
        let mut pool = Pool::new(PoolConfig::default());
        pool.sync_accounts(
            &vec![json!({"nickname": "无 uid"}), json!({"uid": ""})],
            None,
        );
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

    #[test]
    fn global_realm_uses_stricter_in_flight_limit() {
        let mut pool = Pool::new(PoolConfig::default());
        pool.upsert("g1", Some(RealmTag::Global), "");
        assert!(pool.acquire("g1"));
        assert!(pool.acquire("g1"));
        assert!(!pool.acquire("g1"), "国际版上限为 2");
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

        // 冷却结束后再次触发 → 翻倍
        let later = NOW + first + 1;
        pool.apply_upstream_error("u1", "", &event, later);
        assert_eq!(pool.get("u1").unwrap().until_ms - later, 1_200_000);
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
        assert!(pool.apply_upstream_error("u1", "", &event, NOW), "第三次应禁用");
        assert!(pool.get("u1").unwrap().disabled);
    }

    #[test]
    fn apply_error_server_trips_breaker_at_threshold() {
        let mut pool = Pool::new(PoolConfig::default());
        pool.upsert("u1", None, "");
        for _ in 0..2 {
            pool.apply_upstream_error("u1", "", &UpstreamEvent::Server, NOW);
        }
        assert_eq!(pool.get("u1").unwrap().breaker_until_ms, 0, "未达阈值不熔断");
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
        assert!(!object.contains_key("cooldown_until"), "不得出现 snake_case");
        assert!(!object.contains_key("nickname"), "展示名统一叫 name");
        // 无冷却 / 无原因 ⇒ `null`，前端据此不显示。
        assert_eq!(object["cooldownUntil"], Value::Null);
        assert_eq!(object["cooldownReason"], Value::Null);
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
        assert!(object["cooldownUntil"].as_i64().unwrap_or(0) > NOW, "有解冻时刻");
        assert!(
            object["cooldownReason"].as_str().unwrap_or("").len() > 0,
            "有冷却原因"
        );
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
        let ok = lines.iter().find(|line| line.contains("正常")).expect("存在");
        assert!(ok.contains("可用"), "{ok}");
        assert!(ok.contains("积分=9"), "{ok}");
        let dead = lines.iter().find(|line| line.contains("会话失效")).expect("存在");
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
        assert!((entry.success_ema - 0.4).abs() < 1e-9, "已有 EMA 不得被计数覆盖");
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
            .pick_account(NOW, Some(RealmTag::Cn), "", &tried, 1, None)
            .expect("有候选");
        tried.insert(first.clone());

        // 失败的号进入冷却后，换号重试应拿到另一个
        pool.apply_upstream_error(&first, "", &UpstreamEvent::NotFound, NOW);
        let second = pool
            .pick_account(NOW, Some(RealmTag::Cn), "", &tried, 2, None)
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
            .pick_account(NOW, Some(RealmTag::Cn), "", &HashSet::new(), 1, Some("poor"))
            .expect("有候选");
        assert_eq!(picked, "poor", "偏好账号可用时必须优先选中");

        // ② 偏好已被本请求试过 ⇒ 跳过它、回落自动择优（否则会在同一请求里重复撞）。
        let tried: HashSet<String> = ["poor".to_string()].into_iter().collect();
        let picked = pool
            .pick_account(NOW, Some(RealmTag::Cn), "", &tried, 2, Some("poor"))
            .expect("仍有候选");
        assert_ne!(picked, "poor", "已试过的偏好必须被跳过");

        // ③ 偏好不在池里 ⇒ 回落自动择优（绝不返回 None）。
        let picked = pool
            .pick_account(NOW, Some(RealmTag::Cn), "", &HashSet::new(), 3, Some("no-such"))
            .expect("偏好不存在也必须回落到自动择优");
        assert!(
            ["rich", "poor"].contains(&picked.as_str()),
            "回落结果必须来自池内：{picked}"
        );

        // ④ 无偏好 ⇒ 不引入任何行为变化（仍能选出账号）。
        assert!(
            pool.pick_account(NOW, Some(RealmTag::Cn), "", &HashSet::new(), 4, None)
                .is_some(),
            "无偏好时行为不变"
        );
    }

    #[test]
    fn credits_refresh_interval_normalizes_non_positive_to_default() {
        assert_eq!(PoolConfig::default().credits_refresh_interval_ms, 30 * 60 * 1000);
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
        assert!(!pool.thaw_hard_credit_if_recovered("soft", 500), "软冷却不得解冻");
        assert!(!pool.thaw_hard_credit_if_recovered("soft", 0));

        // 硬冷却但余额为 0：不得解冻。
        pool.upsert("hard", Some(RealmTag::Cn), "");
        pool.apply_upstream_error("hard", "", &UpstreamEvent::HardCredit, NOW);
        assert!(!pool.thaw_hard_credit_if_recovered("hard", 0), "0 余额不得解冻");
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
        assert!(entry.reason.contains(HARD_CREDIT_COOLDOWN_REASON), "reason 不得被改动");
    }
}
