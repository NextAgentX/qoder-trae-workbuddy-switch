//! 会话**导出 / 导入**（issue #87）—— **单文件 JSON 包**。
//!
//! ## 为什么是「一个 JSON 文件」而不是 zip
//!
//! 本仓的依赖面是刻意收窄的（core 只有 serde_json / rusqlite / uuid 这一小撮）。
//! 为了一个「把会话搬到另一台机器」的功能引入 zip 编解码，等于给**所有**用户加一个
//! 解压攻击面（zip 路径穿越、zip bomb），而收益只是「少一个文件」。因此：
//!
//! - 导出 = 一个 `.json` 文件（内含 manifest + 每个会话的行数据与正文）；
//! - 导入 = 读同一个文件。
//!
//! 代价是正文在 JSON 里要转义、体积约翻倍；对「几百个会话、几十 MB」的量级可接受，
//! 而且用户拿到的是一份**人可读**、可 diff、可用任何工具检查的文件。
//!
//! ## 包内结构
//!
//! ```json
//! {
//!   "kind": "buddy-switch.sessions",
//!   "schema": 1,
//!   "exportedAt": "2026-11-03T03:00:00Z",
//!   "region": "cn",
//!   "sessions": [
//!     {
//!       "sourceCid": "<源会话 id>",
//!       "workspace": "ws-a",              // projects/ 下的目录名（不透明字符串）
//!       "title": "会话标题（给人看）",
//!       "row": { "columns": [...], "values": [...] },
//!       "jsonl": "<会话正文原文>"
//!     }
//!   ]
//! }
//! ```
//!
//! ## 三条硬约束
//!
//! 1. **导入只做加法**：每条会话都生成**新 id**，绝不覆盖既有会话（`INSERT OR REPLACE`
//!    是按新 id 插，撞不到任何现有行）。重复导入同一份包 ⇒ 得到两份副本，**不静默去重**
//!    —— 「我以为导入覆盖了，结果两份」比「我以为合并了，结果只有一份」更难查。
//! 2. **`workspace` 只当目录名**：它是扩展按 `md5(工作区)` 生成的不透明串，本层**不重算**，
//!    但也**不许**它带路径分隔符 / `..` / 绝对路径 —— 否则一份构造过的包能写到数据目录之外。
//! 3. **失败不半写**：正文写成功但索引行写失败 ⇒ 删掉刚写的正文再报错（不留孤儿文件，
//!    否则客户端看不到它、用户也不知道它占着盘）。

use base64::Engine;
use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};

use crate::modules::region::Region;
use crate::modules::session;

/// 包的类型标识（导入时**必须**逐字匹配，防止用户把别的 json 丢进来）。
pub const BUNDLE_KIND: &str = "buddy-switch.sessions";

/// 包的结构版本。**只在结构不兼容地变化时才加**；加之前先想清楚旧包怎么办。
pub const BUNDLE_SCHEMA: u64 = 1;

