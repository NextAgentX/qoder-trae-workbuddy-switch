//! 积分回填：把上游**账单**里的积分写回 Trae 网关的请求日志。
//!
//! ## 为什么需要它
//!
//! Trae 的 SSE 只回报 token（`event:token_usage`），**没有任何积分字段**，而网关的
//! 请求日志要显示「这次请求花了多少积分」。上游账单接口有这张表，但它：
//!
//! - **不是实时**：实测出账比请求结束晚 **6–8 秒**；
//! - **不能按会话对齐**：上游的 `session_id` 与网关发出去的三个派生 id 都不相同；
//! - **是分页的**：一页最多 50 行，最新在前。
//!
//! 所以积分只能**事后回填**，并且必须满足三条硬约束（见下）。
//!
//! ## 硬约束 1：上游调用次数要尽量低
//!
//! | 机制 | 取值 | 效果 |
//! |:--|:--|:--|
//! | 循环 tick | 60s | 只扫内存日志（≤`log_keep` 条），**没有 pending 就一次请求都不发** |
//! | 最小等待 | pending 最老一条 ≥ 20s | 出账前拉是白拉 |
//! | 单账号冷却 | 自动 60s / 手动 120s | 自动与 tick 同频；手动防连点 |
//! | 分页上限 | ≤3 页 × 50 行 | 一次拉取**统一回填**，不按会话拉 |
//! | 放弃阈值 | 15 分钟 / 4 次 | 不再为一条永远匹配不上的记录打上游 |
//! | 退避 | 429 等失败 → 5 / 15 / 60 分钟 | 上游限频时自我收敛 |
//!
//! **「本地已经有积分就不再拉」落在两处**：① pending 集合本身就排除了已回填的条目；
//! ② 查询窗口由**未回填条目的最早时间**决定 —— 没有 pending 就没有窗口，也就没有请求。
//!
//! ## 硬约束 2：批量回填，不按会话
//!
//! 一轮里：日志 → 按 `(variant, uid)` 分组 → 每组**一次**分页查询 → 全部条目
//! 一次匹配 → **一次**加锁、**一次**落盘（见 [`RequestLog::apply_credits`]）。
//! 会话维度在这里完全不存在（上游不给我们可用的会话号）。
//!
//! ## 硬约束 3：绝不在请求路径上拉账单
//!
//! 与 [`crate::credits_refresh`] 同一纪律：只由后台周期循环触发。请求路径上多一次
//! 上游往返会直接抬高每次对话的延迟，而积分是**展示数据**，晚一两分钟完全可接受。
//!
//! ## 匹配规则（为什么不是等值比较）
//!
//! 唯一能在两端共用的身份是 `(input_token, output_token)`；模型名两边**不同形**
//! （网关 `deepseek-v4.1-flash` vs 账单 `DeepSeek-V4.1-Flash`），只能作为同距离时的
//! tiebreak。完整判据与边界见 `buddy_switch_core::modules::trae::usage_detail::match_rows`。
//!
//! ## 状态文件
//!
//! `~/.buddy-switch/trae/credit_backfill.json`：已消费的账单行键（去重环）+ 每账号
//! 冷却 / 退避 + 每条 pending 的尝试次数。**可丢可重建**（丢了最坏是多消费一行账单）。

use std::collections::{BTreeMap, HashSet};
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use buddy_switch_core::modules::config as core_config;
use buddy_switch_core::modules::trae::account;
use buddy_switch_core::modules::trae::credits;
use buddy_switch_core::modules::trae::paths;
use buddy_switch_core::modules::trae::store;
use buddy_switch_core::modules::trae::usage_detail::{self, PendingEntry, UsageRow};
use buddy_switch_core::modules::trae::variant::TraeVariant;

use crate::logging::{CreditFill, RequestLog};
use crate::trae::TraeGatewayState;

/// 后台循环的 tick 间隔。
///
/// 只是「扫一遍内存日志」的代价（≤200 条），**不等于**上游请求频率：
/// 没有 pending 时一轮上游调用都没有。
pub const TICK_INTERVAL_MS: u64 = 60_000;

/// 单账号两次上游查询之间的最小间隔（**手动刷新**口径）。
///
/// 后台 tick 用 [`TICK_INTERVAL_MS`]（60 秒）；手动刷新必须更保守 —— 用户可能连点，
/// 而每次点击都真打一次上游。两种入口共用 `last_request_ms`：自动刚查过时手动也会被
/// 挡住，这正是「2 分钟只能拉一次」的意思。
pub const PER_ACCOUNT_COOLDOWN_MS: i64 = 120_000;

/// 触发来源：决定同一账号的最小查询间隔。
///
/// ## 为什么必须区分
///
/// 两个入口的频率目标不同（自动 60 秒 / 手动 2 分钟），但共用同一份
/// `last_request_ms`。若只用一个阈值，要么自动被压到 2 分钟（违背「60 秒一次」），
/// 要么手动可以每 60 秒打一次（违背「频繁刷新要限流」）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trigger {
    /// 后台 tick（60 秒一轮）。间隔与 tick 同值 ⇒ 正常情况下每轮都能查。
    Auto,
    /// 用户点「刷新」（或 server 的 `/credits/refresh`）：2 分钟一次。
    Manual,
}

