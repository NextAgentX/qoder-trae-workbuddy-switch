//! Qoder 账号池：**区域级**（一本区域库 = 一个池），选号复用共享算法。
//!
//! ## 设计要点
//!
//! - 条目 [`QoderPoolEntry`] 实现 [`PoolEntryLike`] —— 选号直接调
//!   [`crate::pool::pick_with_preference`]（四因子加权 → 成本分层 → 短名单 → LRU 兜底）。
//!   Qoder 侧**缺的数据一律中性降级**（[`PoolEntryLike`] 契约的硬要求）。
//! - **模型级冷却**：上游限流是**按模型**报的（「usage exceeds frequency limit…
//!   switch to the other models」）⇒ 429 只冷当前模型，不把整个账号打死。
//! - **账号级冷却**：401/403（凭据死）给长冷却并标记 `auth_dead`（刷新成功可复活）；
//!   传输层 / 5xx 给短冷却。
//! - 池从账号库 `sync_for(region)` 重建（每次网关取号前同步，保证「导入即入池」）。

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use serde_json::{json, Value};

use buddy_switch_core::modules::qoder::{account, region::QoderRegion};

use crate::pool::entry::CostTier;
use crate::pool::entry_like::PoolEntryLike;
use crate::pool::pick::{pick_with_preference, PickPolicy};
use crate::pool::{ProbeOutcome, RealmTag, UpstreamEvent};

/// 探测回填的**冷却下限**（毫秒）—— 与 WorkBuddy 侧 `crate::pool::MIN_PROBE_COOLDOWN_MS` **同值**。
///
/// ## 为什么需要它
///
/// Qoder 上游对「模型级限流」**不声明解冻墙钟**（`classify_http` 只看状态码），
/// 探测拿到 429 时 `reset_at_ms` 恒为 `None`。若直接落 `now + 1ms` 就等于**没冷却**
/// （下一毫秒又去撞同一堵墙），而调用方却以为「已置冷却」。
///
/// ## 为什么是 60s（而不是复用 WB 的常量）
///
/// 本模块**已经**用 60s 作为 429 的模型级冷却时长（见 [`QoderPool::apply_error`] 的
/// `RateLimited` 分支：`now + 60_000`）—— 探测回填的下限取**同一个值**，两者才不漂移。
/// 两个常量**分模块各自声明**（WB 的 `MIN_PROBE_COOLDOWN_MS` 与 Qoder 这条）：它们约束
/// 的是**两个平台各自的**回填语义，共用一个反而会把「改一个平台」误伤另一个平台。
pub const MIN_PROBE_COOLDOWN_MS: i64 = 60_000;

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}

/// 上游错误分类（决定冷却时长与是否换号）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QoderErrKind {
    /// 429：按**模型**冷却 − 换号。
    RateLimited,
    /// 401/403：凭据死 ⇒ 长冷却 + 标记，换号。
    AuthDead,
    /// 5xx / 传输层：短冷却 + 换号。
    Transient,
    /// 4xx（非上述）：客户端参数错 ⇒ **不换号**，原样回给调用方。
    ClientError,
    /// ★ 上游**排队**（`10605` / `isQueued`，通常裹在 401/403 信封里）：
    /// 账号没问题、也不是凭据死 ⇒ **不冷却、不标 dead**，只轮换。
    Queued,
    /// ★ 402：**余额耗尽**。额度按每日 10:00（UTC+8）重置 ⇒ 短冷却毫无意义
    /// （几分钟后放出去只会再撞一次），要停到下一个重置窗口，见 `quota::park_exhausted`。
    BalanceExhausted,
}

impl QoderErrKind {
    pub fn as_str(self) -> &'static str {
        match self {
            QoderErrKind::RateLimited => "rate_limited",
            QoderErrKind::AuthDead => "auth_dead",
            QoderErrKind::Transient => "transient",
            QoderErrKind::ClientError => "client_error",
            QoderErrKind::Queued => "queued",
            QoderErrKind::BalanceExhausted => "balance_exhausted",
        }
    }

    /// 是否值得换号重试。
    pub fn should_rotate(self) -> bool {
        !matches!(self, QoderErrKind::ClientError)
    }
}

/// HTTP 状态码分类（**只看状态码**；探测路径没有 body 可用）。
pub fn classify_http(status: u16) -> QoderErrKind {
    match status {
        429 => QoderErrKind::RateLimited,
        // ★ 402 = 余额耗尽（**不是**客户端参数错）：从前落到 `ClientError` ⇒ 不换号、
        //   不冷却，每次请求都去撞同一个已经没钱的账号。
        402 => QoderErrKind::BalanceExhausted,
        401 | 403 => QoderErrKind::AuthDead,
        500..=599 => QoderErrKind::Transient,
        400..=499 => QoderErrKind::ClientError,
        _ => QoderErrKind::Transient,
    }
}

/// ★ 这条 detail 是不是「上游排队」信号（`10605` / `isQueued`）。
///
/// 为什么必须单独判：Qoder 把排队塞进 **401/403** 信封里，只看状态码会把它当成
/// 「凭据失效」⇒ 账号被标 `auth_dead` + 30 分钟冷却 ⇒ 单号池直接变「无可用账号」，
/// 面板看起来就是凭证坏了（参考实现 issue #71 的同一个坑）。
///
/// ⚠️ 必须排除**真正的**会话失效词：`401 token expired` 也带 401，不能被当成排队放过去。
pub(crate) fn queue_signal(detail: &str) -> bool {
    let low = detail.to_ascii_lowercase();
    let queued = detail.contains("10605") || low.contains("isqueued");
    if !queued {
        return false;
    }
    const SESSION_DEAD_HINTS: [&str; 5] = [
        "token_invalid",
        "invalid token",
        "token expired",
        "session expired",
        "login expired",
    ];
    !SESSION_DEAD_HINTS.iter().any(|hint| low.contains(hint))
}

/// 排队时给客户端看的文案（流式 / 非流式**同一句**，避免两处各拼一份）。
///
/// 入参可能已经是我们自己的措辞（信封错误帧先格式化过一次）⇒ 幂等，不重复前缀。
pub(crate) fn queue_message(detail: &str) -> String {
    if detail.contains("上游排队中") {
        detail.to_string()
    } else {
        format!("上游排队中（不是账号或凭据问题，稍后重试即可）：{detail}")
    }
}

