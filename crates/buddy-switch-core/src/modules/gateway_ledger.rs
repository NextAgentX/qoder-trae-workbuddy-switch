//! Trae / Qoder 的**网关逐日账本**（WB `token_daily` 的网关部分在这两个平台上的等价物）。
//!
//! ## 为什么需要它（issue #63 的账本视图缺这两个平台）
//!
//! 账本视图要回答「这段窗口里，**网关**这本账每天多少」。WorkBuddy 的答案来自
//! [`crate::modules::token_daily`]（后台周期把 `gateway_logs.json` 增量累加到 90 天落库）。
//! Trae / Qoder 原先只有**环形日志本身**（保留 `log_keep` 条，缺省 200）——
//! 那是一个「最近几百次请求」的窗口，滚过去就没了：
//!
//! - 30 天窗口里它只能覆盖今天最近的一段，**其余日子既不是 0 也不可得**；
//! - 重启后日志仍在，但一旦窗口滚过某天，那天的量就**永远补不回来**。
//!
//! 所以这里做的不是「再读一遍日志」，而是**落库**：每轮只把 `ts > 水位线` 的新条目
//! 累加进当日桶（与 `token_daily::gateway_round` 同一套增量语义），保留 90 天。
//!
//! ## 三条硬约束（与 `token_daily` 同源，改这里之前先读）
//!
//! 1. **只读上游**：绝不改网关写路径、绝不改日志文件。
//! 2. **只增不减**：桶是累加的，不是「当前窗口快照」—— 环形裁剪滚掉一条已记账的请求时，
//!    数字不许倒退（`token_daily` 2026-10-15 的实测事故：CN 桶 10.3M → 3.6M → 18.7M）。
//! 3. **各维度合计 = 总量**：密钥 / 模型 / 账号 / 端点由**同一次扫描、同一水位线**产出，
//!    任何一边多算一条，页面上「按模型合计 = 网关总量」这条自洽约束当场破掉。
//!
//! ## 与 WB 的**刻意差异**
//!
//! | 维度 | WB `token_daily` | 本模块 |
//! |:--|:--|:--|
//! | 作用域轴 | `Region`（cn / global） | `LedgerScope`（cn / global / **unlabeled**） |
//! | 客户端桶 | 有（扫 jsonl） | **没有**：Trae 无可解析的客户端落盘，Qoder 的客户端用量另有 `qoder::usage` |
//! | 归属判定 | 行内 `region` | Trae 看 `variant`、Qoder 看 `region`（见 [`scope_of`]） |
//! | credit 字段 | `credit` | `credit` 或 `credits`（Qoder 网关写的是后者） |
//!
//! `unlabeled` 是**旧日志**的归属（Trae 早于 `variant` 字段、Qoder 早于 `region` 字段）——
//! 单列一档而不是丢掉或塞进 cn：丢掉会让「全部」小于两边之和，塞进 cn 会把国际流量
//! 记到国内账上。两条都是**伪造**。

use chrono::{Duration as ChronoDuration, Local, NaiveDate, TimeZone};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::modules::config::{atomic_write, now_ms};
use crate::modules::qoder::paths as qoder_paths;
/* ★ 「按密钥」维度的桶键判据**只留一份**（`token_daily::key_bucket_of`）：三种
「没有 Key」的区分（键不存在 / 值为 null / 带上归属的 id）必须在两条账本上逐字相同，
否则同一个请求在 WB 与 Trae 的排行里会落进不同的桶，用户看到两套口径。 */
use crate::modules::token_daily::key_bucket_of;
use crate::modules::trae::paths as trae_paths;
use crate::modules::trae::variant::TraeVariant;

/// 落盘保留天数（与 `token_daily::TOKEN_DAILY_RETENTION_DAYS` 同值：同一张账本视图要能选 90 天）。
pub const LEDGER_RETENTION_DAYS: i64 = 90;
/// 条数安全网（保留窗内仍可能出现病态 / 未来的日期，超出即丢最旧）。
pub const LEDGER_MAX_DAYS: usize = 400;
/// 存储 schema 版本。
///
/// v2（2026-10-28）：`DayRecord` 新增 `keys` 维度 + `keysPartial`（用量页「按密钥看用量」
/// 的正确数据源）。旧文件缺这两个字段 ⇒ `serde(default)` 读成空，由 `record` 的
/// **播种轮**（`seed_keys`）把当天已有用量整窗补一次，其余日期由前端披露「无该维度」。
pub const LEDGER_VERSION: u32 = 2;

/// 缺省日志保留条数（与两条网关 `log_keep` 的缺省值一致）。
const DEFAULT_LOG_KEEP: usize = 200;

/// 账本里的 `model` 为空时的哨兵（前端翻成「未标注模型」，**不丢量**）。
pub const UNKNOWN_MODEL: &str = "__unknown__";

static LEDGER_WRITE_LOCK: Mutex<()> = Mutex::new(());

// ---------------------------------------------------------------------------
// 产品与作用域
// ---------------------------------------------------------------------------

/// 有网关账本的产品。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LedgerProduct {
    Trae,
    Qoder,
}