/// 导出：把 `cids` 指向的会话打进一个 JSON 文件。
///
/// 只读：不写客户端任何数据，也不改数据库。
pub fn export_sessions_for(
    region: Region,
    uid: &str,
    cids: &[String],
    out_path: &Path,
) -> Result<Value, String> {
    let projects = session::session_data_dir(region).join("projects");
    let index = session::build_projects_index_for(region);
    let db_path = session::workbuddy_db_path_for(region);
    let conn = session::open_db(&db_path, true);

    let mut sessions: Vec<Value> = Vec::new();
    let mut skipped: Vec<Value> = Vec::new();
    for cid in cids {
        let Some(jsonl_path) = index.get(cid.as_str()) else {
            skipped.push(json!({"id": cid, "reason": "找不到会话正文（可能已删除）"}));
            continue;
        };
        let workspace = match jsonl_path
            .parent()
            .and_then(|dir| dir.strip_prefix(&projects).ok())
            .and_then(|rel| rel.to_str())
        {
            Some(value) => value.to_string(),
            None => {
                skipped.push(json!({"id": cid, "reason": "会话正文不在 projects/ 下，已跳过"}));
                continue;
            }
        };
        let body = match std::fs::read_to_string(jsonl_path) {
            Ok(text) => text,
            Err(error) => {
                skipped.push(json!({"id": cid, "reason": format!("读正文失败：{error}")}));
                continue;
            }
        };
        // 索引行可以缺失（全新账号 / 只有正文的形态），导出时如实标出来：
        // 导入侧据此只写正文、不写行（并回报 `rowWritten: false`）。
        let row = match conn.as_ref() {
            Some(conn) => session::read_session_row(conn, cid, uid).unwrap_or(None),
            None => None,
        };
        let title = row
            .as_ref()
            .map(|(cols, vals)| session::row_title(cols, vals))
            .unwrap_or_else(|| "(无标题)".to_string());
        sessions.push(json!({
            "sourceCid": cid,
            "workspace": workspace,
            "title": title,
            "row": row.map(|(columns, values)| json!({
                "columns": columns,
                "values": values.iter().map(value_to_json).collect::<Vec<_>>(),
            })),
            "jsonl": body,
        }));
    }

    if sessions.is_empty() {
        return Err("没有可导出的会话".to_string());
    }

    let bundle = json!({
        "kind": BUNDLE_KIND,
        "schema": BUNDLE_SCHEMA,
        "exportedAt": crate::modules::config::utc_iso(),
        "region": region.as_str(),
        "sessions": sessions,
    });
    let text = serde_json::to_string_pretty(&bundle)
        .map_err(|error| format!("序列化导出包失败：{error}"))?;
    if let Some(parent) = out_path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| format!("创建导出目录失败：{error}"))?;
    }
    // 原子写：中途失败不留半份包（用户拿到的要么完整、要么不存在）。
    crate::modules::config::atomic_write(out_path, &text)
        .map_err(|error| format!("写导出包失败：{error}"))?;

    Ok(json!({
        "path": out_path.to_string_lossy(),
        "exported": sessions_len(&bundle),
        "skipped": skipped,
        "bytes": text.len(),
    }))
}

/// 导出**当前账号的全部会话**（v1 的默认口径：备份 / 换机不需要先学会多选）。
///
/// 会话清单走 [`session::list_sessions_for_user_for`] —— 与「会话页看到的那一份」
/// 同源，**不另写一套筛选**（两套筛选迟早对不上：页面看得到 12 个、导出只出 10 个）。
pub fn export_all_for(region: Region, uid: &str, out_path: &Path) -> Result<Value, String> {
    let listed = session::list_sessions_for_user_for(region, uid);
    let cids: Vec<String> = listed
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.get("id").and_then(Value::as_str))
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    if cids.is_empty() {
        return Err("当前账号没有可导出的会话".to_string());
    }
    export_sessions_for(region, uid, &cids, out_path)
}

/// 预览：只读包并回报**摘要**（UI 在真正导入前给用户看一眼）。
pub fn preview_bundle(bundle_path: &Path) -> Result<Value, String> {
    let bundle = read_bundle(bundle_path)?;
    let sessions = bundle
        .get("sessions")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let titles: Vec<Value> = sessions
        .iter()
        .take(50)
        .map(|item| {
            json!({
                "title": item.get("title").cloned().unwrap_or(Value::Null),
                "sourceCid": item.get("sourceCid").cloned().unwrap_or(Value::Null),
                "workspace": item.get("workspace").cloned().unwrap_or(Value::Null),
                "hasRow": item.get("row").map(|row| !row.is_null()).unwrap_or(false),
            })
        })
        .collect();
    Ok(json!({
        "kind": BUNDLE_KIND,
        "schema": bundle.get("schema").cloned().unwrap_or(Value::Null),
        "exportedAt": bundle.get("exportedAt").cloned().unwrap_or(Value::Null),
        "region": bundle.get("region").cloned().unwrap_or(Value::Null),
        "count": sessions.len(),
        "sessions": titles,
    }))
}