impl Trigger {
    fn min_interval_ms(self) -> i64 {
        match self {
            Trigger::Auto => TICK_INTERVAL_MS as i64,
            Trigger::Manual => PER_ACCOUNT_COOLDOWN_MS,
        }
    }
}

/// pending 条目至少「静置」这么久才去查账单（实测出账 6–8 秒）。
pub const MIN_SETTLE_AGE_MS: i64 = 20_000;

/// 超过这个年龄仍未匹配 ⇒ 放弃（标 `unmatched`）。
pub const MAX_BACKFILL_AGE_MS: i64 = 15 * 60_000;

/// 同一条 pending 最多尝试几次（跨进程累计，存在状态文件里）。
pub const MAX_ATTEMPTS: u32 = 4;

/// 查询窗口的最大回溯（防止日志里一条陈年 pending 把窗口拉到几天前）。
pub const MAX_WINDOW_SECS: i64 = 2 * 60 * 60;

/// 窗口向前多留的余量（覆盖「账单行比最后一条日志稍早」的情况）。
pub const WINDOW_OVERLAP_SECS: i64 = 60;

/// 单次查询最多翻几页（一页 50 行）。
///
/// 3 页 = 150 行，已远超「一个账号在冷却窗口内可能产生的请求数」
/// （120 秒冷却 × 每次最多 50 行/页）。翻更多页只会在待回填条目异常堆积时
/// 拉长一次上游请求的耗时，性价比很低。
pub const MAX_PAGES: u32 = 3;

/// 失败退避阶梯（毫秒）：第一次失败 5 分钟，之后 15 / 60 分钟。
const BACKOFF_STEPS_MS: [i64; 3] = [5 * 60_000, 15 * 60_000, 60 * 60_000];

/// 去重环保留的行键数量（远超 `log_keep` 的 200，够覆盖重叠窗口）。
const SEEN_RING_CAP: usize = 400;

// ---------------------------------------------------------------------------
// 取数缝（测试用它注入假账单，绝不发真实网络）
// ---------------------------------------------------------------------------

/// 取数失败的两类：账号不在了（跳过、不惩罚）与上游出错（退避）。
#[derive(Debug, Clone, PartialEq)]
pub enum FetchError {
    /// 账号库里没有这个 uid（用户删了账号 / 换了账号库）。
    AccountMissing,
    /// 网络或上游业务错误。
    Upstream(String),
}

/// 取账单的可注入缝（与 `credits_refresh::CreditFetcher` 同一套思路）。
pub trait UsageFetcher: Send + Sync {
    fn fetch<'a>(
        &'a self,
        variant: TraeVariant,
        uid: &'a str,
        start_time: i64,
        end_time: i64,
        max_pages: u32,
    ) -> Pin<Box<dyn Future<Output = Result<(Vec<UsageRow>, u64), FetchError>> + Send + 'a>>;
}

/// 生产取数器：解析账号 → 取该账号的设备标识 → 打上游账单接口。
pub struct CoreUsageFetcher;

impl UsageFetcher for CoreUsageFetcher {
    fn fetch<'a>(
        &'a self,
        variant: TraeVariant,
        uid: &'a str,
        start_time: i64,
        end_time: i64,
        max_pages: u32,
    ) -> Pin<Box<dyn Future<Output = Result<(Vec<UsageRow>, u64), FetchError>> + Send + 'a>> {
        Box::pin(async move {
            let account = account::find_for(variant, uid).ok_or(FetchError::AccountMissing)?;
            let device =
                credits::device_for_jwt_for(variant, &account.jwt).map_err(FetchError::Upstream)?;
            usage_detail::fetch_window(
                variant,
                &account.jwt,
                &device,
                start_time,
                end_time,
                max_pages,
            )
            .await
            .map_err(FetchError::Upstream)
        })
    }
}

// ---------------------------------------------------------------------------
// 状态
// ---------------------------------------------------------------------------

fn state_version() -> u32 {
    1
}

/// 单个账号的回填状态。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AccountState {
    /// 已消费的账单行键（**只追加、封顶 [`SEEN_RING_CAP`]**）。
    ///
    /// 为什么需要它：查询窗口按 pending 回收会重叠，同一行账单可能被反复拉到。
    /// 没有这个环，一条已回填的记录被裁掉后，同 token 的另一条 pending 会**再吃一次**
    /// 同一行账单（积分错位）。
    pub seen: Vec<String>,
    /// 最近一次**发起**上游查询的时间（毫秒）。
    pub last_request_ms: i64,
    /// 退避截止时间（毫秒，`0` = 无退避）。
    pub backoff_until_ms: i64,
    /// 连续失败次数（成功后清零）。
    pub fails: u32,
}

/// 回填器状态（落盘）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct BackfillState {
    pub version: u32,
    /// 键 = `<variant>|<uid>`。
    pub accounts: BTreeMap<String, AccountState>,
    /// 每条 pending 的累计尝试次数（键 = 日志条目 `id`）。
    pub attempts: BTreeMap<String, u32>,
}

impl Default for BackfillState {
    fn default() -> Self {
        Self {
            version: state_version(),
            accounts: BTreeMap::new(),
            attempts: BTreeMap::new(),
        }
    }
}

/// 状态 + 落盘路径 + dirty 标记。
pub struct BackfillStore {
    state: BackfillState,
    path: Option<PathBuf>,
    dirty: bool,
}

