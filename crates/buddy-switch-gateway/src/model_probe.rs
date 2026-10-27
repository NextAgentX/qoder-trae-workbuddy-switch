//! 账号池 × 模型 **限流探测**（一次性作业）—— **按作用域（`Scope`）参数化**。
//!
//! ## 作用域参数化（2026-10 改造）
//!
//! 作业与命令的入参从 `region: String` 升级为 `scope: String`（`wb-cn` / `qoder-cn` …，
//! 由 core `product::Scope::parse` **fail-closed** 解析）。探测循环与平台无关：
//! `start` / `run` 只跟 [`ProbeTarget`] 打交道 —— 一个作用域 = 一套「账号轴 / 模型轴 /
//! 判据 / 发请求 / 回填」。本期实现 WB / Qoder / **Trae**（Trae 采 T1：**只探测、不回填**，
//! 见 [`ProbeTarget::adopt`] 的 Trae 分支）。
//!
//! ⚠️ 三个平台的 **state 类型不同**（`GatewayState` / `QoderGatewayState` /
//! `TraeGatewayState`），故 [`ProbeTarget`] 是 **enum**（每个变体自己持有对应 state），
//! **不是**「方法收 `&GatewayState`」的 trait 形状。
//!
//! ## 它解决什么
//!
//! 「这个模型在我的账号里是不是被限流了？」—— 池的 `model_cooldowns` 只回答
//! 「**经网关的真实流量**打过、且被打成冷却的」，客户端台账只回答「**日志里出现过**的」。
//! 两者对「**没跑过的模型**」都恒为「未知」。
//!
//! 本模块补的就是这一块：对「账号 × 模型」矩阵**逐格发一次最小请求**，把「未知」变成实判。
//!
//! ## 四条硬约束（改动时不得破坏）
//!
//! 1. **不复用会换号的出口** —— `routes::relay::relay` 自带 `max_rotate` 轮换，用它会
//!    把结果**归因到别的账号**上，矩阵立刻是假数据。这里单账号、单次、**零重试**，
//!    直接调 `UpstreamClient::chat_stream_with`。
//! 2. **探测本身不写池治理状态** —— 唯一写动作是收尾时的**显式回填**
//!    （`Pool::adopt_probe_result`，见那里的文档：只置/清冷却，不推进退避计数）。
//! 3. **判据零新造** —— 一律复用 [`crate::pool::classify_event`]（全仓唯一限流判据）；
//!    本模块只做「事件 → 单元格状态」的**纯映射**（[`probe_state_of`]，可单测 + 变异）。
//! 4. **不发空 `Bearer`** —— 缺 `access_token` 的账号一律 `skipped`（见 [`probe_cell`]）。
//!
//! ## 为什么是「作业 + 轮询」而不是同步返回
//!
//! 4 账号 × 12 模型 = 48 格，每格一次真实上游往返（秒级）⇒ 整轮**分钟级**。
//! 同步阻塞 IPC 会卡住界面。形态照抄仓内既有的 `switch_progress`
//! （进程内作业状态 + 轮询），**不引入 SSE**（本仓前端零 `EventSource`）。
//!
//! ## 并发与超时（安全审计 2026-10-16 修正）
//!
//! - **作业位是原子抢占的**（[`busy_flag`] 的 `compare_exchange`）：`start` 里
//!   「检查 Running → 写入作业」横跨多个 `await`，仅靠锁内检查是 **TOCTOU**（并发两次都过检、
//!   后者覆盖前者 ⇒ 双作业 + 取消位被复位）。
//! - **单格有硬超时**（[`PROBE_CELL_TIMEOUT_SECS`]）：宿主客户端的 600s 总超时对一格来说太长，
//!   一格挂住会把整轮拖死（取消只在格间检查）⇒ 一格最多 30s。
//!
//! ## 一期已知的简化（诚实记录）
//!
//! - **账号串行**（方案 §3.3 写的是「跨账号并发 2」）：串行更安全（不会自己把账号推成 429），
//!   代价是整轮更慢。并发留到有真实耗时数据之后再定。
//! - **模型轴**：调用方给了 `models` 就用它；没给则回落**该作用域的目录** ——
//!   WB 用 `catalogs.current(region)`（纯内存），Qoder 用网关的**内存清单缓存**
//!   （[`QoderGatewayState::cached_catalog_models`]），**都不额外联网**。
//!   而不是「逐账号各拉一次 `/models`」——后者要 N 次额外上游往返，收益不抵成本。

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock, RwLock};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use buddy_switch_core::modules::account;
use buddy_switch_core::modules::product::{
    scope_of_qoder_region, scope_of_trae_region, scope_of_wb_region, Product, Scope,
};
use buddy_switch_core::modules::qoder::region::QoderRegion;
use buddy_switch_core::modules::qoder::{api as qoder_api, cosy, PATH_CHAT_SSE};
// ★ 编译修复（2026-10-22）：`classify_http_with_detail` 从本文件搬到了
// `qoder::pool`（按产品线归位），调用点没跟着加 use ⇒ crate 编不过。
use crate::qoder::pool::classify_http_with_detail;
// 同一次归位：探测读首帧错误走 `qoder::sse::first_upstream_error`（此前直接写 `sse::`）。
use crate::qoder::sse;
use buddy_switch_core::modules::region::Region;
use buddy_switch_core::modules::trae::region::TraeRegion;
use buddy_switch_core::modules::trae::variant::TraeVariant;
use buddy_switch_core::modules::upstream::UpstreamChatResult;

use crate::error::truncate_with_marker;
use crate::outbound::{self, OutboundMeta};
use crate::pool::{classify_event, ProbeOutcome, RealmTag, UpstreamEvent};
use crate::qoder::payload;
use crate::qoder::pool::QoderErrKind;
use crate::qoder::routes::chat_once_for_account;
use crate::qoder::{QoderGateway, QoderGatewayState};
use crate::session_headers::{self, ConversationContext};

/// 探活失败原因（`ProbeCell::reason`）的字符上限。
///
/// 超限必须补截断标记（[`truncate_with_marker`]）：前端把 `reason` 直接展示给用户，
/// 没有标记时「上游只发了这么多」与「网关截掉了」看起来一模一样。
const PROBE_REASON_LIMIT: usize = 400;
use crate::state::GatewayState;
use crate::timeutil;
use crate::trae::payload as trae_payload;
use crate::trae::pool::{
    classify_http as trae_classify_http, classify_solo as trae_classify_solo, TraeErrKind, TraePool,
};
use crate::trae::routes::chat_once_for_account as trae_chat_once_for_account;
use crate::trae::sse as trae_sse;
use crate::trae::{TraeGateway, TraeGatewayState};
use crate::{GatewayProduct, WbGateway};

/// 单次探测的请求体文本：极短、中性（避免触发上游内容审核）。
pub const PROBE_TEXT: &str = "ping";

/// 格数硬上限（超出即 `truncated`，不静默丢掉）。
pub const MAX_CELLS: usize = 400;

/// 单条轴的长度上限（`uids` / `models` 来自 IPC，不能无限长）。
pub const MAX_AXIS: usize = 200;

/// 格间最小间隔：给上游留喘息，避免「探测自己把好号打成 429」。
pub const PROBE_GAP_MS: u64 = 150;

/// 单格硬超时（秒）。见模块文档「并发与超时」。
pub const PROBE_CELL_TIMEOUT_SECS: u64 = 30;

/// 单格的输出状态（**序列化名与前端/原型逐字一致**）。
///
/// ★★ 只有前三档算**限流**。`NoCredits`（402）与 `AuthDead` 必须**分列** ——
/// 把「没积分」报成「限流」会让用户去傻等解锁（`client_limits` 已踩过这个坑）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProbeState {
    /// 上游 2xx：可用。
    Ok,
    /// 模型级限流（`6004` / `IsModelRateLimit`）。
    RateLimitedModel,
    /// 账号级限流（429 且无模型标记）。
    RateLimitedAccount,
    /// 模型被上游封禁（`11102`）。
    ModelBlocked,
    /// 402 / 余额不足文案。
    NoCredits,
    /// 会话失效（`12153` / 401）。
    AuthDead,
    /// 404：该账号看不到/不支持这个模型。
    ModelUnavailable,
    /// 5xx / 其它 4xx。
    UpstreamError,
    /// ★ **上游排队**（`10605` / `isQueued`，裹在 **403** 信封里）。
    ///
    /// ## 为什么必须**独立成档**（三个「都不是」）
    ///
    /// - 不是**限流**（上游明说 `serviceAvailable:false` + `retryAfterSeconds:30`
    ///   ⇒ 等 30 秒就好，混进「限流」三档会让用户去傻等解锁）；
    /// - 不是**凭据失效**（`403` 看着像 `auth_dead`，但账号好着 —— 中继侧
    ///   `pool::queue_signal` 正是为此把排队与 `auth_dead` 分开）；
    /// - 不是**上游异常**（它明说 `modelKey` + `queueType` ⇒ 是**可换模型绕开**的）。
    ///
    /// ★ **不算限流** ⇒ 不进「限流账号数」统计（前端 `PROBE_LIMIT_STATES` 不含本档）。
    /// ★ **回填 = `Other`（不写冷却）**：与中继侧「排队不惩罚账号」一致
    ///   （`pool::apply_error` 对 `Queued` 直接 return，既不冷却也不标死）。
    UpstreamQueued,
    /// 传输层失败（`status == 0`）或单格超时。
    TransportError,
    /// 没探（无凭据 / 信封 / 账号不在库）。
    Skipped,
}

/// ★ **唯一映射点**：上游事件 → 单元格状态。纯函数，单测 + 变异覆盖。
pub fn probe_state_of(event: &UpstreamEvent) -> ProbeState {
    match event {
        UpstreamEvent::SoftRate {
            model_scoped: true, ..
        } => ProbeState::RateLimitedModel,
        UpstreamEvent::SoftRate {
            model_scoped: false,
            ..
        } => ProbeState::RateLimitedAccount,
        UpstreamEvent::ModelBlocked => ProbeState::ModelBlocked,
        UpstreamEvent::HardCredit => ProbeState::NoCredits,
        UpstreamEvent::SessionDead => ProbeState::AuthDead,
        // issue #100：401/403 同样落 `auth_dead` —— 探测要能回答「这个号的凭据还能不能用」。
        // 旧映射把它并进 `upstream_error`，于是一个「凭据过期」的号在探测结论里
        // 与「上游抖动」同档，看的人没法判断该去重新登录还是等一等。
        UpstreamEvent::AuthRejected => ProbeState::AuthDead,
        UpstreamEvent::NotFound => ProbeState::ModelUnavailable,
        UpstreamEvent::Server | UpstreamEvent::Client => ProbeState::UpstreamError,
    }
}

/// 单元格状态 → **池采纳**语义（回填用）。`Skipped` / 其它档一律 `Other`（**不动冷却**）。
pub fn probe_outcome_of(state: ProbeState, reset_at_ms: Option<i64>) -> ProbeOutcome {
    match state {
        ProbeState::Ok => ProbeOutcome::Available,
        ProbeState::RateLimitedModel => ProbeOutcome::ModelRateLimited { reset_at_ms },
        ProbeState::ModelBlocked => ProbeOutcome::ModelBlocked,
        ProbeState::RateLimitedAccount => ProbeOutcome::AccountRateLimited { reset_at_ms },
        _ => ProbeOutcome::Other,
    }
}

/// 作业阶段。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProbePhase {
    /// 从未跑过（或已被换作用域清掉）。
    Idle,
    /// 进行中。
    Running,
    /// 跑完（可能被截断）。
    Done,
    /// 用户取消；`cells` 里是**部分结果**。
    Cancelled,
}

/// 一格结果。
///
/// `Deserialize` 是给**探测结果台账**（落盘后读回）用的；`PartialEq` 让台账能判断
/// 「这一格到底变没变」（决定要不要写盘）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProbeCell {
    pub uid: String,
    pub nickname: String,
    pub model: String,
    pub state: ProbeState,
    pub http_status: u16,
    pub reset_at_ms: Option<i64>,
    /// 上游原文（截断 400 字符），排查用。
    ///
    /// ⚠️ 前端必须**纯文本**渲染（它是不可信输入：控制字符 / ANSI / 标记都可能出现）。
    pub reason: String,
    pub latency_ms: i64,
}