impl LedgerProduct {
    /// 稳定标识（日志 / 诊断用）。
    pub fn as_str(self) -> &'static str {
        match self {
            LedgerProduct::Trae => "trae",
            LedgerProduct::Qoder => "qoder",
        }
    }

    /// 该产品的网关请求日志文件。
    ///
    /// ⚠️ 两份日志**格式不同但都解析得动**：Trae 是 JSON 数组；Qoder 的文件名带
    /// `.jsonl`、内容其实也是 JSON 数组（`RequestLog::save` 写的是 `to_string_pretty`）。
    /// [`load_log_entries`] 两种都认，未来真改成逐行也不会静默读空。
    fn log_file(self) -> PathBuf {
        match self {
            LedgerProduct::Trae => trae_paths::api_gateway_log_file(),
            LedgerProduct::Qoder => qoder_paths::logs_dir().join("gateway_requests.jsonl"),
        }
    }

    /// 网关配置文件（只用来读 `log_keep`，判断「今日是否可能已被裁」）。
    fn config_file(self) -> PathBuf {
        match self {
            LedgerProduct::Trae => trae_paths::api_gateway_file(),
            LedgerProduct::Qoder => qoder_paths::gateway_config_file(),
        }
    }

    /// 账本文件：`{product}/gateway_ledger[.{scope}].json`。
    ///
    /// 按 scope 分文件（与 WB 按 region 分文件同形）：一个 scope 一个写者，
    /// 落盘只在**自己那份**上做，三档之间不会互相覆盖。
    fn ledger_file(self, scope: LedgerScope) -> PathBuf {
        let dir = match self {
            LedgerProduct::Trae => trae_paths::trae_dir(),
            LedgerProduct::Qoder => qoder_paths::qoder_dir(),
        };
        let name = format!("gateway_ledger.{}.json", scope.as_str());
        dir.join(name)
    }

    /// 该产品**全部**落盘 scope（后台任务按它逐档记录）。
    fn scopes(self) -> &'static [LedgerScope] {
        match self {
            // Trae 的旧日志没有 `variant` 键（早于该字段）⇒ 必须保留 unlabeled 一档。
            LedgerProduct::Trae => &[LedgerScope::Cn, LedgerScope::Global, LedgerScope::Unlabeled],
            // Qoder 的 `region` 是**后加**的（`record_request` 从引入就写，但更早的
            // 落盘日志可能没有）⇒ 同样保留 unlabeled，否则那批请求整段消失。
            LedgerProduct::Qoder => &[LedgerScope::Cn, LedgerScope::Global, LedgerScope::Unlabeled],
        }
    }
}

/// 账本作用域（持久化轴：一档一份文件）。
///
/// 与 `TraeTokenScope` / `QoderRegion` **刻意分开**：那两个是「实体」（区域），
/// 这里是「账本的落盘分区」，多了 `Unlabeled`。合并会让「区域」这个既有概念
/// 多出一个不存在的取值。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LedgerScope {
    Cn,
    Global,
    /// 旧日志：行内没有归属字段。**单列一档**，不并入任何区域（见模块头）。
    Unlabeled,
}

impl LedgerScope {
    pub fn as_str(self) -> &'static str {
        match self {
            LedgerScope::Cn => "cn",
            LedgerScope::Global => "global",
            LedgerScope::Unlabeled => "unlabeled",
        }
    }

    /// 解析查询范围：`cn` / `global` / `unlabeled` / `all`（其余一律 `all`）。
    ///
    /// 返回**有序列表**（`all` = 三档），供 [`usage_dim`] 在内存里合并 ——
    /// 合并放在后端而不是前端，是因为 Trae 的 `all` 有三档（WB 只有两档，
    /// 前端 `mergeDims` 因此够用），三档递归合并在渲染层写容易漏一档。
    fn parse_query(raw: &str) -> Vec<LedgerScope> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "cn" => vec![LedgerScope::Cn],
            "global" | "intl" | "international" => vec![LedgerScope::Global],
            "unlabeled" | "none" | "unknown" => vec![LedgerScope::Unlabeled],
            _ => vec![LedgerScope::Cn, LedgerScope::Global, LedgerScope::Unlabeled],
        }
    }
}

// ---------------------------------------------------------------------------
// 存储形状
// ---------------------------------------------------------------------------

/// 一个维度桶（模型 / 账号 / 端点共用）。三条维度都由同一次扫描产出。
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
struct DimBucket {
    #[serde(default)]
    input: u64,
    #[serde(default)]
    output: u64,
    /// 命中上游前缀缓存的输入 token（`input` 的子集；0 = 上游没报，不是「没命中」）。
    ///
    /// ★ issue #154：账本级命中率的唯一来源（环形日志只覆盖几分钟）。
    #[serde(default, skip_serializing_if = "is_zero")]
    cached: u64,
    #[serde(default)]
    requests: u64,
    #[serde(default)]
    failed: u64,
    #[serde(default)]
    credit: f64,
}

/// `skip_serializing_if` 谓词：0 不落盘（老读码器读到缺字段也是 0，语义不变）。
fn is_zero(value: &u64) -> bool {
    *value == 0
}

impl DimBucket {
    fn total(&self) -> u64 {
        self.input.saturating_add(self.output)
    }

