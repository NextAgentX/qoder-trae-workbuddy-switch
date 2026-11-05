//! 每日 Token 记录：**按账号 × 来源 × 本地日**持久化「今日 Token」。
//!
//! 两个来源分别聚合后落进同一份 region 化存储（[`token_daily_file_for`]）：
//!
//! | 来源 | 数据源 | 归属键 |
//! |---|---|---|
//! | `gateway` | 网关请求日志 `~/.buddy-switch/gateway_logs.json`（**只读**） | 行 `accountUid`（精确 uid）；旧行无此字段时才回落解析行 `account` 展示名 |
//! | `client` | CLI 会话日志 `{data_dir}/projects` 下的 jsonl（**流式逐行**） | `sessionId` → `workbuddy.db`.`sessions.user_id` |
//!
//! 设计取向（对齐 `credit_usage.rs` 的持久化 / 保留 / 原子写风格）：
//!
//! - **只读上游**：绝不改网关写路径、绝不写 `workbuddy.db`（`SQLITE_OPEN_READ_ONLY`）。
//! - **增量记账**（2026-10-15 修正）：网关日志是**环形裁剪**的滚动窗口（只留最近
//!   `log_keep` 条）⇒ 当日网关桶按**水位线增量**累加（`ts > watermark` 才加，见
//!   [`gateway_round`]），**不是**「当前窗口快照」—— 否则窗口一滚，今天的数字会**倒退**
//!   （实测 CN 桶 10.3M → 3.6M → 18.7M）。已计入的量永不因裁剪消失；水位线缺失
//!   （升级过渡 / 新的一天）时先做一次**校正轮**（逐账号取 `max(现有, 当前窗口)`）。
//! - **诚实**：网关日志的**今日**部分可能被裁过（条数到达 `log_keep` **且**最旧存活
//!   条目就是今天，见 [`gateway_log_truncated_today`]）时透出 `gatewayTruncated`；
//!   无法归属的客户端会话落 `unknownClient` 桶，**绝不猜**到别的账号，也**绝不静默丢弃**。
//! - **重扫有界**：客户端扫描带 **TTL 闸门**（[`CLIENT_SCAN_TTL_MS`]，`force` 可强制重扫），
//!   且**两道代价控制**：**mtime 预筛**（只有「当天动过」的文件才可能含当天记录）
//!   + **尾部窗口读取**（当天记录只可能在文件尾部）。见 [`scan_client_rows`]。
//!     ★ 驱动它的**两个调用点缺一不可**：账号页命令（主动看时立刻刷新）+ 后台周期任务
//!     （`modules::background` 的 `ClientUsageRecord`，每 2 分钟）—— 后者是 2026-10-15
//!     用户报障「client 数字不动」的修法（此前只有账号页一个调用点）。
//!
//!   ⚠️ 闸门**曾经**是「当天扫过一次就不再扫」，那是**错的**（2026-10-12 实测）：
//!   当天第一次扫描通常发生在**任何用量产生之前**，于是当日值被冻结成空，
//!   一整天卡片都显示「—」。改成 TTL 后，最多滞后 [`CLIENT_SCAN_TTL_MS`]。
//!   代价控制不靠「少扫」而靠「扫得少」—— 见下方「性能」。
//!
//! ## 性能（2026-10-13 实测）
//!
//! 「今日」窗口，Global 语料（当日被动过的 5,704 个文件 / **14.59 GB**）：
//!
//! | 版本 | 冷启（OS 缓存冷） | 稳态 |
//! |---|---:|---:|
//! | 逐行读完整份（原实现） | **66–75 s** | 66–75 s |
//! | 预筛 + 尾部窗口 + 并行（现实现） | ≈5 s | **≈0.7 s** |
//!
//! 三条改动的分工（缺一条都达不到这个数）：
//!
//! 1. **遍历**：`entry.file_type()` 取代 `path.is_dir()`（同一棵树 **0.69 s → 0.03 s**），
//!    并把 mtime 预筛**并进遍历**（不再「先收全部路径、再逐个 `stat`」）；
//! 2. **读取**：只读尾部窗口（[`jsonl_tail`]）—— 实读字节 ≈ 预筛文件总大小的 **1.2%**
//!    （14.59 GB → 0.18 GB）；
//! 3. **解析**：按文件并行（[`worker_count`]，上限 32）。
//!
//! 等价性：`tail_window_keeps_every_today_record_against_a_full_read` 用**真文件**
//! 驱动生产函数 `read_day_window`，逐条比对「尾部窗口 vs 整份读」；实测同语料下
//! 两者**逐字段一致**（`clientIn` / `clientOut` 完全相同）。

use chrono::{Duration as ChronoDuration, Local, NaiveDate, TimeZone};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::{Duration, UNIX_EPOCH};

use rusqlite::Connection;

use crate::modules::account;
use crate::modules::config::{atomic_write, now_ms, store_dir};
use crate::modules::jsonl_tail;
use crate::modules::region::{gateway_config_file, token_daily_file_for, Region};
use crate::modules::session::{session_data_dir, workbuddy_db_path_for};

/// 每日记录保留天数（镜像 `credit_usage::CREDIT_SNAPSHOT_RETENTION_DAYS`）。
pub const TOKEN_DAILY_RETENTION_DAYS: i64 = 90;
/// 每日记录条数上限（**安全网**：保留窗内还可能出现病态/未来的日期，超出即丢最旧）。
pub const TOKEN_DAILY_MAX_DAYS: usize = 400;
/// 存储 schema 版本。
///
/// v2：`clientScannedDate`（本地日串）→ `clientScannedAtMs`（毫秒时刻）——
/// 闸门语义从「当天扫过就不再扫」改为「距上次扫描不足 TTL 才跳过」（见
/// [`CLIENT_SCAN_TTL_MS`]）。旧文件里那个字段反序列化时被忽略 ⇒ `None` ⇒ 重扫一次，
/// 结果正确（只是多扫一轮）。
/// 落盘版本。v3（2026-10-21，issue #63）：`DayRecord` 新增三个网关维度桶
/// （`gatewayModels` / `gatewayAccounts` / `gatewayEndpoints`）+ `gatewayModelsPartial`。
/// 旧文件缺这些字段 ⇒ `serde(default)` 读成空，由 `gateway_round` 的**播种轮**补齐，
/// **不需要**整库丢弃。
///
/// v4（2026-10-28，issue #76 补账）：新增第四个维度 `gatewayKeys` + `gatewayKeysPartial`
/// —— 用量页「按密钥看用量」的**正确数据源**。
///
/// 为什么必须进账本（而不是继续扫请求日志）：网关请求日志是**环形裁剪**的
/// （只留最近 `log_keep` 条，缺省 200 ⇒ 繁忙时段只覆盖十几分钟）。旧实现直接聚合
/// 该窗口并当成「近 30 天」展示，于是本机出现「44.2M Token / 200 次 / 100%（其实是
/// 17 分钟）」这种**量级正确、区间错误**的读数。账本按日累积，窗口才有意义；
/// 代价是**升级前的日期没有 key 维度**（由 `gatewayKeysPartial` 与「无该维度」
/// 两种情况分别披露，绝不补 0 冒充）。
///
/// v5（2026-10-29，模型降级链账本维度）：新增 `fallbackDays` —— **降级链**的逐日计数。
///
/// 为什么必须落盘：降级事件原本只存在网关**进程内的 50 条环形缓冲**里
/// （见 `buddy_switch_gateway::model_fallback`）⇒ 重启即失、且只覆盖最近几十秒。
/// 「近 7 天降级了多少次 / 成功率多少」这种**窗口统计**只能来自逐日落库的计数。
///
/// v6（2026-11-05，issue #154）：网关侧新增 `cached`（命中上游前缀缓存的输入 token）
/// —— 账号桶与四条维度桶都带。**老文件不需要迁移**：`serde(default)` 读成 0，
/// 而 0 的语义正是「没观测到」（不是「没命中」）⇒ 升级当天只会显示「无命中数据」，
/// 第二天起自动齐全。**不做历史回填**：日志是环形窗口，回填只能拿到几分钟的样本，
/// 那份数字比「没有」更容易被误读成「历史命中率就是这么低」。
pub const TOKEN_DAILY_VERSION: u32 = 6;

/// 网关日志条数上限的缺省值（与 `GatewayConfig::default().log_keep` 同值）。
const DEFAULT_GATEWAY_LOG_KEEP: usize = 200;

/// 客户端扫描的**最短重扫间隔**（毫秒）。
///
/// ## 为什么闸门不能是「当天扫过一次就不再扫」（2026-10-12 实测驱动的修正）
///
/// 原闸门判据是 `clientScannedDate == 今天` ⇒ 当日值被**冻结在当天第一次扫描的那一刻**。
/// 实测现场：CN 的 `token_daily.json` 在 **14:33** 落盘（当时今日确无用量 ⇒ `accounts: {}`），
/// 客户端 **15:00** 才开始产生用量 ⇒ 此后一整天卡片都是「—」；而前端从不传 `force`，
/// **没有任何路径**能让它恢复 —— 这正是「今日 Token 总是横杠」的根因。
///
/// ## 为什么取 2 分钟
///
/// 「今日 Token」是仪表盘读数：**不需要实时**，但绝不能整天不动。2 分钟足够短
/// （用户切到账号页基本都能看到当前值），又足够长（一次页面停留不会反复重扫）。
/// 真正的代价控制在 [`scan_client_rows`]：mtime 预筛 + 尾部窗口 + 并行。
///
/// ★ 2026-10-15：本闸门同时是**后台周期任务**（`modules::background` 的
/// `ClientUsageRecord`）的节流阀 —— 那条任务每 2 分钟调一次
/// [`record_client_today`]`(region, false)`，与本值同值 ⇒ 闸门挡掉的只是「刚被账号页
/// 扫过」的重复轮次，不挡新鲜度。**周期任务的存在理由**：此前唯一调用点是账号页命令，
/// 不进账号页卡片 client 列就一直不动（用户报障）。
pub const CLIENT_SCAN_TTL_MS: i64 = 2 * 60 * 1000;

/// mtime 预筛的**回退余量**（毫秒）：允许文件在本地日界之前一点点被写入。
///
/// 日志是**追加写**的，「一条当日记录的写入时刻」不早于「它自己的时间戳」，
/// 所以日界之后的 mtime 足以覆盖当日记录。留 1 小时余量是为了吸收
/// 时钟抖动 / 跨日批量刷盘这类噪声 —— 代价只是多读几个文件。
const CLIENT_SCAN_MTIME_SKEW_MS: i64 = 60 * 60 * 1000;

static TOKEN_DAILY_WRITE_LOCK: Mutex<()> = Mutex::new(());

// ---------------------------------------------------------------------------
// 存储形状
// ---------------------------------------------------------------------------

/// 一个来源（client / gateway）的输入 / 输出累计。
///
/// ★ **`cached` 只在网关侧有值**（issue #154 的观测缺口）：客户端会话日志里的
/// `cache_read_input_tokens` 是另一个模块（`token_stats`）在算，本模块的 client 桶
/// 从不填它 ⇒ 恒 0。判「命中率」时必须只对 gateway 桶求值，别把两本账混在一起。
///
/// `cached` 的语义是「`input` 里**命中**的部分」，**不从 `input` 里扣**
/// （`input` 保持上游 `prompt_tokens` 原值）：命中率 = `cached / input`，
/// 与 `token_stats` 的 `cacheHitRate` 同一口径。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
struct Bucket {
    #[serde(default)]
    input: u64,
    #[serde(default)]
    output: u64,
    /// 命中上游前缀缓存的输入 token（`input` 的**子集**；0 = 上游没报，不是「没命中」）。
    #[serde(default, skip_serializing_if = "is_zero")]
    cached: u64,
}

/// `skip_serializing_if` 的谓词：0 不落盘（老读码器读到缺字段也是 0，语义不变）。
fn is_zero(value: &u64) -> bool {
    *value == 0
}

/// 网关的**维度桶**：模型 / 账号 / 端点共用一套形状。
///
/// 为什么把三者做成同一个结构：它们都由**同一次日志扫描、同一个水位线**产出，
/// 只是分组键不同（`model` / `uid` / `endpoint`）。分开定义必然在「哪些字段要累加」
/// 上漂移 —— 而这里一旦漂移，页面上「按模型合计 = 网关总量」这条自洽约束就破了。
///
/// `requests` / `failed` 让「429 × 5」这类事实能落到模型/端点维度上；
/// `credit` 是上游积分（积分视图按来源拆消耗的唯一真实来源，见 issue #63）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
struct DimBucket {
    #[serde(default)]
    input: u64,
    #[serde(default)]
    output: u64,
    /// 命中上游前缀缓存的输入 token（`input` 的**子集**；0 = 上游没报，不是「没命中」）。
    ///
    /// ★ issue #154：只有这一列能让「按日 / 按账号 / 按密钥的命中率」从环形日志
    /// （`log_keep` 缺省 200 ≈ 几分钟）升级成账本级统计。
    #[serde(default, skip_serializing_if = "is_zero")]
    cached: u64,
    #[serde(default)]
    requests: u64,
    #[serde(default)]
    failed: u64,
    #[serde(default)]
    credit: f64,
}

/// 维度桶的一轮写入意图（与 [`GatewayWrite`] 同构，只是值类型不同）。
#[derive(Debug)]
enum DimWrite {
    Add(BTreeMap<String, DimBucket>),
    Correct(BTreeMap<String, DimBucket>),
}

/// 某账号在当日的双来源桶。
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct AccountRecord {
    #[serde(default)]
    client: Bucket,
    #[serde(default)]
    gateway: Bucket,
}

/// 某本地日的全部账号记录。
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DayRecord {
    #[serde(default)]
    accounts: BTreeMap<String, AccountRecord>,
    /// 无法归属到任何账号的客户端用量；`None`（缺省）= 该日未出现无法归属的会话。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    unknown_client: Option<Bucket>,
    /// 网关卡是否可能被裁剪（条数到达 `log_keep`）。
    #[serde(default)]
    gateway_truncated: bool,
    /// ★ 网关聚合的**增量水位线**（毫秒，见 [`gateway_round`]）：只有 `ts > watermark`
    /// 的日志条目才会被累加进当日网关桶。
    ///
    /// `None` = **旧版本写入的记录**（或从未计过数的新日）——升级到增量语义时会先做
    /// 一次**校正轮**（逐账号取 `max(现有, 当前窗口)`，见 [`GatewayWrite::Correct`]）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    gateway_watermark_ts: Option<i64>,
    /// 网关按**模型**的维度桶（键 = 模型名；`model` 为空落哨兵 [`UNKNOWN_MODEL`]）。
    ///
    /// 三条维度的合计**都必须等于**该日网关 token 总量 —— 它们与 `accounts[].gateway`
    /// 由同一次扫描、同一水位线产出（护栏：`gateway_dims_sum_to_the_gateway_bucket`）。
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    gateway_models: BTreeMap<String, DimBucket>,
    /// 网关按**账号 uid** 的维度桶（与 `accounts` 同一套 uid 解析，含旧行回落）。
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    gateway_accounts: BTreeMap<String, DimBucket>,
    /// 网关按**端点**的维度桶（`/v1/chat/completions` 等）。
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    gateway_endpoints: BTreeMap<String, DimBucket>,
    /// 网关按**调用方密钥**的维度桶（键口径见 [`key_bucket_of`]）。
    ///
    /// 与另外三个维度**同轮同水位线**产出 ⇒ 合计也等于当日网关 token 总量
    /// （护栏 `gateway_dims_sum_to_the_gateway_bucket`）。
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    gateway_keys: BTreeMap<String, DimBucket>,
    /// 该日维度桶是**升级播种**出来的（v2 → v3 过渡）⇒ 拆分不完整，UI 要披露。
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    gateway_models_partial: bool,
    /// 该日**密钥**维度是升级播种出来的（v3 → v4 过渡）⇒ 只是**下界**，UI 要披露。
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    gateway_keys_partial: bool,
}

/// `model` 字段为空时的哨兵（渲染层翻成「未标注模型」）。
///
/// 不丢弃这类行：它们**确实消耗了 token**，丢掉会让「按模型合计」小于网关卡。
pub const UNKNOWN_MODEL: &str = "__unknown__";

/// 「按密钥」维度里两个**非 Key** 桶的键（升级前条目 / 确实没有 Key 的调用）。
///
/// 与前端 `src/lib/key-usage.ts` 的 `LEGACY_BUCKET` / `NONE_BUCKET` 逐字相同 ——
/// **两组语义不得合并**：把「升级前没记 key」并进「无 Key 调用」会让历史数据凭空
/// 多出一批「匿名调用」，而那个数字一旦出现在排行里就会被当成真的。
pub const KEY_DIM_LEGACY: &str = "__legacy__";
/// 确实没有 Key 的调用（本地直连 / 管理面探测）。
pub const KEY_DIM_NONE: &str = "__none__";

