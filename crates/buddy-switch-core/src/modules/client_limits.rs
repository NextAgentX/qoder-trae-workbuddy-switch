//! 客户端限流可视化：**账号 × 模型 × 恢复时刻**（纯只读观测）。
//!
//! 三张已有的本地表 join 即得，**不依赖网关、不代理客户端流量、不要求客户端配合**：
//!
//! | # | 数据源 | 内容 |
//! |---|---|---|
//! | ① | `{data_dir}/projects/**/*.jsonl` | 限流错误本体 + 发生时刻 + `sessionId` |
//! | ② | `{data_dir}/workbuddy.db` → `sessions`（**只读**） | `model` + `user_id` |
//! | ③ | 账号库 `accounts.json` / `accounts.global.json` | `uid` → `nickname` |
//!
//! `{data_dir}` = CN `~/.workbuddy`、Global `~/.workbuddy-ai`（见 [`session_data_dir`]）。
//!
//! 完整背景与验收口径见 `.trellis/tasks/09-22-client-rate-limit-visibility/prd.md`。
//!
//! ## 硬约束（PRD R1–R10，改动前先读）
//!
//! - **只读**：`workbuddy.db` 以 `SQLITE_OPEN_READ_ONLY` 打开，**绝不写库**（它是 WAL 库，
//!   客户端运行时也在写；本功能不得要求用户关闭客户端）。
//! - **以 jsonl 为主表**：join 失败（会话已删除 / playground）**不得丢弃**记录，
//!   落「未知账号 / 未知模型」兜底（实测 76/399 会话 join 不上）。
//! - **uid 走 `auth_raw.account.uid` / `profile_raw.uid`**，**绝不用**账号库本地 `id`
//!   （实测 `user_id` = 云端 uid ≠ 本地 `id`）。
//! - **恢复时刻固定按 UTC+8 解释**（复用 [`cst::cst_offset`]），**不按系统时区**。
//! - 解析失败保留原始 `code` / `message`，**不静默丢弃**。
//! - 本模块**不做任何写操作**、不改变账号状态、不触发切换。

use rusqlite::Connection;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::modules::account;
use crate::modules::config::now_ms;
use crate::modules::cst;
use crate::modules::region::Region;
use crate::modules::session::{session_data_dir, workbuddy_db_path_for};

/// 默认扫描窗口（近 7 天，PRD S5）。
pub const DEFAULT_SINCE_DAYS: i64 = 7;

const DAY_MS: i64 = 24 * 60 * 60 * 1000;

/// 一条从 jsonl 提取的**原始**限流错误（未 join、未聚合）。
#[derive(Debug, Clone)]
struct RawClientError {
    session_id: String,
    timestamp: i64,
    code: Option<i64>,
    status: Option<i64>,
    message: String,
    agent: Option<String>,
    cwd: Option<String>,
    reset_at: Option<i64>,
}

/// 一条聚合后的「限流台账」条目（下发前端的形状）。
///
/// `account` / `model` 为 `None` 表示 **join 失败**（未知账号 / 未知模型）—— 由前端渲染
/// 「未知」而**不是**丢弃该条（PRD R1）。
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientLimitEntry {
    /// 账号昵称（join 失败为 `null`）。
    pub account: Option<String>,
    /// 原始 `user_id`（= 云端 uid），便于排障；join 失败为 `null`。
    pub account_uid: Option<String>,
    /// 会话所选模型；join 失败为 `null`。⚠️ `hy4-preview` 与 `hy4-preview-f` 是**两个**模型，不可前缀合并。
    pub model: Option<String>,
    /// 上游错误码（`6004` 模型级限流 / `6020` / `6022` 排队 / …）。
    pub code: Option<i64>,
    /// HTTP 状态码（多为 `429`）。
    pub status: Option<i64>,
    /// 恢复时刻（毫秒）；文案漂移解析不出为 `null`。
    pub reset_at: Option<i64>,
    /// 最近一次发生时刻（毫秒）。
    pub occurred_at: i64,
    /// `reset_at > now` —— 界面主排序依据。
    pub active: bool,
    /// **原始**上游文案（PRD R6：解析失败时保留原文，不静默丢弃）。
    pub message: String,
    /// 会话 ID（便于跳转定位）。
    pub session_id: String,
    /// 会话工作目录（可能为真实项目路径，展示层只给必要信息）。
    pub cwd: Option<String>,
    /// `providerData.agent`（`cli` / 子代理名）。
    pub agent: Option<String>,
    /// 所属 region（`cn` / `global`）。
    pub region: String,
}