impl BackfillStore {
    /// 从磁盘读；文件不存在 / 损坏 ⇒ 空状态（**不是错误**：这只影响去重精度）。
    pub fn load(path: PathBuf) -> Self {
        let state = std::fs::read_to_string(&path)
            .ok()
            .and_then(|text| serde_json::from_str::<BackfillState>(&text).ok())
            .unwrap_or_default();
        Self {
            state,
            path: Some(path),
            dirty: false,
        }
    }

    /// 内存态（单测用，绝不碰磁盘）。
    pub fn in_memory() -> Self {
        Self {
            state: BackfillState::default(),
            path: None,
            dirty: false,
        }
    }

    /// 只有状态真的变过才写盘。
    pub fn save_if_dirty(&mut self) {
        if !self.dirty {
            return;
        }
        let Some(path) = self.path.as_ref() else {
            self.dirty = false;
            return;
        };
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(text) = serde_json::to_string_pretty(&self.state) {
            let _ = core_config::atomic_write(path, &text);
        }
        self.dirty = false;
    }

    pub fn account(&self, key: &str) -> Option<&AccountState> {
        self.state.accounts.get(key)
    }

    fn account_mut(&mut self, key: &str) -> &mut AccountState {
        self.state.accounts.entry(key.to_string()).or_default()
    }

    /// 该账号现在可以发请求吗？（冷却 + 退避）
    ///
    /// `min_interval_ms` 由触发来源决定（见 [`Trigger`]）：自动 60 秒、手动 120 秒。
    pub fn can_request(&self, key: &str, now_ms: i64, min_interval_ms: i64) -> bool {
        let Some(state) = self.account(key) else {
            return true;
        };
        if now_ms < state.backoff_until_ms {
            return false;
        }
        now_ms.saturating_sub(state.last_request_ms) >= min_interval_ms
    }

    /// 已消费行键集合。
    pub fn seen_set(&self, key: &str) -> HashSet<String> {
        self.account(key)
            .map(|state| state.seen.iter().cloned().collect())
            .unwrap_or_default()
    }

    fn mark_request(&mut self, key: &str, now_ms: i64) {
        let state = self.account_mut(key);
        state.last_request_ms = now_ms;
        self.dirty = true;
    }

    fn mark_matched(&mut self, key: &str, row_key: &str) {
        let state = self.account_mut(key);
        state.seen.push(row_key.to_string());
        if state.seen.len() > SEEN_RING_CAP {
            let drop_count = state.seen.len() - SEEN_RING_CAP;
            state.seen.drain(..drop_count);
        }
        self.dirty = true;
    }

    fn mark_success(&mut self, key: &str) {
        let state = self.account_mut(key);
        if state.fails != 0 || state.backoff_until_ms != 0 {
            state.fails = 0;
            state.backoff_until_ms = 0;
            self.dirty = true;
        }
    }

    fn mark_error(&mut self, key: &str, now_ms: i64) {
        let state = self.account_mut(key);
        state.fails = state.fails.saturating_add(1);
        let step = BACKOFF_STEPS_MS
            [(state.fails.saturating_sub(1) as usize).min(BACKOFF_STEPS_MS.len() - 1)];
        state.backoff_until_ms = now_ms.saturating_add(step);
        self.dirty = true;
    }

    fn attempts_of(&self, id: &str) -> u32 {
        self.state.attempts.get(id).copied().unwrap_or(0)
    }

    fn bump_attempt(&mut self, id: &str) {
        *self.state.attempts.entry(id.to_string()).or_insert(0) += 1;
        self.dirty = true;
    }

    /// 丢掉已经不在 pending 集合里的尝试计数（防止状态文件无界增长）。
    fn prune_attempts(&mut self, live: &HashSet<String>) {
        let before = self.state.attempts.len();
        self.state.attempts.retain(|id, _| live.contains(id));
        if self.state.attempts.len() != before {
            self.dirty = true;
        }
    }
}

// ---------------------------------------------------------------------------
// 纯函数：待回填解析 / 分组 / 窗口
// ---------------------------------------------------------------------------

/// 一条待回填的日志（已解析出账号与变体）。
#[derive(Debug, Clone, PartialEq)]
pub struct Pending {
    pub id: String,
    pub ts_ms: i64,
    pub model: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub variant: TraeVariant,
    pub uid: String,
}

impl Pending {
    fn to_core(&self) -> PendingEntry {
        PendingEntry {
            id: self.id.clone(),
            ts_ms: self.ts_ms,
            model: self.model.clone(),
            input_tokens: self.input_tokens,
            output_tokens: self.output_tokens,
        }
    }
}