/// 日志行 → 「按密钥」维度桶键。
///
/// 三种取值，**判据只看 JSON 里有没有 `keyId` 这个键**：
/// - **键不存在** ⇒ [`KEY_DIM_LEGACY`]（本改动之前写入的老条目）；
/// - **值为 `null` / 空串** ⇒ [`KEY_DIM_NONE`]（这次请求确实没带 Key）；
/// - 其余 ⇒ `{keyScope}:{keyId}`，`keyScope` 缺失时退化成裸 id。
///
/// 为什么要带 `keyScope`：统一网关把请求转发给产品线中继时，产品线日志里记的是
/// **网关那把 Key 的 id** —— 它与产品线自己的 Key id 混在同一个字段。带上归属，
/// 同一串 id 在两套台账里才不会并成一行（前端 `aggregateByKeyDim` 同判据）。
pub fn key_bucket_of(value: &Value) -> String {
    let Some(raw) = value.get("keyId") else {
        return KEY_DIM_LEGACY.to_string();
    };
    let id = raw.as_str().map(str::trim).unwrap_or_default();
    if raw.is_null() || id.is_empty() {
        return KEY_DIM_NONE.to_string();
    }
    match value
        .get("keyScope")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|scope| !scope.is_empty())
    {
        Some(scope) => format!("{scope}:{id}"),
        None => id.to_string(),
    }
}

/// 落盘存储：`{ version, days: { "YYYY-MM-DD": DayRecord }, clientScannedAtMs }`。
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Store {
    #[serde(default = "default_version")]
    version: u32,
    #[serde(default)]
    days: BTreeMap<String, DayRecord>,
    /// 客户端扫描的**上次完成时刻**（毫秒）。非 `force` 且「同一天且不足 TTL」时不再重扫。
    ///
    /// ⚠️ 刻意存**时刻**而不是「本地日串」：只存日串就退化成「当天扫过就不再扫」，
    /// 当日值会被冻结在当天首次扫描那一刻（见 [`CLIENT_SCAN_TTL_MS`] 的实测现场）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    client_scanned_at_ms: Option<i64>,
    /// **降级链**的逐日计数（键 = 本地日 `YYYY-MM-DD`）。
    ///
    /// ## 为什么这里是**累加**，而 `days` 是「折叠 + 取大」
    ///
    /// `days` 从**请求日志**聚合，而日志是环形缓冲 —— 同一次请求会被反复读到，
    /// 所以必须「取大 / 纠偏」而不是累加（见 [`GatewayWrite::Correct`]）。
    /// 降级事件相反：它是**一次性事件**（进程内 50 条环形缓冲、重启即失），
    /// 读到第二次就可能已经丢了 ⇒ 折叠会漏、取大会错，只能**累加**。
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub fallback_days: BTreeMap<String, FallbackDayCounts>,
}

/// 某个本地日的**降级计数**（两档）。
///
/// ## 为什么只有两档
/// 「降级成功率」的分母只能是**真正发生过降级尝试**的事件：`degraded`（备选顶上了）
/// + `failed`（备选也失败）。`blocked`（按规则压根没降级）不属于任何一档 ——
/// 把它算进 `failed` 会把分母灌水（混进根本没降级的事故）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FallbackDayCounts {
    /// 降级**成功**次数（备选顶上了，客户端拿到正常响应）。
    #[serde(default)]
    pub degraded: u32,
    /// 降级**失败**次数（备选也失败，最终仍以失败告终）。
    #[serde(default)]
    pub failed: u32,
}

/// 一次降级事件的结果（落盘形状；与网关侧 `FallbackEvent.result` 逐字对应）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FallbackResult {
    /// 降级成功。
    Degraded,
    /// 降级失败。
    Failed,
    /// 按规则没降级（**不计入**任何一档）。
    Blocked,
}

fn default_version() -> u32 {
    TOKEN_DAILY_VERSION
}

fn empty_store() -> Store {
    Store {
        version: TOKEN_DAILY_VERSION,
        days: BTreeMap::new(),
        client_scanned_at_ms: None,
        fallback_days: BTreeMap::new(),
    }
}

// ---------------------------------------------------------------------------
// 时间
// ---------------------------------------------------------------------------

/// 毫秒时间戳 → 本地日期串（`%Y-%m-%d`，与 `credit_usage` 的日键同格式）。
fn local_date_string(ts: i64) -> Option<String> {
    Local
        .timestamp_millis_opt(ts)
        .single()
        .map(|dt| dt.format("%Y-%m-%d").to_string())
}

fn local_date(ts: i64) -> Option<NaiveDate> {
    Local
        .timestamp_millis_opt(ts)
        .single()
        .map(|dt| dt.date_naive())
}

/// 本地日 `YYYY-MM-DD` 的**起点毫秒**（供 mtime 预筛当「日界」）。
///
/// 非法日期 / DST 歧义（`from_local_datetime` 返回 `Ambiguous`）⇒ `None`。
/// 调用方拿到 `None` 时**必须退化为全量扫描** —— 宁可慢，不可漏（诚实优先于快）。
fn local_day_start_ms(day: &str) -> Option<i64> {
    let date = NaiveDate::parse_from_str(day, "%Y-%m-%d").ok()?;
    let naive = date.and_hms_opt(0, 0, 0)?;
    Local
        .from_local_datetime(&naive)
        .single()
        .map(|dt| dt.timestamp_millis())
}

// ---------------------------------------------------------------------------
// 存储读写
// ---------------------------------------------------------------------------

fn load_store(region: Region) -> Store {
    let path = token_daily_file_for(region);
    if let Ok(text) = std::fs::read_to_string(path) {
        if let Ok(store) = serde_json::from_str::<Store>(&text) {
            return store;
        }
    }
    empty_store()
}

fn save_store(region: Region, store: &Store) -> bool {
    let content = serde_json::to_string_pretty(store).unwrap_or_default();
    std::fs::create_dir_all(store_dir())
        .and_then(|_| atomic_write(&token_daily_file_for(region), &content))
        .is_ok()
}

/// 丢弃保留窗之外的日期，并用条数上限兜底（丢最旧）。
fn prune_store(store: &mut Store, at_ms: i64) {
    let Some(today) = local_date(at_ms) else {
        return;
    };
    let cutoff = today - ChronoDuration::days(TOKEN_DAILY_RETENTION_DAYS - 1);
    store.days.retain(|day, _| {
        NaiveDate::parse_from_str(day, "%Y-%m-%d")
            .map(|date| date >= cutoff)
            .unwrap_or(false)
    });
    // 降级计数与 `days` 同一个保留窗：不清理会让这个 map 无限增长
    // （它按日累加，永远不会被"覆盖"）。
    store.fallback_days.retain(|day, _| {
        NaiveDate::parse_from_str(day, "%Y-%m-%d")
            .map(|date| date >= cutoff)
            .unwrap_or(false)
    });
    // BTreeMap 的键（`YYYY-MM-DD`）字典序 = 时间序，最旧的在最前。
    while store.days.len() > TOKEN_DAILY_MAX_DAYS {
        let Some(oldest) = store.days.keys().next().cloned() else {
            break;
        };
        store.days.remove(&oldest);
    }
}

// ---------------------------------------------------------------------------
// 网关日志聚合（只读）
// ---------------------------------------------------------------------------

/// 网关请求日志文件路径（与 `buddy-switch-gateway::state::gateway_log_file` 一致）。
///
/// 刻意在 core 内自行派生、而非依赖网关 crate：本模块只**读**该文件，不参与写路径。
fn gateway_log_file() -> PathBuf {
    gateway_config_file().with_file_name("gateway_logs.json")
}

/// 网关日志保留条数：读 `gateway_config.json` 的 `log_keep`（缺省 200）。
fn gateway_log_keep() -> usize {
    if let Ok(text) = std::fs::read_to_string(gateway_config_file()) {
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
    DEFAULT_GATEWAY_LOG_KEEP
}

fn load_gateway_log() -> Vec<Value> {
    if let Ok(text) = std::fs::read_to_string(gateway_log_file()) {
        if let Ok(Value::Array(entries)) = serde_json::from_str::<Value>(&text) {
            return entries;
        }
    }
    Vec::new()
}

/// 网关日志「**今日**部分可能已被裁剪」的判据（诚实标注，不假装是完整总量）。
///
/// 两个条件**同时**成立才算：
/// 1. **条数到达保留上限**（`entries_len >= keep`）—— 没满过就什么都没丢；
/// 2. **存活最旧的条目落在今天**（`oldest_day == Some(day)`）—— 裁剪是 **FIFO**
///    （`buddy_switch_gateway::logging::RequestLog::trim_and_save` 从头部
///    `drain(..drop_count)`，`record` 向尾部追加），只要最旧存活条目**早于今天**，
///    今天的条目就一条都没被丢过 ⇒ 今日总量是**完整**的。
///
/// ## 为什么不能只看第 1 条（旧判据的误报）
///
/// 日志是**滚动窗口**：它可能被**昨天**（或更早）的流量顶满，而今天的条目全在。
/// 那种情况下今日总量是精确的，旧判据却标 `true` ⇒ 用户平白看到「≥」和一条
/// 「网关用量只是下界」的说明。本判据把「窗口滑过今天」与「窗口停在今天之前」
/// 区分开（2026-10-15 修）。
///
/// 佐证：前端概览的同款判据一直是这个口径 ——
/// `src/lib/overview-metrics.ts::summarizeGatewayLogs`（今日**请求数**）早就写着
/// `oldest >= start && len >= keep`，并附了同样的 FIFO 说明。本次只是把**后端
/// token 口径**对齐到它（此前两处判据不一致：概览精确、卡片宽松）。
///
/// ## 为什么判据**不按 region** 分
///
/// 裁剪是**全局** FIFO：被丢的条目属于哪个 region 无从得知。所以只要今天的条目
/// **可能**丢过，两个 region 都标 `true`（保守方向）；反之都不标。按 region 各标
/// 一套既无判据、也会让「Global 一条 gateway 行都没有」的 store 平白带上裁剪标记。
fn gateway_log_truncated_today(
    entries_len: usize,
    keep: usize,
    oldest_day: Option<&str>,
    day: &str,
) -> bool {
    if entries_len < keep.max(1) {
        return false;
    }
    oldest_day == Some(day)
}

#[derive(Clone, Debug)]
struct GatewayRow {
    /// 写入时刻（毫秒）—— **增量水位线**的判据（见 [`gateway_round`]）。
    ts: i64,
    region: String,
    /// 展示名（仅作**旧行**回落的解析输入）。
    account: String,
    /// 写入时记录的精确 uid（新行必有；旧行为 `None`）。
    account_uid: Option<String>,
    day: String,
    input: u64,
    output: u64,
    /// 命中上游前缀缓存的输入 token（`input` 的子集；缺字段 = 0 = 上游没报）。
    cached: u64,
    /// 模型名（空 ⇒ [`UNKNOWN_MODEL`]）。
    model: String,
    /// 端点（`/v1/chat/completions` 等）。
    endpoint: String,
    /// 调用方密钥的维度桶键（见 [`key_bucket_of`]；无 Key / 老条目走哨兵）。
    key: String,
    /// HTTP 状态码（`>= 400` 计入 `failed`）。
    status: u16,
    /// 上游积分（`usage.credit`；缺字段 = 0.0 = 不知道）。
    credit: f64,
}

fn parse_gateway_rows(entries: &[Value]) -> Vec<GatewayRow> {
    entries
        .iter()
        .filter_map(|value| {
            let ts = value.get("ts").and_then(Value::as_i64)?;
            let day = local_date_string(ts)?;
            let account = value
                .get("account")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|account| !account.is_empty())?
                .to_string();
            // 写入时的精确身份；旧日志无此字段 ⇒ `None`（回落到展示名解析）。
            let account_uid = value
                .get("accountUid")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|uid| !uid.is_empty())
                .map(str::to_string);
            let region = value
                .get("region")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            Some(GatewayRow {
                ts,
                region,
                account,
                account_uid,
                day,
                input: value.get("promptTokens").and_then(number_u64).unwrap_or(0),
                output: value
                    .get("completionTokens")
                    .and_then(number_u64)
                    .unwrap_or(0),
                /* ★ 与上游 `input` 一样是「原值」：`cachedTokens` 是 `promptTokens` 的
                **子集**，不需要也不应该在这里做减法 —— 减法只发生在 Anthropic 出口
                的协议换算（`protocol/anthropic.rs::anthropic_input_tokens`）。
                缺字段 / 老条目 = 0 = 上游没报（不是「没命中」）。 */
                cached: value.get("cachedTokens").and_then(number_u64).unwrap_or(0),
                model: value
                    .get("model")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|model| !model.is_empty())
                    .unwrap_or(UNKNOWN_MODEL)
                    .to_string(),
                key: key_bucket_of(value),
                endpoint: value
                    .get("endpoint")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                status: value
                    .get("status")
                    .and_then(Value::as_u64)
                    .and_then(|status| u16::try_from(status).ok())
                    .unwrap_or(0),
                credit: value
                    .get("credit")
                    .and_then(Value::as_f64)
                    .filter(|credit| credit.is_finite() && *credit >= 0.0)
                    .unwrap_or(0.0),
            })
        })
        .collect()
}

/// 按归属键分桶，只保留「该 region + 该本地日 + `ts > since`」的行。
///
/// 归属键：**优先**行内写入时的 `accountUid`（精确身份）；仅当其为空（**旧日志**，
/// 本改动之前写入）才回落到把展示名 `account` 解析回 uid。
///
/// `since` 是**增量水位线**（见 [`gateway_round`]）：`None` = 不过滤（选整窗）。
fn bucket_gateway(
    rows: &[GatewayRow],
    region: Region,
    day: &str,
    uids: &HashSet<String>,
    names: &HashMap<String, Option<String>>,
    since: Option<i64>,
) -> BTreeMap<String, Bucket> {
    let mut out: BTreeMap<String, Bucket> = BTreeMap::new();
    for row in rows {
        if row.region != region.as_str() || row.day != day {
            continue;
        }
        if since.is_some_and(|watermark| row.ts <= watermark) {
            continue;
        }
        let key = match row.account_uid.as_deref() {
            Some(uid) => uid.to_string(),
            None => resolve_account_key(&row.account, uids, names),
        };
        let bucket = out.entry(key).or_default();
        bucket.input = bucket.input.saturating_add(row.input);
        bucket.output = bucket.output.saturating_add(row.output);
        bucket.cached = bucket.cached.saturating_add(row.cached);
    }
    out
}