/// 回填的**逐条**明细（每项 = 一格真的被写了的冷却）。
///
/// ## 为什么需要它
///
/// 完成条只写「已回填 3 条」是**不可核对**的 —— 方案 §5.3「回填反馈」明确要求
/// 「可展开看明细」：用户得能看到「**哪个账号的哪个模型**被置了 / 被清了，到什么时候」。
///
/// ⚠️ 明细**必须由回填侧记录**，不能让前端拿 `cells` 反推：反推的结果与
/// `adopted_added` / `adopted_cleared` 计数可能不一致（例如该模型早已冷却到更晚的时刻），
/// 那就成了**编数据**。
#[derive(Debug, Clone, Serialize)]
pub struct ProbeAdoptRecord {
    pub uid: String,
    pub nickname: String,
    pub model: String,
    /// `"added"` = 置了冷却；`"cleared"` = 清掉了冷却。
    pub action: String,
    /// 回填**之后**该模型的冷却截止（`"cleared"` 恒为 `0`）。
    pub until_ms: i64,
}

/// 一次探测作业。
#[derive(Debug, Clone, Serialize)]
pub struct ProbeJob {
    /// 作用域字面量（`Scope::as_str()` 的稳定值，**直接进 wire**）：`"wb-cn"` … `"qoder-global"`。
    pub scope: String,
    pub phase: ProbePhase,
    pub started_at_ms: i64,
    pub finished_at_ms: Option<i64>,
    /// 总格数（`plan.len()`）。
    pub total: usize,
    /// 已探格数。
    pub done: usize,
    /// 正在探的「账号 · 模型」。
    pub current: String,
    /// 是否因 [`MAX_CELLS`] 被截断。
    pub truncated: bool,
    pub adopt: bool,
    /// 回填：置了几条模型冷却。
    pub adopted_added: usize,
    /// 回填：清了几条（`ok` 的格）。
    pub adopted_cleared: usize,
    /// 回填**逐条明细**（唯一来源；前端展开看的就是它）。
    pub adopted: Vec<ProbeAdoptRecord>,
    pub cells: Vec<ProbeCell>,
    /// 待探清单（**不序列化**，只服务本轮循环）。
    #[serde(skip)]
    pub plan: Vec<(String, String)>,
}

fn job_slot() -> &'static Mutex<Option<ProbeJob>> {
    static SLOT: OnceLock<Mutex<Option<ProbeJob>>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(None))
}

/// 取消位（进程级；逐格检查）。
fn cancel_flag() -> &'static AtomicBool {
    static FLAG: AtomicBool = AtomicBool::new(false);
    &FLAG
}

/// **作业位**（进程级）：`start` 用它原子抢占，杜绝 TOCTOU。
///
/// ⚠️ 抢到之后由 [`run`] 的 **RAII 归位器**释放（正常结束 / 取消结束 / 早退 / **panic 展开**
/// 四种退出都覆盖）；[`cancel`] 另有一条**兜底自愈**（没有真在跑的作业时顺手释放）。
/// `start` 成功但调用方**没 spawn `run`** 会让作业位一直占着（下一次 `start` 报
/// 「已有探测在进行中」）—— 此时用户可调一次 `cancel` 自救，不必重启进程。
fn busy_flag() -> &'static AtomicBool {
    static FLAG: AtomicBool = AtomicBool::new(false);
    &FLAG
}

/// 读作业快照（`{phase:"idle"}` 表示从未跑过）。
pub fn status() -> Value {
    let guard = job_slot().lock().unwrap();
    match guard.as_ref() {
        Some(job) => serde_json::to_value(job).unwrap_or(Value::Null),
        None => json!({ "phase": "idle" }),
    }
}

/// 请求取消（逐格检查；已探到的格**保留**）。
///
/// ★ **兜底自愈**：如果没有真在跑的作业（作业槽为空 / `phase != Running`），顺手释放作业位。
/// 覆盖「`run` panic / 宿主没 spawn `run` ⇒ 作业位永久占着」这一死角 —— 让用户能用「取消」
/// 自救，而不是只能重启进程。
///
/// ⚠️ **作业真在跑时绝不能释放**：否则第二次 `start` 会与第一个作业同时往同一个作业槽写。
pub fn cancel() -> Value {
    cancel_flag().store(true, Ordering::SeqCst);
    let running = job_slot()
        .lock()
        .unwrap()
        .as_ref()
        .map(|job| job.phase == ProbePhase::Running)
        .unwrap_or(false);
    if !running {
        busy_flag().store(false, Ordering::SeqCst);
    }
    json!({ "ok": true })
}

// ---------------------------------------------------------------------------
// 探测结果台账（跨重启累积、按作用域隔离）

/// 一个作用域的台账：`uid → model → 格`。
///
/// 用 `HashMap` 而不是矩阵：`(uid, model)` 是稀疏的（账号页只探台账里出现过的格），
/// 矩阵形状会逼着存一堆「不存在」的空格。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
struct ScopeLedger {
    /// 最近一次写入时刻（毫秒）；前端只用于「实测于 …」这类说明，不参与判据。
    updated_at_ms: i64,
    cells: HashMap<String, HashMap<String, ProbeCell>>,
}

/// 落盘形状：`{ "version": 1, "scopes": { "wb-cn": { … }, … } }`。
///
/// ⚠️ `scopes` 的键是 **String**（不是 `Scope`）：未来新增/删除作用域时，未知键只应被忽略，
/// 而不是让整份反序列化失败（`disabled_models` 踩过：未知键 ⇒ 回落空表 ⇒ 清空全部偏好）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
struct LedgerFile {
    version: u32,
    scopes: HashMap<String, ScopeLedger>,
}

/// 台账格式版本（**只做诊断**：字段是纯增量的，故不拿出来做 fail-closed 拒绝）。
const LEDGER_VERSION: u32 = 1;

/// 台账文件路径：`~/.buddy-switch/probe_results.json`（与 `gateway_config.json` 同目录）。
fn ledger_file() -> std::path::PathBuf {
    buddy_switch_core::modules::region::gateway_config_file().with_file_name("probe_results.json")
}

/// 进程内唯一台账句柄（写侧串行）。懒加载一次，之后只在 `finish` 时写。
fn shared_ledger() -> &'static RwLock<LedgerFile> {
    static SHARED: OnceLock<RwLock<LedgerFile>> = OnceLock::new();
    SHARED.get_or_init(|| RwLock::new(load_ledger()))
}

fn load_ledger() -> LedgerFile {
    load_ledger_from(&ledger_file())
}

/// 读台账文件（**纯 IO，路径可注入**：单测用它做真实 round-trip）。
///
/// 缺失 / 损坏一律回落空表：台账是加分项，**绝不能**因为一个坏文件让探测或命令报错。
fn load_ledger_from(path: &std::path::Path) -> LedgerFile {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str::<LedgerFile>(&text).ok())
        .unwrap_or_default()
}

/// 原子写台账文件（路径可注入；`atomic_write` 是「临时文件 + rename」）。
fn save_ledger_to(path: &std::path::Path, store: &LedgerFile) -> Result<(), String> {
    let content = serde_json::to_string_pretty(store).map_err(|error| error.to_string())?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    buddy_switch_core::modules::config::atomic_write(path, &content)
        .map_err(|error| error.to_string())
}

impl LedgerFile {
    /// 合并一批格（**同格覆盖、异格保留**）；返回是否真的有变化（决定要不要写盘）。
    ///
    /// ★ **只合并不擦除**：某轮只探 1 格不会把其它格抹掉。这是「账号页逐行实测」能累积的前提，
    /// 与前端 `mergeProbeCells` 同一口径（后端也必须有，否则重启即空）。
    fn merge(&mut self, scope: &str, cells: &[ProbeCell]) -> bool {
        let entry = self.scopes.entry(scope.to_string()).or_default();
        let mut changed = false;
        for cell in cells {
            if cell.uid.is_empty() || cell.model.is_empty() {
                continue;
            }
            let bucket = entry.cells.entry(cell.uid.clone()).or_default();
            match bucket.get(&cell.model) {
                Some(previous) if previous == cell => continue,
                _ => {
                    bucket.insert(cell.model.clone(), cell.clone());
                    changed = true;
                }
            }
        }
        if changed {
            entry.updated_at_ms = timeutil::now_ms();
            self.version = LEDGER_VERSION;
        }
        changed
    }

    /// 该作用域的格（扁平、确定性排序：`uid` → `model`）。
    ///
    /// 排序不是为了好看：`HashMap` 迭代顺序每次都变，不排序会让同一份数据两次读出来不一致
    /// （前端 `mergeProbeCells` 靠值比较判变化，顺序抖动会造出无意义的写入与重渲染）。
    fn flat(&self, scope: &str) -> Vec<ProbeCell> {
        let Some(entry) = self.scopes.get(scope) else {
            return Vec::new();
        };
        let mut out: Vec<ProbeCell> = entry
            .cells
            .values()
            .flat_map(|models| models.values().cloned())
            .collect();
        out.sort_by(|a, b| a.uid.cmp(&b.uid).then_with(|| a.model.cmp(&b.model)));
        out
    }
}

/// 读某作用域的**实测台账**快照（跨重启）。没有记录时返回空 `cells`（不是错误）。
///
/// 与 [`status`] 的分工：
///
/// | | `status` | 本函数 |
/// |---|---|---|
/// | 是什么 | **本轮作业**（进度 / 相位 / 本次回填） | 这些格**最近一次**的实判 |
/// | 生命周期 | 进程内单槽，新作业覆盖 | 落盘累积（按作用域） |
/// | 谁读 | 网卡进度条 + 矩阵 | 账号页台账卡（重启后仍要看到实测结论） |
///
/// ⚠️ 前端不判 `phase`：`cells` 为空就是没探过。`updated_at_ms` 为 `null` 同理。
pub fn ledger_snapshot(scope: &str) -> Value {
    let guard = shared_ledger().read().unwrap();
    let cells = guard.flat(scope);
    let updated = guard.scopes.get(scope).map(|entry| entry.updated_at_ms);
    json!({
        "scope": scope,
        "updated_at_ms": updated.filter(|value| *value > 0),
        "cells": cells,
    })
}

/// 把一轮作业的结果并入台账并落盘（**尽力**：失败不影响探测结论，内存态已正确）。
fn record_ledger(scope: &str, cells: &[ProbeCell]) {
    if cells.is_empty() {
        return;
    }
    let snapshot = {
        let mut guard = shared_ledger().write().unwrap();
        if !guard.merge(scope, cells) {
            return;
        }
        guard.clone()
    };
    // 落盘失败不报错：台账是**加分项**（下次 finish 会再写一次），不该让探测本身失败。
    let _ = save_ledger_to(&ledger_file(), &snapshot);
}

/// 测试专用：复位作业槽 / 取消位 / 作业位。**只在测试构建里存在**（别在生产误清在跑作业）。
#[cfg(test)]
pub fn reset_for_test() {
    *job_slot().lock().unwrap() = None;
    cancel_flag().store(false, Ordering::SeqCst);
    busy_flag().store(false, Ordering::SeqCst);
}

/// 测试专用：直接放一份作业进去（用于断言 wire 形状）。
#[cfg(test)]
pub fn set_job_for_test(job: ProbeJob) {
    *job_slot().lock().unwrap() = Some(job);
}

#[cfg(test)]
fn test_cell(uid: &str, model: &str, state: ProbeState) -> ProbeCell {
    ProbeCell {
        uid: uid.to_string(),
        nickname: format!("n-{uid}"),
        model: model.to_string(),
        state,
        http_status: 200,
        reset_at_ms: None,
        reason: String::new(),
        latency_ms: 1,
    }
}

fn realm_of(region: Region) -> RealmTag {
    match region {
        Region::Cn => RealmTag::Cn,
        Region::Global => RealmTag::Global,
    }
}

// ---------------------------------------------------------------------------
// 探测目标（平台无关的探测循环的抽象）
// ---------------------------------------------------------------------------

/// ★ **唯一映射点（Qoder）**：Qoder 上游错误 → 单元格状态。纯函数，单测 + 变异覆盖。
///
/// ## 为什么不能复用 [`probe_state_of`]（WB 专属）
///
/// WB 的判据来自**响应体**（[`classify_event`]：`6004` / `IsModelRateLimit` / `11102` / 余额文案…）。
/// Qoder 的 HTTP 层判据是状态码（[`classify_http_with_detail`]），但**必须带 body** ——
/// 上游把「排队」塞进 **403 信封**里（`10605` / `isQueued`），只看状态码会把它当成
/// 「凭据失效」（`classify_http` 的老毛病），`pool::queue_signal` 正是为此存在。
///
/// ## 档位对应
///
/// | 上游 | 单元格 |
/// |:--|:--|
/// | 排队（`10605` / `isQueued`，403 信封） | `upstream_queued`（**独立成档**：等 30 秒就好，不是限流、不是凭据死） |
/// | 429 | `rate_limited_model`（Qoder 上游本来就**按模型**报限流） |
/// | 401 / 403（无排队信号） | `auth_dead` |
/// | 404 | `model_unavailable` |
/// | 402 | `upstream_error`（探测的「一格」是**模型**维度：凭据好着，只是没钱了） |
/// | 5xx / 传输 | `upstream_error` |
pub fn probe_state_of_qoder(status: u16, detail: &str) -> ProbeState {
    match classify_http_with_detail(status, detail) {
        QoderErrKind::Queued => ProbeState::UpstreamQueued,
        QoderErrKind::RateLimited => ProbeState::RateLimitedModel,
        QoderErrKind::AuthDead => ProbeState::AuthDead,
        QoderErrKind::Transient => ProbeState::UpstreamError,
        QoderErrKind::ClientError => {
            if status == 404 {
                ProbeState::ModelUnavailable
            } else {
                ProbeState::UpstreamError
            }
        }
        // 402：账号级余额耗尽。探测的「一格」是**模型**维度，这里如实报上游错误
        // （把账号标成「凭据死」是错的 —— 凭据好着，只是没钱了）。
        QoderErrKind::BalanceExhausted => ProbeState::UpstreamError,
    }
}