    /// `Add` 语义：逐字段相加（水位线之后的新条目）。
    fn add(&mut self, other: &DimBucket) {
        self.input = self.input.saturating_add(other.input);
        self.output = self.output.saturating_add(other.output);
        self.cached = self.cached.saturating_add(other.cached);
        self.requests = self.requests.saturating_add(other.requests);
        self.failed = self.failed.saturating_add(other.failed);
        self.credit += other.credit;
    }

    /// `Correct` 语义：逐字段取大（两个读数都只是当日的**子集**，相加会双计）。
    fn merge_max(&mut self, other: &DimBucket) {
        self.input = self.input.max(other.input);
        self.output = self.output.max(other.output);
        self.cached = self.cached.max(other.cached);
        self.requests = self.requests.max(other.requests);
        self.failed = self.failed.max(other.failed);
        if other.credit > self.credit {
            self.credit = other.credit;
        }
    }
}

/// 某本地日的账本记录。
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DayRecord {
    /// 增量水位线（毫秒）：只有 `ts > watermark` 的条目才累加。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    watermark_ms: Option<i64>,
    /// 当日日志「可能已被裁」（条数到上限且最旧存活条目就在今天）⇒ 当日总量是**下界**。
    #[serde(default)]
    truncated: bool,
    /// 当日全量（三条维度的合计必须等于它 —— 单测守住）。
    #[serde(default)]
    total: DimBucket,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    models: BTreeMap<String, DimBucket>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    accounts: BTreeMap<String, DimBucket>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    endpoints: BTreeMap<String, DimBucket>,
    /// 按**调用方密钥**的维度（键见 `token_daily::key_bucket_of`）。
    ///
    /// 与另外三个维度同轮同水位线产出 ⇒ 合计同样等于 `total`。
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    keys: BTreeMap<String, DimBucket>,
    /// 该日的**密钥**维度是升级播种出来的（v1 账本没有这一维）⇒ 只是**下界**。
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    keys_partial: bool,
}

/// 落盘存储：`{ version, days: { "YYYY-MM-DD": DayRecord } }`。
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Store {
    #[serde(default = "default_version")]
    version: u32,
    #[serde(default)]
    days: BTreeMap<String, DayRecord>,
}

fn default_version() -> u32 {
    LEDGER_VERSION
}

fn empty_store() -> Store {
    Store {
        version: LEDGER_VERSION,
        days: BTreeMap::new(),
    }
}

// ---------------------------------------------------------------------------
// 时间
// ---------------------------------------------------------------------------

fn local_date_string(ts: i64) -> Option<String> {
    Local
        .timestamp_millis_opt(ts)
        .single()
        .map(|dt| dt.format("%Y-%m-%d").to_string())
}

fn prune_store(store: &mut Store, at_ms: i64) {
    let Some(today) = Local
        .timestamp_millis_opt(at_ms)
        .single()
        .map(|dt| dt.date_naive())
    else {
        return;
    };
    let cutoff = today - ChronoDuration::days(LEDGER_RETENTION_DAYS - 1);
    store.days.retain(|day, _| {
        NaiveDate::parse_from_str(day, "%Y-%m-%d")
            .map(|date| date >= cutoff)
            .unwrap_or(false)
    });
    while store.days.len() > LEDGER_MAX_DAYS {
        let Some(oldest) = store.days.keys().next().cloned() else {
            break;
        };
        store.days.remove(&oldest);
    }
}

// ---------------------------------------------------------------------------
// 存储读写
// ---------------------------------------------------------------------------

fn load_store(product: LedgerProduct, scope: LedgerScope) -> Store {
    match std::fs::read_to_string(product.ledger_file(scope)) {
        Ok(text) => serde_json::from_str::<Store>(&text).unwrap_or_else(|_| empty_store()),
        Err(_) => empty_store(),
    }
}

fn save_store(product: LedgerProduct, scope: LedgerScope, store: &Store) {
    let path = product.ledger_file(scope);
    let content = serde_json::to_string_pretty(store).unwrap_or_default();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = atomic_write(&path, &content);
}

// ---------------------------------------------------------------------------
// 日志读取（只读上游）
// ---------------------------------------------------------------------------

/// 读网关日志条目。
///
/// 两种格式都认（见 [`LedgerProduct::log_file`]）：先当 JSON 数组解析，失败再按行
/// 解析（`jsonl`）。**解析不出就返回空**——宁可这一轮不记，也不许把坏文件当成
/// 「今天的用量是 0」写进账本。
fn load_log_entries(path: &Path) -> Vec<Value> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    if let Ok(Value::Array(entries)) = serde_json::from_str::<Value>(&text) {
        return entries;
    }
    text.lines()
        .filter_map(|line| serde_json::from_str::<Value>(line.trim()).ok())
        .collect()
}

/// 日志保留条数（读各平台网关配置的 `log_keep` / `logKeep`，缺省 200）。
fn log_keep(product: LedgerProduct) -> usize {
    if let Ok(text) = std::fs::read_to_string(product.config_file()) {
        if let Ok(value) = serde_json::from_str::<Value>(&text) {
            let keep = value
                .get("log_keep")
                .and_then(Value::as_u64)
                .or_else(|| value.get("logKeep").and_then(Value::as_u64));
            if let Some(keep) = keep {
                return (keep as usize).max(1);
            }
        }
    }
    DEFAULT_LOG_KEEP
}