/// 聚合键：`(uid, model, reset_at)` —— 同一三元组视为**同一次限流**（PRD 目标模型）。
type LimitKey = (Option<String>, Option<String>, Option<i64>);

/// 当前限流台账（按 region + 时间窗口）。
///
/// `since_days`：`None` ⇒ 默认近 7 天；`Some(0)` ⇒ 全量（无 mtime 上界）；
/// `Some(n>0)` ⇒ 近 n 天；其余（负数）⇒ 默认近 7 天。
///
/// 返回 `{ region, since, now, counts: { scanned, deduped, active }, limits: [...] }`。
pub fn client_limits_for(region: Region, since_days: Option<i64>) -> Value {
    let now = now_ms();
    let since_ms = resolve_since_ms(since_days, now);
    let errors = scan_client_errors_for(region, since_ms);
    let meta = load_session_meta_for(region);
    let names = load_uid_nicknames_for(region);
    let scanned = errors.len();
    let limits = build_limits(region, errors, &meta, &names, now);
    let active = limits.iter().filter(|e| e.active).count();
    json!({
        "region": region.as_str(),
        "since": if since_ms > 0 { json!(since_ms) } else { Value::Null },
        "now": now,
        "counts": {
            "scanned": scanned,
            "deduped": limits.len(),
            "active": active,
        },
        "limits": limits,
    })
}

/// 扫描窗口的毫秒下界；`0` 表示全量（无上界）。
fn resolve_since_ms(since_days: Option<i64>, now: i64) -> i64 {
    match since_days {
        Some(0) => 0,
        Some(days) if days > 0 => now - days * DAY_MS,
        _ => now - DEFAULT_SINCE_DAYS * DAY_MS,
    }
}

// ---------------------------------------------------------------------------
// ① jsonl 扫描
// ---------------------------------------------------------------------------

/// 递归扫 `{data_dir}/projects/**/*.jsonl`，提取 `providerData.error.code` 存在的记录。
///
/// `since_ms > 0` 时按文件 `mtime` 过滤（PRD R10 有界扫描）；`since_ms <= 0` 全量。
fn scan_client_errors_for(region: Region, since_ms: i64) -> Vec<RawClientError> {
    let projects = session_data_dir(region).join("projects");
    let mut files: Vec<PathBuf> = Vec::new();
    collect_jsonl_files(&projects, since_ms, &mut files);
    let mut out = Vec::new();
    for path in files {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        for line in text.lines() {
            if let Some(record) = parse_error_line(line) {
                out.push(record);
            }
        }
    }
    out
}

/// 递归收集 `.jsonl` 文件；按 `mtime` 过滤（`since_ms > 0` 时）。
///
/// 取不到 `mtime` 时**不排除**（宁可多扫一个文件，也不漏一条记录）——与 R1 的
/// 「join 失败不丢弃」同一取向：观测功能的默认倾向是**保守保留**。
fn collect_jsonl_files(dir: &Path, since_ms: i64, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_jsonl_files(&path, since_ms, out);
            continue;
        }
        if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
            continue;
        }
        if since_ms > 0 {
            let fresh = entry
                .metadata()
                .ok()
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_millis() as i64 >= since_ms)
                .unwrap_or(true);
            if !fresh {
                continue;
            }
        }
        out.push(path);
    }
}

/// 从一行 jsonl 提取限流错误；非错误行 / 结构缺失返回 `None`。
///
/// 取 `providerData.error` 为对象且 `code` 存在的记录（PRD S1）。`code` 缺失的行
/// （实测 24 条，多为非上游错误）**不取** —— 它们不是「限流」事件。
fn parse_error_line(line: &str) -> Option<RawClientError> {
    let line = line.trim();
    if line.is_empty() || !line.contains("\"error\"") {
        return None;
    }
    let value: Value = serde_json::from_str(line).ok()?;
    let provider = value.get("providerData")?;
    let error = provider.get("error")?;
    if !error.is_object() {
        return None;
    }
    let code = error.get("code").and_then(Value::as_i64)?;
    let message = error
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    Some(RawClientError {
        session_id: text_field(&value, "sessionId"),
        timestamp: value.get("timestamp").and_then(Value::as_i64).unwrap_or(0),
        code: Some(code),
        status: error.get("status").and_then(Value::as_i64),
        reset_at: parse_reset_at_ms(&message),
        message,
        agent: provider
            .get("agent")
            .and_then(Value::as_str)
            .map(|s| s.to_string()),
        cwd: value
            .get("cwd")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string()),
    })
}

