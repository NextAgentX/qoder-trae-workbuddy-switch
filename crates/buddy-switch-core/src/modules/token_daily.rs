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
//! - **诚实**：网关卡被裁剪过（条数到达 `log_keep`）时透出 `gatewayTruncated`；无法归属的
//!   客户端会话落 `unknownClient` 桶，**绝不猜**到别的账号，也**绝不静默丢弃**。
//! - **不重复扫 18MB 文件**：客户端扫描带「今日已扫」闸门（`force` 可强制重扫）。

use chrono::{Duration as ChronoDuration, Local, NaiveDate, TimeZone};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use rusqlite::Connection;

use crate::modules::account;
use crate::modules::config::{atomic_write, now_ms, store_dir};
use crate::modules::region::{gateway_config_file, token_daily_file_for, Region};
use crate::modules::session::{session_data_dir, workbuddy_db_path_for};

/// 每日记录保留天数（镜像 `credit_usage::CREDIT_SNAPSHOT_RETENTION_DAYS`）。
pub const TOKEN_DAILY_RETENTION_DAYS: i64 = 90;
/// 每日记录条数上限（**安全网**：保留窗内还可能出现病态/未来的日期，超出即丢最旧）。
pub const TOKEN_DAILY_MAX_DAYS: usize = 400;
/// 存储 schema 版本。
pub const TOKEN_DAILY_VERSION: u32 = 1;

/// 网关日志条数上限的缺省值（与 `GatewayConfig::default().log_keep` 同值）。
const DEFAULT_GATEWAY_LOG_KEEP: usize = 200;

static TOKEN_DAILY_WRITE_LOCK: Mutex<()> = Mutex::new(());

// ---------------------------------------------------------------------------
// 存储形状
// ---------------------------------------------------------------------------

/// 一个来源（client / gateway）的输入 / 输出累计。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
struct Bucket {
    #[serde(default)]
    input: u64,
    #[serde(default)]
    output: u64,
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
}

/// 落盘存储：`{ version, days: { "YYYY-MM-DD": DayRecord }, clientScannedDate }`。
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Store {
    #[serde(default = "default_version")]
    version: u32,
    #[serde(default)]
    days: BTreeMap<String, DayRecord>,
    /// 客户端扫描「今日已算」标记（本地日期尾）。非 `force` 且相等时不再重扫。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    client_scanned_date: Option<String>,
}

fn default_version() -> u32 {
    TOKEN_DAILY_VERSION
}

fn empty_store() -> Store {
    Store {
        version: TOKEN_DAILY_VERSION,
        days: BTreeMap::new(),
        client_scanned_date: None,
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

/// 日志条数到达保留上限 ⇒ 可能已被裁剪（诚实标注，不假装是完整总量）。
fn gateway_log_truncated(entries_len: usize, keep: usize) -> bool {
    entries_len >= keep.max(1)
}

#[derive(Clone, Debug)]
struct GatewayRow {
    region: String,
    /// 展示名（仅作**旧行**回落的解析输入）。
    account: String,
    /// 写入时记录的精确 uid（新行必有；旧行为 `None`）。
    account_uid: Option<String>,
    day: String,
    input: u64,
    output: u64,
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
                region,
                account,
                account_uid,
                day,
                input: value.get("promptTokens").and_then(number_u64).unwrap_or(0),
                output: value
                    .get("completionTokens")
                    .and_then(number_u64)
                    .unwrap_or(0),
            })
        })
        .collect()
}