/// 一条日志的账本视图。
#[derive(Clone, Debug)]
struct Row {
    ts: i64,
    day: String,
    scope: LedgerScope,
    account: String,
    model: String,
    endpoint: String,
    /// 调用方密钥的维度桶键（`token_daily::key_bucket_of` 的判据）。
    key: String,
    status: u16,
    input: u64,
    output: u64,
    /// 命中上游前缀缓存的输入 token（`input` 的子集；缺字段 = 0 = 上游没报）。
    cached: u64,
    credit: f64,
}

fn number_u64(value: &Value) -> Option<u64> {
    value
        .as_u64()
        .or_else(|| value.as_i64().and_then(|n| u64::try_from(n).ok()))
        .or_else(|| {
            value
                .as_f64()
                .filter(|n| n.is_finite() && *n >= 0.0)
                .map(|n| n as u64)
        })
}

/// 行的归属作用域。
///
/// - **Trae**：看 `variant`（后端 `record_request` 写的下划线形态）⇒ 经
///   [`TraeVariant::region`] 落到区域；缺失 / 不认识 ⇒ `Unlabeled`。
///   这里用 `TraeVariant::parse` 是**读日志**（写日志的那一侧就是 `as_str()`），
///   不是解析前端传参 —— 后者的纪律（只认 `Scope`/`variant_for_program`）不适用于本处。
/// - **Qoder**：看 `region`（`record_request` 写的 `region.as_str()`）；缺失 ⇒ `Unlabeled`。
fn scope_of(product: LedgerProduct, value: &Value) -> LedgerScope {
    match product {
        LedgerProduct::Trae => value
            .get("variant")
            .and_then(Value::as_str)
            .and_then(TraeVariant::parse)
            .map(|variant| match variant.region() {
                crate::modules::trae::region::TraeRegion::Cn => LedgerScope::Cn,
                crate::modules::trae::region::TraeRegion::Global => LedgerScope::Global,
            })
            .unwrap_or(LedgerScope::Unlabeled),
        LedgerProduct::Qoder => match value
            .get("region")
            .and_then(Value::as_str)
            .map(|region| region.trim().to_ascii_lowercase())
            .as_deref()
        {
            Some("cn") => LedgerScope::Cn,
            Some("global") | Some("intl") | Some("international") => LedgerScope::Global,
            _ => LedgerScope::Unlabeled,
        },
    }
}

/// 行的积分：`credit`（WB 口径）与 `credits`（Qoder 网关口径）都认。
///
/// 两个键**都缺** ⇒ `0.0`（= 不知道，不是免费）。Trae 的网关日志目前不记 credit
/// ⇒ 那张账本只有 token 维度，积分视图不会拿它冒充「网关积分消耗」。
fn row_credit(value: &Value) -> f64 {
    ["credit", "credits"]
        .iter()
        .find_map(|key| value.get(*key).and_then(Value::as_f64))
        .filter(|credit| credit.is_finite() && *credit >= 0.0)
        .unwrap_or(0.0)
}

fn parse_rows(product: LedgerProduct, entries: &[Value]) -> Vec<Row> {
    entries
        .iter()
        .filter_map(|value| {
            let ts = value.get("ts").and_then(Value::as_i64)?;
            let day = local_date_string(ts)?;
            let text = |key: &str| {
                value
                    .get(key)
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .unwrap_or_default()
                    .to_string()
            };
            // 账号归属：新日志有 `accountUid`（精确）；旧日志只有 `account`。
            // Trae / Qoder 的 `account` **本来就是 uid**（见两侧 `record_request`），
            // 因此这里不需要 WB 那套「展示名 → uid」回解。
            let account = {
                let uid = text("accountUid");
                if uid.is_empty() {
                    text("account")
                } else {
                    uid
                }
            };
            let model = {
                let model = text("model");
                if model.is_empty() {
                    UNKNOWN_MODEL.to_string()
                } else {
                    model
                }
            };
            Some(Row {
                ts,
                day,
                scope: scope_of(product, value),
                account,
                model,
                endpoint: text("endpoint"),
                key: key_bucket_of(value),
                status: value
                    .get("status")
                    .and_then(Value::as_u64)
                    .and_then(|status| u16::try_from(status).ok())
                    .unwrap_or(0),
                input: value.get("promptTokens").and_then(number_u64).unwrap_or(0),
                output: value
                    .get("completionTokens")
                    .and_then(number_u64)
                    .unwrap_or(0),
                // ★ issue #154：原值（`promptTokens` 的子集），不做减法 —— 减法只在
                //   Anthropic 出口的协议换算里做。缺字段 / 老条目 = 0 = 没观测到。
                cached: value.get("cachedTokens").and_then(number_u64).unwrap_or(0),
                credit: row_credit(value),
            })
        })
        .collect()
}

// ---------------------------------------------------------------------------
// 一轮聚合（水位线增量）
// ---------------------------------------------------------------------------