fn text_field(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

// ---------------------------------------------------------------------------
// ② sessions 表（只读）
// ---------------------------------------------------------------------------

/// 读 `sessions` 表取 `sessionId → (model, user_id)` 映射；**只读**打开（PRD R4）。
///
/// 列缺失时（老客户端 schema）用 `NULL` 占位而非整表失败 —— join 退化为「未知」，
/// 但**不丢记录**（R1）。表不存在 / 库不可读返回空表。
fn load_session_meta_for(region: Region) -> HashMap<String, (Option<String>, Option<String>)> {
    let mut map = HashMap::new();
    let db = workbuddy_db_path_for(region);
    if !db.is_file() {
        return map;
    }
    let Ok(conn) = Connection::open_with_flags(
        &db,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    ) else {
        return map;
    };
    let _ = conn.busy_timeout(Duration::from_secs(5));
    let columns = table_columns(&conn, "sessions");
    if !columns.iter().any(|c| c == "id") {
        return map;
    }
    let model_expr = if columns.iter().any(|c| c == "model") {
        "model"
    } else {
        "NULL"
    };
    let uid_expr = if columns.iter().any(|c| c == "user_id") {
        "user_id"
    } else {
        "NULL"
    };
    let sql = format!("SELECT id, {model_expr}, {uid_expr} FROM sessions");
    let Ok(mut stmt) = conn.prepare(&sql) else {
        return map;
    };
    let Ok(rows) = stmt.query_map([], |row| {
        Ok((
            row.get::<_, Option<String>>(0)?,
            row.get::<_, Option<String>>(1)?,
            row.get::<_, Option<String>>(2)?,
        ))
    }) else {
        return map;
    };
    for (id, model, uid) in rows.flatten() {
        let Some(id) = nonempty(id) else {
            continue;
        };
        map.insert(id, (nonempty(model), nonempty(uid)));
    }
    map
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
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

// ---------------------------------------------------------------------------
// ③ 账号库 uid → nickname
// ---------------------------------------------------------------------------

/// 读账号库构造 `uid → nickname` 映射（PRD R2：uid 走 `auth_raw.account.uid` / `profile_raw.uid`）。
fn load_uid_nicknames_for(region: Region) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for acc in account::load_accounts_for(region) {
        let Some(uid) = account_uid(&acc) else {
            continue;
        };
        map.entry(uid)
            .or_insert_with(|| account::account_display_name(&acc));
    }
    map
}

/// 从账号记录提取 uid：`auth_raw.account.uid`（CN）→ `profile_raw.uid`（Global）→ 顶层 `uid`。
///
/// ⚠️ **绝不**用本地 `id`（PRD R2）。Global 的 `auth_raw` 顶层键与 CN 不同（实测只有
/// `accessToken` 等，**没有** `account`），故需要第二条路径 `profile_raw.uid`；顶层 `uid`
/// 是两者都有的兜底（实测与上述两处逐字相等）。
fn account_uid(acc: &Value) -> Option<String> {
    let pick = |v: &Value| {
        v.as_str()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(String::from)
    };
    acc.get("auth_raw")
        .and_then(|a| a.get("account"))
        .and_then(|a| a.get("uid"))
        .and_then(pick)
        .or_else(|| {
            acc.get("profile_raw")
                .and_then(|p| p.get("uid"))
                .and_then(pick)
        })
        .or_else(|| acc.get("uid").and_then(pick))
}

// ---------------------------------------------------------------------------
// ④ 聚合与状态判定
// ---------------------------------------------------------------------------

/// 按 `(uid, model, reset_at)` 聚合去重，取最近一条；`active = reset_at > now`。
///
/// **纯函数**（显式入参，不碰磁盘）—— 便于单测覆盖 join / 去重 / 兜底三条链。
fn build_limits(
    region: Region,
    errors: Vec<RawClientError>,
    meta: &HashMap<String, (Option<String>, Option<String>)>,
    names: &HashMap<String, String>,
    now: i64,
) -> Vec<ClientLimitEntry> {
    let mut map: HashMap<LimitKey, ClientLimitEntry> = HashMap::new();
    for error in errors {
        let (model, uid) = meta.get(&error.session_id).cloned().unwrap_or((None, None));
        let key: LimitKey = (uid.clone(), model.clone(), error.reset_at);
        let entry = map.entry(key).or_insert_with(|| ClientLimitEntry {
            account: uid.as_ref().and_then(|u| names.get(u).cloned()),
            account_uid: uid.clone(),
            model: model.clone(),
            code: error.code,
            status: error.status,
            reset_at: error.reset_at,
            occurred_at: error.timestamp,
            active: error.reset_at.map(|r| r > now).unwrap_or(false),
            message: error.message.clone(),
            session_id: error.session_id.clone(),
            cwd: error.cwd.clone(),
            agent: error.agent.clone(),
            region: region.as_str().to_string(),
        });
        if error.timestamp > entry.occurred_at {
            entry.occurred_at = error.timestamp;
            entry.code = error.code;
            entry.status = error.status;
            entry.message = error.message;
            entry.session_id = error.session_id;
            entry.cwd = error.cwd;
            entry.agent = error.agent;
        }
    }
    let mut limits: Vec<ClientLimitEntry> = map.into_values().collect();
    limits.sort_by(|a, b| {
        b.active
            .cmp(&a.active)
            .then(b.reset_at.unwrap_or(0).cmp(&a.reset_at.unwrap_or(0)))
            .then(b.occurred_at.cmp(&a.occurred_at))
    });
    limits
}

// ---------------------------------------------------------------------------
// 时间解析
// ---------------------------------------------------------------------------

/// 从上游错误文案解析「恢复时刻」（毫秒）。
///
/// ⚠️ **锚时间戳形态，不锚周边文案**（PRD 附录 C3）：中英文模板都含
/// `<YYYY-MM-DD HH:MM:SS> UTC+8`；文案会漂移，形态稳定。
///
/// 解析失败返回 `None`（R6：调用方保留原文，**不静默丢弃**）。
fn parse_reset_at_ms(message: &str) -> Option<i64> {
    for (idx, _) in message.match_indices("UTC+8") {
        let prefix = message.get(..idx)?.trim_end();
        let start = prefix.len().checked_sub(19)?;
        // `start` 可能落在多字节字符中间 ⇒ `get` 返回 `None`，跳过该命中点。
        let Some(candidate) = prefix.get(start..) else {
            continue;
        };
        if let Some(ms) = parse_cst_datetime(candidate) {
            return Some(ms);
        }
    }
    None
}

/// `YYYY-MM-DD[ T]HH:MM:SS` → 按**固定 UTC+8** 解释的毫秒时间戳。
fn parse_cst_datetime(text: &str) -> Option<i64> {
    let bytes = text.as_bytes();
    if bytes.len() != 19 {
        return None;
    }
    if bytes[4] != b'-' || bytes[7] != b'-' || bytes[13] != b':' || bytes[16] != b':' {
        return None;
    }
    if bytes[10] != b' ' && bytes[10] != b'T' {
        return None;
    }
    let num = |range: std::ops::Range<usize>| -> Option<i32> { text.get(range)?.parse::<i32>().ok() };
    let year = num(0..4)?;
    let month = num(5..7)? as u32;
    let day = num(8..10)? as u32;
    let hour = num(11..13)? as u32;
    let minute = num(14..16)? as u32;
    let second = num(17..19)? as u32;
    let naive = chrono::NaiveDate::from_ymd_opt(year, month, day)?.and_hms_opt(hour, minute, second)?;
    use chrono::TimeZone;
    cst::cst_offset()
        .from_local_datetime(&naive)
        .single()
        .map(|dt| dt.timestamp_millis())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};

    fn cst_millis(year: i32, month: u32, day: u32, hour: u32, minute: u32) -> i64 {
        cst::cst_ms(year, month, day, hour, minute)
    }

    fn raw(
        session_id: &str,
        timestamp: i64,
        code: i64,
        message: &str,
        reset_at: Option<i64>,
    ) -> RawClientError {
        RawClientError {
            session_id: session_id.to_string(),
            timestamp,
            code: Some(code),
            status: Some(429),
            message: message.to_string(),
            agent: Some("cli".to_string()),
            cwd: Some("c:\\proj".to_string()),
            reset_at,
        }
    }

    #[test]
    fn parse_reset_at_uses_fixed_utc8_for_chinese_template() {
        let message = "429 您的使用量已超出频率限制，将在 2026-09-16 16:34:24 UTC+8 重置，您也可以切换其他模型继续使用。 (req/abc)";
        // 2026-09-16 16:34:24 UTC+8 == 2026-09-16 08:34:24 UTC。
        // 断言**固定 UTC 瞬时**（而非本机本地时刻）：若把基准从 UTC+8 改成系统本地时区，
        // 在非 UTC+8 机器上这里即变红（PRD 验收 5 的可证伪点）。
        let expected = Utc
            .with_ymd_and_hms(2026, 9, 16, 8, 34, 24)
            .single()
            .expect("合法 UTC")
            .timestamp_millis();
        assert_eq!(parse_reset_at_ms(message), Some(expected));
    }

    #[test]
    fn parse_reset_at_handles_english_template_and_t_separator() {
        // 2026-09-16 16:34:24 UTC+8 == 2026-09-16 08:34:24 UTC。
        let expected = Utc
            .with_ymd_and_hms(2026, 9, 16, 8, 34, 24)
            .single()
            .expect("合法 UTC")
            .timestamp_millis();
        let english = "429 usage exceeds frequency limit, your usage will reset at 2026-09-16 16:34:24 UTC+8, alternatively...";
        assert_eq!(parse_reset_at_ms(english), Some(expected));
        let t_form = "reset at 2026-09-16T16:34:24 UTC+8";
        assert_eq!(parse_reset_at_ms(t_form), Some(expected));
    }

    #[test]
    fn parse_reset_at_returns_none_when_text_drifts() {
        // 文案漂移（无时间戳 / 无 UTC+8）⇒ None，调用方保留原文（R6）。
        assert_eq!(parse_reset_at_ms("429 queue.waiting.title"), None);
        assert_eq!(parse_reset_at_ms("reset at 2026-09-16 16:34:24"), None);
        assert_eq!(parse_reset_at_ms("将在稍后重置"), None);
        // 时间戳畸形 ⇒ None，而不是 panic。
        assert_eq!(parse_reset_at_ms("reset at 2026-13-40 99:99:99 UTC+8"), None);
    }

    #[test]
    fn parse_reset_at_survives_multibyte_prefix() {
        // 命中点前一个字符是多字节（中文）时，字节切片不得 panic。
        let message = "重置将在 2026-09-16 16:34:24 UTC+8 生效";
        let expected = Utc
            .with_ymd_and_hms(2026, 9, 16, 8, 34, 24)
            .single()
            .expect("合法 UTC")
            .timestamp_millis();
        assert_eq!(parse_reset_at_ms(message), Some(expected));
    }

    #[test]
    fn account_uid_prefers_auth_then_profile_and_never_uses_local_id() {
        let cn = json!({
            "id": "local-id-cn",
            "uid": "uid-top",
            "auth_raw": { "account": { "uid": "uid-auth" } },
            "profile_raw": { "uid": "uid-profile" },
        });
        assert_eq!(account_uid(&cn).as_deref(), Some("uid-auth"));

        // Global：auth_raw 无 account（实测）⇒ 走 profile_raw。
        let global = json!({
            "id": "local-id-global",
            "uid": "uid-top",
            "auth_raw": { "accessToken": "x" },
            "profile_raw": { "uid": "uid-profile" },
        });
        assert_eq!(account_uid(&global).as_deref(), Some("uid-profile"));

        // 都没有 ⇒ 顶层 uid 兜底；**绝不**返回本地 id。
        let bare = json!({ "id": "local-id-only", "uid": "uid-top" });
        assert_eq!(account_uid(&bare).as_deref(), Some("uid-top"));

        let no_uid = json!({ "id": "local-id-only" });
        assert_eq!(account_uid(&no_uid), None);
    }

    #[test]
    fn build_limits_dedupes_same_uid_model_reset_and_keeps_latest() {
        let now = cst_millis(2026, 9, 16, 10, 0);
        let reset = cst_millis(2026, 9, 16, 11, 30);
        let mut meta = HashMap::new();
        meta.insert(
            "s1".to_string(),
            (Some("deepseek-v4.1-flash".to_string()), Some("uid-a".to_string())),
        );
        meta.insert(
            "s2".to_string(),
            (Some("deepseek-v4.1-flash".to_string()), Some("uid-a".to_string())),
        );
        let mut names = HashMap::new();
        names.insert("uid-a".to_string(), "Jackey".to_string());

        // 三条同一 (uid, model, reset_at) —— 实测同一恢复时刻会在多条记录 / 多个会话里重复。
        let errors = vec![
            raw("s1", cst_millis(2026, 9, 16, 9, 0), 6004, "第一条", Some(reset)),
            raw("s2", cst_millis(2026, 9, 16, 9, 30), 6004, "第二条(最新)", Some(reset)),
            raw("s1", cst_millis(2026, 9, 16, 9, 15), 6004, "中间", Some(reset)),
        ];
        let limits = build_limits(Region::Cn, errors, &meta, &names, now);
        assert_eq!(limits.len(), 1, "同 (uid, model, reset_at) 必须聚合为一条");
        let entry = &limits[0];
        assert_eq!(entry.account.as_deref(), Some("Jackey"));
        assert_eq!(entry.model.as_deref(), Some("deepseek-v4.1-flash"));
        assert_eq!(entry.occurred_at, cst_millis(2026, 9, 16, 9, 30));
        assert_eq!(entry.message, "第二条(最新)");
        assert!(entry.active, "reset_at 在未来 ⇒ active");
    }

    #[test]
    fn build_limits_keeps_join_failures_as_unknown() {
        let now = cst_millis(2026, 9, 16, 10, 0);
        // meta 里**没有** "ghost"（已删除 / playground）⇒ join 失败。
        let meta: HashMap<String, (Option<String>, Option<String>)> = HashMap::new();
        let names: HashMap<String, String> = HashMap::new();
        let errors = vec![raw("ghost", now - 1000, 6004, "429 ...", None)];
        let limits = build_limits(Region::Cn, errors, &meta, &names, now);
        assert_eq!(limits.len(), 1, "join 失败不得丢弃（R1）");
        assert_eq!(limits[0].account, None);
        assert_eq!(limits[0].account_uid, None);
        assert_eq!(limits[0].model, None);
        assert_eq!(limits[0].session_id, "ghost");
        assert!(!limits[0].active, "reset_at 未知 ⇒ 不判 active");
    }

    #[test]
    fn build_limits_sorts_active_first_then_by_reset() {
        let now = cst_millis(2026, 9, 16, 10, 0);
        let mut meta = HashMap::new();
        meta.insert("a".to_string(), (Some("m".to_string()), Some("uid-a".to_string())));
        meta.insert("b".to_string(), (Some("m".to_string()), Some("uid-b".to_string())));
        let names: HashMap<String, String> = HashMap::new();
        let errors = vec![
            // 已恢复（过去）
            raw("a", now - 60_000, 6004, "old", Some(now - 60_000)),
            // 仍受限（未来）
            raw("b", now - 60_000, 6004, "live", Some(now + 3_600_000)),
        ];
        let limits = build_limits(Region::Cn, errors, &meta, &names, now);
        assert_eq!(limits.len(), 2);
        assert!(limits[0].active, "仍受限的排最前");
        assert_eq!(limits[0].account_uid.as_deref(), Some("uid-b"));
        assert!(!limits[1].active);
    }

    #[test]
    fn resolve_since_ms_default_all_and_days() {
        let now = 1_000_000_000_000;
        assert_eq!(resolve_since_ms(None, now), now - DEFAULT_SINCE_DAYS * DAY_MS);
        assert_eq!(resolve_since_ms(Some(0), now), 0);
        assert_eq!(resolve_since_ms(Some(30), now), now - 30 * DAY_MS);
        assert_eq!(resolve_since_ms(Some(-5), now), now - DEFAULT_SINCE_DAYS * DAY_MS);
    }
}