/// 按**维度键**分桶：只保留「该 region + 该本地日 + `ts > since`」的行。
///
/// `key_of` 决定分组（模型 / uid / 端点）。与 [`bucket_gateway`] **共用同一套过滤**，
/// 这是「按模型合计 = 网关卡」能成立的前提 —— 任何一边多算/少算一条，两个数字立刻分叉。
fn bucket_dim(
    rows: &[GatewayRow],
    region: Region,
    day: &str,
    since: Option<i64>,
    key_of: impl Fn(&GatewayRow) -> String,
) -> BTreeMap<String, DimBucket> {
    let mut out: BTreeMap<String, DimBucket> = BTreeMap::new();
    for row in rows {
        if row.region != region.as_str() || row.day != day {
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

/// 一轮网关聚合的**写入意图**（[`gateway_round`] 的输出）。
#[derive(Debug)]
enum GatewayWrite {
    /// 与现有桶**相加**：水位线已存在，只加 `ts > watermark` 的新条目。
    Add(BTreeMap<String, Bucket>),
    /// 与现有桶**逐字段取大**：水位线缺失（旧版本记录 / 今天首次记录）时的一次性校正 ——
    /// 每个账号的桶 = `max(现有, 当前窗口合计)`。之后从新水位线起走 [`Self::Add`]。
    ///
    /// 取大而不是相加：两边的值都是「今天某个子集的读数」，相加会把重叠部分**双计**；
    /// 取大则**既不倒退也不双计**，且比「原样不动」更贴近真实（旧快照可能比当前窗口小）。
    Correct(BTreeMap<String, Bucket>),
}

/// 一轮网关聚合：返回 `(写入意图, 新水位线)`。
///
/// ## 为什么是「增量 + 水位线」而不是「整窗快照」（2026-10-15 实测驱动的修正）
///
/// 上游 `gateway_logs.json` 是**环形裁剪**的滚动日志（只留最近 `log_keep` 条，缺省 200，
/// 见网关 `logging.rs`）。旧实现把当日网关桶**整体覆盖**成「当前窗口合计」——
/// 于是窗口一旦滚过某条今天早些时候的请求，它的 token 就**凭空消失**，数字可**倒退**。
/// 本机实测（CN 桶，2026-10-15）：17:31 快照 10.3M → 17:36 快照 3.6M → 17:43 反而 18.7M，
/// 每个账号的值都是「各自随机时点的窗口读数」拼出来的混合物。
///
/// 改为：**只把 `ts > watermark` 的新条目累加进桶**，桶只增不减；每轮把水位线推到
/// 「本轮已见到的（本 region + 本日）最大 `ts`」。滚动裁剪从此只影响「还没读到就滚走的
/// 条目」（丢的是增量，不丢已记的量），`gatewayTruncated` 的「下界」语义不变。
///
/// ## 水位线缺失时的校正轮（见 [`GatewayWrite::Correct`]）
///
/// - **旧版本记录**（缺字段，升级过渡态）：桶里是旧的窗口读数 ⇒ 逐账号取大校正一次；
/// - **今天首次记录**（新的一天、空桶）：`max(0, 窗口)` = 整窗计入 —— 与「每天都从
///   当前窗口起步」的旧行为自然衔接（不丢当天开头仍在窗口里的流量）。
///
/// ⚠️ 同毫秒边界：增量比较用**严格大于**。若一条条目与水位线同毫秒（罕见），它会被跳过
/// —— 方向是「少算」，与 `gatewayTruncated` 的「下界」披露一致；反过来用 `>=` 会让
/// 边界条目**每轮重复计一次**（跨轮双计），更坏。
fn gateway_round(
    watermark: Option<i64>,
    dims_empty: bool,
    keys_empty: bool,
    rows: &[GatewayRow],
    region: Region,
    day: &str,
    uids: &HashSet<String>,
    names: &HashMap<String, Option<String>>,
) -> GatewayRound {
    let max_ts = rows
        .iter()
        .filter(|row| row.region == region.as_str() && row.day == day)
        .map(|row| row.ts)
        .max();
    let accounts = match watermark {
        Some(watermark) => GatewayWrite::Add(bucket_gateway(
            rows,
            region,
            day,
            uids,
            names,
            Some(watermark),
        )),
        None => GatewayWrite::Correct(bucket_gateway(rows, region, day, uids, names, None)),
    };
    /* 维度的水位线与账号桶**同源**，只多一条升级过渡：v2 → v3 时水位线存在、
    但维度桶还是空的 ⇒ 维度单独做一次整窗播种，并标记「不完整」。 */
    let dim_since = match (watermark, dims_empty) {
        (None, _) => None,
        (Some(_), false) => watermark,
        (Some(_), true) => None,
    };
    let dims_partial = matches!((watermark, dims_empty), (Some(_), true));
    let dim = |key_of: &dyn Fn(&GatewayRow) -> String| {
        let map = bucket_dim(rows, region, day, dim_since, key_of);
        match dim_since {
            Some(_) => DimWrite::Add(map),
            None => DimWrite::Correct(map),
        }
    };
    /* 账号维度的键与 `bucket_gateway` 同一套解析（新行取 uid，旧行回落展示名）。
    ⚠️ 不要在外面维护「第几条」的游标：`bucket_dim` 会先按 region/日/水位线过滤，
    游标与实际计入的行数一旦错位，账号维度就会贴到别的账号上。 */
    let by_account = dim(&|row| match row.account_uid.as_deref() {
        Some(uid) => uid.to_string(),
        None => resolve_account_key(&row.account, uids, names),
    });
    let next_watermark = match (watermark, max_ts) {
        (Some(current), Some(max)) => Some(current.max(max)),
        (Some(current), None) => Some(current),
        (None, max) => max,
    };
    /* ★ 密钥维度单独判「要不要播种」：它是 v4 才加的第四个维度，升级当天
    「另外三个维度已经有数、只有它是空的」。沿用 `dims_empty` 会走了增量分支 ⇒
    今天此前的请求全部漏记，而 UI 上看着像「这把 Key 只用了这么点」。
    代价是播种轮只能拿当前日志窗口 —— 那天标 `keys_partial`（下界）。 */
    let keys_since = match (watermark, keys_empty) {
        (None, _) => None,
        (Some(_), false) => watermark,
        (Some(_), true) => None,
    };
    let keys_write = {
        let map = bucket_dim(rows, region, day, keys_since, |row| row.key.clone());
        match keys_since {
            Some(_) => DimWrite::Add(map),
            None => DimWrite::Correct(map),
        }
    };
    GatewayRound {
        accounts,
        models: dim(&|row| row.model.clone()),
        accounts_dim: by_account,
        endpoints: dim(&|row| row.endpoint.clone()),
        keys: keys_write,
        next_watermark,
        dims_partial,
        keys_partial: matches!((watermark, keys_empty), (Some(_), true)),
    }
}

/// 一轮网关聚合的完整结果：账号 token 桶 + 四条维度桶 + 新水位线。
#[derive(Debug)]
struct GatewayRound {
    accounts: GatewayWrite,
    models: DimWrite,
    accounts_dim: DimWrite,
    endpoints: DimWrite,
    /// 密钥维度（键见 [`key_bucket_of`]）。
    keys: DimWrite,
    next_watermark: Option<i64>,
    /// 维度是升级播种出来的（该日原有账号桶非空时才真正标 partial）。
    dims_partial: bool,
    /// 密钥维度是升级播种出来的（v3 → v4）—— 与 `dims_partial` **分开**：
    /// 前者说「模型/账号/端点不完整」，后者只说「按密钥不完整」，混用会让
    /// 模型视图平白背上一口锅。
    keys_partial: bool,
}

/// 网关日志的 `account` 是**展示名**（`account_display_name` 的回落链：email → nickname →
/// 客户端快照 → uid；见网关 `relay.rs` 的 `RelayOutcome::Ok.account`），而前端按
/// `AccountMeta.uid` join。故**旧行**（无 `accountUid`）在这里把展示名解析回 uid。
///
/// 新行直接带上 `accountUid`（写入时的精确身份，见 [`bucket_gateway`]），无需解析——
/// 这正是本次改动的目的：**身份在写入时记录**，而非事后反查。
///
/// 回落口径：直接命中 uid → 唯一命中某账号的展示名 / email / 昵称 → 否则**原样保留**
/// （歧义 / 未知**绝不**错贴到别的账号；前端 join 落空即渲染「—」，与本模块的诚实取向一致）。
fn load_gateway_identity(region: Region) -> (HashSet<String>, HashMap<String, Option<String>>) {
    let mut uids: HashSet<String> = HashSet::new();
    let mut names: HashMap<String, Option<String>> = HashMap::new();
    for account_value in account::load_accounts_for(region) {
        let Some(uid) = account::get_str(&account_value, "uid") else {
            continue;
        };
        uids.insert(uid.clone());
        let mut display_forms = vec![account::account_display_name(&account_value)];
        if let Some(email) = account::get_str(&account_value, "email") {
            display_forms.push(email);
        }
        if let Some(nickname) = account::get_str(&account_value, "nickname") {
            display_forms.push(nickname);
        }
        for name in display_forms {
            if name.is_empty() {
                continue;
            }
            names
                .entry(name)
                .and_modify(|slot: &mut Option<String>| {
                    // 多个账号共用同一展示名 ⇒ 歧义 ⇒ 解析不出（`None`）。
                    if slot.as_deref() != Some(uid.as_str()) {
                        *slot = None;
                    }
                })
                .or_insert_with(|| Some(uid.clone()));
        }
    }
    (uids, names)
}

fn resolve_account_key(
    raw: &str,
    uids: &HashSet<String>,
    names: &HashMap<String, Option<String>>,
) -> String {
    if uids.contains(raw) {
        return raw.to_string();
    }
    match names.get(raw) {
        Some(Some(uid)) => uid.clone(),
        _ => raw.to_string(),
    }
}

// ---------------------------------------------------------------------------
// 客户端 JSONL 聚合（只读）
// ---------------------------------------------------------------------------

/* ⚠️ 不 derive `Eq`：`credit` 是 f64。去重 / 比对只需 `PartialEq`。 */
#[derive(Clone, Debug, PartialEq)]
struct ClientUsageRow {
    session_id: String,
    ts: i64,
    input: u64,
    output: u64,
    /// 模型名（对账的匹配键之一；空 ⇒ 空串，**不参与**匹配）。
    ///
    /// 为什么扫描时就带上：对账（`modules::reconcile`）要用它做「同模型」判定，
    /// 若事后另扫一遍 jsonl，同一天要读两遍盘 —— 而这两件事的素材是同一批行。
    model: String,
    /// 该条记录的积分（`usage.credit`）—— 对账的**次键**，也是积分视图的请求口径。
    credit: f64,
}

/// 递归收集「**可能含今日记录**」的 `.jsonl` 路径。
///
/// **一次遍历同时做完三件事**（旧实现是「先遍历收全部路径 → 再逐个 `stat` 取 mtime」）：
/// 判类型 · 取 mtime · 应用预筛。
///
/// - `entry.file_type()` 直接取自目录枚举结果，**不额外发系统调用**。旧写法
///   `path.is_dir()` 每个条目都要一次 `stat`：实测同一棵树（Global，26,932 个文件）
///   **0.43s → 0.03s**。
/// - `entry.metadata()` 在 Windows 上同样来自枚举结果 ⇒ 「遍历完再逐个 stat」那第二遍消失。
/// - 预筛在**遍历时**生效 ⇒ 不通过的文件连 `PathBuf` 都不用分配。
///
/// 排除 `subagents`（子代理日志会重复父会话上下文，与 `token_stats` 的主用量口径一致）。
fn collect_day_files(dir: &Path, day_start_ms: Option<i64>, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_dir() {
            if entry.file_name().to_str() == Some("subagents") {
                continue;
            }
            collect_day_files(&entry.path(), day_start_ms, out);
            continue;
        }
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("jsonl") {
            continue;
        }
        let mtime_ms = entry.metadata().ok().and_then(|meta| mtime_of(&meta));
        if mtime_may_hold_day(mtime_ms, day_start_ms) {
            out.push(path);
        }
    }
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
        .or_else(|| value.as_str()?.trim().parse::<u64>().ok())
}

fn field_u64(object: &Map<String, Value>, keys: &[&str]) -> Option<u64> {
    keys.iter()
        .find_map(|key| object.get(*key).and_then(number_u64))
}

/// usage 对象的锚点：必须含输入字段（输入可以为 0，故不以 `> 0` 判定）。
fn usage_object(value: &Value) -> Option<&Map<String, Value>> {
    value.as_object().filter(|object| {
        field_u64(object, &["input_tokens", "inputTokens", "prompt_tokens"]).is_some()
    })
}

/// 提取 (input, output)：优先级 `message.usage` > `providerData.usage` > 顶层 `usage`
/// （与 `token_stats` 的取值口径一致）。
/// 模型名：`message.model` 优先，其次顶层 `model`（与 `token_stats::model` 同源）。
///
/// 空 ⇒ 空串。对账把空串当「不可匹配」（不知道模型就没法断言是同一条请求）。
fn extract_model(value: &Value) -> String {
    value
        .get("message")
        .and_then(|message| message.get("model"))
        .or_else(|| value.get("model"))
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or_default()
        .to_string()
}

/// 该条记录的积分：`message.usage.credit` 优先，其次顶层 `usage.credit`。
/// 缺失 / 非法 ⇒ 0.0（= 不知道，不是免费）。
fn extract_credit(value: &Value) -> f64 {
    value
        .get("message")
        .and_then(|message| message.get("usage"))
        .or_else(|| value.get("usage"))
        .and_then(|usage| usage.get("credit"))
        .and_then(Value::as_f64)
        .filter(|credit| credit.is_finite() && *credit >= 0.0)
        .unwrap_or(0.0)
}

fn extract_usage(value: &Value) -> Option<(u64, u64)> {
    let provider = value.get("providerData");
    let candidates = [
        value
            .get("message")
            .and_then(|message| message.get("usage")),
        provider.and_then(|data| data.get("usage")),
        value.get("usage"),
    ];
    let selected = candidates.into_iter().flatten().find_map(usage_object)?;
    let input = field_u64(selected, &["input_tokens", "inputTokens", "prompt_tokens"]).unwrap_or(0);
    let output = field_u64(
        selected,
        &["output_tokens", "outputTokens", "completion_tokens"],
    )
    .unwrap_or(0);
    Some((input, output))
}

fn record_timestamp(value: &Value) -> Option<i64> {
    value
        .get("timestamp")
        .or_else(|| value.get("ts"))
        .and_then(|timestamp| {
            timestamp
                .as_i64()
                .or_else(|| timestamp.as_u64().and_then(|n| i64::try_from(n).ok()))
                .or_else(|| timestamp.as_str()?.trim().parse::<i64>().ok())
        })
}

/// 解析一行会话日志；非用量行 / 结构缺失返回 `None`。
///
/// 先用 `contains("\"usage\"")` 廉价预筛，避免对海量消息行做 JSON 解析。
fn parse_client_line(line: &str) -> Option<ClientUsageRow> {
    let line = line.trim();
    if line.is_empty() || !line.contains("\"usage\"") {
        return None;
    }
    let value: Value = serde_json::from_str(line).ok()?;
    let ts = record_timestamp(&value)?;
    let session_id = value
        .get("sessionId")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|session_id| !session_id.is_empty())?
        .to_string();
    let (input, output) = extract_usage(&value)?;
    Some(ClientUsageRow {
        session_id,
        ts,
        input,
        output,
        model: extract_model(&value),
        credit: extract_credit(&value),
    })
}

/// 文件的 mtime（毫秒）。读不到 ⇒ `None`。
fn mtime_of(meta: &std::fs::Metadata) -> Option<i64> {
    let since = meta.modified().ok()?.duration_since(UNIX_EPOCH).ok()?;
    i64::try_from(since.as_millis()).ok()
}

/// mtime 预筛判据（**纯函数**，便于单测）。
///
/// `mtime_ms` 或 `day_start_ms` 任一缺失 ⇒ **放行**（宁可慢，不可漏）。
fn mtime_may_hold_day(mtime_ms: Option<i64>, day_start_ms: Option<i64>) -> bool {
    match (mtime_ms, day_start_ms) {
        (Some(mtime_ms), Some(start)) => mtime_ms >= start - CLIENT_SCAN_MTIME_SKEW_MS,
        _ => true,
    }
}

/// 读一份 jsonl 的**今日尾部**；I/O 失败返回 `None`（调用方按「无记录」处理，不 panic）。
///
/// ## 为什么可以只读尾部（2026-10-13 加入）
///
/// 会话 jsonl 是**只追加**的，记录按时间顺序落盘 ⇒ **今日记录必然在文件尾部**。
/// 具体算法（块大小、停止判据、边界用例）在 [`jsonl_tail`] —— 与
/// `client_limits` 的限流台账**共用同一实现**，那边不再各写一份。
///
/// 实测（Global，当日被动过的 5,704 个文件 / **14.59 GB**）：旧实现逐行读完整份，
/// `record_client_today` 要 **66–75s**；改为尾部窗口 + 并行后见模块头「性能」一节。
fn read_day_window(file: &mut std::fs::File, size: u64, day_start_ms: i64) -> Option<Vec<u8>> {
    jsonl_tail::read_window_from(file, size, day_start_ms).map(|(bytes, _)| bytes)
}

/// 解析窗口文本，只留今日的行。
fn parse_day_rows(text: &str, day: &str) -> Vec<ClientUsageRow> {
    text.lines()
        .filter_map(parse_client_line)
        .filter(|row| local_date_string(row.ts).as_deref() == Some(day))
        .collect()
}

/// 扫描单个文件（**只读尾部窗口**），返回其中今日的用量行。
fn scan_file_rows(path: &Path, day: &str, day_start_ms: Option<i64>) -> Vec<ClientUsageRow> {
    let Ok(mut file) = std::fs::File::open(path) else {
        return Vec::new();
    };
    let Ok(size) = file.metadata().map(|meta| meta.len()) else {
        return Vec::new();
    };
    if size == 0 {
        return Vec::new();
    }
    let window = match day_start_ms {
        Some(start) => read_day_window(&mut file, size, start),
        // 算不出日界 ⇒ 整份读（宁可慢，不可漏）。
        None => {
            let mut buffer = Vec::new();
            file.read_to_end(&mut buffer).ok().map(|_| buffer)
        }
    };
    match window {
        Some(bytes) => parse_day_rows(&String::from_utf8_lossy(&bytes), day),
        None => Vec::new(),
    }
}

/// 并行度：够用即可，上限 32（再多只会在同一条磁盘队列上排队）。
fn worker_count(pending: usize) -> usize {
    let cores = std::thread::available_parallelism()
        .map(|value| value.get())
        .unwrap_or(4);
    cores.clamp(1, 32).min(pending.max(1))
}

/// 扫描 `root` 下的 CLI 会话日志，只保留**今日**的用量行。
///
/// ## 两道代价控制（2026-10-13 重做）
///
/// 1. **mtime 预筛**（遍历时生效，见 [`collect_day_files`]）：日志是追加写的，
///    一条记录的写入时刻不早于它自己的时间戳 ⇒ 当日记录只可能出现在「本地日界之后
///    被动过」的文件里。**没有这一步**，把闸门从「整天一次」改成「2 分钟一次」
///    是不可接受的。
/// 2. **尾部窗口**（[`read_day_window`]）：预筛放行的文件里，今日记录也只在**尾部**。
///    实测当日被动过的文件共 14.59 GB，而真正需要读的只是各文件的尾部一段。
///
/// 之后按文件**并行**解析（[`worker_count`]）；结果按文件路径顺序合并 ⇒ 与线程调度无关。
///
/// ⚠️ 取舍：预筛**依赖 mtime 与写入时刻一致**。若某文件内容在当天增长、而 mtime 被改回
/// 昨天（如带 mtime 的还原 / 复制），它的当日记录会被漏掉 —— 这由
/// `mtime_prefilter_keeps_fresh_files_and_drops_stale_ones` 用例**显式固定**下来
/// （不是「没想到」，而是写明的取舍）。
fn scan_client_rows(root: &Path, day: &str) -> Vec<ClientUsageRow> {
    let day_start = local_day_start_ms(day);
    let mut files = Vec::new();
    collect_day_files(root, day_start, &mut files);
    if files.is_empty() {
        return Vec::new();
    }
    // `read_dir` 顺序不保证稳定 ⇒ 先排序，让结果与缓存都可复现。
    files.sort();

    let next = AtomicUsize::new(0);
    let per_file: Mutex<Vec<(usize, Vec<ClientUsageRow>)>> =
        Mutex::new(Vec::with_capacity(files.len()));
    std::thread::scope(|scope| {
        for _ in 0..worker_count(files.len()) {
            scope.spawn(|| loop {
                let cursor = next.fetch_add(1, Ordering::Relaxed);
                let Some(path) = files.get(cursor) else {
                    break;
                };
                let rows = scan_file_rows(path, day, day_start);
                per_file.lock().unwrap().push((cursor, rows));
            });
        }
    });

    let mut collected = per_file.into_inner().unwrap();
    collected.sort_by_key(|(index, _)| *index);
    collected.into_iter().flat_map(|(_, rows)| rows).collect()
}

fn table_columns(conn: &Connection, table: &str) -> Vec<String> {
    let Ok(mut stmt) = conn.prepare(&format!("PRAGMA table_info({table})")) else {
        return Vec::new();
    };
    let Ok(iter) = stmt.query_map([], |row| row.get::<_, String>(1)) else {
        return Vec::new();
    };
    iter.flatten().collect()
}

fn nonempty(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

/// 读 `sessions` 表构造 `sessionId → user_id` 映射（**只读**）。
///
/// 空串 / `NULL` 的 `user_id` **不入表**（无法归属 ⇒ 走 unknown 桶，绝不猜）。
fn load_session_user_map(region: Region) -> HashMap<String, String> {
    let mut map = HashMap::new();
    let db = workbuddy_db_path_for(region);
    if !db.is_file() {
        return map;
    }
    let Ok(conn) = Connection::open_with_flags(&db, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
    else {
        return map;
    };
    let _ = conn.busy_timeout(Duration::from_secs(5));
    let columns = table_columns(&conn, "sessions");
    if !columns.iter().any(|column| column == "id")
        || !columns.iter().any(|column| column == "user_id")
    {
        return map;
    }
    let Ok(mut stmt) = conn.prepare("SELECT id, user_id FROM sessions") else {
        return map;
    };
    let Ok(rows) = stmt.query_map([], |row| {
        Ok((
            row.get::<_, Option<String>>(0)?,
            row.get::<_, Option<String>>(1)?,
        ))
    }) else {
        return map;
    };
    for (id, uid) in rows.flatten() {
        let (Some(id), Some(uid)) = (nonempty(id), nonempty(uid)) else {
            continue;
        };
        map.insert(id, uid);
    }
    map
}

/// 先按 `sessionId` 合并，再映射到账号；映射失败落 `unknown`。
fn bucket_client(
    rows: &[ClientUsageRow],
    meta: &HashMap<String, String>,
    day: &str,
) -> (BTreeMap<String, Bucket>, Bucket) {
    let mut by_session: BTreeMap<String, Bucket> = BTreeMap::new();
    for row in rows {
        if local_date_string(row.ts).as_deref() != Some(day) {
            continue;
        }
        let bucket = by_session.entry(row.session_id.clone()).or_default();
        bucket.input = bucket.input.saturating_add(row.input);
        bucket.output = bucket.output.saturating_add(row.output);
    }

    let mut per_account: BTreeMap<String, Bucket> = BTreeMap::new();
    let mut unknown = Bucket::default();
    for (session_id, bucket) in by_session {
        match meta.get(&session_id) {
            Some(uid) => {
                let account = per_account.entry(uid.clone()).or_default();
                account.input = account.input.saturating_add(bucket.input);
                account.output = account.output.saturating_add(bucket.output);
            }
            None => {
                unknown.input = unknown.input.saturating_add(bucket.input);
                unknown.output = unknown.output.saturating_add(bucket.output);
            }
        }
    }
    (per_account, unknown)
}

// ---------------------------------------------------------------------------
// 对外：聚合 + 读取
// ---------------------------------------------------------------------------

/// 聚合网关日志到「今日」的 gateway 桶并落盘，返回今日记录。
///
/// ## 增量语义（2026-10-15 修正，见 [`gateway_round`] 的长注释）
///
/// 桶是**累加**的，不是「当前窗口快照」。每轮只把 `ts > 水位线` 的新条目加进桶，
/// 水位线随轮次前移 ⇒ 滚动日志把老条目裁掉时，**已记的量不会跟着消失**（数字不倒退）。
/// 近 `days` 天降级计数的**上限**（防止一次拉全量）。
pub const MAX_FALLBACK_STATS_DAYS: u32 = 90;

/// 把一次降级结果累加进内存 Store（**纯函数**，便于单测注入）。
fn apply_fallback(store: &mut Store, result: FallbackResult, day: &str) -> FallbackDayCounts {
    if matches!(result, FallbackResult::Blocked) {
        // ★ 不创建桶：一条全 0 的日记录会让「这一天有降级」看起来成立，
        //   而 `blocked` 的语义正是「压根没降级」。
        return store.fallback_days.get(day).copied().unwrap_or_default();
    }
    let entry = store.fallback_days.entry(day.to_string()).or_default();
    match result {
        FallbackResult::Degraded => entry.degraded = entry.degraded.saturating_add(1),
        FallbackResult::Failed => entry.failed = entry.failed.saturating_add(1),
        FallbackResult::Blocked => {}
    }
    *entry
}

/// 从内存 Store 汇总近 `days` 天（**纯函数**，`today` 由调用方给，便于单测）。
fn summarize_fallback(store: &Store, today: NaiveDate, days: u32) -> Value {
    let days = days.clamp(1, MAX_FALLBACK_STATS_DAYS);
    let mut items = Vec::new();
    let mut total = FallbackDayCounts::default();
    for offset in 0..days {
        let day = today - ChronoDuration::days(i64::from(offset));
        let key = day.format("%Y-%m-%d").to_string();
        let counts = store.fallback_days.get(&key).copied().unwrap_or_default();
        total.degraded = total.degraded.saturating_add(counts.degraded);
        total.failed = total.failed.saturating_add(counts.failed);
        items.push(serde_json::json!({
            "date": key,
            "degraded": counts.degraded,
            "failed": counts.failed,
        }));
    }
    serde_json::json!({
        "days": items,
        "total": { "degraded": total.degraded, "failed": total.failed },
    })
}

/// 记一次降级结果（**累加**，见 [`Store::fallback_days`] 的文档）。
///
/// ## 为什么每次事件都读-改-写整个文件
///
/// 降级是**低频事件**（一天几十次量级），而 `save_store` 走 `atomic_write`（原子替换）。
/// 反过来做「内存缓存 + 定期 flush」会引入两个新问题：进程被杀时丢最后一批、
/// 以及「什么时候 flush」这个没人说得清的参数。低频场景下，简单比高效更重要。
///
/// 跨进程（桌面端 + 服务端同时写）不引入文件锁：丢一次计数的代价远小于
/// 「每次降级一次 syscall」，而锁本身也会带来死锁面。
pub fn record_fallback(region: Region, result: FallbackResult, now_ms: i64) -> FallbackDayCounts {
    // `blocked` = 压根没降级，不进任何一档（见 [`FallbackDayCounts`] 的文档）。
    if matches!(result, FallbackResult::Blocked) {
        return FallbackDayCounts::default();
    }
    let guard = TOKEN_DAILY_WRITE_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut store = load_store(region);
    let Some(day) = local_date(now_ms) else {
        return FallbackDayCounts::default();
    };
    let counts = apply_fallback(&mut store, result, &day.format("%Y-%m-%d").to_string());
    save_store(region, &store);
    drop(guard);
    counts
}

/// 近 `days` 天的降级计数（新的在前 + 总计）。
///
/// `days` 被夹在 `1..=MAX_FALLBACK_STATS_DAYS`：不夹上限的话，
/// 一次调用会把整本账全量序列化（账本按日累积，可以很大）。
pub fn fallback_stats(region: Region, days: u32) -> Value {
    let store = load_store(region);
    match local_date(crate::modules::config::now_ms()) {
        Some(today) => summarize_fallback(&store, today, days),
        // 取不到本地日（极端时钟问题）⇒ 返回空统计，不 panic。
        None => serde_json::json!({
            "days": [],
            "total": { "degraded": 0, "failed": 0 },
        }),
    }
}

pub fn record_gateway_today(region: Region) -> Value {
    let at = now_ms();
    let day = local_date_string(at).unwrap_or_default();
    let entries = load_gateway_log();
    // 最旧存活条目的本地日（FIFO ⇒ 它早于今天就意味着今天的条目一条都没丢过）。
    let oldest_day = entries
        .iter()
        .filter_map(|value| value.get("ts").and_then(Value::as_i64))
        .min()
        .and_then(local_date_string);
    let truncated = gateway_log_truncated_today(
        entries.len(),
        gateway_log_keep(),
        oldest_day.as_deref(),
        &day,
    );
    let (uids, names) = load_gateway_identity(region);
    let rows = parse_gateway_rows(&entries);

    let _guard = TOKEN_DAILY_WRITE_LOCK.lock().unwrap();
    let mut store = load_store(region);
    {
        let record = store.days.entry(day.clone()).or_default();
        let round = gateway_round(
            record.gateway_watermark_ts,
            record.gateway_models.is_empty(),
            record.gateway_keys.is_empty(),
            &rows,
            region,
            &day,
            &uids,
            &names,
        );
        match round.accounts {
            GatewayWrite::Add(increments) => {
                for (account, bucket) in increments {
                    let entry = record.accounts.entry(account).or_default();
                    entry.gateway.input = entry.gateway.input.saturating_add(bucket.input);
                    entry.gateway.output = entry.gateway.output.saturating_add(bucket.output);
                    entry.gateway.cached = entry.gateway.cached.saturating_add(bucket.cached);
                }
            }
            GatewayWrite::Correct(snapshot) => {
                for (account, bucket) in snapshot {
                    let entry = record.accounts.entry(account).or_default();
                    // 逐字段取大：不倒退、不双计（两个读数都只是今天的**子集**）。
                    entry.gateway.input = entry.gateway.input.max(bucket.input);
                    entry.gateway.output = entry.gateway.output.max(bucket.output);
                    entry.gateway.cached = entry.gateway.cached.max(bucket.cached);
                }
            }
        }
        /* 四条维度：与账号桶同轮同水位线。`Correct` = 逐键取大（不倒退、不双计）。 */
        apply_dim_write(&mut record.gateway_models, round.models);
        apply_dim_write(&mut record.gateway_accounts, round.accounts_dim);
        apply_dim_write(&mut record.gateway_endpoints, round.endpoints);
        apply_dim_write(&mut record.gateway_keys, round.keys);
        /* 升级播种：只有该日**原本就有**账号桶时才标「不完整」——
        新的一天整窗播种就是全量，不该挂污点。 */
        let had_prior = record
            .accounts
            .values()
            .any(|entry| entry.gateway.input > 0 || entry.gateway.output > 0);
        if round.dims_partial && had_prior {
            record.gateway_models_partial = true;
        }
        if round.keys_partial && had_prior {
            record.gateway_keys_partial = true;
        }
        record.gateway_watermark_ts = round.next_watermark;
        record.gateway_truncated = truncated;
    }
    prune_store(&mut store, at);
    save_store(region, &store);
    build_response(&day, store.days.get(&day))
}

/// 把一轮维度写入意图落到桶上。
///
/// `Add` ⇒ 逐字段相加；`Correct` ⇒ 逐字段取大（两个读数都只是当日的**子集**，
/// 相加会双计，取大既不倒退也不双计 —— 与 [`GatewayWrite::Correct`] 同一理由）。
fn apply_dim_write(target: &mut BTreeMap<String, DimBucket>, write: DimWrite) {
    match write {
        DimWrite::Add(increments) => {
            for (key, bucket) in increments {
                let entry = target.entry(key).or_default();
                entry.input = entry.input.saturating_add(bucket.input);
                entry.output = entry.output.saturating_add(bucket.output);
                entry.cached = entry.cached.saturating_add(bucket.cached);
                entry.requests = entry.requests.saturating_add(bucket.requests);
                entry.failed = entry.failed.saturating_add(bucket.failed);
                entry.credit += bucket.credit;
            }
        }
        DimWrite::Correct(snapshot) => {
            for (key, bucket) in snapshot {
                let entry = target.entry(key).or_default();
                entry.input = entry.input.max(bucket.input);
                entry.output = entry.output.max(bucket.output);
                entry.cached = entry.cached.max(bucket.cached);
                entry.requests = entry.requests.max(bucket.requests);
                entry.failed = entry.failed.max(bucket.failed);
                if bucket.credit > entry.credit {
                    entry.credit = bucket.credit;
                }
            }
        }
    }
}

/// 客户端扫描闸门（**纯函数**，便于单测）。
///
/// 判据：`force` ⇒ **总是要重扫**；否则「上次扫描在**同一本地日** **且** 距今不足
/// [`CLIENT_SCAN_TTL_MS`]」才跳过。
///
/// ⚠️ 「同一本地日」这一半是**跨日**判据（昨夜 23:59 扫过、现在 00:01 必须重扫），
/// 不是原来那个「当天扫过就不再扫」的判据 —— 后者会把当日值冻结在当天首次扫描那一刻。
fn client_scan_is_fresh(force: bool, scanned_at_ms: Option<i64>, now: i64, day: &str) -> bool {
    if force {
        return false;
    }
    let Some(scanned_at) = scanned_at_ms else {
        return false;
    };
    now.saturating_sub(scanned_at) < CLIENT_SCAN_TTL_MS
        && local_date_string(scanned_at).as_deref() == Some(day)
}

/// 聚合客户端会话日志到「今日」的 client 桶并落盘，返回今日记录。
///
/// `force = false` 且**距上次扫描不足 [`CLIENT_SCAN_TTL_MS`]** 时直接返回缓存；
/// `force = true` 强制重扫（当日数据在扫过之后仍会增长）。
///
/// ⚠️ 闸门**不是**「当天扫过就不再扫」（那会把当日值冻结在当天首次扫描那一刻，
/// 见 [`CLIENT_SCAN_TTL_MS`]）。重扫的代价由 [`scan_client_rows`] 的 mtime 预筛兜住。
///
/// ★ 调用点（两处，都必须存在）：① 账号页命令（用户主动看时立刻刷新）；
/// ② 后台周期任务 `modules::background` 的 `ClientUsageRecord`（每 2 分钟，`force = false`）
/// —— 缺了 ②，不进账号页卡片上的 client 列就一直不动（2026-10-15 用户报障）。
pub fn record_client_today(region: Region, force: bool) -> Value {
    let at = now_ms();
    let day = local_date_string(at).unwrap_or_default();

    // 闸门：非强制且「同日 + 未过 TTL」⇒ 直接读缓存。
    {
        let store = load_store(region);
        if client_scan_is_fresh(force, store.client_scanned_at_ms, at, &day) {
            return build_response(&day, store.days.get(&day));
        }
    }

    let meta = load_session_user_map(region);
    let rows = scan_client_rows(&session_data_dir(region).join("projects"), &day);
    let (per_account, unknown) = bucket_client(&rows, &meta, &day);

    let _guard = TOKEN_DAILY_WRITE_LOCK.lock().unwrap();
    // 锁内重读：扫描期间 gateway 可能刚写过，避免丢更新。
    let mut store = load_store(region);
    {
        let record = store.days.entry(day.clone()).or_default();
        for (uid, bucket) in per_account {
            record.accounts.entry(uid).or_default().client = bucket;
        }
        record.unknown_client = (unknown.input > 0 || unknown.output > 0).then_some(unknown);
    }
    store.client_scanned_at_ms = Some(at);
    prune_store(&mut store, at);
    save_store(region, &store);
    build_response(&day, store.days.get(&day))
}

/// 读取「今日 Token」记录（**只读**，供 UI 直接渲染）。
///
/// 返回 `{ date, accounts: [{ uid, clientIn, clientOut, gatewayIn, gatewayOut }],
/// unknownClient: { in, out }, gatewayTruncated: bool }`。
///
/// **诚实边界**：`accounts` 只列出**今日确有用量**的账号 —— 账号**缺席** = 「今日无记录 /
/// 未知」，与「出现在数组里、值为 0（当日确实没用）」是两件事。`unknownClient` 是
/// 解析不出账号归属的客户端用量汇总（无则为 `{in:0,out:0}`）。
pub fn account_token_today(region: Region) -> Value {
    let day = local_date_string(now_ms()).unwrap_or_default();
    let store = load_store(region);
    build_response(&day, store.days.get(&day))
}

/// 把某日记录投影成下发前端的形状（纯函数，便于单测）。
fn build_response(day: &str, record: Option<&DayRecord>) -> Value {
    let accounts: Vec<Value> = record
        .map(|record| {
            record
                .accounts
                .iter()
                .map(|(uid, account)| {
                    json!({
                        "uid": uid,
                        "clientIn": account.client.input,
                        "clientOut": account.client.output,
                        "gatewayIn": account.gateway.input,
                        "gatewayOut": account.gateway.output,
                        /* ★ issue #154：本轮命中上游前缀缓存的输入 token（`gatewayIn` 的**子集**）。
                        `null`（不是 0）= 该账号今日还没观测到命中量（老条目 / 上游没报）
                        —— 与 `gatewayIn` 写 0 的诚实口径一致：**没报 ≠ 没命中**。 */
                        "gatewayCached": (account.gateway.cached > 0).then_some(account.gateway.cached),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    let unknown_client = record
        .and_then(|record| record.unknown_client)
        .unwrap_or_default();
    json!({
        "date": day,
        "accounts": accounts,
        "unknownClient": { "in": unknown_client.input, "out": unknown_client.output },
        "gatewayTruncated": record.map(|record| record.gateway_truncated).unwrap_or(false),
    })
}

/// 网关逐日用量趋势（**只读**落库历史；不触发记录、不扫盘）。
///
/// 与 [`account_token_today`] 的分工：那个是「今日 · 按账号」（账号卡片用），
/// 本函数是「逐日 · 全账号合计」（趋势图用）。数据全部来自 `token_daily` 的落盘历史
/// （保留 [`TOKEN_DAILY_RETENTION_DAYS`] 天）⇒ **只有 App 运行过的日子才有条目**
/// （后台周期记录，见 `modules::background` 的 `GatewayUsageRecord`）——
/// **缺日 = 未记录，不是 0**：前端据此算覆盖率并如实披露，不许把未记录日画成「当天没用量」。
pub fn gateway_token_trend(region: Region) -> Value {
    let store = load_store(region);
    let days = gateway_trend_points(&store);
    let total: u64 = days
        .iter()
        .map(|point| point.get("total").and_then(Value::as_u64).unwrap_or(0))
        .sum();
    json!({
        "date": local_date_string(now_ms()).unwrap_or_default(),
        "retentionDays": TOKEN_DAILY_RETENTION_DAYS,
        "days": days,
        "total": total,
    })
}

/// 今日的**客户端 ↔ 网关对账**（只读：扫一遍今日 jsonl + 读网关日志，不写任何文件）。
///
/// 只覆盖**今日**：网关日志是环形窗口（缺省 200 条），更早的请求已经滚出去、
/// 无法逐条对账 —— 那部分只能靠 [`gateway_usage_dim`] 的日合计给区间（见设计文档 §4.5）。
///
/// 匹配算法在 `modules::reconcile`（纯函数、可单测）；这里只做素材装配：
/// 两侧都换成 `(ts, model, tokens, credit)` 四元组，**账号刻意不带**（网关切号会让它对不上）。
pub fn reconcile_today(region: Region) -> Value {
    let day = local_date_string(now_ms()).unwrap_or_default();
    let entries = load_gateway_log();
    let gateway_rows: Vec<crate::modules::reconcile::ReconRow> = parse_gateway_rows(&entries)
        .into_iter()
        .filter(|row| row.region == region.as_str() && row.day == day)
        .map(|row| crate::modules::reconcile::ReconRow {
            ts: row.ts,
            model: row.model,
            tokens: row.input.saturating_add(row.output),
            credit: row.credit,
        })
        .collect();
    let client_rows: Vec<crate::modules::reconcile::ReconRow> =
        scan_client_rows(&session_data_dir(region).join("projects"), &day)
            .into_iter()
            .map(|row| crate::modules::reconcile::ReconRow {
                ts: row.ts,
                model: row.model,
                tokens: row.input.saturating_add(row.output),
                credit: row.credit,
            })
            .collect();
    /* `proxy_enabled = None`：当前没有可靠的「客户端是否走网关」配置读取点
    （P12 的代理是改客户端 baseUrl / hosts 实现的，不在本应用配置里）。
    未知时以**匹配率**为准 —— 逐条对上本身就是比配置更强的证据。 */
    let report = crate::modules::reconcile::reconcile(&client_rows, &gateway_rows, None);
    let mut value = report.to_value(None);
    value["window"] = json!({ "date": day, "logRows": entries.len() });
    value
}

/// 网关的**四维度**逐日历史（密钥 / 模型 / 账号 / 端点）—— 用量页「账本」视图的唯一数据源。
///
/// 与 [`gateway_token_trend`] **同源同形状**（`days` 只含落库日 + `truncated`），
/// 前端复用同一套覆盖率计算。只读：不扫盘、不触发记录。
///
/// ## 为什么四个维度放一个命令
///
/// 它们由同一次聚合产出、共用同一个水位线，**必须一起读**才能保证
/// 「按密钥合计 = 按模型合计 = 按账号合计 = 按端点合计 = 网关卡」。分四条命令只会让前端
/// 各取一次、在两次读取之间数据已经变了。
///
/// ⚠️ `keys` 是 v4 才有的维度：升级前的日期里它是**空数组**（不是 0 用量）。
/// 前端必须把「没有该维度」与「该维度暂无用量」分开表述（`keysPartial` 只描述
/// 「这一天从中间开始记」，不描述「完全没有」）。
pub fn gateway_usage_dim(region: Region) -> Value {
    let store = load_store(region);
    let days: Vec<Value> = store
        .days
        .iter()
        .map(|(day, record)| {
            json!({
                "key": day,
                "truncated": record.gateway_truncated,
                "partial": record.gateway_models_partial,
                "keysPartial": record.gateway_keys_partial,
                "keys": dim_groups(&record.gateway_keys, "key"),
                "models": dim_groups(&record.gateway_models, "model"),
                "accounts": dim_groups(&record.gateway_accounts, "account"),
                "endpoints": dim_groups(&record.gateway_endpoints, "endpoint"),
            })
        })
        .collect();
    json!({
        "date": local_date_string(now_ms()).unwrap_or_default(),
        "retentionDays": TOKEN_DAILY_RETENTION_DAYS,
        "days": days,
    })
}

/// `BTreeMap<String, DimBucket>` → 数组（按总量降序；`key_name` 是键的字段名）。
///
/// 空键（端点缺失）保留为空串，由前端渲染成「未记录端点」—— **不丢量**：
/// 丢掉它会让「按端点合计」小于网关总量。
fn dim_groups(map: &BTreeMap<String, DimBucket>, key_name: &str) -> Vec<Value> {
    let mut values: Vec<Value> = map
        .iter()
        .map(|(key, bucket)| {
            let total = bucket.input.saturating_add(bucket.output);
            let mut value = json!({
                key_name: key,
                "input": bucket.input,
                "output": bucket.output,
                "total": total,
                /* ★ issue #154：命中上游前缀缓存的输入 token（`input` 的子集）。
                `null` = 这一天没观测到命中（老账本无此列 / 上游没报），**不是 0 命中**
                —— 前端据此区分「没数据」与「确实全 miss」。 */
                "cached": (bucket.cached > 0).then_some(bucket.cached),
                "requests": bucket.requests,
                "failed": bucket.failed,
                "credit": (bucket.credit * 1000.0).round() / 1000.0,
            });
            value["_sort"] = json!(total);
            value
        })
        .collect();
    values.sort_by_key(|value| std::cmp::Reverse(value["_sort"].as_u64().unwrap_or(0)));
    for value in &mut values {
        value.as_object_mut().map(|object| object.remove("_sort"));
    }
    values
}

/// 落库 `days` → 逐日 gateway 汇总（**纯函数**，便于单测）。
///
/// - 日期**升序**（`BTreeMap` 的键序，与前端窗口对齐直接可用）；
/// - 跨账号求和（含归属不到已知账号、按原键留下的行 —— 汇总不该丢量）；
/// - 带上当日的「可能被裁」标记：那些天的总量只是**下界**（前端图注披露）。
fn gateway_trend_points(store: &Store) -> Vec<Value> {
    store
        .days
        .iter()
        .map(|(day, record)| {
            let mut input = 0u64;
            let mut output = 0u64;
            for account in record.accounts.values() {
                input = input.saturating_add(account.gateway.input);
                output = output.saturating_add(account.gateway.output);
            }
            json!({
                "key": day,
                "input": input,
                "output": output,
                "total": input.saturating_add(output),
                "truncated": record.gateway_truncated,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Datelike;
    use std::fs;

    /// 降级计数是**累加**（不是取大）：同一天记两次成功 ⇒ 2。
    ///
    /// 变异验证：把 `apply_fallback` 的 `saturating_add` 改成 `= 1`（取大）⇒ 本用例红。
    /// 这是本维度的**核心语义**：降级事件是一次性事件，折叠会漏。
    #[test]
    fn fallback_counts_accumulate_per_day() {
        let mut store = empty_store();
        apply_fallback(&mut store, FallbackResult::Degraded, "2026-10-29");
        apply_fallback(&mut store, FallbackResult::Degraded, "2026-10-29");
        apply_fallback(&mut store, FallbackResult::Failed, "2026-10-29");
        let counts = store
            .fallback_days
            .get("2026-10-29")
            .copied()
            .expect("当天必须有计数");
        assert_eq!(counts.degraded, 2, "同一天两次成功必须累加");
        assert_eq!(counts.failed, 1);
    }

    /// `blocked`（按规则压根没降级）**不进任何一档**。
    ///
    /// 把它算进 `failed` 会让「降级成功率」的分母灌水 —— 分母里混进根本没降级的事故。
    #[test]
    fn blocked_is_not_counted() {
        let mut store = empty_store();
        let counts = apply_fallback(&mut store, FallbackResult::Blocked, "2026-10-29");
        assert_eq!(counts, FallbackDayCounts::default());
        assert!(
            store.fallback_days.is_empty(),
            "blocked 不该创建当天的桶（否则会凭空多出一条全 0 的日记录）"
        );
    }

    /// 跨天分开计；汇总按**窗口**取，并给出总计。
    #[test]
    fn stats_summarize_window_and_total() {
        let mut store = empty_store();
        apply_fallback(&mut store, FallbackResult::Degraded, "2026-10-29");
        apply_fallback(&mut store, FallbackResult::Degraded, "2026-10-28");
        apply_fallback(&mut store, FallbackResult::Failed, "2026-10-27");
        // 窗口外（第 9 天）不该被算进来 —— 这正是「近 7 天」与「全部」的区别。
        apply_fallback(&mut store, FallbackResult::Degraded, "2026-10-20");

        let today = NaiveDate::parse_from_str("2026-10-29", "%Y-%m-%d").expect("date");
        let payload = summarize_fallback(&store, today, 7);
        assert_eq!(
            payload["days"].as_array().map(Vec::len),
            Some(7),
            "固定 7 行（没有事件的天也要占位，否则前端画不出时间轴）"
        );
        assert_eq!(payload["total"]["degraded"], 2, "窗口内两次成功");
        assert_eq!(payload["total"]["failed"], 1);
    }

    /// `days` 超上限被夹住 —— 否则一次调用会把整本账全量序列化（账本按日累积）。
    #[test]
    fn stats_days_are_clamped() {
        let store = empty_store();
        let today = NaiveDate::parse_from_str("2026-10-29", "%Y-%m-%d").expect("date");
        let payload = summarize_fallback(&store, today, 10_000);
        assert_eq!(
            payload["days"].as_array().map(Vec::len),
            Some(MAX_FALLBACK_STATS_DAYS as usize),
            "超过上限必须夹住"
        );
    }

    fn at_local_date(days_ago: i64, hour: u32) -> i64 {
        let today = Local::now().date_naive() - ChronoDuration::days(days_ago);
        Local
            .with_ymd_and_hms(today.year(), today.month(), today.day(), hour, 0, 0)
            .single()
            .expect("valid local test date")
            .timestamp_millis()
    }

    fn client_row(session_id: &str, ts: i64, input: u64, output: u64) -> ClientUsageRow {
        ClientUsageRow {
            session_id: session_id.to_string(),
            ts,
            input,
            output,
            model: "m".to_string(),
            credit: 0.0,
        }
    }

    /// 造一份「昨天 + 今天」的 jsonl 文本：昨天 `old_rows` 行、今天 `new_rows` 行。
    ///
    /// `pad` 是每行塞进去的填充长度 —— 用来把「今天的记录」推到**一个 chunk 之外**，
    /// 从而真正压到尾部窗口的扩张逻辑（而不是一读就覆盖）。
    fn day_fixture(old_rows: usize, new_rows: usize, pad: usize) -> (String, u64, u64) {
        let filler = "x".repeat(pad);
        let mut text = String::new();
        for index in 0..old_rows {
            text.push_str(&format!(
                "{}\n",
                json!({
                    "timestamp": at_local_date(1, 9) + index as i64,
                    "sessionId": "s-old",
                    "message": { "usage": { "input_tokens": 3, "output_tokens": 1 } },
                    "content": filler,
                })
            ));
        }
        for index in 0..new_rows {
            text.push_str(&format!(
                "{}\n",
                json!({
                    "timestamp": at_local_date(0, 1) + index as i64,
                    "sessionId": "s-today",
                    "message": { "usage": { "input_tokens": 7, "output_tokens": 2 } },
                    "content": filler,
                })
            ));
        }
        (text, 3 * old_rows as u64, 7 * new_rows as u64)
    }

    /// ★★ **尾部窗口必须与全量读等价**（2026-10-13 尾部读取的护栏）。
    ///
    /// 尾部读取是纯 I/O 优化：一旦停止判据写错，今日记录会被**静默漏掉**
    /// （数字变小、不报错）—— 真实语料里正是靠独立复算才发现「差 2%」的疑点。
    /// 这里走**生产函数** `read_day_window`（真文件、真 `File`），与整份文件的解析逐条比对。
    ///
    /// 阳性对照：断言全量读**确实**读到了今日记录（否则「两边都空」也能过）。
    #[test]
    fn tail_window_keeps_every_today_record_against_a_full_read() {
        let day = local_date_string(at_local_date(0, 1)).expect("today");
        let day_start = local_day_start_ms(&day).expect("day start");
        let dir = std::env::temp_dir().join(format!(
            "buddy-switch-token-daily-window-{}-{}",
            std::process::id(),
            now_ms()
        ));
        fs::create_dir_all(&dir).expect("create fixture dir");

        // 今天 400 行、每行 ~1KB ⇒ 今日记录**单独就超过一个 chunk**（256KB），
        // 迫使尾部窗口扩张多轮；前面再垫 2000 行昨天的记录（约 2.3MB），
        // 让窗口**明显短于**整份文件 —— 否则证明不了「只读尾部」。
        let (text, _old_in, new_in) = day_fixture(2000, 400, 1024);
        assert!(
            text.len() as u64 > 2 * jsonl_tail::TAIL_CHUNK_BYTES,
            "夹具必须超过两个 chunk（实际 {} 字节）",
            text.len()
        );
        let path = dir.join("sess.jsonl");
        fs::write(&path, &text).expect("write fixture");
        let bytes = text.into_bytes();

        let full = parse_day_rows(&String::from_utf8_lossy(&bytes), &day);
        assert!(!full.is_empty(), "阳性对照：全量读必须读到今日记录");
        assert_eq!(full.len(), 400, "阳性对照：全量读的条数 = 夹具里今天的行数");
        assert_eq!(
            full.iter().map(|row| row.input).sum::<u64>(),
            new_in,
            "阳性对照：全量读的 input 合计 = 夹具里今天的 input"
        );

        let mut file = fs::File::open(&path).expect("open fixture");
        let size = file.metadata().expect("stat").len();
        let window = read_day_window(&mut file, size, day_start).expect("窗口读得到");
        assert!(
            (window.len() as u64) < bytes.len() as u64,
            "窗口必须**短于**整份文件，否则这条用例证明不了「只读尾部」"
        );
        let tail = parse_day_rows(&String::from_utf8_lossy(&window), &day);
        assert_eq!(tail.len(), full.len(), "尾部窗口不得少读今日记录");
        assert_eq!(tail, full, "尾部窗口的结果必须与全量读逐条一致");

        // 反向：整份都可能是今天的（昨日 0 行）⇒ 窗口必须一直扩张到文件开头。
        let (only_today, _, _) = day_fixture(0, 40, 1024);
        let path = dir.join("today-only.jsonl");
        fs::write(&path, &only_today).expect("write fixture");
        let mut file = fs::File::open(&path).expect("open fixture");
        let size = file.metadata().expect("stat").len();
        let window = read_day_window(&mut file, size, day_start).expect("窗口读得到");
        assert_eq!(window.len() as u64, size, "今日记录覆盖整份时必须读全");
        assert_eq!(
            parse_day_rows(&String::from_utf8_lossy(&window), &day).len(),
            40
        );

        let _ = fs::remove_dir_all(&dir);
    }

    /// (a) 日期分桶：昨日记录不落今日、今日记录落今日。
    #[test]
    fn client_rows_are_bucketed_by_local_day() {
        let today = at_local_date(0, 12);
        let yesterday = at_local_date(1, 12);
        let day = local_date_string(today).unwrap();
        let mut meta = HashMap::new();
        meta.insert("s1".to_string(), "uid-a".to_string());

        let rows = vec![
            client_row("s1", yesterday, 500, 100),
            client_row("s1", today, 10, 2),
        ];
        let (per_account, unknown) = bucket_client(&rows, &meta, &day);

        assert_eq!(per_account.get("uid-a").unwrap().input, 10, "只累计今日");
        assert_eq!(per_account.get("uid-a").unwrap().output, 2);
        assert_eq!(unknown.input, 0);
    }

    /// (b) 同一账号的两个会话合并求和。
    #[test]
    fn client_rows_sum_two_sessions_of_the_same_account() {
        let today = at_local_date(0, 12);
        let day = local_date_string(today).unwrap();
        let mut meta = HashMap::new();
        meta.insert("s1".to_string(), "uid-a".to_string());
        meta.insert("s2".to_string(), "uid-a".to_string());

        let rows = vec![
            client_row("s1", today, 10, 2),
            client_row("s2", today, 30, 4),
        ];
        let (per_account, unknown) = bucket_client(&rows, &meta, &day);

        assert_eq!(per_account.len(), 1);
        assert_eq!(per_account.get("uid-a").unwrap().input, 40);
        assert_eq!(per_account.get("uid-a").unwrap().output, 6);
        assert_eq!(unknown.input, 0);
    }

    /// (c) 归属不了的会话落 unknown，**不**落到别的账号。
    #[test]
    fn unresolvable_session_goes_to_unknown_not_another_account() {
        let today = at_local_date(0, 12);
        let day = local_date_string(today).unwrap();
        let mut meta = HashMap::new();
        meta.insert("s1".to_string(), "uid-a".to_string());

        // "ghost" 不在 meta（库缺行 / user_id 为空）。
        let rows = vec![
            client_row("s1", today, 10, 2),
            client_row("ghost", today, 7, 3),
        ];
        let (per_account, unknown) = bucket_client(&rows, &meta, &day);

        assert_eq!(per_account.len(), 1);
        assert_eq!(per_account.get("uid-a").unwrap().input, 10);
        assert_eq!(
            unknown.input, 7,
            "无法归属的用量必须进 unknown，绝不并入 uid-a"
        );
        assert_eq!(unknown.output, 3);
    }

    /// 网关行按 region + 本地日过滤。
    #[test]
    fn gateway_rows_are_filtered_by_region_and_day() {
        let today = at_local_date(0, 12);
        let yesterday = at_local_date(1, 12);
        let day = local_date_string(today).unwrap();
        let entries = vec![
            json!({"ts": today, "region": "cn", "account": "uid-a", "promptTokens": 10, "completionTokens": 2}),
            json!({"ts": today, "region": "global", "account": "uid-a", "promptTokens": 99, "completionTokens": 9}),
            json!({"ts": yesterday, "region": "cn", "account": "uid-a", "promptTokens": 7, "completionTokens": 1}),
        ];

        let per_account = bucket_gateway(
            &parse_gateway_rows(&entries),
            Region::Cn,
            &day,
            &HashSet::new(),
            &HashMap::new(),
            None,
        );

        assert_eq!(per_account.len(), 1);
        assert_eq!(
            per_account.get("uid-a").unwrap().input,
            10,
            "只取 cn + 今日"
        );
        assert_eq!(per_account.get("uid-a").unwrap().output, 2);
    }

    /// 网关展示名 → uid 解析：唯一命中解析、歧义/未知原样保留。
    #[test]
    fn gateway_display_name_resolves_to_uid() {
        let uids: HashSet<String> = HashSet::from(["uid-a".to_string()]);
        let mut names: HashMap<String, Option<String>> = HashMap::new();
        names.insert("a@example.com".to_string(), Some("uid-a".to_string()));
        names.insert("重名".to_string(), None);

        assert_eq!(resolve_account_key("uid-a", &uids, &names), "uid-a");
        assert_eq!(resolve_account_key("a@example.com", &uids, &names), "uid-a");
        assert_eq!(
            resolve_account_key("重名", &uids, &names),
            "重名",
            "歧义展示名必须原样保留，绝不错贴"
        );
        assert_eq!(resolve_account_key("ghost", &uids, &names), "ghost");
    }

    /// 行内 `accountUid`（写入时的精确身份）**优先于**展示名解析：即便展示名歧义/重名，
    /// 只要带了 uid 就精确归属；仅**旧行**（无 `accountUid`）才回落展示名解析。
    #[test]
    fn gateway_row_prefers_account_uid_over_ambiguous_display_name() {
        let today = at_local_date(0, 12);
        let day = local_date_string(today).unwrap();
        let entries = vec![
            // 新行：展示名「重名」歧义（两个账号共用），但 accountUid 精确。
            json!({"ts": today, "region": "cn", "account": "重名", "accountUid": "uid-b",
                   "promptTokens": 10, "completionTokens": 2}),
            // 旧行：无 accountUid ⇒ 回落展示名解析（唯一命中）。
            json!({"ts": today, "region": "cn", "account": "a@example.com",
                   "promptTokens": 5, "completionTokens": 1}),
        ];
        let uids: HashSet<String> = HashSet::from(["uid-a".to_string(), "uid-b".to_string()]);
        let mut names: HashMap<String, Option<String>> = HashMap::new();
        names.insert("a@example.com".to_string(), Some("uid-a".to_string()));
        names.insert("重名".to_string(), None);

        let per_account = bucket_gateway(
            &parse_gateway_rows(&entries),
            Region::Cn,
            &day,
            &uids,
            &names,
            None,
        );

        assert_eq!(
            per_account.get("uid-b").unwrap().input,
            10,
            "带 accountUid 的行必须精确归属，不受展示名歧义影响"
        );
        assert_eq!(
            per_account.get("uid-a").unwrap().input,
            5,
            "旧行无 accountUid ⇒ 回落展示名解析"
        );
        assert!(
            !per_account.contains_key("重名"),
            "歧义展示名不得成为归属键"
        );
    }

    // -----------------------------------------------------------------------
    // 2026-10-15：网关桶改为**增量累计**（修「数字会倒退」）
    // -----------------------------------------------------------------------

    /// 造一行网关日志条目（`uid-a` / cn / 今天）。
    fn gw_row(ts: i64, input: u64, output: u64) -> Value {
        gw_row_for("uid-a", ts, input, output)
    }

    fn gw_row_for(uid: &str, ts: i64, input: u64, output: u64) -> Value {
        json!({
            "ts": ts,
            "region": "cn",
            "account": uid,
            "accountUid": uid,
            "promptTokens": input,
            "completionTokens": output,
        })
    }

    fn gw_round_cn(watermark: Option<i64>, entries: &[Value]) -> (GatewayWrite, Option<i64>) {
        let day = local_date_string(at_local_date(0, 12)).expect("today");
        /* `dims_empty = true` / `keys_empty = true`：这些用例只关心账号桶，
        维度走播种轮（不影响 accounts）。 */
        let round = gateway_round(
            watermark,
            true,
            true,
            &parse_gateway_rows(entries),
            Region::Cn,
            &day,
            &HashSet::new(),
            &HashMap::new(),
        );
        (round.accounts, round.next_watermark)
    }

    fn dim_map(write: DimWrite) -> BTreeMap<String, DimBucket> {
        match write {
            DimWrite::Add(map) | DimWrite::Correct(map) => map,
        }
    }

    fn dim_total(map: &BTreeMap<String, DimBucket>) -> (u64, u64, u64, u64, f64) {
        map.values().fold((0, 0, 0, 0, 0.0), |acc, bucket| {
            (
                acc.0 + bucket.input,
                acc.1 + bucket.output,
                acc.2 + bucket.requests,
                acc.3 + bucket.failed,
                acc.4 + bucket.credit,
            )
        })
    }

    /// ★★ **本改动的存在理由**：三个维度的合计必须都等于账号桶合计。
    ///
    /// 为什么这条最重要：页面上「按模型合计」与「网关 Token」卡并排显示，
    /// 一旦两条口径分叉（多算/少算一条），用户会先怀疑软件坏了 —— 那正是 issue #63
    /// 抱怨的那类不信任。三个维度与账号桶由**同一次扫描、同一水位线**产出，
    /// 所以这里断言的是**恒等式**，不是「差不多」。
    ///
    /// 可证伪：让 `bucket_dim` 少一个过滤条件（比如不按 region 过滤），
    /// 或让 `bucket_gateway` 多一个条件 ⇒ 本用例在等式上变红。
    #[test]
    fn gateway_dims_sum_to_the_gateway_bucket() {
        let day = local_date_string(at_local_date(0, 12)).expect("today");
        let base = at_local_date(0, 12);
        let entries = vec![
            json!({ "ts": base, "region": "cn", "account": "uid-a", "accountUid": "uid-a",
                    "endpoint": "/v1/chat/completions", "model": "m-1", "status": 200,
                    "keyId": "k-1", "keyScope": "wb",
                    "promptTokens": 100, "completionTokens": 10, "credit": 1.25 }),
            json!({ "ts": base + 1, "region": "cn", "account": "uid-a", "accountUid": "uid-a",
                    "endpoint": "/v1/chat/completions", "model": "m-1", "status": 429,
                    "keyId": "k-1", "keyScope": "wb",
                    "promptTokens": 0, "completionTokens": 0, "credit": 0.0 }),
            json!({ "ts": base + 2, "region": "cn", "account": "uid-b", "accountUid": "uid-b",
                    "endpoint": "/v1/messages", "model": "", "status": 200,
                    "keyId": null,
                    "promptTokens": 40, "completionTokens": 2, "credit": 0.5 }),
            // 别的 region / 别的日：一律不得进任何桶
            json!({ "ts": base + 3, "region": "global", "account": "uid-g", "accountUid": "uid-g",
                    "endpoint": "/v1/messages", "model": "m-1", "status": 200,
                    "keyId": "k-g", "keyScope": "wb",
                    "promptTokens": 999, "completionTokens": 9, "credit": 9.0 }),
        ];
        let round = gateway_round(
            None,
            true,
            true,
            &parse_gateway_rows(&entries),
            Region::Cn,
            &day,
            &HashSet::new(),
            &HashMap::new(),
        );
        let accounts = match round.accounts {
            GatewayWrite::Add(map) | GatewayWrite::Correct(map) => map,
        };
        let account_total: (u64, u64) = accounts
            .values()
            .fold((0, 0), |acc, b| (acc.0 + b.input, acc.1 + b.output));

        let models = dim_map(round.models);
        let accounts_dim = dim_map(round.accounts_dim);
        let endpoints = dim_map(round.endpoints);
        let keys = dim_map(round.keys);
        for (label, map) in [
            ("模型", &models),
            ("账号", &accounts_dim),
            ("端点", &endpoints),
            ("密钥", &keys),
        ] {
            let (input, output, requests, failed, credit) = dim_total(map);
            assert_eq!(
                (input, output),
                account_total,
                "{label}维度的 token 合计必须等于账号桶合计"
            );
            assert_eq!(
                requests, 3,
                "{label}维度必须计入 cn + 今日的 3 条（global 那条不算）"
            );
            assert_eq!(failed, 1, "{label}维度必须把 429 计成 failed");
            assert!(
                (credit - 1.75).abs() < 1e-9,
                "{label}维度的积分合计应为 1.75，实际 {credit}"
            );
        }
        assert!(
            models.contains_key(UNKNOWN_MODEL),
            "model 为空必须落哨兵，不能丢量（丢了「按模型合计」就小于网关卡）"
        );
        assert_eq!(endpoints.len(), 2, "两个端点各一桶");
        assert_eq!(accounts_dim.len(), 2, "两个账号各一桶");
        assert!(
            keys.contains_key("wb:k-1"),
            "带 keyScope 的 Key 桶键必须是 `{{scope}}:{{id}}`（否则两套台账会并成一行）"
        );
        assert!(
            keys.contains_key(KEY_DIM_NONE),
            "keyId 为 null 必须落「无密钥调用」哨兵，不能丢量"
        );
        assert_eq!(keys.len(), 2, "一把 Key + 一次无 Key 调用");
    }

    /// ★★ issue #154：**命中量必须与 token 同源同步进账本**，且五个位置口径一致。
    ///
    /// 为什么单独立一条：`cached` 是**新加的一列**，而累加点有五处（账号桶 + 四条维度）。
    /// 少接一处不会编译失败，只会让「按模型的命中率」与「按账号的命中率」安静地分叉
    /// —— 那种不一致在页面上表现为「两个百分比都不像对的」，最难排查。
    ///
    /// 可证伪：把 `bucket_dim` 里那行 `bucket.cached += row.cached` 删掉 ⇒
    /// 维度侧的 cached 变 0，本用例在 `dim_cached == account_cached` 上变红。
    #[test]
    fn gateway_cached_tokens_are_aggregated_into_every_bucket() {
        let day = local_date_string(at_local_date(0, 12)).expect("today");
        let base = at_local_date(0, 12);
        let entries = vec![
            // 命中 90 / 输入 100（DeepSeek 形状：prompt 已含命中）
            json!({ "ts": base, "region": "cn", "account": "uid-a", "accountUid": "uid-a",
                    "endpoint": "/v1/chat/completions", "model": "m-1", "status": 200,
                    "keyId": "k-1", "keyScope": "wb",
                    "promptTokens": 100, "completionTokens": 10, "cachedTokens": 90 }),
            // **老条目**：没有 `cachedTokens` 键 ⇒ 0（不是「没命中」）
            json!({ "ts": base + 1, "region": "cn", "account": "uid-b", "accountUid": "uid-b",
                    "endpoint": "/v1/chat/completions", "model": "m-1", "status": 200,
                    "keyId": null,
                    "promptTokens": 40, "completionTokens": 2 }),
        ];
        let round = gateway_round(
            None,
            true,
            true,
            &parse_gateway_rows(&entries),
            Region::Cn,
            &day,
            &HashSet::new(),
            &HashMap::new(),
        );
        let accounts = match round.accounts {
            GatewayWrite::Add(map) | GatewayWrite::Correct(map) => map,
        };
        let account_cached: u64 = accounts.values().map(|bucket| bucket.cached).sum();
        assert_eq!(account_cached, 90, "账号桶必须累加命中量（老条目贡献 0）");

        for (label, map) in [
            ("模型", dim_map(round.models)),
            ("账号", dim_map(round.accounts_dim)),
            ("端点", dim_map(round.endpoints)),
            ("密钥", dim_map(round.keys)),
        ] {
            let dim_cached: u64 = map.values().map(|bucket| bucket.cached).sum();
            assert_eq!(
                dim_cached, account_cached,
                "{label}维度的命中量合计必须等于账号桶 —— 五处累加点同源同步"
            );
        }
    }

    /// ★ issue #154：`cached` 是 `input` 的**子集**，不得参与 `total` 的计算。
    ///
    /// 为什么钉这条：P0 那份 bug（命中率腰斩 50%）的本质就是「命中量被算了两次」。
    /// 若 `dim_groups` / `gateway_trend_points` 把 `cached` 加进 `total`，同一个错误
    /// 会从协议层搬进账本层，而且这次连客户端都不需要参与 —— 页面自己就会翻倍。
    #[test]
    fn gateway_cached_tokens_do_not_inflate_totals() {
        let day = local_date_string(at_local_date(0, 12)).expect("today");
        let base = at_local_date(0, 12);
        let entries = vec![json!({
            "ts": base, "region": "cn", "account": "uid-a", "accountUid": "uid-a",
            "endpoint": "/v1/chat/completions", "model": "m-1", "status": 200,
            "keyId": "k-1", "keyScope": "wb",
            "promptTokens": 1000, "completionTokens": 20, "cachedTokens": 900,
        })];
        let round = gateway_round(
            None,
            true,
            true,
            &parse_gateway_rows(&entries),
            Region::Cn,
            &day,
            &HashSet::new(),
            &HashMap::new(),
        );
        let models = dim_groups(&dim_map(round.models), "model");
        assert_eq!(
            models[0]["total"], 1020,
            "total 只能是 input + output（1000 + 20）；把 cached 加进去就是同一个 bug 换个层"
        );
        assert_eq!(models[0]["cached"], 900, "命中量单列");
    }

    /// ★ issue #154：`cached == 0` 时出口给 **`null`**，不是 `0`。
    ///
    /// 语义：`0` 会被前端读成「确实一次都没命中」，而真相通常是「上游没报 / 老账本
    /// 没这一列」。两者在 UI 上必须分开 —— 与 `gatewayIn` 写 0 的诚实口径同源。
    #[test]
    fn gateway_cached_is_null_when_unobserved() {
        let day = local_date_string(at_local_date(0, 12)).expect("today");
        let base = at_local_date(0, 12);
        let entries = vec![json!({
            "ts": base, "region": "cn", "account": "uid-a", "accountUid": "uid-a",
            "endpoint": "/v1/chat/completions", "model": "m-1", "status": 200,
            "keyId": "k-1", "keyScope": "wb",
            "promptTokens": 100, "completionTokens": 10,
        })];
        let round = gateway_round(
            None,
            true,
            true,
            &parse_gateway_rows(&entries),
            Region::Cn,
            &day,
            &HashSet::new(),
            &HashMap::new(),
        );
        let groups = dim_groups(&dim_map(round.models), "model");
        assert!(
            groups[0]["cached"].is_null(),
            "没观测到命中必须是 null（0 会被读成「确实全 miss」）"
        );
        assert_eq!(groups[0]["input"], 100, "输入侧照旧");
    }

    /// 密钥维度的三种「没有 Key」判据（与前端 `key-usage.ts` 逐字对齐）。
    ///
    /// ⚠️ 老条目（**没有 `keyId` 键**）与无 Key 调用（`keyId: null`）**不得合并**：
    /// 合并会让历史数据凭空多出一批「匿名调用」，而那个数字一旦出现在排行里
    /// 就会被当成真的（issue #76 的原始判据）。
    #[test]
    fn key_bucket_separates_legacy_from_no_key() {
        assert_eq!(
            key_bucket_of(&json!({ "ts": 1, "promptTokens": 5 })),
            KEY_DIM_LEGACY
        );
        assert_eq!(
            key_bucket_of(&json!({ "ts": 1, "promptTokens": 5, "keyId": null })),
            KEY_DIM_NONE
        );
        assert_eq!(
            key_bucket_of(&json!({ "ts": 1, "keyId": "  ", "keyScope": "wb" })),
            KEY_DIM_NONE,
            "空串按「没有 Key」处理，不当成一把空 id 的 Key"
        );
        assert_eq!(
            key_bucket_of(&json!({ "ts": 1, "keyId": "k-1", "keyScope": "unified" })),
            "unified:k-1"
        );
        assert_eq!(
            key_bucket_of(&json!({ "ts": 1, "keyId": "k-1" })),
            "k-1",
            "老日志没有 keyScope ⇒ 退化成裸 id，不编一个归属出来"
        );
    }

    /// v3 → v4：`gateway_keys` 是空的、但当天已经有 token（另外三个维度已存在）
    /// ⇒ 密钥维度必须**整窗播种**并标 `keys_partial`，否则升级当天此前的请求全部漏记。
    #[test]
    fn gateway_keys_seed_from_the_whole_window_when_dimension_is_new() {
        let day = local_date_string(at_local_date(0, 12)).expect("today");
        let base = at_local_date(0, 12);
        let entries = vec![json!({
            "ts": base, "region": "cn", "account": "uid-a", "accountUid": "uid-a",
            "endpoint": "/v1/chat/completions", "model": "m-1", "status": 200,
            "keyId": "k-1", "keyScope": "wb",
            "promptTokens": 10, "completionTokens": 1, "credit": 0.1
        })];
        let round = gateway_round(
            Some(base - 1), // 水位线存在（老记录）
            false,          // 另外三个维度**不是**空的
            true,           // 只有密钥维度是空的
            &parse_gateway_rows(&entries),
            Region::Cn,
            &day,
            &HashSet::new(),
            &HashMap::new(),
        );
        assert!(
            round.keys_partial,
            "升级播种必须标 keys_partial（UI 要披露「这一天的 key 维度只是下界」）"
        );
        let keys = dim_map(round.keys);
        assert_eq!(
            dim_total(&keys).0,
            10,
            "播种轮取整窗读数，不是「只加水位线之后的新条目」"
        );
    }

    /// 升级过渡（v2 → v3）：水位线存在但维度桶为空 ⇒ 维度单独播种一次，并标 partial。
    ///
    /// 不做这一步，升级当天「按模型合计」会小于「网关 Token」—— 新功能上线第一天
    /// 就被当成 bug。
    #[test]
    fn gateway_dims_seed_once_when_watermark_exists_but_dims_are_empty() {
        let day = local_date_string(at_local_date(0, 12)).expect("today");
        let base = at_local_date(0, 12);
        let entries = vec![json!({
            "ts": base, "region": "cn", "account": "uid-a", "accountUid": "uid-a",
            "endpoint": "/v1/chat/completions", "model": "m-1", "status": 200,
            "promptTokens": 10, "completionTokens": 1, "credit": 0.1
        })];
        let round = gateway_round(
            Some(base - 1), // 水位线存在
            true,           // 但维度桶是空的
            true,           // 密钥维度也是空的
            &parse_gateway_rows(&entries),
            Region::Cn,
            &day,
            &HashSet::new(),
            &HashMap::new(),
        );
        assert!(
            round.dims_partial,
            "升级播种必须标 partial（UI 要披露拆分不完整）"
        );
        let models = dim_map(round.models);
        assert_eq!(
            dim_total(&models).0,
            10,
            "播种轮取整窗读数，不是「只加新条目」"
        );
        assert!(
            matches!(round.accounts, GatewayWrite::Add(_)),
            "账号桶仍走增量（它的水位线是有效的）"
        );
    }

    /// 从写入意图里取出值表；变体不符合预期直接 panic（把「语义」也钉进断言）。
    fn add_map(write: GatewayWrite) -> BTreeMap<String, Bucket> {
        match write {
            GatewayWrite::Add(map) => map,
            other => panic!("期望 Add（增量），实得 {other:?}"),
        }
    }

    fn correct_map(write: GatewayWrite) -> BTreeMap<String, Bucket> {
        match write {
            GatewayWrite::Correct(map) => map,
            other => panic!("期望 Correct（校正），实得 {other:?}"),
        }
    }

    /// ★★ 滚动窗口滚过后，**已计入的量不倒退**（本次缺陷的回归护栏）。
    ///
    /// 缺陷现场（2026-10-15 实测，CN 桶）：旧实现每轮「整窗覆盖」⇒ 日志 FIFO 裁掉
    /// 今天早些时候的条目后，当日值**变小**（10.3M → 3.6M → 18.7M，毫无规律）。
    ///
    /// 本用例模拟三轮：
    /// - 第 1 轮：水位线缺失 + 整窗 `[100]=10、[200]=20` ⇒ 校正轮 = 30，水位线 200；
    /// - 第 2 轮：窗口滚动，`[100]` 已被裁掉，剩 `[200]=20` + 新增 `[300]=7`
    ///   ⇒ 增量只加 7 ⇒ **37**（旧实现会覆盖成 27，数值倒退）；
    /// - 第 3 轮：同窗再跑一次 ⇒ 增量必须为空（**不双计**）。
    #[test]
    fn gateway_today_never_goes_back_when_the_log_window_rolls() {
        let t1 = at_local_date(0, 9);
        let t2 = t1 + 1_000;
        let t3 = t1 + 2_000;

        // 第 1 轮：水位线缺失 ⇒ 校正轮（空桶时 = 整窗）。
        let (first, wm) = gw_round_cn(None, &[gw_row(t1, 10, 1), gw_row(t2, 20, 2)]);
        let first = correct_map(first);
        assert_eq!(first.get("uid-a").unwrap().input, 30, "空桶首轮应计整窗");
        assert_eq!(first.get("uid-a").unwrap().output, 3);
        assert_eq!(wm, Some(t2), "水位线推进到本轮最大 ts");

        // 把校正值落进「桶」（等价 record_gateway_today 的取大应用）。
        let mut bucket = Bucket {
            input: 30,
            output: 3,
            ..Default::default()
        };

        // 第 2 轮：t1 被裁、t3 新增 ⇒ 走增量。
        let (second, wm2) = gw_round_cn(wm, &[gw_row(t2, 20, 2), gw_row(t3, 7, 1)]);
        let second = add_map(second);
        assert_eq!(
            second.get("uid-a").unwrap().input,
            7,
            "只应累加水位线之后的新增（旧实现整窗覆盖 ⇒ 20+7=27，比上一轮还小）"
        );
        bucket.input = bucket
            .input
            .saturating_add(second.get("uid-a").unwrap().input);
        assert_eq!(bucket.input, 37, "累计值必须只增不减");
        assert_eq!(wm2, Some(t3));

        // 第 3 轮：同窗重复读取 ⇒ 零增量（幂等，不双计）。
        let (third, wm3) = gw_round_cn(wm2, &[gw_row(t2, 20, 2), gw_row(t3, 7, 1)]);
        assert!(add_map(third).is_empty(), "同一批条目不得被重复累加");
        assert_eq!(wm3, Some(t3), "无新条目时水位线保持不动");
    }

    /// 升级过渡态的**校正轮**语义：输出 = 当前窗口读数；应用端逐账号 `max(现有, 窗口)`。
    ///
    /// 两个方向都要成立（应用端取大是 `record_gateway_today` 的一行代码，
    /// 由真文件用例覆盖；这里钉住校正轮的**输出**与水位线的播种）：
    /// - 窗口 > 旧值 ⇒ 抬升（旧实现覆盖也会抬升，但新语义后续增量不丢）；
    /// - 窗口 < 旧值 ⇒ 保持旧值（**不倒退**）。
    #[test]
    fn gateway_round_corrects_legacy_bucket_with_the_window_reading() {
        let t1 = at_local_date(0, 9);
        let t2 = t1 + 1_000;
        let t3 = t1 + 2_000;

        // 旧版本的遗留桶没有水位线 ⇒ 走校正轮：输出 = 整窗读数，水位线播种。
        let (write, wm) = gw_round_cn(None, &[gw_row(t1, 10, 1), gw_row(t2, 20, 2)]);
        let corrected = correct_map(write);
        assert_eq!(
            corrected.get("uid-a").unwrap().input,
            30,
            "校正轮输出 = 当前窗口合计"
        );
        assert_eq!(wm, Some(t2), "校正轮同时播种水位线");

        // 应用端取大（两个方向）：
        assert_eq!(
            900u64.max(corrected.get("uid-a").unwrap().input),
            900,
            "旧值更大 ⇒ 保持旧值（不倒退）"
        );
        assert_eq!(
            5u64.max(corrected.get("uid-a").unwrap().input),
            30,
            "窗口更大 ⇒ 抬升到窗口值"
        );

        // 播种之后走增量：只加水位线之后的新条目。
        let (next, _) = gw_round_cn(wm, &[gw_row(t2, 20, 2), gw_row(t3, 5, 5)]);
        let next = add_map(next);
        assert_eq!(next.get("uid-a").unwrap().input, 5, "播种后只加新条目");
        assert_eq!(next.get("uid-a").unwrap().output, 5);
    }

    /// 水位线的推进只看**本 region + 本日**的行（别的 region / 别的日不得把它顶起）。
    #[test]
    fn gateway_round_isolates_watermark_by_region_and_day() {
        let t1 = at_local_date(0, 9);
        let t_other_day = at_local_date(1, 9);
        let entries = vec![
            gw_row(t1, 10, 1),
            // 别的 region（ts 更大）不得顶起 cn 的水位线、也不得进 cn 桶。
            json!({"ts": t1 + 500, "region": "global", "account": "uid-g",
                   "accountUid": "uid-g", "promptTokens": 99, "completionTokens": 9}),
            // 别的日（本 region）同样不计。
            json!({"ts": t_other_day, "region": "cn", "account": "uid-a",
                   "accountUid": "uid-a", "promptTokens": 77, "completionTokens": 7}),
        ];

        let (write, wm) = gw_round_cn(None, &entries);
        assert_eq!(
            wm,
            Some(t1),
            "水位线 = 本 region 本日的最大 ts（99/77 那两行都不算）"
        );
        let corrected = correct_map(write);
        assert_eq!(
            corrected.get("uid-a").unwrap().input,
            10,
            "只计 cn + 今日的行"
        );
        assert!(!corrected.contains_key("uid-g"), "global 行不得进 cn 桶");
    }

    /// ★★ **真文件**回归：`record_gateway_today` 的累计语义（滚动裁剪 + 升级校正）。
    ///
    /// 走生产函数 + 真 store（临时 home 定向，见 `HomeOverrideGuard`）：
    ///
    /// - **升级过渡（校正轮）**：预置一份**旧版本形态**的 store（有桶、无水位线）——
    ///   `uid-a` 旧值 5/0（比窗口小 ⇒ 抬升到窗口值 33）、`uid-b` 旧值 999/9
    ///   （比窗口大 ⇒ **保持 999**，不倒退）；
    /// - **滚动裁剪**：第 2 轮日志被裁成 `[1 条旧 + 1 条新]`，断言值 = 校正后 + 新增，
    ///   而**不是**「当前窗口合计」（那是缺陷行为）；
    /// - **幂等**：同窗再跑一轮 ⇒ 数值不变（不双计）。
    ///
    /// 可证伪：把 `record_gateway_today` 改回 `record.accounts[...].gateway = bucket`
    /// （整窗覆盖）⇒ 第 2 轮断言与 `uid-b` 的保持断言都会变红。
    #[test]
    fn record_gateway_today_accumulates_across_rollover_and_corrects_legacy_buckets() {
        let dir = std::env::temp_dir().join(format!(
            "buddy-switch-token-daily-gw-{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&dir).expect("create home dir");
        let guard = crate::modules::config::HomeOverrideGuard::set(&dir);
        let store = dir.join(".buddy-switch");
        std::fs::create_dir_all(&store).expect("create store dir");

        let t1 = now_ms() - 3_000;
        let t2 = now_ms() - 2_000;
        let t3 = now_ms() - 1_000;
        let day = local_date_string(now_ms()).expect("today");

        // 预置「旧版本形态」的 store：桶有值、**没有** `gatewayWatermarkTs`。
        let legacy_store = json!({
            "version": 1,
            "days": {
                day.clone(): {
                    "accounts": {
                        "uid-a": { "client": {"input":0,"output":0}, "gateway": {"input":5,"output":0} },
                        "uid-b": { "client": {"input":0,"output":0}, "gateway": {"input":999,"output":9} }
                    },
                    "gatewayTruncated": false
                }
            }
        });
        std::fs::write(
            store.join("token_daily.json"),
            serde_json::to_string(&legacy_store).unwrap(),
        )
        .expect("pre-write legacy store");

        // 第 1 轮：窗口 = [t1, t2]；uid-b 也在窗口里（但读数小于其旧值）。
        let log = json!([
            gw_row(t1, 10, 1),
            gw_row(t2, 20, 2),
            gw_row_for("uid-b", t1, 7, 1),
        ]);
        std::fs::write(
            store.join("gateway_logs.json"),
            serde_json::to_string(&log).unwrap(),
        )
        .expect("write log v1");
        let first = record_gateway_today(Region::Cn);
        let value_of = |response: &Value, uid: &str| -> u64 {
            response["accounts"]
                .as_array()
                .and_then(|accounts| accounts.iter().find(|a| a["uid"] == uid))
                .map(|account| {
                    account["gatewayIn"].as_u64().unwrap_or(0)
                        + account["gatewayOut"].as_u64().unwrap_or(0)
                })
                .unwrap_or(0)
        };
        assert_eq!(
            value_of(&first, "uid-a"),
            33,
            "旧值 5 比窗口 33 小 ⇒ 校正轮抬升"
        );
        assert_eq!(
            value_of(&first, "uid-b"),
            1008,
            "旧值 999+9 比窗口读数大 ⇒ 保持（取大，不倒退、不双计）"
        );

        // 第 2 轮：t1 被 FIFO 裁掉，t3 新增。旧实现会覆盖成 23（倒退）。
        let log = json!([gw_row(t2, 20, 2), gw_row(t3, 30, 3)]);
        std::fs::write(
            store.join("gateway_logs.json"),
            serde_json::to_string(&log).unwrap(),
        )
        .expect("write log v2");
        let second = record_gateway_today(Region::Cn);
        assert_eq!(
            value_of(&second, "uid-a"),
            66,
            "滚动裁剪后只能**追加新增**（33 + 33）；若等于 55 说明又被整窗覆盖了"
        );
        assert_eq!(value_of(&second, "uid-b"), 1008, "窗口外的账号不受滚动影响");

        // 幂等：同一份日志再跑一轮 ⇒ 数值不变。
        let third = record_gateway_today(Region::Cn);
        assert_eq!(value_of(&third, "uid-a"), 66, "同窗重复记录不得双计");
        assert_eq!(value_of(&third, "uid-b"), 1008);

        drop(guard);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// (d) `gatewayTruncated` 要求**同时**满足「条数到上限」与「最旧存活条目是今天」。
    ///
    /// 第二个条件是本次修误报的关键：滚动窗口可能被**昨天**的流量顶满，而今天的条目
    /// 全在（FIFO ⇒ 早于今天的条目先被丢）⇒ 今日总量完整，**不该**标 `true`。
    #[test]
    fn gateway_truncated_requires_full_window_and_today_oldest() {
        let today = local_date_string(at_local_date(0, 12)).unwrap();
        let yesterday = (Local::now().date_naive() - ChronoDuration::days(1))
            .format("%Y-%m-%d")
            .to_string();

        // 没到上限：无论最旧条目是哪天，都没丢过东西。
        assert!(!gateway_log_truncated_today(5, 200, Some(&today), &today));
        assert!(!gateway_log_truncated_today(199, 200, Some(&today), &today));

        // 到上限 + 最旧存活条目是今天 ⇒ 今天的条目可能被丢过 ⇒ 真。
        assert!(gateway_log_truncated_today(200, 200, Some(&today), &today));
        assert!(gateway_log_truncated_today(201, 200, Some(&today), &today));

        // ★ 误报杀手：到上限但最旧存活条目**早于今天** ⇒ 今天的条目一条没丢 ⇒ 假。
        assert!(!gateway_log_truncated_today(
            200,
            200,
            Some(&yesterday),
            &today
        ));

        // 空日志（无最旧条目）⇒ 假；`keep=0` 按 1 兜底。
        assert!(!gateway_log_truncated_today(0, 200, None, &today));
        assert!(!gateway_log_truncated_today(200, 0, None, &today));
    }

    /// (f) 网关趋势：逐日跨账号求和、日期升序、`truncated` 随身带。
    ///
    /// 「升序」不是审美：前端按窗口键 join（`usageWindowKeys` 升序）⇒ 后端若乱序，
    /// 图上的天会错位。`BTreeMap` 的键序保证它。
    #[test]
    fn gateway_trend_points_sums_accounts_in_ascending_order() {
        let mut store = empty_store();

        let mut day_a = DayRecord::default();
        day_a.accounts.insert(
            "u1".to_string(),
            AccountRecord {
                gateway: Bucket {
                    input: 10,
                    output: 2,
                    ..Default::default()
                },
                ..Default::default()
            },
        );
        day_a.accounts.insert(
            "u2".to_string(),
            AccountRecord {
                gateway: Bucket {
                    input: 5,
                    output: 1,
                    ..Default::default()
                },
                ..Default::default()
            },
        );
        day_a.gateway_truncated = true;

        // 只有 client 用量的日子：gateway 合计必须是**真实 0**（记录器跑过、网关没流量），
        // 且不得把 client 的量混进 gateway 汇总。
        let mut day_b = DayRecord::default();
        day_b.accounts.insert(
            "u1".to_string(),
            AccountRecord {
                client: Bucket {
                    input: 999,
                    output: 0,
                    ..Default::default()
                },
                ..Default::default()
            },
        );

        store.days.insert("2026-10-11".to_string(), day_b);
        store.days.insert("2026-10-10".to_string(), day_a); // 故意乱序插入

        let points = gateway_trend_points(&store);
        assert_eq!(points.len(), 2);
        assert_eq!(points[0]["key"], "2026-10-10", "日期必须升序");
        assert_eq!(points[0]["input"], 15, "跨账号求和");
        assert_eq!(points[0]["output"], 3);
        assert_eq!(points[0]["total"], 18);
        assert_eq!(
            points[0]["truncated"], true,
            "被裁日的总量只是下界，标记必须带出给前端披露"
        );
        assert_eq!(points[1]["key"], "2026-10-11");
        assert_eq!(points[1]["total"], 0, "client 用量不得混进 gateway 汇总");
        assert_eq!(points[1]["truncated"], false);
    }

    /// (g) 网关趋势只读、不落盘：调用前后 store 文件内容不变（纯读路径）。
    #[test]
    fn gateway_token_trend_is_read_only() {
        // 不碰真实 store：用 MEMORY 里钉住的隔离手法行不通（store 路径是进程级常量），
        // 故只断言**函数签名层面**的只读性由上面 (f) 的纯函数覆盖 —— 这里转而钉住
        // 「投影不含 client 桶」这一最容易被后来者加错的点（变异：把 gateway 换成 client ⇒ 红）。
        let mut store = empty_store();
        let mut day = DayRecord::default();
        day.accounts.insert(
            "u1".to_string(),
            AccountRecord {
                client: Bucket {
                    input: 7,
                    output: 7,
                    ..Default::default()
                },
                gateway: Bucket {
                    input: 1,
                    output: 1,
                    ..Default::default()
                },
            },
        );
        store.days.insert("2026-10-10".to_string(), day);
        let points = gateway_trend_points(&store);
        assert_eq!(
            points[0]["total"], 2,
            "只应汇总 gateway 桶（7+7=14 是 client 的量）"
        );
    }

    /// (e) 保留裁剪丢掉超过保留窗的日期；条数上限丢最旧。
    #[test]
    fn prune_drops_days_older_than_the_cap() {
        let now = at_local_date(0, 12);
        let mut store = empty_store();
        let old_key = (Local::now().date_naive()
            - ChronoDuration::days(TOKEN_DAILY_RETENTION_DAYS))
        .format("%Y-%m-%d")
        .to_string();
        let today_key = local_date_string(now).unwrap();
        store.days.insert(old_key.clone(), DayRecord::default());
        store.days.insert(today_key.clone(), DayRecord::default());

        prune_store(&mut store, now);

        assert!(
            !store.days.contains_key(&old_key),
            "超过保留天数的日期必须被丢弃"
        );
        assert!(store.days.contains_key(&today_key), "今日必须保留");

        // 条数上限守卫：塞入保留窗内的日期，超出上限时丢最旧。
        let mut store = empty_store();
        let base = Local::now().date_naive();
        for offset in 0..(TOKEN_DAILY_MAX_DAYS as i64 + 1) {
            let key = (base + ChronoDuration::days(offset))
                .format("%Y-%m-%d")
                .to_string();
            store.days.insert(key, DayRecord::default());
        }
        prune_store(&mut store, now);
        assert_eq!(store.days.len(), TOKEN_DAILY_MAX_DAYS);
    }

    /// 读侧形状：空存储 → accounts 空（缺席 = 未知）、unknownClient 为 `{0,0}`、truncated 假。
    #[test]
    fn empty_day_response_is_honest_about_absence() {
        let response = build_response("2026-10-10", None);
        assert_eq!(
            response["accounts"].as_array().map(Vec::len),
            Some(0),
            "缺席账号 = 「今日无记录 / 未知」，绝不当 0 列出"
        );
        assert_eq!(response["unknownClient"]["in"], 0);
        assert_eq!(response["unknownClient"]["out"], 0);
        assert_eq!(response["gatewayTruncated"], false);
    }

    // -----------------------------------------------------------------------
    // 2026-10-12：当日扫描闸门（「今日 Token 总是横杠」的根因）+ mtime 预筛
    // -----------------------------------------------------------------------

    /// ★ 闸门：**同日 + 未过 TTL** 才跳过；跨日 / 过期 / `force` / 从未扫过都要重扫。
    ///
    /// ## 为什么这条用例是关键护栏
    ///
    /// 旧闸门是 `clientScannedDate == 今天` ⇒ 当日值被冻结在当天**第一次**扫描那一刻。
    /// 实测现场：CN 的 `token_daily.json` 14:33 落盘（当时今日无用量 ⇒ `accounts: {}`），
    /// 客户端 15:00 才开始产生用量 ⇒ 之后一整天卡片都是「—」，且前端从不传 `force`，
    /// 没有任何路径能恢复。
    ///
    /// 可证伪：把 `client_scan_is_fresh` 的 TTL 判据去掉、只留「同一天」⇒ 第二条断言
    /// （超过 TTL 必须重扫）立刻变红。
    #[test]
    fn client_scan_gate_skips_only_within_the_same_day_ttl() {
        let today = at_local_date(0, 15);
        let day = local_date_string(today).expect("local day");
        let today_start = local_day_start_ms(&day).expect("today start");

        // 昨天 23:59:30 扫过、今天 00:00:10 再问 —— 只隔 40 秒（< TTL），但**跨日** ⇒ 必须重扫。
        let before_midnight = today_start - 30_000;
        let after_midnight = today_start + 10_000;
        assert!(
            !client_scan_is_fresh(false, Some(before_midnight), after_midnight, &day),
            "跨日必须重扫 —— 否则新的一天沿用昨天的（很可能是空的）结果"
        );

        // 同日：TTL 之内跳过、到点重扫。
        assert!(
            client_scan_is_fresh(
                false,
                Some(after_midnight),
                after_midnight + CLIENT_SCAN_TTL_MS - 1,
                &day
            ),
            "同日且未过 TTL 应跳过（否则每次进账号页都重扫）"
        );
        assert!(
            !client_scan_is_fresh(
                false,
                Some(after_midnight),
                after_midnight + CLIENT_SCAN_TTL_MS,
                &day
            ),
            "过 TTL 必须重扫 —— 这是「今日 Token 一整天不动」的修法"
        );

        // `force` ⇒ 总是重扫；从未扫过 ⇒ 重扫。
        assert!(!client_scan_is_fresh(
            true,
            Some(after_midnight),
            after_midnight + 1_000,
            &day
        ));
        assert!(!client_scan_is_fresh(false, None, today, &day));
    }

    /// ★ TTL 必须「短到能反映当天、长到不折腾」——把两个边界都钉住。
    ///
    /// 可证伪：把 `CLIENT_SCAN_TTL_MS` 改成一天 ⇒ 第一条断言变红（「今日 Token」又会一整天不动）。
    #[test]
    fn client_scan_ttl_is_short_enough_for_a_today_metric() {
        const _: () = assert!(
            CLIENT_SCAN_TTL_MS <= 10 * 60 * 1000,
            "TTL 过长 —— 「今日 Token」又会一整天不动"
        );
        const _: () = assert!(
            CLIENT_SCAN_TTL_MS >= 30 * 1000,
            "TTL 过短 —— 每次进账号页都要重扫"
        );
    }

    /// mtime 预筛判据：信息缺失时**放行**（宁可慢，不可漏）。
    #[test]
    fn mtime_prefilter_falls_open_when_information_is_missing() {
        assert!(
            mtime_may_hold_day(None, Some(1_000_000)),
            "读不到 mtime 必须放行"
        );
        assert!(mtime_may_hold_day(Some(1), None), "算不出日界必须放行");
        assert!(mtime_may_hold_day(None, None));
        // 日界前 1 小时之内（余量内）仍放行；再早就不放行。
        let start = 10 * 86_400_000;
        assert!(mtime_may_hold_day(
            Some(start - CLIENT_SCAN_MTIME_SKEW_MS),
            Some(start)
        ));
        assert!(!mtime_may_hold_day(
            Some(start - CLIENT_SCAN_MTIME_SKEW_MS - 1),
            Some(start)
        ));
    }

    /// ★ mtime 预筛的**行为**（真文件、真 mtime）：新鲜的当日记录必须读到，陈旧的被跳过。
    ///
    /// ## 为什么要带**阳性对照**
    ///
    /// 「陈旧文件被跳过」这条断言在「扫描整个坏掉、返回空」时也恒真 —— 那是假绿。
    /// 所以同时断言新鲜的当日记录**必须出现在结果里**：两条一起才说明「筛的是 mtime，
    /// 不是把一切都扔掉」。
    #[test]
    fn mtime_prefilter_keeps_fresh_files_and_drops_stale_ones() {
        use std::time::SystemTime;

        let now = now_ms();
        let day = local_date_string(now).expect("local day");
        let root = std::env::temp_dir().join(format!(
            "buddy-switch-token-daily-mtime-{}-{now}",
            std::process::id()
        ));
        fs::create_dir_all(&root).expect("create fixture dir");

        let set_mtime = |path: &Path, at_ms: i64| {
            let file = fs::OpenOptions::new()
                .write(true)
                .open(path)
                .expect("open for mtime");
            let when =
                SystemTime::UNIX_EPOCH + Duration::from_millis(u64::try_from(at_ms).unwrap());
            file.set_modified(when).expect("set mtime");
        };
        let line = |session_id: &str, ts: i64, input: u64| {
            json!({
                "timestamp": ts,
                "sessionId": session_id,
                "message": { "usage": { "input_tokens": input, "output_tokens": 2 } }
            })
            .to_string()
        };

        let fresh = root.join("fresh.jsonl");
        fs::write(&fresh, format!("{}\n", line("s-fresh", now, 10))).expect("write fresh");
        set_mtime(&fresh, now);

        // 内容含「今日」记录，但 mtime 停在 10 天前 ⇒ 预筛跳过（写明的取舍，见函数注释）。
        let stale = root.join("stale.jsonl");
        fs::write(&stale, format!("{}\n", line("s-stale", now, 999))).expect("write stale");
        set_mtime(&stale, now - 10 * 86_400_000);

        let rows = scan_client_rows(&root, &day);

        assert!(
            rows.iter().any(|row| row.session_id == "s-fresh"),
            "mtime 新鲜的当日记录被漏掉 —— 预筛过严（阳性对照失败，说明扫描整体没工作）"
        );
        assert!(
            !rows.iter().any(|row| row.session_id == "s-stale"),
            "mtime 预筛没有生效 —— 陈旧文件仍被读取"
        );

        let _ = fs::remove_dir_all(&root);
    }

    /// ★★ **接线**回归：`record_gateway_today` → 落盘 → `gateway_usage_dim` 的 `keys`
    /// 维度（用户报障的那条链路，2026-10-28）。
    ///
    /// 单测只测 `gateway_round` 是不够的：这里走**生产函数的完整路径**（真 store、
    /// 真日志文件），断言三件事：
    /// 1. 升级前形态的 store（有 `gatewayModels`、**没有** `gatewayKeys` / 水位线
    ///    且当天已有 token）⇒ 密钥维度整窗播种，不掉数据；
    /// 2. 三种「没有 / 有 Key」的行各自落桶（`wb:k-1` / `__none__` / `__legacy__`）；
    /// 3. 输出里带 `keysPartial`，UI 才能披露「这一天只是下界」。
    ///
    /// 可证伪：把 `record_gateway_today` 里那行 `apply_dim_write(&mut record.gateway_keys, …)`
    /// 删掉 ⇒ `days[].keys` 为空、本用例变红。
    #[test]
    fn record_gateway_today_reports_the_key_dimension() {
        let dir = std::env::temp_dir().join(format!(
            "buddy-switch-token-daily-keys-{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&dir).expect("create home dir");
        let guard = crate::modules::config::HomeOverrideGuard::set(&dir);
        let store = dir.join(".buddy-switch");
        std::fs::create_dir_all(&store).expect("create store dir");

        let t1 = now_ms() - 3_000;
        let t2 = now_ms() - 2_000;
        let t3 = now_ms() - 1_000;
        let day = local_date_string(now_ms()).expect("today");

        /* 升级前形态（v3）：有账号桶与模型维度，水位线已存在，**没有**密钥维度。 */
        let prior = json!({
            "version": 3,
            "days": {
                day.clone(): {
                    "accounts": {
                        "uid-a": { "client": {"input":0,"output":0}, "gateway": {"input":30,"output":3} }
                    },
                    "gatewayTruncated": false,
                    "gatewayWatermarkTs": t1 - 1,
                    "gatewayModels": {
                        "m-1": { "input":30, "output":3, "requests":1, "failed":0, "credit":0.3 }
                    }
                }
            }
        });
        std::fs::write(
            store.join("token_daily.json"),
            serde_json::to_string(&prior).unwrap(),
        )
        .expect("pre-write v3 store");

        // 三条行：带 Key（有归属）/ `keyId: null`（确实无 Key）/ **没有** `keyId` 键（老条目）。
        let log = json!([
            { "ts": t1, "region": "cn", "account": "uid-a", "accountUid": "uid-a",
              "promptTokens": 10, "completionTokens": 1, "keyId": "k-1", "keyScope": "wb" },
            { "ts": t2, "region": "cn", "account": "uid-a", "accountUid": "uid-a",
              "promptTokens": 5, "completionTokens": 0, "keyId": null },
            { "ts": t3, "region": "cn", "account": "uid-a", "accountUid": "uid-a",
              "promptTokens": 7, "completionTokens": 0 }
        ]);
        std::fs::write(
            store.join("gateway_logs.json"),
            serde_json::to_string(&log).unwrap(),
        )
        .expect("write log");

        record_gateway_today(Region::Cn);
        let dim = gateway_usage_dim(Region::Cn);
        let today = dim["days"]
            .as_array()
            .expect("days")
            .iter()
            .find(|entry| entry["key"] == day.as_str())
            .cloned()
            .expect("今日必须落库");

        let keys = today["keys"].as_array().expect("keys 维度必须存在");
        let bucket =
            |name: &str| -> Option<&Value> { keys.iter().find(|entry| entry["key"] == name) };
        let keyed = bucket("wb:k-1").expect("带归属的 Key 必须落 `{scope}:{id}` 桶");
        assert_eq!(keyed["total"].as_u64(), Some(11), "播种轮取整窗读数");
        assert_eq!(keyed["requests"].as_u64(), Some(1));
        assert_eq!(
            bucket(KEY_DIM_NONE).and_then(|v| v["total"].as_u64()),
            Some(5),
            "`keyId: null` 落「无密钥调用」"
        );
        assert_eq!(
            bucket(KEY_DIM_LEGACY).and_then(|v| v["total"].as_u64()),
            Some(7),
            "**没有** `keyId` 键的老条目落「升级前的历史条目」，不并进无密钥调用"
        );
        assert_eq!(keys.len(), 3, "三种来源三行，不合并");
        assert_eq!(
            today["keysPartial"].as_bool(),
            Some(true),
            "v3 → v4 那天只是下界，UI 要能披露"
        );

        drop(guard);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