/// 导入：把包里的会话**作为新会话**写进 `uid`（region 决定写哪棵树）。
///
/// 返回逐条结果：`imported` / `failed`。**不静默跳过失败** —— 每条失败都带原因。
pub fn import_sessions_for(region: Region, uid: &str, bundle_path: &Path) -> Result<Value, String> {
    let bundle = read_bundle(bundle_path)?;
    let entries = bundle
        .get("sessions")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if entries.is_empty() {
        return Err("包里没有会话".to_string());
    }

    let projects = session::session_data_dir(region).join("projects");
    let db_path = session::workbuddy_db_path_for(region);
    let conn = session::open_db(&db_path, false).ok_or_else(|| {
        format!(
            "打不开会话数据库 {}（客户端是否已启动过？）",
            db_path.display()
        )
    })?;
    let dst_cols = session::table_columns(&conn, "sessions");

    let mut imported: Vec<Value> = Vec::new();
    let mut failed: Vec<Value> = Vec::new();
    for entry in &entries {
        let source_cid = entry
            .get("sourceCid")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        match import_one(region, uid, &projects, &conn, &dst_cols, entry) {
            Ok((new_cid, row_written)) => imported.push(json!({
                "sourceCid": source_cid,
                "newId": new_cid,
                "rowWritten": row_written,
            })),
            Err(reason) => failed.push(json!({"sourceCid": source_cid, "error": reason})),
        }
    }

    Ok(json!({
        "imported": imported,
        "failed": failed,
        "importedCount": imported.len(),
        "failedCount": failed.len(),
    }))
}

/// 导入单条。成功返回 `(新 id, 是否写了索引行)`。
fn import_one(
    region: Region,
    uid: &str,
    projects: &Path,
    conn: &rusqlite::Connection,
    dst_cols: &[String],
    entry: &Value,
) -> Result<(String, bool), String> {
    let source_cid = entry.get("sourceCid").and_then(Value::as_str).unwrap_or("");
    if source_cid.is_empty() {
        return Err("缺少 sourceCid".to_string());
    }
    let workspace = entry.get("workspace").and_then(Value::as_str).unwrap_or("");
    if !is_safe_workspace(workspace) {
        return Err(format!(
            "workspace 非法（{workspace}）—— 拒绝写到数据目录之外"
        ));
    }
    let body = entry.get("jsonl").and_then(Value::as_str).unwrap_or("");
    if body.is_empty() {
        return Err("正文为空".to_string());
    }

    // ★ 新 id 必须带连字符（`Uuid::new_v4().to_string()`）：32 位无连字符形式
    //   客户端认不出来（与 `copy_session_to_user` 同一条规矩）。
    let new_cid = uuid::Uuid::new_v4().to_string();
    let dir = projects.join(workspace);
    std::fs::create_dir_all(&dir).map_err(|error| format!("创建目录失败：{error}"))?;
    let target = dir.join(format!("{new_cid}.jsonl"));
    // 正文里的会话 id 要一起换掉，否则客户端按 id 找不到这条会话。
    let text = body.replace(source_cid, &new_cid);
    std::fs::write(&target, text).map_err(|error| format!("写正文失败：{error}"))?;

    // 索引行：包里有就写，没有就只留正文（并如实回报 `rowWritten: false`）。
    let Some(row) = entry.get("row").filter(|row| !row.is_null()) else {
        return Ok((new_cid, false));
    };
    let columns = json_strings(row.get("columns"));
    let values: Vec<rusqlite::types::Value> = row
        .get("values")
        .and_then(Value::as_array)
        .map(|items| items.iter().map(json_to_value).collect())
        .unwrap_or_default();
    if columns.is_empty() || columns.len() != values.len() {
        // 行数据自相矛盾 ⇒ 不留孤儿正文。
        let _ = std::fs::remove_file(&target);
        return Err("索引行的列与值数量不一致".to_string());
    }
    if let Err(reason) =
        session::insert_session_row(conn, dst_cols, &columns, &values, &new_cid, uid)
    {
        // ★ 行写失败 ⇒ 删掉刚写的正文：留着它对客户端不可见，只是白占盘。
        let _ = std::fs::remove_file(&target);
        return Err(reason);
    }
    // 归属校验已在 `insert_session_row` 内部回读完成（issue #146 的验收口径）。
    let _ = region;
    Ok((new_cid, true))
}