/// ★ **唯一映射点（Trae）**：Trae 上游错误类别 → 单元格状态。纯函数，单测 + 变异覆盖。
///
/// ## Trae **无法细分**模型级 / 账号级 ⇒ 一律 `RateLimitedModel`
///
/// Trae 池只有**账号级**冷却（`TraePoolEntry` 没有 `model_cooldowns`），上游也不区分
/// 「哪个模型被限」与「整个账号被限」⇒ 三档限流里只能落 `RateLimitedModel`。
/// **取舍**：把账号级限流也记成模型级，是**探测呈现**层面的近似；因为 Trae 探测**不回填**
/// （见 [`ProbeTarget::adopt`] 的 Trae 分支），这个近似**不会**影响路由 / 冷却。
///
/// ## 判据零新造
///
/// 复用 `crate::trae::pool` 的两个分类入口：`classify_http`（HTTP 层）与 `classify_solo`
/// （SSE `event:error` 层）—— 全仓唯一的 Trae 错误分类入口，本模块只做「类别 → 单元格状态」映射。
///
/// ## 红线：`PlanLimit`（套餐额度用尽）**不是限流**
///
/// 方案 §1.2 的 Trae 列把 `PlanLimit`（`1005` / 含 `plan`）列为 `no_credits`，与限流**分列**
/// —— 把「额度用尽」报成「限流」会让用户去傻等解锁（`client_limits` 已踩过这个坑）。
pub fn probe_state_of_trae(kind: TraeErrKind) -> ProbeState {
    match kind {
        // 限流（429 / 4008 / 含 quota·exceeded·rate）：Trae 无法细分 ⇒ 模型级。
        TraeErrKind::SoftRate => ProbeState::RateLimitedModel,
        // 套餐额度用尽（1005 / 含 plan）：**不是限流**，独立成 `no_credits`。
        TraeErrKind::PlanLimit => ProbeState::NoCredits,
        // 会话失效（带 `12153` 标记）与「凭据被拒」（裸 401/403）都归 `AuthDead`：
        // 对用户来说下一步都是「去把这个号重新登录一次」，区别只在自动自愈的概率。
        // ★ issue #108：此前 401 一律进 SessionDead，因此这里只有一条分支；
        // 现在分类拆开了，**两条都必须映射**，否则 `AuthRejected` 会掉进
        // `UpstreamError`，把「去重新登录」误报成「上游抖动」。
        TraeErrKind::SessionDead | TraeErrKind::AuthRejected => ProbeState::AuthDead,
        // 404：该账号看不到 / 不支持这个模型。
        TraeErrKind::NotFound => ProbeState::ModelUnavailable,
        // 5xx / 4xx / 业务码 / 「不归因于账号」（如 code 0 断流、4001 模型配置为空）：
        // 中性归「上游异常」—— 都不是限流。
        TraeErrKind::Server | TraeErrKind::Client | TraeErrKind::Business | TraeErrKind::None => {
            ProbeState::UpstreamError
        }
    }
}

/// 探测目标：一个作用域 = 一套「账号轴 / 模型轴 / 判据 / 发请求 / 回填」。
///
/// ## 为什么是 enum 而不是 trait
///
/// 三个平台的 **state 类型不同**（`GatewayState` / `QoderGatewayState` / `TraeGatewayState`），
/// 无法用「方法收 `&GatewayState`」的统一引用参数表达。故每个变体**自己持有**对应平台的
/// state，方法实现按平台分叉。新增平台 = 加一个变体。
///
/// `start` / `run` 只跟本类型打交道，**不再出现 `Region`**。
#[derive(Clone)]
pub enum ProbeTarget {
    /// WorkBuddy（region 决定账号库 / 目录 / 判据）。
    Wb(GatewayState, Region),
    /// Qoder（region 决定账号库 / 内存清单缓存 / 判据）。
    Qoder(QoderGatewayState, QoderRegion),
    /// Trae（variant 决定账号池 / 客户端清单 / 判据；**只探测、不回填**）。
    Trae(TraeGatewayState, TraeVariant),
}

/// 手写 `Debug`（**只打作用域**，不打印 state）。
///
/// 理由：两个平台 state 类型**不都**实现 `Debug`（`GatewayState` 只 derive `Clone`），
/// 而 `ProbeTarget` 需要在单测里配合 `unwrap_err()` 等 API ⇒ 手写一个轻量实现，
/// 顺带避免把整个 state（含密钥 / 池）打进日志。
impl std::fmt::Debug for ProbeTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ProbeTarget({})", self.scope())
    }
}

impl ProbeTarget {
    /// 作业里记的**作用域字面量**（进 wire）：`"wb-cn"` … `"qoder-global"`。
    ///
    /// 走 core 的 `scope_of_*_region`（穷举、无回落），**不**手工拼字符串 —— 免得平台短标识漂移。
    pub fn scope(&self) -> &'static str {
        match self {
            ProbeTarget::Wb(_, region) => scope_of_wb_region(*region).as_str(),
            ProbeTarget::Qoder(_, region) => scope_of_qoder_region(*region).as_str(),
            ProbeTarget::Trae(_, variant) => scope_of_trae_region(variant.region()).as_str(),
        }
    }

    /// 账号轴：同步该作用域的账号库进池，返回 **uid 列表**（昵称在 [`Self::probe_cell`] 里现取）。
    async fn account_uids(&self) -> Vec<String> {
        match self {
            ProbeTarget::Wb(state, region) => {
                let realm = realm_of(*region);
                // 与账号库对齐 —— 否则「刚导入、还没发过请求」的账号不在池里，探测会漏掉它们。
                let accounts = account::load_accounts_for(*region);
                let mut pool = state.pool.write().await;
                pool.sync_accounts(&accounts, Some(realm));
                pool.entries()
                    .filter(|entry| entry.realm == Some(realm))
                    .map(|entry| entry.uid.clone())
                    .collect()
            }
            ProbeTarget::Qoder(state, region) => {
                let mut pools = state.pools.lock().await;
                let pool = pools.entry(*region).or_default();
                // `sync_for` 从账号库重建（保留运行态），保证「导入即入池」。
                pool.sync_for(*region);
                pool.entries().map(|entry| entry.uid.clone()).collect()
            }
            ProbeTarget::Trae(state, variant) => {
                let mut pools = state.pools.lock().await;
                let pool = pools
                    .entry(*variant)
                    .or_insert_with(|| TraePool::for_variant(*variant));
                // `sync_for` 按**变体**（其实按区域）从账号库重建 —— 与网关选号同源，
                // 保证「导入即入池」；`sync_for` 会滤掉空 JWT 的账号。
                pool.sync_for(*variant);
                pool.entries()
                    .iter()
                    .map(|entry| entry.uid.clone())
                    .collect()
            }
        }
    }

    /// 模型轴：该作用域目录里的模型 id（**纯内存、不联网**；见模块头「模型轴」）。
    async fn catalog_models(&self) -> Vec<String> {
        match self {
            ProbeTarget::Wb(state, region) => state
                .catalogs
                .current(*region)
                .models
                .into_iter()
                .map(|model| model.id)
                .collect(),
            ProbeTarget::Qoder(state, region) => state
                .cached_catalog_models(*region)
                .await
                .into_iter()
                .filter_map(|entry| {
                    entry
                        .get("id")
                        .or_else(|| entry.get("key"))
                        .and_then(Value::as_str)
                        .map(str::to_string)
                })
                .collect(),
            ProbeTarget::Trae(state, variant) => {
                // 按**变体**读客户端（上游下发）清单 —— 所有账号共用同一份 ⇒ **全行同列**
                // （方案 §1.1 的坑）。复用网关 `/v1/models` 的同一份选择逻辑
                // （`models_response_for`：只取该变体 `function` 真能调的模型；客户端缓存
                // 读不到时它自己回落静态清单），保证「探测的模型 == 网关会列的模型」。
                // ★ Max Mode 不再有影子条目 ⇒ 探测与清单同源，无需再分岔。
                //
                // ⚠️ 读客户端 `state.vscdb` 是**同步阻塞** IO（网关 `/v1/models` 同样走
                // `spawn_blocking`）⇒ 这里也搬进阻塞线程池，别卡住异步运行时。
                let variant = *variant;
                let max_mode = state.max_mode_now();
                tokio::task::spawn_blocking(move || {
                    let body = trae_payload::models_response_for(
                        variant,
                        &std::collections::BTreeSet::new(),
                        max_mode,
                    );
                    body.get("data")
                        .and_then(Value::as_array)
                        .map(|rows| {
                            rows.iter()
                                .filter_map(|row| {
                                    row.get("id").and_then(Value::as_str).map(str::to_string)
                                })
                                .collect::<Vec<String>>()
                        })
                        .unwrap_or_default()
                })
                .await
                .unwrap_or_default()
            }
        }
    }

    /// 探一格（单账号、单次、零重试、有硬超时）。
    async fn probe_cell(&self, uid: &str, model: &str) -> ProbeCell {
        match self {
            ProbeTarget::Wb(state, region) => probe_cell_wb(state, *region, uid, model).await,
            ProbeTarget::Qoder(state, region) => probe_cell_qoder(state, *region, uid, model).await,
            ProbeTarget::Trae(state, variant) => probe_cell_trae(state, *variant, uid, model).await,
        }
    }

    /// 回填（唯一写动作）：把探测结论写进池冷却。返回 `(added, cleared, 明细)`。
    ///
    /// ★ **Trae 是显式空操作**（T1：只探测、不回填）—— 理由见该分支的注释。
    async fn adopt(&self, cells: &[ProbeCell], now: i64) -> (usize, usize, Vec<ProbeAdoptRecord>) {
        match self {
            ProbeTarget::Wb(state, region) => adopt_wb(state, *region, cells, now).await,
            ProbeTarget::Qoder(state, region) => adopt_qoder(state, *region, cells, now).await,
            ProbeTarget::Trae(_, _) => {
                // ★★ **显式空操作**（T1：Trae 只探测、不回填）。
                //
                // ## 为什么必须是空操作（方案 §4.5.4 的 T1）
                //
                // Trae 池**没有「模型级冷却」这个概念** —— `TraePoolEntry` 只有账号级
                // `cooldown_until` / `cooldown_reason`，`apply_error(uid, kind, reason)`
                // **连 `model` 参数都没有**。若把「模型 M 被限流」回填成**账号级**冷却，
                // 会把该账号的**其它模型一起打死**（过度冷却）—— 正是方案 §4.5.4 明确
                // **不建议**的 T3。故本期 Trae 采 T1：探测结果**只呈现**，绝不写池。
                //
                // ⚠️ 返回空计数 + 空明细（不是 `todo!()`）—— 前端据此把 `adopted` 恒显示
                // 为空，与「Trae 卡片上隐藏/禁用回填开关」配套，**不让用户以为回填生效了**。
                (0, 0, Vec::new())
            }
        }
    }
}

/// 由作用域字符串构造探测目标（**fail-closed**：未知 / 空 / 拼错一律 `Err`）。
///
/// `Scope::parse` 的错误用 `ScopeError::message()`（可读、列出合法取值）。
pub fn target_for_scope(raw: &str) -> Result<ProbeTarget, String> {
    let scope = Scope::parse(raw).map_err(|error| error.message())?;
    match scope.product() {
        Product::Workbuddy => Ok(ProbeTarget::Wb(
            <WbGateway as GatewayProduct>::shared_state(),
            scope.wb_region().expect("WB 作用域必有 WB 区域"),
        )),
        Product::Qoder => Ok(ProbeTarget::Qoder(
            <QoderGateway as GatewayProduct>::shared_state(),
            scope.qoder_region().expect("Qoder 作用域必有 Qoder 区域"),
        )),
        Product::Trae => Ok(ProbeTarget::Trae(
            <TraeGateway as GatewayProduct>::shared_state(),
            trae_variant_of(scope.trae_region().expect("Trae 作用域必有 Trae 区域")),
        )),
    }
}