/// 一轮写入意图。
///
/// - `Add`：水位线存在 ⇒ 只加 `ts > watermark` 的新条目；
/// - `Correct`：水位线缺失（新的一天 / 首次记录）⇒ 整窗**取大**播种。
///
/// 为什么不直接整窗覆盖：日志是 FIFO 环形裁剪的，整窗覆盖会让「今天早些时候
/// 被裁掉的那几条」凭空消失，当日数字可**倒退**（`token_daily` 2026-10-15 实测）。
#[derive(Clone, Copy, Debug)]
enum Write {
    Add,
    Correct,
}

struct Round {
    write: Write,
    total: DimBucket,
    models: BTreeMap<String, DimBucket>,
    accounts: BTreeMap<String, DimBucket>,
    endpoints: BTreeMap<String, DimBucket>,
    /// 密钥维度（键见 `token_daily::key_bucket_of`）。
    keys: BTreeMap<String, DimBucket>,
    next_watermark: Option<i64>,
}

/// 只保留「本 scope + 本日 + `ts > since`」的行，并按 `key_of` 分桶。
fn bucket_rows(
    rows: &[Row],
    scope: LedgerScope,
    day: &str,
    since: Option<i64>,
    key_of: impl Fn(&Row) -> String,
) -> BTreeMap<String, DimBucket> {
    let mut out: BTreeMap<String, DimBucket> = BTreeMap::new();
    for row in rows {
        if row.scope != scope || row.day != day {
            continue;
        }
        if since.is_some_and(|watermark| row.ts <= watermark) {
            continue;
        }
        let bucket = out.entry(key_of(row)).or_default();
        bucket.input = bucket.input.saturating_add(row.input);
        bucket.output = bucket.output.saturating_add(row.output);
        bucket.cached = bucket.cached.saturating_add(row.cached);
        bucket.requests = bucket.requests.saturating_add(1);
        if row.status >= 400 {
            bucket.failed = bucket.failed.saturating_add(1);
        }
        bucket.credit += row.credit;
    }
    out
}

fn round(rows: &[Row], scope: LedgerScope, day: &str, watermark: Option<i64>) -> Round {
    let write = match watermark {
        Some(_) => Write::Add,
        None => Write::Correct,
    };
    let since = watermark;
    let total = bucket_rows(rows, scope, day, since, |_| String::new())
        .remove("")
        .unwrap_or_default();
    Round {
        write,
        total,
        models: bucket_rows(rows, scope, day, since, |row| row.model.clone()),
        accounts: bucket_rows(rows, scope, day, since, |row| row.account.clone()),
        endpoints: bucket_rows(rows, scope, day, since, |row| row.endpoint.clone()),
        keys: bucket_rows(rows, scope, day, since, |row| row.key.clone()),
        next_watermark: match (
            watermark,
            rows.iter()
                .filter(|row| row.scope == scope && row.day == day)
                .map(|row| row.ts)
                .max(),
        ) {
            (Some(current), Some(max)) => Some(current.max(max)),
            (Some(current), None) => Some(current),
            (None, max) => max,
        },
    }
}

fn apply(
    target: &mut BTreeMap<String, DimBucket>,
    source: BTreeMap<String, DimBucket>,
    write: &Write,
) {
    for (key, bucket) in source {
        let entry = target.entry(key).or_default();
        match write {
            Write::Add => entry.add(&bucket),
            Write::Correct => entry.merge_max(&bucket),
        }
    }
}

/// 「今日可能已被裁」判据：条数到上限 **且** 最旧存活条目就落在今天。
///
/// 只看条数会误报（窗口被昨天的流量顶满、今天的条目全在时，今日总量是精确的）。
/// 判据与 `token_daily::gateway_log_truncated_today` 同源；**不按 scope 分**：
/// 裁剪是全局 FIFO，被丢的条目属于哪一档无从得知 ⇒ 三档一起标（保守方向）。
fn truncated_today(entries_len: usize, keep: usize, oldest_day: Option<&str>, day: &str) -> bool {
    entries_len >= keep.max(1) && oldest_day == Some(day)
}