/// 上游 `(status, detail)` → **对外**错误形状（issue #129）。
///
/// ## 为什么对外必须换一套形状
///
/// 客户端（DeepSeek Harness 的 `openai-completions` 适配器）**只做文本正则**：
/// `429` / `rate limit` → `RATE_LIMIT`（可重试），`5\d\d` → `SERVER`（可重试），
/// `401|403` → `AUTH`（**终态**）。Qoder 的排队（`10605` 裹在 403 里）与余额耗尽
/// 原样透出时，客户端要么归成 `AUTH`（终态、还提示「密钥无效」）、要么归成
/// `PI_AI_ERROR` —— 两条都不会重试，而它们本可以靠「换号 + 重跑」救回来。
///
/// 对外文案**不拼上游 detail**：detail 里常有 `quota` / `insufficient` 之类短语，
/// 而客户端的 `QUOTA` 判定排在 `RATE_LIMIT` **之前**且不可重试
/// （见 [`crate::failover`] 模块头）。真实原因由日志与账号池状态承载。
pub fn client_error_of(status: u16, detail: &str) -> crate::failover::ClientError {
    let kind = classify_http_with_detail(status, detail);
    let event = match kind {
        // 限流 / 排队 / 余额耗尽 / 凭据失效 ⇒ 统一 429：对客户端都是「这个号不行了」。
        // ★ `AuthDead` 也走 429 而不是 401 —— 流中途的凭据问题可能只是这一个号过期了。
        QoderErrKind::RateLimited
        | QoderErrKind::Queued
        | QoderErrKind::BalanceExhausted
        | QoderErrKind::AuthDead => UpstreamEvent::SoftRate {
            reset_at_ms: None,
            model_scoped: false,
        },
        QoderErrKind::Transient => UpstreamEvent::Server,
        QoderErrKind::ClientError => UpstreamEvent::Client,
    };
    crate::failover::client_error(&event, "", "")
}

/// 带 body 的分类：`401/403` 里若是排队信号 ⇒ [`QoderErrKind::Queued`]（不惩罚账号）。
pub fn classify_http_with_detail(status: u16, detail: &str) -> QoderErrKind {
    if matches!(status, 401 | 403) && queue_signal(detail) {
        return QoderErrKind::Queued;
    }
    classify_http(status)
}

fn classify_err_kind_text(kind: QoderErrKind) -> &'static str {
    kind.as_str()
}

/// 池中的一条 Qoder 账号。
#[derive(Debug, Clone)]
pub struct QoderPoolEntry {
    pub uid: String,
    pub name: String,
    pub access_token: String,
    pub refresh_token: String,
    /// 凭据死（401/403）：长冷却 + 需刷新才能复活。
    pub auth_dead: bool,
    /// 账号级冷却解冻时刻（毫秒；0 = 无）。
    pub cooldown_until: i64,
    /// 冷却原因（诊断）。
    pub cooldown_reason: Option<String>,
    /// 模型级冷却：模型名 → 解冻时刻（毫秒）。
    pub model_cooldowns: HashMap<String, i64>,
    pub last_error: Option<String>,
    /// 该账号**在途请求数**（进程级；`BUDDY_SWITCH_QODER_MAX_INFLIGHT` > 0 时才作为闸门）。
    ///
    /// 用 `Arc<AtomicUsize>` 而不是普通计数：release 发生在流结束 / 客户端断开的
    /// **Drop** 里，那里不能 await 池锁；原子量让释放无需拿锁、也不会泄漏。
    pub inflight: Arc<AtomicUsize>,
    success_ema: f64,
    error_ema: f64,
    last_used_ms: i64,
    used_seq: u64,
}

impl QoderPoolEntry {
    fn new(account: &account::RawAccount) -> Self {
        Self {
            uid: account.uid.clone(),
            name: account::display_name(account),
            access_token: account.access_token.clone().unwrap_or_default(),
            refresh_token: account.refresh_token.clone().unwrap_or_default(),
            auth_dead: false,
            cooldown_until: 0,
            cooldown_reason: None,
            model_cooldowns: HashMap::new(),
            last_error: None,
            inflight: Arc::new(AtomicUsize::new(0)),
            success_ema: 0.0,
            error_ema: 0.0,
            last_used_ms: 0,
            used_seq: 0,
        }
    }

    /// 是否有可调用的凭据（无 access 的账号**不入候选**，PAT 不能直接推理）。
    pub fn has_access(&self) -> bool {
        !self.access_token.is_empty()
    }

    /// 是否需要**人工干预**才能重新可用：无凭据（PAT 不可推理）/ 凭据死（需刷新令牌）。
    ///
    /// 对应到共用池卡上的 `disabled` 状态（「已禁用」格子 + 行内「恢复」出口）。
    /// 与 `cooling` 的区别是：冷却会自己过去，这两个状态**不会自己好**
    /// —— 刷新令牌（`qoder_refresh_account`）是唯一的出口。
    pub fn needs_intervention(&self) -> bool {
        !self.has_access() || self.auth_dead
    }

    /// 该条目的可读原因（只在真的不可用时调用）。
    fn unusable_reason(&self, now: i64) -> String {
        if !self.has_access() {
            return "没有可调用的访问令牌（PAT 只能用于非推理链路）".to_string();
        }
        if self.auth_dead {
            return self
                .cooldown_reason
                .clone()
                .unwrap_or_else(|| "凭据失效（需刷新令牌）".to_string());
        }
        if self.cooldown_until > now {
            return self
                .cooldown_reason
                .clone()
                .unwrap_or_else(|| "上游暂时不可用".to_string());
        }
        "可用".to_string()
    }

    /// 诊断一行。
    pub fn describe(&self, now: i64) -> String {
        let state = if self.auth_dead {
            "凭据失效（需刷新）"
        } else if self.cooldown_until > now {
            "冷却中"
        } else {
            "可用"
        };
        format!("{} · {state}", self.name)
    }
}

impl PoolEntryLike for QoderPoolEntry {
    fn uid(&self) -> &str {
        &self.uid
    }

    fn credits(&self) -> i64 {
        // Qoder 的额度不以「池内积分」建模（额度查询是独立只读视图）⇒ 中性 0。
        0
    }

    fn expiring_credits(&self, _now_ms: i64) -> i64 {
        0
    }

    fn success_ema(&self) -> f64 {
        self.success_ema
    }

    fn error_ema(&self) -> f64 {
        self.error_ema
    }

    fn last_used_ms(&self) -> i64 {
        self.last_used_ms
    }

    fn used_seq(&self) -> u64 {
        self.used_seq
    }

    fn in_flight(&self) -> i64 {
        0
    }

    fn realm(&self) -> Option<RealmTag> {
        // 池本身按区域分家（一本区域库一个池）⇒ 不需要 realm 过滤。
        None
    }