/// 从日志条目解析 pending；不是 pending / 字段不全一律 `None`。
///
/// **只认显式 `creditState == "pending"`**：升级前写入的老条目没有这个键，
/// 它们的账单早已过期，按 token 反猜只会写进错误数字。
pub fn parse_pending(value: &Value) -> Option<Pending> {
    if value.get("creditState").and_then(Value::as_str) != Some("pending") {
        return None;
    }
    let id = value.get("id").and_then(Value::as_str)?.to_string();
    let ts_ms = value.get("ts").and_then(Value::as_i64)?;
    let uid = value
        .get("accountUid")
        .and_then(Value::as_str)
        .filter(|text| !text.trim().is_empty())
        .or_else(|| value.get("account").and_then(Value::as_str))
        .map(str::trim)
        .filter(|text| !text.is_empty())?
        .to_string();
    let variant = value
        .get("variant")
        .and_then(Value::as_str)
        .and_then(TraeVariant::parse)?;
    let number = |key: &str| -> u64 { value.get(key).and_then(Value::as_u64).unwrap_or(0) };
    let input_tokens = number("promptTokens");
    let output_tokens = number("completionTokens");
    if input_tokens == 0 && output_tokens == 0 {
        // 没有 token 就没有可对齐的身份 —— 上游账单不可能被可靠匹配。
        return None;
    }
    Some(Pending {
        id,
        ts_ms,
        model: value
            .get("model")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        input_tokens,
        output_tokens,
        variant,
        uid,
    })
}

/// 分组键：`<variant>|<uid>`（两条产品线的同名 uid 不合并）。
pub fn group_key(variant: TraeVariant, uid: &str) -> String {
    format!("{}|{}", variant.as_str(), uid)
}

/// 查询窗口（unix 秒）：`[最老 pending - 余量, now]`，且不早于 `now - 2h`。
pub fn window_for(oldest_ts_ms: i64, now_ms: i64) -> (i64, i64) {
    let end = now_ms.div_euclid(1000);
    let floor = end - MAX_WINDOW_SECS;
    let start = (oldest_ts_ms.div_euclid(1000) - WINDOW_OVERLAP_SECS).max(floor);
    (start, end)
}

/// 这一组需要翻几页：按待回填条数估，封顶 [`MAX_PAGES`]。
///
/// `+1` 是**余量**：窗口里还有已回填 / 不匹配的行（它们也占页），只按 pending 数
/// 算页数会在「同一时间段有别的会话流量」时少拿一页。多拿一页的代价是空请求，
/// 少拿一页的代价是**积分永远补不上**，宁可多一页。
pub fn pages_for(pending: usize) -> u32 {
    let needed = (pending as u64) / usage_detail::MAX_PAGE_SIZE + 1;
    needed.min(MAX_PAGES as u64) as u32
}

// ---------------------------------------------------------------------------
// 一轮
// ---------------------------------------------------------------------------

/// 一轮的结果（诊断 / 测试断言用；不对外暴露）。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Report {
    /// 本轮进入 pending 的条目数（不含已放弃的）。
    pub pending: usize,
    /// 实际发出的上游查询次数。
    pub requests: usize,
    /// 成功回填的条数。
    pub matched: usize,
    /// 判为放弃（超龄 / 尝试用尽）的条数。
    pub given_up: usize,
    /// 因「太新」跳过查询的组数。
    pub too_fresh: usize,
    /// 因冷却 / 退避跳过查询的组数。
    pub cooled: usize,
    /// 账号库里已找不到的组数。
    pub account_missing: usize,
    /// 上游出错的组数。
    pub upstream_errors: usize,
    /// 真正被写回的条目数（含放弃）。
    pub written: usize,
    /// 本轮上游失败的诊断行（**不在这里落盘**）。
    ///
    /// 由生产入口 [`run_once`] 统一写 `app.log`。放在 `report` 里而不是 `run_with`
    /// 里直接写，是为了让单测（注入假 fetcher）**绝不碰真实日志文件** ——
    /// 否则一次 `cargo test` 就会往用户的 `~/.buddy-switch/trae/logs/app.log`
    /// 里留下假账号 / 假 429。
    pub errors: Vec<String>,
}

/// 进程内的「一轮」互斥锁。
///
/// 后台 tick（60 秒）与用户点「刷新」（`run_once` 直调）是**两条独立入口**：
/// 各自从磁盘读一份 `BackfillStore`，若同时进入 `can_request` 判断，两份状态都认为
/// 「可以发」⇒ 同一账号会被连打两次，120 秒冷却形同虚设。
/// 用一把进程级锁把整轮串起来，代价最多是手动刷新等一次 tick（轮内很快）。
static RUN_GUARD: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// 生产入口：由后台循环每 [`TICK_INTERVAL_MS`] 调用一次；
/// 也被「刷新」命令直接调用（提前跑一轮，语义相同）。
pub async fn run_once(state: &TraeGatewayState, trigger: Trigger) -> Report {
    let _guard = RUN_GUARD.lock().await;
    let log = state.log.clone();
    let mut store = BackfillStore::load(paths::credit_backfill_file());
    let report = run_with(
        log,
        &CoreUsageFetcher,
        &mut store,
        core_config::now_ms(),
        trigger,
    )
    .await;
    /* 诊断行只在生产入口落盘：单测直接调 `run_with`，不会污染真实 app.log。 */
    for line in &report.errors {
        store::append_log(&paths::app_log_file(), line);
    }
    report
}