/// 把日志增量记进账本，返回该 scope 的今日摘要。
///
/// `scope` 只决定**写哪份文件**；不属于它的行不会进这份账。
pub fn record(product: LedgerProduct, scope: LedgerScope) -> Value {
    let at = now_ms();
    let day = local_date_string(at).unwrap_or_default();
    let entries = load_log_entries(&product.log_file());
    let oldest_day = entries
        .iter()
        .filter_map(|value| value.get("ts").and_then(Value::as_i64))
        .min()
        .and_then(local_date_string);
    let truncated = truncated_today(
        entries.len(),
        log_keep(product),
        oldest_day.as_deref(),
        &day,
    );
    let rows = parse_rows(product, &entries);

    let _guard = LEDGER_WRITE_LOCK.lock().unwrap();
    let mut store = load_store(product, scope);
    {
        let prior_watermark = store.days.get(&day).and_then(|record| record.watermark_ms);
        let completed = round(&rows, scope, &day, prior_watermark);
        let record = store.days.entry(day.clone()).or_default();
        /* ★ 密钥维度是后加的：老账本当天已经有量（`total.requests > 0`）而密钥桶是空的
        ⇒ 必须**整窗播种**（取大）并标 `keys_partial`。走增量会让当天此前的请求全部
        漏记，UI 上看着像「这把 Key 只用了这么点」—— 那正是本次要修的读数。 */
        let seed_keys = record.keys.is_empty() && record.total.requests > 0;
        if seed_keys {
            record.keys_partial = true;
        }
        record.watermark_ms = completed.next_watermark.or(prior_watermark);
        record.truncated = truncated;
        match completed.write {
            Write::Add => record.total.add(&completed.total),
            Write::Correct => record.total.merge_max(&completed.total),
        }
        apply(&mut record.models, completed.models, &completed.write);
        apply(&mut record.accounts, completed.accounts, &completed.write);
        apply(&mut record.endpoints, completed.endpoints, &completed.write);
        apply(
            &mut record.keys,
            completed.keys,
            if seed_keys {
                &Write::Correct
            } else {
                &completed.write
            },
        );
    }
    prune_store(&mut store, at);
    save_store(product, scope, &store);
    let today = store.days.get(&day).cloned().unwrap_or_default();
    json!({
        "date": day,
        "scope": scope.as_str(),
        "truncated": today.truncated,
        "total": today.total.total(),
        "requests": today.total.requests,
        "credit": round3(today.total.credit),
    })
}

/// 落库历史 → 账本视图（**只读**：不记录、不写盘）。
///
/// 返回体与 WB 的 `token_daily::gateway_usage_dim` **逐字段同形**（`days[].keys /
/// models / accounts / endpoints` 的每项都含 `total/requests/failed/credit`），
/// 前端因此复用同一个 `GatewayUsageDim` 类型与同一套拆分函数。
///
/// `scope` 为 `all` 时在**后端内存里**合并三档：Trae 有三档（cn / global / unlabeled），
/// 前端逐档取三次再递归合并容易漏一档 —— 合并口径只留一份。
pub fn usage_dim(product: LedgerProduct, scope: &str) -> Value {
    /// 跨档合并的中间形状（四个维度 + 两个披露标记）。
    ///
    /// 用结构体而不是元组：维度从 3 涨到 4 时，元组的下标（`slot.2` / `slot.3`）
    /// 已经读不出语义，而漏改一处不会编译失败 —— 只会让两个维度互换。
    #[derive(Default)]
    struct MergeDay {
        models: BTreeMap<String, DimBucket>,
        accounts: BTreeMap<String, DimBucket>,
        endpoints: BTreeMap<String, DimBucket>,
        keys: BTreeMap<String, DimBucket>,
        truncated: bool,
        keys_partial: bool,
    }
    let scopes = LedgerScope::parse_query(scope);
    let mut days: BTreeMap<String, MergeDay> = BTreeMap::new();
    for scope in scopes {
        let store = load_store(product, scope);
        for (day, record) in store.days {
            let slot = days.entry(day).or_default();
            // 跨档是**相加**（同一天的不同 scope 互不重叠，是不同批请求），
            // 不是取大 —— 取大会让「全部」小于两档之和。
            for (key, bucket) in record.models {
                slot.models.entry(key).or_default().add(&bucket);
            }
            for (key, bucket) in record.accounts {
                slot.accounts.entry(key).or_default().add(&bucket);
            }
            for (key, bucket) in record.endpoints {
                slot.endpoints.entry(key).or_default().add(&bucket);
            }
            for (key, bucket) in record.keys {
                slot.keys.entry(key).or_default().add(&bucket);
            }
            slot.truncated = slot.truncated || record.truncated;
            // 「至少有一档是从中间开始记的」⇒ 那天就是下界（披露取**或**）。
            slot.keys_partial = slot.keys_partial || record.keys_partial;
        }
    }
    let days: Vec<Value> = days
        .iter()
        .map(|(day, slot)| {
            json!({
                "key": day,
                "truncated": slot.truncated,
                // 与本模块的落盘同步生成 ⇒ 模型/账号/端点不存在「升级播种不完整」，
                // 但字段保留：前端 `GatewayUsageDim` 的形状要求它存在。
                "partial": false,
                // ★ 与 `partial` **分开**：密钥维度是后加的，老日子只能给下界。
                "keysPartial": slot.keys_partial,
                "keys": dim_groups(&slot.keys, "key"),
                "models": dim_groups(&slot.models, "model"),
                "accounts": dim_groups(&slot.accounts, "account"),
                "endpoints": dim_groups(&slot.endpoints, "endpoint"),
            })
        })
        .collect();
    json!({
        "date": local_date_string(now_ms()).unwrap_or_default(),
        "scope": scope,
        "retentionDays": LEDGER_RETENTION_DAYS,
        "days": days,
    })
}