/// 读包 + 校验类型与结构版本。
fn read_bundle(path: &Path) -> Result<Value, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|error| format!("读导出包失败（{}）：{error}", path.display()))?;
    let bundle: Value =
        serde_json::from_str(&text).map_err(|error| format!("导出包不是合法 JSON：{error}"))?;
    match bundle.get("kind").and_then(Value::as_str) {
        Some(BUNDLE_KIND) => {}
        other => {
            return Err(format!(
                "这不是会话导出包（kind={}）。请选 BuddySwitch 导出的 .json 文件。",
                other.unwrap_or("缺失")
            ))
        }
    }
    let schema = bundle.get("schema").and_then(Value::as_u64).unwrap_or(0);
    if schema != BUNDLE_SCHEMA {
        return Err(format!(
            "导出包结构版本 {schema} 与当前版本 {BUNDLE_SCHEMA} 不兼容 —— 请用同一个（或更新的）版本导出后再导入。"
        ));
    }
    Ok(bundle)
}

/// `workspace` 只允许**单个目录名**：不带分隔符、不是 `.` / `..`、不是绝对路径。
///
/// 这是唯一一处「包里的字符串会变成路径」的地方 —— 不加这一层，一份构造过的包就能
/// 把会话正文写到数据目录之外（导入是**别人给的文件**，必须当不可信输入处理）。
fn is_safe_workspace(workspace: &str) -> bool {
    if workspace.is_empty() || workspace == "." || workspace == ".." {
        return false;
    }
    if workspace.contains('/') || workspace.contains('\\') || workspace.contains(':') {
        return false;
    }
    // 控制字符 / 通配符一律拒绝：目录名是 md5 派生的不透明串，不需要这些字符。
    !workspace
        .chars()
        .any(|ch| ch.is_control() || matches!(ch, '*' | '?' | '"' | '<' | '>' | '|'))
}