/// 编排主体（取数缝 + 状态都可注入，单测不发网络、不碰 home）。
///
/// ## 为什么先 `mark_request` 再 `await`
///
/// 同一账号的查询要跨 `await`。若等返回后才记「最近请求时间」，两个并发 tick
/// （例如上一轮卡在上游 120s 超时）会同时通过冷却判断、对同一账号发两次查询。
/// 先占位再请求，最坏是「请求失败但冷却照算」—— 那比打上游两倍要轻。
pub async fn run_with(
    log: Arc<RequestLog>,
    fetcher: &dyn UsageFetcher,
    store: &mut BackfillStore,
    now_ms: i64,
    trigger: Trigger,
) -> Report {
    let mut report = Report::default();
    let mut updates: Vec<(String, CreditFill)> = Vec::new();
    let snapshot = log.list();

    let mut pending: Vec<Pending> = Vec::new();
    for value in &snapshot {
        let is_pending = value.get("creditState").and_then(Value::as_str) == Some("pending");
        if !is_pending {
            continue;
        }
        let Some(entry) = parse_pending(value) else {
            /* ★ 解析不了（未知 variant / 缺 uid / 缺 id）的 pending：超龄后放弃。
            没有这条兜底，它会永远停在 `pending` —— 日志永远显示 `…`，
            账本也只能靠 30 分钟宽限放行。当前的两个变体都能解析，但将来新增
            变体字符串时，缺的正是这条路径。 */
            if let (Some(id), Some(ts)) = (
                value.get("id").and_then(Value::as_str),
                value.get("ts").and_then(Value::as_i64),
            ) {
                if now_ms.saturating_sub(ts) > MAX_BACKFILL_AGE_MS {
                    updates.push((id.to_string(), CreditFill::Unmatched));
                    report.given_up += 1;
                }
            }
            continue;
        };
        let age = now_ms.saturating_sub(entry.ts_ms);
        if age > MAX_BACKFILL_AGE_MS || store.attempts_of(&entry.id) >= MAX_ATTEMPTS {
            updates.push((entry.id.clone(), CreditFill::Unmatched));
            report.given_up += 1;
            continue;
        }
        pending.push(entry);
    }
    report.pending = pending.len();

    if !pending.is_empty() {
        let mut groups: BTreeMap<String, Vec<Pending>> = BTreeMap::new();
        for entry in pending.iter() {
            groups
                .entry(group_key(entry.variant, &entry.uid))
                .or_default()
                .push(entry.clone());
        }

        for (key, entries) in groups {
            let first = &entries[0];
            let oldest = entries
                .iter()
                .map(|entry| entry.ts_ms)
                .min()
                .unwrap_or(first.ts_ms);
            if now_ms.saturating_sub(oldest) < MIN_SETTLE_AGE_MS {
                report.too_fresh += 1;
                continue;
            }
            if !store.can_request(&key, now_ms, trigger.min_interval_ms()) {
                report.cooled += 1;
                continue;
            }

            let (start, end) = window_for(oldest, now_ms);
            let pages = pages_for(entries.len());
            store.mark_request(&key, now_ms);

            match fetcher
                .fetch(first.variant, &first.uid, start, end, pages)
                .await
            {
                Ok((rows, _total)) => {
                    report.requests += 1;
                    store.mark_success(&key);
                    let seen = store.seen_set(&key);
                    let core_entries: Vec<PendingEntry> =
                        entries.iter().map(Pending::to_core).collect();
                    let hits = usage_detail::match_rows(&core_entries, &rows, &seen);
                    let mut matched_ids: HashSet<String> = HashSet::new();
                    for hit in hits {
                        store.mark_matched(&key, &hit.row_key);
                        updates.push((hit.entry_id.clone(), CreditFill::Matched(hit.credits)));
                        matched_ids.insert(hit.entry_id);
                        report.matched += 1;
                    }
                    // 真正查过但没命中的条目：累计尝试次数（命中/放弃的不再需要）。
                    for entry in &entries {
                        if !matched_ids.contains(&entry.id) {
                            store.bump_attempt(&entry.id);
                        }
                    }
                }
                Err(FetchError::AccountMissing) => {
                    report.account_missing += 1;
                }
                Err(FetchError::Upstream(error)) => {
                    report.requests += 1;
                    report.upstream_errors += 1;
                    store.mark_error(&key, now_ms);
                    /* 只收集，落盘交给生产入口 `run_once`（见 `Report::errors`）。 */
                    report.errors.push(format!(
                        "[credit-backfill] {} 取账单失败：{}",
                        key,
                        error.chars().take(200).collect::<String>()
                    ));
                }
            }
        }
    }

    let live: HashSet<String> = pending.iter().map(|entry| entry.id.clone()).collect();
    store.prune_attempts(&live);
    report.written = log.apply_credits(&updates);
    store.save_if_dirty();
    report
}