/// `BTreeMap<String, DimBucket>` → 数组（按总量降序）。空键保留为空串（前端渲染「未记录」）。
fn dim_groups(map: &BTreeMap<String, DimBucket>, key_name: &str) -> Vec<Value> {
    let mut values: Vec<(u64, Value)> = map
        .iter()
        .map(|(key, bucket)| {
            let total = bucket.total();
            (
                total,
                json!({
                    key_name: key,
                    "input": bucket.input,
                    "output": bucket.output,
                    "total": total,
                    /* ★ issue #154：命中上游前缀缓存的输入 token（`input` 的子集）。
                    `null` = 这天没观测到命中（老账本无此列 / 上游没报），**不是 0 命中**
                    —— 与 `token_daily::dim_groups` 逐字同形，前端一套判据即可。 */
                    "cached": (bucket.cached > 0).then_some(bucket.cached),
                    "requests": bucket.requests,
                    "failed": bucket.failed,
                    "credit": round3(bucket.credit),
                }),
            )
        })
        .collect();
    values.sort_by_key(|(total, _)| std::cmp::Reverse(*total));
    values.into_iter().map(|(_, value)| value).collect()
}

/// 浮点保留三位（与 `token_daily::dim_groups` 同口径：避免 `0.30000000000000004`）。
fn round3(value: f64) -> f64 {
    (value * 1000.0).round() / 1000.0
}

/// 后台任务调用：把该产品的**全部** scope 各记一轮。
///
/// 覆盖全部档是必须的：日志是单文件、行内带归属，只记当前作用域会让另外两档的
/// 历史永远空着（切区域后账本整段是空的）。
pub fn record_all(product: LedgerProduct) {
    for scope in product.scopes() {
        let _ = record(product, *scope);
    }
}