/// 按归属键分桶，只保留「该 region + 该本地日」的行。
///
/// 归属键：**优先**行内写入时的 `accountUid`（精确身份）；仅当其为空（**旧日志**，
/// 本改动之前写入）才回落到把展示名 `account` 解析回 uid。
fn bucket_gateway(
    rows: &[GatewayRow],
    region: Region,
    day: &str,
    uids: &HashSet<String>,
    names: &HashMap<String, Option<String>>,
) -> BTreeMap<String, Bucket> {
    let mut out: BTreeMap<String, Bucket> = BTreeMap::new();
    for row in rows {
        if row.region != region.as_str() || row.day != day {
            continue;
        }
        let key = match row.account_uid.as_deref() {
            Some(uid) => uid.to_string(),
            None => resolve_account_key(&row.account, uids, names),
        };
        let bucket = out.entry(key).or_default();
        bucket.input = bucket.input.saturating_add(row.input);
        bucket.output = bucket.output.saturating_add(row.output);
    }
    out
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
fn load_gateway_identity(
    region: Region,
) -> (HashSet<String>, HashMap<String, Option<String>>) {
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

#[derive(Clone, Debug)]
struct ClientUsageRow {
    session_id: String,
    ts: i64,
    input: u64,
    output: u64,
}

/// 递归收集 `.jsonl`；**排除 `subagents`**（子代理日志会重复父会话上下文，
/// 与 `token_stats` 的主用量口径一致）。
fn collect_jsonl_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if path.file_name().and_then(|name| name.to_str()) == Some("subagents") {
                continue;
            }
            collect_jsonl_files(&path, out);
        } else if path.extension().and_then(|extension| extension.to_str()) == Some("jsonl") {
            out.push(path);
        }
    }
}

fn number_u64(value: &Value) -> Option<u64> {
    value
        .as_u64()
        .or_else(|| value.as_i64().and_then(|n| u64::try_from(n).ok()))
        .or_else(|| value.as_f64().filter(|n| n.is_finite() && *n >= 0.0).map(|n| n as u64))
        .or_else(|| value.as_str()?.trim().parse::<u64>().ok())
}

fn field_u64(object: &Map<String, Value>, keys: &[&str]) -> Option<u64> {
    keys.iter().find_map(|key| object.get(*key).and_then(number_u64))
}

/// usage 对象的锚点：必须含输入字段（输入可以为 0，故不以 `> 0` 判定）。
fn usage_object(value: &Value) -> Option<&Map<String, Value>> {
    value.as_object().filter(|object| {
        field_u64(object, &["input_tokens", "inputTokens", "prompt_tokens"]).is_some()
    })
}

/// 提取 (input, output)：优先级 `message.usage` > `providerData.usage` > 顶层 `usage`
/// （与 `token_stats` 的取值口径一致）。
fn extract_usage(value: &Value) -> Option<(u64, u64)> {
    let provider = value.get("providerData");
    let candidates = [
        value.get("message").and_then(|message| message.get("usage")),
        provider.and_then(|data| data.get("usage")),
        value.get("usage"),
    ];
    let selected = candidates.into_iter().flatten().find_map(usage_object)?;
    let input =
        field_u64(selected, &["input_tokens", "inputTokens", "prompt_tokens"]).unwrap_or(0);
    let output =
        field_u64(selected, &["output_tokens", "outputTokens", "completion_tokens"]).unwrap_or(0);
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
    })
}