// ---------------------------------------------------------------------------
// 单测
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logging::RequestLog;
    use serde_json::json;
    use std::sync::Mutex;

    /// 假取数器：记录每次请求参数，按队列返回预设结果。
    struct FakeFetcher {
        calls: Mutex<Vec<(String, i64, i64, u32)>>,
        results: Mutex<Vec<Result<(Vec<UsageRow>, u64), FetchError>>>,
    }

    impl FakeFetcher {
        fn new(results: Vec<Result<(Vec<UsageRow>, u64), FetchError>>) -> Self {
            Self {
                calls: Mutex::new(Vec::new()),
                results: Mutex::new(results),
            }
        }
        fn calls(&self) -> Vec<(String, i64, i64, u32)> {
            self.calls.lock().unwrap().clone()
        }
    }

    impl UsageFetcher for FakeFetcher {
        fn fetch<'a>(
            &'a self,
            _variant: TraeVariant,
            uid: &'a str,
            start_time: i64,
            end_time: i64,
            max_pages: u32,
        ) -> Pin<Box<dyn Future<Output = Result<(Vec<UsageRow>, u64), FetchError>> + Send + 'a>>
        {
            self.calls
                .lock()
                .unwrap()
                .push((uid.to_string(), start_time, end_time, max_pages));
            let next = {
                let mut results = self.results.lock().unwrap();
                if results.is_empty() {
                    Ok((Vec::new(), 0))
                } else {
                    results.remove(0)
                }
            };
            Box::pin(async move { next })
        }
    }

    fn temp_log(tag: &str) -> (Arc<RequestLog>, PathBuf) {
        let dir =
            std::env::temp_dir().join(format!("bs-credit-backfill-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("创建隔离目录");
        let path = dir.join("logs.json");
        let _ = std::fs::remove_file(&path);
        (Arc::new(RequestLog::new(path, 200, false)), dir)
    }

    fn pending_entry(id: &str, ts_ms: i64, input: u64, output: u64) -> Value {
        json!({
            "id": id,
            "ts": ts_ms,
            "endpoint": "/v1/chat/completions",
            "method": "POST",
            "account": "uid-1",
            "accountUid": "uid-1",
            "model": "deepseek-v4.1-flash",
            "status": 200,
            "latencyMs": 100,
            "promptTokens": input,
            "completionTokens": output,
            "cachedTokens": 0,
            "credit": Value::Null,
            "creditState": "pending",
            "stream": true,
            "error": Value::Null,
            "variant": "trae_merged",
            "keyId": Value::Null,
            "keyScope": Value::Null,
        })
    }

    fn row(time: i64, input: u64, output: u64, credits: f64) -> UsageRow {
        UsageRow {
            usage_time: time,
            model_name: "DeepSeek-V4.1-Flash".to_string(),
            credits,
            input_token: input,
            output_token: output,
            cache_read_token: 0,
            cache_write_token: 0,
            row_key: format!("{time}|{input}|{output}|DeepSeek-V4.1-Flash|{credits:.4}"),
        }
    }

    /// 没有 pending ⇒ 一次上游请求都不发（这是「频率尽量低」的底线）。
    #[tokio::test]
    async fn no_pending_means_no_upstream_call() {
        let (log, dir) = temp_log("no-pending");
        log.record(pending_entry("settled", 1_000_000, 10, 2));
        // 先把它标成已结算
        log.apply_credits(&[("settled".to_string(), CreditFill::Matched(0.5))]);

        let fetcher = FakeFetcher::new(vec![]);
        let mut store = BackfillStore::in_memory();
        let report = run_with(
            log.clone(),
            &fetcher,
            &mut store,
            1_000_000 + 60_000,
            Trigger::Auto,
        )
        .await;

        assert_eq!(report.pending, 0);
        assert!(fetcher.calls().is_empty(), "无 pending 不得发请求");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// happy path：一条 pending 被账单行回填，且只发一次请求。
    #[tokio::test]
    async fn matches_and_writes_back_in_one_request() {
        let (log, dir) = temp_log("happy");
        let ts = 1_794_051_874_000;
        log.record(pending_entry("e1", ts, 27639, 199));

        let fetcher = FakeFetcher::new(vec![Ok((vec![row(1_794_051_882, 27639, 199, 0.2792)], 1))]);
        let mut store = BackfillStore::in_memory();
        let report = run_with(
            log.clone(),
            &fetcher,
            &mut store,
            ts + 60_000,
            Trigger::Auto,
        )
        .await;

        assert_eq!(report.requests, 1, "一次请求应覆盖全部 pending");
        assert_eq!(report.matched, 1);
        let entry = log.list().pop().unwrap();
        assert_eq!(entry["creditState"], "matched");
        assert!((entry["credit"].as_f64().unwrap() - 0.2792).abs() < 1e-9);
        assert_eq!(fetcher.calls().len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 太新（<20s）的 pending 不发请求；同一账号在冷却期内也不发。
    #[tokio::test]
    async fn respects_settle_age_and_cooldown() {
        let (log, dir) = temp_log("cooldown");
        let ts = 1_794_051_874_000;
        log.record(pending_entry("e1", ts, 10, 2));

        let fetcher = FakeFetcher::new(vec![Ok((vec![], 0)), Ok((vec![], 0))]);
        let mut store = BackfillStore::in_memory();

        // 太新：age=10s < 20s
        let report = run_with(
            log.clone(),
            &fetcher,
            &mut store,
            ts + 10_000,
            Trigger::Auto,
        )
        .await;
        assert_eq!(report.too_fresh, 1);
        assert!(fetcher.calls().is_empty());

        // 够老：发一次
        let report = run_with(
            log.clone(),
            &fetcher,
            &mut store,
            ts + 30_000,
            Trigger::Auto,
        )
        .await;
        assert_eq!(report.requests, 1);

        // 冷却内（+30s，距上次仅 30s < 120s 手动口径）：不发
        let report = run_with(
            log.clone(),
            &fetcher,
            &mut store,
            ts + 60_000,
            Trigger::Manual,
        )
        .await;
        assert_eq!(report.cooled, 1);
        assert_eq!(fetcher.calls().len(), 1, "冷却期内不得重复打上游");

        // 过了冷却：再发
        run_with(
            log.clone(),
            &fetcher,
            &mut store,
            ts + 200_000,
            Trigger::Auto,
        )
        .await;
        assert_eq!(fetcher.calls().len(), 2);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// ★ 两个入口的间隔不同：自动 60 秒（=tick）、手动 120 秒。
    ///
    /// 用户目标：自动「60 秒一次」，手动「2 分钟只能拉一次」。两者共用
    /// `last_request_ms`，只靠一个阈值必然让其中一个目标落空。
    ///
    /// 可证伪：把 `Trigger::min_interval_ms` 改成常量 ⇒ 两个断言必有一个失败。
    #[tokio::test]
    async fn manual_trigger_is_stricter_than_auto() {
        let ts = 1_794_051_874_000;

        // 自动：t=30s 查一次，t=91s（+61s）可以再查。
        let (log, dir) = temp_log("trigger-auto");
        log.record(pending_entry("e1", ts, 10, 2));
        let fetcher = FakeFetcher::new(vec![Ok((vec![], 0)), Ok((vec![], 0))]);
        let mut store = BackfillStore::in_memory();
        run_with(
            log.clone(),
            &fetcher,
            &mut store,
            ts + 30_000,
            Trigger::Auto,
        )
        .await;
        let report = run_with(
            log.clone(),
            &fetcher,
            &mut store,
            ts + 91_000,
            Trigger::Auto,
        )
        .await;
        assert_eq!(report.requests, 1, "自动入口 60 秒后必须能再查");
        let _ = std::fs::remove_dir_all(&dir);

        // 手动：同样的 61 秒后仍被 120 秒冷却挡住。
        let (log, dir) = temp_log("trigger-manual");
        log.record(pending_entry("e1", ts, 10, 2));
        let fetcher = FakeFetcher::new(vec![Ok((vec![], 0)), Ok((vec![], 0))]);
        let mut store = BackfillStore::in_memory();
        run_with(
            log.clone(),
            &fetcher,
            &mut store,
            ts + 30_000,
            Trigger::Manual,
        )
        .await;
        let report = run_with(
            log.clone(),
            &fetcher,
            &mut store,
            ts + 91_000,
            Trigger::Manual,
        )
        .await;
        assert_eq!(report.cooled, 1, "手动入口 2 分钟内不得重复查询");
        assert_eq!(fetcher.calls().len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 尝试用尽 ⇒ 标 unmatched，并且之后不再查询（即使仍年轻）。
    #[tokio::test]
    async fn gives_up_after_max_attempts() {
        let (log, dir) = temp_log("give-up");
        let ts = 1_794_051_874_000;
        log.record(pending_entry("e1", ts, 10, 2));

        let empty: Vec<Result<(Vec<UsageRow>, u64), FetchError>> = (0..MAX_ATTEMPTS as usize + 2)
            .map(|_| Ok((vec![], 0)))
            .collect();
        let fetcher = FakeFetcher::new(empty);
        let mut store = BackfillStore::in_memory();

        // 每轮推进 130s（越过冷却），连跑 MAX_ATTEMPTS 轮
        for index in 0..MAX_ATTEMPTS {
            let now = ts + 30_000 + index as i64 * 130_000;
            run_with(log.clone(), &fetcher, &mut store, now, Trigger::Auto).await;
        }
        let calls_before = fetcher.calls().len();
        assert_eq!(calls_before, MAX_ATTEMPTS as usize);

        // 下一轮：尝试次数用尽 ⇒ 直接放弃，不再请求
        let now = ts + 30_000 + MAX_ATTEMPTS as i64 * 130_000;
        let report = run_with(log.clone(), &fetcher, &mut store, now, Trigger::Auto).await;
        assert_eq!(report.given_up, 1);
        assert_eq!(report.requests, 0);
        assert_eq!(fetcher.calls().len(), calls_before);
        assert_eq!(log.list()[0]["creditState"], "unmatched");
        assert!(
            log.list()[0]["credit"].is_null(),
            "放弃不得把未知写成 0（0 的语义是「免费」）"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 超龄（>15 分钟）⇒ 不请求、直接 unmatched。
    #[tokio::test]
    async fn old_entries_are_given_up_without_request() {
        let (log, dir) = temp_log("old");
        let ts = 1_794_051_874_000;
        log.record(pending_entry("e1", ts, 10, 2));

        let fetcher = FakeFetcher::new(vec![]);
        let mut store = BackfillStore::in_memory();
        let report = run_with(
            log.clone(),
            &fetcher,
            &mut store,
            ts + MAX_BACKFILL_AGE_MS + 1,
            Trigger::Auto,
        )
        .await;

        assert_eq!(report.given_up, 1);
        assert!(fetcher.calls().is_empty());
        assert_eq!(log.list()[0]["creditState"], "unmatched");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 上游失败 ⇒ 退避；退避期内不再请求；成功后清零。
    #[tokio::test]
    async fn upstream_error_sets_backoff() {
        let (log, dir) = temp_log("backoff");
        let ts = 1_794_051_874_000;
        log.record(pending_entry("e1", ts, 10, 2));

        let fetcher = FakeFetcher::new(vec![
            Err(FetchError::Upstream("HTTP 429".into())),
            Ok((vec![], 0)),
        ]);
        let mut store = BackfillStore::in_memory();

        let report = run_with(
            log.clone(),
            &fetcher,
            &mut store,
            ts + 30_000,
            Trigger::Auto,
        )
        .await;
        assert_eq!(report.upstream_errors, 1);

        // 冷却 + 退避：任何一个都足以拦住下一次
        let report = run_with(
            log.clone(),
            &fetcher,
            &mut store,
            ts + 60_000,
            Trigger::Auto,
        )
        .await;
        assert_eq!(report.requests, 0);

        // 退避 5 分钟后恢复
        let report = run_with(
            log.clone(),
            &fetcher,
            &mut store,
            ts + 30_000 + 5 * 60_000 + 1,
            Trigger::Auto,
        )
        .await;
        assert_eq!(report.requests, 1);
        assert_eq!(store.account("trae_merged|uid-1").unwrap().fails, 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 已消费的行不会在下一轮被另一条同 token 的 pending 再吃一次。
    #[tokio::test]
    async fn seen_ring_prevents_double_consumption() {
        let (log, dir) = temp_log("seen");
        let ts = 1_794_051_874_000;
        log.record(pending_entry("e1", ts, 10, 2));
        log.record(pending_entry("e2", ts + 1_000, 10, 2));

        let rows = vec![row(1_794_051_880, 10, 2, 0.4)];
        let fetcher = FakeFetcher::new(vec![Ok((rows.clone(), 1)), Ok((rows, 1))]);
        let mut store = BackfillStore::in_memory();

        let first = run_with(
            log.clone(),
            &fetcher,
            &mut store,
            ts + 60_000,
            Trigger::Auto,
        )
        .await;
        assert_eq!(first.matched, 1);

        // 同时刻窗口重叠，第二轮只剩 e2；账单里只有同一行 ⇒ 不应再消费
        let second = run_with(
            log.clone(),
            &fetcher,
            &mut store,
            ts + 200_000,
            Trigger::Auto,
        )
        .await;
        assert_eq!(second.matched, 0, "同一行账单不得回填两条日志");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 老条目（没有 creditState）永远不进 pending —— 账单早过期，猜只会写错。
    #[test]
    fn legacy_entries_are_never_pending() {
        let legacy = json!({"ts": 1_000, "model": "m", "promptTokens": 10, "completionTokens": 2});
        assert!(parse_pending(&legacy).is_none());
    }

    /// ★ 解析不了的 pending（未知 variant）超龄后必须放弃，不能永远停在 `…`。
    ///
    /// 缺陷现场：`parse_pending` 返回 `None` ⇒ 既不会进入分组、也不会被放弃，
    /// 日志永久 `pending`（前端一直显示 `…`），账本只能等 30 分钟宽限放行。
    ///
    /// 可证伪：删掉 `run_with` 里解析失败分支的放弃逻辑 ⇒ 第二次断言仍是 `pending`。
    #[tokio::test]
    async fn unparseable_pending_is_given_up_when_stale() {
        let (log, dir) = temp_log("unparseable");
        let ts = 1_794_051_874_000;
        let mut entry = pending_entry("e1", ts, 10, 2);
        entry["variant"] = json!("future_variant");
        log.record(entry);

        let fetcher = FakeFetcher::new(vec![]);
        let mut store = BackfillStore::in_memory();

        // 未超龄：保持 pending，不发请求。
        let report = run_with(
            log.clone(),
            &fetcher,
            &mut store,
            ts + 30_000,
            Trigger::Auto,
        )
        .await;
        assert_eq!(report.given_up, 0);
        assert_eq!(log.list()[0]["creditState"], "pending");

        // 超龄：标 unmatched，仍然不发请求。
        let report = run_with(
            log.clone(),
            &fetcher,
            &mut store,
            ts + MAX_BACKFILL_AGE_MS + 1,
            Trigger::Auto,
        )
        .await;
        assert_eq!(report.given_up, 1, "解析不了的 pending 超龄后必须被放弃");
        assert!(fetcher.calls().is_empty());
        assert_eq!(log.list()[0]["creditState"], "unmatched");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 窗口按最老 pending 回收，且不早于 2 小时。
    #[test]
    fn window_is_anchored_on_oldest_pending() {
        let oldest = 1_794_051_874_000;
        // 5 分钟后：窗口起点 = 最老 pending 往前 60 秒（远在 2 小时地板之内）。
        let now = oldest + 300_000;
        let (start, end) = window_for(oldest, now);
        assert_eq!(end, now.div_euclid(1000));
        assert_eq!(start, 1_794_051_874 - WINDOW_OVERLAP_SECS);

        // 陈年 pending：窗口被 2 小时地板挡住（否则会把几天前的账单全拉一遍）。
        let now = 1_794_060_000_000;
        let (start, end) = window_for(1_790_000_000_000, now);
        assert_eq!(end, 1_794_060_000);
        assert_eq!(start, end - MAX_WINDOW_SECS);
    }

    /// 分页数按条目数估，封顶 [`MAX_PAGES`] 页。
    #[test]
    fn pages_scale_with_pending_but_are_capped() {
        assert_eq!(pages_for(1), 1);
        assert_eq!(pages_for(50), 2);
        assert_eq!(pages_for(99), 2);
        // 100 条起要 3 页，正好等于上限（3 页 = 150 行）
        assert_eq!(pages_for(100), MAX_PAGES);
        assert_eq!(pages_for(10_000), MAX_PAGES);
    }
}