    fn is_disabled(&self) -> bool {
        false
    }

    fn cool_expiry_ms(&self) -> i64 {
        self.cooldown_until
    }

    fn is_hard_cooled(&self, _now_ms: i64) -> bool {
        // 无「余额不足」概念 ⇒ 无硬冷却。
        false
    }

    fn healthy_for_request(&self, now: i64, model: &str) -> bool {
        if !self.has_access() || self.auth_dead {
            return false;
        }
        if self.cooldown_until > now {
            return false;
        }
        match self.model_cooldowns.get(model) {
            Some(until) => *until <= now,
            None => true,
        }
    }

    fn prune_model_cooldowns(&mut self, now: i64) {
        self.model_cooldowns.retain(|_, until| *until > now);
    }

    fn cost_per_1k(&self, _now_ms: i64, _model: &str, _ttl_ms: i64) -> f64 {
        0.0
    }

    fn cost_tier(&self, _now_ms: i64, _model: &str, _ttl_ms: i64) -> CostTier {
        CostTier::Unknown
    }

    fn mark_picked(&mut self, now: i64, seq: u64) {
        self.last_used_ms = now;
        self.used_seq = seq;
    }

    fn mark_probed(&mut self, now: i64) {
        self.last_used_ms = now;
    }
}

/// 最小选号间隔（毫秒）：`BUDDY_SWITCH_QODER_MIN_INTERVAL_MS`，缺省 / `0` = 关闭。
///
/// 语义是**软约束**（`PickPolicy::min_pick_gap_ms`）：短名单里「刚用过、还没到间隔」的号
/// 会被剔除；若全员都在间隔内，选号内核会按 LRU 兜底放行 —— **不会拒服务**。
/// 参考实现只对团队共享账号启用这套节奏保护（issue #56），故本仓默认关。
pub fn parse_min_pick_gap(raw: Option<&str>) -> i64 {
    raw.and_then(|text| text.trim().parse::<i64>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(0)
}

/// 实际生效的最小选号间隔（每次选号都读环境变量 ⇒ 运行期可调）。
fn min_pick_gap_ms() -> i64 {
    parse_min_pick_gap(
        std::env::var("BUDDY_SWITCH_QODER_MIN_INTERVAL_MS")
            .ok()
            .as_deref(),
    )
}

/// Qoder 的选号策略（**独立函数**，别动 [`PickPolicy`] 的默认值 —— 那是 WB 的行为基线）。
pub fn qoder_pick_policy() -> PickPolicy {
    PickPolicy {
        expiring_weight: 0.0,
        expiry_urgency_weight: 0.0,
        idle_weight_per_hour: 0.0,
        idle_weight_max: 0.0,
        // 防惊群间隔：默认 0（关），可用环境变量开（软约束，见 `parse_min_pick_gap`）。
        min_pick_gap_ms: min_pick_gap_ms(),
        model_cost_ttl_ms: 0,
        max_in_flight: 0,
        max_in_flight_global: 0,
        // 账号数量少、用户要可预期行为（与 Trae 侧同一取舍）。
        deterministic: true,
    }
}

/// 一个区域的账号池。
#[derive(Debug, Default)]
pub struct QoderPool {
    entries: BTreeMap<String, QoderPoolEntry>,
    pick_seq: u64,
    /// 「刷新成功」等外部事件用于强制重建的脏标记。
    dirty: bool,
    /// 本池所属区域（由 `sync_for` 固化）。
    ///
    /// 用途只有一个：拼**排除表的作用域键**（`qoder:cn` / `qoder:global`）。
    /// 池本身按区域分家，但选号内核不感知区域，故需要在这里留一份。
    region: Option<QoderRegion>,
}

/// 每账号在途请求上限（`BUDDY_SWITCH_QODER_MAX_INFLIGHT`；0 / 缺省 = 关闭）。
///
/// 为什么默认关：参考实现只对**团队 / 企业账号**启用这套自用保护（issue #56），
/// 本仓没有团队账号这一分类 —— 默认给个人账号降速是错的。需要「多设备共用一个号 /
/// 想更保守」的用户可以显式开。
pub fn max_inflight() -> usize {
    std::env::var("BUDDY_SWITCH_QODER_MAX_INFLIGHT")
        .ok()
        .and_then(|raw| raw.trim().parse::<usize>().ok())
        .unwrap_or(0)
}

/// 在途计数的 RAII 守卫：**Drop 时自减**（流结束 / 客户端断开都会走到）。
pub struct InflightGuard {
    counter: Arc<AtomicUsize>,
}

impl Drop for InflightGuard {
    fn drop(&mut self) {
        // 防下溢：如果释放路径被意外走两次，也不能把计数减成天文数字
        // （手写 CAS 循环：`fetch_update` 已被标记 deprecated）。
        let mut current = self.counter.load(Ordering::SeqCst);
        while current > 0 {
            match self.counter.compare_exchange(
                current,
                current - 1,
                Ordering::SeqCst,
                Ordering::SeqCst,
            ) {
                Ok(_) => break,
                Err(actual) => current = actual,
            }
        }
    }
}

impl QoderPool {
    /// 抢占一个在途名额；`limit == 0` ⇒ 不启用闸门（恒返回守卫，计数仍记）。
    ///
    /// 返回 `None` 表示**该账号已满**，调用方应当跳过它（换号或报忙），**不要排队等待**：
    /// 网关卡在 `chat_for_region` 里等，会把客户端超时拖成「上游很慢」。
    pub fn acquire(&mut self, uid: &str, limit: usize) -> Option<InflightGuard> {
        let entry = self.entries.get(uid)?;
        if limit > 0 && entry.inflight.load(Ordering::SeqCst) >= limit {
            return None;
        }
        // 先加再核（并发下可能短暂超一）⇒ 超了就回滚，宁可这次跳过也不突破上限。
        entry.inflight.fetch_add(1, Ordering::SeqCst);
        if limit > 0 && entry.inflight.load(Ordering::SeqCst) > limit {
            entry.inflight.fetch_sub(1, Ordering::SeqCst);
            return None;
        }
        Some(InflightGuard {
            counter: entry.inflight.clone(),
        })
    }

    pub fn new() -> Self {
        Self::default()
    }

    /// 从账号库重建（保留同 uid 的运行时状态：冷却 / EMA / used_seq）。
    ///
    /// 返回池内条目数。`dirty` 时强制重建一次；否则也总是重建 ——
    /// 账号库是唯一事实来源，池只是它的运行态视图（import 之后立刻生效）。
    pub fn sync_for(&mut self, region: QoderRegion) -> usize {
        self.region = Some(region);
        let file = account::load_accounts_for(region);
        let mut next: BTreeMap<String, QoderPoolEntry> = BTreeMap::new();
        for account in &file.accounts {
            let mut entry = QoderPoolEntry::new(account);
            if let Some(previous) = self.entries.get(&entry.uid) {
                // 保留运行态；令牌与名字以库为准（刷新 / 改备注后即时生效）。
                entry.auth_dead = previous.auth_dead && entry.access_token == previous.access_token;
                // 在途计数是进程级事实：重建条目时按 uid 继承，否则「正在跑的请求」
                // 会凭空消失（闸门失效 + 释放时少减一次）。
                entry.inflight = previous.inflight.clone();
                entry.cooldown_until = previous.cooldown_until;
                entry.cooldown_reason = previous.cooldown_reason.clone();
                entry.model_cooldowns = previous.model_cooldowns.clone();
                entry.last_error = previous.last_error.clone();
                entry.success_ema = previous.success_ema;
                entry.error_ema = previous.error_ema;
                entry.last_used_ms = previous.last_used_ms;
                entry.used_seq = previous.used_seq;
            }
            next.insert(entry.uid.clone(), entry);
        }
        self.entries = next;
        self.dirty = false;
        self.entries.len()
    }

    /// 排除表的作用域键（`cn` / `global`；尚未同步过时回落 `cn`）。
    fn region_key(&self) -> &'static str {
        self.region.unwrap_or(QoderRegion::Cn).as_str()
    }

    /// 用户显式排除的 uid → 备注（选号与视图共用同一份读口）。
    fn excluded_map(&self) -> std::collections::BTreeMap<String, String> {
        crate::pool::exclusions::load_for(&format!("qoder:{}", self.region_key()))
    }

    pub fn entries(&self) -> impl Iterator<Item = &QoderPoolEntry> {
        self.entries.values()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 选中一个可用于 `model` 的账号（排除 `tried`；`preferred` 优先但可回落）。
    pub fn pick(
        &mut self,
        model: &str,
        tried: &HashSet<String>,
        preferred_uid: Option<&str>,
        // ★ issue #76：本密钥的允许名单（`None` = 全部账号）。
        allowed: Option<&HashSet<String>>,
    ) -> Option<String> {
        let now = now_ms();
        let policy = qoder_pick_policy();
        let seed = now as u64 ^ (self.pick_seq.wrapping_mul(0x9E37_79B9_7F4A_7C15));
        // 用户显式排除的账号：**硬约束**（自动选号 / 兜底 / 偏好都跳过）。
        let excluded = crate::pool::exclusions::uid_set(&format!("qoder:{}", self.region_key()));
        // 偏好值可能是**策略哨兵**（`__max_credits__` / `__random__`），见 `parse_preferred`。
        let (mode, preferred_uid) = crate::pool::parse_preferred(preferred_uid);
        let input = crate::pool::PickInput {
            realm: None,
            model,
            tried,
            excluded: &excluded,
            allowed,
            mode,
            seed,
        };
        pick_with_preference(
            &mut self.entries,
            &mut self.pick_seq,
            &policy,
            now,
            &input,
            preferred_uid,
        )
    }

    /// 成功收尾：复位错误态 + EMA 上升。
    pub fn note_success(&mut self, uid: &str) {
        if let Some(entry) = self.entries.get_mut(uid) {
            entry.success_ema = entry.success_ema * 0.9 + 0.1;
            entry.error_ema *= 0.9;
            entry.cooldown_until = 0;
            entry.cooldown_reason = None;
            entry.last_error = None;
            entry.auth_dead = false;
        }
    }

    /// 记一次错误并按类型冷却。
    pub fn apply_error(&mut self, uid: &str, kind: QoderErrKind, reason: &str, model: &str) {
        let now = now_ms();
        let Some(entry) = self.entries.get_mut(uid) else {
            return;
        };
        /* ★ 排队**不是账号问题**：既不推 EMA、也不设冷却、更不标 `auth_dead`。
        推 EMA 会让健康账号在池里排到后面，最终表现为「无可用账号」。 */
        if kind == QoderErrKind::Queued {
            entry.last_error = Some(format!("queued: {reason}"));
            return;
        }
        entry.success_ema *= 0.9;
        entry.error_ema = entry.error_ema * 0.9 + 0.1;
        entry.last_error = Some(format!("{}: {reason}", classify_err_kind_text(kind)));
        match kind {
            QoderErrKind::RateLimited => {
                // 只冷**这个模型**（上游限流就是按模型报的），账号仍服务其它模型。
                entry
                    .model_cooldowns
                    .insert(model.to_string(), now + 60_000);
            }
            QoderErrKind::AuthDead => {
                entry.auth_dead = true;
                entry.cooldown_until = now + 30 * 60_000;
                entry.cooldown_reason = Some("凭据失效（需刷新令牌）".to_string());
            }
            QoderErrKind::Transient => {
                entry.cooldown_until = now + 10_000;
                entry.cooldown_reason = Some("上游暂时不可用".to_string());
            }
            QoderErrKind::BalanceExhausted => {
                // 停到下一个额度重置窗口（10:00 UTC+8）；写进池条目的冷却轴让选号
                // 立刻跳过，同时落进 `quota` 的停放账（跨重启有效）。
                let until = crate::qoder::quota::park_exhausted(
                    self.region.unwrap_or(QoderRegion::Cn),
                    uid,
                    reason,
                );
                entry.cooldown_until = until;
                entry.cooldown_reason = Some("余额耗尽（等待额度重置）".to_string());
            }
            QoderErrKind::ClientError | QoderErrKind::Queued => {}
        }
    }

    /// ★ **采纳一次探测结果** —— 语义与 WorkBuddy 的 [`crate::pool::Pool::adopt_probe_result`]
    /// **逐条对齐**（见那里的文档）。
    ///
    /// ## 只置/清 `model_cooldowns`，**绝不推 EMA / 错误计数**
    ///
    /// 探测是**我们主动打的**，被 429 本来就是常态。若走 [`QoderPool::apply_error`]，它会
    /// 推进 `success_ema` / `error_ema` / `last_error` —— 探测次数会混进退避与健康度，
    /// 冷却「越探越长」（自我实现的限流）。故本入口**只动** `model_cooldowns`：
    /// `success_ema` / `error_ema` / `last_error` / `cooldown_until` / `auth_dead` 逐字段不变。
    ///
    /// ## 三档语义
    ///
    /// - `Available` ⇒ **清掉**该模型的冷却（探到可用就该恢复）；
    /// - `ModelRateLimited` ⇒ **置**该模型冷却（Qoder 上游 429 是按模型报的，故没有账号级细分；
    ///   无墙钟时用 [`MIN_PROBE_COOLDOWN_MS`] 补齐下限）；
    /// - 其余档（`ModelBlocked` / `AccountRateLimited` / `Other`）⇒ **不动**。
    ///
    /// 返回**是否真的改变了冷却**（幂等：同一结论重复采纳不重复计数；既有冷却更晚时不会被缩短）。
    pub fn adopt_probe_result(
        &mut self,
        uid: &str,
        model: &str,
        outcome: &ProbeOutcome,
        now: i64,
    ) -> bool {
        let Some(entry) = self.entries.get_mut(uid) else {
            return false;
        };
        match outcome {
            ProbeOutcome::Available => {
                if model.is_empty() {
                    false
                } else {
                    entry.model_cooldowns.remove(model).is_some()
                }
            }
            // Qoder 池的 `model_cooldowns` 只存 `until`（`HashMap<String, i64>`）⇒
            // 没有「上游文案时刻」这一维可落，`upstream_reset_at_ms` 忽略。
            ProbeOutcome::ModelRateLimited { reset_at_ms, .. } => {
                if model.is_empty() {
                    return false;
                }
                let before = entry.model_cooldowns.get(model).copied().unwrap_or(0);
                // ★ 下限：未知 / 已过期的墙钟一律补齐（否则只冷 1ms = 等于没冷却）。
                let reset_at = reset_at_ms
                    .filter(|value| *value > now)
                    .unwrap_or(now.saturating_add(MIN_PROBE_COOLDOWN_MS));
                entry.model_cooldowns.insert(model.to_string(), reset_at);
                entry.model_cooldowns.get(model).copied().unwrap_or(0) > before
            }
            // 其余档（模型封禁 / 账号级限流 / 其它）在 Qoder 侧无对应冷却轴 ⇒ 不动。
            ProbeOutcome::ModelBlocked
            | ProbeOutcome::AccountRateLimited { .. }
            | ProbeOutcome::Other => false,
        }
    }

    /// 池视图（`/status` 与网关页「账号池」卡）。
    pub fn status_list(&self, now: i64) -> Vec<Value> {
        let excluded = self.excluded_map();
        self.entries
            .values()
            .map(|entry| {
                let exclusion = excluded.get(&entry.uid);
                json!({
                    "uid": entry.uid,
                    "name": entry.name,
                    "available": entry.healthy_for_request(now, ""),
                    // 用户显式排除（硬约束：选号 / 兜底 / 偏好全跳过）。
                    "excluded": exclusion.is_some(),
                    "excludedNote": exclusion
                        .cloned()
                        .filter(|note| !note.is_empty()),
                    "authDead": entry.auth_dead,
                    "cooling": entry.cooldown_until > now,
                    "cooldownUntil": entry.cooldown_until,
                    "cooldownReason": entry.cooldown_reason,
                    "modelCooldowns": entry.model_cooldowns,
                    "lastError": entry.last_error,
                    "successEma": entry.success_ema,
                    "errorEma": entry.error_ema,
                    "lastUsedMs": entry.last_used_ms,
                })
            })
            .collect()
    }

    /// 池计数（形状与 WB / Trae **共用同一张前端卡**的 `GatewayPoolSummary` 同源）。
    ///
    /// ## 为什么多一格 `model_cooling`、少两格 `expired` / `zero_credits`
    ///
    /// - Qoder 池**不建模积分**（额度是独立只读视图）⇒ 积分两格没有判据，
    ///   恒 0 且前端不渲染（`QODER_POOL_TILES`，与 WB 去掉「积分过期」同一纪律）。
    /// - 上游 429 是**按模型**报的（「…switch to the other models」）⇒ 一个账号可能
    ///   「对 A 模型冷却、对 B 模型仍可服务」。这是 Qoder 侧独有的真实状态，
    ///   用一格数出来比让用户去推 `modelCooldowns` 更诚实。
    ///
    /// `disabled` 的口径 = [`QoderPoolEntry::needs_intervention`]（无凭据 / 凭据死二者之一）
    /// —— `available` 与它们互斥且穷尽，三格相加恒等于 `total`。
    pub fn summary(&self, now: i64) -> Value {
        let excluded = self.excluded_map();
        let mut available = 0usize;
        let mut cooling = 0usize;
        let mut disabled = 0usize;
        let mut model_cooling = 0usize;
        let mut excluded_count = 0usize;
        for entry in self.entries.values() {
            // ★ 排除是互斥第六态：必须从 `available` 里扣掉，否则数字与事实不符。
            if excluded.contains_key(&entry.uid) {
                excluded_count += 1;
            } else if entry.needs_intervention() {
                disabled += 1;
            } else if entry.cooldown_until > now {
                cooling += 1;
            } else {
                available += 1;
            }
            if entry.model_cooldowns.values().any(|until| *until > now) {
                model_cooling += 1;
            }
        }
        json!({
            "total": self.entries.len(),
            "available": available,
            "cooling": cooling,
            "disabled": disabled,
            "expired": 0,
            "zero_credits": 0,
            "total_credits": 0,
            "model_cooling": model_cooling,
            "excluded": excluded_count,
        })
    }

    /// 账号明细（**共用池卡**的形状：`GatewayPoolAccount` —— WB / Trae 两侧同款字段名）。
    ///
    /// 与 [`QoderPool::status_list`]（网关 `/status` 的自用视图）刻意分家：
    /// 那边带池内诊断字段（EMA / `lastUsedMs` / 逐模型冷却表），这边只给卡片要的
    /// `status` 五态 + 冷却三件套。
    pub fn accounts_view(&self, now: i64) -> Vec<Value> {
        let excluded = self.excluded_map();
        self.entries
            .values()
            .map(|entry| {
                let needs_intervention = entry.needs_intervention();
                let cooling = entry.cooldown_until > now;
                let is_excluded = excluded.contains_key(&entry.uid);
                let status = if is_excluded {
                    "excluded"
                } else if needs_intervention {
                    "disabled"
                } else if cooling {
                    "cooling"
                } else {
                    "available"
                };
                json!({
                    "uid": entry.uid,
                    "name": entry.name,
                    "status": status,
                    // Qoder 池不建模积分 ⇒ 明确 `null`（卡片显示「—」），
                    // **不**填 0 —— 0 会被读成「这个账号没额度了」。
                    "credits": Value::Null,
                    "cooling": cooling,
                    // 冷却三件套只在**真的不可用**时给（与 WB / Trae 同一规则）：
                    // 一条早已过期的冷却原因挂在「可用」徽标旁边会被读成当前限流。
                    "cooldownUntil": if cooling && entry.cooldown_until > 0 {
                        Some(entry.cooldown_until)
                    } else {
                        None
                    },
                    "cooldownReason": if cooling || needs_intervention {
                        Some(entry.unusable_reason(now))
                    } else {
                        None
                    },
                    "disabled": needs_intervention,
                    "excluded": is_excluded,
                    "excludedNote": excluded
                        .get(&entry.uid)
                        .filter(|note| !note.is_empty())
                        .cloned(),
                    "authDead": entry.auth_dead,
                    "modelCooldowns": entry.model_cooldowns,
                })
            })
            .collect()
    }

    /// 逐账号「为什么不能路由」（诊断区，形状与 Trae 侧同款：`名字(尾8位:原因)`）。
    pub fn diagnose(&self, now: i64) -> Vec<String> {
        self.diagnose_for(now, None)
    }

    /// 逐账号诊断；`allowed` 生效时**只列这把 Key 允许用的账号**（issue #76）。
    ///
    /// 与 Trae 侧同款理由：白名单用户该看到的是「我允许用的那几个号」为什么不可用。
    pub fn diagnose_for(&self, now: i64, allowed: Option<&HashSet<String>>) -> Vec<String> {
        self.entries
            .values()
            .filter(|entry| match allowed {
                Some(only) => only.contains(&entry.uid),
                None => true,
            })
            .map(|entry| {
                let tail = entry
                    .uid
                    .get(entry.uid.len().saturating_sub(8)..)
                    .unwrap_or("");
                let cooling_models: Vec<&str> = entry
                    .model_cooldowns
                    .iter()
                    .filter(|(_, until)| **until > now)
                    .map(|(model, _)| model.as_str())
                    .collect();
                let state = if entry.needs_intervention() {
                    entry.unusable_reason(now)
                } else if !cooling_models.is_empty() {
                    format!("可用（模型冷却：{}）", cooling_models.join("/"))
                } else {
                    "可用".to_string()
                };
                format!("{}({tail}:{state})", entry.name)
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(uid: &str, token: &str) -> QoderPoolEntry {
        QoderPoolEntry {
            uid: uid.into(),
            name: uid.into(),
            access_token: token.into(),
            refresh_token: String::new(),
            auth_dead: false,
            cooldown_until: 0,
            cooldown_reason: None,
            model_cooldowns: HashMap::new(),
            last_error: None,
            inflight: Arc::new(AtomicUsize::new(0)),
            success_ema: 0.0,
            error_ema: 0.0,
            last_used_ms: 0,
            used_seq: 0,
        }
    }

    /// 模型级冷却只冷该模型；账号级冷却冷全部；无凭据不入候选。
    ///
    /// 变异：把 `RateLimited` 的冷却从 `model_cooldowns` 改成 `cooldown_until`
    /// ⇒ 第二条断言红（一次 429 就把账号对**所有**模型打死 —— 参考实现明确避免的形态）。
    #[test]
    fn cooldowns_are_scoped_correctly() {
        let now = now_ms();
        // 无 access ⇒ 不可用（PAT 不能直接推理）。
        let no_access = entry("q0", "");
        assert!(!no_access.healthy_for_request(now, "m"));

        // 模型冷却只影响该模型。
        let mut e = entry("q1", "dt-x");
        e.model_cooldowns.insert("m-a".into(), now + 60_000);
        assert!(!e.healthy_for_request(now, "m-a"));
        assert!(e.healthy_for_request(now, "m-b"), "不该串冷其它模型");

        // 账号级冷却冷全部。
        let mut e2 = entry("q2", "dt-y");
        e2.cooldown_until = now + 60_000;
        assert!(!e2.healthy_for_request(now, "any"));

        // auth_dead 直接不可用，刷新（换 token 重建）后才复活。
        let mut e3 = entry("q3", "dt-z");
        e3.auth_dead = true;
        assert!(!e3.healthy_for_request(now, "m"));
    }

    #[test]
    fn pool_pick_skips_tried_and_respects_preference() {
        let mut pool = QoderPool::new();
        pool.entries.insert("a".into(), entry("a", "t-a"));
        pool.entries.insert("b".into(), entry("b", "t-b"));
        let tried = HashSet::new();
        // 偏好命中。
        assert_eq!(
            pool.pick("m", &tried, Some("b"), None),
            Some("b".to_string())
        );
        // 偏好被 tried 排除 ⇒ 回落自动择优。
        let mut tried = HashSet::new();
        tried.insert("b".to_string());
        assert_eq!(
            pool.pick("m", &tried, Some("b"), None),
            Some("a".to_string())
        );
        // 全被排除 ⇒ None。
        tried.insert("a".to_string());
        assert_eq!(pool.pick("m", &tried, None, None), None);
    }

    #[test]
    fn apply_error_sets_expected_state() {
        let mut pool = QoderPool::new();
        pool.entries.insert("a".into(), entry("a", "t-a"));
        // 429 ⇒ 只冷该模型。
        pool.apply_error("a", QoderErrKind::RateLimited, "boom", "m-a");
        assert!(pool.entries["a"].model_cooldowns.contains_key("m-a"));
        assert_eq!(pool.entries["a"].cooldown_until, 0);
        // 401 ⇒ auth_dead + 长冷却；成功收尾复位。
        pool.apply_error("a", QoderErrKind::AuthDead, "dead", "m-a");
        assert!(pool.entries["a"].auth_dead);
        pool.note_success("a");
        assert!(!pool.entries["a"].auth_dead);
        assert_eq!(pool.entries["a"].cooldown_until, 0);
    }

    /// ★ 探测回填：只置/清 `model_cooldowns`，**绝不推 EMA / 错误计数**（与 WB 逐条对齐）。
    ///
    /// 变异①（落点 = `adopt_probe_result`）：把它改成调 `apply_error` ⇒ 「EMA 不变」断言红
    /// —— 那正是「探测次数混进退避指数、冷却越探越长」的缺陷形态。
    /// 变异②：把下限去掉（直接落 `now` 或 `now + 1`）⇒ 「下限 = 60s」断言红。
    #[test]
    fn adopt_probe_result_only_touches_model_cooldowns() {
        let mut pool = QoderPool::new();
        pool.entries.insert("a".into(), entry("a", "t-a"));
        // 先经真实错误压上 EMA，作为「不被探测改动」的基线。
        pool.apply_error("a", QoderErrKind::RateLimited, "boom", "m-other");
        let success_ema = pool.entries["a"].success_ema;
        let error_ema = pool.entries["a"].error_ema;
        let last_error = pool.entries["a"].last_error.clone();
        assert!(error_ema > 0.0, "前置：应有非零错误 EMA");

        let now = now_ms();
        // ① 置冷却：无墙钟 ⇒ 用下限 60s（而不是 now + 1ms）。
        let changed = pool.adopt_probe_result(
            "a",
            "m-a",
            &ProbeOutcome::ModelRateLimited {
                reset_at_ms: None,
                upstream_reset_at_ms: None,
            },
            now,
        );
        assert!(changed);
        assert_eq!(
            pool.entries["a"].model_cooldowns["m-a"],
            now + MIN_PROBE_COOLDOWN_MS,
            "无墙钟必须用冷却下限补齐（否则只冷 1ms = 等于没冷却）"
        );

        // ★ 不推 EMA / 错误计数 / last_error（变异：改成调 `apply_error` 即红）。
        assert_eq!(pool.entries["a"].success_ema, success_ema);
        assert_eq!(pool.entries["a"].error_ema, error_ema);
        assert_eq!(pool.entries["a"].last_error, last_error);

        // ③ 幂等：同结论重复采纳不重复计数（既有冷却更晚 ⇒ 不缩短）。
        assert!(!pool.adopt_probe_result(
            "a",
            "m-a",
            &ProbeOutcome::ModelRateLimited {
                reset_at_ms: None,
                upstream_reset_at_ms: None
            },
            now,
        ));

        // ④ 其余档不动冷却。
        assert!(!pool.adopt_probe_result("a", "m-a", &ProbeOutcome::Other, now));
        assert!(!pool.adopt_probe_result(
            "a",
            "m-a",
            &ProbeOutcome::AccountRateLimited { reset_at_ms: None },
            now,
        ));
        assert!(!pool.adopt_probe_result("a", "m-a", &ProbeOutcome::ModelBlocked, now));

        // ⑤ Available ⇒ 清掉该模型；已清 ⇒ false。
        assert!(pool.adopt_probe_result("a", "m-a", &ProbeOutcome::Available, now));
        assert!(!pool.entries["a"].model_cooldowns.contains_key("m-a"));
        assert!(
            !pool.adopt_probe_result("a", "m-a", &ProbeOutcome::Available, now),
            "已清 ⇒ false（幂等）"
        );

        // ⑥ 不在池里 / 空模型名 ⇒ false。
        assert!(!pool.adopt_probe_result("ghost", "m-a", &ProbeOutcome::Available, now));
        assert!(!pool.adopt_probe_result("a", "", &ProbeOutcome::Available, now));
    }

    /// 管理面视图的**两条不变量**：
    /// ① 三格互斥且穷尽（`available + cooling + disabled == total`）；
    /// ② 「需干预」的两支（无凭据 / 凭据死）都进 `disabled`，且**模型级冷却不占这三格**
    ///    —— 它另有 `model_cooling` 一格（变异：把 `needs_intervention` 换成只判 `auth_dead`
    ///    ⇒ 第一条断言红：只有 PAT 的账号会被错报成「可用」）。
    #[test]
    fn admin_views_are_exhaustive_and_flag_intervention() {
        let now = now_ms();
        let mut pool = QoderPool::new();
        pool.entries.insert("ok".into(), entry("ok", "t-ok"));
        pool.entries.insert("pat".into(), entry("pat", ""));
        let mut dead = entry("dead", "t-dead");
        dead.auth_dead = true;
        dead.cooldown_reason = Some("凭据失效（需刷新令牌）".into());
        pool.entries.insert("dead".into(), dead);
        let mut cold = entry("cold", "t-cold");
        cold.cooldown_until = now + 60_000;
        pool.entries.insert("cold".into(), cold);
        let mut model_cold = entry("mc", "t-mc");
        model_cold.model_cooldowns.insert("m".into(), now + 60_000);
        pool.entries.insert("mc".into(), model_cold);

        let summary = pool.summary(now);
        assert_eq!(summary["total"], 5);
        assert_eq!(summary["disabled"], 2, "PAT 无凭据 + 凭据死 都应计入需干预");
        assert_eq!(summary["cooling"], 1);
        assert_eq!(summary["available"], 2);
        assert_eq!(
            summary["available"].as_u64().unwrap()
                + summary["cooling"].as_u64().unwrap()
                + summary["disabled"].as_u64().unwrap(),
            5,
            "三格必须互斥且穷尽"
        );
        assert_eq!(summary["model_cooling"], 1, "模型级冷却单独一格");
        assert_eq!(summary["expired"], 0);
        assert_eq!(summary["zero_credits"], 0);

        let accounts = pool.accounts_view(now);
        let by_uid = |uid: &str| {
            accounts
                .iter()
                .find(|row| row["uid"] == uid)
                .unwrap_or_else(|| panic!("缺 {uid}"))
                .clone()
        };
        assert_eq!(by_uid("ok")["status"], "available");
        assert_eq!(by_uid("ok")["disabled"], false);
        assert!(
            by_uid("ok")["credits"].is_null(),
            "池不建模积分 ⇒ null，不能写 0"
        );
        assert_eq!(by_uid("pat")["status"], "disabled");
        assert_eq!(by_uid("dead")["status"], "disabled");
        assert_eq!(by_uid("cold")["status"], "cooling");
        assert_eq!(
            by_uid("mc")["status"],
            "available",
            "只冷模型 ⇒ 账号本身可用"
        );
        // 可用行**不带**冷却原因（过期原因挂在「可用」旁边会被读成当前限流）。
        assert!(by_uid("ok")["cooldownReason"].is_null());
        assert!(by_uid("cold")["cooldownReason"].is_string());

        // 诊断行：需干预的账号给出可读原因，不是只报「不可用」。
        let diagnose = pool.diagnose(now);
        assert_eq!(diagnose.len(), 5);
        assert!(diagnose.iter().any(|line| line.contains("PAT")));
        assert!(diagnose.iter().any(|line| line.contains("模型冷却")));

        // ★ issue #76：名单口径只列这把 Key 允许用的账号 —— 全池数字对白名单用户是噪声，
        //   还会把他推去修一堆本来就不许用的账号。
        let only: std::collections::HashSet<String> = ["pat".to_string()].into_iter().collect();
        let scoped = pool.diagnose_for(now, Some(&only));
        assert_eq!(scoped.len(), 1, "名单外账号不得出现在诊断里: {scoped:?}");
        assert!(scoped[0].contains("PAT"), "{scoped:?}");
        let missing: std::collections::HashSet<String> = ["zzz".to_string()].into_iter().collect();
        assert!(
            pool.diagnose_for(now, Some(&missing)).is_empty(),
            "名单没覆盖本池 ⇒ 空诊断（上层据此说「名单没覆盖本区域」）"
        );
    }
    /// ★ 排队信号（`10605` / `isQueued`）藏在 401/403 里 ⇒ **不惩罚账号**。
    ///
    /// 可证伪：删掉 `classify_http_with_detail` 的排队分支（退回只看状态码）⇒
    /// 第一个断言拿到的就是 `AuthDead`，账号会被标死 + 冷却 30 分钟。
    #[test]
    fn queued_envelope_is_not_an_account_failure() {
        assert_eq!(
            classify_http_with_detail(403, "{\"code\":10605,\"msg\":\"isQueued\"}"),
            QoderErrKind::Queued
        );
        assert_eq!(
            classify_http_with_detail(401, "isQueued: true"),
            QoderErrKind::Queued
        );
        // 真·凭据失效不能被当成排队吞掉（否则账号永远不刷新）。
        assert_eq!(
            classify_http_with_detail(401, "{\"code\":\"TOKEN_INVALID\"}"),
            QoderErrKind::AuthDead
        );
        assert_eq!(
            classify_http_with_detail(401, "token expired"),
            QoderErrKind::AuthDead
        );
        // 其它状态码不受影响。
        assert_eq!(
            classify_http_with_detail(429, "10605"),
            QoderErrKind::RateLimited
        );
        assert_eq!(
            classify_http_with_detail(400, "isQueued"),
            QoderErrKind::ClientError
        );
        assert_eq!(
            classify_http_with_detail(500, "10605"),
            QoderErrKind::Transient
        );
        // 排队要换号（另一个号可能没排队），但**不是**客户端错。
        assert!(QoderErrKind::Queued.should_rotate());
        assert_eq!(QoderErrKind::Queued.as_str(), "queued");
    }

    /// 最小间隔解析：只认正整数；`0` / 负数 / 乱码 / 缺省一律**关**（0）。
    ///
    /// 可证伪：把 `.filter(|value| *value > 0)` 删掉 ⇒ 负数会被当成间隔传进选号内核，
    /// 每个号都被剔除（然后靠 LRU 兜底），行为变成「看起来没生效」。
    #[test]
    fn min_pick_gap_parsing_is_conservative() {
        assert_eq!(parse_min_pick_gap(None), 0);
        assert_eq!(parse_min_pick_gap(Some("")), 0);
        assert_eq!(parse_min_pick_gap(Some("250")), 250);
        assert_eq!(parse_min_pick_gap(Some(" 1000 ")), 1000);
        assert_eq!(parse_min_pick_gap(Some("0")), 0, "0 = 关");
        assert_eq!(parse_min_pick_gap(Some("-5")), 0, "负数不得当成间隔");
        assert_eq!(parse_min_pick_gap(Some("abc")), 0, "乱码 = 关，不是崩");
    }

    /// ★ 402 = **余额耗尽**：不是「客户端参数错」，必须换号 + 停放（不是短冷却）。
    ///
    /// 可证伪：把 `classify_http` 的 `402 => BalanceExhausted` 删掉 ⇒ 402 落进
    /// `ClientError`（`should_rotate() == false`）⇒ 每条请求都去撞同一个没钱的账号。
    #[test]
    fn classify_http_402_is_balance_exhausted() {
        let kind = classify_http(402);
        assert_eq!(kind, QoderErrKind::BalanceExhausted);
        assert!(kind.should_rotate(), "余额耗尽要换号（别的号可能还有额度）");
        assert_eq!(kind.as_str(), "balance_exhausted");
        // 其余状态码不受影响。
        assert_eq!(classify_http(400), QoderErrKind::ClientError);
        assert_eq!(classify_http(401), QoderErrKind::AuthDead);
        assert_eq!(classify_http(429), QoderErrKind::RateLimited);
    }

    /// `apply_error(Queued)` 不改 `auth_dead` / 冷却 / EMA —— 只留痕。
    #[test]
    fn queued_does_not_penalise_the_entry() {
        let mut pool = QoderPool::new();
        pool.entries.insert("a".into(), entry("a", "t-a"));
        let before = pool.entries["a"].clone();
        pool.apply_error("a", QoderErrKind::Queued, "isQueued", "m-a");
        let after = &pool.entries["a"];
        assert!(!after.auth_dead, "排队不是凭据死");
        assert_eq!(after.cooldown_until, before.cooldown_until, "不得设冷却");
        assert_eq!(after.success_ema, before.success_ema, "不得推 EMA");
        assert_eq!(after.error_ema, before.error_ema, "不得推 EMA");
        assert!(
            after
                .last_error
                .as_deref()
                .unwrap_or("")
                .starts_with("queued"),
            "必须留痕: {:?}",
            after.last_error
        );
    }
    /// 在途闸门：默认关（limit=0 恒成功）；开启后按账号限流；**Drop 即归还**。
    ///
    /// 可证伪：把 `InflightGuard::drop` 的自减删掉 ⇒ 「释放后可再抢」永远失败。
    #[test]
    fn inflight_gate_limits_account_and_releases_on_drop() {
        let mut pool = QoderPool::new();
        pool.entries.insert("a".into(), entry("a", "t-a"));
        let counter = pool.entries["a"].inflight.clone();

        // limit = 0 ⇒ 不设闸门（但仍计数）。
        let guard = pool.acquire("a", 0).expect("关闭闸门必须恒成功");
        assert_eq!(counter.load(Ordering::SeqCst), 1);
        drop(guard);
        assert_eq!(counter.load(Ordering::SeqCst), 0, "Drop 必须归还名额");

        // limit = 1 ⇒ 第二个并发必须被拒。
        let guard = pool.acquire("a", 1).expect("空位应能抢");
        assert!(
            pool.acquire("a", 1).is_none(),
            "满员必须返回 None（调用方据此跳过该号，而不是排队）"
        );
        drop(guard);
        assert!(
            pool.acquire("a", 1).is_some(),
            "释放后必须能再抢（否则一次异常就永久占位）"
        );

        // 未知账号 / 上限为 0 的缺省语义。
        assert!(pool.acquire("missing", 1).is_none());
        assert_eq!(max_inflight(), 0, "缺省必须关闭（不能默认给个人账号降速）");
    }
}