/// Trae 区域 → 该区域**当前客户端**的变体（`trae-cn` → `TraeMerged`，
/// `trae-global` → `GlobalTraeMerged`）。
///
/// ## 为什么是**合并版**
///
/// 账号库 / 冷却 / 积分都按 `variant.region()` 分家 ⇒ 区域内**任一**变体读到的账号轴
/// 完全相同；但**模型清单**按变体分家（`read_client_model_list`），且 `function_for`
/// 决定请求体里的 `function` 白名单。官方已把两条老程序位合并成单一客户端
/// （CN `Trae CN` / 国际 `Trae`，见 `TraeVariant::all` 与 `parse("cn"/"global")`），
/// 因此探测用**合并版**变体 —— 与前端 `regionPrograms` 给的 `trae_merged` /
/// `global_trae_merged` 逐字一致。
fn trae_variant_of(region: TraeRegion) -> TraeVariant {
    match region {
        TraeRegion::Cn => TraeVariant::TraeMerged,
        TraeRegion::Global => TraeVariant::GlobalTraeMerged,
    }
}

/// WB 回填：只置/清冷却，**绝不推 EMA**（唯一入口 [`crate::pool::Pool::adopt_probe_result`]）。
async fn adopt_wb(
    state: &GatewayState,
    region: Region,
    cells: &[ProbeCell],
    now: i64,
) -> (usize, usize, Vec<ProbeAdoptRecord>) {
    let realm = realm_of(region);
    let mut pool = state.pool.write().await;
    // 与账号库再对齐一次（探测期间用户可能导入/删了账号）。
    let accounts = account::load_accounts_for(region);
    pool.sync_accounts(&accounts, Some(realm));
    let (mut added, mut cleared) = (0usize, 0usize);
    let mut records: Vec<ProbeAdoptRecord> = Vec::new();
    for cell in cells {
        if cell.state == ProbeState::Skipped {
            continue;
        }
        // 墙钟缺失 / 过期时的**冷却下限**落在 `adopt_probe_result` 内部（回填的唯一入口）——
        // 这里原样透传，**不要**在调用方再补一遍（换个调用方就漏）。
        let outcome = probe_outcome_of(cell.state, cell.reset_at_ms);
        if pool.adopt_probe_result(&cell.uid, Some(realm), &cell.model, &outcome, now) {
            let was_cleared = matches!(outcome, ProbeOutcome::Available);
            let until_ms = if was_cleared {
                0
            } else {
                pool.get(&cell.uid)
                    .and_then(|entry| entry.model_cooldowns.get(&cell.model))
                    .map(|cool| cool.until_ms)
                    .unwrap_or(0)
            };
            records.push(ProbeAdoptRecord {
                uid: cell.uid.clone(),
                nickname: cell.nickname.clone(),
                model: cell.model.clone(),
                action: if was_cleared { "cleared" } else { "added" }.to_string(),
                until_ms,
            });
            if was_cleared {
                cleared += 1;
            } else {
                added += 1;
            }
        }
    }
    (added, cleared, records)
}

/// Qoder 回填：只置/清 `model_cooldowns`（[`crate::qoder::pool::QoderPool::adopt_probe_result`]），
/// **绝不推 EMA**。语义与 [`adopt_wb`] 逐条对齐。
async fn adopt_qoder(
    state: &QoderGatewayState,
    region: QoderRegion,
    cells: &[ProbeCell],
    now: i64,
) -> (usize, usize, Vec<ProbeAdoptRecord>) {
    let mut pools = state.pools.lock().await;
    let pool = pools.entry(region).or_default();
    // 与账号库再对齐一次（探测期间用户可能导入/刷新了账号）。
    pool.sync_for(region);
    let (mut added, mut cleared) = (0usize, 0usize);
    let mut records: Vec<ProbeAdoptRecord> = Vec::new();
    for cell in cells {
        if cell.state == ProbeState::Skipped {
            continue;
        }
        let outcome = probe_outcome_of(cell.state, cell.reset_at_ms);
        if pool.adopt_probe_result(&cell.uid, &cell.model, &outcome, now) {
            let was_cleared = matches!(outcome, ProbeOutcome::Available);
            let until_ms = if was_cleared {
                0
            } else {
                pool.entries()
                    .find(|entry| entry.uid == cell.uid)
                    .and_then(|entry| entry.model_cooldowns.get(&cell.model))
                    .copied()
                    .unwrap_or(0)
            };
            records.push(ProbeAdoptRecord {
                uid: cell.uid.clone(),
                nickname: cell.nickname.clone(),
                model: cell.model.clone(),
                action: if was_cleared { "cleared" } else { "added" }.to_string(),
                until_ms,
            });
            if was_cleared {
                cleared += 1;
            } else {
                added += 1;
            }
        }
    }
    (added, cleared, records)
}

/// 去重保序。
fn dedup_keep_order(items: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for item in items {
        let item = item.trim().to_string();
        if item.is_empty() || !seen.insert(item.clone()) {
            continue;
        }
        out.push(item);
    }
    out
}

/// 展开待探清单：**账号为外层**（与原型/前端的矩阵行序一致），超出 [`MAX_CELLS`] 截断。
///
/// 返回 `(plan, truncated)`。纯函数（可单测：顺序、截断）。
pub(crate) fn build_plan(accounts: &[String], models: &[String]) -> (Vec<(String, String)>, bool) {
    let mut plan = Vec::new();
    'outer: for uid in accounts {
        for model in models {
            plan.push((uid.clone(), model.clone()));
            if plan.len() >= MAX_CELLS {
                break 'outer;
            }
        }
    }
    let truncated = plan.len() < accounts.len() * models.len();
    (plan, truncated)
}

/// 显式格清单的一项（`probe_start` 的 `cells` 参数）。
///
/// ## 为什么需要它（和 `uids × models` 的区别）
///
/// `uids × models` 是**笛卡尔积**：账号页台账里有 3 条（3 个账号 / 2 个模型）就会跑 **6 格** ——
/// 多打的 3 格既费配额、又答非所问（用户要查证的只是台账里出现过的组合）。
/// 账号页的「实测 N 格」因此传**显式格清单**：`cells: [{uid, model}, …]`，2 项就是 2 格。
///
/// ⚠️ **与两根轴互斥**：`cells` 给了就忽略 `uids` / `models`（[`prepare`] 里先判它）。
/// 网卡「开始探测」的选择态仍走两根轴（那里用户是显式勾了账号与模型，笛卡尔积正是他要的）。
#[derive(Debug, Clone, Deserialize)]
pub struct ProbeStartCell {
    pub uid: String,
    pub model: String,
}

/// 去重保序（显式格清单用；`uid` / `model` 都 trim，空的丢掉）。
///
/// `probe_ledger` 与前端都是按 `(uid, model)` 定位的 ⇒ 重复项必须去重，
/// 否则同一格会被探两次（还会让 `total` 与真实格数不符）。
pub(crate) fn dedup_pairs(
    pairs: impl IntoIterator<Item = (String, String)>,
) -> Vec<(String, String)> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for (uid, model) in pairs {
        let (uid, model) = (uid.trim().to_string(), model.trim().to_string());
        if uid.is_empty() || model.is_empty() || !seen.insert((uid.clone(), model.clone())) {
            continue;
        }
        out.push((uid, model));
    }
    out
}

/// 启动一次探测：**原子抢占作业位** + 建作业（快），逐格执行由调用方 spawn [`run`]。
///
/// 返回本轮格数。作业位被占时返回 `Err`。
pub async fn start(
    target: &ProbeTarget,
    uids: Option<Vec<String>>,
    models: Option<Vec<String>>,
    cells: Option<Vec<ProbeStartCell>>,
    adopt: bool,
) -> Result<usize, String> {
    // ★ 原子抢占：`compare_exchange` 让并发两次 `start` **只有一个**能进。
    // 只靠「锁内查 phase」是 TOCTOU —— 检查与写入横跨 `pool.write().await`，锁早就放了。
    if busy_flag()
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return Err("已有探测在进行中，请等它结束或先取消".to_string());
    }
    match prepare(target, uids, models, cells, adopt).await {
        Ok(total) => Ok(total),
        Err(error) => {
            busy_flag().store(false, Ordering::SeqCst);
            Err(error)
        }
    }
}

/// [`start`] 的主体（校验 + 建作业）。任何 `Err` 由 [`start`] 负责释放作业位。
async fn prepare(
    target: &ProbeTarget,
    uids: Option<Vec<String>>,
    models: Option<Vec<String>>,
    cells: Option<Vec<ProbeStartCell>>,
    adopt: bool,
) -> Result<usize, String> {
    // 账号轴：同步账号库进池后取 uid 列表（池为空 ⇒ 明确报错）。
    let pool_rows = target.account_uids().await;
    if pool_rows.is_empty() {
        return Err(format!("{} 的账号池里没有账号", target.scope()));
    }

    /* ★ 两条互斥路径（`cells` 优先）：
    - 显式格清单（账号页台账卡）：**只探点名的格**，不铺笛卡尔积；
    - 两根轴（网卡选择态）：账号为外层展开 —— 那里用户是显式勾了账号与模型。 */
    let (plan, truncated) = match cells {
        Some(list) => {
            let wanted = dedup_pairs(list.into_iter().map(|cell| (cell.uid, cell.model)));
            if wanted.is_empty() {
                return Err("没有选中任何格".to_string());
            }
            // 显式清单**不截断**：用户点名的格被悄悄丢掉，比直接报错更糟（他会以为自己测过了）。
            if wanted.len() > MAX_CELLS {
                return Err(format!("格数超过上限（{MAX_CELLS}）"));
            }
            // 只保留池里真有的账号（与 `uids` 分支同口径：不在库 ⇒ 跳过，而不是整轮报错）。
            let plan: Vec<(String, String)> = wanted
                .into_iter()
                .filter(|(uid, _)| pool_rows.iter().any(|row| row == uid))
                .collect();
            if plan.is_empty() {
                return Err("选中的格都不在该作用域的账号池里".to_string());
            }
            (plan, false)
        }
        None => {
            let accounts: Vec<String> = match uids {
                Some(list) => {
                    if list.len() > MAX_AXIS {
                        return Err(format!("账号数超过上限（{MAX_AXIS}）"));
                    }
                    let wanted = dedup_keep_order(list);
                    if wanted.is_empty() {
                        return Err("没有选中任何账号".to_string());
                    }
                    let picked: Vec<String> = wanted
                        .iter()
                        .filter(|uid| pool_rows.iter().any(|row| row == *uid))
                        .cloned()
                        .collect();
                    if picked.is_empty() {
                        return Err("选中的账号都不在该作用域的池里".to_string());
                    }
                    picked
                }
                None => pool_rows,
            };

            let models = match models {
                Some(list) => {
                    if list.len() > MAX_AXIS {
                        return Err(format!("模型数超过上限（{MAX_AXIS}）"));
                    }
                    dedup_keep_order(list)
                }
                // 回落该作用域的**目录**（纯内存、不联网；见模块头「模型轴」）。
                None => dedup_keep_order(target.catalog_models().await),
            };
            if models.is_empty() {
                return Err("没有可探测的模型（清单为空，请先「重新识别」）".to_string());
            }

            build_plan(&accounts, &models)
        }
    };
    let total = plan.len();

    cancel_flag().store(false, Ordering::SeqCst);
    *job_slot().lock().unwrap() = Some(ProbeJob {
        scope: target.scope().to_string(),
        phase: ProbePhase::Running,
        started_at_ms: timeutil::now_ms(),
        finished_at_ms: None,
        total,
        done: 0,
        current: String::new(),
        truncated,
        adopt,
        adopted_added: 0,
        adopted_cleared: 0,
        adopted: Vec::new(),
        cells: Vec::with_capacity(total),
        plan,
    });
    Ok(total)
}