fn json_strings(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn sessions_len(bundle: &Value) -> usize {
    bundle
        .get("sessions")
        .and_then(Value::as_array)
        .map(Vec::len)
        .unwrap_or(0)
}

/// 行值 → JSON。`Blob` 用 `{"$blob": "<base64>"}` 标记 —— 直接转字符串会**悄悄改坏**
/// 二进制列（sessions 表目前没有，但换个客户端版本就可能出现）。
fn value_to_json(value: &rusqlite::types::Value) -> Value {
    use rusqlite::types::Value as V;
    match value {
        V::Null => Value::Null,
        V::Integer(number) => json!(number),
        V::Real(number) => json!(number),
        V::Text(text) => Value::String(text.clone()),
        V::Blob(bytes) => {
            let mut object = Map::new();
            object.insert(
                "$blob".to_string(),
                Value::String(base64::engine::general_purpose::STANDARD.encode(bytes)),
            );
            Value::Object(object)
        }
    }
}

/// [`value_to_json`] 的逆运算。
fn json_to_value(value: &Value) -> rusqlite::types::Value {
    use rusqlite::types::Value as V;
    match value {
        Value::Null => V::Null,
        Value::Bool(flag) => V::Integer(i64::from(*flag)),
        Value::Number(number) => match number.as_i64() {
            Some(int) => V::Integer(int),
            None => V::Real(number.as_f64().unwrap_or(0.0)),
        },
        Value::String(text) => V::Text(text.clone()),
        Value::Array(_) => V::Text(value.to_string()),
        Value::Object(object) => match object.get("$blob").and_then(Value::as_str) {
            Some(encoded) => base64::engine::general_purpose::STANDARD
                .decode(encoded)
                .map(V::Blob)
                .unwrap_or_else(|_| V::Text(value.to_string())),
            None => V::Text(value.to_string()),
        },
    }
}

/// 导出包默认文件名（`BuddySwitch-sessions-<时间>.json`）。
pub fn default_file_name() -> String {
    format!(
        "BuddySwitch-sessions-{}.json",
        crate::modules::config::utc_iso()
            .replace([':', '-'], "")
            .replace('T', "-")
            .replace('Z', "")
    )
}

/// 导出包的落点候选目录（供命令层拼默认路径）。
pub fn default_dir() -> PathBuf {
    crate::modules::config::home_dir().join("Downloads")
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    /// 搭一个隔离的 home：`<home>/.workbuddy/{projects/ws-a, workbuddy.db}`。
    ///
    /// 必须隔离 —— `session_data_dir` 是**无参全局路径函数**（读 home），
    /// 不隔离就会读写用户真实的 `~/.workbuddy`。
    fn fixture(tag: &str) -> (crate::modules::config::HomeOverrideGuard, PathBuf) {
        let home = std::env::temp_dir().join(format!(
            "buddy-switch-bundle-{tag}-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let root = home.join(".workbuddy");
        std::fs::create_dir_all(root.join("projects").join("ws-a")).unwrap();
        let guard = crate::modules::config::HomeOverrideGuard::set(&home);
        (guard, root)
    }

    fn seed_session(root: &Path, cid: &str, uid: &str) {
        std::fs::write(
            root.join("projects")
                .join("ws-a")
                .join(format!("{cid}.jsonl")),
            format!("{{\"type\":\"message\",\"sessionId\":\"{cid}\",\"n\":0}}\n"),
        )
        .unwrap();
        let conn = Connection::open(root.join("workbuddy.db")).unwrap();
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS sessions (
                 id TEXT PRIMARY KEY, user_id TEXT, title TEXT, cwd TEXT,
                 created_at INTEGER, updated_at INTEGER, deleted_at INTEGER
             );",
        )
        .unwrap();
        conn.execute(
            "INSERT INTO sessions (id, user_id, title, cwd, created_at, updated_at, deleted_at)
             VALUES (?1, ?2, '导出的会话', '/tmp/ws-a', 1, 2, NULL)",
            rusqlite::params![cid, uid],
        )
        .unwrap();
    }

    /// ★ 导出 → 导入的**完整闭环**：新 id、正文换 id、索引行归属目标账号。
    ///
    /// 可证伪：把 `import_one` 里的 `body.replace(source_cid, &new_cid)` 去掉 ⇒
    /// 正文里仍是旧 id，第三条断言红（客户端按新 id 找不到会话）。
    #[test]
    fn export_then_import_round_trips_with_new_id() {
        let (_guard, root) = fixture("roundtrip");
        seed_session(&root, "cid-old", "uid-a");

        let out = root.join("bundle.json");
        let report = export_sessions_for(Region::Cn, "uid-a", &["cid-old".to_string()], &out)
            .expect("导出应成功");
        assert_eq!(report["exported"], 1, "{report}");
        assert!(out.is_file(), "包必须落盘");

        // 预览：能读出条数与标题（UI 导入前给用户看的那一步）。
        let preview = preview_bundle(&out).expect("预览应成功");
        assert_eq!(preview["count"], 1, "{preview}");
        assert_eq!(preview["sessions"][0]["title"], "导出的会话");

        // 导回**同一个账号**（换 id 的新会话），验证「只做加法」。
        let imported = import_sessions_for(Region::Cn, "uid-a", &out).expect("导入应成功");
        assert_eq!(imported["importedCount"], 1, "{imported}");
        assert_eq!(imported["failedCount"], 0, "{imported}");
        let new_cid = imported["imported"][0]["newId"]
            .as_str()
            .unwrap()
            .to_string();
        assert_ne!(new_cid, "cid-old", "导入必须生成新 id");
        assert!(
            new_cid.contains('-'),
            "必须是带连字符的 UUID（32 位 hex 客户端认不出）"
        );

        // 正文：新文件存在，且内容里的 sessionId 已换成新 id。
        let body = std::fs::read_to_string(
            root.join("projects")
                .join("ws-a")
                .join(format!("{new_cid}.jsonl")),
        )
        .expect("正文应存在");
        assert!(body.contains(&new_cid), "正文里的会话 id 必须换掉: {body}");
        assert!(!body.contains("cid-old"), "不许残留旧 id: {body}");

        // 索引行：归属目标账号、标题保留。
        let conn = Connection::open(root.join("workbuddy.db")).unwrap();
        let (uid, title): (String, String) = conn
            .query_row(
                "SELECT user_id, title FROM sessions WHERE id = ?1",
                rusqlite::params![new_cid],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("新会话必须有索引行");
        assert_eq!(uid, "uid-a");
        assert_eq!(title, "导出的会话");
        // 原会话原封不动。
        assert!(root.join("projects/ws-a/cid-old.jsonl").is_file());
    }

    /// 导入**不覆盖**：同一份包导两次 ⇒ 两个新 id（「只做加法」）。
    #[test]
    fn importing_twice_creates_two_sessions_instead_of_overwriting() {
        let (_guard, root) = fixture("twice");
        seed_session(&root, "cid-old", "uid-a");
        let out = root.join("bundle.json");
        export_sessions_for(Region::Cn, "uid-a", &["cid-old".to_string()], &out).unwrap();

        let first = import_sessions_for(Region::Cn, "uid-a", &out).unwrap();
        let second = import_sessions_for(Region::Cn, "uid-a", &out).unwrap();
        let a = first["imported"][0]["newId"].as_str().unwrap().to_string();
        let b = second["imported"][0]["newId"].as_str().unwrap().to_string();
        assert_ne!(a, b, "两次导入必须是两条独立会话（不静默去重、也不覆盖）");

        let conn = Connection::open(root.join("workbuddy.db")).unwrap();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM sessions", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 3, "原会话 + 两次导入 = 3 行");
    }

    /// 别人给的包必须当**不可信输入**：kind / schema / workspace 三道闸。
    #[test]
    fn rejects_foreign_or_malformed_bundles() {
        let (_guard, root) = fixture("reject");

        let wrong_kind = root.join("wrong.json");
        std::fs::write(
            &wrong_kind,
            r#"{"kind":"something.else","schema":1,"sessions":[]}"#,
        )
        .unwrap();
        let error = import_sessions_for(Region::Cn, "uid-a", &wrong_kind).unwrap_err();
        assert!(error.contains("不是会话导出包"), "{error}");

        let wrong_schema = root.join("schema.json");
        std::fs::write(
            &wrong_schema,
            format!(r#"{{"kind":"{BUNDLE_KIND}","schema":99,"sessions":[]}}"#),
        )
        .unwrap();
        let error = import_sessions_for(Region::Cn, "uid-a", &wrong_schema).unwrap_err();
        assert!(error.contains("不兼容"), "{error}");

        // 路径穿越：workspace 里带 `..` 或分隔符 ⇒ 整条拒绝，且**不留下任何文件**。
        let traversal = root.join("traversal.json");
        std::fs::write(
            &traversal,
            format!(
                r#"{{"kind":"{BUNDLE_KIND}","schema":1,"sessions":[
                     {{"sourceCid":"c1","workspace":"../escape","title":"t","row":null,"jsonl":"x"}}
                   ]}}"#
            ),
        )
        .unwrap();
        let report = import_sessions_for(Region::Cn, "uid-a", &traversal).unwrap();
        assert_eq!(report["importedCount"], 0, "{report}");
        assert!(
            report["failed"][0]["error"]
                .as_str()
                .unwrap()
                .contains("workspace 非法"),
            "{report}"
        );
    }

    /// 包里的会话**没有索引行**时：只写正文，并如实回报 `rowWritten: false`。
    ///
    /// 反例：把它当失败 ⇒ 用户「正文搬过来了却报错」；把它当成功且不标记 ⇒
    /// 客户端列表里看不到，用户以为导入坏了。
    #[test]
    fn import_without_row_writes_body_only_and_says_so() {
        let (_guard, root) = fixture("norow");
        let bundle = root.join("norow.json");
        std::fs::write(
            &bundle,
            format!(
                r#"{{"kind":"{BUNDLE_KIND}","schema":1,"sessions":[
                     {{"sourceCid":"c9","workspace":"ws-a","title":"t","row":null,
                       "jsonl":"{{\"sessionId\":\"c9\"}}\n"}}
                   ]}}"#
            ),
        )
        .unwrap();
        let report = import_sessions_for(Region::Cn, "uid-a", &bundle).unwrap();
        assert_eq!(report["importedCount"], 1, "{report}");
        assert_eq!(report["imported"][0]["rowWritten"], false, "{report}");
    }
}