/// 流式扫描该 region 的 CLI 会话日志，只保留**今日**的用量行。
fn scan_client_rows(region: Region, day: &str) -> Vec<ClientUsageRow> {
    let root = session_data_dir(region).join("projects");
    let mut files = Vec::new();
    collect_jsonl_files(&root, &mut files);
    let mut rows = Vec::new();
    for path in files {
        let Ok(file) = std::fs::File::open(&path) else {
            continue;
        };
        for line in BufReader::new(file).lines() {
            let Ok(line) = line else {
                continue;
            };
            if let Some(row) = parse_client_line(&line) {
                if local_date_string(row.ts).as_deref() == Some(day) {
                    rows.push(row);
                }
            }
        }
    }
    rows
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
pub fn record_gateway_today(region: Region) -> Value {
    let at = now_ms();
    let day = local_date_string(at).unwrap_or_default();
    let entries = load_gateway_log();
    let truncated = gateway_log_truncated(entries.len(), gateway_log_keep());
    let (uids, names) = load_gateway_identity(region);
    let per_account = bucket_gateway(&parse_gateway_rows(&entries), region, &day, &uids, &names);

    let _guard = TOKEN_DAILY_WRITE_LOCK.lock().unwrap();
    let mut store = load_store(region);
    {
        let record = store.days.entry(day.clone()).or_default();
        for (account, bucket) in per_account {
            record.accounts.entry(account).or_default().gateway = bucket;
        }
        record.gateway_truncated = truncated;
    }
    prune_store(&mut store, at);
    save_store(region, &store);
    build_response(&day, store.days.get(&day))
}

/// 聚合客户端会话日志到「今日」的 client 桶并落盘，返回今日记录。
///
/// `force = false` 且今日已扫过时**直接返回**（不重扫 18MB 文件）；`force = true`
/// 强制重扫（当日数据在扫过之后可能仍会增长）。
pub fn record_client_today(region: Region, force: bool) -> Value {
    let at = now_ms();
    let day = local_date_string(at).unwrap_or_default();

    // 闸门：非强制且今日已算 ⇒ 绝不重扫。
    {
        let store = load_store(region);
        if !force && store.client_scanned_date.as_deref() == Some(day.as_str()) {
            return build_response(&day, store.days.get(&day));
        }
    }

    let meta = load_session_user_map(region);
    let rows = scan_client_rows(region, &day);
    let (per_account, unknown) = bucket_client(&rows, &meta, &day);

    let _guard = TOKEN_DAILY_WRITE_LOCK.lock().unwrap();
    // 锁内重读：扫描期间 gateway 可能刚写过，避免丢更新。
    let mut store = load_store(region);
    {
        let record = store.days.entry(day.clone()).or_default();
        for (uid, bucket) in per_account {
            record.accounts.entry(uid).or_default().client = bucket;
        }
        record.unknown_client =
            (unknown.input > 0 || unknown.output > 0).then_some(unknown);
    }
    store.client_scanned_date = Some(day.clone());
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

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Datelike;

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
        }
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
        assert_eq!(unknown.input, 7, "无法归属的用量必须进 unknown，绝不并入 uid-a");
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
        );

        assert_eq!(per_account.len(), 1);
        assert_eq!(per_account.get("uid-a").unwrap().input, 10, "只取 cn + 今日");
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
        let uids: HashSet<String> =
            HashSet::from(["uid-a".to_string(), "uid-b".to_string()]);
        let mut names: HashMap<String, Option<String>> = HashMap::new();
        names.insert("a@example.com".to_string(), Some("uid-a".to_string()));
        names.insert("重名".to_string(), None);

        let per_account =
            bucket_gateway(&parse_gateway_rows(&entries), Region::Cn, &day, &uids, &names);

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

    /// (d) `gatewayTruncated` 仅在日志到达保留上限时为真。
    #[test]
    fn gateway_truncated_only_at_the_retention_cap() {
        assert!(!gateway_log_truncated(5, 200));
        assert!(!gateway_log_truncated(199, 200));
        assert!(gateway_log_truncated(200, 200));
        assert!(gateway_log_truncated(201, 200));
    }

    /// (e) 保留裁剪丢掉超过保留窗的日期；条数上限丢最旧。
    #[test]
    fn prune_drops_days_older_than_the_cap() {
        let now = at_local_date(0, 12);
        let mut store = empty_store();
        let old_key = (Local::now().date_naive() - ChronoDuration::days(TOKEN_DAILY_RETENTION_DAYS))
            .format("%Y-%m-%d")
            .to_string();
        let today_key = local_date_string(now).unwrap();
        store.days.insert(old_key.clone(), DayRecord::default());
        store.days.insert(today_key.clone(), DayRecord::default());

        prune_store(&mut store, now);

        assert!(!store.days.contains_key(&old_key), "超过保留天数的日期必须被丢弃");
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
}