/// 逐格执行（调用方 spawn）。结束时会按 `adopt` 决定是否回填池冷却。
///
/// ⚠️ **作业位的释放由顶部的 RAII 归位器（[`ReleaseBusy`]）保证** —— 覆盖四种退出：
/// 正常结束 / 取消结束 / 早退（作业被清）/ **panic 展开**。函数体里**不再**有任何零散的
/// `busy_flag().store(false, …)`：「两处各自负责释放」这种写法迟早会漂移，且漏掉 panic 那条出口
/// （一旦 `run` panic 或宿主压根没 spawn 它，作业位就永久 `true` —— 之后每次 `start` 都报
/// 「已有探测在进行中」，**用户可见却不可自愈**，只能重启进程）。
pub async fn run(target: ProbeTarget) {
    /// 作业位的 **RAII 归位器**：`run` 返回或 panic 展开时都会释放。
    ///
    /// 放在 `run` 内部而不是 `start`：`start` 只是**抢占**，作业位的生命周期属于这一轮执行。
    struct ReleaseBusy;
    impl Drop for ReleaseBusy {
        fn drop(&mut self) {
            busy_flag().store(false, Ordering::SeqCst);
        }
    }
    let _release = ReleaseBusy;

    loop {
        if cancel_flag().load(Ordering::SeqCst) {
            finish(ProbePhase::Cancelled, &target).await;
            return;
        }
        let next = {
            let mut guard = job_slot().lock().unwrap();
            let Some(job) = guard.as_mut() else {
                // 作业被清（例如换作用域）⇒ 直接退出；作业位由上面的 RAII guard 释放。
                return;
            };
            match job.plan.pop() {
                Some(pair) => {
                    job.current = format!("{} · {}", pair.0, pair.1);
                    Some(pair)
                }
                None => None,
            }
        };
        let Some((uid, model)) = next else {
            finish(ProbePhase::Done, &target).await;
            return;
        };

        // 作业在两次加锁之间被清（例如 `cancel` 自愈 / 换作用域）⇒ 直接退出（与改动前一致）。
        if job_slot().lock().unwrap().is_none() {
            return;
        }

        let cell = target.probe_cell(&uid, &model).await;

        {
            let mut guard = job_slot().lock().unwrap();
            let Some(job) = guard.as_mut() else {
                return;
            };
            job.done += 1;
            job.cells.push(cell);
        }
        tokio::time::sleep(Duration::from_millis(PROBE_GAP_MS)).await;
    }
}

/// 收尾：可选回填 + 置终态。**不再负责释放作业位** —— 那由 [`run`] 的 RAII 归位器统一保证。
async fn finish(phase: ProbePhase, target: &ProbeTarget) {
    let (adopt, cells) = {
        let guard = job_slot().lock().unwrap();
        match guard.as_ref() {
            Some(job) => (job.adopt, job.cells.clone()),
            None => return,
        }
    };

    let (mut added, mut cleared) = (0usize, 0usize);
    let mut records: Vec<ProbeAdoptRecord> = Vec::new();
    if adopt {
        let now = timeutil::now_ms();
        let outcome = target.adopt(&cells, now).await;
        added = outcome.0;
        cleared = outcome.1;
        records = outcome.2;
    }

    {
        let mut guard = job_slot().lock().unwrap();
        if let Some(job) = guard.as_mut() {
            job.phase = phase;
            job.finished_at_ms = Some(timeutil::now_ms());
            job.current = String::new();
            job.adopted_added = added;
            job.adopted_cleared = cleared;
            job.adopted = records;
        }
    }

    /* ★ 并入**跨重启台账**（与作业槽的分工见 [`ledger_snapshot`]）：
    作业槽只回答「本轮跑到哪」，而账号页要的是「这些格最近一次怎么样」——
    不落盘的话重启即空，两处「同步」就只剩本次会话。 */
    record_ledger(&target.scope(), &cells);
}

/// 探一格（WB）：**单账号、单次、零重试、有硬超时**。
///
/// 判据一律复用 [`classify_event`]（全仓唯一 WB 限流判据），只做「事件 → 单元格状态」的映射。
async fn probe_cell_wb(state: &GatewayState, region: Region, uid: &str, model: &str) -> ProbeCell {
    let lookup = account::find_account_for(region, uid);
    let nickname = lookup
        .as_ref()
        .map(account::account_display_name)
        .unwrap_or_default();
    let skipped = |reason: &str| ProbeCell {
        uid: uid.to_string(),
        nickname: nickname.clone(),
        model: model.to_string(),
        state: ProbeState::Skipped,
        http_status: 0,
        reset_at_ms: None,
        reason: reason.to_string(),
        latency_ms: 0,
    };

    let Some(acc) = lookup else {
        return skipped("账号不在库里");
    };
    // ① 信封凭据送进网关会被折成**空 `Bearer`** ⇒ 上游必回 401，那是假阴性。
    if account::is_envelope(&acc, "access_token") {
        return skipped("凭据是加密信封，无法直接调用");
    }
    // ② 空 / 缺失 token 同样会发出 `Authorization: Bearer `（空）—— 方案 §8.6 的红线。
    //    真实流量那条路会因选号失败提前退出，探测是自己造请求，必须在这里拦。
    let token = account::get_str(&acc, "access_token").unwrap_or_default();
    if token.trim().is_empty() {
        return skipped("没有可用的 access_token");
    }

    let options = state.outbound_options().await;
    let raw = json!({
        "model": model,
        "stream": true,
        "max_tokens": 1,
        "messages": [{ "role": "user", "content": PROBE_TEXT }],
    })
    .to_string();
    let body = outbound::prepare_outbound_body(
        region,
        &raw,
        &options,
        &OutboundMeta {
            uid: uid.to_string(),
            conversation_id: None,
        },
        false,
    );

    let context = ConversationContext::new(None, None, None, None, None);
    let mut extra = session_headers::conversation_headers(&context, &session_headers::new_hex_id());
    extra.extend(session_headers::device_headers(uid));

    let started = Instant::now();
    // ★ 单格硬超时：宿主客户端的总超时是 600s，对一格太长（一格挂住会拖死整轮）。
    let result = tokio::time::timeout(
        Duration::from_secs(PROBE_CELL_TIMEOUT_SECS),
        state
            .upstream
            .chat_stream_with(region, &acc, &body, None, Some(&extra)),
    )
    .await;
    let latency_ms = started.elapsed().as_millis() as i64;

    let result = match result {
        Ok(inner) => inner,
        Err(_elapsed) => {
            return ProbeCell {
                uid: uid.to_string(),
                nickname,
                model: model.to_string(),
                state: ProbeState::TransportError,
                http_status: 0,
                reset_at_ms: None,
                reason: format!("单格超时（{}s）", PROBE_CELL_TIMEOUT_SECS),
                latency_ms,
            }
        }
    };

    match result {
        // 2xx ⇒ **立刻丢弃响应体**（不聚合、不落账本），只留状态与耗时。
        UpstreamChatResult::Ok(_) => ProbeCell {
            uid: uid.to_string(),
            nickname,
            model: model.to_string(),
            state: ProbeState::Ok,
            http_status: 200,
            reset_at_ms: None,
            reason: String::new(),
            latency_ms,
        },
        UpstreamChatResult::Err {
            status, message, ..
        } => {
            if status == 0 {
                return ProbeCell {
                    uid: uid.to_string(),
                    nickname,
                    model: model.to_string(),
                    state: ProbeState::TransportError,
                    http_status: 0,
                    reset_at_ms: None,
                    reason: truncate_with_marker(&message, PROBE_REASON_LIMIT),
                    latency_ms,
                };
            }
            let event = classify_event(status, &message);
            let state_key = probe_state_of(&event);
            let reset_at_ms = match &event {
                UpstreamEvent::SoftRate { reset_at_ms, .. } => *reset_at_ms,
                _ => None,
            };
            ProbeCell {
                uid: uid.to_string(),
                nickname,
                model: model.to_string(),
                state: state_key,
                http_status: status,
                reset_at_ms,
                reason: truncate_with_marker(&message, PROBE_REASON_LIMIT),
                latency_ms,
            }
        }
    }
}

/// 探一格（Qoder）：**单账号、单次、零重试、有硬超时**。
///
/// ## 判据零新造
///
/// Qoder 的判据是 **HTTP 状态码 + body**（[`probe_state_of_qoder`]）：状态码给大类，
/// body 区分「排队（`10605`）」与「凭据失效」—— 只看状态码会把 403 排队误判成
/// 「登录失效」（`pool::classify_http_with_detail` 正是为此存在）。
///
/// ## 发请求：单账号、单次、零重试（本轮最难的一处）
///
/// 用 [`chat_once_for_account`]（从 `qoder::routes::chat_for_region` 抽出的**单账号单次**入口）——
/// **绝不**用 `chat_for_region` 本身（它带 `max_rotate` 换号轮换，会把结果归因到别的账号）。
/// 请求体走 Qoder 现成构造：`build_chat_body` + `qoder_encode` + `cosy::shared_cache`。
///
/// ## ★ 2xx **不等于**成功（2026-10-22 实测修掉的一个假阴性）
///
/// 上游的**排队**信号是 **HTTP 200 + SSE 信封里的错误帧**
/// （`statusCodeValue:403` + `10605/isQueued`）⇒ 只看 HTTP 状态会把「排队」报成
/// 「全部可用（上游 200）」，而真实流量拿到的是排队错误。
/// 故 2xx 时**读一次流**（[`sse::first_upstream_error`]），只为取那**第一帧错误**：
/// - 找到 ⇒ 按 `(状态码, 详情)` 判档（排队 ⇒ `upstream_queued`）；
/// - 没找到 ⇒ 真的可用（流被读完即丢弃，不聚合、不落账本）。
///
/// 代价可控：探测请求是 `max_tokens:1` + 短文案，流极短。
async fn probe_cell_qoder(
    state: &QoderGatewayState,
    region: QoderRegion,
    uid: &str,
    model: &str,
) -> ProbeCell {
    let entry = {
        let pools = state.pools.lock().await;
        pools
            .get(&region)
            .and_then(|pool| pool.entries().find(|entry| entry.uid == uid).cloned())
    };
    let nickname = entry
        .as_ref()
        .map(|entry| entry.name.clone())
        .unwrap_or_default();
    let skipped = |reason: &str| ProbeCell {
        uid: uid.to_string(),
        nickname: nickname.clone(),
        model: model.to_string(),
        state: ProbeState::Skipped,
        http_status: 0,
        reset_at_ms: None,
        reason: reason.to_string(),
        latency_ms: 0,
    };
    let Some(entry) = entry else {
        return skipped("账号不在库里");
    };
    // Qoder 无「加密信封」概念：PAT 不能推理 ⇒ 池里 `has_access() == false`（与选号同判据）。
    if !entry.has_access() {
        return skipped("没有可调用的访问令牌（PAT 只能用于非推理链路）");
    }

    // 模型元数据：清单里查不到 ⇒ 中性合成（与中继 `payload::meta_for` 同源）。
    let catalog = state.cached_catalog_models(region).await;
    let meta = payload::meta_for(&catalog, model, state.max_context_now());

    // 最小请求体（`max_tokens: 1` + 探测文案）：与中继同一套构造。
    let payload_value = json!({
        "model": model,
        "stream": true,
        "max_tokens": 1,
        "messages": [{ "role": "user", "content": PROBE_TEXT }],
    });
    let encoded = match qoder_api::build_chat_body(&payload_value, &meta, region, "") {
        Ok(body_value) => cosy::qoder_encode(body_value.to_string().as_bytes()),
        Err(error) => {
            return skipped(&format!("请求体构造失败：{error}"));
        }
    };

    let account_input = cosy::CosyAccountInput {
        uid: &entry.uid,
        nickname: &entry.name,
        access_token: &entry.access_token,
        refresh_token: &entry.refresh_token,
        user_type: None,
        org_id: None,
        org_name: None,
    };
    let session = match cosy::shared_cache().get(&account_input) {
        Ok(session) => session,
        Err(error) => return skipped(&format!("签名会话创建失败：{error}")),
    };
    let headers = session.headers(&encoded, PATH_CHAT_SSE, Some(&meta.key), true);

    let started = Instant::now();
    // ★ 单格硬超时（宿主客户端的总超时是 600s，对一格太长）。
    //   **超时必须罩住「发请求 + 读流」两段** —— 只罩发请求的话，读流卡住会拖死整轮。
    let result = tokio::time::timeout(Duration::from_secs(PROBE_CELL_TIMEOUT_SECS), async {
        let response = chat_once_for_account(state, region, &encoded, &headers).await?;
        let http_status = response.status().as_u16();
        if !response.status().is_success() {
            // 非 2xx：错误体不是 SSE 流，读一次即可（判档用 HTTP 状态码 + 错误体）。
            let detail = response.text().await.unwrap_or_default();
            return Ok::<_, String>((http_status, Some((http_status, detail))));
        }
        // ★ 2xx **不等于**成功：上游的排队信号是「HTTP 200 + 信封里的错误帧」
        //   （`statusCodeValue:403` + `10605/isQueued`）。
        //   读一次流只为找那**第一帧错误**（`sse::first_upstream_error`）。
        //   ⚠️ 判档用**信封里的状态码**（403/429…），不是 HTTP 的 200 ——
        //   否则排队（403 信封）会落进「上游异常」而不是「上游排队」。
        let found = sse::first_upstream_error(response.bytes_stream()).await;
        Ok((http_status, found))
    })
    .await;
    let latency_ms = started.elapsed().as_millis() as i64;

    let (http_status, detail) = match result {
        Ok(Ok(parts)) => parts,
        Ok(Err(transport_error)) => {
            return ProbeCell {
                uid: uid.to_string(),
                nickname,
                model: model.to_string(),
                state: ProbeState::TransportError,
                http_status: 0,
                reset_at_ms: None,
                reason: truncate_with_marker(&transport_error, PROBE_REASON_LIMIT),
                latency_ms,
            };
        }
        Err(_elapsed) => {
            return ProbeCell {
                uid: uid.to_string(),
                nickname,
                model: model.to_string(),
                state: ProbeState::TransportError,
                http_status: 0,
                reset_at_ms: None,
                reason: format!("单格超时（{}s）", PROBE_CELL_TIMEOUT_SECS),
                latency_ms,
            };
        }
    };

    match detail {
        // 流里没有错误帧 ⇒ 真的可用（正文被丢弃，不聚合、不落账本）。
        None => ProbeCell {
            uid: uid.to_string(),
            nickname,
            model: model.to_string(),
            state: ProbeState::Ok,
            http_status,
            reset_at_ms: None,
            reason: String::new(),
            latency_ms,
        },
        // 有错误帧 ⇒ 按 `(状态码, 详情)` 判档（排队 ⇒ `upstream_queued`）。
        Some((judge_status, detail)) => {
            let detail = truncate_with_marker(&detail, PROBE_REASON_LIMIT);
            ProbeCell {
                uid: uid.to_string(),
                nickname,
                model: model.to_string(),
                state: probe_state_of_qoder(judge_status, &detail),
                http_status,
                reset_at_ms: None,
                reason: detail,
                latency_ms,
            }
        }
    }
}