/// 该产品落盘 scope 的稳定标识（诊断 / 测试用）。
pub fn scopes_of(product: LedgerProduct) -> Vec<&'static str> {
    product
        .scopes()
        .iter()
        .map(|scope| scope.as_str())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Datelike;

    fn at_today(hour: u32) -> i64 {
        let today = Local::now().date_naive();
        Local
            .with_ymd_and_hms(
                today.year() as i32,
                today.month() as u32,
                today.day() as u32,
                hour,
                0,
                0,
            )
            .single()
            .expect("valid local time")
            .timestamp_millis()
    }

    #[test]
    fn scope_of_maps_variant_to_region_and_keeps_unlabeled() {
        assert_eq!(
            scope_of(LedgerProduct::Trae, &json!({ "variant": "trae_merged" })),
            LedgerScope::Cn,
            "国内合并版落国内"
        );
        assert_eq!(
            scope_of(
                LedgerProduct::Trae,
                &json!({ "variant": "global_trae_merged" })
            ),
            LedgerScope::Global
        );
        assert_eq!(
            scope_of(LedgerProduct::Trae, &json!({})),
            LedgerScope::Unlabeled,
            "旧日志没有 variant ⇒ 未标注（不许塞进 cn）"
        );
        assert_eq!(
            scope_of(LedgerProduct::Qoder, &json!({ "region": "global" })),
            LedgerScope::Global
        );
        assert_eq!(
            scope_of(LedgerProduct::Qoder, &json!({ "region": "cn" })),
            LedgerScope::Cn
        );
        assert_eq!(
            scope_of(LedgerProduct::Qoder, &json!({})),
            LedgerScope::Unlabeled
        );
    }

    #[test]
    fn query_scope_parsing_covers_all_tiers() {
        assert_eq!(LedgerScope::parse_query("cn"), vec![LedgerScope::Cn]);
        assert_eq!(
            LedgerScope::parse_query("all"),
            vec![LedgerScope::Cn, LedgerScope::Global, LedgerScope::Unlabeled]
        );
        // 未知值 = 全部：账本视图默认「全部」，不能因为拼错就悄悄只看一档。
        assert_eq!(LedgerScope::parse_query("nonsense").len(), 3);
    }

    /// 四条维度的合计必须等于总量（页面上「按模型 = 按账号 = 网关总量」的自洽前提）。
    #[test]
    fn dims_sum_to_the_same_total() {
        let rows = vec![
            Row {
                ts: at_today(9),
                day: local_date_string(at_today(9)).unwrap(),
                scope: LedgerScope::Cn,
                account: "u1".into(),
                model: "m1".into(),
                endpoint: "/v1/chat/completions".into(),
                key: "trae:k1".into(),
                status: 200,
                input: 10,
                output: 2,
                cached: 9,
                credit: 0.5,
            },
            Row {
                ts: at_today(10),
                day: local_date_string(at_today(10)).unwrap(),
                scope: LedgerScope::Cn,
                account: "u2".into(),
                model: "m2".into(),
                endpoint: "/v1/responses".into(),
                key: "trae:k1".into(),
                status: 502,
                input: 7,
                output: 1,
                cached: 0,
                credit: 0.0,
            },
        ];
        let day = local_date_string(at_today(9)).unwrap();
        let completed = round(&rows, LedgerScope::Cn, &day, None);
        assert_eq!(
            completed.total,
            DimBucket {
                input: 17,
                output: 3,
                cached: 9,
                requests: 2,
                failed: 1,
                credit: 0.5
            }
        );
        let sum = |map: &BTreeMap<String, DimBucket>| {
            map.values().fold(DimBucket::default(), |mut acc, bucket| {
                acc.add(bucket);
                acc
            })
        };
        assert_eq!(sum(&completed.models), completed.total);
        assert_eq!(sum(&completed.accounts), completed.total);
        assert_eq!(sum(&completed.endpoints), completed.total);
        assert_eq!(
            sum(&completed.keys),
            completed.total,
            "密钥维度也必须与总量自洽（否则「按密钥看用量」的合计对不上网关）"
        );
        assert_eq!(
            completed.keys.len(),
            1,
            "同一个 keyId 落一个桶（两行都是 trae:k1）"
        );
    }

    /// 水位线语义：同一批行第二次进来不该再加一遍（跨轮双计是账本最严重的错）。
    #[test]
    fn watermark_prevents_double_counting() {
        let ts = at_today(9);
        let day = local_date_string(ts).unwrap();
        let rows: Vec<Row> = vec![Row {
            ts,
            day: day.clone(),
            scope: LedgerScope::Cn,
            account: "u1".into(),
            model: "m1".into(),
            endpoint: "/v1/chat/completions".into(),
            key: "trae:k1".into(),
            status: 200,
            input: 10,
            output: 2,
            cached: 0,
            credit: 0.0,
        }];
        let first = round(&rows, LedgerScope::Cn, &day, None);
        assert_eq!(first.total.total(), 12);
        // 第二轮：水位线 = 上一轮最大 ts ⇒ 同一条不再计入。
        let second = round(&rows, LedgerScope::Cn, &day, first.next_watermark);
        assert_eq!(second.total.total(), 0, "已记账的条目不许重复加");
    }

    /// 环形裁剪：窗口滚过去之后，已记的量不能倒退（`Correct` 只对缺失水位线的首次记录使用）。
    #[test]
    fn rolling_window_never_shrinks_the_day() {
        let ts_early = at_today(8);
        let ts_late = at_today(9);
        let day = local_date_string(ts_early).unwrap();
        let row = |ts: i64, input: u64| Row {
            ts,
            day: day.clone(),
            scope: LedgerScope::Cn,
            account: "u1".into(),
            model: "m1".into(),
            endpoint: "/e".into(),
            key: "trae:k1".into(),
            status: 200,
            input,
            output: 0,
            cached: 0,
            credit: 0.0,
        };
        let first = round(&[row(ts_early, 100)], LedgerScope::Cn, &day, None);
        let mut record = DayRecord::default();
        record.total.merge_max(&first.total);
        record.watermark_ms = first.next_watermark;
        // 窗口滚掉第一条，只剩后到的一条。
        let second = round(
            &[row(ts_late, 5)],
            LedgerScope::Cn,
            &day,
            record.watermark_ms,
        );
        record.total.add(&second.total);
        record.watermark_ms = second.next_watermark;
        assert_eq!(record.total.total(), 105, "已记的 100 不许因裁剪消失");
    }

    #[test]
    fn truncated_flag_needs_full_window_and_today_as_oldest() {
        assert!(!truncated_today(199, 200, Some("2026-10-28"), "2026-10-28"));
        assert!(truncated_today(200, 200, Some("2026-10-28"), "2026-10-28"));
        // 窗口被昨天顶满、今天的条目全在 ⇒ 今日总量精确，不该标记。
        assert!(!truncated_today(200, 200, Some("2026-10-27"), "2026-10-28"));
    }

    #[test]
    fn credit_reads_both_key_spellings() {
        assert_eq!(row_credit(&json!({ "credit": 1.5 })), 1.5);
        assert_eq!(row_credit(&json!({ "credits": 0.25 })), 0.25);
        assert_eq!(row_credit(&json!({})), 0.0);
        assert_eq!(
            row_credit(&json!({ "credits": -1.0 })),
            0.0,
            "负数视为不知道"
        );
    }

    #[test]
    fn records_all_three_scopes_for_trae() {
        assert_eq!(scopes_of(LedgerProduct::Trae).len(), 3);
        assert!(scopes_of(LedgerProduct::Trae).contains(&"unlabeled"));
    }

    /// `dim_groups` 的形状：按总量降序，且带齐前端 `GatewayUsageDim` 需要的每个键。
    ///
    /// 落盘往返（`record` → `usage_dim`）留给集成测试
    /// `tests/gateway_ledger.rs` —— 它会改进程级 `BUDDY_SWITCH_HOME`，
    /// 放进单元测试会与并行运行的其他用例互相污染。
    #[test]
    fn dim_groups_sorts_by_total_and_keeps_every_key() {
        let mut map: BTreeMap<String, DimBucket> = BTreeMap::new();
        map.insert(
            "small".into(),
            DimBucket {
                input: 10,
                output: 5,
                cached: 4,
                requests: 2,
                failed: 1,
                credit: 0.125,
            },
        );
        map.insert(
            "big".into(),
            DimBucket {
                input: 100,
                output: 50,
                cached: 0,
                requests: 7,
                failed: 0,
                credit: 0.0,
            },
        );
        let rows = dim_groups(&map, "model");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0]["model"], "big", "按总量降序");
        assert_eq!(rows[0]["total"], 150);
        assert_eq!(rows[0]["requests"], 7);
        assert_eq!(rows[1]["failed"], 1);
        // 浮点必须收敛到三位（否则前端会显示 0.12500000000000003 这类值）。
        assert_eq!(rows[1]["credit"], 0.125);
    }
}