/// 探一格（Trae）：**单账号、单次、零重试、有硬超时、不写池**。
///
/// ## 判据：Trae 有**两套**分类入口，都要用（`probe_state_of_trae` 统一映射）
///
/// - HTTP **非 2xx**（含传输层 `status == 0`）⇒ `trae::pool::classify_http`；
/// - HTTP **2xx 但流内 `event:error`** ⇒ `trae::pool::classify_solo`。
///
/// ⚠️ **不能只看状态码**：Trae 上游把**业务错误放在 SSE 的 `event:error` 里，此时 HTTP
/// 仍是 200**。方案 §1.2 的 Trae 列明确把 `4008` / 含 `quota`·`exceeded`·`rate` 计入限流
/// —— 那些**只**出现在 SSE 错误事件里 ⇒ 若「2xx 即丢响应体」，SSE 级限流会被**误报成 `ok`**。
/// 故这里对 2xx **读一次**（`max_tokens: 1` ⇒ 极短）只为取出该事件，随后**丢弃**正文
/// （不聚合、不落账本、不写日志）。
///
/// ## 发请求：单账号、单次、零重试（本轮最难的一处）
///
/// 用 [`trae_chat_once_for_account`]（从 `trae::routes::chat_for_variant` 的换号循环里抽出的
/// **单账号单次**入口）—— **绝不**用 `chat_for_variant` / `attempt_once` 本身（它们带
/// `max_rotate` 换号轮换，且会把失败**写回池冷却**）。请求体走 Trae 现成构造
/// （`prepare_llm_chat_body`，内含 `TRAE_LLM_CHAT_PATH` 那套字段）。
///
/// ## 探测**不写池**（方案红线 2）
///
/// 这里只调 `chat_once_for_account`（纯出站）+ `sse::aggregate`（只读流），**不**调
/// `TraePool::apply_error` / `note_success` —— 探测自己把号打成冷却 = 自我实现的限流。
async fn probe_cell_trae(
    state: &TraeGatewayState,
    variant: TraeVariant,
    uid: &str,
    model: &str,
) -> ProbeCell {
    // 按 uid **直取**（不选号）：`pick` 在账号不可用时**会换一个号**，探测的「一格」
    // 必须只打这一个 uid。
    let account = {
        let mut pools = state.pools.lock().await;
        let pool = pools
            .entry(variant)
            .or_insert_with(|| TraePool::for_variant(variant));
        pool.sync_for(variant);
        pool.account_for(uid)
    };
    let nickname = account
        .as_ref()
        .map(|entry| entry.name.clone())
        .unwrap_or_default();
    let skipped = |reason: &str| ProbeCell {
        uid: uid.to_string(),
        nickname: nickname.clone(),
        model: model.to_string(),
        state: ProbeState::Skipped,
        http_status: 0,
        reset_at_ms: None,
        reason: reason.to_string(),
        latency_ms: 0,
    };
    let Some(account) = account else {
        return skipped("账号不在池里（无可用 JWT）");
    };
    // `sync_for` 已滤掉空 JWT，这里再拦一次（方案 §3.2：绝不发空凭据）。
    if account.jwt.trim().is_empty() {
        return skipped("没有可用的 JWT");
    }

    // 最小请求体（`max_tokens: 1` + 探测文案）：与中继同一套构造（`prepare_llm_chat_body`）。
    let raw = json!({
        "model": model,
        "stream": true,
        "max_tokens": 1,
        "messages": [{ "role": "user", "content": PROBE_TEXT }],
    });
    let raw = serde_json::to_vec(&raw).unwrap_or_default();

    let started = Instant::now();
    // ★ 单格硬超时：**发请求 + 读流都算在内** —— Trae 的业务错误在 SSE 里，必须读到
    //   `event:error` 或流结束；只把「拿响应头」纳入超时的话，上游中途挂住会永久卡住整轮。
    let outcome = tokio::time::timeout(Duration::from_secs(PROBE_CELL_TIMEOUT_SECS), async {
        // 探测不是客户端流量：不落抓取（`None`），否则会把号池探测混进排查材料里。
        let response =
            trae_chat_once_for_account(state, &account, variant, model, &raw, None).await?;
        let status = response.status().as_u16();
        // ★ HTTP 2xx **不代表成功**：Trae 上游把业务错误放在 SSE 的 `event:error` 里。
        //   读一次（`max_tokens: 1` ⇒ 极短）只为取出该事件；正文**立刻丢弃**（不聚合、不落账本）。
        let (_payload, error, _usage) = trae_sse::aggregate(response, "", model, None).await;
        Ok::<(u16, Option<(i64, String)>), (u16, String)>((status, error))
    })
    .await;
    let latency_ms = started.elapsed().as_millis() as i64;

    let (status, error) = match outcome {
        Ok(Ok(parts)) => parts,
        Ok(Err((status, detail))) => {
            // `status == 0` 是传输层失败（连不上）；其余是 HTTP 非 2xx。
            let state_key = if status == 0 {
                ProbeState::TransportError
            } else {
                probe_state_of_trae(trae_classify_http(status, &detail))
            };
            return ProbeCell {
                uid: uid.to_string(),
                nickname,
                model: model.to_string(),
                state: state_key,
                http_status: status,
                reset_at_ms: None,
                reason: truncate_with_marker(&detail, PROBE_REASON_LIMIT),
                latency_ms,
            };
        }
        Err(_elapsed) => {
            return ProbeCell {
                uid: uid.to_string(),
                nickname,
                model: model.to_string(),
                state: ProbeState::TransportError,
                http_status: 0,
                reset_at_ms: None,
                reason: format!("单格超时（{}s）", PROBE_CELL_TIMEOUT_SECS),
                latency_ms,
            };
        }
    };

    if let Some((code, message)) = error {
        return ProbeCell {
            uid: uid.to_string(),
            nickname,
            model: model.to_string(),
            state: probe_state_of_trae(trae_classify_solo(code, &message)),
            http_status: status,
            reset_at_ms: None,
            reason: truncate_with_marker(&message, PROBE_REASON_LIMIT),
            latency_ms,
        };
    }
    ProbeCell {
        uid: uid.to_string(),
        nickname,
        model: model.to_string(),
        state: ProbeState::Ok,
        http_status: status,
        reset_at_ms: None,
        reason: String::new(),
        latency_ms,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 串行化「碰进程级静态」的用例（作业槽 / 取消位 / 作业位）。
    ///
    /// ⚠️ 实测 flake（1/8 概率）：`status_wire_shape_is_stable` 与
    /// `global_flags_round_trip_and_reset` 并行跑时会互相 `reset_for_test()` /
    /// `set_job_for_test()` ⇒「cancel() 仍应置取消位」「作业在跑时不得释放」随机红。
    /// 这两条都必须先拿这把锁 —— 全局状态 + 并行测试只能靠串行化，不靠"小心"。
    static GLOBAL_STATE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// 拿全局状态锁（被毒化也继续：断言失败的信息比中毒更重要）。
    fn lock_global_state() -> std::sync::MutexGuard<'static, ()> {
        GLOBAL_STATE_LOCK
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
    }

    fn soft(model_scoped: bool) -> UpstreamEvent {
        UpstreamEvent::SoftRate {
            reset_at_ms: Some(1_700_000_000_000),
            model_scoped,
        }
    }

    fn job_of(phase: ProbePhase, cells: Vec<ProbeCell>) -> ProbeJob {
        ProbeJob {
            scope: "wb-cn".to_string(),
            phase,
            started_at_ms: 1_700_000_000_000,
            finished_at_ms: None,
            total: cells.len(),
            done: cells.len(),
            current: String::new(),
            truncated: false,
            adopt: false,
            adopted_added: 0,
            adopted_cleared: 0,
            adopted: Vec::new(),
            cells,
            plan: Vec::new(),
        }
    }

    /// ★ 前三档才是「限流」；402 积分不足与登录失效必须**分列**。
    ///
    /// 变异（QA 已实测）：把 `HardCredit` 也映射成 `RateLimitedModel` ⇒ 本用例红
    /// （那正是「把没积分报成限流、用户去傻等解锁」的坑，`client_limits` 已踩过）。
    #[test]
    fn only_three_states_count_as_rate_limited() {
        assert_eq!(probe_state_of(&soft(true)), ProbeState::RateLimitedModel);
        assert_eq!(probe_state_of(&soft(false)), ProbeState::RateLimitedAccount);
        assert_eq!(
            probe_state_of(&UpstreamEvent::ModelBlocked),
            ProbeState::ModelBlocked
        );

        assert_eq!(
            probe_state_of(&UpstreamEvent::HardCredit),
            ProbeState::NoCredits
        );
        assert_eq!(
            probe_state_of(&UpstreamEvent::SessionDead),
            ProbeState::AuthDead
        );
        // issue #100：401/403 也是「凭据不可用」，必须落 `auth_dead` 而不是
        // 与「上游抖动」同档 —— 两者对用户的可行动指引完全不同。
        assert_eq!(
            probe_state_of(&UpstreamEvent::AuthRejected),
            ProbeState::AuthDead
        );
        assert_eq!(
            probe_state_of(&UpstreamEvent::NotFound),
            ProbeState::ModelUnavailable
        );
        assert_eq!(
            probe_state_of(&UpstreamEvent::Server),
            ProbeState::UpstreamError
        );
        assert_eq!(
            probe_state_of(&UpstreamEvent::Client),
            ProbeState::UpstreamError
        );
    }

    /// 模型级与账号级**不能混**（判反了会让「换模型可用」的用户白等）。
    #[test]
    fn model_scoped_flag_decides_model_vs_account() {
        assert_ne!(
            probe_state_of(&soft(true)),
            probe_state_of(&soft(false)),
            "模型级与账号级必须是两个不同的格"
        );
    }

    /// 采纳语义：只有三档限流 + `ok` 会动冷却，其余一律 `Other`。
    #[test]
    fn only_limit_states_and_ok_touch_cooling() {
        assert_eq!(
            probe_outcome_of(ProbeState::Ok, None),
            ProbeOutcome::Available
        );
        assert!(matches!(
            probe_outcome_of(ProbeState::RateLimitedModel, Some(1)),
            ProbeOutcome::ModelRateLimited {
                reset_at_ms: Some(1)
            }
        ));
        assert_eq!(
            probe_outcome_of(ProbeState::ModelBlocked, None),
            ProbeOutcome::ModelBlocked
        );
        assert!(matches!(
            probe_outcome_of(ProbeState::RateLimitedAccount, None),
            ProbeOutcome::AccountRateLimited { reset_at_ms: None }
        ));
        for state in [
            ProbeState::NoCredits,
            ProbeState::AuthDead,
            ProbeState::ModelUnavailable,
            ProbeState::UpstreamError,
            ProbeState::TransportError,
            ProbeState::Skipped,
            ProbeState::UpstreamQueued,
        ] {
            assert_eq!(
                probe_outcome_of(state, None),
                ProbeOutcome::Other,
                "{state:?} 不得动冷却 —— 402/登录失效/网络错误/排队都不是「限流」"
            );
        }
    }

    /// ★ Qoder 判据映射：唯一判据是 **状态码 + body**（body 用来把「排队」与「凭据死」分开）。
    ///
    /// 变异①：把 429 映射成 `RateLimitedAccount` ⇒ 第一条断言红（Qoder 上游本来就按模型报，无账号级细分）。
    /// 变异②：把 404 也归 `UpstreamError` ⇒ 404 那条断言红（「该账号看不到这个模型」≠ 上游错误）。
    /// 变异③：把「排队（403 + `10605`）」也映射成 `AuthDead` ⇒ 排队那两条断言红
    ///   （2026-10-22 真机踩到：排队裹在 403 信封里，只看状态码会把好账号判成登录失效）。
    #[test]
    fn qoder_status_maps_to_probe_state() {
        assert_eq!(probe_state_of_qoder(429, ""), ProbeState::RateLimitedModel);
        assert_eq!(probe_state_of_qoder(401, ""), ProbeState::AuthDead);
        assert_eq!(probe_state_of_qoder(403, ""), ProbeState::AuthDead);
        assert_eq!(probe_state_of_qoder(404, ""), ProbeState::ModelUnavailable);
        assert_eq!(probe_state_of_qoder(400, ""), ProbeState::UpstreamError);
        assert_eq!(probe_state_of_qoder(500, ""), ProbeState::UpstreamError);
        assert_eq!(probe_state_of_qoder(503, ""), ProbeState::UpstreamError);
        // 429 与 401/403 必须落在**不同**档（限流 ≠ 凭据死）。
        assert_ne!(probe_state_of_qoder(429, ""), probe_state_of_qoder(401, ""));
        // 模型不可见与上游错误必须分列。
        assert_ne!(probe_state_of_qoder(404, ""), probe_state_of_qoder(500, ""));
        // ★ 2026-10-22：detail 参与判定 —— Qoder 的**排队**是「HTTP 200 + 信封 403」，
        //   只看状态码会把它判成「可用」（假阴性，真机踩过）。空 detail 与排队 detail
        //   在同一状态码下必须落在**不同**档。
        assert_eq!(
            probe_state_of_qoder(403, "{\"statusCodeValue\":403,\"isQueued\":true}"),
            ProbeState::UpstreamQueued,
            "排队 ⇒ 上游排队（独立成档：不是限流、不是凭据死）"
        );
        assert_eq!(
            probe_state_of_qoder(403, "{\"code\":\"10605\",\"isQueued\":true}"),
            ProbeState::UpstreamQueued,
            "排队（10605）⇒ 上游排队"
        );
        assert_ne!(
            probe_state_of_qoder(403, ""),
            probe_state_of_qoder(403, "isQueued"),
            "detail 必须真的参与判定"
        );
        // ★ 排队**不得**落「凭据失效」：账号好着，只是这个模型在排队。
        assert_ne!(
            probe_state_of_qoder(403, "isQueued"),
            ProbeState::AuthDead,
            "排队不是凭据失效（否则用户去傻等重新登录）"
        );
        // ★ 排队**不得**落「限流」三档：等 30 秒就好，不是被限流。
        assert_ne!(
            probe_state_of_qoder(403, "isQueued"),
            ProbeState::RateLimitedModel,
            "排队不是限流（否则用户去傻等解锁）"
        );
    }

    /// `target_for_scope` **fail-closed**：未知 / 空 / 拼错一律 `Err`（可读文案，列出合法取值）。
    ///
    /// 只测 `Err` 分支（不构造真实 state，保持测试无副作用）；合法作用域的接线另见
    /// [`target_for_scope_wires_trae`] / [`trae_region_maps_to_current_variant`]。
    #[test]
    fn target_for_scope_is_fail_closed() {
        for bad in ["", "wb", "trae-glboal", "WB-CN", "qoder-cn ", "wb-cn-x"] {
            let error = target_for_scope(bad).unwrap_err();
            assert!(error.contains("wb-cn"), "{error}");
            assert!(error.contains("qoder-global"), "{error}");
        }
    }

    /// ★ Trae 已接线：`trae-cn` / `trae-global` 必须能构造出探测目标（不再是「未实现」）。
    ///
    /// 变异：把 `target_for_scope` 的 `Product::Trae` 分支改回 `Err(...)` ⇒ 本用例红。
    #[test]
    fn target_for_scope_wires_trae() {
        for scope in ["trae-cn", "trae-global"] {
            let target = target_for_scope(scope).unwrap_or_else(|error| panic!("{scope}: {error}"));
            assert_eq!(target.scope(), scope);
        }
    }

    /// Trae 区域 → 变体（纯函数）：CN → 合并版、国际 → 国际合并版。
    ///
    /// 与 `TraeVariant::parse` 的**区域标识**口径必须一致（前端 `regionPrograms` 同源）——
    /// 否则探测读到的客户端清单 / `function` 白名单会与前端展示的程序位对不上。
    #[test]
    fn trae_region_maps_to_current_variant() {
        assert_eq!(trae_variant_of(TraeRegion::Cn), TraeVariant::TraeMerged);
        assert_eq!(
            trae_variant_of(TraeRegion::Global),
            TraeVariant::GlobalTraeMerged
        );
        // ⚠️ 刻意**不**断言 `== TraeVariant::parse("cn")`。
        //
        // `parse` 的口径正在被 issue #28 那笔改动从「遗留 `TraeWork`」迁到「合并版
        // `TraeMerged`」，而那是**另一笔**改动。把本用例绑上去，会让**本提交依赖那笔
        // 未落地的改动** —— 实测：在只有本提交的树上，`parse("cn")` 仍是 `TraeWork`，
        // 于是断言红（`left: TraeMerged / right: TraeWork`）。
        //
        // 本模块的契约只有一条：**区域 → 合并版变体**（上面两条断言），不依赖 `parse`。
    }

    /// ★ Trae 判据映射：复用 `trae::pool` 的两个分类入口，全枚举覆盖。
    ///
    /// 变异①：把 `PlanLimit` 也映射成 `RateLimitedModel` ⇒ 第二条断言红
    ///   （那正是「把额度用尽报成限流、用户去傻等解锁」的坑）。
    /// 变异②：把 `SoftRate` 映射成 `RateLimitedAccount` ⇒ 首条断言红
    ///   （Trae **无法细分**账号级 ⇒ 一律模型级）。
    #[test]
    fn trae_err_kind_maps_to_probe_state() {
        assert_eq!(
            probe_state_of_trae(TraeErrKind::SoftRate),
            ProbeState::RateLimitedModel
        );
        assert_eq!(
            probe_state_of_trae(TraeErrKind::PlanLimit),
            ProbeState::NoCredits
        );
        assert_eq!(
            probe_state_of_trae(TraeErrKind::SessionDead),
            ProbeState::AuthDead
        );
        assert_eq!(
            probe_state_of_trae(TraeErrKind::NotFound),
            ProbeState::ModelUnavailable
        );
        assert_eq!(
            probe_state_of_trae(TraeErrKind::Server),
            ProbeState::UpstreamError
        );
        assert_eq!(
            probe_state_of_trae(TraeErrKind::Client),
            ProbeState::UpstreamError
        );
        assert_eq!(
            probe_state_of_trae(TraeErrKind::Business),
            ProbeState::UpstreamError
        );
        assert_eq!(
            probe_state_of_trae(TraeErrKind::None),
            ProbeState::UpstreamError
        );

        // ★ 红线：PlanLimit（套餐额度用尽）**不是限流**，必须与 SoftRate 分列。
        assert_ne!(
            probe_state_of_trae(TraeErrKind::PlanLimit),
            probe_state_of_trae(TraeErrKind::SoftRate),
            "额度用尽与限流必须分列（否则用户去傻等解锁）"
        );
        // Trae **无**账号级 / 模型封禁概念 ⇒ 全枚举都不得落这两档。
        for kind in [
            TraeErrKind::SoftRate,
            TraeErrKind::PlanLimit,
            TraeErrKind::SessionDead,
            TraeErrKind::NotFound,
            TraeErrKind::Server,
            TraeErrKind::Client,
            TraeErrKind::Business,
            TraeErrKind::None,
        ] {
            let state = probe_state_of_trae(kind);
            assert_ne!(
                state,
                ProbeState::RateLimitedAccount,
                "{kind:?} 不得落账号级"
            );
            assert_ne!(state, ProbeState::ModelBlocked, "{kind:?} 不得落模型封禁");
        }
    }

    /// 序列化名必须与前端/原型**逐字一致**（改名会让前端整列变「未知」）。
    #[test]
    fn state_names_are_the_wire_contract() {
        let names: Vec<String> = [
            ProbeState::Ok,
            ProbeState::RateLimitedModel,
            ProbeState::RateLimitedAccount,
            ProbeState::ModelBlocked,
            ProbeState::NoCredits,
            ProbeState::AuthDead,
            ProbeState::ModelUnavailable,
            ProbeState::UpstreamError,
            ProbeState::UpstreamQueued,
            ProbeState::TransportError,
            ProbeState::Skipped,
        ]
        .into_iter()
        .map(|state| {
            serde_json::to_value(state)
                .unwrap()
                .as_str()
                .unwrap()
                .to_string()
        })
        .collect();
        assert_eq!(
            names,
            vec![
                "ok",
                "rate_limited_model",
                "rate_limited_account",
                "model_blocked",
                "no_credits",
                "auth_dead",
                "model_unavailable",
                "upstream_error",
                "upstream_queued",
                "transport_error",
                "skipped",
            ]
        );
    }

    /// 去重保序：重复/空白项被剔除，**顺序不变**（否则矩阵列序会乱跳）。
    #[test]
    fn dedup_keeps_order_and_drops_blanks() {
        let out = dedup_keep_order(vec![
            "b".to_string(),
            " a ".to_string(),
            "b".to_string(),
            "".to_string(),
            "  ".to_string(),
            "c".to_string(),
        ]);
        assert_eq!(out, vec!["b".to_string(), "a".to_string(), "c".to_string()]);
    }

    /// 清单**账号为外层**（矩阵行序），且在上限处截断并**如实报告** `truncated`。
    ///
    /// 变异：把外层换成 `models` ⇒ 顺序断言红；把截断去掉 ⇒ `truncated` 断言红。
    #[test]
    fn build_plan_is_account_major_and_truncates_loudly() {
        let accounts: Vec<String> = (0..30).map(|i| format!("u{i}")).collect();
        let models: Vec<String> = (0..20).map(|i| format!("m{i}")).collect();
        let (plan, truncated) = build_plan(&accounts, &models);
        assert_eq!(plan.len(), MAX_CELLS, "必须在上限处截断");
        assert!(truncated, "被截断必须如实报出，不能静默丢");
        assert_eq!(plan[0], ("u0".to_string(), "m0".to_string()));
        assert_eq!(plan[1], ("u0".to_string(), "m1".to_string()), "外层是账号");
        assert_eq!(plan[20], ("u1".to_string(), "m0".to_string()));

        let (small, truncated) = build_plan(&["u0".to_string()], &["m0".to_string()]);
        assert_eq!(small.len(), 1);
        assert!(!truncated, "没超上限就不能报截断");
    }

    /// 显式格清单：**去重保序 + trim + 丢空**。
    ///
    /// 变异：去掉去重 ⇒ 重复项断言红；去掉 trim ⇒ 首项断言红（`probe_ledger` 按 `(uid, model)`
    /// 定位，带空格就永远对不上）。
    #[test]
    fn explicit_pairs_are_trimmed_and_deduped() {
        let pairs = dedup_pairs(vec![
            (" u1 ".to_string(), "m1".to_string()),
            ("u1".to_string(), "m1".to_string()), // 重复
            (String::new(), "m1".to_string()),    // 空 uid
            ("u2".to_string(), "  ".to_string()), // 空 model
            ("u2".to_string(), "m1".to_string()),
        ]);
        assert_eq!(
            pairs,
            vec![
                ("u1".to_string(), "m1".to_string()),
                ("u2".to_string(), "m1".to_string()),
            ]
        );
    }

    /// 台账**落盘 round-trip**：写进去 → 读回来形状不变；损坏文件回落空表（不 panic）。
    ///
    /// 变异：让 `load_ledger_from` 在解析失败时 `unwrap()` ⇒ 损坏文件那条断言 panic；
    /// 把 `save_ledger_to` 写成 `fs::write`（非原子）不会红，但会失去「临时文件 + rename」
    /// 的抗半写能力 —— 故这条只守「能读回」与「坏文件不炸」。
    #[test]
    fn ledger_round_trips_through_disk() {
        let dir = std::env::temp_dir().join(format!("bs-probe-ledger-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("probe_results.json");

        let mut store = LedgerFile::default();
        store.merge(
            "wb-cn",
            &[test_cell("u1", "m1", ProbeState::RateLimitedModel)],
        );
        save_ledger_to(&path, &store).unwrap();

        let back = load_ledger_from(&path);
        let cells = back.flat("wb-cn");
        assert_eq!(cells.len(), 1);
        assert_eq!(cells[0].state, ProbeState::RateLimitedModel);
        assert_eq!(cells[0].uid, "u1");
        assert_eq!(cells[0].model, "m1");

        // 损坏文件：回落空表（不能因为一个坏文件让命令面报错）。
        std::fs::write(&path, "{ not json").unwrap();
        assert!(load_ledger_from(&path).flat("wb-cn").is_empty());

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 台账：**同格覆盖、异格保留、作用域隔离**。
    ///
    /// 变异：把 `merge` 改成「先清空再插入」⇒ 第二条「第二轮只剩 1 格」断言红；
    /// 去掉作用域键 ⇒ 隔离断言红。
    #[test]
    fn ledger_merges_without_erasing_and_keeps_scopes_apart() {
        let mut ledger = LedgerFile::default();
        assert!(ledger.merge("wb-cn", &[test_cell("u1", "m1", ProbeState::Ok)]));
        // 第二轮只探一格：**不得**抹掉第一格（账号页逐行实测就靠这条累积）。
        ledger.merge(
            "wb-cn",
            &[test_cell("u1", "m2", ProbeState::RateLimitedModel)],
        );
        assert_eq!(
            ledger.flat("wb-cn").len(),
            2,
            "第二轮只探 1 格，却丢了第一格"
        );
        // 同格覆盖（同一格再测出限流 ⇒ 以新结论为准）。
        ledger.merge(
            "wb-cn",
            &[test_cell("u1", "m1", ProbeState::RateLimitedModel)],
        );
        let cell = ledger
            .flat("wb-cn")
            .into_iter()
            .find(|cell| cell.uid == "u1" && cell.model == "m1")
            .expect("该格必须还在");
        assert_eq!(cell.state, ProbeState::RateLimitedModel);
        // 完全相同的格 ⇒ 不算变化（不写盘）。
        assert!(
            !ledger.merge(
                "wb-cn",
                &[test_cell("u1", "m1", ProbeState::RateLimitedModel)]
            ),
            "同值合并不该报告变化（会造出无意义的写盘）"
        );
        // 作用域隔离：wb-cn 的格不能出现在 wb-global。
        assert!(ledger.flat("wb-global").is_empty());
        assert!(ledger.scopes.get("wb-global").is_none());
        // 确定性排序（uid → model）：同一份数据两次读出来必须一致。
        let first: Vec<String> = ledger.flat("wb-cn").into_iter().map(|c| c.model).collect();
        let second: Vec<String> = ledger.flat("wb-cn").into_iter().map(|c| c.model).collect();
        assert_eq!(first, second);
    }

    /// 前端要读的 wire 形状：`idle` / `running` / `done` 三种都必须**是对象且带 phase**。
    ///
    /// 变异：把 `status()` 的 idle 分支改成 `Value::Null` ⇒ red（前端 `job.phase` 直接炸）。
    #[test]
    fn status_wire_shape_is_stable() {
        let _guard = lock_global_state();
        reset_for_test();
        assert_eq!(status()["phase"], json!("idle"));

        set_job_for_test(job_of(ProbePhase::Running, Vec::new()));
        let running = status();
        assert_eq!(running["phase"], json!("running"));
        assert_eq!(running["scope"], json!("wb-cn"));
        assert!(running["total"].is_number(), "total 必须是数字");
        assert!(running["cells"].is_array());

        set_job_for_test(job_of(
            ProbePhase::Done,
            vec![test_cell("u1", "glm-5.3", ProbeState::RateLimitedModel)],
        ));
        let done = status();
        assert_eq!(done["phase"], json!("done"));
        assert_eq!(done["cells"][0]["state"], json!("rate_limited_model"));
        assert_eq!(done["cells"][0]["uid"], json!("u1"));
        // 回填明细：**必须恒为数组**（前端要展开它；`null` 会让前端直接炸）。
        assert!(done["adopted"].is_array(), "`adopted` 必须是数组");
        reset_for_test();
    }

    /// ★ 三个进程级标志（取消位 / 作业位 / 作业槽）的抢占与复位 —— **合并成一条**。
    ///
    /// ⚠️ 它们是**进程级静态**：拆成多条测试会互相 reset/cancel
    /// （QA 已指出这是真实的 flake 窗口，故此处刻意只留一条测试碰它们）。
    ///
    /// 涵盖三个修复点的回归：
    /// - **作业位原子抢占**（TOCTOU 修复）：并发第二次必须失败；
    /// - **释放后可再抢**：否则一次异常就永久起不来；
    /// - ★ **`cancel()` 兜底自愈**：没有真在跑的作业时释放作业位（覆盖「`run` panic /
    ///   宿主没 spawn `run` ⇒ 作业位永久占着，用户只能重启进程」的死角），
    ///   但作业**真在跑**时**绝不**释放（否则双作业并发写同一槽）。
    #[test]
    fn global_flags_round_trip_and_reset() {
        let _guard = lock_global_state();
        reset_for_test();
        assert!(!cancel_flag().load(Ordering::SeqCst));
        assert!(!busy_flag().load(Ordering::SeqCst));
        assert_eq!(status()["phase"], json!("idle"), "复位后应是 idle");

        // ── 作业位原子抢占（TOCTOU 修复点）──
        assert!(
            busy_flag()
                .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok(),
            "空位应能抢占"
        );
        assert!(
            busy_flag()
                .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
                .is_err(),
            "★ 已被占时第二次必须失败（TOCTOU 修复点）"
        );
        busy_flag().store(false, Ordering::SeqCst);
        assert!(
            busy_flag()
                .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok(),
            "释放后必须能再抢（否则一次异常就永久起不来）"
        );

        // ── ★ cancel() 自愈①：作业槽**为空**时，取消应顺手释放作业位 ──
        reset_for_test(); // 作业槽 = None、三个位归零
        busy_flag().store(true, Ordering::SeqCst); // 模拟「没有作业，却占着作业位」
        cancel();
        assert!(
            cancel_flag().load(Ordering::SeqCst),
            "cancel() 仍应置取消位"
        );
        assert!(
            !busy_flag().load(Ordering::SeqCst),
            "★ 无在跑作业时 cancel() 必须释放作业位（用户能自救，不必重启进程）"
        );

        // ── ★ cancel() 自愈②：作业存在但**不在 Running**（如已 done）同样释放 ──
        set_job_for_test(job_of(ProbePhase::Done, Vec::new()));
        busy_flag().store(true, Ordering::SeqCst);
        cancel();
        assert!(
            !busy_flag().load(Ordering::SeqCst),
            "★ 作业已结束（phase=done）时 cancel() 也应释放作业位"
        );

        // ── ★ cancel() 红线：作业**正在跑**（phase=running）时**绝不能**释放 ──
        set_job_for_test(job_of(ProbePhase::Running, Vec::new()));
        busy_flag().store(true, Ordering::SeqCst);
        cancel();
        assert!(
            busy_flag().load(Ordering::SeqCst),
            "★ 作业在跑时 cancel() 不得释放作业位（否则第二次 start 会与它并发写同一槽）"
        );

        // ── ★ run() 的 RAII 归位器：作业槽为空 ⇒ 走早退分支，返回时也必须释放作业位 ──
        //
        // 用**内联单线程 runtime** 跑 `run`（而不是另开一条 `#[tokio::test]`）：这三个是
        // **进程级静态**，只允许本测试碰它们；另开一条会在并行 runner 下与本条互相
        // reset/cancel —— 那正是 QA 点名的真实 flake 窗口。
        reset_for_test(); // 取消位 = false、作业槽 = None
        busy_flag().store(true, Ordering::SeqCst); // 模拟「run 已抢占作业位」
        {
            let state = crate::state::GatewayState::new(crate::GatewayConfig::default());
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_time()
                .build()
                .expect("建单线程 runtime");
            // 作业槽为空 ⇒ `run` 走早退分支（不碰 target、不发请求）。
            runtime.block_on(run(ProbeTarget::Wb(state, Region::Cn)));
        }
        assert!(
            !busy_flag().load(Ordering::SeqCst),
            "★ run() 退出（含早退 / panic 展开）必须经 RAII guard 释放作业位"
        );

        reset_for_test();
        assert!(
            !cancel_flag().load(Ordering::SeqCst),
            "复位后必须能再跑一轮"
        );
        assert!(!busy_flag().load(Ordering::SeqCst));
        assert_eq!(status()["phase"], json!("idle"));
    }

    /// ★★ **接线护栏**：两个宿主、四处的调用点都在。
    ///
    /// 形态照抄 [`crate::catalog_probe`] 的 `both_hosts_wire_the_catalog_probe` —— 本仓对
    /// 「**函数存在、类型全对、构建全绿、单测全绿，但生产上没人调用**」那类事故的标准守卫。
    ///
    /// ## 为什么必须单独有一条
    ///
    /// 命令面分散在四处（`api.ts` 的 ROUTES / `api.rs` 的路由与 handler / `commands.rs` 的实现 /
    /// `lib.rs` 的登记）。`check-api-contract.cjs` 只管「**名字**四处一致」，管不了**语义**上最要命
    /// 的那一条：**`start` 之后必须 spawn `run`** —— 少这一句，作业位（原子抢占）永远不释放，
    /// 之后每一次「开始探测」都只报「已有探测在进行中」，而**构建与门禁全绿**。
    ///
    /// ## 标记一律带**实参**（不是裸函数名）
    ///
    /// 裸 `model_probe::start` 在**定义行**（`pub async fn start(`）也会命中 ⇒ 删掉调用点后
    /// 护栏会**空转**。故断言全部带实参 / 路径 / 方法形态。
    ///
    /// ## 变异可证伪
    ///
    /// 任一处删除即红：删 `commands.rs` 的 `start(...)`、或那行 `spawn(...)`；删 `lib.rs` 里
    /// 三个 `commands::probe_*` 任一；删 `api.rs` 的三条 `.route(...)`、或把 `post` 写成 `get`、
    /// 或删那行 `spawn`。
    #[test]
    fn both_hosts_wire_every_probe_exit() {
        const TAURI_COMMANDS_RS: &str = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../src-tauri/src/commands.rs"
        );
        const TAURI_LIB_RS: &str =
            concat!(env!("CARGO_MANIFEST_DIR"), "/../../src-tauri/src/lib.rs");
        const SERVER_API_RS: &str = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../crates/buddy-switch-server/src/api.rs"
        );

        let read = |path: &str| {
            std::fs::read_to_string(path).unwrap_or_else(|error| panic!("读不到 {path}: {error}"))
        };

        // ① 桌面端：三个出口 + ★ 那条最容易漏的 `spawn(run)`。
        let desktop = read(TAURI_COMMANDS_RS);
        for (marker, why) in [
            (
                "model_probe::target_for_scope(&scope)",
                "桌面端 `probe_start` 没有按作用域构造探测目标（fail-closed 入口空转）",
            ),
            (
                "model_probe::start(&target, uids, models, cells, adopt)",
                "桌面端 `probe_start` 没有真的启动探测",
            ),
            (
                "tauri::async_runtime::spawn(buddy_switch_gateway::model_probe::run(target))",
                "★ 桌面端少了 spawn run —— 作业位会永久占住，之后每次探测都报「已有探测在进行中」",
            ),
            ("model_probe::status()", "桌面端 `probe_status` 没有接线"),
            ("model_probe::cancel()", "桌面端 `probe_cancel` 没有接线"),
            (
                "model_probe::ledger_snapshot(&scope)",
                "★ 桌面端 `probe_ledger` 没有接线 —— 账号页重启后看不到实测结论（跨重启同步空转）",
            ),
        ] {
            assert!(
                desktop.contains(marker),
                "{TAURI_COMMANDS_RS} 缺 `{marker}`（{why}）"
            );
        }

        // ② 桌面端登记表：漏登记 ⇒ invoke 报 `command not found`（而构建全绿）。
        let desktop_lib = read(TAURI_LIB_RS);
        for name in [
            "commands::probe_start",
            "commands::probe_status",
            "commands::probe_cancel",
            "commands::probe_ledger",
        ] {
            assert!(
                desktop_lib.contains(name),
                "{TAURI_LIB_RS} 的 `generate_handler!` 缺 `{name}` —— 桌面端会报 command not found"
            );
        }

        // ③ 服务端：**连通 HTTP 方法一起断言**（method 写反会打到错的 handler，且构建全绿）。
        let server = read(SERVER_API_RS);
        for marker in [
            r#".route("/api/gateway/probe/start", post(api_probe_start))"#,
            r#".route("/api/gateway/probe/status", get(api_probe_status))"#,
            r#".route("/api/gateway/probe/cancel", post(api_probe_cancel))"#,
            r#".route("/api/gateway/probe/ledger", get(api_probe_ledger))"#,
            "model_probe::target_for_scope(&scope)",
            "model_probe::start(&target, uids, models, cells, adopt)",
            "tokio::spawn(buddy_switch_gateway::model_probe::run(target))",
        ] {
            assert!(server.contains(marker), "{SERVER_API_RS} 缺 `{marker}`");
        }
    }
}
