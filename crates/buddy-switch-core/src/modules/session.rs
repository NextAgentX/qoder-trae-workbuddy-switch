//! 会话列表与按需复制（路径 B：生成新 id，云端可正常同步）。
//!
//! 对照 server.py `current_user_uid` / `list_sessions_for_user` /
//! `_find_project_jsonl` / `copy_session_to_user` / `_register_edge_sync_mapping` /
//! `copy_sessions_for_switch` / `backup_workbuddy_db` / `workbuddy_db_path`。
//!
//! ## 批量复制（切换流程）的三段管线
//!
//! [`copy_sessions_for_switch_cross`] 不走「一个会话一路做到底」的旧循环，而是分三段：
//! ① **phase 0（顺序、只读）**：账本去重 + Claw 判定 + 读源行 + 解析 jsonl 路径
//!    （源 / 目标 db、边车库、projects 索引、账本都**只开一次**，全程复用）；
//! ② **phase A（并行、纯文件）**：多线程复制 jsonl 正文（`read → replace(cid) → write`）——
//!    正文是 GB 级 I/O（真机 524 个 jsonl ≈ 1GB），这是切换等待的大头；
//! ③ **phase B（顺序、写库）**：**整批只备份一次** workbuddy.db，再逐会话插索引行 /
//!    注册云端映射；账本内存累积、末尾一次落盘。
//!
//! 三段的分界即安全边界：db 写全部集中在 phase B 的顺序段（客户端此刻已关闭），
//! 并行段只碰**互不相干的新文件**，不共享任何可变状态。
//!
//! WorkBuddy 5.x 数据三件套（缺一不可）：
//!   1) 正文：`~/.workbuddy/projects/{workspace}/{cid}.jsonl`（JSONL 含 sessionId 字段）
//!   2) 元数据：`~/.workbuddy/workbuddy.db` sessions 表（id = conversation id = UUID）
//!   3) 云端映射：`~/.workbuddy/edge-sync-mapping-v{N}.db` edge_sync_mapping
//!      （session_id=conversation_id，msg_channel=convmsg:{uid} 决定云端归属）
//!      ⚠️ 档位号 **N 会随客户端升级往上搬**（实测 v2 → v3 → v4），由
//!      [`edge_sync_db_path_for`] 动态发现，**不要写死**。

use rusqlite::{Connection, OptionalExtension};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};

/// 源会话行：(列名, 列值)。两处「读源会话行」共用同一份返回形状，
/// 抽成别名避免同一条深嵌套签名在 impl 里写两遍（clippy::type_complexity）。
type SessionRow = (Vec<String>, Vec<rusqlite::types::Value>);
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use crate::modules::auth_file;
use crate::modules::config::{backup_dir, home_dir, now_ms, now_secs, utc_iso};
use crate::modules::region::Region;

/// 打开数据库并设置 busy_timeout（对照 Python `sqlite3.connect(timeout=5)`）。
pub(crate) fn open_db(path: &Path, read_only: bool) -> Option<Connection> {
    let conn = if read_only {
        Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).ok()?
    } else {
        Connection::open(path).ok()?
    };
    let _ = conn.busy_timeout(Duration::from_secs(5));
    Some(conn)
}

/// 客户端会话数据目录（CN `.workbuddy` / Global `.workbuddy-ai`）。
pub fn session_data_dir(region: Region) -> PathBuf {
    let name = match region {
        Region::Cn => ".workbuddy",
        Region::Global => ".workbuddy-ai",
    };
    home_dir().join(name)
}

/// CN 会话数据库路径。
pub fn workbuddy_db_path() -> PathBuf {
    workbuddy_db_path_for(Region::Cn)
}

/// 按 region 会话数据库路径。
pub fn workbuddy_db_path_for(region: Region) -> PathBuf {
    session_data_dir(region).join("workbuddy.db")
}

/// CN 会话边车映射数据库路径（旧签名，仅供单测使用）。
#[cfg(test)]
fn edge_sync_db_path() -> PathBuf {
    edge_sync_db_path_for(Region::Cn)
}

/// 边车映射库的文件名前缀 / 后缀（`edge-sync-mapping-v{N}.db`）。
const EDGE_SYNC_DB_PREFIX: &str = "edge-sync-mapping-v";
const EDGE_SYNC_DB_SUFFIX: &str = ".db";

/// 老客户端用的档位名：一个都不存在时回落到它（保持历史行为）。
const EDGE_SYNC_DB_LEGACY: &str = "edge-sync-mapping-v2.db";

/// region 会话边车映射数据库路径。
///
/// # 为什么要**动态发现档位号**（2026-09-24 真机实测）
///
/// 客户端把该库的档位号一路往上搬：真机上 `~/.workbuddy/` 只剩 **v4**（v2 已迁走），
/// `~/.workbuddy-ai/` 同时留着 **v3 与 v4**（v3 的 WAL 停在 09-23 19:24，v4 的一直在写）。
/// 写死 v2 的后果**不是报错**而是**静默**：`insert_edge_sync_mapping` 会新建一个空的
/// v2 库、找不到 `edge_sync_mapping` 表、返回 `false` ⇒ 每次会话复制都提示
/// 「云端映射注册失败」。
///
/// # 取法：**档位号最大**的那个已存在的库
///
/// - 只有 v4 ⇒ v4（真机国内版的形态）；
/// - v3 与 v4 并存 ⇒ **v4**（v3 是上一代、客户端已不再读它；写进去等于没写）；
/// - 只有 v2 ⇒ v2（老客户端）；
/// - 一个都没有 ⇒ 回落 v2（历史默认；客户端不存在时建哪个都是空的）。
///
/// ⚠️ 刻意**不**用「最低档位优先」：那会在 v3/v4 并存的机器上选中已经没人读的 v3，
/// 把「静默失效」换个地方重演。也不写死 v4：档位号还会继续涨。
fn edge_sync_db_path_for(region: Region) -> PathBuf {
    let dir = session_data_dir(region);
    match latest_edge_sync_db_version(&dir) {
        Some(version) => dir.join(format!(
            "{EDGE_SYNC_DB_PREFIX}{version}{EDGE_SYNC_DB_SUFFIX}"
        )),
        None => dir.join(EDGE_SYNC_DB_LEGACY),
    }
}

/// 扫目录取 `edge-sync-mapping-v{N}.db` 里**最大**的 N；一个都没有返回 `None`。
///
/// 只认 `.db` 本体：`-shm` / `-wal` 是 SQLite 的边车文件，不参与选档
/// （`edge-sync-mapping-v3.db-wal` 的 `strip_suffix(".db")` 天然失败，自动排除）。
fn latest_edge_sync_db_version(dir: &Path) -> Option<u32> {
    std::fs::read_dir(dir)
        .ok()?
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| {
            let name = entry.file_name();
            let version = name
                .to_str()?
                .strip_prefix(EDGE_SYNC_DB_PREFIX)?
                .strip_suffix(EDGE_SYNC_DB_SUFFIX)?;
            version.parse::<u32>().ok()
        })
        .max()
}

/// 当前认证账号的 uid（CN 认证文件 account.uid）。
pub fn current_user_uid() -> Option<String> {
    current_user_uid_for(Region::Cn)
}

/// 按 region 当前认证账号的 uid（认证文件 account.uid）。
pub fn current_user_uid_for(region: Region) -> Option<String> {
    let auth = auth_file::read_auth_file_for(region)?;
    auth.get("account")
        .and_then(|a| a.get("uid"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

fn table_exists(conn: &Connection, name: &str) -> bool {
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
        [name],
        |r| r.get::<_, i64>(0),
    )
    .unwrap_or(0)
        == 1
}

fn column_exists(conn: &Connection, table: &str, column: &str) -> bool {
    let Ok(mut stmt) = conn.prepare(&format!("PRAGMA table_info({table})")) else {
        return false;
    };
    let Ok(iter) = stmt.query_map([], |row| row.get::<_, String>(1)) else {
        return false;
    };
    let names: Vec<String> = iter.flatten().collect();
    names.iter().any(|name| name == column)
}

fn nonempty_text(value: Option<String>) -> Option<String> {
    value
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// WorkBuddy 侧栏展示名：优先 custom_title（用户改名 / 定时任务名），否则 title。
fn session_display_title(title: Option<String>, custom_title: Option<String>) -> String {
    nonempty_text(custom_title)
        .or_else(|| nonempty_text(title))
        .unwrap_or_else(|| "(无标题)".to_string())
}

/// Claw 是账号绑定的 IM 渠道工作区，复制会话行不够，目标账号也用不了。
fn is_claw_workspace(cwd: &str) -> bool {
    cwd.trim()
        .trim_end_matches(['/', '\\'])
        .rsplit(['/', '\\'])
        .next()
        .is_some_and(|name| name.eq_ignore_ascii_case("claw"))
}

/// 列出某账号未删除的会话（CN；workbuddy.db sessions 表，db 为准）。
///
/// `title` 为 WorkBuddy 侧栏同款展示名；`isPlayground` 对应侧栏「任务」，
/// 其余按 `cwd` 最后一段归入「空间」。
pub fn list_sessions_for_user(uid: &str) -> Value {
    list_sessions_for_user_for(Region::Cn, uid)
}

/// 按 region 列出某账号未删除的会话。
///
/// 归属匹配见 `USER_SCOPE_STRICT` / `USER_SCOPE_WIDENED`：当前账号的行**并上**「空归属」行，
/// 避免客户端把 `user_id` 落成空串时被误判成「账号无会话」。
pub fn list_sessions_for_user_for(region: Region, uid: &str) -> Value {
    let db = workbuddy_db_path_for(region);
    if !db.is_file() {
        return json!([]);
    }
    let Some(conn) = open_db(&db, true) else {
        return json!([]);
    };
    if !table_exists(&conn, "sessions") {
        return json!([]);
    }
    let has_custom = column_exists(&conn, "sessions", "custom_title");
    let has_playground = column_exists(&conn, "sessions", "is_playground");
    let columns = match (has_custom, has_playground) {
        (true, true) => "id, cwd, title, custom_title, updated_at, is_playground",
        (true, false) => "id, cwd, title, custom_title, updated_at, 0",
        (false, true) => "id, cwd, title, NULL, updated_at, is_playground",
        (false, false) => "id, cwd, title, NULL, updated_at, 0",
    };
    // ★ `hasHistory` 需要「这份会话有没有 jsonl 正文」。`projects/` 的两层目录**只扫一次**
    //   （[`build_projects_index_for`]），之后每行只是一次 HashMap 查询。
    //   ⚠️ 旧实现对**每一行**调 [`find_project_jsonl_for`]（每次重扫两层目录）—— 真机
    //   Global 分区 12k 行 × 22 个工作区目录 ≈ 27 万次 syscall，是「会话页加载很久」的
    //   第二大来源。索引只建一次成本极低（22 次 `read_dir`）。
    let projects = build_projects_index_for(region);

    // 归属匹配 = 严格命中当前账号的行 **∪** 「空归属」行（见 `USER_SCOPE_*`）。
    // 取并集而不是「严格为空才放宽」：后者在「既有当前账号的会话、又有旧空归属行」的库里
    // 仍然看不到旧会话（用户现场：索引恢复后切换弹窗里依然列不出来）。
    let mut sessions = query_session_rows(&conn, columns, USER_SCOPE_STRICT, uid, &projects);
    if !uid.is_empty() {
        for s in query_session_rows(&conn, columns, USER_SCOPE_WIDENED, uid, &projects) {
            if !sessions.iter().any(|e| e.get("id") == s.get("id")) {
                sessions.push(s);
            }
        }
        // 两段各自按 updatedAt 倒序，合并后需重排，保持「最近活动在前」。
        sessions.sort_by(|a, b| {
            let ka = a.get("updatedAt").and_then(Value::as_i64).unwrap_or(0);
            let kb = b.get("updatedAt").and_then(Value::as_i64).unwrap_or(0);
            kb.cmp(&ka)
        });
    }
    json!(sessions)
}

/// `sessions.user_id` 的归属过滤：`STRICT` 命中当前账号，`WIDENED` 额外纳入「空归属」行。
///
/// **为什么要纳入空归属行**：实测（2026-09-23，本机 18:47 的 Global 库备份）**116 条会话的
/// `user_id` 全是空字符串** —— 这是客户端某些版本 / 迁移后的落库形态，不是损坏，也**不该**
/// 被当成「账号没有会话」。严格过滤会让 UI 静默退化成「账号暂无会话」并**禁用复制**
/// （用户现场报障：切换账号时无法复制会话）。
///
/// 取并集（而非「严格命中 0 行才放宽」）：库里同时存在当前账号的行与旧空归属行时，
/// 旧会话同样必须列得出来。
///
/// 边界：**只纳入 `NULL` / 空串**（本机这份库里「没人认领」的行），**不会**把别的非空 uid
/// 的会话列出来（负向对照见 `list_sessions_does_not_widen_to_other_accounts`）；
/// 当前 uid 为空串时不做合并（两段会完全重叠）。
const USER_SCOPE_STRICT: &str = "user_id = ?1";
const USER_SCOPE_WIDENED: &str = "(user_id = ?1 OR user_id IS NULL OR user_id = '')";

/// 按给定归属范围查会话行并映射成前端结构；任何一步失败都返回空表（**绝不报错**，
/// 让上层能继续走降级扫描）。
///
/// `projects` = `cid → jsonl 路径` 索引（[`build_projects_index_for`]，调用方**建一次**）。
/// ⚠️ 不在这里自己扫目录：这个函数按行调用，内部扫目录就是 O(行数 × 工作区数)。
fn query_session_rows(
    conn: &Connection,
    columns: &str,
    scope: &str,
    uid: &str,
    projects: &HashMap<String, PathBuf>,
) -> Vec<Value> {
    let sql = format!(
        "SELECT {columns} FROM sessions WHERE {scope} AND deleted_at IS NULL \
         ORDER BY updated_at DESC"
    );
    let Ok(mut stmt) = conn.prepare(&sql) else {
        return Vec::new();
    };
    let Ok(rows) = stmt.query_map([uid], |row| {
        Ok((
            row.get::<_, Option<String>>(0)?,
            row.get::<_, Option<String>>(1)?,
            row.get::<_, Option<String>>(2)?,
            row.get::<_, Option<String>>(3)?,
            row.get::<_, Option<i64>>(4)?,
            row.get::<_, Option<i64>>(5)?,
        ))
    }) else {
        return Vec::new();
    };

    let mut sessions: Vec<Value> = Vec::new();
    for r in rows.flatten() {
        let (cid, cwd, title, custom_title, updated_at, is_playground) = r;
        let cid = cid.unwrap_or_default();
        let cwd = cwd.unwrap_or_default();
        if is_claw_workspace(&cwd) {
            continue;
        }
        sessions.push(json!({
            "id": cid,
            "title": session_display_title(title, custom_title),
            "cwd": cwd,
            "updatedAt": updated_at.unwrap_or(0),
            "hasHistory": projects.contains_key(&cid),
            "isPlayground": is_playground.unwrap_or(0) != 0,
        }));
    }
    sessions
}

/// 按 region 列出某账号未删除的会话，db 不可读 / 为空时降级扫描 projects 目录。
///
/// 返回结构（与 `list_sessions_for_user_for` 不同的、更结构化的形态）：
///   {
///     "sessions": [...],
///     "source": "db" | "scan" | "empty" | "no-dir",
///     "warning"?: "..." // source != "db" 时给出
///   }
///
/// `source == "no-dir"` 表示**数据目录里根本没有会话数据**（`workbuddy.db` 与 `projects/` 都不存在）
/// —— 典型成因是客户端刚被重装 / 重置（2026-09-23 现场：整个 `~/.workbuddy` 被重建为空）。
/// 它必须与「账号确实没有会话」区分开：后者报成前者会**误导用户**（UI 会显示「当前账号暂无会话」，
/// 而真实原因是数据目录空了）。
///
/// 命令层（`commands.rs::list_sessions`、`api.rs::api_sessions`）直接透传给前端；
/// 旧 `list_sessions_for_user_for` 保持纯数组形态、继续给单测和只关心 session 数组
/// 的调用方使用，避免一处改动牵动全栈。
///
/// 优先级：db 读到非空 → "db"；db 读到空但 projects 有 jsonl → "scan"；
/// 两者都空且数据目录存在 → "empty"；数据目录里连 db / projects 都没有 → "no-dir"。
/// **绝不让 UI 把「db 损坏」误显示成「账号无会话」**。
pub fn list_sessions_with_fallback_for(region: Region, uid: &str) -> Value {
    let from_db = list_sessions_for_user_for(region, uid);
    let db_count = from_db.as_array().map(|a| a.len()).unwrap_or(0);
    if db_count > 0 {
        return json!({
            "sessions": from_db,
            "source": "db",
        });
    }
    let scanned = scan_sessions_from_jsonl_for(region);
    if !scanned.is_empty() {
        return json!({
            "sessions": scanned,
            "source": "scan",
            "warning": "数据库索引不可读，已从会话文件扫描；元数据可能不完整（如 customTitle / 任务/空间归属）",
        });
    }
    if !data_dir_has_session_data(region) {
        return json!({
            "sessions": [],
            "source": "no-dir",
        });
    }
    json!({
        "sessions": [],
        "source": "empty",
    })
}

/// 数据目录里是否存在任何会话数据载体（`workbuddy.db` 或 `projects/`）。
///
/// 两者皆无 ⇒ 客户端的会话数据不在本机（重装 / 重置 / 从未登录过），
/// 与「账号在这份库里没有会话」是两回事，必须让 UI 分辨得出。
fn data_dir_has_session_data(region: Region) -> bool {
    workbuddy_db_path_for(region).is_file() || session_data_dir(region).join("projects").is_dir()
}

/// 按 region 在 `{data_dir}/projects/{workspace}/{cid}.jsonl` 定位会话正文。
fn find_project_jsonl_for(region: Region, cid: &str) -> Option<PathBuf> {
    find_project_jsonl_in(&session_data_dir(region).join("projects"), cid)
}

/// [`find_project_jsonl_for`] 的**可重定向**版本（根目录由调用方给 ⇒ 单测不碰真实 home）。
fn find_project_jsonl_in(projects: &Path, cid: &str) -> Option<PathBuf> {
    if !projects.is_dir() {
        return None;
    }
    let direct = projects.join(format!("{cid}.jsonl"));
    if direct.is_file() {
        return Some(direct);
    }
    for entry in std::fs::read_dir(projects).ok()?.flatten() {
        if !entry.path().is_dir() {
            continue;
        }
        let p = entry.path().join(format!("{cid}.jsonl"));
        if p.is_file() {
            return Some(p);
        }
    }
    None
}

/// 一次 `read_dir` 建出 `cid → jsonl 路径` 索引（**批量复制的 phase A/B 共用**）。
///
/// ## 为什么批量必须建索引
///
/// [`find_project_jsonl_for`] 每次调用都要把 `projects/` 的两层目录**重新扫一遍**
/// （直立在先、逐子目录兜底）。单次成本不大，但批量复制对**每个会话**都要找正文 ——
/// N 个会话就是 N 遍全目录扫描；真机 `projects/` 有几百个工作区目录时，
/// 这部分纯 syscall 开销随 N 线性叠上来。索引只建一次，之后全是 HashMap 查询。
///
/// 布局语义与 [`find_project_jsonl_for`] **逐条对齐**（改一处必须同步另一处）：
/// - 只收 `.jsonl`（`.file-rollback.ndjson` / `.meta.json` / 子代理文件不参与）；
/// - **直立在先**：`projects/{cid}.jsonl` 与子目录里的同名文件并存时，直立赢；
/// - 子目录兜底：`projects/{workspace}/{cid}.jsonl`。
pub(crate) fn build_projects_index_for(region: Region) -> HashMap<String, PathBuf> {
    build_projects_index_in(&session_data_dir(region).join("projects"))
}

/// [`build_projects_index_for`] 的**可重定向**版本（根目录由调用方给 ⇒ 单测不碰真实 home）。
fn build_projects_index_in(projects: &Path) -> HashMap<String, PathBuf> {
    let mut index: HashMap<String, PathBuf> = HashMap::new();
    let Ok(entries) = std::fs::read_dir(projects) else {
        return index;
    };
    let mut subdirs: Vec<PathBuf> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            subdirs.push(path);
            continue;
        }
        if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
            continue;
        }
        if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
            // 直立案直接 insert：万一子目录里也有同名文件，直立在查找顺序里优先。
            index.insert(stem.to_string(), path);
        }
    }
    for dir in subdirs {
        let Ok(children) = std::fs::read_dir(&dir) else {
            continue;
        };
        for child in children.flatten() {
            let path = child.path();
            if !path.is_file() {
                continue;
            }
            if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                continue;
            }
            if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                index.entry(stem.to_string()).or_insert(path);
            }
        }
    }
    index
}

/// 逐行 hash 的**上限** —— 公共前缀不可能超过较短的那个，且分歧多数发生在近期。
/// 只存前 N 行 hash ⇒ 批量调用时**响应体可控**（不随会话长度线性膨胀）。
pub const PREFIX_HASH_LIMIT: usize = 512;

/// 一个会话 jsonl 的**内容指纹**（P8「组状态派生」的数据源）。
///
/// ## 为什么是「指纹」而不是「消息数」
///
/// 原型要的「落后 N 条 / 有分歧」需要**内容级**判据 —— `updatedAt` **不能**表达
/// 「谁落后」（`P8-ARCH.md` §1.2：「**时间新 ≠ 内容新**」；用时间冒充内容是**撒谎**）。
/// 本函数从 jsonl 取两样东西：
///
/// - `lineCount`：**完整**行数（实测每行一条 `type:"message"` ⇒ ≈ 消息数）；
/// - `prefixHashes`：**前 [`PREFIX_HASH_LIMIT`] 行**的逐行 hash（**只存 hash，不落盘正文**）。
///
/// ⇒ 两副本的**公共前缀长度** = 两个 `prefixHashes` 的最长公共前缀 ⇒ 那才是「共同旧版」；
/// 各自的 `lineCount - 公共前缀` = 各自新增的条数（**内容级 delta**）。
///
/// ⚠️ **只读**：不改写任何会话文件。
/// ⚠️ **本函数每次都会真读文件**；要复用请走 [`FingerprintStore`]（按 `size + mtime` 失效）。
#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionFingerprint {
    pub line_count: usize,
    pub prefix_hashes: Vec<u64>,
    /// **整份正文**的累计摘要（逐行 FNV，行间续入 `\n`；与 `prefix_hashes` 同一套归一化）。
    ///
    /// ## 为什么必须有它（issue #141）
    ///
    /// `prefix_hashes` 只覆盖**前 [`PREFIX_HASH_LIMIT`] 行**，而判据曾经写成
    /// 「公共前缀 < 最短副本行数 ⇒ `diverge`」⇒ 任何 **> 512 行**的组，公共前缀的
    /// **上限**就是 512，`common < min_lines` **恒为真** ⇒ 逐字节相同的长会话也被报成
    /// 「有分歧」（且 `behind` 对长会话永不可达，「处理分歧」走全量比对又认为本来就一致
    /// ⇒ 点完什么都不发生，用户看到的是死循环）。
    ///
    /// 整份摘要给出一个**窗口之外**的判据：行数相同 + 摘要相同 ⇒ 内容相同（`latest`），
    /// 不必把窗口放大到全文（那会让响应体与落盘缓存随会话长度线性膨胀）。
    /// 行数不同时仍需验证「短的是不是长的前缀」，见 [`prefix_digest_of_file`]。
    ///
    /// ⚠️ **代价**：算它要把**每一行**都过一遍 FNV（原先只 hash 前 512 行）。冷缓存那一次
    /// 会多出约 66 GB 的纯 CPU 哈希（真机账本口径），相对 7 分钟的 IO 约一成；而结果随
    /// `(size, mtime)` 落盘复用 ⇒ 同一份文件只付一次。**不要**为了省这点 CPU 把它改成
    /// 「只对长会话算」：那会让 `full_hash` 的含义随文件长度变化，判据更难推理。
    #[serde(skip)]
    pub full_hash: u64,
}

/// 读一个会话的**内容指纹**；jsonl 不存在 / 不可读时返回 `None`（**fail-closed**）。
/// 指纹里用来**替换「本会话自己的 id」**的固定占位符。
///
/// ## 为什么必须归一化（**实测发现的假分歧**）
///
/// WorkBuddy 的会话 jsonl **几乎每一行都嵌着 `"sessionId":"<cid>"`** ——
/// 实测一份 15 行的真实会话里 **14 行**含该 cid（唯一不含的是 `file-history-snapshot`）。
/// 而副本是**另一个 cid**（复制时整份 `text.replace(cid, new_cid)`）。
///
/// ⇒ 若直接对**原始行**做 hash，两个副本**永远没有公共前缀** ⇒
/// `derive_content_status` 会把**每一个组都判成 `diverge`**（**假分歧**）——
/// 明明内容一致，却告诉用户「有分歧」，比不显示更坏（§3.10 第 1 条「图不能撒谎」）。
///
/// ⇒ hash 前把本会话的 cid 换成这个占位符，使指纹表达的是**内容**，而不是「谁的副本」。
/// 占位符选得足够独特，避免与真实正文撞车。
pub const SESSION_ID_PLACEHOLDER: &str = "\u{1f}buddy-switch-session-id\u{1f}";

/// 文件级指纹（路径由调用方给出 ⇒ 单测只需临时文件，**不碰真实 home**）。
pub fn fingerprint_of_file(path: &Path, cid: &str) -> Option<SessionFingerprint> {
    let text = std::fs::read_to_string(path).ok()?;
    Some(fingerprint_of_text(&text, cid))
}

// ---------------------------------------------------------------------------
// ★ 会话页性能地基：指纹复用（size + mtime）与有界并行读取
// ---------------------------------------------------------------------------

/// 读指纹时的并行度上限。
///
/// 正文在**磁盘**上（真机 Global 分区 66 GB）⇒ 再多线程也只是互相抢磁道。
/// 4 是「能把 NVMe 队列喂饱、又不至于让机械盘彻底抖动」的稳妥值。
const FINGERPRINT_READ_THREADS: usize = 4;

/// 缓存条目的本体：指纹 + **失效判据**（大小 / mtime）+ 一个**前缀探针**。
#[derive(Clone)]
struct CachedFingerprint {
    size: u64,
    mtime_ms: i64,
    fingerprint: SessionFingerprint,
    /// 探针：`(前 N 行的累计摘要)`，N 由**搭档副本的行数**决定。
    ///
    /// 只留**一个**槽位而不是一张表：同一份文件在一组里通常只被同一个搭档追问，
    /// 而每多留一个槽位就要为 25k 个条目各付 16 字节。换搭档 ⇒ 覆盖重算（读一次）。
    probe: Option<(usize, u64)>,
}

/// ★ 指纹缓存（键 = `region ␟ cid`）—— **进程内 + 落盘**两级。
///
/// ## 为什么必须有它
///
/// 会话页要给账本里**每个副本**算内容状态。真机实测（2026-10-14，Global 分区）：
/// 账本 25,078 条边 → 409 组 / 25,487 个副本 → **66 GB** 正文；原始读吞吐 150 MB/s
/// ⇒ 全量读一遍 7 分钟以上（Python 参照实现带解析只有 92 MB/s）。「每次开页都重算」
/// 因此完全不可接受。
///
/// 指纹只由 **(路径, 大小, mtime)** 决定 ⇒ 三者未变即可复用，**命中时零 IO**。
/// 客户端在会话里继续打字只会让文件变长 ⇒ 只有**活跃的那几份**会被重读。
///
/// ## 落盘（本步新增）
///
/// 进程内缓存只解决「同一个应用会话里重复开页」；**重启后第一次**开页仍要读 66 GB。
/// ⇒ 把条目落到 `store_dir()/session-fingerprints.bin`（**应用自有目录**，不写进
/// 客户端数据目录），下次启动直接复用。
///
/// 格式是**定长二进制**（`magic + 条目数 + 条目×N`）：25,487 个副本 ≈ **106 MB**
/// （每条 = cid + size + mtime + lineCount + ≤512 个 u64）。JSON 存同样的东西要
/// 转义 + 十进制编码（约 300 MB），纯浪费；这也是**刻意不用 JSON** 的唯一理由。
///
/// ⚠️ **不落盘的诱惑要顶住一半**：只在「本次真的读过盘 / 条目集合变了」时才重写
/// （`dirty`），全命中的开页**一个字节都不写**。
///
/// ⚠️ **失效判据仍只有 (size, mtime)**：文件被**同尺寸同 mtime** 改写（同一毫秒内
/// 覆写，且长度恰好相等）会读到旧读数。这是**刻意接受**的已知边界 —— 客户端追加式
/// 写入几乎必然改尺寸或 mtime，而引入内容校验就等于每次开页重读 66 GB（本末倒置）。
#[derive(Default)]
pub struct FingerprintStore {
    entries: HashMap<String, CachedFingerprint>,
    /// 是否有未落盘的改动（`put` / `prune` 置位，成功保存后清位）。
    dirty: bool,
    /// 自上次落盘以来**真的读过盘**的条目数（命中不算）。
    fresh_reads: usize,
    /// 上次落盘时的条目数 —— 用来识别「条目集合变了（新增 / 剪枝）」这类值得写的改动。
    entries_at_load: usize,
}

/// 落盘缓存的文件名（位于 [`crate::modules::config::store_dir`]，即应用自有目录）。
const FINGERPRINT_CACHE_FILE: &str = "session-fingerprints.bin";

/// 落盘格式的魔数 + 版本。**改格式必须换它** —— 否则老文件会被按新布局解读，
/// 读出满屏垃圾读数（比缓存失效坏得多，因为它是**静默的错**）。
///
/// 版本史：`BSFPC001` 初版 → `BSFPC002` 加 `lineCount` → `BSFPC003` 加
/// `full_hash`（整份摘要）与探针位（issue #141 的前缀验证）。
const FINGERPRINT_CACHE_MAGIC: &[u8; 8] = b"BSFPC003";

/// 条目数上限：超过就不落盘（内存缓存照常工作）。
///
/// 每条 ≈ 4.2 KB ⇒ 50,000 条 ≈ 210 MB，已经是「为省一次 66 GB 读而付的合理代价」的
/// 上限。再大宁可退化成进程内缓存，也不在用户磁盘上无声堆一个 GB 级文件。
const FINGERPRINT_CACHE_MAX_ENTRIES: usize = 50_000;

/// **值得重写整份缓存**的最少新增读数。
///
/// 落盘是**整份重写**（106 MB）⇒ 为了「刚才聊天让 3 个会话变长了」就重写一次，
/// 代价与收益完全不成比例。规则：**条目集合变了**（首次构建 / 剪枝）或**攒够**
/// [`FINGERPRINT_CACHE_MIN_PERSIST_READS`] 条新读数才写。
///
/// ⚠️ 攒不够就不写是**安全**的：文件里留着的是**旧 (size, mtime)**，重启后照常被
/// 判为失效、重读那几份 —— 只是多读几十 MB，不会给出错误读数。
const FINGERPRINT_CACHE_MIN_PERSIST_READS: usize = 32;

/// 落盘缓存的路径（应用自有 store 目录；`BUDDY_SWITCH_HOME` 会被一并重定向 ⇒ 单测安全）。
fn fingerprint_cache_path() -> PathBuf {
    crate::modules::config::store_dir().join(FINGERPRINT_CACHE_FILE)
}

/// 进程内共享的指纹缓存（**首次使用时**才从磁盘载入 —— 冷启动不为它读盘）。
static FINGERPRINTS: OnceLock<Mutex<FingerprintStore>> = OnceLock::new();

fn fingerprint_store() -> &'static Mutex<FingerprintStore> {
    FINGERPRINTS.get_or_init(|| Mutex::new(FingerprintStore::load_from(&fingerprint_cache_path())))
}

/// 缓存键：**region 分家**（CN / Global 的 cid 是两套 uuid 空间，不许互相命中）。
fn fingerprint_key(region: Region, cid: &str) -> String {
    format!("{}{LEDGER_SEP}{cid}", region.as_str())
}

/// 文件 mtime（ms）。取不到 ⇒ `-1`：仍是**可用**的判据（同一路径每次都得到 `-1`，
/// 退化成「只要 size 不变就复用」），不是错误。
fn mtime_ms_of(meta: &std::fs::Metadata) -> i64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64)
        .unwrap_or(-1)
}

/// 读一份**内容稳定**的指纹：读前 / 读后各 stat 一次，`size` 与 `mtime` 都没变才采信。
///
/// ## 为什么必须校验稳定性
///
/// 会话文件在**用户正在用**的时候会被客户端追加。若在追加中途读完，拿到的是**撕裂的**
/// 内容 —— 行数偏少、前缀 hash 也可能对不上，据此判出来的「谁落后几条」是错的
/// （比不显示更坏）。判据是**读前后一致**：不一致就重试一次，仍不稳定 ⇒ `None`
/// （本次落 `unknown`，**不臆造**），且**不写缓存**（免得把撕裂的读数钉死）。
fn read_stable_fingerprint(path: &Path, cid: &str) -> Option<(SessionFingerprint, u64, i64)> {
    for _ in 0..2 {
        let before = std::fs::metadata(path).ok()?;
        let fingerprint = fingerprint_of_file(path, cid)?;
        let after = std::fs::metadata(path).ok()?;
        if before.len() == after.len() && mtime_ms_of(&before) == mtime_ms_of(&after) {
            return Some((fingerprint, after.len(), mtime_ms_of(&after)));
        }
    }
    None
}

impl FingerprintStore {
    /// 取一份**仍然有效**的缓存读数（`size` / `mtime` 都未变）；否则 `None`。
    fn valid(&self, key: &str, path: &Path) -> Option<SessionFingerprint> {
        let cached = self.entries.get(key)?;
        let meta = std::fs::metadata(path).ok()?;
        if meta.len() != cached.size || mtime_ms_of(&meta) != cached.mtime_ms {
            return None;
        }
        Some(cached.fingerprint.clone())
    }

    /// 登记一份刚读出来的读数（含它对应的身份）。
    fn put(&mut self, key: String, size: u64, mtime_ms: i64, fingerprint: SessionFingerprint) {
        self.entries.insert(
            key,
            CachedFingerprint {
                size,
                mtime_ms,
                fingerprint,
                probe: None,
            },
        );
        self.dirty = true;
        self.fresh_reads += 1;
    }

    /// 取**已记录的前缀探针**（`offset` = 需要的行数）：命中且行数相同 ⇒ 直接给摘要。
    fn probe(&self, key: &str, offset: usize) -> Option<u64> {
        match self.entries.get(key)?.probe {
            Some((cached_offset, digest)) if cached_offset == offset => Some(digest),
            _ => None,
        }
    }

    /// 记下**刚算出来**的前缀探针（随条目一起落盘 ⇒ 下次开页零 IO）。
    fn record_probe(&mut self, key: &str, offset: usize, digest: u64) {
        let Some(entry) = self.entries.get_mut(key) else {
            return;
        };
        entry.probe = Some((offset, digest));
        self.dirty = true;
    }

    /// 从磁盘载入（**fail-safe**：文件缺失 / 魔数不符 / 中途损坏都退化成空表）。
    ///
    /// ⚠️ 中途损坏时**保留已解析的部分**而不是整体丢弃：缓存只是为了省一次读，
    /// 部分可用也比全废好。但**魔数 / 版本不符 ⇒ 整体丢弃** —— 那是布局变了，
    /// 按新布局读老字节会得到**静默的错误读数**。
    fn load_from(path: &Path) -> FingerprintStore {
        let mut store = FingerprintStore::default();
        let Ok(bytes) = std::fs::read(path) else {
            return store;
        };
        let Some(body) = bytes.strip_prefix(FINGERPRINT_CACHE_MAGIC.as_slice()) else {
            return store;
        };
        let mut cursor = 0usize;
        let Some(count) = read_u64(body, &mut cursor) else {
            return store;
        };
        for _ in 0..count {
            let Some(key_len) = read_u16(body, &mut cursor) else {
                return store;
            };
            let Some(key) = read_bytes(body, &mut cursor, key_len as usize).map(String::from_utf8)
            else {
                return store;
            };
            let Ok(key) = key else {
                return store;
            };
            let (Some(size), Some(mtime_ms), Some(line_count)) = (
                read_u64(body, &mut cursor),
                read_i64(body, &mut cursor),
                read_u64(body, &mut cursor),
            ) else {
                return store;
            };
            let Some(hash_len) = read_u16(body, &mut cursor) else {
                return store;
            };
            let mut prefix_hashes: Vec<u64> = Vec::with_capacity(hash_len as usize);
            for _ in 0..hash_len {
                let Some(hash) = read_u64(body, &mut cursor) else {
                    return store;
                };
                prefix_hashes.push(hash);
            }
            let (Some(full_hash), Some(probe_offset), Some(probe_digest)) = (
                read_u64(body, &mut cursor),
                read_u64(body, &mut cursor),
                read_u64(body, &mut cursor),
            ) else {
                return store;
            };
            store.entries.insert(
                key,
                CachedFingerprint {
                    size,
                    mtime_ms,
                    fingerprint: SessionFingerprint {
                        line_count: line_count as usize,
                        prefix_hashes,
                        full_hash,
                    },
                    // `offset == 0` 是「没有探针」的哨兵（行数 0 的探针没有意义）。
                    probe: (probe_offset > 0).then_some((probe_offset as usize, probe_digest)),
                },
            );
        }
        // 载入不是「改动」：紧接着的保存只应发生在真有新读数时。
        store.dirty = false;
        store
    }

    /// 按 `region` 剪掉**本次没被请求**的条目（一个会话被删掉后，它的条目就永远取不到了）。
    ///
    /// ⚠️ 只在**本次账本读成功**（有组）时调用 —— 账本损坏读到空时剪枝会把整份缓存清空，
    /// 那正好是最需要它的时候。
    fn prune_region(&mut self, region: Region, keep: &std::collections::HashSet<String>) -> usize {
        let prefix = format!("{}{LEDGER_SEP}", region.as_str());
        let before = self.entries.len();
        self.entries
            .retain(|key, _| !key.starts_with(&prefix) || keep.contains(key));
        let removed = before - self.entries.len();
        if removed > 0 {
            self.dirty = true;
        }
        removed
    }

    /// 值得写才落盘；返回是否真的写了。
    ///
    /// 两个判据（见 [`FINGERPRINT_CACHE_MIN_PERSIST_READS`]）：
    /// ① **条目集合变了**（首次构建、剪枝）—— 这是结构性变化，必须落盘；
    /// ② **攒够新读数** —— 避免「聊几句天就重写 106 MB」。
    /// 另外条目太多 ⇒ 不写（见 [`FINGERPRINT_CACHE_MAX_ENTRIES`]）。
    ///
    /// ⚠️ 不满足条件时**不清 `dirty` / 计数器** —— 改动攒着，下次接着算。
    fn save_if_worthwhile(&mut self, path: &Path) -> bool {
        if !self.dirty || self.entries.len() > FINGERPRINT_CACHE_MAX_ENTRIES {
            return false;
        }
        let structural = self.entries.len() != self.entries_at_load;
        if !structural && self.fresh_reads < FINGERPRINT_CACHE_MIN_PERSIST_READS {
            return false;
        }
        let mut bytes: Vec<u8> = Vec::with_capacity(16 + self.entries.len() * 4200);
        bytes.extend_from_slice(FINGERPRINT_CACHE_MAGIC);
        bytes.extend_from_slice(&(self.entries.len() as u64).to_le_bytes());
        for (key, entry) in &self.entries {
            bytes.extend_from_slice(&(key.len() as u16).to_le_bytes());
            bytes.extend_from_slice(key.as_bytes());
            bytes.extend_from_slice(&entry.size.to_le_bytes());
            bytes.extend_from_slice(&entry.mtime_ms.to_le_bytes());
            bytes.extend_from_slice(&(entry.fingerprint.line_count as u64).to_le_bytes());
            let hashes = &entry.fingerprint.prefix_hashes;
            bytes.extend_from_slice(&(hashes.len() as u16).to_le_bytes());
            for hash in hashes {
                bytes.extend_from_slice(&hash.to_le_bytes());
            }
            bytes.extend_from_slice(&entry.fingerprint.full_hash.to_le_bytes());
            let (probe_offset, probe_digest) = entry.probe.unwrap_or((0, 0));
            bytes.extend_from_slice(&(probe_offset as u64).to_le_bytes());
            bytes.extend_from_slice(&probe_digest.to_le_bytes());
        }
        if crate::modules::config::atomic_write_bytes(path, &bytes).is_err() {
            // 写失败**不清 dirty**：下次还有机会重写（否则一次瞬时失败就永久停写）。
            return false;
        }
        self.dirty = false;
        self.fresh_reads = 0;
        self.entries_at_load = self.entries.len();
        true
    }
}

/// 从 `cursor` 处读一个 `u16`（越界 ⇒ `None`）。
fn read_u16(buf: &[u8], cursor: &mut usize) -> Option<u16> {
    let end = *cursor + 2;
    let slice = buf.get(*cursor..end)?;
    *cursor = end;
    Some(u16::from_le_bytes([slice[0], slice[1]]))
}

/// 从 `cursor` 处读一个 `u64`（越界 ⇒ `None`）。
fn read_u64(buf: &[u8], cursor: &mut usize) -> Option<u64> {
    let end = *cursor + 8;
    let slice = buf.get(*cursor..end)?;
    *cursor = end;
    Some(u64::from_le_bytes(slice.try_into().ok()?))
}

/// 从 `cursor` 处读一个 `i64`（越界 ⇒ `None`）。
fn read_i64(buf: &[u8], cursor: &mut usize) -> Option<i64> {
    read_u64(buf, cursor).map(|value| value as i64)
}

/// 从 `cursor` 处读 `len` 字节（越界 ⇒ `None`）。
fn read_bytes(buf: &[u8], cursor: &mut usize, len: usize) -> Option<Vec<u8>> {
    let end = *cursor + len;
    let slice = buf.get(*cursor..end)?;
    *cursor = end;
    Some(slice.to_vec())
}

/// 有界并行地把 `cid → 路径` 读成指纹（**顺序与入参一一对应**）。
///
/// ## 为什么是「分块」而不是工作窃取
///
/// 并行段**零共享可变状态**（`session.rs` 模块头那条安全边界）：任务切成
/// [`FINGERPRINT_READ_THREADS`] 份、每份自有所有权、最后按序拼回 ——
/// 不需要原子计数、不需要锁，也就没有「并行段的锁把顺序段拖慢」这一类问题。
fn read_fingerprints_parallel(
    jobs: &[(String, PathBuf)],
) -> Vec<Option<(SessionFingerprint, u64, i64)>> {
    if jobs.is_empty() {
        return Vec::new();
    }
    let workers = jobs.len().min(FINGERPRINT_READ_THREADS);
    let chunk = jobs.len().div_ceil(workers);
    let mut out: Vec<Option<(SessionFingerprint, u64, i64)>> = Vec::with_capacity(jobs.len());
    std::thread::scope(|scope| {
        let handles: Vec<_> = jobs
            .chunks(chunk)
            .map(|part| {
                scope.spawn(move || {
                    part.iter()
                        .map(|(cid, path)| read_stable_fingerprint(path, cid))
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        for handle in handles {
            // 线程 panic 不该让整页加载失败：拿不到的那部分就是 `None`（诚实落 unknown）。
            out.extend(handle.join().unwrap_or_default());
        }
    });
    out
}

/// 把 `members` 的指纹**从缓存复用 / 必要时读盘**（并行）取回，顺序与 `members` 对齐。
///
/// `store` 显式传入（**不用全局**）—— 单测要能拿一个只属于它的临时缓存，
/// 否则会读写真实用户的 `~/.buddy-switch/session-fingerprints.bin`（本仓踩过四次的坑）。
fn fingerprints_for_members(
    store: &mut FingerprintStore,
    region: Region,
    projects: &HashMap<String, PathBuf>,
    members: &[(String, String)],
) -> Vec<Option<SessionFingerprint>> {
    let mut out: Vec<Option<SessionFingerprint>> = vec![None; members.len()];
    // ① 先在缓存里找（命中零 IO）；缺失的登记为待读任务。
    let mut pending: Vec<(usize, String, PathBuf)> = Vec::new();
    for (index, (_uid, cid)) in members.iter().enumerate() {
        match projects.get(cid) {
            // ⚠️ 目录里没有这份 jsonl ⇒ **没有内容可读**（不是错误）⇒ 留 `None`。
            None => continue,
            Some(path) => {
                let key = fingerprint_key(region, cid);
                match store.valid(&key, path) {
                    Some(hit) => out[index] = Some(hit),
                    None => pending.push((index, cid.clone(), path.clone())),
                }
            }
        }
    }
    if pending.is_empty() {
        return out;
    }

    // ② 未命中的并行读（读盘是本函数的全部成本）。
    let jobs: Vec<(String, PathBuf)> = pending
        .iter()
        .map(|(_, cid, path)| (cid.clone(), path.clone()))
        .collect();
    let reads = read_fingerprints_parallel(&jobs);

    // ③ 回填：**只有稳定读数才写**（见 `read_stable_fingerprint`）。
    for ((index, cid, _), read) in pending.into_iter().zip(reads) {
        let Some((fingerprint, size, mtime_ms)) = read else {
            continue;
        };
        store.put(
            fingerprint_key(region, &cid),
            size,
            mtime_ms,
            fingerprint.clone(),
        );
        out[index] = Some(fingerprint);
    }
    out
}

/// 正文级指纹（**纯函数**）—— [`fingerprint_of_file`] 的内核。
///
/// 单独抽出来的理由：调用方**手上已经有正文**时不该再读一遍盘。
/// （P14 S2 的基线若在 phase A 顺手算就能零额外 I/O；本期刻意没走那条路，
/// 见 [`CopyBatch::record_baseline`] 的说明。）
///
/// ★ 性能：归一化**逐行按需**做 —— 不再对整份正文做一次 `text.replace(cid, …)`。
/// 那一版会为每个会话额外分配一份与正文等大的字符串（真机均值 2.85 MB × 25k 个副本
/// ≈ 70 GB 的纯分配/拷贝）。只有**真的含本会话 id** 的那一行才需要替换，其余行直接 hash。
pub fn fingerprint_of_text(text: &str, cid: &str) -> SessionFingerprint {
    let mut line_count = 0usize;
    let mut prefix_hashes: Vec<u64> = Vec::new();
    let mut full_hash = FNV_OFFSET_BASIS;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        line_count += 1;
        // ★ 先 trim 再归一化：占位符不含空白 ⇒ 与「先整串 replace 再 trim」等价。
        let normalized = normalized_line(line, cid);
        if prefix_hashes.len() < PREFIX_HASH_LIMIT {
            prefix_hashes.push(hash_line(&normalized));
        }
        // ★ 整份摘要与逐行 hash **同一套归一化**（否则两个副本的 `full_hash` 永远不等）。
        full_hash = fnv_step(full_hash, &normalized);
    }
    SessionFingerprint {
        line_count,
        prefix_hashes,
        full_hash,
    }
}

/// 一行正文的**归一化**形态（把本会话自己的 cid 换成占位符）。
///
/// 逐行按需替换：整串 `replace` 会为每个会话多分配一份与正文等大的字符串
/// （真机均值 2.85 MB × 25k 副本 ≈ 70 GB 的纯拷贝）。
fn normalized_line<'a>(line: &'a str, cid: &str) -> std::borrow::Cow<'a, str> {
    if cid.is_empty() || !line.contains(cid) {
        std::borrow::Cow::Borrowed(line)
    } else {
        std::borrow::Cow::Owned(line.replace(cid, SESSION_ID_PLACEHOLDER))
    }
}

/// 只读**前 `upto` 行**算累计摘要（与 [`SessionFingerprint::full_hash`] 同口径）。
///
/// ## 用途（issue #141）
///
/// 判「短副本是不是长副本的**前缀**」（即 `behind`）需要长副本**前 N 行**的摘要，
/// 而指纹里只存了整份摘要与前 512 行的逐行 hash。这里**只读到第 N 行就停**
/// （不是读全文），并允许调用方把它记进 [`FingerprintStore`] 的探针位 —— 于是同一对
/// 文件第二次开页**零 IO**。
///
/// ⚠️ 行数不足 `upto` ⇒ `None`：那说明文件比指纹记录的短（正在被改写 / 已被截断），
/// 此时**不给结论**（宁可 `unknown`，也不把「读不到」当成「前缀相同」）。
fn prefix_digest_of_file(path: &Path, cid: &str, upto: usize) -> Option<u64> {
    use std::io::BufRead as _;
    if upto == 0 {
        return Some(FNV_OFFSET_BASIS);
    }
    let file = std::fs::File::open(path).ok()?;
    let mut state = FNV_OFFSET_BASIS;
    let mut seen = 0usize;
    for line in std::io::BufReader::new(file).lines() {
        let line = line.ok()?;
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        seen += 1;
        state = fnv_step(state, &normalized_line(line, cid));
        if seen >= upto {
            return Some(state);
        }
    }
    None
}

/// FNV-1a 64 的初始值与乘数（**不引第三方依赖**：本 crate 的依赖面刻意保持小）。
///
/// ⚠️ 本仓**刻意不用 `sha2`**：本模块的 hash **只在进程内比较、不落盘为「内容身份」**
/// ⇒ 参考实现换 `sha2` 的理由（跨进程 / 跨版本稳定）在本仓**不成立**。
/// 唯一例外是要落盘的那份摘要 —— 那属于 P14 S2，届时单独引入。
const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// 把 `bytes` 续进一个 FNV-1a 64 状态（**可增量** ⇒ 多段能拼成一次 hash）。
fn fnv1a(bytes: &[u8], mut state: u64) -> u64 {
    for b in bytes {
        state ^= *b as u64;
        state = state.wrapping_mul(FNV_PRIME);
    }
    state
}

/// 把**一行**续进累计摘要：先并入行字节，再并入 `\n`（行分隔也参与 hash ⇒
/// 「同样的字节被切成不同行」不会撞成同一个摘要）。
///
/// ⚠️ 必须是**可增量**的：`prefix_digest_of_file` 要能算出「前 N 行」的摘要并与
/// 整份摘要比较，靠的就是同一套逐步累积。
fn fnv_step(state: u64, line: &str) -> u64 {
    fnv1a(b"\n", fnv1a(line.as_bytes(), state))
}

/// 稳定 hash（FNV-1a 64）—— **不引第三方依赖**（本 crate 只有 `serde_json`）。
///
/// 同一行的**任何**字节变化都会改变结果 ⇒ 可用于判「这一行是否与对方相同」。
fn hash_line(line: &str) -> u64 {
    fnv1a(line.as_bytes(), FNV_OFFSET_BASIS)
}

/// 两个指纹的**公共前缀长度**（「共同旧版」的条数）。
///
/// ⚠️ 返回的是**行数**，不是字节 —— 语义即「双方一致到第几条」。
pub fn common_prefix_len(a: &SessionFingerprint, b: &SessionFingerprint) -> usize {
    a.prefix_hashes
        .iter()
        .zip(b.prefix_hashes.iter())
        .take_while(|(x, y)| x == y)
        .count()
}

/// P8 安全同步（增量追加）的执行报告。
///
/// `appendedLines = 0` 表示「本来就一致」，**不是失败** —— 调用方据此区分
/// 「无事可做」与「出错」（后者是 `Err`）。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppendDeltaReport {
    /// 实际追加的行数（0 = 已一致，未写入任何字节）。
    pub appended_lines: usize,
    /// 追加前目标的完整行数（= 共同旧版条数）。
    pub base_lines: usize,
    /// 追加后目标应有的完整行数（= 源的行数）。
    pub total_lines: usize,
}

/// ★ P8「安全同步」：把 `src` 里**目标还没有的尾部行**追加到 `dst`。
///
/// ## 语义（`PLAN.md` §3.15.4）
///
/// - **同步是加法**：只 `append`，**绝不删除 / 改写目标已有内容**。这是用户对
///   「同步」最大的恐惧（被覆盖）⇒ 它是**硬约束**，不是实现细节。
/// - **只处理「落后」**：目标的每一行必须与源**逐字相同**（即目标是源的**严格前缀**）。
/// - **分歧 fail-closed**：公共前缀 < 目标行数 ⇒ 双方在共同旧版之后**各自改过**
///   ⇒ **返回 `Err`，且一个字节都不写**。绝不「猜一个合并策略」。
/// - **判据是内容，不是时间**：`P8-ARCH.md` §3.15.2「时间新 ≠ 内容新」——
///   本函数**不看** `updatedAt` / mtime，只看行内容。
///
/// ## id 重写
///
/// 会话 jsonl 里嵌着对自己 id 的引用，而副本是**另一个 cid** ⇒ 追加的行必须把
/// `src_cid` 换成 `dst_cid`。规则与 [`copy_session_to_user_cross`] 的整份复制**同源**
/// （那边是 `text.replace(cid, &new_cid)`）—— 两处必须一致，否则「复制出的副本」
/// 与「同步过去的增量」会写出两种 id 形态。
///
/// ## 为什么是「文件级」函数
///
/// 路径由调用方给出 ⇒ 单测只需两个临时文件，**不必**动 `BUDDY_SWITCH_HOME`。
/// （无参全局路径函数 + 并行单测 = 假失败，本仓踩过四次。）
pub fn append_delta_between_files(
    src: &Path,
    dst: &Path,
    src_cid: &str,
    dst_cid: &str,
) -> Result<AppendDeltaReport, String> {
    use std::io::Write;

    if src == dst {
        return Err("源与目标是同一个文件，拒绝同步".into());
    }
    let src_text = std::fs::read_to_string(src).map_err(|e| format!("读取源会话失败：{e}"))?;
    let dst_text = std::fs::read_to_string(dst).map_err(|e| format!("读取目标会话失败：{e}"))?;

    // ★ 比较前**必须**把源自己的 cid 换成目标的 cid（与 [`SESSION_ID_PLACEHOLDER`] 同源的
    //   那个坑）：副本的每一行都带**各自的** `sessionId`，不归一化就永远「分歧」。
    let src_normalized = if src_cid == dst_cid {
        src_text.clone()
    } else {
        src_text.replace(src_cid, dst_cid)
    };
    let src_lines = logical_lines(&src_normalized);
    let dst_lines = logical_lines(&dst_text);

    let common = src_lines
        .iter()
        .zip(dst_lines.iter())
        .take_while(|(a, b)| a == b)
        .count();

    // ★ 分歧判据：**公共前缀 < 较短的一方** ⇒ 双方在共同旧版之后各自改过。
    //   （注意不能写成 `< 目标行数`：目标比源长且源是其前缀时，那是「目标更新」，不是分歧。）
    let shorter = src_lines.len().min(dst_lines.len());
    if common < shorter {
        return Err(format!(
            "目标会话与源在共同旧版之后各自有改动（共同旧版 {common} 行，源 {} 行 / 目标 {} 行）⇒ 不做追加",
            src_lines.len(),
            dst_lines.len()
        ));
    }

    let base_lines = dst_lines.len();
    if base_lines >= src_lines.len() {
        // 目标不比源少 ⇒ 已一致（或目标更新）⇒ 无事可做，且**不写任何字节**。
        return Ok(AppendDeltaReport {
            appended_lines: 0,
            base_lines,
            total_lines: base_lines,
        });
    }

    // jsonl 是行式记录：追加块必须以 `\n` 收尾，否则下一次追加会与这一条粘成一行。
    let mut delta = src_lines[base_lines..].join("\n");
    delta.push('\n');

    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(dst)
        .map_err(|e| format!("打开目标会话失败：{e}"))?;
    // 目标末行若没有换行符，先补一个 —— 否则新记录会与旧记录粘成一行（JSON 解析即坏）。
    if !dst_text.is_empty() && !dst_text.ends_with('\n') {
        file.write_all(b"\n")
            .map_err(|e| format!("写入目标会话失败：{e}"))?;
    }
    file.write_all(delta.as_bytes())
        .map_err(|e| format!("写入目标会话失败：{e}"))?;

    Ok(AppendDeltaReport {
        appended_lines: src_lines.len() - base_lines,
        base_lines,
        total_lines: src_lines.len(),
    })
}

/// 逻辑行：按 `\n` 切分，并丢掉「文件以换行结尾」产生的那一个空尾元素。
///
/// ⚠️ 不能用 `split_inclusive('\n')` 直接比较：末行有没有 `\n` 会让**同一行**看起来不同
/// （`"a\n"` vs `"a"`）⇒ 会把「目标末行缺换行」误判成分歧。
fn logical_lines(text: &str) -> Vec<&str> {
    let mut lines: Vec<&str> = text.split('\n').collect();
    if matches!(lines.last(), Some(&"")) {
        lines.pop();
    }
    lines
}

/// 解析「源 / 目标会话正文」两个路径；任一侧缺失 ⇒ `Err`（**fail-closed**，宁可不做也不猜路径）。
fn resolve_jsonl_pair(
    source_region: Region,
    source_cid: &str,
    target_region: Region,
    target_cid: &str,
) -> Result<(PathBuf, PathBuf), String> {
    let src = find_project_jsonl_for(source_region, source_cid)
        .ok_or_else(|| format!("找不到源会话正文（{source_cid}）"))?;
    let dst = find_project_jsonl_for(target_region, target_cid)
        .ok_or_else(|| format!("找不到目标会话正文（{target_cid}）"))?;
    Ok((src, dst))
}

/// 按 region / cid 解析真实路径后执行 [`append_delta_between_files`]。
///
/// 两侧任一会话正文找不到 ⇒ `Err`（**fail-closed**：宁可不做，也不猜路径）。
pub fn sync_session_replica_for(
    source_region: Region,
    source_cid: &str,
    target_region: Region,
    target_cid: &str,
) -> Result<AppendDeltaReport, String> {
    let (src, dst) = resolve_jsonl_pair(source_region, source_cid, target_region, target_cid)?;
    append_delta_between_files(&src, &dst, source_cid, target_cid)
}

/// ★ P14 S1：写入口复核令牌时需要的「组快照」。
///
/// 只有两样东西：**结构指纹**（组还是不是你看到的那个组）+ **成员 cid 集合**
/// （你提交的这条任务属不属于这个组）。
struct GroupGuard {
    fingerprint: String,
    session_ids: BTreeSet<String>,
}

/// ★ P14 S1：账本 → `组键 → 组快照`。
///
/// **只在真需要时才构建**：调用方先扫一遍 items 看有没有令牌，没有就完全不读账本
/// ⇒ 老调用方零成本（与「令牌是可选参数」的回滚口径一致）。
///
/// ⚠️ 与 [`replica_groups_from_ledger`] 共用 [`replica_group_members_from_ledger`] ——
/// **不许各算一份**，否则前端拿到的令牌与这里算出来的指纹会永远对不上。
fn group_guard_index(region: Region) -> BTreeMap<String, GroupGuard> {
    let ledger = load_copy_ledger_at(&copy_ledger_path_for(region));
    replica_group_members_from_ledger(&ledger)
        .into_iter()
        .map(|(key, members)| {
            let session_ids = members.iter().map(|(_, cid)| cid.clone()).collect();
            (
                key,
                GroupGuard {
                    fingerprint: group_fingerprint(&members),
                    session_ids,
                },
            )
        })
        .collect()
}

/// 该条任务携带的乐观令牌（`previewToken` 非空）。
fn item_has_preview_token(item: &Value) -> bool {
    item.get("previewToken")
        .and_then(Value::as_str)
        .map(str::trim)
        .is_some_and(|token| !token.is_empty())
}

/// 该条任务携带的会话组键（空 / 缺失 ⇒ `None`）。
///
/// **回显给前端**：批量提交里一条失败之后，前端要能按**组**重算任务再重试
/// —— 见 `SessionsPage.retrySync`（直接复用旧 items 会把旧令牌再送一遍，必然再失败）。
fn item_group_key(item: &Value) -> Option<String> {
    item.get("groupKey")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|key| !key.is_empty())
        .map(str::to_string)
}

/// ★ P14 S1：**令牌复核**。`Err(原因)` ⇒ 该条**直接判失败、不进文件层**（同批其余照跑）。
///
/// ## 三种情形
///
/// - **未携带令牌** ⇒ `Ok(())`，**退化为今天的行为**。这是刻意的：令牌是**可选参数**
///   （见 `docs/specs/P14-TASKS.md` S1 的回滚口径），旧调用方不传也能跑。
///   ⚠️ **代价要如实记住**：不传 = 不做陈旧检测。**前端一律传**，所以线上等价于强制。
/// - **组键在账本里找不到** ⇒ `Err`（组已被删除，或账本被别的操作改过）；
/// - **指纹不符** ⇒ `Err`（组结构变了：多了 / 少了成员）。
///
/// ## 为什么还要校验「两个 cid 属于同一组」
///
/// 令牌只证明「**组**没变」，不证明「**你提交的这条任务**属于这个组」。两者都验才算把
/// 「前端算出来的任务清单」与「后端眼里的组」对齐 —— 而这层校验在今天**完全不存在**。
fn verify_preview_token(
    item: &Value,
    source_cid: &str,
    target_cid: &str,
    index: Option<&BTreeMap<String, GroupGuard>>,
) -> Result<(), String> {
    if !item_has_preview_token(item) {
        return Ok(());
    }
    let group_key = item
        .get("groupKey")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim();
    if group_key.is_empty() {
        return Err("提交缺少会话组标识，请刷新后重试".to_string());
    }
    // 调用方保证：只要有任一条带了令牌，`index` 一定已构建。
    let Some(index) = index else {
        return Err("会话组索引不可用，请刷新后重试".to_string());
    };
    let Some(guard) = index.get(group_key) else {
        return Err("会话组已不存在（可能已被删除或被其他操作改写），请刷新后重试".to_string());
    };
    let token = item
        .get("previewToken")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim();
    if guard.fingerprint != token {
        return Err("会话组已被其他操作修改，请刷新后重试".to_string());
    }
    if !guard.session_ids.contains(source_cid) || !guard.session_ids.contains(target_cid) {
        return Err("提交的会话不属于该会话组，请刷新后重试".to_string());
    }
    Ok(())
}

/// P8 安全同步的**批量**入口（逐条执行 + **部分失败**）—— 两宿主共用。
///
/// `items` 形状：`[{ "sourceCid": "…", "targetCid": "…" }, …]`（camelCase，与前端一致）。
/// 返回 `{ results: [...], okCount, failCount }`，每条含 `ok` / `appendedLines` / `error`。
///
/// ## 为什么是批量 + 逐条返回
///
/// 一次同步多个落后副本是**批量写操作** ⇒ 必须能表达**部分失败**（`PLAN.md` §3.6）：
/// 调用方据此给「汇总条 + 只重试失败项」的出口 —— 一条分歧不该让其余全部白跑。
///
/// ## 为什么循环放在 core（而不是两个宿主各写一遍）
///
/// 与 [`list_replica_groups_for`] 同款：**判据属于 core**，宿主只做「取参数 / 鉴权 /
/// 丢到阻塞线程」。循环写两遍迟早漂移（一边漏了空 id 校验、一边漏了失败计数）。
pub fn sync_session_replicas_for(region: Region, items: &[Value]) -> Value {
    let mut results: Vec<Value> = Vec::with_capacity(items.len());
    let mut ok_count = 0usize;

    // ★ P14 S1：**只在真有令牌时**才读账本建索引（老调用方零成本）。
    let guard_index = items
        .iter()
        .any(item_has_preview_token)
        .then(|| group_guard_index(region));

    for item in items {
        let source_cid = item
            .get("sourceCid")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim()
            .to_string();
        let target_cid = item
            .get("targetCid")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim()
            .to_string();

        // 空 id 直接判失败，**不进**文件层（否则会拿空串去 replace，语义无意义）。
        if source_cid.is_empty() || target_cid.is_empty() {
            results.push(json!({
                "sourceCid": source_cid,
                "targetCid": target_cid,
                "groupKey": item_group_key(item),
                "ok": false,
                "error": "源 / 目标会话 id 不能为空",
            }));
            continue;
        }

        // ★ P14 S1：令牌复核 —— 不符即该条失败，**一个字节都不写**（同批其余照跑）。
        if let Err(error) =
            verify_preview_token(item, &source_cid, &target_cid, guard_index.as_ref())
        {
            results.push(json!({
                "sourceCid": source_cid,
                "targetCid": target_cid,
                "groupKey": item_group_key(item),
                "ok": false,
                "error": error,
            }));
            continue;
        }

        // ⚠️ 方向由调用方给（源应是行数更多的那份）。给反了**不会写坏数据** ——
        //    目标不落后 ⇒ 走「已一致」分支 ⇒ `ok: true, appendedLines: 0`。
        match sync_session_replica_for(region, &source_cid, region, &target_cid) {
            Ok(report) => {
                ok_count += 1;
                results.push(json!({
                    "sourceCid": source_cid,
                    "targetCid": target_cid,
                    "groupKey": item_group_key(item),
                    "ok": true,
                    "appendedLines": report.appended_lines,
                }));
            }
            Err(error) => results.push(json!({
                "sourceCid": source_cid,
                "targetCid": target_cid,
                "groupKey": item_group_key(item),
                "ok": false,
                "error": error,
            })),
        }
    }

    let fail_count = results.len() - ok_count;
    json!({
        "results": results,
        "okCount": ok_count,
        "failCount": fail_count,
    })
}

/// P8 分歧处理的执行报告。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UnifyReport {
    /// 是否真的覆盖了目标（`false` = 本来就与源一致 ⇒ **未写盘、也未备份**）。
    pub overwritten: bool,
    /// 目标原有的行数。
    pub previous_lines: usize,
    /// 覆盖后的行数（= 源的行数）。
    pub total_lines: usize,
    /// ★ 用户**会丢掉的**条数（目标相对共同旧版多出来的那部分）—— 供 UI **如实告知**。
    pub lost_lines: usize,
    /// 备份路径（`overwritten: false` 时为 `None`）。**没备份成功就不覆盖**。
    pub backup: Option<String>,
}

/// ★ P8 **分歧处理**：把**选定的那一份**统一到目标副本 —— 这是**覆盖**，不是加法。
///
/// ## 为什么必须是「覆盖」
///
/// 分歧的定义就是「双方在共同旧版之后**各自改过**」⇒ 加法**无法**让两边一致
/// （各自都有对方没有的内容）。`PLAN.md` §3.15.2 因此写死：
/// 「**有分叉时必须先选保留哪一份**才能统一（**不许静默覆盖**）」。
/// 用户显式选了「保留哪一份」，其余副本的分歧内容**会被丢弃** ⇒
/// UI 必须把 `lost_lines` **如实**告诉用户，并给二次确认（§3.5 不许假承诺）。
///
/// ## 三条安全约束
///
/// 1. **先备份再覆盖**：目标 jsonl 原样存到 `backup_root/<dst_cid>.jsonl`；
///    **备份写不成就不覆盖** —— 与 `backup_failure_is_reported_instead_of_faked` 同一纪律：
///    谎报「有回滚点」比不备份更坏；
/// 2. **一致就不动**：内容已与源一致 ⇒ `overwritten: false`，**不写盘也不备份**；
/// 3. **id 归一化**：写进去的内容要把 `src_cid` 换成 `dst_cid`（同 [`copy_session_to_user_cross`]）。
pub fn unify_replica_between_files(
    src: &Path,
    dst: &Path,
    src_cid: &str,
    dst_cid: &str,
    backup_root: &Path,
) -> Result<UnifyReport, String> {
    if src == dst {
        return Err("源与目标是同一个文件，拒绝统一".into());
    }
    let src_text = std::fs::read_to_string(src).map_err(|e| format!("读取源会话失败：{e}"))?;
    let dst_text = std::fs::read_to_string(dst).map_err(|e| format!("读取目标会话失败：{e}"))?;

    // 与「安全同步」同源的 id 归一化：副本每行都带**各自的** sessionId。
    let normalized = if src_cid == dst_cid {
        src_text.clone()
    } else {
        src_text.replace(src_cid, dst_cid)
    };
    let src_lines = logical_lines(&normalized);
    let dst_lines = logical_lines(&dst_text);

    let common = src_lines
        .iter()
        .zip(dst_lines.iter())
        .take_while(|(a, b)| a == b)
        .count();
    let lost_lines = dst_lines.len().saturating_sub(common);

    // ① 逐行相同且行数相同 ⇒ 本来就一致，**不写盘**。
    if common == src_lines.len() && src_lines.len() == dst_lines.len() {
        return Ok(UnifyReport {
            overwritten: false,
            previous_lines: dst_lines.len(),
            total_lines: src_lines.len(),
            lost_lines: 0,
            backup: None,
        });
    }

    // ② 先备份；**备份不成功就绝不覆盖**。
    std::fs::create_dir_all(backup_root).map_err(|e| format!("创建备份目录失败：{e}"))?;
    let backup_path = backup_root.join(format!("{dst_cid}.jsonl"));
    std::fs::copy(dst, &backup_path)
        .map_err(|e| format!("备份目标会话失败（**未做任何覆盖**）：{e}"))?;
    // ★ 复核备份**真的落盘且字节数一致** —— 只信 `copy` 的返回值不够
    //   （本仓有过「吞掉错误、无条件返回备份路径」的事故）。
    let backed_up = std::fs::metadata(&backup_path)
        .map(|m| m.len())
        .unwrap_or(0);
    let original = std::fs::metadata(dst).map(|m| m.len()).unwrap_or(0);
    if backed_up != original {
        return Err(format!(
            "备份校验失败（备份 {backed_up} 字节 / 原文件 {original} 字节）⇒ **未做任何覆盖**"
        ));
    }

    // ③ 覆盖 —— 这一行是**破坏性**的：目标原有的分歧内容就此丢弃（已有备份兜底）。
    std::fs::write(dst, normalized.as_bytes()).map_err(|e| format!("写入目标会话失败：{e}"))?;

    Ok(UnifyReport {
        overwritten: true,
        previous_lines: dst_lines.len(),
        total_lines: src_lines.len(),
        lost_lines,
        backup: Some(backup_path.to_string_lossy().into_owned()),
    })
}

/// P8 分歧处理的**批量**入口（逐条执行 + **部分失败**）—— 两宿主共用。
///
/// `items` 形状：`[{ "sourceCid": "…", "targetCid": "…" }, …]`，其中 `sourceCid` 是
/// **用户选定要保留的那一份**，`targetCid` 是**被它覆盖**的其余副本。
///
/// 返回 `{ results: [...], okCount, failCount }`，每条含
/// `overwritten` / `lostLines` / `previousLines` / `totalLines` / `backup` / `error`。
///
/// ⚠️ 与 [`sync_session_replicas_for`] 的两点差别（**别混用**）：
/// - 这里**会删内容**（覆盖），同步只追加；UI 必须对这里做二次确认；
/// - 备份目录按**本次调用**共用（`sessions-unify/<utc>`），不是每条一个目录。
pub fn unify_session_replicas_for(region: Region, items: &[Value]) -> Value {
    let backup_root = backup_dir().join("sessions-unify").join(utc_iso());
    let mut results: Vec<Value> = Vec::with_capacity(items.len());
    let mut ok_count = 0usize;

    // ★ P14 S1：同 [`sync_session_replicas_for`] —— 只在真有令牌时才读账本建索引。
    let guard_index = items
        .iter()
        .any(item_has_preview_token)
        .then(|| group_guard_index(region));

    for item in items {
        let source_cid = item
            .get("sourceCid")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim()
            .to_string();
        let target_cid = item
            .get("targetCid")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim()
            .to_string();

        if source_cid.is_empty() || target_cid.is_empty() {
            results.push(json!({
                "sourceCid": source_cid,
                "targetCid": target_cid,
                "groupKey": item_group_key(item),
                "ok": false,
                "error": "源 / 目标会话 id 不能为空",
            }));
            continue;
        }

        // ★ P14 S1：令牌复核 —— 必须在**备份 / 覆盖之前**，否则「拒绝」也会留下副作用。
        if let Err(error) =
            verify_preview_token(item, &source_cid, &target_cid, guard_index.as_ref())
        {
            results.push(json!({
                "sourceCid": source_cid,
                "targetCid": target_cid,
                "groupKey": item_group_key(item),
                "ok": false,
                "error": error,
            }));
            continue;
        }

        let outcome =
            resolve_jsonl_pair(region, &source_cid, region, &target_cid).and_then(|(src, dst)| {
                let report = unify_replica_between_files(
                    &src,
                    &dst,
                    &source_cid,
                    &target_cid,
                    &backup_root,
                )?;
                // 把 `dst` 一起带出来 —— 记基线要读**覆盖后**的目标副本。
                Ok((report, dst))
            });

        match outcome {
            Ok((report, dst)) => {
                ok_count += 1;
                // ★ P14 S2：**真的覆盖了**才刷新一致点（「本来就一致」不是一次新的会合，
                //   见 `record_pair_baseline_for` 的说明）。
                if report.overwritten {
                    record_pair_baseline_for(region, &source_cid, &target_cid, &dst);
                }
                results.push(json!({
                    "sourceCid": source_cid,
                    "targetCid": target_cid,
                    "groupKey": item_group_key(item),
                    "ok": true,
                    "overwritten": report.overwritten,
                    "previousLines": report.previous_lines,
                    "totalLines": report.total_lines,
                    "lostLines": report.lost_lines,
                    "backup": report.backup,
                }));
            }
            Err(error) => results.push(json!({
                "sourceCid": source_cid,
                "targetCid": target_cid,
                "groupKey": item_group_key(item),
                "ok": false,
                "error": error,
            })),
        }
    }

    let fail_count = results.len() - ok_count;
    // 分歧处理同样会往 `backups/sessions-unify/<utc>` 拍备份 ⇒ 一并做自动保留
    // （与 `run_copy_batch` 同一条保留纪律，见 `cache::prune_session_backup_dirs`）。
    crate::modules::cache::prune_session_backup_dirs(crate::modules::cache::SESSION_BACKUP_KEEP);
    json!({
        "results": results,
        "okCount": ok_count,
        "failCount": fail_count,
    })
}

/// jsonl 头部能拿到的最小元数据（cwd / 标题）。
///
/// ## ★ 字段名是实测出来的，别照字面猜
///
/// WorkBuddy 5.5.x 的会话 jsonl 里，元数据**不在首行的同一个对象上**（实测三份真实会话）：
/// - `cwd`：**每一行都带**（首行 `type:"message"` 也有），值是真实路径（如 `d:\_03_WorkBuddy\workbuddy-switch`）；
/// - 标题：在 `type:"ai-title"` 那一行的 **`aiTitle`** 字段（实测**第 3 行**）；
/// - `customTitle`（用户改名 / 定时任务名）出现在**更靠后**的记录里（实测第 56 行）。
///
/// ⚠️ 早期实现只认 `title` ⇒ 标题**永远取不到**，降级扫描出来的会话**全是「(无标题)」**。
/// 而 db 的 `title` 列实测**就等于 `aiTitle`**（逐条比对过），`custom_title` 是另一列
/// ⇒ 优先级与 [`session_display_title`] 对齐：**`customTitle` > `aiTitle`**
/// （`title` 作为兼容别名一起认，供旧格式与既有测试夹具使用）。
#[derive(Debug, Default, Clone)]
struct JsonlMeta {
    cwd: String,
    title: String,
}

/// 只读文件头这么多字节去找元数据。
///
/// **刻意不读整个文件**：会话正文实测可到 2.3MB，而 `projects/**` 动辄几百个文件；
/// cwd / 标题都在前几行。旧实现 `read_to_string` 把整份读进来只取前 8 行，
/// 且扫描器对同一文件**调了两次** —— 等于每个文件读两遍全文（现已合并为 [`push_jsonl_session`] 里的一次）。
const JSONL_META_HEAD_BYTES: u64 = 64 * 1024;

/// 头部内最多看多少行。实测元数据在第 3 / 第 56 行，128 行留足余量；
/// 真正的成本闸是上面的**字节**上限（一行超长时不会白扫）。
const JSONL_META_MAX_LINES: usize = 128;

/// 从 jsonl 头部解析 cwd / 标题。文件不存在 / 不可读 / 非 JSON / 字段缺失
/// 均返回 `None`，**绝不报错**（降级路径的目标是「让 UI 至少列得出来」）。
fn read_jsonl_meta(path: &Path) -> Option<JsonlMeta> {
    let file = std::fs::File::open(path).ok()?;
    let mut head = Vec::new();
    file.take(JSONL_META_HEAD_BYTES)
        .read_to_end(&mut head)
        .ok()?;
    // 头部可能正好切在一行中间（甚至多字节字符中间）⇒ 有损解码，末行解析失败会被跳过。
    let text = String::from_utf8_lossy(&head);

    let mut cwd = String::new();
    let mut ai_title = String::new();
    let mut custom_title = String::new();
    for line in text.lines().take(JSONL_META_MAX_LINES) {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if cwd.is_empty() {
            cwd = text_field(&v, "cwd");
        }
        if ai_title.is_empty() {
            ai_title = text_field(&v, "aiTitle");
            if ai_title.is_empty() {
                ai_title = text_field(&v, "title");
            }
        }
        if custom_title.is_empty() {
            custom_title = text_field(&v, "customTitle");
        }
        // 三项都齐了就不必再解析后面的行（正常格式第 3 行就该齐）。
        if !cwd.is_empty() && !ai_title.is_empty() && !custom_title.is_empty() {
            break;
        }
    }
    let title = if custom_title.is_empty() {
        ai_title
    } else {
        custom_title
    };
    if cwd.is_empty() && title.is_empty() {
        return None;
    }
    Some(JsonlMeta { cwd, title })
}

/// 取字符串字段并 trim；缺失 / 非字符串（脏值）一律给空串，不参与展示。
fn text_field(v: &Value, key: &str) -> String {
    v.get(key)
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string()
}

/// 从 `~/.workbuddy{,-ai}/projects/` 扫描会话 jsonl，从文件名得到 cid、
/// 从**文件头**（见 [`read_jsonl_meta`]）解析 cwd / 标题。**仅在 sessions 表不可读 / 为空时调用**。
///
/// **只在两层内取文件**：`projects/<cid>.jsonl`（扁平布局）与 `projects/<workspace>/<cid>.jsonl`。
/// ⚠️ **刻意不再往下递归**：`projects/<workspace>/<cid>/subagents/agent-*.jsonl` 是**子代理**
/// 记录，不是会话 —— 实测（2026-09-24）国内版 `projects/` 里 391 个真会话旁边躺着 **81 个**
/// 子代理文件；无界递归会把它们当成会话，切换弹窗里凭空多出几十条 `agent-xxxx` 假条目。
/// （本函数此前的注释写着「层数写死」，但实现是无界递归 —— 这里把注释与实现对齐。）
///
/// 返回顺序：按文件 mtime DESC（最近活动在前），与 db 版同语义。
/// Claw 工作区会被跳过，与 db 版语义对齐（缺 cwd 时不判 Claw）。
///
/// 限制：仅扫描 `.jsonl`；`.file-rollback.ndjson` / `.meta.json` 等忽略。
fn scan_sessions_from_jsonl_for(region: Region) -> Vec<Value> {
    let projects = session_data_dir(region).join("projects");
    if !projects.is_dir() {
        return Vec::new();
    }
    // ★ issue #147：反扫列表必须能标出「这条其实是副本」—— 否则用户会把上一代副本
    //   当成源会话再勾选一遍（前端据此默认不勾选）。
    let ledger = load_copy_ledger_at(&copy_ledger_path_for(region));
    let (copy_origins, _) = build_origin_indexes(&ledger);
    let mut sessions: Vec<(i64, Value)> = Vec::new();
    let Ok(entries) = std::fs::read_dir(&projects) else {
        return Vec::new();
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() {
            push_jsonl_session(&path, &copy_origins, &mut sessions);
            continue;
        }
        let Ok(children) = std::fs::read_dir(&path) else {
            continue;
        };
        for child in children.flatten() {
            push_jsonl_session(&child.path(), &copy_origins, &mut sessions);
        }
    }
    sessions.sort_by_key(|s| std::cmp::Reverse(s.0));
    sessions.into_iter().map(|(_, v)| v).collect()
}

/// 把单个路径收成一条会话记录；不是 `.jsonl` 文件 / 读不出元数据时跳过（**绝不报错**）。
///
/// `copy_origins` 是账本血缘索引：命中即说明该 cid 是某次复制生成的副本，
/// 条目上打 `isCopy: true`（issue #147）。
fn push_jsonl_session(
    path: &Path,
    copy_origins: &HashMap<String, (String, String)>,
    out: &mut Vec<(i64, Value)>,
) {
    if path.extension().and_then(|s| s.to_str()) != Some("jsonl") {
        return;
    }
    let Some(cid) = path
        .file_stem()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty())
    else {
        return;
    };
    let Ok(md) = std::fs::metadata(path) else {
        return;
    };
    if !md.is_file() {
        return;
    }
    // **只读一次**：cwd 与标题同在这一个头部里，读两遍等于每个文件读两遍全文。
    let meta = read_jsonl_meta(path);
    let cwd = meta.as_ref().map(|m| m.cwd.clone()).unwrap_or_default();
    if !cwd.is_empty() && is_claw_workspace(&cwd) {
        return;
    }
    let title = meta
        .map(|m| m.title)
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| "(无标题)".to_string());
    let mtime = md
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    out.push((
        mtime,
        json!({
            "id": cid,
            "title": title,
            "cwd": cwd,
            "updatedAt": mtime,
            "hasHistory": true,
            "isPlayground": false,
            "degraded": true,
            // ★ issue #147：副本标记 —— 前端据此默认不勾选，避免「副本的副本」。
            "isCopy": copy_origins.contains_key(cid),
        }),
    ));
}

/// 备份 workbuddy.db（**SQLite 在线备份 API**，产物是自包含单文件快照）。
///
/// 返回值刻意用 `Result<Option<PathBuf>, String>` 三态，而不是 `Option<PathBuf>`：
///
/// | 返回 | 含义 |
/// | --- | --- |
/// | `Ok(Some(path))` | 备份成功 |
/// | `Ok(None)` | **无需**备份（源库不存在）—— 不是失败 |
/// | `Err(msg)` | **备份失败** —— 必须让用户知道 |
///
/// 为什么不能只用 `Option`：`None` 会把「无需备份」与「备份失败」压成同一个值，
/// 调用方就无法只对后者报警。这正是本次修的缺陷 —— 三个副本原先都用
/// `let _ = std::fs::copy(..)` 吞掉错误、然后**无条件**返回备份路径，
/// 于是报告里 `backup` 字段永远有值，用户以为有回滚点，真出事才发现备份目录是空的。
/// （纪律：**失败不得谎报成功**。）
fn backup_workbuddy_db(region: Region, backup_root: &Path) -> Result<Option<PathBuf>, String> {
    backup_db_to(workbuddy_db_path_for(region), backup_root)
}

/// 在线备份的分步页数（每步拷贝多少页）。
///
/// 分步的意义：`Backup` 每步之后会把读锁放开，源库被别的进程写时才不会把我们
/// 卡在 `Busy` 上死等；一次拷完在大库上会长时间持锁（本仓的 `workbuddy.db` 可以很大）。
const DB_BACKUP_PAGES_PER_STEP: i32 = 1024;

/// [`backup_workbuddy_db`] 的**纯函数形态**（显式入参，测试用，不碰进程级 home）。
///
/// 与 `account::library_display_name_in` 同款拆分理由：单测不该依赖 `BUDDY_SWITCH_HOME`。
///
/// ## ★ 为什么不是 `fs::copy`（P14 S3 的改动）
///
/// 旧实现把 `workbuddy.db` + `-wal` + `-shm` **三个文件各拷一次**，三个问题：
///
/// 1. **三次拷贝发生在不同时刻** ⇒ 三者可能互相对不上（源库此刻正在写）；
/// 2. **`-shm` 是派生文件**（SQLite 官方明说它不该被复制，应由进程自行重建）；
/// 3. 结果是「备份**必须**三个文件一起保留才**可能**可用」—— 用户一旦只拿走
///    `workbuddy.db`（备份最自然的用法：一个文件就是一份快照），就**静默丢数据**。
///
/// 在线备份 API 产出的是**单文件、自包含、某一时刻的一致快照**：拷到哪儿都完整，
/// 而且能被 `PRAGMA integrity_check` 验。**这两条都有单测钉着**
/// （`db_backup_is_a_self_contained_snapshot`）。
///
/// ## 失败一律**清掉半截产物**
///
/// 一个不完整 / 校验不过的 `workbuddy.db` 看起来像可用备份，**比没有更坏**
/// （用户以为有回滚点）。⇒ 任何一步失败都 `remove_file` 再返回 `Err`。
fn backup_db_to(db: PathBuf, backup_root: &Path) -> Result<Option<PathBuf>, String> {
    if !db.is_file() {
        return Ok(None);
    }
    std::fs::create_dir_all(backup_root).map_err(|e| format!("创建备份目录失败: {e}"))?;
    let target = backup_root.join("workbuddy.db");
    // 目标位置被一个**目录**占住 ⇒ 明确报错（不要让 SQLite 给出一个含糊的 open 失败）。
    if target.is_dir() {
        return Err(format!(
            "备份失败：{} 已被一个目录占住，无法写入快照",
            target.display()
        ));
    }
    // 清掉上一轮的残留（含旧版本会留下的 -wal / -shm —— 单文件快照旁边不该躺着无关边车）。
    for path in snapshot_sidecars(&target) {
        let _ = std::fs::remove_file(path);
    }

    // 连接全部收在**闭包**里：Windows 上「文件仍被打开」时删不掉
    //（下面的失败清理依赖这一点）。
    let outcome = (|| -> Result<(), String> {
        // 源库**只读**打开：备份绝不该写用户客户端的数据。
        let source = Connection::open_with_flags(&db, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(|e| format!("备份失败：打不开源库 {}（{e}）", db.display()))?;
        let mut dest =
            Connection::open(&target).map_err(|e| format!("备份失败：建不了目标库（{e}）"))?;

        {
            let backup = rusqlite::backup::Backup::new(&source, &mut dest)
                .map_err(|e| format!("备份失败：{e}"))?;
            backup
                .run_to_completion(DB_BACKUP_PAGES_PER_STEP, Duration::from_millis(50), None)
                .map_err(|e| format!("备份失败：{e}"))?;
            // ⚠️ rusqlite **不暴露** `finish()` —— 收尾在 `Backup` 的 `Drop` 里
            //    （`sqlite3_backup_finish`）⇒ 必须让它**出作用域**，不能 `mem::forget`。
        }

        // ★ 让快照落成**单文件**。
        //
        // `journal_mode` 是**持久化在库头里的属性**：从 WAL 源库备份出来的库**也是 WAL**
        // ⇒ 旁边照样会生成 `-wal` / `-shm`，用户只拿走 `workbuddy.db` 就**又丢数据**。
        // 切回 rollback journal（`DELETE`）会把 WAL 内容并回主库并删掉边车。
        let mode: String = dest
            .query_row("PRAGMA journal_mode=DELETE", [], |row| row.get(0))
            .map_err(|e| format!("备份失败：快照落盘（{e}）"))?;
        if !mode.eq_ignore_ascii_case("delete") {
            return Err(format!(
                "备份失败：快照未能落成单文件（journal_mode={mode}）"
            ));
        }
        Ok(())
    })();

    if let Err(error) = outcome {
        // ⚠️ 连接已在闭包里全部析构 —— Windows 上文件仍被打开时 `remove_file` 会**静默失败**。
        let _ = std::fs::remove_file(&target);
        for path in snapshot_sidecars(&target) {
            let _ = std::fs::remove_file(path);
        }
        return Err(error);
    }

    // ★ 自检：备份必须真的是一份**可读且完整**的库 —— 不信「API 返回 Ok」。
    if let Err(error) = verify_sqlite_snapshot(&target) {
        let _ = std::fs::remove_file(&target);
        return Err(format!("备份校验失败（**已丢弃该备份**）：{error}"));
    }

    Ok(Some(target))
}

/// 一个 SQLite 主库文件旁边**可能**出现的边车文件（`-wal` / `-shm`）。
fn snapshot_sidecars(db: &Path) -> Vec<PathBuf> {
    ["-wal", "-shm"]
        .iter()
        .map(|suffix| PathBuf::from(format!("{}{suffix}", db.to_string_lossy())))
        .collect()
}

/// `PRAGMA integrity_check` —— 备份产物必须**自己就能过**。
///
/// 只读打开（校验不该动它）。返回 `Err(说明)` 时调用方会**丢弃**该备份。
fn verify_sqlite_snapshot(path: &Path) -> Result<(), String> {
    let conn = Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|e| format!("打不开备份文件：{e}"))?;
    let verdict: String = conn
        .query_row("PRAGMA integrity_check", [], |row| row.get(0))
        .map_err(|e| format!("完整性检查无法执行：{e}"))?;
    if verdict == "ok" {
        Ok(())
    } else {
        Err(format!("完整性检查未通过：{verdict}"))
    }
}

/// 把 source_uid 的一个会话复制为 target_uid 的新会话（路径 B：生成新 id）。
///
/// 全部按「新 id」复制一份给目标账号，源账号数据完全不动。
/// 新 id 必须用带连字符的 UUID 格式（`Uuid::new_v4().to_string()`），与官方一致；
/// 32 位无连字符形式会导致 WorkBuddy 无法识别新会话。
pub fn copy_session_to_user(
    cid: &str,
    source_uid: &str,
    target_uid: &str,
) -> Result<Value, String> {
    copy_session_to_user_for(Region::Cn, cid, source_uid, target_uid)
}

/// 按 region（同一版本内）把 source_uid 的一个会话复制为 target_uid 的新会话（路径 B）。
pub fn copy_session_to_user_for(
    region: Region,
    cid: &str,
    source_uid: &str,
    target_uid: &str,
) -> Result<Value, String> {
    copy_session_to_user_cross(region, region, cid, source_uid, target_uid)
}

/// 跨版本复制：正文与元数据**读自 `source_region`**，副本与云端映射**写入 `target_region`**。
///
/// **去重**：若该源会话此前已复制给同一目标账号且副本仍在，则不重复复制，
/// 直接返回既有副本（`deduplicated: true`）。语义与 [`copy_sessions_for_switch_cross`]
/// 报告里的 `skipped` 条目逐字一致（同一条管线的两个出口）。
///
/// **降级**：源 sessions 表不可读时，从 jsonl 第一行推断 cwd 做 Claw 检查；
/// 源行缺失（全新账号 / 仅 jsonl 存在）时**合成最小索引行**（issue #146）。
/// 目标 sessions 表不可写时，jsonl 仍能复制，但报告 `sessionRowWritten: false` +
/// `sessionRowError` + 批次 `failed: true` —— **不得**承诺「重启后客户端会重建索引」：
/// 客户端只认 `sessions` 表，不会从 jsonl 反推。
///
/// 单发入口：内部走与批量复制**同一条**三段管线（见模块文档），
/// 再把唯一一个会话的结局还原成旧签名的 `Result`（Claw 拒绝 → `Err`）。
pub fn copy_session_to_user_cross(
    source_region: Region,
    target_region: Region,
    cid: &str,
    source_uid: &str,
    target_uid: &str,
) -> Result<Value, String> {
    let ids = [cid.to_string()];
    let report = run_copy_batch(
        source_region,
        target_region,
        source_uid,
        target_uid,
        &ids,
        None,
    );
    // 批量报告里只有一个元素：把三种结局还原成旧签名的返回值。
    if let Some(errors) = report.get("errors").and_then(Value::as_array) {
        if let Some(first) = errors.first() {
            if first.get("id").and_then(Value::as_str) == Some(cid) {
                return Err(first
                    .get("error")
                    .and_then(Value::as_str)
                    .unwrap_or("复制失败")
                    .to_string());
            }
        }
    }
    if let Some(skipped) = report.get("skipped").and_then(Value::as_array) {
        if let Some(first) = skipped.first() {
            return Ok(first.clone());
        }
    }
    report
        .get("copied")
        .and_then(Value::as_array)
        .and_then(|items| items.first())
        .cloned()
        .ok_or_else(|| format!("会话未复制: {cid}"))
}

// ---------------------------------------------------------------------------
// 批量复制（切换流程专用）：三段管线
// ---------------------------------------------------------------------------

/// jsonl 正文复制的并行度上限。
///
/// 为什么是 4：正文复制是纯文件 I/O（读 → replace → 写），机械硬盘上加线程无用、
/// SSD 上 4 条队列基本能吃满；再加只会让 `projects/` 目录同时出现更多新文件，
/// 抢同一个目录 inode 锁的副作用大于收益。会话数更少时按需下调（见 phase A）。
const MAX_JSONL_COPY_WORKERS: usize = 4;

/// 一次批量复制的**共享上下文**：三段管线全程复用同一批资源。
///
/// 旧实现是「一个会话一路做到底」，每个会话都要：开源库读 cwd、再开源库读整行、
/// **整库备份一次**、开目标库插行、开边车库注册、读写一遍账本、全目录扫一遍
/// projects —— 这些**与会话数成正比**的常数开销叠起来，就是「复制会话让切换等
/// 很久」的大头之一。本结构把它们全部收敛为「整批一次」。
struct CopyBatch {
    source_region: Region,
    target_region: Region,
    source_uid: String,
    target_uid: String,
    /// 源 workbuddy.db（只读，懒开）—— phase 0 读源行共用。
    src_conn: Option<Connection>,
    /// 目标 workbuddy.db（读写，懒开）—— 去重存活判定与插索引行共用。
    dst_conn: Option<Connection>,
    /// 目标边车映射库（读写，懒开）—— 注册云端映射共用。
    edge_conn: Option<Connection>,
    /// 目标 sessions 表的列名清单（懒取一次；目标库不可用时为空表）。
    dst_columns: Option<Vec<String>>,
    /// projects 目录的 cid → 正文路径索引（懒建一次）。
    projects_index: Option<HashMap<String, PathBuf>>,
    /// 复制账本（整批一次读入内存；`dirty` 控制末尾落盘）。
    ledger: serde_json::Map<String, Value>,
    /// 副本 id → 血缘 `(origin_uid, origin_cid)`（issue #147；加载时构建、登记时增量更新）。
    copy_origins: HashMap<String, (String, String)>,
    /// `(origin_cid, target_uid)`：该原始会话是否已复制给目标账号（跨代去重）。
    origin_copied: HashSet<(String, String)>,
    ledger_path: PathBuf,
    ledger_dirty: bool,
    /// **整批一次**的库备份（懒触发：第一个真正复制的会话之前）。
    backup_attempted: bool,
    backup: Option<PathBuf>,
    backup_error: Option<String>,
    /// ★ P14 S2：基线表（整批一次读入；`dirty` 控制末尾落盘）。
    baselines: serde_json::Map<String, Value>,
    baselines_path: PathBuf,
    baselines_dirty: bool,
}

/// 一个待复制会话的执行计划（phase 0 产出，phase A/B 消费）。
struct CopyPlan {
    cid: String,
    /// 进度展示名（custom_title 优先，与侧栏同口径）。
    title: String,
    /// 新会话 id（带连字符 UUID；WorkBuddy 只认这种形式）。
    new_cid: String,
    /// phase 0 读出的源行（列名 + 值）；`None` = 源行缺失（降级：只复制正文）。
    row: Option<(Vec<String>, Vec<rusqlite::types::Value>)>,
    /// 源正文路径（`None` = 该会话没有正文文件）。
    jsonl_src: Option<PathBuf>,
    /// 副本正文落点（与 `jsonl_src` 同生同灭）。
    jsonl_dst: Option<PathBuf>,
}

impl CopyBatch {
    fn new(
        source_region: Region,
        target_region: Region,
        source_uid: &str,
        target_uid: &str,
    ) -> Self {
        // 账本整批一次读入（旧实现每个会话都读一遍、写完再读一遍）。
        let ledger_path = copy_ledger_path_for(target_region);
        let ledger = load_copy_ledger_at(&ledger_path);
        // ★ issue #147：血缘索引同一次加载时构建，复制前的「副本识别」靠它。
        let (copy_origins, origin_copied) = build_origin_indexes(&ledger);
        // 基线表同样整批一次读入（与账本**同 region 目录**，见 `session_baseline_path_for`）。
        let baselines_path = session_baseline_path_for(target_region);
        let baselines = load_session_baselines_at(&baselines_path);
        Self {
            source_region,
            target_region,
            source_uid: source_uid.to_string(),
            target_uid: target_uid.to_string(),
            src_conn: None,
            dst_conn: None,
            edge_conn: None,
            dst_columns: None,
            projects_index: None,
            ledger,
            copy_origins,
            origin_copied,
            ledger_path,
            ledger_dirty: false,
            backup_attempted: false,
            backup: None,
            backup_error: None,
            baselines,
            baselines_path,
            baselines_dirty: false,
        }
    }

    /// 源库只读连接（懒开；库不存在时为 None）。
    fn src_conn(&mut self) -> Option<&Connection> {
        if self.src_conn.is_none() {
            self.src_conn = open_db(&workbuddy_db_path_for(self.source_region), true);
        }
        self.src_conn.as_ref()
    }

    /// 目标库读写连接（懒开；**库文件不存在时绝不去建** —— 与旧实现的 `is_file` 闸对齐）。
    fn dst_conn(&mut self) -> Option<&Connection> {
        if self.dst_conn.is_none() {
            let path = workbuddy_db_path_for(self.target_region);
            if path.is_file() {
                self.dst_conn = open_db(&path, false);
            }
        }
        self.dst_conn.as_ref()
    }

    /// 目标 sessions 表列名（懒取；表 / 库不可用 → 空表）。
    fn dst_columns(&mut self) -> Vec<String> {
        if self.dst_columns.is_none() {
            let columns = match self.dst_conn() {
                Some(conn) if table_exists(conn, "sessions") => table_columns(conn, "sessions"),
                _ => Vec::new(),
            };
            self.dst_columns = Some(columns);
        }
        self.dst_columns.clone().unwrap_or_default()
    }

    /// 目标边车映射库连接（懒开；库不存在时为 None —— 不得新建空库，
    /// 见 [`edge_sync_db_path_for`] 的「写死档位号 = 静默失效」说明）。
    fn edge_conn(&mut self) -> Option<&Connection> {
        if self.edge_conn.is_none() {
            let path = edge_sync_db_path_for(self.target_region);
            if path.is_file() {
                self.edge_conn = open_db(&path, false);
            }
        }
        self.edge_conn.as_ref()
    }

    /// 读源会话行（列名 + 值）；源库 / 表 / 行缺失 → `Ok(None)`；Claw → `Err`。
    fn read_session_row(&mut self, cid: &str) -> Result<Option<SessionRow>, String> {
        let source_uid = self.source_uid.clone();
        let Some(conn) = self.src_conn() else {
            return Ok(None);
        };
        read_session_row(conn, cid, &source_uid)
    }

    /// 目标账号下该副本是否仍存活（去重判据的一半）。
    fn session_exists(&mut self, cid: &str, uid: &str) -> bool {
        let Some(conn) = self.dst_conn() else {
            return false;
        };
        session_exists_with(conn, cid, uid)
    }

    /// 账本命中**且副本仍存活** → 既有副本 id（语义与旧 `ledger_hit_for` 一致）。
    fn ledger_hit(&mut self, cid: &str) -> Option<String> {
        let key = copy_ledger_key_for(
            self.source_region,
            self.target_region,
            &self.source_uid,
            &self.target_uid,
            cid,
        );
        let new_cid = ledger_new_of(self.ledger.get(&key)?)?;
        let target_uid = self.target_uid.clone();
        self.session_exists(&new_cid, &target_uid)
            .then_some(new_cid)
    }

    /// 该 cid 本身是副本时返回它的血缘 `(origin_uid, origin_cid)`（issue #147）。
    fn copy_origin(&self, cid: &str) -> Option<(String, String)> {
        self.copy_origins.get(cid).cloned()
    }

    /// 该原始会话是否**已经复制给当前目标账号**（issue #147 的跨代去重判据）。
    fn origin_already_copied(&self, origin_cid: &str) -> bool {
        self.origin_copied
            .contains(&(origin_cid.to_string(), self.target_uid.clone()))
    }

    /// 登记一次复制（先进内存；落盘见 [`Self::flush_ledger`]）。
    ///
    /// ★ issue #147：值里带**血缘**。源会话本身是副本时沿用它的 origin，否则
    /// origin 就是「源账号 + 源 cid」。没有血缘时账本只能回答「谁复制过谁」，
    /// 无法回答「这个 cid 是不是副本」⇒ 副本会被当成源会话再复制一代（雪崩）。
    fn ledger_record(&mut self, source_cid: &str, new_cid: &str) {
        let key = copy_ledger_key_for(
            self.source_region,
            self.target_region,
            &self.source_uid,
            &self.target_uid,
            source_cid,
        );
        let origin = self
            .copy_origins
            .get(source_cid)
            .cloned()
            .unwrap_or_else(|| (self.source_uid.clone(), source_cid.to_string()));
        let value = json!({
            "new": new_cid,
            "originUid": origin.0,
            "originCid": origin.1,
        });
        self.ledger.insert(key, value);
        self.origin_copied
            .insert((origin.1.clone(), self.target_uid.clone()));
        self.copy_origins.insert(new_cid.to_string(), origin);
        self.ledger_dirty = true;
    }

    /// 账本整批一次落盘；无新登记时视为成功（不写文件）。
    fn flush_ledger(&mut self) -> bool {
        if !self.ledger_dirty {
            return true;
        }
        let ok = save_copy_ledger_at(&self.ledger_path, &self.ledger);
        if ok {
            self.ledger_dirty = false;
        }
        ok
    }

    /// ★ P14 S2：复制成功 ⇒ 把「此刻两份内容一致」记成一条基线。
    ///
    /// ## 为什么指纹从**刚落盘的目标副本**上读，而不是在 phase A 顺手算
    ///
    /// phase A 那边正文已经在内存里（零额外 I/O），但**不动它**更划算：
    /// 那一段有「并行复制一个都不落」的变异护栏，为省一次「刚写过、必然在页缓存里」
    /// 的读去改它，风险与收益不成比例。将来若真成瓶颈，再把指纹搬回 phase A
    /// （`fingerprint_of_text` 已经备好）。
    ///
    /// ## 失败即**不建**基线
    ///
    /// 读不到副本 / 正文为空 ⇒ **不写**这条基线。宁可「没有一致点」（界面不显示），
    /// 也不臆造一个 —— 基线的全部意义就是「这两个内容曾经逐行相同」。
    fn record_baseline(&mut self, source_cid: &str, new_cid: &str, dst: &Path) {
        let Some(fingerprint) = baseline_fingerprint_of(dst, new_cid) else {
            return;
        };
        self.baselines.insert(
            baseline_key(source_cid, new_cid),
            baseline_value(&fingerprint),
        );
        self.baselines_dirty = true;
    }

    /// 基线表整批一次落盘；无新基线时视为成功（不写文件）。
    fn flush_baselines(&mut self) -> bool {
        if !self.baselines_dirty {
            return true;
        }
        let ok = save_session_baselines_at(&self.baselines_path, &self.baselines);
        if ok {
            self.baselines_dirty = false;
        }
        ok
    }

    /// **整批一次**备份（懒触发；已触发过直接复用同一路径）。
    ///
    /// 旧实现对**每个会话**各拍一份 `workbuddy.db`（含 -wal/-shm）—— N 个会话就是
    /// N 次整库拷贝，而它们拍的都是同一时刻之前的状态，纯浪费。现在整批只拍一次，
    /// 且仍然只在「第一个真正复制的会话」之前拍（没有会话要写库时完全不拍）。
    fn ensure_backup(&mut self) -> Option<PathBuf> {
        if !self.backup_attempted {
            self.backup_attempted = true;
            let root = backup_dir().join("sessions").join(utc_iso());
            match backup_workbuddy_db(self.target_region, &root) {
                Ok(path) => self.backup = path,
                Err(error) => self.backup_error = Some(error),
            }
        }
        self.backup.clone()
    }

    /// 源会话正文路径（走整批索引；索引懒建一次）。
    fn find_jsonl(&mut self, cid: &str) -> Option<PathBuf> {
        if self.projects_index.is_none() {
            self.projects_index = Some(build_projects_index_for(self.source_region));
        }
        self.projects_index
            .as_ref()
            .and_then(|index| index.get(cid).cloned())
    }
}

/// 去重命中的报告条目（与旧实现的独占返回逐字一致：`deduplicated: true`）。
fn dedup_report(cid: &str, existing: &str) -> Value {
    json!({
        "id": cid,
        "newId": existing,
        "jsonlCopied": false,
        "mappingWritten": false,
        "backup": Value::Null,
        "deduplicated": true,
    })
}

/// 从源行里取进度展示名（`custom_title` 优先，与 [`session_display_title`] 同优先级）。
pub(crate) fn row_title(cols: &[String], vals: &[rusqlite::types::Value]) -> String {
    let column = |name: &str| -> Option<String> {
        let index = cols.iter().position(|c| c == name)?;
        match vals.get(index)? {
            rusqlite::types::Value::Text(text) => Some(text.clone()),
            _ => None,
        }
    };
    session_display_title(column("title"), column("custom_title"))
}

/// phase A 的单条正文复制：`read → replace(cid → new_cid) → write`。
///
/// 线程安全：只碰本计划自己的两个文件，**无任何共享可变状态** —— 这是敢把
/// GB 级正文复制丢进线程池的前提。
fn copy_jsonl_for_plan(plan: &CopyPlan) -> bool {
    let (Some(src), Some(dst)) = (&plan.jsonl_src, &plan.jsonl_dst) else {
        return false;
    };
    if let Some(parent) = dst.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let Ok(text) = std::fs::read_to_string(src) else {
        return false;
    };
    // 替换 sessionId 等旧 id 引用（与 id 归一化规则同源）。
    let text = text.replace(&plan.cid, &plan.new_cid);
    std::fs::write(dst, text).is_ok()
}

/// 批量复制主管线（三段）。`session_ids` 的顺序即报告顺序；`progress` 可空。
///
/// 三段的分界即安全边界：phase 0 / B 是顺序段（db 读写都在这里，客户端此刻已关闭），
/// phase A 是并行段但只复制**互不相干的新文件**。详见模块文档。
fn run_copy_batch(
    source_region: Region,
    target_region: Region,
    source_uid: &str,
    target_uid: &str,
    session_ids: &[String],
    progress: Option<&(dyn Fn(&str) + Send + Sync)>,
) -> Value {
    let mut batch = CopyBatch::new(source_region, target_region, source_uid, target_uid);

    // ---- phase 0：顺序、只读（去重 / Claw / 读行 / 定路径）----
    let mut plans: Vec<CopyPlan> = Vec::new();
    // 批内已排上计划的源会话 → 新 id：同一 id 在一次批量里只排一次。
    // （旧实现靠「账本落盘后再读」撞对；批内记账更快，也不会因插行软失败而重复复制。）
    let mut planned: HashMap<&str, String> = HashMap::new();
    let mut skipped: Vec<Value> = Vec::new();
    let mut errors: Vec<Value> = Vec::new();
    for cid in session_ids {
        // ⓪ 副本识别（issue #147）：源会话本身是某次复制的产物 ⇒ 按血缘去重。
        //    没有这一步，B 的副本会在下一次切换时被当成「源会话」再复制一代，
        //    每切一次账号副本就翻一倍（现场实测 18 个真实会话堆成 300+ 个 jsonl）。
        if let Some((origin_uid, origin_cid)) = batch.copy_origin(cid) {
            let reason = if origin_uid == target_uid {
                Some("该会话是副本，原始会话属于目标账号")
            } else if batch.origin_already_copied(&origin_cid) {
                Some("该会话是副本，原始会话已复制到目标账号")
            } else {
                None
            };
            if let Some(reason) = reason {
                skipped.push(json!({
                    "id": cid,
                    "deduplicated": true,
                    "reason": reason,
                }));
                continue;
            }
        }
        // ① 账本去重（内存账本 + 副本存活判定）
        if let Some(existing) = batch.ledger_hit(cid) {
            skipped.push(dedup_report(cid, &existing));
            continue;
        }
        // ② 批内去重：同一 id 重复勾选 / 重复传入时，只复制第一个
        if let Some(existing) = planned.get(cid.as_str()) {
            skipped.push(dedup_report(cid, existing));
            continue;
        }
        // ③ 读源行（Claw 判定也在内）；源库不可用时降级从 jsonl 推断 cwd
        let row = match batch.read_session_row(cid) {
            Ok(row) => row,
            Err(reason) => {
                errors.push(json!({ "id": cid, "error": reason }));
                continue;
            }
        };
        if row.is_none() {
            let cwd = batch
                .find_jsonl(cid)
                .as_deref()
                .and_then(read_jsonl_meta)
                .map(|meta| meta.cwd)
                .unwrap_or_default();
            if !cwd.is_empty() && is_claw_workspace(&cwd) {
                errors.push(json!({
                    "id": cid,
                    "error": "Claw 工作区绑定当前账号渠道，不支持复制",
                }));
                continue;
            }
        }
        let new_cid = uuid::Uuid::new_v4().to_string();
        let title = row
            .as_ref()
            .map(|(cols, vals)| row_title(cols, vals))
            .unwrap_or_else(|| "(无标题)".to_string());
        let jsonl_src = batch.find_jsonl(cid);
        let jsonl_dst = jsonl_src
            .as_ref()
            .map(|src| target_jsonl_path_for(source_region, target_region, src, &new_cid));
        planned.insert(cid.as_str(), new_cid.clone());
        plans.push(CopyPlan {
            cid: cid.clone(),
            title,
            new_cid,
            row,
            jsonl_src,
            jsonl_dst,
        });
    }

    // ---- phase A：并行复制正文（纯文件；线程间零共享可变状态）----
    let total = plans.len();
    let mut jsonl_copied = vec![false; total];
    if total > 0 {
        if let Some(p) = progress {
            p(&format!("正在复制 {total} 个会话的聊天记录到目标账号…"));
        }
        let next = AtomicUsize::new(0);
        let done = AtomicUsize::new(0);
        let flags: Vec<AtomicBool> = (0..total).map(|_| AtomicBool::new(false)).collect();
        let plan_refs: Vec<&CopyPlan> = plans.iter().collect();
        let workers = total.min(MAX_JSONL_COPY_WORKERS);
        std::thread::scope(|scope| {
            for _ in 0..workers {
                let next = &next;
                let done = &done;
                let flags = &flags;
                let plan_refs = &plan_refs;
                scope.spawn(move || loop {
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    if index >= total {
                        break;
                    }
                    flags[index].store(copy_jsonl_for_plan(plan_refs[index]), Ordering::Relaxed);
                    let finished = done.fetch_add(1, Ordering::Relaxed) + 1;
                    if let Some(p) = progress {
                        p(&format!(
                            "正在复制会话（{finished}/{total}）：{}",
                            plan_refs[index].title
                        ));
                    }
                });
            }
        });
        for (index, flag) in flags.iter().enumerate() {
            jsonl_copied[index] = flag.load(Ordering::Relaxed);
        }
    }

    // ---- phase B：顺序写库（整批一次备份；共享连接逐会话插行）----
    let target_uid = batch.target_uid.clone();
    let mut copied: Vec<Value> = Vec::with_capacity(total);
    for (index, plan) in plans.iter().enumerate() {
        // 备份整批只做一次：在**第一个**真正复制的会话之前拍快照。
        let backup = batch.ensure_backup();
        let columns = batch.dst_columns();
        // ★ issue #146：源行缺失（全新账号 / 仅 jsonl 存在）时**合成**最小索引行。
        //   跳过写库 = 目标账号在客户端里永远「暂无任务」，且下次切换会把刚复制出的
        //   副本当成源会话再复制一遍（雪崩，见 #147）。
        let session_row = match (plan.row.as_ref(), batch.dst_conn()) {
            (Some((cols, vals)), Some(dst)) => {
                insert_session_row(dst, &columns, cols, vals, &plan.new_cid, &target_uid)
            }
            (None, Some(dst)) => {
                let (cols, vals) = synthesize_session_row(&columns, plan);
                insert_session_row(dst, &columns, &cols, &vals, &plan.new_cid, &target_uid)
            }
            (_, None) => Err("目标库不可用（workbuddy.db 不存在）".to_string()),
        };
        let session_row_written = session_row.is_ok();
        let mapping_written = match batch.edge_conn() {
            Some(edge) => insert_edge_sync_mapping_conn(edge, &plan.new_cid, &target_uid),
            None => false,
        };
        batch.ledger_record(&plan.cid, &plan.new_cid);

        // 降级警告优先级不变：备份失败最优先（关系还有没有回滚点），
        // 其次才是索引行 / 云端映射。
        let jsonl_ok = jsonl_copied[index];
        // ★ P14 S2：正文**真的**复制成功 ⇒ 记一条「上次一致点」。
        //   ⚠️ 只在 `jsonl_ok` 时记 —— 没复制出正文就没有「一致」可言（**不臆造**）。
        if jsonl_ok {
            if let Some(dst) = &plan.jsonl_dst {
                batch.record_baseline(&plan.cid, &plan.new_cid, dst);
            }
        }
        let warning = match &batch.backup_error {
            Some(error) => Some(format!(
                "复制前备份失败（{error}）；本次复制没有可回滚的备份点，请手动备份后重试"
            )),
            None if jsonl_ok && !session_row_written => Some(format!(
                "会话索引未写入（{}）；目标账号在客户端里看不到该会话",
                session_row
                    .as_ref()
                    .err()
                    .map(String::as_str)
                    .unwrap_or("原因未知")
            )),
            None if jsonl_ok && !mapping_written => {
                Some("云端映射注册失败；新会话暂时无法在 WorkBuddy 中显示云端历史".to_string())
            }
            None => None,
        };
        let mut report = json!({
            "id": plan.cid,
            "newId": plan.new_cid,
            "jsonlCopied": jsonl_ok,
            "sessionRowWritten": session_row_written,
            "mappingWritten": mapping_written,
            "backup": backup.map(|path| path.to_string_lossy().to_string()),
            "deduplicated": false,
            "ledgerWritten": false,
        });
        if let Some(warning) = warning {
            report["warning"] = json!(warning);
        }
        // ★ issue #146：失败原因必须可诊断，不能只留一句泛化的 warning。
        if let Err(error) = &session_row {
            report["sessionRowError"] = json!(error);
            report["failed"] = json!(true);
        }
        copied.push(report);
    }

    // 账本整批一次落盘（失败不致命：下次会重新复制）。
    if batch.flush_ledger() {
        for item in copied.iter_mut() {
            item["ledgerWritten"] = json!(true);
        }
    }
    // ★ P14 S2：基线表同样整批一次落盘（失败不致命：下次复制会重建）。
    batch.flush_baselines();

    // 会话复制备份的自动保留：本批（可能）刚拍了一份 `backups/sessions/<utc>`，
    // 超出 `SESSION_BACKUP_KEEP` 的旧份在这里清掉 —— 否则每批复制留一份、无界增长
    // （现场曾累积 3042 份 / 18GB）。显式清理入口见 `cache::clear`。
    crate::modules::cache::prune_session_backup_dirs(crate::modules::cache::SESSION_BACKUP_KEEP);

    let mut report = json!({
        "sourceUid": batch.source_uid,
        "targetUid": batch.target_uid,
        "sourceRegion": source_region.as_str(),
        "targetRegion": target_region.as_str(),
        "copied": copied,
    });
    if !skipped.is_empty() {
        report["skipped"] = json!(skipped);
    }
    if !errors.is_empty() {
        report["errors"] = json!(errors);
    }
    // ★ issue #146：索引行写失败时整批标记 failed —— 「报告成功但目标账号看不到」
    //   是本次事故最坏的一面，不能再让调用方从 `copied` 计数里猜。
    if copied
        .iter()
        .any(|item| item.get("sessionRowError").is_some())
    {
        report["failed"] = json!(true);
    }
    report
}

/// 会话正文副本的落点：保持「相对 projects 目录的子路径」不变，换到目标版本目录下。
///
/// 同版本时结果与原实现完全一致（同一 workspace 目录、只换文件名）；
/// 跨版本时把正文搬到 `target_region` 的 projects，否则副本写进源版本目录、
/// 目标版本的 WorkBuddy 根本看不到它。
fn target_jsonl_path_for(
    source_region: Region,
    target_region: Region,
    src_jsonl: &Path,
    new_cid: &str,
) -> PathBuf {
    let source_projects = session_data_dir(source_region).join("projects");
    let target_projects = session_data_dir(target_region).join("projects");
    let in_target = match src_jsonl.strip_prefix(&source_projects) {
        Ok(rel) => target_projects.join(rel),
        Err(_) => target_projects,
    };
    in_target.with_file_name(format!("{new_cid}.jsonl"))
}

/// 表的列名清单（按 PRAGMA 顺序）；表不存在时为空。
pub(crate) fn table_columns(conn: &Connection, table: &str) -> Vec<String> {
    let Ok(mut stmt) = conn.prepare(&format!("PRAGMA table_info({table})")) else {
        return Vec::new();
    };
    let Ok(iter) = stmt.query_map([], |row| row.get::<_, String>(1)) else {
        return Vec::new();
    };
    iter.flatten().collect()
}

/// 读出源会话行的列名与值；顺带做 Claw 判定。
///
/// 源库不存在 / 无 sessions 表 / 无此行 → `Ok(None)`（对照 Python 版静默跳过）。
///
/// 入参是**连接**而不是路径：批量管线的 phase 0 对每个会话都要读行，
/// 连接由调用方（[`CopyBatch`]）整批复用 —— 旧实现每个会话各开一次库。
pub(crate) fn read_session_row(
    conn: &Connection,
    cid: &str,
    source_uid: &str,
) -> Result<Option<SessionRow>, String> {
    if !table_exists(conn, "sessions") {
        return Ok(None);
    }
    let mut stmt = conn
        .prepare("SELECT * FROM sessions WHERE id = ?1 AND user_id = ?2")
        .map_err(|e| e.to_string())?;
    let cols: Vec<String> = stmt.column_names().iter().map(|s| s.to_string()).collect();
    let mut rows = stmt
        .query(rusqlite::params![cid, source_uid])
        .map_err(|e| e.to_string())?;
    let Some(row) = rows.next().map_err(|e| e.to_string())? else {
        return Ok(None);
    };
    let mut vals: Vec<rusqlite::types::Value> = Vec::with_capacity(cols.len());
    for (i, col) in cols.iter().enumerate() {
        let v = row
            .get::<_, rusqlite::types::Value>(i)
            .unwrap_or(rusqlite::types::Value::Null);
        if col == "cwd" {
            if let rusqlite::types::Value::Text(ref path) = v {
                if is_claw_workspace(path) {
                    return Err("Claw 工作区绑定当前账号渠道，不支持复制".into());
                }
            }
        }
        vals.push(v);
    }
    Ok(Some((cols, vals)))
}

/// 源会话行缺失时合成的最小索引行（issue #146）。
///
/// ## 为什么不能「跳过写库」
///
/// 全新账号在 `sessions` 表里 0 行 ⇒ 会话列表退回 jsonl 反扫（`degraded: true`），
/// 这些 cid 在库里**属于别的账号或根本不存在** ⇒ [`read_session_row`] 必然返回 `None`。
/// 旧实现在这里直接不写索引行，只留一条「请重启 WorkBuddy 让其重建索引」的 warning ——
/// 而客户端**不会**从 jsonl 重建索引，于是目标账号永远显示「暂无任务」，
/// 且下次切换会把刚复制出的副本当成源会话再复制一遍（雪崩，见 #147）。
///
/// 合成行只填客户端要求的非空列（`id` / `user_id` / `cwd` / `status` / `transport` /
/// 两个时间戳）；`id` 与 `user_id` 由 [`insert_session_row`] 统一覆盖，这里给占位值。
/// 目标库缺的列直接不出现，其余列交给库默认值 —— 真失败由写后回读报错，不再静默。
fn synthesize_session_row(dst_cols: &[String], plan: &CopyPlan) -> SessionRow {
    let meta = plan.jsonl_src.as_deref().and_then(read_jsonl_meta);
    let cwd = meta
        .as_ref()
        .map(|meta| meta.cwd.clone())
        .unwrap_or_default();
    // 标题与侧栏同口径：jsonl 里的（customTitle > aiTitle）优先，退回 phase 0 算出的展示名。
    let title = meta
        .as_ref()
        .map(|meta| meta.title.clone())
        .filter(|title| !title.trim().is_empty())
        .unwrap_or_else(|| plan.title.clone());
    let now = now_ms();
    let candidates: Vec<(&str, rusqlite::types::Value)> = vec![
        ("id", rusqlite::types::Value::Text(String::new())),
        ("user_id", rusqlite::types::Value::Text(String::new())),
        ("cwd", rusqlite::types::Value::Text(cwd)),
        ("title", rusqlite::types::Value::Text(title)),
        (
            "status",
            rusqlite::types::Value::Text("completed".to_string()),
        ),
        (
            "transport",
            rusqlite::types::Value::Text("local".to_string()),
        ),
        ("created_at", rusqlite::types::Value::Integer(now)),
        ("updated_at", rusqlite::types::Value::Integer(now)),
    ];
    let mut cols = Vec::with_capacity(candidates.len());
    let mut vals = Vec::with_capacity(candidates.len());
    for (col, value) in candidates {
        if !dst_cols.iter().any(|existing| existing == col) {
            continue;
        }
        cols.push(col.to_string());
        vals.push(value);
    }
    (cols, vals)
}

/// 把**已读出的源行**写入目标库（动态列，覆盖 id/user_id/时间戳）。
///
/// 源与目标可以是两个版本的 db。目标库缺某一列时**丢弃该列**而不是整条失败 ——
/// 两版 schema 高度同源但允许有差异，丢弃比让整次复制失败更接近用户预期。
///
/// ## 返回值语义（issue #146）
///
/// `Ok(())` = 目标库**回读确认**该行存在且 `user_id` 就是目标账号；
/// `Err(原因)` = 没写成 / 写歪了。旧实现返回 `bool` 并把 `rusqlite::Error` 整个丢掉，
/// 调用方无法区分「表不存在」「NOT NULL 违约」「归属被改」—— 于是只能笼统地报
/// 「索引不可写，请重启客户端」，把可诊断的失败变成了玄学。
///
/// ★ 必须回读：`execute` 成功只代表语句没报错，触发器 / 约束 / 主键冲突都可能让行
/// 落到别处或根本不存在，而「报告成功但目标账号看不到」正是本次事故的根因。
/// Claw 拒绝发生在**读行**阶段（[`read_session_row`]），与本函数无关。
pub(crate) fn insert_session_row(
    dst: &Connection,
    dst_cols: &[String],
    cols: &[String],
    vals: &[rusqlite::types::Value],
    new_cid: &str,
    target_uid: &str,
) -> Result<(), String> {
    if !table_exists(dst, "sessions") {
        return Err("目标库没有 sessions 表".to_string());
    }
    let mut insert_cols: Vec<&str> = Vec::with_capacity(cols.len());
    let mut insert_vals: Vec<rusqlite::types::Value> = Vec::with_capacity(vals.len());
    for (col, v) in cols.iter().zip(vals) {
        if !dst_cols.iter().any(|c| c == col) {
            continue;
        }
        let v = match col.as_str() {
            "id" => rusqlite::types::Value::Text(new_cid.to_string()),
            "user_id" => rusqlite::types::Value::Text(target_uid.to_string()),
            "created_at" | "updated_at" => rusqlite::types::Value::Integer(now_ms()),
            "deleted_at" => rusqlite::types::Value::Null,
            _ => v.clone(),
        };
        insert_cols.push(col.as_str());
        insert_vals.push(v);
    }
    if insert_cols.is_empty() {
        return Err("目标库 sessions 表没有可写列".to_string());
    }

    let placeholders = insert_cols
        .iter()
        .map(|_| "?")
        .collect::<Vec<_>>()
        .join(", ");
    let colnames = insert_cols.join(", ");
    let sql = format!("INSERT OR REPLACE INTO sessions ({colnames}) VALUES ({placeholders})");
    let params: Vec<&rusqlite::types::Value> = insert_vals.iter().collect();
    dst.execute(&sql, rusqlite::params_from_iter(params))
        .map_err(|e| format!("写入会话索引失败：{e}"))?;

    // ★ 写后回读：确认这一行真的属于目标账号（issue #146 的验收口径）。
    let owner = dst
        .query_row(
            "SELECT user_id FROM sessions WHERE id = ?1",
            rusqlite::params![new_cid],
            |row| row.get::<_, Option<String>>(0),
        )
        .optional()
        .map_err(|e| format!("回读会话索引失败：{e}"))?;
    match owner.flatten() {
        Some(uid) if uid == target_uid => Ok(()),
        Some(uid) => Err(format!("会话索引归属不是目标账号（{uid}）")),
        None => Err("会话索引写入后回读不到该行".to_string()),
    }
}

/// 按**路径**的插入入口（`#[cfg(test)]`：只服务单测）。
///
/// 生产路径一律走 [`run_copy_batch`] 的三段管线（连接整批复用），这里留一份
/// 「开库 → 读行 → 插行」的薄包装，让行变换逻辑可以被**不经整条管线**地直接断言
/// （动态列丢弃 / id 覆盖 / deleted_at 置空 / 跨库不写脏源库）。
#[cfg(test)]
fn insert_session_copy(
    src_db_path: &Path,
    dst_db_path: &Path,
    new_cid: &str,
    cid: &str,
    source_uid: &str,
    target_uid: &str,
) -> Result<bool, String> {
    if !src_db_path.is_file() {
        return Ok(false);
    }
    let Some(src) = open_db(src_db_path, true) else {
        return Ok(false);
    };
    let Some((cols, vals)) = read_session_row(&src, cid, source_uid)? else {
        return Ok(false);
    };
    if !dst_db_path.is_file() {
        return Ok(false);
    }
    let Some(dst) = open_db(dst_db_path, false) else {
        return Ok(false);
    };
    let dst_cols = table_columns(&dst, "sessions");
    insert_session_row(&dst, &dst_cols, &cols, &vals, new_cid, target_uid).map(|()| true)
}

/// 把新会话注册进 edge_sync_mapping（云端归属关键）。失败不致命，返回 False。
///
/// 连接版：批量管线整批只开一次边车库（见 [`CopyBatch::edge_conn`]）；
/// 路径版留给单测与按路径调用方。**表不存在 → false，绝不新建空库**。
fn insert_edge_sync_mapping_conn(conn: &Connection, new_cid: &str, target_uid: &str) -> bool {
    if !table_exists(conn, "edge_sync_mapping") {
        return false;
    }
    let created_at = now_secs();
    conn.execute(
        "INSERT OR REPLACE INTO edge_sync_mapping \
         (session_id, conversation_id, msg_channel, created_at) VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![
            new_cid,
            new_cid,
            format!("convmsg:{target_uid}"),
            created_at
        ],
    )
    .is_ok()
}

/// 把新会话注册进 edge_sync_mapping 的**路径版**（`#[cfg(test)]`：只服务单测）。
///
/// 生产路径走 [`CopyBatch::edge_conn`] + [`insert_edge_sync_mapping_conn`]（整批一次连接）。
#[cfg(test)]
fn insert_edge_sync_mapping(db_path: &Path, new_cid: &str, target_uid: &str) -> bool {
    if !db_path.is_file() {
        return false;
    }
    let Some(conn) = open_db(db_path, false) else {
        return false;
    };
    insert_edge_sync_mapping_conn(&conn, new_cid, target_uid)
}

// ---------------------------------------------------------------------------
// 复制去重账本（同一源会话 → 同一目标账号只复制一次）
// ---------------------------------------------------------------------------

/// 复制去重账本相对于 session 数据目录的文件名。
///
/// 存放形如 `{ "source_uid\u{1f}target_uid\u{1f}source_cid": new_cid }` 的映射。
const COPY_LEDGER_FILE: &str = "buddy-switch-copy-ledger.json";

/// 复合键分隔符（Unit Separator；正常 uid / uuid 不会包含）。
const LEDGER_SEP: char = '\u{1f}';

fn copy_ledger_path_for(region: Region) -> PathBuf {
    session_data_dir(region).join(COPY_LEDGER_FILE)
}

// ---------------------------------------------------------------------------
// P14 S2 · 基线对（「上次一致点」）
// ---------------------------------------------------------------------------

/// 基线文件的文件名（region 由**目录**分家，与 [`COPY_LEDGER_FILE`] 同款）。
///
/// ⚠️ 与账本**分开两个文件**：账本是「谁复制过谁」的**去重机制**（改它会静默改变
/// P8 行为），基线是「上次一致到第几条」的**附属信息**。混在一起会让「清基线」
/// 这种无害操作变成「清去重记录」——那是不可逆的。
const SESSION_BASELINE_FILE: &str = "buddy-switch-session-baselines.json";

/// 基线键的分隔符（Record Separator）。
///
/// ⚠️ **必须与顶点键内部的 `\u{1f}` 不同**，否则解析键时会与顶点自带的分隔符混淆。
const BASELINE_SEP: char = '\u{1e}';

/// 一对副本的「**上次一致点**」—— 内容身份的**稳定**快照。
///
/// ## 为什么可以落盘（与「本模块刻意不用 sha2」不矛盾）
///
/// [`SessionFingerprint`] 的 hash 是 FNV-1a：**纯函数**，同一份字节在任何进程 /
/// 任何版本里都得到同一个值 ⇒ 落盘安全。参考实现之所以改用 `sha2`，是因为它用的是
/// `DefaultHasher`（那个才跨版本不稳）。**本仓没有那个问题，所以不引入 `sha2` 依赖。**
///
/// ## 与 `prefixHashes` 上限的关系
///
/// 逐行 hash 只取**前 [`PREFIX_HASH_LIMIT`] 行**（与 [`fingerprint_of_file`] 同一口径）。
/// 超长会话的「一致点」在 512 行之后**不可分辨** —— 这是本仓既有取舍，基线**沿用**它，
/// 不偷偷放大（两套上限并存会让「共同旧版」与「上次一致点」在不同尺度上打架）。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PairBaseline {
    /// 建立时刻（ms）。
    pub created_at: i64,
    /// 一致点的行数。
    pub line_count: usize,
    /// 前 N 行的逐行 hash（见结构体文档）。
    pub prefix_hashes: Vec<u64>,
}

fn session_baseline_path_for(region: Region) -> PathBuf {
    session_data_dir(region).join(SESSION_BASELINE_FILE)
}

/// 基线键：两个会话 id 的**有序**拼接（`a < b`）⇒ 与「谁复制给谁」的方向无关。
///
/// ⚠️ 用 **cid** 而不是 `uid␟cid`：建立基线时手边只有 cid（`unify` 路径拿不到 uid），
/// 而 cid 是 UUID ⇒ 同一 region 内不会撞。顶点键是给**组连通**用的，两者键空间不同，
/// **不要互相套用**。
fn baseline_key(a: &str, b: &str) -> String {
    let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
    format!("{lo}{BASELINE_SEP}{hi}")
}

/// 读取基线表；文件缺失 / 损坏 / 结构不符 ⇒ 空表（视为**没有基线**，**不臆造**）。
fn load_session_baselines_at(path: &Path) -> serde_json::Map<String, Value> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return serde_json::Map::new();
    };
    match serde_json::from_str::<Value>(&text) {
        Ok(Value::Object(map)) => map,
        _ => serde_json::Map::new(),
    }
}

/// 原子写回基线表。失败不致命（下次复制会重建），返回是否成功。
fn save_session_baselines_at(path: &Path, baselines: &serde_json::Map<String, Value>) -> bool {
    if let Some(parent) = path.parent() {
        if std::fs::create_dir_all(parent).is_err() {
            return false;
        }
    }
    let content =
        serde_json::to_string_pretty(&Value::Object(baselines.clone())).unwrap_or_default();
    crate::modules::config::atomic_write(path, &content).is_ok()
}

/// 读一份**可用于基线**的指纹：读不到 / 正文为空 ⇒ `None`（**不建**一个空的一致点）。
fn baseline_fingerprint_of(path: &Path, cid: &str) -> Option<SessionFingerprint> {
    let fingerprint = fingerprint_of_file(path, cid)?;
    (fingerprint.line_count > 0).then_some(fingerprint)
}

/// 一条基线的 JSON 形态 —— **两条写入路径共用**（`CopyBatch` 的复制、`unify` 的覆盖）。
///
/// 抽出来的理由：两条路径若各写一份字段，迟早一边多一个 `sourceUid`、一边少一个
/// `lineCount`，而「上次一致点」在两个来源上就**不可比**了。
fn baseline_value(fingerprint: &SessionFingerprint) -> Value {
    json!({
        "createdAt": now_ms(),
        "lineCount": fingerprint.line_count,
        "prefixHashes": fingerprint.prefix_hashes,
    })
}

/// ★ P14 S2：**分歧统一**成功（真的覆盖了）⇒ 记一条基线。
///
/// 与 [`CopyBatch::record_baseline`] **同口径**（都走 [`baseline_value`]）：
/// 覆盖之后两份内容逐行一致 ⇒ 这一刻就是新的「上次一致点」。
///
/// ⚠️ 只在 `overwritten == true` 时调用 —— 「本来就一致」的那条分支**没写盘**，
/// 不是一次新的会合，不该刷新一致点（否则会把旧的、更早的一致点覆盖掉，
/// 反而丢掉「他们从什么时候开始分叉」这条信息）。
fn record_pair_baseline_for(region: Region, source_cid: &str, target_cid: &str, dst: &Path) {
    let Some(fingerprint) = baseline_fingerprint_of(dst, target_cid) else {
        return;
    };
    let path = session_baseline_path_for(region);
    let mut baselines = load_session_baselines_at(&path);
    baselines.insert(
        baseline_key(source_cid, target_cid),
        baseline_value(&fingerprint),
    );
    save_session_baselines_at(&path, &baselines);
}

/// 解析账本路径：仅用于单测把读写重定向到临时目录，**普通调用一律传 `None`**。
fn resolve_ledger_path(region: Region, override_path: Option<&Path>) -> PathBuf {
    match override_path {
        Some(p) => p.to_path_buf(),
        None => copy_ledger_path_for(region),
    }
}

/// 构造账本复合键：`source_uid ␟ target_uid ␟ source_cid`。
///
/// 三元组缺一不可：同一源会话复制给不同目标账号是两次独立操作；
/// 同一目标账号从不同源账号复制同名 cid 也应各自成条。
fn copy_ledger_key(source_uid: &str, target_uid: &str, source_cid: &str) -> String {
    format!("{source_uid}{LEDGER_SEP}{target_uid}{LEDGER_SEP}{source_cid}")
}

/// 跨版本复制的账本键：在三元键前加**源 region** 前缀。
///
/// 同版本刻意保持原键 —— 否则既有账本全部失配，老用户会被判成「没复制过」而重复复制；
/// 跨版本必须加前缀 —— 否则「CN 的 uid-a 复制给 X」与「Global 的 uid-a 复制给 X」
/// 会共用一条登记，其中一侧被错误跳过。
fn copy_ledger_key_for(
    source_region: Region,
    target_region: Region,
    source_uid: &str,
    target_uid: &str,
    source_cid: &str,
) -> String {
    let base = copy_ledger_key(source_uid, target_uid, source_cid);
    if source_region == target_region {
        base
    } else {
        format!("{}{LEDGER_SEP}{base}", source_region.as_str())
    }
}

/// 账本值里的字段名（issue #147：值从裸 `new_cid` 升级为带血缘的对象）。
///
/// 旧值是字符串 `"<new_cid>"`，新值是
/// `{"new": "<new_cid>", "originUid": "<原始账号>", "originCid": "<原始会话>"}`。
/// 两种都要能读 —— 用户升级前的账本不能作废（否则所有会话被重新复制一遍）。
const LEDGER_NEW: &str = "new";
const LEDGER_ORIGIN_UID: &str = "originUid";
const LEDGER_ORIGIN_CID: &str = "originCid";

/// 从账本值取副本 id：兼容旧格式（裸字符串）与新格式（对象）。
fn ledger_new_of(value: &Value) -> Option<String> {
    let new = match value {
        Value::String(text) => Some(text.as_str()),
        Value::Object(map) => map.get(LEDGER_NEW).and_then(Value::as_str),
        _ => None,
    }?;
    let trimmed = new.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

/// 从账本值取血缘 `(origin_uid, origin_cid)`；旧格式（无血缘）返回 `None`。
fn ledger_origin_of(value: &Value) -> Option<(String, String)> {
    let map = value.as_object()?;
    let uid = map.get(LEDGER_ORIGIN_UID)?.as_str()?.trim();
    let cid = map.get(LEDGER_ORIGIN_CID)?.as_str()?.trim();
    (!uid.is_empty() && !cid.is_empty()).then(|| (uid.to_string(), cid.to_string()))
}

/// 从账本键里取 `target_uid`（键形如 `[region␟]source␟target␟cid`）。
fn ledger_key_target(key: &str) -> Option<String> {
    let parts: Vec<&str> = key.split(LEDGER_SEP).collect();
    match parts.as_slice() {
        [_, target, _] => Some((*target).to_string()),
        [_, _, target, _] => Some((*target).to_string()),
        _ => None,
    }
}

/// 从账本构建两个血缘索引（issue #147）。
///
/// - `copy_origins`：副本 id → `(origin_uid, origin_cid)` —— 回答「这个 cid 是哪个原始会话的副本」；
/// - `origin_copied`：`(origin_cid, target_uid)` —— 回答「该原始会话是否已复制给目标账号」。
///
/// 旧格式条目（值是裸字符串）没有血缘 ⇒ 只能当「它不是副本」，两个索引里都不出现。
fn build_origin_indexes(
    ledger: &serde_json::Map<String, Value>,
) -> (HashMap<String, (String, String)>, HashSet<(String, String)>) {
    let mut copy_origins = HashMap::new();
    let mut origin_copied = HashSet::new();
    for (key, value) in ledger {
        let Some(new_cid) = ledger_new_of(value) else {
            continue;
        };
        let Some(origin) = ledger_origin_of(value) else {
            continue;
        };
        if let Some(target) = ledger_key_target(key) {
            origin_copied.insert((origin.1.clone(), target));
        }
        copy_origins.insert(new_cid, origin);
    }
    (copy_origins, origin_copied)
}

/// 读取复制账本；文件缺失 / 损坏 / 结构不符时返回空表（视为无登记）。
fn load_copy_ledger_at(path: &Path) -> serde_json::Map<String, Value> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return serde_json::Map::new();
    };
    match serde_json::from_str::<Value>(&text) {
        Ok(Value::Object(map)) => map,
        _ => serde_json::Map::new(),
    }
}

/// 原子写回复制账本。写入失败不致命（下次会重新复制），返回是否成功。
fn save_copy_ledger_at(path: &Path, ledger: &serde_json::Map<String, Value>) -> bool {
    if let Some(parent) = path.parent() {
        if std::fs::create_dir_all(parent).is_err() {
            return false;
        }
    }
    let content = serde_json::to_string_pretty(&Value::Object(ledger.clone())).unwrap_or_default();
    crate::modules::config::atomic_write(path, &content).is_ok()
}

/// 该账号名下是否存在未删除的指定会话（db 为准；**连接由调用方给**）。
///
/// 连接版供批量管线整批复用（[`CopyBatch::dst_conn`]）—— 旧实现每次判定都重开一次库。
fn session_exists_with(conn: &Connection, cid: &str, uid: &str) -> bool {
    if !table_exists(conn, "sessions") {
        return false;
    }
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sessions \
         WHERE id = ?1 AND user_id = ?2 AND deleted_at IS NULL)",
        rusqlite::params![cid, uid],
        |r| r.get::<_, i64>(0),
    )
    .map(|n| n != 0)
    .unwrap_or(false)
}

/// 在账本中登记一次成功的复制（`#[cfg(test)]`：只服务单测按路径重定向）。
///
/// ⚠️ 生产路径走 [`CopyBatch::ledger_record`] + [`CopyBatch::flush_ledger`]（整批一次落盘），
/// 本函数只服务「单测把账本读写重定向到临时目录」的场景；改去重语义前先读
/// [`list_replica_groups_for`] 头部那段警告。
#[cfg(test)]
fn record_copy_ledger_at(
    source_region: Region,
    target_region: Region,
    override_path: Option<&Path>,
    source_uid: &str,
    target_uid: &str,
    source_cid: &str,
    new_cid: &str,
) -> bool {
    let path = resolve_ledger_path(target_region, override_path);
    let mut ledger = load_copy_ledger_at(&path);
    ledger.insert(
        copy_ledger_key_for(
            source_region,
            target_region,
            source_uid,
            target_uid,
            source_cid,
        ),
        Value::String(new_cid.to_string()),
    );
    save_copy_ledger_at(&path, &ledger)
}

/// 副本组的顶点标识：`uid␟cid`（仅用于本次连通计算，**不外泄**）。
fn replica_vertex_key(uid: &str, cid: &str) -> String {
    format!("{uid}{LEDGER_SEP}{cid}")
}

/// ★ P14 S1：**组结构指纹**（乐观令牌的本体）—— 只由「组里有哪些副本」决定。
///
/// ## 它挡的是什么
///
/// 用户看到的副本组是**某一时刻**的账本快照。若在用户点「同步 / 统一」之前账本又变了
/// （复制到新账号 ⇒ 组里多一个成员），前端算出来的任务清单就是**对着旧组**算的：
/// 源副本的选取、要覆盖的目标集合都可能已经不对。⇒ **指纹不符即拒绝提交，让用户刷新**。
///
/// ## 三条设计约束
///
/// 1. **顺序无关**：账本的插入顺序不该改变指纹（调用方可能从任意顺序的 map 迭代出来）；
/// 2. **不读 jsonl**：只吃「成员顶点」这一份**已经在手**的数据 —— [`replica_groups_from_ledger`]
///    是**纯函数**（无 IO），指纹必须保持这一点，否则它的可测性就没了；
/// 3. **不落盘**：与参考实现不同（那边把令牌写成 `previews/{id}.json`，为的是跨
///    「预览 → 确认弹窗 → 提交」三段存活）。本仓的提交是**同进程内的连续动作**，且后端
///    **本来就会独立复核内容**（[`append_delta_between_files`] 的 `common < shorter` 拒绝）
///    ⇒ 落盘是过度设计。令牌随 `list_session_replicas` 的响应返回、由前端原样带回。
///
/// ## ⚠️ 这是「陈旧检测」，不是「安全边界」
///
/// 64 位 FNV 可以被构造碰撞，但伪造令牌在这里**换不到任何东西** —— 真正的写入门禁是
/// 后端对**内容**的独立复核（见 [`append_delta_between_files`] / [`unify_replica_between_files`]）。
/// **不要**把它当鉴权用，也不要因为它可碰撞就认为它没用：它挡的是**误操作**，不是攻击者。
///
/// 返回 16 位小写十六进制（稳定 ⇒ 可直接进 JSON、可当 React key 的一部分）。
pub fn group_fingerprint(members: &[(String, String)]) -> String {
    let mut keys: Vec<String> = members
        .iter()
        .map(|(uid, cid)| replica_vertex_key(uid, cid))
        .collect();
    keys.sort();
    keys.dedup();

    let mut state = fnv1a(&(keys.len() as u64).to_be_bytes(), FNV_OFFSET_BASIS);
    for key in &keys {
        // ★ 长度前缀：否则 ["ab","c"] 与 ["a","bc"] 会拼成同一串字节（撞指纹）。
        state = fnv1a(&(key.len() as u64).to_be_bytes(), state);
        state = fnv1a(key.as_bytes(), state);
    }
    format!("{state:016x}")
}

/// 把账本键解析回**同 region** 的 `(source_uid, target_uid, source_cid)`。
///
/// **只接受三元键**（同 region 复制）。跨 region 的四元键（带源 region 前缀，见
/// [`copy_ledger_key_for`]）本期**不引入**（P8 ✅ #2）⇒ 返回 `None`。
///
/// ⚠️ 键格式的定义在 [`copy_ledger_key`] / [`copy_ledger_key_for`] —— 解析与构造必须
/// **同处一地维护**（这正是下面 [`list_replica_groups_for`] 函数头那条注释要守的同一件事）。
fn parse_copy_ledger_key_same_region(key: &str) -> Option<(String, String, String)> {
    let mut parts = key.split(LEDGER_SEP);
    let source_uid = parts.next()?;
    let target_uid = parts.next()?;
    let source_cid = parts.next()?;
    // 还有第 4 段 ⇒ 跨 region 四元键 ⇒ 本期不引入。
    if parts.next().is_some() {
        return None;
    }
    if source_uid.is_empty() || target_uid.is_empty() || source_cid.is_empty() {
        return None;
    }
    Some((
        source_uid.to_string(),
        target_uid.to_string(),
        source_cid.to_string(),
    ))
}

/// ★ 只读：把 copy ledger 的边连通成「会话副本组」（P8 T02）。
///
/// ⚠️ **这是一个内部去重账本被提升为产品语义；改动去重逻辑前先看这里。**
///    ledger 本是「同一源会话 → 同一目标账号只复制一次」的去重机制（[`CopyBatch::ledger_hit`] /
///    [`CopyBatch::ledger_record`]），P8 把它当作「副本边」的数据源暴露给前端。**优化 / 重构去重
///    会静默改变 P8 的行为**（那是最难查的一类缺陷）—— 改前先读本注释，并跑
///    `replica_groups_*` 的单测。
///
/// ## 数据源与范围
/// - 数据源 = `buddy-switch-copy-ledger.json`（[`COPY_LEDGER_FILE`]），**只读**（绝不写回）。
/// - **只读当前 region** 的账本（P8 ✅ #2）：跨 region 的四元键**不引入**（语义不清）。
/// - **不改 ledger 结构、不新增模型** —— 只是把既有边读出来连通。
///
/// ## 「组」的定义
/// 把 `(source_uid, source_cid)` 与 `(target_uid, new_cid)` 当作图的顶点、账本条当作边，
/// **连通即一组**（连通分量）。返回值**只含 ledger 里确有的字段**（`accountUid` /
/// `sessionId` / `region`）；`title` / `updatedAt` / `hasHistory` **不在 ledger 里**
/// ⇒ 由**前端**用 `list_sessions` / 账号 store join 补齐，**后端不臆造**。
///
/// ## ⚠️ 返回值无内容信息
/// 值是 `new_cid`（字符串，无消息计数 / base）⇒ 前端**推不出**「落后 / 分叉」
/// （§3.15.2「时间新 ≠ 内容新」）。本命令**不得**被用来派生组状态。
///
/// 返回：
/// ```json
/// { "groups": [ { "key": "…", "replicas": [ { "accountUid": "…", "sessionId": "…", "region": "cn" } ] } ],
///   "source": "ledger" | "empty" }
/// ```
pub fn list_replica_groups_for(region: Region) -> Value {
    let (entries, source) = replica_group_entries(region, None);
    if entries.is_empty() {
        // 无可用登记（文件缺失 / 空 / 只有跨 region 边）⇒ 前端渲染结构化空态。
        // ⚠️ **此时不动缓存**：账本损坏读到空时剪枝 / 落盘会把好端端的一份缓存毁掉。
        return json!({ "groups": [], "source": "empty" });
    }
    // ★ 内容状态：**在这里算完**，只把每组几个整数回给前端（见 [`derive_content_status`]）。
    //   `projects/` 索引只建一次 —— 逐副本重扫目录是「加载很久」的第二大来源。
    let projects = build_projects_index_for(region);
    let mut store = fingerprint_store().lock().unwrap();
    let mut groups: Vec<Value> = Vec::with_capacity(entries.len());
    // 本次真的请求过的键 ⇒ 用来剪掉「会话已被删掉」的陈旧条目（缓存不无限长）。
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    for entry in &entries {
        let fingerprints = fingerprints_for_members(&mut store, region, &projects, &entry.members);
        for (_uid, cid) in &entry.members {
            seen.insert(fingerprint_key(region, cid));
        }
        let baseline = newest_baseline(&entry.baselines);
        let mut json = entry.json.clone();
        // ★ issue #141：超窗口组的前缀验证。**只有**「最短副本 > 512 行且行数不等」时才会
        //   真的读盘，且只读到第 N 行就停（不是读全文）；算出来的摘要记进探针位随缓存落盘
        //   ⇒ 同一对文件下次开页**零 IO**（`FingerprintStore` 的「命中不读盘」不被破坏）。
        let mut probe = |index: usize, offset: usize| -> Option<u64> {
            let fingerprint = fingerprints.get(index)?.as_ref()?;
            if offset >= fingerprint.line_count {
                return Some(fingerprint.full_hash);
            }
            let (_uid, cid) = entry.members.get(index)?;
            let path = projects.get(cid)?;
            let key = fingerprint_key(region, cid);
            if let Some(digest) = store.probe(&key, offset) {
                return Some(digest);
            }
            let digest = prefix_digest_of_file(path, cid, offset)?;
            store.record_probe(&key, offset, digest);
            Some(digest)
        };
        if let Some(object) = json.as_object_mut() {
            object.insert(
                "content".to_string(),
                derive_content_status(&entry.members, &fingerprints, baseline, &mut probe),
            );
        }
        groups.push(json);
    }
    // ★ 落盘：全命中时 `dirty` 为假 ⇒ **一个字节都不写**（见 [`FingerprintStore`] 的说明）。
    //   载入失败 / 写失败都不影响本次读数（缓存只是加速器）。
    store.prune_region(region, &seen);
    let cache_path = fingerprint_cache_path();
    store.save_if_worthwhile(&cache_path);
    json!({ "groups": groups, "source": source })
}

/// [`list_replica_groups_for`] 的**纯结构**版本（**不读 jsonl**，仅单测重定向账本用）。
///
/// ⚠️ 与 [`list_replica_groups_for`] 的差别只有「有没有 `content`」这一块 ——
/// 这是刻意的：单测必须能在**不碰真实 home** 的前提下断言组结构与「上次一致点」的归属，
/// 而内容状态天然要读盘（走 [`replica_group_entries`] + 显式传入的 projects 根）。
#[cfg(test)]
fn list_replica_groups_at(region: Region, override_path: Option<&Path>) -> Value {
    let (entries, source) = replica_group_entries(region, override_path);
    let groups: Vec<Value> = entries.into_iter().map(|entry| entry.json).collect();
    if groups.is_empty() {
        json!({ "groups": [], "source": "empty" })
    } else {
        json!({ "groups": groups, "source": source })
    }
}

/// 一个副本组的**内部形态**：结构（`json`，给前端）+ 成员顶点（给内容状态）。
struct ReplicaGroupEntry {
    /// 成员顶点 `(uid, cid)`，已排序、已去重。
    members: Vec<(String, String)>,
    /// 该组在组内命中的基线（两端成员都在组内），**已按 `createdAt` 升序**。
    baselines: Vec<GroupBaseline>,
    /// 出给前端的 JSON（`key` / `replicas` / `previewToken` / `baseline`）。
    /// ⚠️ **不含逐行 hash** —— 那是 200 MB 级的历史包袱（见 [`GroupContentStatus`]）。
    json: Value,
}

/// 账本 + 基线表 → 组（**唯一**的组结构来源，纯函数；`override_path` 只服务单测）。
fn replica_group_entries(
    region: Region,
    override_path: Option<&Path>,
) -> (Vec<ReplicaGroupEntry>, &'static str) {
    let path = resolve_ledger_path(region, override_path);
    let ledger = load_copy_ledger_at(&path);
    // ★ P14 S2：基线表与账本**同目录**（`session_baseline_path_for` 也是
    //   `session_data_dir(region).join(…)`）⇒ 给了账本重定向时基线也跟着重定向，
    //   单测**不会摸到真实 home**（这是本仓踩过四次的坑）。
    let baselines_path = match override_path {
        Some(path) => path.with_file_name(SESSION_BASELINE_FILE),
        None => session_baseline_path_for(region),
    };
    let baselines = load_session_baselines_at(&baselines_path);
    let entries = replica_groups_from_ledger(region, &ledger, &parse_group_baselines(&baselines));
    let source = if entries.is_empty() {
        "empty"
    } else {
        "ledger"
    };
    (entries, source)
}

/// 把账本（`键 → new_cid`）连通成 `(组键, 成员顶点)`；输出**稳定**（组与组内成员都排序）。
///
/// ## 为什么单独抽出来
///
/// 这是 [`replica_groups_from_ledger`]（出 JSON 给前端）与 [`group_fingerprint_index`]
/// （出令牌给写入口复核）的**共同底座**。两处必须看到**同一份**组结构 —— 否则
/// 前端拿到的令牌与后端复核时算出来的指纹会永远对不上（一类极难查的「莫名失效」）。
///
/// 组键 = 分量内字典序最小的 `uid␟cid`（稳定、可复现；用于 React key / 抽屉定位）。
fn replica_group_members_from_ledger(
    ledger: &serde_json::Map<String, Value>,
) -> Vec<(String, Vec<(String, String)>)> {
    // 邻接表：顶点键 `uid␟cid` → 邻居集合；`label` 供输出还原 `(uid, cid)`。
    let mut adjacency: HashMap<String, BTreeSet<String>> = HashMap::new();
    let mut label: HashMap<String, (String, String)> = HashMap::new();

    for (key, value) in ledger {
        // 只认**同 region 三元键**；跨 region 四元键本期不引入（✅ #2）。
        let Some((source_uid, target_uid, source_cid)) = parse_copy_ledger_key_same_region(key)
        else {
            continue;
        };
        // 值为空串 / 空对象的登记视作无登记（与 `CopyBatch::ledger_hit` 的判据一致）。
        // ★ issue #147：值可能是 `{new, originUid, originCid}` 对象，统一走 `ledger_new_of`。
        let Some(new_cid) = ledger_new_of(value) else {
            continue;
        };
        let source_key = replica_vertex_key(&source_uid, &source_cid);
        let target_key = replica_vertex_key(&target_uid, &new_cid);
        label
            .entry(source_key.clone())
            .or_insert((source_uid, source_cid));
        label
            .entry(target_key.clone())
            .or_insert((target_uid, new_cid.clone()));
        adjacency
            .entry(source_key.clone())
            .or_default()
            .insert(target_key.clone());
        adjacency.entry(target_key).or_default().insert(source_key);
    }

    // 顶点按字典序遍历，保证组内成员与组间顺序都可复现。
    let mut vertices: Vec<String> = adjacency.keys().cloned().collect();
    vertices.sort();

    let mut visited: BTreeSet<String> = BTreeSet::new();
    let mut groups: Vec<(String, Vec<(String, String)>)> = Vec::new();
    for start in &vertices {
        if visited.contains(start) {
            continue;
        }
        // BFS 取连通分量。
        let mut queue: VecDeque<String> = VecDeque::new();
        queue.push_back(start.clone());
        visited.insert(start.clone());
        let mut members: Vec<(String, String)> = Vec::new();
        while let Some(current) = queue.pop_front() {
            if let Some((uid, cid)) = label.get(&current) {
                members.push((uid.clone(), cid.clone()));
            }
            if let Some(neighbors) = adjacency.get(&current) {
                for neighbor in neighbors {
                    if visited.insert(neighbor.clone()) {
                        queue.push_back(neighbor.clone());
                    }
                }
            }
        }
        members.sort();
        members.dedup();
        let group_key = members
            .first()
            .map(|(uid, cid)| replica_vertex_key(uid, cid))
            .unwrap_or_default();
        groups.push((group_key, members));
    }

    groups.sort_by(|left, right| left.0.cmp(&right.0));
    groups
}

/// 解析基线键（`cid\u{1e}cid`）→ `(a, b)`；结构不符 ⇒ `None`（**当作没有基线**）。
///
/// ⚠️ 键的构造与解析必须**同处一地维护**（同 [`copy_ledger_key`] 的纪律）。
fn parse_baseline_key(key: &str) -> Option<(String, String)> {
    let mut parts = key.split(BASELINE_SEP);
    let a = parts.next()?;
    let b = parts.next()?;
    if parts.next().is_some() || a.is_empty() || b.is_empty() {
        return None;
    }
    Some((a.to_string(), b.to_string()))
}

/// 组内的一条「上次一致点」（含两端 cid —— 键里的信息，值里没有）。
#[derive(Clone)]
struct GroupBaseline {
    a: String,
    b: String,
    created_at: i64,
    line_count: usize,
    prefix_hashes: Vec<u64>,
}

/// 基线表 → 结构化条目（一次解析，各组复用）。结构不符的条目**当作没有基线**。
fn parse_group_baselines(baselines: &serde_json::Map<String, Value>) -> Vec<GroupBaseline> {
    baselines
        .iter()
        .filter_map(|(key, value)| {
            let (a, b) = parse_baseline_key(key)?;
            Some(GroupBaseline {
                a,
                b,
                created_at: value.get("createdAt").and_then(Value::as_i64).unwrap_or(0),
                line_count: value.get("lineCount").and_then(Value::as_u64).unwrap_or(0) as usize,
                prefix_hashes: value
                    .get("prefixHashes")
                    .and_then(Value::as_array)
                    .map(|hashes| hashes.iter().filter_map(Value::as_u64).collect())
                    .unwrap_or_default(),
            })
        })
        .collect()
}

/// 本组**最近一次**一致点：`createdAt` 最大；并列时按 `(a, b)` 字典序取最大。
///
/// ⚠️ 并列必须**确定性**地取同一个 —— 否则同一次加载的两次渲染可能选到不同基线
/// （与前端原实现 `newestBaseline` 同判据，逐条对齐）。
fn newest_baseline(baselines: &[GroupBaseline]) -> Option<&GroupBaseline> {
    let mut best: Option<&GroupBaseline> = None;
    for candidate in baselines {
        best = Some(match best {
            None => candidate,
            Some(current) => {
                let better = if candidate.created_at != current.created_at {
                    candidate.created_at > current.created_at
                } else if candidate.a != current.a {
                    candidate.a > current.a
                } else {
                    candidate.b > current.b
                };
                if better {
                    candidate
                } else {
                    current
                }
            }
        });
    }
    best
}

/// 把账本连通成副本组（内部形态：结构 JSON + 成员顶点 + 组内基线）。
///
/// 组上只回 ledger 里确有的字段（`accountUid` / `sessionId` / `region`）外加：
/// - `previewToken`（= [`group_fingerprint`]）—— 前端提交时原样带回，供 [`verify_preview_token`] 复核；
/// - `baseline`（★ P14 S2）—— 本组**两端成员都在组内**的最近一次一致点（`createdAt` /
///   `lineCount` / `a` / `b`），无基线 ⇒ `null`（**不臆造**一个一致点）。
///
/// ⚠️ **不回 `prefixHashes`**：验证「基线是不是当前内容的真前缀」需要逐行 hash，
/// 但那是**后端**的活（[`derive_content_status`]）；把它塞进列表响应曾是 200 MB 级的
/// 包袱。前端只需要**结论**（`content.sinceBaseline`）。
fn replica_groups_from_ledger(
    region: Region,
    ledger: &serde_json::Map<String, Value>,
    baselines: &[GroupBaseline],
) -> Vec<ReplicaGroupEntry> {
    replica_group_members_from_ledger(ledger)
        .into_iter()
        .map(|(key, members)| {
            let cids: BTreeSet<&str> = members.iter().map(|(_, cid)| cid.as_str()).collect();
            let mut group_baselines: Vec<GroupBaseline> = baselines
                .iter()
                .filter(|baseline| {
                    cids.contains(baseline.a.as_str()) && cids.contains(baseline.b.as_str())
                })
                .cloned()
                .collect();
            // 稳定输出：`createdAt` 升序，其次 `a` / `b` 字典序。
            group_baselines.sort_by(|left, right| {
                (left.created_at, &left.a, &left.b).cmp(&(right.created_at, &right.a, &right.b))
            });

            let replicas: Vec<Value> = members
                .iter()
                .map(|(uid, cid)| {
                    json!({
                        "accountUid": uid,
                        "sessionId": cid,
                        "region": region.as_str(),
                    })
                })
                .collect();
            let baseline_json = newest_baseline(&group_baselines)
                .map(|baseline| {
                    json!({
                        "a": baseline.a,
                        "b": baseline.b,
                        "createdAt": baseline.created_at,
                        "lineCount": baseline.line_count,
                    })
                })
                .unwrap_or(Value::Null);
            let json = json!({
                "key": key,
                "replicas": replicas,
                "previewToken": group_fingerprint(&members),
                "baseline": baseline_json,
            });
            ReplicaGroupEntry {
                members,
                baselines: group_baselines,
                json,
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// ★ 组内容状态（原先在前端派生，现收口到 core）
// ---------------------------------------------------------------------------

/// 组状态（与前端 `SessionGroupStatus` **同取值、同判据**）。
pub const STATUS_LATEST: &str = "latest";
pub const STATUS_BEHIND: &str = "behind";
pub const STATUS_DIVERGE: &str = "diverge";
pub const STATUS_UNKNOWN: &str = "unknown";

/// 一组副本的**内容级**状态 —— 会话列表要展示的**全部**内容结论。
///
/// ## 为什么必须搬到 core
///
/// 前端要自己算这个状态，就得拿到**每个副本**的前 512 行逐行 hash：真机账本
/// 25,487 个副本 ⇒ 约 **200 MB** JSON 过 IPC（1300 万个整数），再在 JS 里逐对比较。
/// 后果是会话页**永远转圈**（JSON.parse + O(N²·512) 双重爆炸）。而结论本身只有
/// **每组几个整数** ⇒ 在 core 算完再回。
///
/// ## 算法：O(N·512) 而不是 O(N²·512)
///
/// 「两两公共前缀的**最小值**」与「**全体一致**的最大前缀长度 `k`」恒等：
/// 任意一对至少有 `k` 行相同 ⇒ 两两最小 ≥ `k`；又因 `k` 极大，必存在两个成员在第
/// `k+1` 行不同（否则 `k` 可以取更大）⇒ 两两最小 ≤ `k`。⇒ 取等。
/// 于是只需**一遍扫过全体成员**，不必逐对比较（真机最大组 1338 个成员：
/// 两两 = 89 万对 ≈ 4.6 亿次比较 → 变成 68 万次）。
///
/// ## 判据（与前端逐字对齐，改一处必须同步另一处）
///
/// - `latest`：行数全相同且内容相同（公共前缀 = 行数；超窗口时用整份摘要）；
/// - `behind`：行数不等，但**最短副本是其余成员的前缀** ⇒ 只是有人多写了，无分叉；
/// - `diverge`：在共同旧版之后各自改过（窗口内即分叉，或前缀关系不成立）；
/// - `unknown`：**有副本取不到指纹**，或**超窗口且无法验证前缀关系**
///   ⇒ 诚实标注，**不臆造**。
///
/// ## ★ issue #141：窗口被截断时**不许**拿 512 当全文
///
/// `prefix_hashes` 最多 [`PREFIX_HASH_LIMIT`] 项，而旧判据是「公共前缀 < 最短副本行数
/// ⇒ `diverge`」⇒ 只要组内最短副本 **> 512 行**，`common ≤ 512 < min_lines` **恒为真**
/// ⇒ 逐字节相同的长会话也报「有分歧」。连带三个后果：`behind` 对长会话**永不可达**
/// （「安全同步（只追加）」按钮再也不出现）、用户被引向**覆盖式**统一（会丢目标侧新行）、
/// `deltas` 虚增（800 行的会话被算成「落后 288 条」）。
///
/// 现在的处置分三档（只在**窗口被截断**时才多走两步）：
///
/// 1. 行数全相同 ⇒ 比 `full_hash`：全等 ⇒ `latest`，否则 `diverge`（**精确**，零额外 IO）；
/// 2. 行数不等 ⇒ 逐成员验证「它是不是**最长副本**的前缀」：取最长副本**前 N 行**的累计
///    摘要（`probe`，N = 该成员行数）与它的 `full_hash` 比。全过 ⇒ `behind`；
///    有一条不过 ⇒ `diverge`；取不到摘要 ⇒ `unknown`（**不把读不到当成相同**）；
/// 3. `commonPrefix` 在「超窗口 + 有分叉」时只是**下界**（≥512）⇒ 用
///    `commonPrefixExact: false` 标出来，界面据此说「至少 N 条」而不是「N 条」。
///
/// ⚠️ **不得**用 `updatedAt` 派生本状态（`P8-ARCH.md` §1.2「时间新 ≠ 内容新」）。
/// 本函数**根本不接触**会话行 —— 输入只有指纹、基线与一个按需取摘要的 `probe`。
fn derive_content_status(
    members: &[(String, String)],
    fingerprints: &[Option<SessionFingerprint>],
    baseline: Option<&GroupBaseline>,
    probe: &mut dyn FnMut(usize, usize) -> Option<u64>,
) -> Value {
    if members.len() < 2 {
        return unknown_content_status();
    }
    let mut list: Vec<&SessionFingerprint> = Vec::with_capacity(fingerprints.len());
    for fingerprint in fingerprints {
        // 缺一个 ⇒ 整体 `unknown`（宁可说「未知」，也不用部分数据冒充完整结论）。
        let Some(fingerprint) = fingerprint.as_ref() else {
            return unknown_content_status();
        };
        list.push(fingerprint);
    }

    // ★ 一遍扫过全体成员：公共前缀 = 所有成员都一致的最大前缀长度（见函数头证明）。
    let shortest_prefix = list
        .iter()
        .map(|f| f.prefix_hashes.len())
        .min()
        .unwrap_or(0);
    let mut common = shortest_prefix;
    for index in 0..shortest_prefix {
        let first = list[0].prefix_hashes[index];
        if list.iter().any(|f| f.prefix_hashes[index] != first) {
            common = index;
            break;
        }
    }

    let counts: Vec<usize> = list.iter().map(|f| f.line_count).collect();
    let min_lines = counts.iter().copied().min().unwrap_or(0);
    let max_lines = counts.iter().copied().max().unwrap_or(0);

    // ★ 窗口真的被截断了？「最短的前缀窗口用满了」**且**「还有窗口之外的行」。
    //   两者缺一都不算截断：例如 512 行的会话，窗口恰好覆盖全文。
    let truncated = shortest_prefix == PREFIX_HASH_LIMIT && min_lines > PREFIX_HASH_LIMIT;

    let mut exact = true;
    let status: &str = if common < shortest_prefix {
        // 窗口内就已经分叉 ⇒ 与窗口大小无关的**确定**结论。
        STATUS_DIVERGE
    } else if !truncated {
        // 公共前缀覆盖到了最短副本（窗口没被用满）⇒ 旧判据在这里本来就是精确的。
        if max_lines == min_lines {
            STATUS_LATEST
        } else {
            STATUS_BEHIND
        }
    } else if max_lines == min_lines {
        // 截断 + 行数相同：窗口之外只可能「全同」或「有改写」，整份摘要一次判定。
        if list.iter().all(|f| f.full_hash == list[0].full_hash) {
            common = min_lines;
            STATUS_LATEST
        } else {
            // 有改写 ⇒ 公共前缀至少 512，但**具体多少**不知道 ⇒ 下界。
            exact = false;
            STATUS_DIVERGE
        }
    } else {
        // 截断 + 行数不等：验证「每个成员都是最长副本的前缀」。
        let longest = counts
            .iter()
            .enumerate()
            .max_by_key(|(_, lines)| **lines)
            .map(|(index, _)| index)
            .unwrap_or(0);
        let mut verdict = Some(true);
        for (index, fingerprint) in list.iter().enumerate() {
            if index == longest {
                continue;
            }
            let digest = if fingerprint.line_count == list[longest].line_count {
                // 同长 ⇒ 前缀即全文。
                Some(list[longest].full_hash)
            } else {
                probe(longest, fingerprint.line_count)
            };
            match digest {
                Some(digest) if digest == fingerprint.full_hash => {}
                Some(_) => {
                    verdict = Some(false);
                    break;
                }
                None => {
                    verdict = None;
                    break;
                }
            }
        }
        match verdict {
            // 全体都是最长副本的前缀 ⇒ 最短那份的**全文**就是全组公共前缀。
            Some(true) => {
                common = min_lines;
                STATUS_BEHIND
            }
            Some(false) => {
                exact = false;
                STATUS_DIVERGE
            }
            None => return unknown_content_status(),
        }
    };

    // 增量按 **cid** 聚合（同 cid 出现在不同账号 ⇒ 读数相同；与前端 `Map` 口径一致）。
    let mut deltas: BTreeMap<String, usize> = BTreeMap::new();
    for ((_, cid), fingerprint) in members.iter().zip(list.iter()) {
        deltas.insert(cid.clone(), fingerprint.line_count.saturating_sub(common));
    }

    // ★ P14 S2：相对「上次一致点」的增量 —— **先验证基线仍是当前内容的真前缀**。
    //   验证不过 ⇒ `null`（该副本改写过更早的内容，「新增了几条」这个问题不成立），
    //   **不编一个数**。基线是**成对**的 ⇒ 只给它的两端读数。
    let mut since_baseline: BTreeMap<String, Option<usize>> = BTreeMap::new();
    if let Some(baseline) = baseline {
        for ((_, cid), fingerprint) in members.iter().zip(list.iter()) {
            if *cid != baseline.a && *cid != baseline.b {
                continue;
            }
            let verified = baseline.line_count <= fingerprint.line_count
                && baseline.prefix_hashes.len() <= fingerprint.prefix_hashes.len()
                && baseline
                    .prefix_hashes
                    .iter()
                    .enumerate()
                    .all(|(index, hash)| fingerprint.prefix_hashes[index] == *hash);
            since_baseline.insert(
                cid.clone(),
                verified.then(|| fingerprint.line_count - baseline.line_count),
            );
        }
    }

    json!({
        "status": status,
        "commonPrefix": common,
        // ★ `false` ⇒ 上面的数只是**下界**（窗口被截断且确有分叉）。
        "commonPrefixExact": exact,
        "deltas": deltas,
        "latestLineCount": max_lines,
        "sinceBaseline": since_baseline,
    })
}

/// 取不到任何内容结论时的**诚实**形态（与前端 `empty` 常量同款）。
fn unknown_content_status() -> Value {
    json!({
        "status": STATUS_UNKNOWN,
        "commonPrefix": Value::Null,
        "commonPrefixExact": false,
        "deltas": BTreeMap::<String, usize>::new(),
        "latestLineCount": Value::Null,
        "sinceBaseline": BTreeMap::<String, Option<usize>>::new(),
    })
}

/// 切换前把勾选的会话复制到目标账号（CN，路径 B）。返回复制报告。
pub fn copy_sessions_for_switch(
    progress: Option<&(dyn Fn(&str) + Send + Sync)>,
    target_acc: &Value,
    session_ids: &[String],
) -> Option<Value> {
    copy_sessions_for_switch_for(Region::Cn, progress, target_acc, session_ids)
}

/// 按 region（同一版本内）切换前把勾选的会话复制到目标账号（路径 B）。返回复制报告。
pub fn copy_sessions_for_switch_for(
    region: Region,
    progress: Option<&(dyn Fn(&str) + Send + Sync)>,
    target_acc: &Value,
    session_ids: &[String],
) -> Option<Value> {
    copy_sessions_for_switch_cross(region, region, progress, target_acc, session_ids)
}

/// 跨版本：把 `source_region` 当前登录账号的勾选会话复制到 `target_region` 的目标账号。
///
/// 源 uid 取自**源版本**的认证文件 —— 这正是跨版本的关键：沿用目标版本去取，
/// 取到的是目标版本自己的登录账号，会把「另一个账号」的会话搬过去。
///
/// `progress` 非空时按会话推送「正在复制会话（i/N）：标题」—— 切换流程靠它把
/// GB 级正文复制的等待**切成可见的步进**，避免界面长时间停在一条消息上被当成卡死。
pub fn copy_sessions_for_switch_cross(
    source_region: Region,
    target_region: Region,
    progress: Option<&(dyn Fn(&str) + Send + Sync)>,
    target_acc: &Value,
    session_ids: &[String],
) -> Option<Value> {
    let target_uid = target_acc
        .get("uid")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .unwrap_or_default();
    if target_uid.is_empty() {
        return None;
    }
    let source_uid = current_user_uid_for(source_region)?;
    // 同版本同 uid = 自我复制，无意义；跨版本 uid 同文不算（两版 uid 不同源）。
    if source_region == target_region && source_uid == target_uid {
        return None;
    }

    Some(run_copy_batch(
        source_region,
        target_region,
        &source_uid,
        &target_uid,
        session_ids,
        progress,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// ★ 备份失败**不得**谎报有备份（本次修复的护栏）。
    ///
    /// 现场：三个副本原先都用 `let _ = std::fs::copy(..)` 吞掉错误、然后**无条件**
    /// 返回备份路径 ⇒ 报告里 `backup` 字段永远有值，用户以为有回滚点，
    /// 真出事时才发现备份目录是空的（比不备份更坏）。
    ///
    /// 可证伪：把 `backup_db_to` 改回「忽略 `copy` 结果、无条件返回路径」，
    /// 「必须报错」的两条断言即红。
    ///
    /// ⚠️ 失败分支的构造方式：不能拿**目录**当源库 —— 那会被 `is_file()` 挡在
    /// 前面、走「无需备份」分支（本用例初版就这么写错了，红得毫无意义）。
    /// 这里改用两种真的会失败、且三平台行为一致的构造。
    #[test]
    fn backup_failure_is_reported_instead_of_faked() {
        let root = std::env::temp_dir().join(format!(
            "buddy-switch-backup-{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&root).unwrap();

        // 1) 源库不存在 ⇒ `Ok(None)`：「无需备份」不是失败。
        let missing = root.join("nope.db");
        assert_eq!(backup_db_to(missing, &root.join("dest")).unwrap(), None);

        // 2) 备份目录建不出来（父路径是个普通文件）⇒ 必须 Err，不得返回路径。
        let source = root.join("real.db");
        create_tiny_sessions_db(&source, &["cid-1", "cid-2"]);
        let blocker = root.join("blocker");
        std::fs::write(&blocker, b"not a dir").unwrap();
        let error = backup_db_to(source.clone(), &blocker.join("dest"))
            .expect_err("备份目录建不出来时必须报错");
        assert!(
            error.contains("创建备份目录失败"),
            "文案应指明原因：{error}"
        );

        // 3) 目标位置已被一个**目录**占住 ⇒ 必须 Err（不能让 SQLite 给一个含糊的 open 失败）。
        let occupied = root.join("occupied");
        std::fs::create_dir_all(occupied.join("workbuddy.db")).unwrap();
        let error = backup_db_to(source.clone(), &occupied).expect_err("目标被占住时必须报错");
        assert!(
            error.contains("已被一个目录占住"),
            "文案应指明是目标被占：{error}"
        );

        // 4) ★ 源库**不是**一个有效 SQLite 库 ⇒ 必须 Err。
        //    ⚠️ 这是 P14 S3 的**语义变化**：旧实现（`fs::copy`）会「成功地」拷一份垃圾，
        //    报告里 `backup` 字段有值、用户以为有回滚点 —— 那正是「失败谎报成功」。
        //    在线备份 API 打不开源库 ⇒ 直接失败，**不产出**任何文件。
        let garbage = root.join("garbage.db");
        std::fs::write(&garbage, b"sqlite-ish").unwrap();
        let garbage_dest = root.join("garbage-dest");
        let error = backup_db_to(garbage, &garbage_dest).expect_err("非 SQLite 源库必须报错");
        assert!(error.contains("备份失败"), "文案应指明备份失败：{error}");
        assert!(
            !garbage_dest.join("workbuddy.db").exists(),
            "失败时**不得**留下半截产物（看起来像可用备份，比没有更坏）"
        );

        // 5) 阳性对照：真实库 ⇒ 真的产出快照，且返回目标路径。
        let dest = root.join("dest");
        let path = backup_db_to(source, &dest)
            .unwrap()
            .expect("源库存在时必须备份成功");
        assert_eq!(path, dest.join("workbuddy.db"));
        assert!(
            verify_sqlite_snapshot(&path).is_ok(),
            "产出的快照必须自己就能过 integrity_check"
        );

        std::fs::remove_dir_all(root).unwrap();
    }

    /// ★★ P14 S3：备份必须是**自包含的单文件快照** —— 只拿走 `workbuddy.db` 也完整。
    ///
    /// ## 这就是本条的**证伪点**
    ///
    /// 旧实现（`fs::copy` 三件套）在源库处于 **WAL 模式且 WAL 未 checkpoint** 时，
    /// 新数据其实躺在 `-wal` 里 ⇒ **单独**打开那个 `workbuddy.db` 会**少一大截行**，
    /// 而且备份目录里会多出 `-wal` / `-shm` 两个文件（本测试第 ① 条断言直接红）。
    ///
    /// ⚠️ 必须**保持源连接打开**：SQLite 在**最后一条连接关闭**时会自动 checkpoint
    /// 并删掉 `-wal` —— 那时旧实现也能「碰巧」通过，测试就失去了鉴别力。
    #[test]
    fn db_backup_is_a_self_contained_snapshot() {
        let root = std::env::temp_dir().join(format!(
            "buddy-switch-online-backup-{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let source = root.join("workbuddy.db");

        // 建库 + WAL 模式 + 写入若干行；**连接保持打开**（WAL 不会被 checkpoint 掉）。
        let conn = Connection::open(&source).unwrap();
        let mode: String = conn
            .query_row("PRAGMA journal_mode=WAL", [], |row| row.get(0))
            .unwrap();
        assert_eq!(
            mode, "wal",
            "夹具必须真的把库切到 WAL，否则本测试没有鉴别力"
        );
        conn.execute_batch("CREATE TABLE sessions (id TEXT PRIMARY KEY, user_id TEXT);")
            .unwrap();
        for index in 0..500 {
            conn.execute(
                "INSERT INTO sessions (id, user_id) VALUES (?1, 'uid-a')",
                [format!("cid-{index:04}")],
            )
            .unwrap();
        }
        assert!(
            source.with_file_name("workbuddy.db-wal").is_file(),
            "夹具必须真的留下一个 -wal（否则测不出「单文件是否自包含」）"
        );

        let dest = root.join("snapshot");
        let path = backup_db_to(source.clone(), &dest)
            .unwrap()
            .expect("备份应成功");

        // ① 备份目录里**只有** workbuddy.db（不再有 -wal / -shm 边车）。
        let names: Vec<String> = std::fs::read_dir(&dest)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            names,
            vec!["workbuddy.db".to_string()],
            "在线备份产物必须**只有一个文件**：{names:?}"
        );

        // ② 把它**单独**挪到一个空目录（模拟「用户只拿走 workbuddy.db」）⇒ 仍必须读到全部 500 行。
        let lone_dir = root.join("lone");
        std::fs::create_dir_all(&lone_dir).unwrap();
        let lone = lone_dir.join("workbuddy.db");
        std::fs::copy(&path, &lone).unwrap();
        let rows: i64 = Connection::open(&lone)
            .unwrap()
            .query_row("SELECT COUNT(*) FROM sessions", [], |row| row.get(0))
            .expect("单文件备份必须自包含");
        assert_eq!(
            rows, 500,
            "只拿走 workbuddy.db 也必须是完整快照（旧实现会少一大截）"
        );

        // ③ 自检本身也要有阳性对照：备份能过 `integrity_check`。
        assert!(verify_sqlite_snapshot(&path).is_ok());

        drop(conn);
        std::fs::remove_dir_all(root).unwrap();
    }

    /// 建一个最小的合法 `sessions` 表（`create_sessions_db` 是完整版，这里只要「是有效 SQLite 库」）。
    fn create_tiny_sessions_db(path: &Path, cids: &[&str]) {
        let conn = Connection::open(path).unwrap();
        conn.execute_batch("CREATE TABLE sessions (id TEXT PRIMARY KEY, user_id TEXT);")
            .unwrap();
        for cid in cids {
            conn.execute(
                "INSERT INTO sessions (id, user_id) VALUES (?1, 'uid-a')",
                [*cid],
            )
            .unwrap();
        }
    }

    #[test]
    fn db_paths_point_to_home() {
        // 用 Path 组件比较，避免 Windows `\` / Unix `/` 分隔符差异。
        // ⚠️ 边车映射库**不在这里断言文件名**：它按档位动态发现（见下一条用例），
        // 而本用例读的是**真实 home**，真机上只有 v4 ⇒ 写死 v2 的断言会假失败。
        assert!(
            workbuddy_db_path().ends_with(std::path::Path::new(".workbuddy").join("workbuddy.db"))
        );
    }

    /// 边车映射库的档位发现：**取最大档位**、只认 `.db` 本体、都没有才回落 v2。
    ///
    /// 现场（2026-09-24 真机）：`~/.workbuddy/` 只有 v4；`~/.workbuddy-ai/` 同时有
    /// v3 与 v4。写死 v2 会让 `insert_edge_sync_mapping` 建空库、找不到表、返回
    /// `false` ⇒ 每次会话复制都提示「云端映射注册失败」（**静默**，不报错）。
    #[test]
    fn edge_sync_db_version_is_discovered_not_hardcoded() {
        let root = std::env::temp_dir().join(format!(
            "buddy-switch-edge-sync-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let cn_dir = root.join(".workbuddy");
        std::fs::create_dir_all(&cn_dir).expect("create dir");
        let guard = crate::modules::config::HomeOverrideGuard::set(&root);

        // 一个都没有 ⇒ 回落 v2（历史默认；客户端不在时建哪个都是空的）。
        assert_eq!(latest_edge_sync_db_version(&cn_dir), None);
        assert_eq!(edge_sync_db_path(), cn_dir.join("edge-sync-mapping-v2.db"));

        // 真机国内版形态：只有 v4 ⇒ v4。
        std::fs::write(cn_dir.join("edge-sync-mapping-v4.db"), b"x").expect("seed v4");
        assert_eq!(edge_sync_db_path(), cn_dir.join("edge-sync-mapping-v4.db"));

        // 上一代残留 v3 与 v4 并存 ⇒ 必须取 v4（v3 客户端已不再读，写进去等于没写）。
        std::fs::write(cn_dir.join("edge-sync-mapping-v3.db"), b"x").expect("seed v3");
        assert_eq!(
            edge_sync_db_path(),
            cn_dir.join("edge-sync-mapping-v4.db"),
            "并存的低档位不得把高档位顶掉"
        );

        // SQLite 边车文件不参与选档：`v7.db-wal` 的 `strip_suffix(".db")` 天然失败。
        std::fs::write(cn_dir.join("edge-sync-mapping-v7.db-wal"), b"x").expect("seed wal");
        assert_eq!(
            latest_edge_sync_db_version(&cn_dir),
            Some(4),
            "-wal / -shm 不得被当成独立档位"
        );

        // 前向兼容：档位号还会继续涨，不能只认 v2/v4 两张白名单。
        std::fs::write(cn_dir.join("edge-sync-mapping-v9.db"), b"x").expect("seed v9");
        assert_eq!(latest_edge_sync_db_version(&cn_dir), Some(9));

        // 老客户端只有 v2 ⇒ v2（v3 / v4 / v9 与边车文件都要先清掉）。
        std::fs::remove_file(cn_dir.join("edge-sync-mapping-v3.db")).expect("remove v3");
        std::fs::remove_file(cn_dir.join("edge-sync-mapping-v4.db")).expect("remove v4");
        std::fs::remove_file(cn_dir.join("edge-sync-mapping-v9.db")).expect("remove v9");
        std::fs::remove_file(cn_dir.join("edge-sync-mapping-v7.db-wal")).expect("remove wal");
        assert_eq!(edge_sync_db_path(), cn_dir.join("edge-sync-mapping-v2.db"));

        // 目录不存在 ⇒ 同样回落 v2（不得 panic）。
        assert_eq!(latest_edge_sync_db_version(&root.join("nope")), None);

        drop(guard);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 会话目录与数据库路径必须按 region 隔离（PRD G1 / D2）。
    ///
    /// CN 用 `~/.workbuddy`，Global 用 `~/.workbuddy-ai`；两版若共用一个目录，
    /// 会话库会互相覆盖。
    #[test]
    fn session_paths_are_region_scoped() {
        // 读要**原子**：本用例把早先算出的目录与稍后重算的 `home_dir()` 相比，
        // 并行用例改 `BUDDY_SWITCH_HOME` 时中途换根就会随机红。
        let _lock = crate::modules::config::env_lock();
        let cn_dir = session_data_dir(Region::Cn);
        let global_dir = session_data_dir(Region::Global);

        assert_eq!(
            cn_dir.file_name().and_then(|n| n.to_str()),
            Some(".workbuddy"),
            "CN 会话目录名"
        );
        assert_eq!(
            global_dir.file_name().and_then(|n| n.to_str()),
            Some(".workbuddy-ai"),
            "Global 会话目录名"
        );
        assert_ne!(cn_dir, global_dir, "CN / Global 会话目录必须隔离");
        // 同一 home，仅目录名不同。
        assert_eq!(cn_dir.parent(), global_dir.parent());
        assert_eq!(
            cn_dir,
            crate::modules::config::home_dir().join(".workbuddy")
        );
        assert_eq!(
            global_dir,
            crate::modules::config::home_dir().join(".workbuddy-ai")
        );

        // 数据库路径由会话目录派生，同样必须按 region 隔离。
        assert_eq!(
            workbuddy_db_path_for(Region::Cn),
            cn_dir.join("workbuddy.db")
        );
        assert_eq!(
            workbuddy_db_path_for(Region::Global),
            global_dir.join("workbuddy.db")
        );
        assert_ne!(
            workbuddy_db_path_for(Region::Cn),
            workbuddy_db_path_for(Region::Global)
        );
        // CN 兼容路径与 region 化路径一致，避免两处逻辑漂移。
        assert_eq!(workbuddy_db_path(), workbuddy_db_path_for(Region::Cn));
    }

    fn temp_db(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "buddy_switch_test_{}_{name}.db",
            uuid::Uuid::new_v4().simple()
        ))
    }

    #[test]
    fn insert_session_copy_duplicates_row_with_target_uid() {
        let db = temp_db("sessions");
        let conn = Connection::open(&db).unwrap();
        conn.execute_batch(
            "CREATE TABLE sessions (
                id TEXT PRIMARY KEY,
                user_id TEXT NOT NULL,
                title TEXT,
                cwd TEXT,
                created_at INTEGER,
                updated_at INTEGER,
                deleted_at INTEGER,
                payload BLOB
            );",
        )
        .unwrap();
        conn.execute(
            "INSERT INTO sessions (id, user_id, title, cwd, created_at, updated_at, deleted_at, payload)
             VALUES ('src-1', 'uid-a', '旧标题', '/ws', 1000, 2000, NULL, x'DEADBEEF')",
            [],
        )
        .unwrap();

        insert_session_copy(&db, &db, "new-uuid-1", "src-1", "uid-a", "uid-b").unwrap();

        let (id, user_id, title, deleted_at): (String, String, String, Option<i64>) = conn
            .query_row(
                "SELECT id, user_id, title, deleted_at FROM sessions WHERE id = 'new-uuid-1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .unwrap();
        assert_eq!(id, "new-uuid-1");
        assert_eq!(user_id, "uid-b");
        assert_eq!(title, "旧标题"); // 普通列原样保留
        assert_eq!(deleted_at, None); // deleted_at 置空

        // 源行保持不变
        let src_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sessions WHERE id = 'src-1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(src_count, 1);
    }

    #[test]
    fn insert_session_copy_missing_source_is_noop() {
        let db = temp_db("noop");
        let conn = Connection::open(&db).unwrap();
        conn.execute_batch(
            "CREATE TABLE sessions (id TEXT PRIMARY KEY, user_id TEXT, title TEXT, created_at INTEGER, updated_at INTEGER, deleted_at INTEGER);",
        )
        .unwrap();
        insert_session_copy(&db, &db, "new-1", "missing", "uid-a", "uid-b").unwrap();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM sessions", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn insert_session_copy_missing_db_is_ok() {
        let db = temp_db("missing");
        // 不创建文件
        assert!(insert_session_copy(&db, &db, "new-1", "src-1", "a", "b").is_ok());
    }

    /// 跨库复制：源行只出现在源库，副本只出现在目标库。
    ///
    /// 这是跨版本复制的核心断言 —— 若实现仍把「读」和「写」绑在同一个 db 上，
    /// 要么副本没进目标库（用户看不到），要么源库被写脏。
    #[test]
    fn insert_session_copy_across_databases() {
        let src_db = temp_db("xsrc");
        let dst_db = temp_db("xdst");
        for db in [&src_db, &dst_db] {
            let conn = Connection::open(db).unwrap();
            conn.execute_batch(
                "CREATE TABLE sessions (
                    id TEXT PRIMARY KEY,
                    user_id TEXT NOT NULL,
                    title TEXT,
                    created_at INTEGER,
                    updated_at INTEGER,
                    deleted_at INTEGER
                 );",
            )
            .unwrap();
        }
        Connection::open(&src_db)
            .unwrap()
            .execute(
                "INSERT INTO sessions (id, user_id, title, created_at, updated_at, deleted_at)
                 VALUES ('src-1', 'uid-a', '跨版本标题', 1000, 2000, NULL)",
                [],
            )
            .unwrap();

        insert_session_copy(&src_db, &dst_db, "new-1", "src-1", "uid-a", "uid-b").unwrap();

        let dst = Connection::open(&dst_db).unwrap();
        let (title, user_id): (String, String) = dst
            .query_row(
                "SELECT title, user_id FROM sessions WHERE id = 'new-1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(title, "跨版本标题");
        assert_eq!(user_id, "uid-b", "副本必须归属目标账号");

        let src_rows: i64 = Connection::open(&src_db)
            .unwrap()
            .query_row("SELECT COUNT(*) FROM sessions", [], |r| r.get(0))
            .unwrap();
        assert_eq!(src_rows, 1, "源库不得被写入");
    }

    #[test]
    fn insert_edge_sync_mapping_registers_channel() {
        let db = temp_db("edge");
        let conn = Connection::open(&db).unwrap();
        conn.execute_batch(
            "CREATE TABLE edge_sync_mapping (
                session_id TEXT,
                conversation_id TEXT,
                msg_channel TEXT,
                created_at INTEGER
            );",
        )
        .unwrap();
        assert!(insert_edge_sync_mapping(&db, "new-1", "uid-b"));
        let (sid, cid, channel): (String, String, String) = conn
            .query_row(
                "SELECT session_id, conversation_id, msg_channel FROM edge_sync_mapping",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(sid, "new-1");
        assert_eq!(cid, "new-1");
        assert_eq!(channel, "convmsg:uid-b");
    }

    #[test]
    fn insert_edge_sync_mapping_missing_table_false() {
        let db = temp_db("edge-no-table");
        let conn = Connection::open(&db).unwrap();
        conn.execute_batch("CREATE TABLE other (x INTEGER);")
            .unwrap();
        assert!(!insert_edge_sync_mapping(&db, "new-1", "uid-b"));
    }

    /// 写源 region 的认证文件（`account.uid` = 源 uid）。
    ///
    /// `copy_sessions_for_switch_cross` 的源 uid 取自**源版本**的认证文件 ——
    /// 夹具必须把它写对，否则批量复制会因「源 uid 取不到」整批跳过。
    fn write_source_auth(home: &Path, region: Region, uid: &str) {
        // 与 `auth_file::auth_candidates_for` **独立编码**（不 import 被测路径，
        // 保证「写错位置 ⇒ 测试红」是可证伪的）。
        #[cfg(target_os = "windows")]
        let dir = "AppData/Local/CodeBuddyExtension/Data/Public/auth";
        #[cfg(target_os = "macos")]
        let dir = "Library/Application Support/CodeBuddyExtension/Data/Public/auth";
        #[cfg(not(any(target_os = "windows", target_os = "macos")))]
        let dir = ".local/share/CodeBuddyExtension/Data/Public/auth";
        let filename = match region {
            Region::Cn => "workbuddy-desktop.info",
            Region::Global => "workbuddy-desktop-ai.info",
        };
        let path = home.join(dir).join(filename);
        std::fs::create_dir_all(path.parent().expect("auth parent")).unwrap();
        std::fs::write(path, format!(r#"{{"account":{{"uid":"{uid}"}}}}"#)).unwrap();
    }

    /// 批量复制夹具：源 region 一个库，`uid-src` 名下 `count` 个会话，
    /// 每个都带 jsonl 正文（正文里嵌着自己的 cid —— 与真机同形态，副本必须替换它）。
    /// 认证文件一并写好（源 uid = `uid-src`）。返回 (home, cids)。
    fn seed_batch_copy_home(label: &str, count: usize) -> (PathBuf, Vec<String>) {
        let home = make_temp_home(label);
        let dir = home.join(".workbuddy");
        let ws = dir.join("projects").join("ws");
        std::fs::create_dir_all(&ws).unwrap();
        let mut cids: Vec<String> = Vec::with_capacity(count);
        for index in 0..count {
            cids.push(format!("cid-batch-{index:04}-aaaa-bbbb-cccc-ddddeeeeffff"));
        }
        let rows: Vec<(&str, &str, &str)> = cids
            .iter()
            .map(|cid| (cid.as_str(), "/proj/ws", "批量会话"))
            .collect();
        create_sessions_db(&dir.join("workbuddy.db"), "uid-src", &rows);
        for (index, cid) in cids.iter().enumerate() {
            std::fs::write(
                ws.join(format!("{cid}.jsonl")),
                format!(
                    "{{\"sessionId\":\"{cid}\",\"cwd\":\"/proj/ws\",\"text\":\"消息 {index}\"}}\n"
                ),
            )
            .unwrap();
        }
        write_source_auth(&home, Region::Cn, "uid-src");
        (home, cids)
    }

    /// ★ 批量复制必须逐会话推送进度（切换剧本里唯一的「等待可视化」出口）。
    ///
    /// 现场：旧实现在整个复制批次期间只推一条「正在复制会话到目标账号…」，
    /// 正文是 GB 级 I/O ⇒ 用户盯着一行消息几十秒，完全看不出「还在动」。
    ///
    /// 可证伪：把 `run_copy_batch` phase A 里的逐会话 `p(&format!(...))` 删掉，
    /// 「每个会话一条 (i/N) 步进」的两条断言即红（`contains` 0 命中）。
    #[test]
    fn batch_copy_reports_per_session_progress() {
        let (home, cids) = seed_batch_copy_home("batch-progress", 3);
        let _g = crate::modules::config::HomeOverrideGuard::set(&home);

        let messages = std::sync::Mutex::new(Vec::new());
        let report = copy_sessions_for_switch_cross(
            Region::Cn,
            Region::Cn,
            Some(&|message: &str| {
                messages.lock().unwrap().push(message.to_string());
            }),
            &json!({ "uid": "uid-dst" }),
            &cids,
        )
        .expect("批量复制应产出报告");

        assert_eq!(
            report["copied"].as_array().expect("copied").len(),
            3,
            "3 个会话都应复制成功：{report}"
        );
        let messages = messages.into_inner().unwrap();
        let steps: Vec<&String> = messages
            .iter()
            .filter(|message| message.contains("/3）"))
            .collect();
        assert_eq!(steps.len(), 3, "每个会话都要有一条步进进度：{messages:?}");
        assert!(
            messages.iter().any(|message| message.contains("（1/3）")),
            "步进必须从 1 开始：{messages:?}"
        );
        assert!(
            messages.iter().any(|message| message.contains("（3/3）")),
            "步进必须到 N 收尾：{messages:?}"
        );
        cleanup_temp_home(&home);
    }

    /// ★ 整批只备份**一次**，且备份的是复制**前**的库。
    ///
    /// 旧实现对每个会话各调一次 `backup_workbuddy_db`：N 个会话 = N 份整库拷贝，
    /// 且第 2..N 份里已经掺进了前面会话插进去的副本行（**不是**纯粹的复制前快照）。
    ///
    /// 可证伪：把 `CopyBatch::ensure_backup` 改回「每会话备一次」
    /// （例如把 `backup_attempted` 闸去掉），「备份行数 = 3」即红 ——
    /// 最后一个备份里会含 3 + 2 = 5 行（前两个会话的副本）。
    #[test]
    fn batch_copy_backs_up_db_once_before_any_write() {
        let (home, cids) = seed_batch_copy_home("batch-backup", 3);
        let _g = crate::modules::config::HomeOverrideGuard::set(&home);

        let report = copy_sessions_for_switch_cross(
            Region::Cn,
            Region::Cn,
            None,
            &json!({ "uid": "uid-dst" }),
            &cids,
        )
        .expect("批量复制应产出报告");
        let copied = report["copied"].as_array().expect("copied");
        assert_eq!(copied.len(), 3);

        let backups: Vec<&str> = copied
            .iter()
            .map(|item| item["backup"].as_str().expect("backup 路径"))
            .collect();
        assert!(
            backups.windows(2).all(|pair| pair[0] == pair[1]),
            "整批必须共用同一个备份点：{backups:?}"
        );

        // 备份内容 = 复制**前**的库：只有 3 个源行，不含任何副本。
        let backup_rows: i64 = Connection::open(Path::new(backups[0]))
            .expect("打开备份库")
            .query_row("SELECT COUNT(*) FROM sessions", [], |r| r.get(0))
            .expect("count backup rows");
        assert_eq!(
            backup_rows, 3,
            "备份必须是复制前的快照（3 源行；按会话各备一份会掺进副本行）"
        );

        // 收尾核对：目标库里应多出 3 行归属 uid-dst 的副本。
        let db = home.join(".workbuddy").join("workbuddy.db");
        let copied_rows: i64 = Connection::open(&db)
            .expect("打开目标库")
            .query_row(
                "SELECT COUNT(*) FROM sessions WHERE user_id = 'uid-dst'",
                [],
                |r| r.get(0),
            )
            .expect("count copied rows");
        assert_eq!(copied_rows, 3, "3 个会话都应写入索引行");
        cleanup_temp_home(&home);
    }

    /// ★★ P14 S2：**复制成功** ⇒ 为「源 cid × 新 cid」记一条基线（= 上次一致点）。
    ///
    /// 可证伪：删掉 `run_copy_batch` phase B 里的 `batch.record_baseline(...)`
    /// （或删掉末尾的 `batch.flush_baselines()`）⇒ 基线表为空 ⇒ 红。
    #[test]
    fn copy_records_a_baseline_for_each_copied_pair() {
        let (home, cids) = seed_batch_copy_home("baseline-copy", 3);
        let _g = crate::modules::config::HomeOverrideGuard::set(&home);

        let report = copy_sessions_for_switch_cross(
            Region::Cn,
            Region::Cn,
            None,
            &json!({ "uid": "uid-dst" }),
            &cids,
        )
        .expect("批量复制应产出报告");
        let copied = report["copied"].as_array().expect("copied");
        assert_eq!(copied.len(), 3);

        let baselines = load_session_baselines_at(&session_baseline_path_for(Region::Cn));
        assert_eq!(baselines.len(), 3, "每个成功复制的会话都应有一条基线");
        for item in copied {
            let source_cid = item["id"].as_str().unwrap();
            let new_cid = item["newId"].as_str().unwrap();
            let entry = baselines
                .get(&baseline_key(source_cid, new_cid))
                .unwrap_or_else(|| panic!("缺少 {source_cid} → {new_cid} 的基线"));
            let parsed: PairBaseline =
                serde_json::from_value(entry.clone()).expect("基线结构必须能读回");
            assert!(
                parsed.line_count > 0,
                "一致点的行数必须是**真读出来**的（不是编的 0）"
            );
            assert!(
                !parsed.prefix_hashes.is_empty(),
                "基线必须带逐行 hash —— 否则「前缀验证」无从谈起"
            );
        }

        cleanup_temp_home(&home);
    }

    /// phase A 的并行复制必须**一个都不落**：8 个会话（跨多个 worker）全部落盘，
    /// 且每份副本都换成了新 id（不残留源 cid）—— 这是并行化正确性的总断言。
    #[test]
    fn batch_copy_parallel_phase_writes_every_jsonl() {
        let (home, cids) = seed_batch_copy_home("batch-parallel", 8);
        let _g = crate::modules::config::HomeOverrideGuard::set(&home);

        let report = copy_sessions_for_switch_cross(
            Region::Cn,
            Region::Cn,
            None,
            &json!({ "uid": "uid-dst" }),
            &cids,
        )
        .expect("批量复制应产出报告");
        let copied = report["copied"].as_array().expect("copied");
        assert_eq!(copied.len(), 8, "8 个会话都应复制：{report}");

        let ws = home.join(".workbuddy").join("projects").join("ws");
        for (index, item) in copied.iter().enumerate() {
            assert_eq!(item["jsonlCopied"], true, "第 {index} 份正文必须落盘");
            let new_cid = item["newId"].as_str().expect("newId");
            let text = std::fs::read_to_string(ws.join(format!("{new_cid}.jsonl")))
                .unwrap_or_else(|e| panic!("副本正文应存在（{new_cid}）：{e}"));
            assert!(text.contains(new_cid), "副本正文必须换成新 id：{text}");
            assert!(
                !text.contains(&cids[index]),
                "副本不得残留源会话 id：{text}"
            );
        }

        // 账本整批一次落盘：8 条登记，键形如 source␟target␟cid。
        let ledger_text = std::fs::read_to_string(
            home.join(".workbuddy")
                .join("buddy-switch-copy-ledger.json"),
        )
        .expect("账本应落盘");
        for cid in &cids {
            assert!(ledger_text.contains(cid), "账本应登记 {cid}：{ledger_text}");
        }
        cleanup_temp_home(&home);
    }

    /// 同一 id 在一次批量里重复传入只复制一次，第二次进 `skipped`
    /// （旧实现靠「账本落盘后再读」撞对；现在批内记账，语义不变）。
    #[test]
    fn batch_copy_skips_duplicate_ids_within_one_batch() {
        let (home, cids) = seed_batch_copy_home("batch-dup", 2);
        let _g = crate::modules::config::HomeOverrideGuard::set(&home);

        let report = copy_sessions_for_switch_cross(
            Region::Cn,
            Region::Cn,
            None,
            &json!({ "uid": "uid-dst" }),
            &[cids[0].clone(), cids[1].clone(), cids[0].clone()],
        )
        .expect("批量复制应产出报告");

        assert_eq!(report["copied"].as_array().expect("copied").len(), 2);
        let skipped = report["skipped"].as_array().expect("skipped");
        assert_eq!(skipped.len(), 1, "重复的那次必须进 skipped：{report}");
        assert_eq!(
            skipped[0]["id"].as_str(),
            Some(cids[0].as_str()),
            "skipped 必须指回重复的源会话"
        );
        assert_eq!(
            skipped[0]["deduplicated"], true,
            "批内重复同样按「已存在」归类"
        );
        cleanup_temp_home(&home);
    }

    #[test]
    fn session_display_title_prefers_custom_title() {
        assert_eq!(
            session_display_title(Some("自动标题".into()), Some("美团每日自动领券".into())),
            "美团每日自动领券"
        );
        assert_eq!(
            session_display_title(None, Some("美团每日自动领券".into())),
            "美团每日自动领券"
        );
        assert_eq!(
            session_display_title(Some("汉字详情页".into()), None),
            "汉字详情页"
        );
        assert_eq!(session_display_title(None, None), "(无标题)");
        assert_eq!(
            session_display_title(Some("  ".into()), Some("".into())),
            "(无标题)"
        );
    }

    #[test]
    fn claw_workspace_detected_by_folder_name() {
        assert!(is_claw_workspace("/Users/apple/WorkBuddy/Claw"));
        assert!(is_claw_workspace("/Users/apple/WorkBuddy/claw/"));
        assert!(is_claw_workspace(r"C:\Users\me\WorkBuddy\Claw"));
        assert!(!is_claw_workspace("/Users/apple/WorkBuddy/ClawBot"));
        assert!(!is_claw_workspace(
            "/Users/apple/Documents/AI-PROJECT/LetterTotTown"
        ));
    }

    /// 账本键必须同时区分源账号、目标账号与源会话：任一维度不同即为不同条目，
    /// 否则「A→C 复制过 x」会错误阻断「B→C 复制 x」。
    #[test]
    fn copy_ledger_key_separates_all_three_dimensions() {
        let base = copy_ledger_key("uid-a", "uid-b", "cid-1");
        assert_ne!(
            base,
            copy_ledger_key("uid-a2", "uid-b", "cid-1"),
            "源账号区分"
        );
        assert_ne!(
            base,
            copy_ledger_key("uid-a", "uid-b2", "cid-1"),
            "目标账号区分"
        );
        assert_ne!(
            base,
            copy_ledger_key("uid-a", "uid-b", "cid-2"),
            "源会话区分"
        );
        assert_eq!(base, copy_ledger_key("uid-a", "uid-b", "cid-1"));
        // 分隔符不得与字段内容混淆：把分隔符塞进字段值也必须产生不同键。
        assert_ne!(
            copy_ledger_key("uid-a", "uid-b", "cid-1"),
            copy_ledger_key("uid-a\u{1f}uid-b", "", "cid-1"),
            "字段内容中的分隔符不得造成键碰撞"
        );
    }

    /// 跨版本账本键必须带源 region 前缀，同版本必须保持原键。
    ///
    /// 两个方向都要钉住：加前缀是为了「CN 的 uid-a」与「Global 的 uid-a」不共用登记；
    /// 同版本不加是为了既有账本不失配（否则老用户会被判成没复制过而重复复制）。
    #[test]
    fn cross_region_ledger_key_is_namespaced_by_source_region() {
        let same = copy_ledger_key_for(Region::Cn, Region::Cn, "uid-a", "uid-b", "cid-1");
        let cross = copy_ledger_key_for(Region::Cn, Region::Global, "uid-a", "uid-b", "cid-1");
        let reverse = copy_ledger_key_for(Region::Global, Region::Cn, "uid-a", "uid-b", "cid-1");

        assert_eq!(
            same,
            copy_ledger_key("uid-a", "uid-b", "cid-1"),
            "同版本键不得变"
        );
        assert_ne!(same, cross, "跨版本必须与同版本区分");
        assert_ne!(cross, reverse, "方向不同必须是不同条目");
        assert!(cross.starts_with("cn\u{1f}"), "前缀应为源 region: {cross}");
        assert!(
            reverse.starts_with("global\u{1f}"),
            "前缀应为源 region: {reverse}"
        );
    }

    /// 会话正文副本必须落在**目标**版本的 projects 下。
    ///
    /// 若沿用源目录，副本会写进源版本目录，目标版本的 WorkBuddy 根本看不到它 ——
    /// 这正是「跨版本不支持」时期最隐蔽的失败形态（不报错，只是没搬过去）。
    #[test]
    fn cross_region_jsonl_copy_lands_in_target_region_projects() {
        // ⚠️ `session_data_dir` 是**无参全局路径函数**（每次调用都重读 `BUDDY_SWITCH_HOME`）。
        //    本用例原先在断言两侧各求值一次 ⇒ 并行单测里只要有别的用例换了 home，
        //    两次就会读到**两个不同的根** ⇒「单独跑绿、全量跑红」。
        //    这正是本仓记过四次的「假失败」模式（确定性缺陷，不是竞态）。
        //    ⇒ 用 `HomeOverrideGuard` 把 home **钉住**，两次求值必然同源。
        //    ⚠️ 不要在这里再手动 `env_lock()` —— `set()` 内部已经取了，不可重入会自锁死。
        let dir = std::env::temp_dir().join(format!(
            "buddy-switch-session-paths-{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let _guard = crate::modules::config::HomeOverrideGuard::set(&dir);

        let src = session_data_dir(Region::Cn)
            .join("projects")
            .join("ws")
            .join("cid-1.jsonl");

        let same = target_jsonl_path_for(Region::Cn, Region::Cn, &src, "new-1");
        assert_eq!(
            same,
            session_data_dir(Region::Cn)
                .join("projects")
                .join("ws")
                .join("new-1.jsonl"),
            "同版本必须原地换名，行为零变化"
        );

        let cross = target_jsonl_path_for(Region::Cn, Region::Global, &src, "new-1");
        assert_eq!(
            cross,
            session_data_dir(Region::Global)
                .join("projects")
                .join("ws")
                .join("new-1.jsonl"),
            "跨版本必须换到目标版本目录"
        );
        assert_ne!(same, cross);

        drop(_guard);
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// 账本读写往返：写入后能读回，且未命中项返回 None。
    ///
    /// 全程只操作临时文件，**不得触碰真实 `~/.workbuddy` 账本**。
    #[test]
    fn copy_ledger_roundtrip_and_miss() {
        let region = Region::Cn;
        let dir = std::env::temp_dir().join(format!(
            "buddy_switch_ledger_{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(COPY_LEDGER_FILE);

        assert!(
            load_copy_ledger_at(&path).is_empty(),
            "账本不存在时应视为空"
        );

        assert!(record_copy_ledger_at(
            region,
            region,
            Some(&path),
            "uid-a",
            "uid-b",
            "cid-1",
            "new-1"
        ));

        let reloaded = load_copy_ledger_at(&path);
        assert_eq!(
            reloaded
                .get(&copy_ledger_key("uid-a", "uid-b", "cid-1"))
                .and_then(Value::as_str),
            Some("new-1")
        );
        assert!(
            reloaded
                .get(&copy_ledger_key("uid-a", "uid-b", "other"))
                .is_none(),
            "未登记的源会话不得命中"
        );

        // 同一键再次登记应覆盖而不是新增条目。
        assert!(record_copy_ledger_at(
            region,
            region,
            Some(&path),
            "uid-a",
            "uid-b",
            "cid-1",
            "new-2"
        ));
        let overwritten = load_copy_ledger_at(&path);
        assert_eq!(overwritten.len(), 1, "同一键不得产生第二条");
        assert_eq!(
            overwritten
                .get(&copy_ledger_key("uid-a", "uid-b", "cid-1"))
                .and_then(Value::as_str),
            Some("new-2")
        );

        // 测试过程不得在真实数据目录留下账本。
        assert!(
            !copy_ledger_path_for(region).exists() || copy_ledger_path_for(region).is_file(),
            "账本路径形态异常"
        );

        std::fs::remove_dir_all(dir).unwrap();
    }

    // -----------------------------------------------------------------------
    // 降级路径：db 不可读 / 为空时从 projects 目录扫描 jsonl
    // -----------------------------------------------------------------------
    //
    // 这些测试把进程级 home 重定向到**临时目录**（via `HomeOverrideGuard`，drop 时
    // 还原），绝不触碰真实 `~/.workbuddy{,-ai}`。home 是进程级全局状态，故一律先取
    // `env_lock`（已在 `HomeOverrideGuard::set` 内串行化），避免与并行测试互相踩踏。

    /// 在临时目录建一个「已存在」的 home（validate_home_override 要求绝对且已存在）。
    fn make_temp_home(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "buddy_switch_fb_{}_{label}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// 删掉临时 home；失败忽略（仅泄漏临时文件，不影响断言）。
    fn cleanup_temp_home(dir: &Path) {
        let _ = std::fs::remove_dir_all(dir);
    }

    /// 建一个合法 sessions 表，写入若干属于 `uid` 的未删除会话。
    fn create_sessions_db(db: &Path, uid: &str, rows: &[(&str, &str, &str)]) {
        if let Some(parent) = db.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        let conn = Connection::open(db).unwrap();
        conn.execute_batch(
            "CREATE TABLE sessions (
                id TEXT PRIMARY KEY,
                user_id TEXT NOT NULL,
                title TEXT,
                cwd TEXT,
                created_at INTEGER,
                updated_at INTEGER,
                deleted_at INTEGER
            );",
        )
        .unwrap();
        for (cid, cwd, title) in rows {
            conn.execute(
                "INSERT INTO sessions (id, user_id, title, cwd, created_at, updated_at, deleted_at)
                 VALUES (?1, ?2, ?3, ?4, 1000, 2000, NULL)",
                rusqlite::params![cid, uid, title, cwd],
            )
            .unwrap();
        }
    }

    /// db 完全缺失 → 降级扫描 projects → source == "scan"，并列出 jsonl 会话。
    ///
    /// 这正是用户现场的根因：workbuddy.db 损坏后 UI 误判「账号无会话」、禁用复制。
    /// 降级路径必须让 UI 仍能看到（至少）jsonl 里存在的会话。
    #[test]
    fn list_sessions_falls_back_to_scan_when_db_missing() {
        let home = make_temp_home("missing-db");
        let _g = crate::modules::config::HomeOverrideGuard::set(&home);
        // 只放 jsonl，不放 workbuddy.db
        let ws = home.join(".workbuddy").join("projects").join("ws");
        std::fs::create_dir_all(&ws).unwrap();
        std::fs::write(
            ws.join("cid-A.jsonl"),
            "{\"cwd\":\"/proj/alpha\",\"title\":\"Alpha 会话\"}\n",
        )
        .unwrap();

        let resp = list_sessions_with_fallback_for(Region::Cn, "uid-x");
        assert_eq!(
            resp.get("source").and_then(Value::as_str),
            Some("scan"),
            "db 缺失应降级到 scan"
        );
        let sessions = resp.get("sessions").and_then(Value::as_array).unwrap();
        assert_eq!(sessions.len(), 1, "应扫描到 1 个 jsonl 会话");
        let s = &sessions[0];
        assert_eq!(s.get("id").and_then(Value::as_str), Some("cid-A"));
        assert_eq!(s.get("title").and_then(Value::as_str), Some("Alpha 会话"));
        assert_eq!(s.get("cwd").and_then(Value::as_str), Some("/proj/alpha"));
        assert_eq!(
            s.get("degraded").and_then(Value::as_bool),
            Some(true),
            "扫描得到的会话应标 degraded"
        );
        assert!(resp.get("warning").is_some(), "降级应给出 warning");
        cleanup_temp_home(&home);
    }

    /// db 存在但**不是合法 sqlite**（磁盘镜像损坏）→ 同样降级到 scan。
    ///
    /// 死磕这条：真实 bug 就是 `database disk image is malformed`。必须证明「损坏」
    /// 不再被静默成空数组，而是走 scan 降级。
    #[test]
    fn list_sessions_falls_back_to_scan_when_db_corrupt() {
        let home = make_temp_home("corrupt-db");
        let _g = crate::modules::config::HomeOverrideGuard::set(&home);
        std::fs::create_dir_all(home.join(".workbuddy")).unwrap();
        std::fs::write(
            home.join(".workbuddy").join("workbuddy.db"),
            b"this is not a sqlite database file at all",
        )
        .unwrap();
        let ws = home.join(".workbuddy").join("projects").join("ws");
        std::fs::create_dir_all(&ws).unwrap();
        std::fs::write(
            ws.join("cid-B.jsonl"),
            "{\"cwd\":\"/proj/beta\",\"title\":\"Beta\"}\n",
        )
        .unwrap();

        let resp = list_sessions_with_fallback_for(Region::Cn, "uid-x");
        assert_eq!(
            resp.get("source").and_then(Value::as_str),
            Some("scan"),
            "损坏 db 必须降级到 scan，而不是静默空数组"
        );
        assert_eq!(
            resp.get("sessions")
                .and_then(Value::as_array)
                .unwrap()
                .len(),
            1
        );
        cleanup_temp_home(&home);
    }

    /// db 可读且有该账号的会话 → source == "db"，**不**重复列出 projects 里的 jsonl。
    ///
    /// 证明正常路径优先级最高，降级只是兜底；否则 db 行与扫描结果会重复。
    #[test]
    fn list_sessions_prefers_db_when_readable() {
        let home = make_temp_home("db-ok");
        let _g = crate::modules::config::HomeOverrideGuard::set(&home);
        create_sessions_db(
            &home.join(".workbuddy").join("workbuddy.db"),
            "uid-x",
            &[("cid-DB", "/proj/db", "DB 会话")],
        );
        // 同时放一个其它 cid 的 jsonl：db 优先，绝不应被重复列。
        let ws = home.join(".workbuddy").join("projects").join("ws");
        std::fs::create_dir_all(&ws).unwrap();
        std::fs::write(
            ws.join("cid-OTHER.jsonl"),
            "{\"cwd\":\"/x\",\"title\":\"X\"}\n",
        )
        .unwrap();

        let resp = list_sessions_with_fallback_for(Region::Cn, "uid-x");
        assert_eq!(resp.get("source").and_then(Value::as_str), Some("db"));
        let sessions = resp.get("sessions").and_then(Value::as_array).unwrap();
        assert_eq!(sessions.len(), 1, "只应列 db 中的 1 条，jsonl 不重复");
        assert_eq!(
            sessions[0].get("id").and_then(Value::as_str),
            Some("cid-DB")
        );
        assert_eq!(
            sessions[0].get("degraded").and_then(Value::as_bool),
            None,
            "db 源不应标 degraded"
        );
        assert!(resp.get("warning").is_none(), "db 源不应有 warning");
        cleanup_temp_home(&home);
    }

    /// ★ 客户端某些版本把会话行的 `user_id` 落成**空串**（2026-09-23 实测：本机 116/116 全为空串）
    /// ⇒ 严格匹配 0 行时必须放宽到「空归属」行，否则 UI 会静默显示「当前账号暂无会话」并禁用复制。
    #[test]
    fn list_sessions_widens_scope_when_strict_uid_matches_nothing() {
        let home = make_temp_home("uid-empty");
        let _g = crate::modules::config::HomeOverrideGuard::set(&home);
        // 库里的行归属为空串，而当前登录 uid 是真实 uuid。
        create_sessions_db(
            &home.join(".workbuddy").join("workbuddy.db"),
            "",
            &[("cid-EMPTY-UID", "/proj/empty", "空归属会话")],
        );

        let resp = list_sessions_with_fallback_for(Region::Cn, "uid-real");
        assert_eq!(
            resp.get("source").and_then(Value::as_str),
            Some("db"),
            "空归属行仍属 db 源"
        );
        let sessions = resp.get("sessions").and_then(Value::as_array).unwrap();
        assert_eq!(
            sessions.len(),
            1,
            "严格匹配为空时应放宽到空归属行，而不是报告「账号无会话」"
        );
        assert_eq!(
            sessions[0].get("id").and_then(Value::as_str),
            Some("cid-EMPTY-UID")
        );
        cleanup_temp_home(&home);
    }

    /// 负向对照：放宽**不得**扩到别的账号 —— 库里只有别人的（非空 uid）会话时仍应视为无会话。
    #[test]
    fn list_sessions_does_not_widen_to_other_accounts() {
        let home = make_temp_home("uid-other");
        let _g = crate::modules::config::HomeOverrideGuard::set(&home);
        create_sessions_db(
            &home.join(".workbuddy").join("workbuddy.db"),
            "uid-someone-else",
            &[("cid-OTHER-UID", "/proj/other", "别人的会话")],
        );

        let resp = list_sessions_with_fallback_for(Region::Cn, "uid-real");
        let sessions = resp.get("sessions").and_then(Value::as_array).unwrap();
        assert!(
            sessions.is_empty(),
            "别人的账号（非空 uid）不得被放宽列出来"
        );
        cleanup_temp_home(&home);
    }

    /// 部分命中：库里既有当前账号的行、又有旧「空归属」行 ⇒ 两类都必须列出（**并集**）。
    ///
    /// 这正是用户现场恢复索引后的形态：新会话带真实 uid、旧会话 `user_id` 为空串。
    /// 若只做「严格命中 0 行才放宽」，这类库仍然列不出旧会话。
    #[test]
    fn list_sessions_unions_own_rows_with_legacy_empty_uid_rows() {
        let home = make_temp_home("uid-mixed");
        let _g = crate::modules::config::HomeOverrideGuard::set(&home);
        let db = home.join(".workbuddy").join("workbuddy.db");
        create_sessions_db(&db, "uid-mine", &[("cid-MINE", "/proj/mine", "我的会话")]);
        // 追加一条旧「空归属」行（不能复用 create_sessions_db：表已存在）。
        let conn = Connection::open(&db).unwrap();
        conn.execute(
            "INSERT INTO sessions (id, user_id, title, cwd, created_at, updated_at, deleted_at)
             VALUES ('cid-LEGACY', '', '旧会话', '/proj/legacy', 1000, 3000, NULL)",
            [],
        )
        .unwrap();
        drop(conn);

        let resp = list_sessions_with_fallback_for(Region::Cn, "uid-mine");
        let sessions = resp.get("sessions").and_then(Value::as_array).unwrap();
        let ids: Vec<&str> = sessions
            .iter()
            .filter_map(|s| s.get("id").and_then(Value::as_str))
            .collect();
        assert!(ids.contains(&"cid-MINE"), "当前账号自己的会话必须列出");
        assert!(
            ids.contains(&"cid-LEGACY"),
            "旧空归属会话必须一并列出（并集）"
        );
        assert_eq!(sessions.len(), 2, "并集不得产生重复行");
        cleanup_temp_home(&home);
    }

    /// 数据目录里连 `workbuddy.db` / `projects/` 都没有 → source == "no-dir"。
    ///
    /// 现场（2026-09-23）：客户端重装把 `~/.workbuddy` 重建为空，UI 却显示「当前账号暂无会话」
    /// —— 用户以为账号没会话，实际是整个数据目录空了。两种情形必须能分辨。
    #[test]
    fn list_sessions_reports_no_dir_when_data_dir_is_gone() {
        let home = make_temp_home("no-dir");
        let _g = crate::modules::config::HomeOverrideGuard::set(&home);
        let resp = list_sessions_with_fallback_for(Region::Cn, "uid-x");
        assert_eq!(resp.get("source").and_then(Value::as_str), Some("no-dir"));
        assert_eq!(
            resp.get("sessions")
                .and_then(Value::as_array)
                .unwrap()
                .len(),
            0
        );
        cleanup_temp_home(&home);
    }

    /// 数据目录在、库也在，只是当前账号没有会话 → source == "empty"
    /// （UI 这才显示「当前账号暂无会话」，措辞必须与 no-dir 区分）。
    #[test]
    fn list_sessions_reports_empty_when_db_exists_but_has_no_sessions() {
        let home = make_temp_home("empty");
        let _g = crate::modules::config::HomeOverrideGuard::set(&home);
        // 合法空库：表在、行数为 0。
        create_sessions_db(&home.join(".workbuddy").join("workbuddy.db"), "uid-x", &[]);
        let resp = list_sessions_with_fallback_for(Region::Cn, "uid-x");
        assert_eq!(resp.get("source").and_then(Value::as_str), Some("empty"));
        assert_eq!(
            resp.get("sessions")
                .and_then(Value::as_array)
                .unwrap()
                .len(),
            0
        );
        cleanup_temp_home(&home);
    }

    /// 扫描应跳过 Claw 工作区，与 db 版语义对齐。
    #[test]
    fn scan_skips_claw_workspaces() {
        let home = make_temp_home("claw");
        let _g = crate::modules::config::HomeOverrideGuard::set(&home);
        let ws = home.join(".workbuddy").join("projects").join("ws");
        std::fs::create_dir_all(&ws).unwrap();
        std::fs::write(
            ws.join("cid-normal.jsonl"),
            "{\"cwd\":\"/proj/normal\",\"title\":\"N\"}\n",
        )
        .unwrap();
        std::fs::write(
            ws.join("cid-claw.jsonl"),
            "{\"cwd\":\"/proj/Claw\",\"title\":\"C\"}\n",
        )
        .unwrap();

        let sessions = scan_sessions_from_jsonl_for(Region::Cn);
        assert_eq!(sessions.len(), 1, "Claw 工作区必须被跳过");
        assert_eq!(
            sessions[0].get("id").and_then(Value::as_str),
            Some("cid-normal")
        );
        cleanup_temp_home(&home);
    }

    /// ★ 子代理记录不是会话：`projects/<ws>/<cid>/subagents/agent-*.jsonl` 必须被跳过。
    ///
    /// 实测现场（2026-09-24）：国内版 `projects/` 下 391 个真会话旁边躺着 **81 个**子代理文件。
    /// 无界递归会把它们当成会话，切换弹窗里凭空多出几十条 `agent-xxxx`。
    #[test]
    fn scan_skips_subagent_transcripts() {
        let home = make_temp_home("subagent");
        let _g = crate::modules::config::HomeOverrideGuard::set(&home);
        let ws = home.join(".workbuddy").join("projects").join("ws");
        std::fs::create_dir_all(&ws).unwrap();
        std::fs::write(
            ws.join("cid-real.jsonl"),
            "{\"cwd\":\"/proj/real\",\"aiTitle\":\"真会话\"}\n",
        )
        .unwrap();
        // 更深一层：<cid>/subagents/agent-*.jsonl（子代理记录）
        let sub = ws.join("cid-real").join("subagents");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(
            sub.join("agent-6a07c65d.jsonl"),
            "{\"cwd\":\"/proj/real\",\"aiTitle\":\"子代理\"}\n",
        )
        .unwrap();

        let sessions = scan_sessions_from_jsonl_for(Region::Cn);
        let ids: Vec<&str> = sessions
            .iter()
            .filter_map(|s| s.get("id").and_then(Value::as_str))
            .collect();
        assert_eq!(ids, vec!["cid-real"], "只应列出真会话，子代理记录必须跳过");
        cleanup_temp_home(&home);
    }

    /// 扫描从 jsonl 首行解析 cwd / title；缺字段时用「(无标题)」。
    #[test]
    fn scan_parses_cwd_and_title_from_jsonl() {
        let home = make_temp_home("meta");
        let _g = crate::modules::config::HomeOverrideGuard::set(&home);
        let ws = home.join(".workbuddy").join("projects").join("ws");
        std::fs::create_dir_all(&ws).unwrap();
        // 首行是纯数据行（无 cwd/title），meta 写在第二行 —— 容忍前几行。
        std::fs::write(
            ws.join("cid-meta.jsonl"),
            "{\"role\":\"user\",\"content\":\"hi\"}\n{\"cwd\":\"/proj/m\",\"title\":\"M 会话\"}\n",
        )
        .unwrap();
        // 首行无任何可解析字段 → 标题回退「(无标题)」。
        std::fs::write(ws.join("cid-anon.jsonl"), "{\"role\":\"assistant\"}\n").unwrap();

        let mut sessions = scan_sessions_from_jsonl_for(Region::Cn);
        // 按 id 查表更稳（mtime 在新文件上等同，顺序不可靠）。
        sessions.sort_by_key(|s| {
            s.get("id")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string()
        });
        assert_eq!(sessions.len(), 2);
        let by_id: std::collections::HashMap<String, Value> = sessions
            .iter()
            .map(|s| {
                (
                    s.get("id").and_then(Value::as_str).unwrap().to_string(),
                    s.clone(),
                )
            })
            .collect();
        let meta = by_id.get("cid-meta").unwrap();
        assert_eq!(meta.get("cwd").and_then(Value::as_str), Some("/proj/m"));
        assert_eq!(meta.get("title").and_then(Value::as_str), Some("M 会话"));
        let anon = by_id.get("cid-anon").unwrap();
        assert_eq!(
            anon.get("title").and_then(Value::as_str),
            Some("(无标题)"),
            "无标题应回退占位符"
        );
        cleanup_temp_home(&home);
    }

    /// **真实 jsonl 格式**：`cwd` 在**每一行**上，标题字段叫 `aiTitle`（实测在第 3 行）。
    ///
    /// 这条用例锁的就是当年那个缺陷：旧实现找的是 `title` 字段，真实文件里根本没有
    /// ⇒ 每个会话都退化成「(无标题)」，而单测用的自制 fixture 恰好有 `title` ⇒ 全绿。
    #[test]
    fn scan_reads_real_format_ai_title() {
        let home = make_temp_home("real-fmt");
        let _g = crate::modules::config::HomeOverrideGuard::set(&home);
        let ws = home.join(".workbuddy").join("projects").join("ws");
        std::fs::create_dir_all(&ws).unwrap();
        // 逐行模仿实测格式：第 1 行 message 带 cwd，第 2 行无标题，第 3 行 ai-title 带 aiTitle。
        std::fs::write(
            ws.join("cid-real.jsonl"),
            concat!(
                "{\"type\":\"message\",\"cwd\":\"/proj/real\",\"role\":\"user\",\"content\":\"hi\"}\n",
                "{\"type\":\"message\",\"cwd\":\"/proj/real\",\"role\":\"assistant\",\"content\":\"yo\"}\n",
                "{\"type\":\"ai-title\",\"cwd\":\"/proj/real\",\"aiTitle\":\"真实自动标题\"}\n",
            ),
        )
        .unwrap();

        let sessions = scan_sessions_from_jsonl_for(Region::Cn);
        assert_eq!(sessions.len(), 1);
        assert_eq!(
            sessions[0].get("cwd").and_then(Value::as_str),
            Some("/proj/real")
        );
        assert_eq!(
            sessions[0].get("title").and_then(Value::as_str),
            Some("真实自动标题"),
            "标题必须取自 aiTitle（真实格式），不是 title"
        );
        cleanup_temp_home(&home);
    }

    /// 用户改过名的会话以 `customTitle` 为准 —— 与 db 版 [`session_display_title`] 同优先级。
    #[test]
    fn scan_prefers_custom_title_over_ai_title() {
        let home = make_temp_home("custom-title");
        let _g = crate::modules::config::HomeOverrideGuard::set(&home);
        let ws = home.join(".workbuddy").join("projects").join("ws");
        std::fs::create_dir_all(&ws).unwrap();
        // customTitle 实测出现得很靠后（第 56 行），这里刻意也放远一点，顺便验证窗口够宽。
        let mut body = String::from(
            "{\"type\":\"message\",\"cwd\":\"/proj/c\",\"role\":\"user\",\"content\":\"hi\"}\n\
             {\"type\":\"ai-title\",\"cwd\":\"/proj/c\",\"aiTitle\":\"自动标题\"}\n",
        );
        for i in 0..40 {
            body.push_str(&format!(
                "{{\"type\":\"message\",\"cwd\":\"/proj/c\",\"role\":\"assistant\",\"content\":\"line {i}\"}}\n"
            ));
        }
        body.push_str(
            "{\"type\":\"custom-title\",\"cwd\":\"/proj/c\",\"customTitle\":\"我改的名字\"}\n",
        );
        std::fs::write(ws.join("cid-custom.jsonl"), body).unwrap();

        let sessions = scan_sessions_from_jsonl_for(Region::Cn);
        assert_eq!(sessions.len(), 1);
        assert_eq!(
            sessions[0].get("title").and_then(Value::as_str),
            Some("我改的名字"),
            "customTitle 优先于 aiTitle"
        );
        cleanup_temp_home(&home);
    }

    /// 头部读取有**行数**上限：元数据落在 128 行之后就读不到（回退「(无标题)」）。
    ///
    /// 这是刻意的成本闸 —— 一行的**字节**长度不受控，只靠字节上限会被一行超长内容拖死。
    #[test]
    fn scan_stops_at_line_cap() {
        let home = make_temp_home("line-cap");
        let _g = crate::modules::config::HomeOverrideGuard::set(&home);
        let ws = home.join(".workbuddy").join("projects").join("ws");
        std::fs::create_dir_all(&ws).unwrap();
        let mut body = String::new();
        for i in 0..200 {
            body.push_str(&format!(
                "{{\"type\":\"message\",\"role\":\"user\",\"n\":{i}}}\n"
            ));
        }
        body.push_str("{\"cwd\":\"/proj/late\",\"aiTitle\":\"来晚了\"}\n");
        std::fs::write(ws.join("cid-late.jsonl"), body).unwrap();

        let sessions = scan_sessions_from_jsonl_for(Region::Cn);
        assert_eq!(sessions.len(), 1);
        assert_eq!(
            sessions[0].get("title").and_then(Value::as_str),
            Some("(无标题)"),
            "第 201 行的元数据不该被读到"
        );
        cleanup_temp_home(&home);
    }

    /// 头部读取有**字节**上限：前 100 行各 1KB（≈100KB > 64KB）时，第 101 行的元数据读不到。
    ///
    /// 与上一条互补：这条里行数（101 < 128）没超，**只有**字节上限生效 ⇒ 两条分别锁定两个闸。
    #[test]
    fn scan_stops_at_byte_cap() {
        let home = make_temp_home("byte-cap");
        let _g = crate::modules::config::HomeOverrideGuard::set(&home);
        let ws = home.join(".workbuddy").join("projects").join("ws");
        std::fs::create_dir_all(&ws).unwrap();
        let filler = "x".repeat(1000);
        let mut body = String::new();
        for i in 0..100 {
            body.push_str(&format!(
                "{{\"type\":\"message\",\"role\":\"user\",\"n\":{i},\"pad\":\"{filler}\"}}\n"
            ));
        }
        body.push_str("{\"cwd\":\"/proj/far\",\"aiTitle\":\"太远了\"}\n");
        std::fs::write(ws.join("cid-far.jsonl"), body).unwrap();

        let sessions = scan_sessions_from_jsonl_for(Region::Cn);
        assert_eq!(sessions.len(), 1);
        assert_eq!(
            sessions[0].get("title").and_then(Value::as_str),
            Some("(无标题)")
        );
        assert_eq!(sessions[0].get("cwd").and_then(Value::as_str), Some(""));
        cleanup_temp_home(&home);
    }

    /// 头部有**非 JSON** 噪声（真实文件首行可能是半截 / 非 JSON）时不能整条丢弃，
    /// 后续行的元数据仍要能解析出来。
    #[test]
    fn scan_tolerates_garbage_head_lines() {
        let home = make_temp_home("garbage-head");
        let _g = crate::modules::config::HomeOverrideGuard::set(&home);
        let ws = home.join(".workbuddy").join("projects").join("ws");
        std::fs::create_dir_all(&ws).unwrap();
        std::fs::write(
            ws.join("cid-garbage.jsonl"),
            "not json at all\n\n{\"cwd\":\"/proj/g\"}\n{\"aiTitle\":\"幸存标题\"}\n",
        )
        .unwrap();

        let sessions = scan_sessions_from_jsonl_for(Region::Cn);
        assert_eq!(sessions.len(), 1);
        assert_eq!(
            sessions[0].get("cwd").and_then(Value::as_str),
            Some("/proj/g")
        );
        assert_eq!(
            sessions[0].get("title").and_then(Value::as_str),
            Some("幸存标题")
        );
        cleanup_temp_home(&home);
    }

    /// 跨 region 扫描隔离：Global 的 projects 不应出现在 CN 的扫描结果里。
    #[test]
    fn scan_is_region_scoped() {
        let home = make_temp_home("region-scope");
        let _g = crate::modules::config::HomeOverrideGuard::set(&home);
        let cn_ws = home.join(".workbuddy").join("projects").join("ws");
        let g_ws = home.join(".workbuddy-ai").join("projects").join("ws");
        std::fs::create_dir_all(&cn_ws).unwrap();
        std::fs::create_dir_all(&g_ws).unwrap();
        std::fs::write(
            cn_ws.join("cid-cn.jsonl"),
            "{\"cwd\":\"/cn\",\"title\":\"C\"}\n",
        )
        .unwrap();
        std::fs::write(
            g_ws.join("cid-g.jsonl"),
            "{\"cwd\":\"/g\",\"title\":\"G\"}\n",
        )
        .unwrap();

        let cn = scan_sessions_from_jsonl_for(Region::Cn);
        assert_eq!(cn.len(), 1);
        assert_eq!(cn[0].get("id").and_then(Value::as_str), Some("cid-cn"));
        let g = scan_sessions_from_jsonl_for(Region::Global);
        assert_eq!(g.len(), 1);
        assert_eq!(g[0].get("id").and_then(Value::as_str), Some("cid-g"));
        cleanup_temp_home(&home);
    }

    /// 复制降级：源 jsonl 存在但 workbuddy.db 缺失 → 仍复制正文，索引行写不进时给 warning。
    ///
    /// 这对应「目标账号 db 也坏了」的最坏情形：jsonl 正文仍落到目标账号，但**索引行写不了**。
    /// ★ 这里断言的是「报告必须说实话」（`sessionRowWritten: false` + 可诊断的
    /// `sessionRowError` + 批次 `failed`），**不是**「重启客户端会自动重建索引」——
    /// 那条旧承诺从未成立，客户端不会从 jsonl 反推索引（issue #146）。
    #[test]
    fn copy_session_degrades_when_db_missing() {
        let home = make_temp_home("copy-degraded");
        let _g = crate::modules::config::HomeOverrideGuard::set(&home);
        // 源 region 放 jsonl，但**没有** workbuddy.db（既不读索引、也写不进目标索引）。
        let src_ws = home.join(".workbuddy").join("projects").join("ws");
        std::fs::create_dir_all(&src_ws).unwrap();
        std::fs::write(
            src_ws.join("cid-S.jsonl"),
            "{\"cwd\":\"/proj/s\",\"title\":\"S\",\"sessionId\":\"cid-S\"}\n",
        )
        .unwrap();

        let result =
            copy_session_to_user_cross(Region::Cn, Region::Cn, "cid-S", "uid-src", "uid-dst")
                .expect("降级复制不应报错");
        assert_eq!(
            result.get("jsonlCopied").and_then(Value::as_bool),
            Some(true),
            "jsonl 必须被复制"
        );
        assert_eq!(
            result.get("sessionRowWritten").and_then(Value::as_bool),
            Some(false),
            "db 缺失则索引行没写"
        );
        assert_eq!(
            result.get("mappingWritten").and_then(Value::as_bool),
            Some(false),
            "edge_sync 映射库缺失则注册失败（不致命）"
        );
        assert!(
            result.get("warning").is_some(),
            "应给出降级 warning（索引未写入），但**不得**承诺重启后客户端会重建索引"
        );
        assert!(
            result.get("sessionRowError").is_some(),
            "失败原因必须可诊断，不能只留一句泛化 warning"
        );
        assert_eq!(
            result.get("failed").and_then(Value::as_bool),
            Some(true),
            "索引行写失败时整批必须标记 failed"
        );
        assert_eq!(
            result.get("deduplicated").and_then(Value::as_bool),
            Some(false)
        );

        // 副本应落在同版本 projects 下、使用新 cid。
        let copied = std::fs::read_dir(&src_ws)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().to_string())
            .any(|name| name.ends_with(".jsonl") && name != "cid-S.jsonl");
        assert!(copied, "应生成新 cid 的 jsonl 副本");
        cleanup_temp_home(&home);
    }

    /// ★ issue #146 回归：源账号在 `sessions` 表里没有该 cid 的行（全新账号 / 列表来自
    /// jsonl 反扫），复制必须**合成**索引行 —— 只复制正文会让目标账号永远「暂无任务」。
    #[test]
    fn copy_synthesizes_index_row_when_source_row_missing() {
        let home = make_temp_home("copy-synth-row");
        let _g = crate::modules::config::HomeOverrideGuard::set(&home);
        let ws = home.join(".workbuddy").join("projects").join("ws");
        std::fs::create_dir_all(&ws).unwrap();
        std::fs::write(
            ws.join("cid-S.jsonl"),
            "{\"cwd\":\"/proj/s\",\"title\":\"S\",\"sessionId\":\"cid-S\"}\n",
        )
        .unwrap();
        // 目标库存在，但 `cid-S` 属于别的账号（源账号是全新的，库里没有它的行）。
        let db = workbuddy_db_path_for(Region::Cn);
        let conn = Connection::open(&db).unwrap();
        conn.execute_batch(
            "CREATE TABLE sessions (
                id TEXT PRIMARY KEY,
                user_id TEXT NOT NULL,
                cwd TEXT,
                title TEXT,
                status TEXT,
                transport TEXT,
                created_at INTEGER,
                updated_at INTEGER,
                deleted_at INTEGER
            );",
        )
        .unwrap();
        conn.execute(
            "INSERT INTO sessions (id, user_id, cwd, title, status, transport, created_at, updated_at)
             VALUES ('cid-S', 'uid-other', '/x', 'old', 'completed', 'local', 1, 1)",
            [],
        )
        .unwrap();
        drop(conn);

        let result =
            copy_session_to_user_cross(Region::Cn, Region::Cn, "cid-S", "uid-src", "uid-dst")
                .expect("复制不应报错");
        assert_eq!(
            result.get("sessionRowWritten").and_then(Value::as_bool),
            Some(true),
            "源行缺失时必须合成索引行：{result}"
        );
        assert!(result.get("sessionRowError").is_none(), "{result}");
        assert!(result.get("failed").is_none(), "{result}");

        let conn = Connection::open(&db).unwrap();
        let (count, cwd): (i64, String) = conn
            .query_row(
                "SELECT COUNT(*), COALESCE(MAX(cwd), '') FROM sessions WHERE user_id = 'uid-dst'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(count, 1, "目标账号必须拿到 1 条索引行");
        assert_eq!(cwd, "/proj/s", "cwd 应从 jsonl 取，供客户端工作区分组");
        cleanup_temp_home(&home);
    }

    /// 复制降级：源 db 损坏（读不到 cwd）→ 从 jsonl 推断 Claw，Claw 仍被拒绝。
    #[test]
    fn copy_session_rejects_claw_even_when_db_corrupt() {
        let home = make_temp_home("copy-claw");
        let _g = crate::modules::config::HomeOverrideGuard::set(&home);
        std::fs::create_dir_all(home.join(".workbuddy")).unwrap();
        std::fs::write(
            home.join(".workbuddy").join("workbuddy.db"),
            b"corrupt sqlite",
        )
        .unwrap();
        let src_ws = home.join(".workbuddy").join("projects").join("ws");
        std::fs::create_dir_all(&src_ws).unwrap();
        std::fs::write(
            src_ws.join("cid-claw.jsonl"),
            "{\"cwd\":\"/proj/Claw\",\"title\":\"C\"}\n",
        )
        .unwrap();

        let err =
            copy_session_to_user_cross(Region::Cn, Region::Cn, "cid-claw", "uid-src", "uid-dst")
                .expect_err("Claw 工作区必须被拒绝");
        assert!(err.contains("Claw"), "拒绝原因应为 Claw: {err}");
        cleanup_temp_home(&home);
    }

    /// 损坏的账本必须退化为「无登记」而不是报错或丢弃已有副本。
    #[test]
    fn corrupt_copy_ledger_is_treated_as_empty() {
        let dir = std::env::temp_dir().join(format!(
            "buddy_switch_ledger_corrupt_{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(COPY_LEDGER_FILE);

        for payload in ["not-json", "[1,2,3]", "\"text\"", "null", ""] {
            std::fs::write(&path, payload).unwrap();
            assert!(
                load_copy_ledger_at(&path).is_empty(),
                "损坏内容应视为空账本：{payload}"
            );
        }

        std::fs::remove_dir_all(dir).unwrap();
    }

    /// ★ P8 T02：账本边 → 连通分量（副本组）；只读、只回 ledger 里确有的字段。
    ///
    /// 可证伪：把 `replica_groups_from_ledger` 改成「每条边各成一组」（不连通），
    /// 「共享源会话的两条边应合成一组」即红；把返回值加上 `title` 等臆造字段，
    /// 「副本行只应有三个字段」即红。
    #[test]
    fn replica_groups_connect_edges_into_components() {
        let dir = std::env::temp_dir().join(format!(
            "buddy_switch_replica_groups_{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(COPY_LEDGER_FILE);
        let ledger = json!({
            copy_ledger_key("uid-a", "uid-b", "cid-1"): "new-1",
            copy_ledger_key("uid-a", "uid-c", "cid-1"): "new-2",
            copy_ledger_key("uid-x", "uid-y", "cid-9"): "new-9",
        });
        std::fs::write(&path, serde_json::to_string(&ledger).unwrap()).unwrap();

        let out = list_replica_groups_at(Region::Cn, Some(&path));
        assert_eq!(out.get("source").and_then(Value::as_str), Some("ledger"));
        let groups = out.get("groups").and_then(Value::as_array).unwrap();
        assert_eq!(groups.len(), 2, "共享源会话的两条边应连通成一组");

        // 第一组：uid-a 的 cid-1 复制到 uid-b / uid-c ⇒ 三个顶点（组键 = 字典序最小顶点）。
        let first = &groups[0];
        assert_eq!(
            first.get("key").and_then(Value::as_str),
            Some(replica_vertex_key("uid-a", "cid-1").as_str()),
            "组键应取分量内字典序最小的顶点"
        );
        let replicas = first.get("replicas").and_then(Value::as_array).unwrap();
        assert_eq!(replicas.len(), 3, "共享源会话应连通成 3 个顶点");
        let has = |uid: &str, cid: &str| {
            replicas.iter().any(|r| {
                r.get("accountUid").and_then(Value::as_str) == Some(uid)
                    && r.get("sessionId").and_then(Value::as_str) == Some(cid)
            })
        };
        assert!(has("uid-a", "cid-1"), "源端顶点");
        assert!(has("uid-b", "new-1"), "目标端顶点");
        assert!(has("uid-c", "new-2"), "第二个目标端顶点");

        // 只回 ledger 里确有的字段（后端**不臆造** title / updatedAt / hasHistory）。
        for replica in replicas {
            let obj = replica.as_object().unwrap();
            assert_eq!(obj.len(), 3, "副本行只应有 accountUid / sessionId / region");
            assert!(obj.contains_key("accountUid"));
            assert!(obj.contains_key("sessionId"));
            assert_eq!(obj.get("region").and_then(Value::as_str), Some("cn"));
        }

        std::fs::remove_dir_all(dir).unwrap();
    }

    /// ★ P8 T02：跨 region 四元键**不引入**；缺失 / 空账本 ⇒ `source: "empty"`。
    ///
    /// 可证伪：让 `parse_copy_ledger_key_same_region` 接受四元键（去掉长度判断），
    /// 第二条断言即红（跨 region 边会被引入）。
    #[test]
    fn replica_groups_ignore_cross_region_and_empty_ledger() {
        let dir = std::env::temp_dir().join(format!(
            "buddy_switch_replica_groups_cross_{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(COPY_LEDGER_FILE);

        // 文件缺失 ⇒ 空。
        let missing = dir.join("nope.json");
        let out = list_replica_groups_at(Region::Cn, Some(&missing));
        assert_eq!(out.get("source").and_then(Value::as_str), Some("empty"));
        assert!(out
            .get("groups")
            .and_then(Value::as_array)
            .unwrap()
            .is_empty());

        // 只有跨 region 四元键（带源 region 前缀）⇒ 本期不引入 ⇒ 空。
        let cross = json!({
            copy_ledger_key_for(Region::Global, Region::Cn, "uid-a", "uid-b", "cid-1"): "new-1",
        });
        std::fs::write(&path, serde_json::to_string(&cross).unwrap()).unwrap();
        let out = list_replica_groups_at(Region::Cn, Some(&path));
        assert_eq!(
            out.get("source").and_then(Value::as_str),
            Some("empty"),
            "跨 region 边本期不引入"
        );
        assert!(out
            .get("groups")
            .and_then(Value::as_array)
            .unwrap()
            .is_empty());

        std::fs::remove_dir_all(dir).unwrap();
    }

    /// ★ issue #147：账本值必须同时读得懂旧格式（裸字符串）与新格式（带血缘对象）。
    ///
    /// 变异：把 `ledger_new_of` 改回 `value.as_str()` ⇒ 升级后所有旧账本失配、
    /// 全部会话被重新复制一遍（正是本次事故的放大器）。
    #[test]
    fn ledger_value_supports_legacy_and_bloodline_shapes() {
        assert_eq!(
            ledger_new_of(&json!("copy-1")).as_deref(),
            Some("copy-1"),
            "旧账本（裸字符串）必须继续可读"
        );
        let value = json!({"new": "copy-2", "originUid": "uid-a", "originCid": "cid-1"});
        assert_eq!(ledger_new_of(&value).as_deref(), Some("copy-2"));
        assert_eq!(
            ledger_origin_of(&value),
            Some(("uid-a".to_string(), "cid-1".to_string()))
        );
        assert_eq!(ledger_origin_of(&json!("copy-1")), None, "旧格式没有血缘");
        assert_eq!(ledger_new_of(&json!({})), None);
        assert_eq!(ledger_new_of(&json!("   ")), None);
    }

    /// 账本键 → `target_uid`：同 region 三元与跨 region 四元都要认。
    #[test]
    fn ledger_key_target_reads_both_key_shapes() {
        let sep = LEDGER_SEP;
        assert_eq!(
            ledger_key_target(&format!("a{sep}b{sep}c")),
            Some("b".to_string())
        );
        assert_eq!(
            ledger_key_target(&format!("cn{sep}a{sep}b{sep}c")),
            Some("b".to_string())
        );
        assert_eq!(ledger_key_target("nonsense"), None);
    }

    /// 副本识别索引：能回答「这个 cid 是哪个原始会话的副本」「已复制给谁」。
    #[test]
    fn origin_indexes_answer_bloodline_questions() {
        let sep = LEDGER_SEP;
        let mut map = serde_json::Map::new();
        map.insert(
            format!("uid-a{sep}uid-b{sep}cid-1"),
            json!({"new": "copy-1", "originUid": "uid-a", "originCid": "cid-1"}),
        );
        // 旧格式条目：无血缘 ⇒ 不进索引（不能凭空猜）。
        map.insert(format!("uid-x{sep}uid-y{sep}cid-9"), json!("copy-9"));

        let (origins, copied) = build_origin_indexes(&map);
        assert_eq!(
            origins.get("copy-1").cloned(),
            Some(("uid-a".to_string(), "cid-1".to_string()))
        );
        assert!(!origins.contains_key("copy-9"), "旧条目没有血缘可查");
        assert!(copied.contains(&("cid-1".to_string(), "uid-b".to_string())));
        assert!(!copied.contains(&("cid-1".to_string(), "uid-a".to_string())));
    }

    /// ★ issue #147 端到端：把副本当源再复制时，必须被血缘判定拦下（skipped），
    /// 而不是再造一代副本。
    #[test]
    fn copying_a_replica_back_is_skipped_by_bloodline() {
        let home = make_temp_home("copy-replica-skip");
        let _g = crate::modules::config::HomeOverrideGuard::set(&home);
        let ws = home.join(".workbuddy").join("projects").join("ws");
        std::fs::create_dir_all(&ws).unwrap();
        std::fs::write(ws.join("cid-1.jsonl"), "{\"cwd\":\"/p\",\"title\":\"T\"}\n").unwrap();
        // B 的副本（上一次 A→B 的产物）
        std::fs::write(
            ws.join("copy-1.jsonl"),
            "{\"cwd\":\"/p\",\"title\":\"T\"}\n",
        )
        .unwrap();

        let mut map = serde_json::Map::new();
        map.insert(
            format!("uid-a{LEDGER_SEP}uid-b{LEDGER_SEP}cid-1"),
            json!({"new": "copy-1", "originUid": "uid-a", "originCid": "cid-1"}),
        );
        std::fs::write(
            copy_ledger_path_for(Region::Cn),
            serde_json::to_string(&Value::Object(map)).unwrap(),
        )
        .unwrap();

        // B 的副本再复制回 A（origin 就是 A 自己的会话）⇒ 必须跳过。
        let result = copy_session_to_user_cross(Region::Cn, Region::Cn, "copy-1", "uid-b", "uid-a")
            .expect("副本识别不应报错");
        assert_eq!(
            result.get("deduplicated").and_then(Value::as_bool),
            Some(true),
            "副本回流必须判为去重跳过：{result}"
        );
        let files = std::fs::read_dir(&ws).unwrap().flatten().count();
        assert_eq!(files, 2, "不得生成新一代副本：{files}");
        cleanup_temp_home(&home);
    }

    /// 账本命中还必须要求副本仍存活：副本被删则允许重新复制。
    #[test]
    fn ledger_only_hits_when_copy_still_alive() {
        let db = temp_db("ledger-alive");
        let conn = Connection::open(&db).unwrap();
        conn.execute_batch(
            "CREATE TABLE sessions (
                id TEXT PRIMARY KEY,
                user_id TEXT NOT NULL,
                title TEXT,
                deleted_at INTEGER
            );",
        )
        .unwrap();
        conn.execute(
            "INSERT INTO sessions (id, user_id, deleted_at) VALUES ('copy-1', 'uid-b', NULL)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO sessions (id, user_id, deleted_at) VALUES ('copy-2', 'uid-b', 12345)",
            [],
        )
        .unwrap();

        // 直接验证存活判定语义（账本文件 + db 路径由 region 派生，无法在本单测内重定向）。
        let alive = |cid: &str| -> bool {
            conn.query_row(
                "SELECT EXISTS(SELECT 1 FROM sessions \
                 WHERE id = ?1 AND user_id = 'uid-b' AND deleted_at IS NULL)",
                rusqlite::params![cid],
                |r| r.get::<_, i64>(0),
            )
            .map(|n| n != 0)
            .unwrap()
        };
        assert!(alive("copy-1"), "未删除的副本应视为存活");
        assert!(!alive("copy-2"), "已删除的副本不应视为存活");
        assert!(!alive("missing"), "不存在的副本不应视为存活");
    }
}

/// P8 内容指纹的单测（**与文件读写解耦** —— 只测纯函数，不需要真实 jsonl）。
#[cfg(test)]
mod fingerprint_tests {
    use super::*;

    fn fp(line_count: usize, prefix_hashes: Vec<u64>) -> SessionFingerprint {
        // 整份摘要由逐行 hash **确定性**折出来：内容不同 ⇒ `full_hash` 不同（与本文件的
        // 真实口径无关，测试只关心「相等 / 不相等」这一层语义）。
        let full_hash = prefix_hashes.iter().fold(FNV_OFFSET_BASIS, |state, hash| {
            fnv_step(state, &hash.to_string())
        });
        SessionFingerprint {
            line_count,
            prefix_hashes,
            full_hash,
        }
    }

    #[test]
    fn hash_line_is_stable_and_change_sensitive() {
        assert_eq!(hash_line("abc"), hash_line("abc"), "同一行必须同 hash");
        assert_ne!(
            hash_line("abc"),
            hash_line("abd"),
            "一个字节变化必须换 hash"
        );
        assert_ne!(hash_line("abc"), hash_line("abc "), "尾部空格也算变化");
    }

    #[test]
    fn common_prefix_counts_shared_lines() {
        // a 与 b 前 2 行一致 ⇒ 「共同旧版」= 2 条；a 多 3 条、b 多 1 条（**内容级 delta**）。
        let a = fp(5, vec![1, 2, 3, 4, 5]);
        let b = fp(3, vec![1, 2, 9]);
        assert_eq!(
            common_prefix_len(&a, &b),
            2,
            "前 2 行一致 ⇒ 共同旧版 = 2 条"
        );
    }

    #[test]
    fn common_prefix_zero_when_first_line_differs() {
        let a = fp(2, vec![7, 8]);
        let b = fp(2, vec![1, 2]);
        assert_eq!(common_prefix_len(&a, &b), 0, "首行即不同 ⇒ 无共同旧版");
    }

    #[test]
    fn common_prefix_is_bounded_by_shorter_side() {
        let a = fp(10, vec![1, 2, 3]);
        let b = fp(2, vec![1, 2]);
        assert_eq!(common_prefix_len(&a, &b), 2, "不得超出较短的一方");
    }

    // ---------------------------------------------------------------- P8 安全同步

    /// 建一个临时目录（会话同步用例专用；**不碰真实 home**）。
    fn sync_tmp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "buddy-switch-sync-{tag}-{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// 正向：目标是源的**严格前缀** ⇒ 只追加差量，**且目标原有字节逐字不变**。
    ///
    /// 可证伪：把实现里的 `append(true)` 换成 `write(true)`（截断重写）即变红 ——
    /// `starts_with(before)` 会失败。这正是「同步是加法」这条硬约束的护栏。
    #[test]
    fn sync_appends_only_the_delta_and_leaves_existing_bytes_untouched() {
        let dir = sync_tmp_dir("append");
        let src = dir.join("src.jsonl");
        let dst = dir.join("dst.jsonl");

        std::fs::write(&src, "{\"a\":1}\n{\"a\":2}\n{\"a\":3}\n{\"a\":4}\n").unwrap();
        std::fs::write(&dst, "{\"a\":1}\n{\"a\":2}\n").unwrap();
        let before = std::fs::read(&dst).unwrap();

        let report = append_delta_between_files(&src, &dst, "cid-a", "cid-a").unwrap();
        assert_eq!(report.appended_lines, 2, "只该追加后 2 行");
        assert_eq!(report.base_lines, 2);
        assert_eq!(report.total_lines, 4);

        let after = std::fs::read(&dst).unwrap();
        assert!(
            after.starts_with(&before),
            "目标原有字节必须逐字不变（加法语义）"
        );
        assert_eq!(
            String::from_utf8(after).unwrap(),
            "{\"a\":1}\n{\"a\":2}\n{\"a\":3}\n{\"a\":4}\n"
        );

        std::fs::remove_dir_all(dir).unwrap();
    }

    /// ★★ 分歧必须 fail-closed：**报错，且一个字节都不写**。
    ///
    /// 这是本功能最重要的一条 —— 「猜一个合并策略」会静默毁掉用户数据。
    /// 可证伪：删掉实现里 `if common < dst_lines.len()` 那个分支，本用例即红。
    #[test]
    fn sync_refuses_on_divergence_and_writes_nothing() {
        let dir = sync_tmp_dir("diverge");
        let src = dir.join("src.jsonl");
        let dst = dir.join("dst.jsonl");

        // 前 2 行相同，第 3 行**各自不同** ⇒ 有分歧。
        std::fs::write(&src, "{\"a\":1}\n{\"a\":2}\nsrc-only\n").unwrap();
        std::fs::write(&dst, "{\"a\":1}\n{\"a\":2}\ndst-only\n").unwrap();
        let before = std::fs::read(&dst).unwrap();

        let error = append_delta_between_files(&src, &dst, "cid-a", "cid-a")
            .expect_err("有分歧时必须拒绝，不得猜合并策略");
        assert!(error.contains("各自有改动"), "文案应指明是分歧：{error}");

        assert_eq!(
            std::fs::read(&dst).unwrap(),
            before,
            "拒绝时目标文件必须一个字节都没变"
        );

        std::fs::remove_dir_all(dir).unwrap();
    }

    /// 已一致 ⇒ 无事可做，`appendedLines = 0`，且**不写**。
    ///
    /// 同时覆盖「目标比源多」（目标更新）—— 那种情况同样不该写。
    #[test]
    fn sync_is_a_noop_when_target_is_not_behind() {
        let dir = sync_tmp_dir("noop");
        let src = dir.join("src.jsonl");
        let dst = dir.join("dst.jsonl");

        std::fs::write(&src, "{\"a\":1}\n{\"a\":2}\n").unwrap();
        std::fs::write(&dst, "{\"a\":1}\n{\"a\":2}\n").unwrap();
        let report = append_delta_between_files(&src, &dst, "cid-a", "cid-a").unwrap();
        assert_eq!(report.appended_lines, 0, "已一致时不该追加");
        assert_eq!(std::fs::read(&dst).unwrap(), b"{\"a\":1}\n{\"a\":2}\n");

        // 目标比源多（目标更新）⇒ 同样是 0，且不截断目标。
        std::fs::write(&src, "{\"a\":1}\n").unwrap();
        let report = append_delta_between_files(&src, &dst, "cid-a", "cid-a").unwrap();
        assert_eq!(report.appended_lines, 0, "目标不落后时不该追加");
        assert_eq!(
            std::fs::read(&dst).unwrap(),
            b"{\"a\":1}\n{\"a\":2}\n",
            "目标更新时更不许动它"
        );

        std::fs::remove_dir_all(dir).unwrap();
    }

    /// 追加的行里要把**源 cid** 换成**目标 cid**，且**只改追加的那部分**。
    ///
    /// 可证伪：去掉实现里的 `delta.replace(src_cid, dst_cid)` 即变红；
    /// 若改成对**整份文件**做 replace（而不是只对 delta），第一行也会被改写 ⇒ 红。
    #[test]
    fn sync_rewrites_source_cid_only_in_appended_lines() {
        let dir = sync_tmp_dir("rewrite");
        let src = dir.join("src.jsonl");
        let dst = dir.join("dst.jsonl");

        std::fs::write(&src, "{\"id\":\"SRC\"}\n{\"id\":\"SRC\",\"n\":2}\n").unwrap();
        std::fs::write(&dst, "{\"id\":\"DST\"}\n").unwrap();

        append_delta_between_files(&src, &dst, "SRC", "DST").unwrap();
        assert_eq!(
            String::from_utf8(std::fs::read(&dst).unwrap()).unwrap(),
            "{\"id\":\"DST\"}\n{\"id\":\"DST\",\"n\":2}\n",
            "追加行的 SRC 应改成 DST，且原有首行不受影响"
        );

        std::fs::remove_dir_all(dir).unwrap();
    }

    /// 目标末行没有换行符时，必须先补 `\n` —— 否则新记录与旧记录粘成一行，JSON 即坏。
    ///
    /// 可证伪：删掉实现里补 `\n` 的那段，断言里的行数/内容即红。
    #[test]
    fn sync_repairs_a_target_that_lacks_a_trailing_newline() {
        let dir = sync_tmp_dir("nonl");
        let src = dir.join("src.jsonl");
        let dst = dir.join("dst.jsonl");

        std::fs::write(&src, "{\"a\":1}\n{\"a\":2}\n").unwrap();
        std::fs::write(&dst, "{\"a\":1}").unwrap(); // ← 末行无换行

        append_delta_between_files(&src, &dst, "cid-a", "cid-a").unwrap();
        assert_eq!(
            String::from_utf8(std::fs::read(&dst).unwrap()).unwrap(),
            "{\"a\":1}\n{\"a\":2}\n",
            "补上换行后应成为两条合法记录，而不是粘成一行"
        );

        std::fs::remove_dir_all(dir).unwrap();
    }

    /// 源 = 目标（同一路径）⇒ 直接拒绝，绝不自己追加给自己。
    #[test]
    fn sync_refuses_when_source_and_target_are_the_same_file() {
        let dir = sync_tmp_dir("same");
        let only = dir.join("only.jsonl");
        std::fs::write(&only, "{\"a\":1}\n").unwrap();

        let error = append_delta_between_files(&only, &only, "cid-a", "cid-a")
            .expect_err("同一文件必须拒绝");
        assert!(error.contains("同一个文件"), "文案应指明原因：{error}");
        assert_eq!(std::fs::read(&only).unwrap(), b"{\"a\":1}\n");

        std::fs::remove_dir_all(dir).unwrap();
    }

    /// ★★ 指纹必须**按内容**可比，不能把「各自的 sessionId」算进去。
    ///
    /// 现场（实测）：真实会话 jsonl 的**几乎每一行**都带 `"sessionId":"<cid>"` ——
    /// 一份 15 行的会话里 14 行含该 cid。而副本是**另一个 cid**
    /// （复制时整份 `text.replace(cid, new_cid)`）。
    /// ⇒ 若按原始行 hash，两个**内容完全一致**的副本公共前缀恒为 0 ⇒
    /// `derive_content_status` 会把**每个组都判成 `diverge`**（假分歧）。
    ///
    /// 可证伪：把 `fingerprint_of_text` 里那行按需归一化（`line.replace(cid, …)`）去掉
    /// （改回直接 hash 原始行），本用例即红。
    #[test]
    fn fingerprint_ignores_the_replicas_own_session_id() {
        let dir = sync_tmp_dir("fp-id");
        let a = dir.join("a.jsonl");
        let b = dir.join("b.jsonl");
        let cid_a = "aaaaaaaa-1111-2222-3333-444444444444";
        let cid_b = "bbbbbbbb-5555-6666-7777-888888888888";

        // 同一份内容，只有各自的 sessionId 不同（模拟「复制出来的副本」）。
        let body = |cid: &str| {
            format!(
                "{{\"type\":\"message\",\"sessionId\":\"{cid}\",\"text\":\"hello\"}}\n\
                 {{\"type\":\"message\",\"sessionId\":\"{cid}\",\"text\":\"world\"}}\n"
            )
        };
        std::fs::write(&a, body(cid_a)).unwrap();
        std::fs::write(&b, body(cid_b)).unwrap();

        let fa = fingerprint_of_file(&a, cid_a).unwrap();
        let fb = fingerprint_of_file(&b, cid_b).unwrap();
        assert_eq!(fa.line_count, 2);
        assert_eq!(
            common_prefix_len(&fa, &fb),
            2,
            "内容一致的两个副本必须**整段**可比 —— 否则组状态恒为 diverge（假分歧）"
        );

        // 阳性对照：内容真的不同 ⇒ 必须**不**判成一致（证明上面的 2 不是因为「hash 恒等」）。
        std::fs::write(&b, body(cid_b).replace("world", "WORLD")).unwrap();
        let fb2 = fingerprint_of_file(&b, cid_b).unwrap();
        assert_eq!(common_prefix_len(&fa, &fb2), 1, "内容变化必须被指纹看见");

        std::fs::remove_dir_all(dir).unwrap();
    }

    /// 建一个「隔离 home + 两个真实会话 jsonl」的现场（`projects/ws/{src,dst}.jsonl`）。
    ///
    /// ⚠️ 必须走 `HomeOverrideGuard`：`session_data_dir` 是**无参全局路径函数**，
    /// 不钉住 home 就会读到真实 `~/.workbuddy`（本仓踩过四次）。
    /// ⚠️ **不要**再手动 `env_lock()` —— `set()` 内部已取，不可重入会自锁死。
    fn sync_fixture(
        tag: &str,
        src_body: &str,
        dst_body: &str,
    ) -> (
        PathBuf,
        crate::modules::config::HomeOverrideGuard,
        &'static str,
    ) {
        let home = std::env::temp_dir().join(format!(
            "buddy-switch-batch-{tag}-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let projects = home.join(".workbuddy").join("projects").join("ws");
        std::fs::create_dir_all(&projects).unwrap();
        std::fs::write(projects.join("src-cid.jsonl"), src_body).unwrap();
        std::fs::write(projects.join("dst-cid.jsonl"), dst_body).unwrap();
        let guard = crate::modules::config::HomeOverrideGuard::set(&home);
        (home, guard, "ws")
    }

    /// ★★ 批量：**一条失败不得让其余白跑**（部分失败是 `PLAN.md` §3.6 的硬要求）。
    ///
    /// 可证伪：把 `sync_session_replicas_for` 里的 `Err` 分支改成 `return`（整批中止），
    /// 则 `okCount` 会变 0 ⇒ 红。
    #[test]
    fn batch_sync_reports_partial_failure_without_aborting_the_rest() {
        // 源 3 行；目标 1 行（落后，可同步）；另有「分歧目标」。
        let (home, guard, ws) = sync_fixture(
            "partial",
            "{\"n\":1}\n{\"n\":2}\n{\"n\":3}\n",
            "{\"n\":1}\n",
        );
        // 造一个与源**分歧**的目标（第 2 行不同）。
        let projects = home.join(".workbuddy").join("projects").join(ws);
        std::fs::write(projects.join("div-cid.jsonl"), "{\"n\":1}\n{\"n\":9}\n").unwrap();
        let div_before = std::fs::read(projects.join("div-cid.jsonl")).unwrap();

        let report = sync_session_replicas_for(
            Region::Cn,
            &[
                json!({ "sourceCid": "src-cid", "targetCid": "dst-cid" }),
                json!({ "sourceCid": "src-cid", "targetCid": "div-cid" }),
            ],
        );

        assert_eq!(report["okCount"], json!(1), "落后那条应当成功");
        assert_eq!(report["failCount"], json!(1), "分歧那条应当失败");
        assert_eq!(report["results"][0]["ok"], json!(true));
        assert_eq!(
            report["results"][0]["appendedLines"],
            json!(2),
            "源 3 行 − 目标 1 行 = 追加 2 行"
        );
        assert_eq!(report["results"][1]["ok"], json!(false));
        assert!(
            report["results"][1]["error"]
                .as_str()
                .unwrap_or_default()
                .contains("各自有改动"),
            "失败原因要如实说明是分歧：{}",
            report["results"][1]["error"]
        );

        // 真的写进去了，且分歧那条**一个字节都没动**。
        assert_eq!(
            String::from_utf8(std::fs::read(projects.join("dst-cid.jsonl")).unwrap()).unwrap(),
            "{\"n\":1}\n{\"n\":2}\n{\"n\":3}\n"
        );
        assert_eq!(
            std::fs::read(projects.join("div-cid.jsonl")).unwrap(),
            div_before
        );

        drop(guard);
        std::fs::remove_dir_all(home).unwrap();
    }

    /// 空 id ⇒ 该条直接判失败，**不进**文件层（也不影响同批其它条）。
    #[test]
    fn batch_sync_rejects_empty_ids() {
        let (home, guard, _ws) = sync_fixture("empty-id", "{\"n\":1}\n{\"n\":2}\n", "{\"n\":1}\n");

        let report = sync_session_replicas_for(
            Region::Cn,
            &[
                json!({ "sourceCid": "", "targetCid": "dst-cid" }),
                json!({ "sourceCid": "src-cid", "targetCid": "   " }),
                json!({ "sourceCid": "src-cid", "targetCid": "dst-cid" }),
            ],
        );

        assert_eq!(report["okCount"], json!(1), "只有第三条该成功");
        assert_eq!(report["failCount"], json!(2));
        assert!(report["results"][0]["error"]
            .as_str()
            .unwrap_or_default()
            .contains("不能为空"));
        assert!(report["results"][1]["error"]
            .as_str()
            .unwrap_or_default()
            .contains("不能为空"));

        drop(guard);
        std::fs::remove_dir_all(home).unwrap();
    }

    // ---------------------------------------------------------------- P14 S1 乐观令牌

    /// 在 [`sync_fixture`] 之上再写一份**账本**（组结构 = 一条边 `uid-a␟src-cid → uid-b␟dst-cid`）。
    ///
    /// 账本路径 = `<home>/.workbuddy/buddy-switch-copy-ledger.json` —— 与生产路径
    /// [`copy_ledger_path_for`] **同一个**，靠 `HomeOverrideGuard` 重定向，**不碰真实 home**。
    fn sync_fixture_with_ledger(
        tag: &str,
        src_body: &str,
        dst_body: &str,
    ) -> (PathBuf, crate::modules::config::HomeOverrideGuard, PathBuf) {
        let (home, guard, _ws) = sync_fixture(tag, src_body, dst_body);
        let ledger_path = home.join(".workbuddy").join(COPY_LEDGER_FILE);
        let ledger = json!({
            copy_ledger_key("uid-a", "uid-b", "src-cid"): "dst-cid",
        });
        std::fs::write(&ledger_path, serde_json::to_string(&ledger).unwrap()).unwrap();
        (home, guard, ledger_path)
    }

    /// ★ P14 S1：组指纹的三条契约 —— **顺序无关** / **重复顶点折叠** / **成员变了就变**。
    ///
    /// 可证伪：删掉 `group_fingerprint` 里的 `keys.sort(); keys.dedup();` ⇒ 第一条红；
    /// 把成员内容排除出 hash（改成返回常量）⇒ 第三条红。
    #[test]
    fn group_fingerprint_contract() {
        let one = vec![("uid-a".to_string(), "cid-1".to_string())];
        let mut two = one.clone();
        two.push(("uid-b".to_string(), "cid-2".to_string()));

        // ① 顺序无关：账本的插入顺序不该改变指纹（调用方可能从任意顺序的 map 迭代）。
        let mut reversed = two.clone();
        reversed.reverse();
        assert_eq!(
            group_fingerprint(&two),
            group_fingerprint(&reversed),
            "成员顺序不该改变指纹"
        );

        // ② 重复顶点折叠：同一个副本登记两次 = 一个副本。
        let mut duplicated = one.clone();
        duplicated.push(one[0].clone());
        assert_eq!(group_fingerprint(&one), group_fingerprint(&duplicated));

        // ③ 阳性对照：成员变了 ⇒ 必须变（证明上面的相等不是因为「恒返回常量」）。
        assert_ne!(
            group_fingerprint(&one),
            group_fingerprint(&two),
            "多一个成员必须改变指纹"
        );
        assert_ne!(
            group_fingerprint(&[]),
            group_fingerprint(&one),
            "空组与单成员组不该同指纹"
        );
    }

    /// ★ P14 S1：**指纹算法冻结** —— 钉住具体输出值。
    ///
    /// 为什么值得钉两个「魔法值」：演示夹具（`src/lib/screenshot-demo.ts` 的
    /// `demoSessionReplicas`）里必须放**真实算法算出来的**令牌 —— 本仓红线是
    /// 「演示数据不许自造结构」（`GAP-ANALYSIS §4.2`），随便编一个 hex 会让演示站
    /// 与真机走两套形状。那两个常量是照本算法手算的 ⇒ 在这里钉住：
    /// 算法一改，演示夹具就会**静默**失真，这条断言就是那个警报。
    ///
    /// 可证伪：改 `group_fingerprint` 的任何一步（少长度前缀 / 不排序 / 换初始值 /
    /// 不折叠重复）⇒ 立刻红。
    #[test]
    fn group_fingerprint_values_are_frozen_for_demo_fixtures() {
        let group1 = vec![
            ("demo-user-001".to_string(), "demo-session-1".to_string()),
            (
                "demo-user-002".to_string(),
                "demo-session-1-copy".to_string(),
            ),
        ];
        let group2 = vec![
            ("demo-user-001".to_string(), "demo-session-2".to_string()),
            (
                "demo-user-003".to_string(),
                "demo-session-2-copy".to_string(),
            ),
        ];
        assert_eq!(group_fingerprint(&group1), "71186784553e0e0b");
        assert_eq!(group_fingerprint(&group2), "7f64fb4bd44b030c");
    }

    /// ★ P14 S1：**出给前端的令牌**与**写入口复核用的指纹**必须同源。
    ///
    /// 这是本切片最容易踩的坑：两边各算一份组结构 ⇒ 前端拿到一个**永远过不了**的令牌
    /// （表现为「每次点同步都提示请刷新」，极难查）。
    ///
    /// 可证伪：让 `replica_groups_from_ledger` 自己重算一遍成员（不走
    /// [`replica_group_members_from_ledger`]）⇒ 这里立刻红。
    #[test]
    fn group_token_matches_the_writer_side_fingerprint() {
        let dir = std::env::temp_dir().join(format!(
            "buddy_switch_group_token_{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(COPY_LEDGER_FILE);
        let ledger = json!({
            copy_ledger_key("uid-a", "uid-b", "cid-1"): "new-1",
            copy_ledger_key("uid-a", "uid-c", "cid-1"): "new-2",
        });
        std::fs::write(&path, serde_json::to_string(&ledger).unwrap()).unwrap();

        let out = list_replica_groups_at(Region::Cn, Some(&path));
        let group = &out["groups"][0];
        let members: Vec<(String, String)> = vec![
            ("uid-a".to_string(), "cid-1".to_string()),
            ("uid-b".to_string(), "new-1".to_string()),
            ("uid-c".to_string(), "new-2".to_string()),
        ];
        let expected = group_fingerprint(&members);
        assert_eq!(
            group["previewToken"].as_str(),
            Some(expected.as_str()),
            "出给前端的令牌必须与写入口的指纹同源"
        );

        std::fs::remove_dir_all(dir).unwrap();
    }

    /// ★★ P14 S1：令牌不符 ⇒ 该条失败，且目标**一个字节都没写**；换新令牌则通过。
    ///
    /// 可证伪：把 `sync_session_replicas_for` 里的 `verify_preview_token` 调用删掉
    /// ⇒ `okCount` 变成 1、目标被追加 2 行 ⇒ 红。
    #[test]
    fn stale_preview_token_is_rejected_without_writing_a_byte() {
        let (home, guard, ledger_path) = sync_fixture_with_ledger(
            "token-stale",
            "{\"n\":1}\n{\"n\":2}\n{\"n\":3}\n",
            "{\"n\":1}\n",
        );
        // ① 按**当前**账本取令牌 —— 这就是前端此刻会拿到的那个。
        let groups = list_replica_groups_at(Region::Cn, Some(&ledger_path));
        let group_key = groups["groups"][0]["key"].as_str().unwrap().to_string();
        let stale_token = groups["groups"][0]["previewToken"]
            .as_str()
            .unwrap()
            .to_string();

        // ② 账本变了：同一源会话又多复制到一个账号 ⇒ 组里多一个成员。
        let ledger = json!({
            copy_ledger_key("uid-a", "uid-b", "src-cid"): "dst-cid",
            copy_ledger_key("uid-a", "uid-c", "src-cid"): "dst-cid-2",
        });
        std::fs::write(&ledger_path, serde_json::to_string(&ledger).unwrap()).unwrap();

        let projects = home.join(".workbuddy").join("projects").join("ws");
        let before = std::fs::read(projects.join("dst-cid.jsonl")).unwrap();

        // ③ 拿**旧**令牌提交 ⇒ 必须被拒，且目标逐字节未变。
        let report = sync_session_replicas_for(
            Region::Cn,
            &[json!({
                "sourceCid": "src-cid",
                "targetCid": "dst-cid",
                "groupKey": group_key,
                "previewToken": stale_token,
            })],
        );
        assert_eq!(report["okCount"], json!(0), "陈旧令牌必须被拒");
        assert_eq!(report["failCount"], json!(1));
        assert!(
            report["results"][0]["error"]
                .as_str()
                .unwrap_or_default()
                .contains("已被其他操作修改"),
            "失败原因要如实说明是组变了：{}",
            report["results"][0]["error"]
        );
        assert_eq!(
            std::fs::read(projects.join("dst-cid.jsonl")).unwrap(),
            before,
            "被拒的条目**一个字节都不许写**"
        );

        // ④ 阳性对照：拿**新**令牌提交 ⇒ 必须通过并真的追加（证明拒绝是因为令牌，不是别的）。
        let fresh = list_replica_groups_at(Region::Cn, Some(&ledger_path));
        let fresh_token = fresh["groups"][0]["previewToken"]
            .as_str()
            .unwrap()
            .to_string();
        assert_ne!(fresh_token, stale_token, "组变了，指纹必须跟着变");
        let report = sync_session_replicas_for(
            Region::Cn,
            &[json!({
                "sourceCid": "src-cid",
                "targetCid": "dst-cid",
                "groupKey": fresh["groups"][0]["key"].as_str().unwrap(),
                "previewToken": fresh_token,
            })],
        );
        assert_eq!(report["okCount"], json!(1), "新令牌必须通过");
        assert_eq!(report["results"][0]["appendedLines"], json!(2));

        drop(guard);
        std::fs::remove_dir_all(home).unwrap();
    }

    /// ★ P14 S1：令牌**复核在备份之前** —— 否则「拒绝」也会留下一个备份目录（副作用）。
    ///
    /// 可证伪：把 `unify_session_replicas_for` 里的 `verify_preview_token` 挪到
    /// `unify_replica_between_files` 之后 ⇒ `sessions-unify` 目录会出现 ⇒ 红。
    #[test]
    fn stale_preview_token_is_rejected_before_any_backup() {
        let (home, guard, ledger_path) = sync_fixture_with_ledger(
            "token-unify",
            "{\"id\":\"SRC\",\"n\":1}\n{\"id\":\"SRC\",\"n\":2}\n",
            "{\"id\":\"DST\",\"n\":1}\n{\"id\":\"DST\",\"n\":9}\n",
        );
        let groups = list_replica_groups_at(Region::Cn, Some(&ledger_path));
        let group_key = groups["groups"][0]["key"].as_str().unwrap().to_string();

        let projects = home.join(".workbuddy").join("projects").join("ws");
        let before = std::fs::read(projects.join("dst-cid.jsonl")).unwrap();

        let report = unify_session_replicas_for(
            Region::Cn,
            &[json!({
                "sourceCid": "src-cid",
                "targetCid": "dst-cid",
                "groupKey": group_key,
                "previewToken": "0000000000000000",
            })],
        );
        assert_eq!(report["okCount"], json!(0), "伪造令牌必须被拒");
        assert_eq!(
            std::fs::read(projects.join("dst-cid.jsonl")).unwrap(),
            before,
            "被拒的分歧统一**一个字节都不许写**"
        );
        assert!(
            !home
                .join(".buddy-switch")
                .join("backups")
                .join("sessions-unify")
                .exists(),
            "被拒的条目不该留下任何备份目录（复核必须在备份之前）"
        );

        drop(guard);
        std::fs::remove_dir_all(home).unwrap();
    }

    /// ★ P14 S1：**未携带令牌 ⇒ 退化为今天的行为**（令牌是可选参数，见 P14-TASKS S1 回滚口径）。
    ///
    /// 可证伪：把 `verify_preview_token` 改成「没有令牌就报错」⇒ 这里红。
    #[test]
    fn missing_preview_token_falls_back_to_previous_behavior() {
        let (home, guard, _ledger) = sync_fixture_with_ledger(
            "token-absent",
            "{\"n\":1}\n{\"n\":2}\n{\"n\":3}\n",
            "{\"n\":1}\n",
        );
        let report = sync_session_replicas_for(
            Region::Cn,
            &[json!({ "sourceCid": "src-cid", "targetCid": "dst-cid" })],
        );
        assert_eq!(report["okCount"], json!(1), "不带令牌 ⇒ 与旧行为一致");

        drop(guard);
        std::fs::remove_dir_all(home).unwrap();
    }

    /// ★ P14 S1：组键在账本里找不到（组被删了 / 被别的操作改写）⇒ 拒绝，不写字节。
    #[test]
    fn unknown_group_key_is_rejected() {
        let (home, guard, _ledger) = sync_fixture_with_ledger(
            "token-unknown",
            "{\"n\":1}\n{\"n\":2}\n{\"n\":3}\n",
            "{\"n\":1}\n",
        );
        let projects = home.join(".workbuddy").join("projects").join("ws");
        let before = std::fs::read(projects.join("dst-cid.jsonl")).unwrap();

        let report = sync_session_replicas_for(
            Region::Cn,
            &[json!({
                "sourceCid": "src-cid",
                "targetCid": "dst-cid",
                "groupKey": "uid-z\u{1f}cid-z",
                "previewToken": "0123456789abcdef",
            })],
        );
        assert_eq!(report["okCount"], json!(0));
        assert!(
            report["results"][0]["error"]
                .as_str()
                .unwrap_or_default()
                .contains("已不存在"),
            "失败原因要说明组没了：{}",
            report["results"][0]["error"]
        );
        assert_eq!(
            std::fs::read(projects.join("dst-cid.jsonl")).unwrap(),
            before
        );

        drop(guard);
        std::fs::remove_dir_all(home).unwrap();
    }

    // ---------------------------------------------------------------- P14 S2 基线对

    /// ★ P14 S2：基线键**与方向无关**，且能解析回两端。
    ///
    /// 可证伪：把 `baseline_key` 里的 `if a <= b` 去掉 ⇒ 第一条红。
    #[test]
    fn baseline_key_is_direction_agnostic() {
        assert_eq!(
            baseline_key("cid-a", "cid-b"),
            baseline_key("cid-b", "cid-a")
        );
        let key = baseline_key("cid-b", "cid-a");
        assert_eq!(
            parse_baseline_key(&key),
            Some(("cid-a".to_string(), "cid-b".to_string())),
            "解析必须回到**有序**的两端"
        );
        // 结构不符 / 段数不对 ⇒ None（当作没有基线，而不是猜）。
        assert_eq!(parse_baseline_key(""), None);
        assert_eq!(parse_baseline_key("only-one"), None);
        assert_eq!(
            parse_baseline_key(&format!("a{BASELINE_SEP}b{BASELINE_SEP}c")),
            None
        );
    }

    /// ★ P14 S2：基线只挂到**两端成员都在组内**的组上。
    ///
    /// 可证伪：把 `replica_groups_from_ledger` 里的双端判定改成只判一端
    /// ⇒ 「组 B 不该挂上别人的基线」红。
    #[test]
    fn group_baselines_are_scoped_to_the_group() {
        let dir = std::env::temp_dir().join(format!(
            "buddy_switch_group_baselines_{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let ledger_path = dir.join(COPY_LEDGER_FILE);
        let ledger = json!({
            copy_ledger_key("uid-a", "uid-b", "cid-1"): "new-1",
            copy_ledger_key("uid-x", "uid-y", "cid-9"): "new-9",
        });
        std::fs::write(&ledger_path, serde_json::to_string(&ledger).unwrap()).unwrap();

        let mut baselines = serde_json::Map::new();
        // 组 A 内部的一对。
        baselines.insert(
            baseline_key("cid-1", "new-1"),
            json!({ "createdAt": 200, "lineCount": 12, "prefixHashes": [1, 2] }),
        );
        // **跨组**的一对（一端在 A、一端在 B）⇒ 两边都不该挂。
        baselines.insert(
            baseline_key("new-1", "cid-9"),
            json!({ "createdAt": 300, "lineCount": 99, "prefixHashes": [] }),
        );
        std::fs::write(
            dir.join(SESSION_BASELINE_FILE),
            serde_json::to_string(&baselines).unwrap(),
        )
        .unwrap();

        let out = list_replica_groups_at(Region::Cn, Some(&ledger_path));
        let groups = out["groups"].as_array().unwrap();
        assert_eq!(groups.len(), 2);

        let group_a = groups
            .iter()
            .find(|g| g["key"].as_str().unwrap().contains("cid-1"))
            .expect("组 A");
        // ★ 组上只出**最近一次**一致点（前端不用再自己挑；逐行 hash 不外泄）。
        assert_eq!(group_a["baseline"]["lineCount"], json!(12));
        assert_eq!(group_a["baseline"]["a"], json!("cid-1"));
        assert_eq!(group_a["baseline"]["b"], json!("new-1"));
        assert_eq!(group_a["baseline"]["createdAt"], json!(200));
        // ⚠️ 逐行 hash **不进响应**（那是 200 MB 级的历史包袱，校验在 core 做）。
        assert!(
            group_a["baseline"].get("prefixHashes").is_none(),
            "组响应里不该出现 prefixHashes"
        );

        let group_b = groups
            .iter()
            .find(|g| g["key"].as_str().unwrap().contains("cid-9"))
            .expect("组 B");
        assert!(
            group_b["baseline"].is_null(),
            "组 B 不该挂上别人的基线（跨组的那对两端不都在组内）"
        );

        std::fs::remove_dir_all(dir).unwrap();
    }

    /// ★ P14 S2：基线文件损坏 ⇒ 视为**没有基线**（不臆造一致点），且**不影响**副本组本身。
    #[test]
    fn corrupt_baseline_file_is_treated_as_no_baseline() {
        let dir = std::env::temp_dir().join(format!(
            "buddy_switch_baselines_corrupt_{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let ledger_path = dir.join(COPY_LEDGER_FILE);
        let ledger = json!({ copy_ledger_key("uid-a", "uid-b", "cid-1"): "new-1" });
        std::fs::write(&ledger_path, serde_json::to_string(&ledger).unwrap()).unwrap();

        for payload in ["not-json", "[1,2,3]", "\"text\"", "null", ""] {
            std::fs::write(dir.join(SESSION_BASELINE_FILE), payload).unwrap();
            let out = list_replica_groups_at(Region::Cn, Some(&ledger_path));
            let groups = out["groups"].as_array().unwrap();
            assert_eq!(groups.len(), 1, "损坏的基线不该影响副本组：{payload}");
            assert!(
                groups[0]["baseline"].is_null(),
                "损坏的基线必须视为「没有基线」：{payload}"
            );
        }

        std::fs::remove_dir_all(dir).unwrap();
    }

    /// ★★ P14 S2：**分歧统一成功**才刷新一致点；**被拒** ⇒ 一个字节不写、也**不建**基线。
    ///
    /// 可证伪：把 `record_pair_baseline_for` 调用挪到令牌复核**之前**（或删掉
    /// `if report.overwritten` 判断）⇒ 第一条断言红。
    #[test]
    fn unify_records_a_baseline_only_when_it_really_overwrote() {
        let (home, guard, ledger_path) = sync_fixture_with_ledger(
            "baseline-unify",
            "{\"id\":\"SRC\",\"n\":1}\n{\"id\":\"SRC\",\"n\":2}\n",
            "{\"id\":\"DST\",\"n\":1}\n{\"id\":\"DST\",\"n\":9}\n",
        );
        let baseline_path = home.join(".workbuddy").join(SESSION_BASELINE_FILE);
        let groups = list_replica_groups_at(Region::Cn, Some(&ledger_path));
        let group_key = groups["groups"][0]["key"].as_str().unwrap().to_string();

        // ① 被拒（伪造令牌）⇒ 不建基线。
        let report = unify_session_replicas_for(
            Region::Cn,
            &[json!({
                "sourceCid": "src-cid",
                "targetCid": "dst-cid",
                "groupKey": group_key,
                "previewToken": "0000000000000000",
            })],
        );
        assert_eq!(report["okCount"], json!(0));
        assert!(
            load_session_baselines_at(&baseline_path).is_empty(),
            "被拒的操作不该留下任何基线（否则会污染「上次一致点」）"
        );

        // ② 正常统一 ⇒ 建一条基线，行数 = 源的行数。
        let token = groups["groups"][0]["previewToken"]
            .as_str()
            .unwrap()
            .to_string();
        let report = unify_session_replicas_for(
            Region::Cn,
            &[json!({
                "sourceCid": "src-cid",
                "targetCid": "dst-cid",
                "groupKey": group_key,
                "previewToken": token,
            })],
        );
        assert_eq!(report["okCount"], json!(1));
        assert_eq!(report["results"][0]["overwritten"], json!(true));

        let baselines = load_session_baselines_at(&baseline_path);
        let entry = baselines
            .get(&baseline_key("src-cid", "dst-cid"))
            .expect("统一成功应留下一条基线");
        let parsed: PairBaseline = serde_json::from_value(entry.clone()).expect("基线结构");
        assert_eq!(parsed.line_count, 2, "一致点 = 源的行数");

        // ③ 再统一一次：此时两份已一致 ⇒ `overwritten == false` ⇒ **不刷新**一致点
        //    （否则会把更早的、更有信息量的一致点覆盖掉）。
        let report = unify_session_replicas_for(
            Region::Cn,
            &[json!({
                "sourceCid": "src-cid",
                "targetCid": "dst-cid",
                "groupKey": group_key,
                "previewToken": token,
            })],
        );
        assert_eq!(report["results"][0]["overwritten"], json!(false));
        let after = load_session_baselines_at(&baseline_path);
        assert_eq!(
            after.get(&baseline_key("src-cid", "dst-cid")).unwrap()["createdAt"],
            entry["createdAt"],
            "「本来就一致」不是一次新的会合 ⇒ 一致点不该被刷新"
        );

        drop(guard);
        std::fs::remove_dir_all(home).unwrap();
    }

    // ---------------------------------------------------------------- P8 分歧处理

    /// ★ 分歧处理：把选定的一份统一到目标 ⇒ **覆盖**，且如实报出「丢了多少条」+ 备份原文件。
    ///
    /// 可证伪：把 `unify_replica_between_files` 改成「追加」（复用 `append`）即红 ——
    /// 分歧两边各自都有对方没有的内容，追加**永远**合不到一起，`dst == src` 断言必失败。
    #[test]
    fn unify_overwrites_target_and_reports_what_is_lost() {
        let dir = sync_tmp_dir("unify");
        let src = dir.join("src.jsonl");
        let dst = dir.join("dst.jsonl");
        let backup = dir.join("backup");

        // 前 1 行相同，第 2 行**各自不同** ⇒ 真分歧。
        std::fs::write(&src, "{\"id\":\"SRC\",\"n\":1}\n{\"id\":\"SRC\",\"n\":2}\n").unwrap();
        std::fs::write(&dst, "{\"id\":\"DST\",\"n\":1}\n{\"id\":\"DST\",\"n\":9}\n").unwrap();

        let report = unify_replica_between_files(&src, &dst, "SRC", "DST", &backup).unwrap();
        assert!(report.overwritten, "分歧必须真的覆盖");
        assert_eq!(report.previous_lines, 2);
        assert_eq!(report.total_lines, 2);
        assert_eq!(report.lost_lines, 1, "目标第 2 行是它独有的 ⇒ 丢 1 条");

        // 目标内容 = 源内容（且 id 已归一化成目标自己的 id）。
        assert_eq!(
            String::from_utf8(std::fs::read(&dst).unwrap()).unwrap(),
            "{\"id\":\"DST\",\"n\":1}\n{\"id\":\"DST\",\"n\":2}\n",
            "覆盖后目标应与选定的一份一致，且 id 归一化"
        );

        // ★ 备份必须**真的**存在，且内容是**覆盖前**的目标原文（否则「可回滚」是假的）。
        let backup_path = std::path::PathBuf::from(report.backup.clone().expect("应给出备份路径"));
        assert!(backup_path.is_file(), "备份文件不存在：{backup_path:?}");
        assert_eq!(
            String::from_utf8(std::fs::read(&backup_path).unwrap()).unwrap(),
            "{\"id\":\"DST\",\"n\":1}\n{\"id\":\"DST\",\"n\":9}\n",
            "备份必须是覆盖前的原文"
        );

        std::fs::remove_dir_all(dir).unwrap();
    }

    /// 已一致 ⇒ 不写盘、不备份（`overwritten: false`，`backup: None`）。
    #[test]
    fn unify_is_a_noop_when_already_consistent() {
        let dir = sync_tmp_dir("unify-noop");
        let src = dir.join("src.jsonl");
        let dst = dir.join("dst.jsonl");
        let backup = dir.join("backup");

        std::fs::write(&src, "{\"id\":\"A\",\"n\":1}\n{\"id\":\"A\",\"n\":2}\n").unwrap();
        std::fs::write(&dst, "{\"id\":\"A\",\"n\":1}\n{\"id\":\"A\",\"n\":2}\n").unwrap();
        let before = std::fs::read(&dst).unwrap();

        let report = unify_replica_between_files(&src, &dst, "A", "A", &backup).unwrap();
        assert!(!report.overwritten, "一致时不该覆盖");
        assert_eq!(report.lost_lines, 0);
        assert!(report.backup.is_none(), "一致时不该产生备份");
        assert_eq!(std::fs::read(&dst).unwrap(), before, "一致时不许动目标");
        assert!(!backup.exists(), "一致时不该建备份目录");

        std::fs::remove_dir_all(dir).unwrap();
    }

    /// ★★ **备份失败 ⇒ 绝不覆盖**（本仓「谎报有回滚点」事故的同一纪律）。
    ///
    /// 可证伪：把 `create_dir_all(...).map_err(...)?` 改成 `let _ = ...`（吞掉错误），
    /// 目标就会被覆盖 ⇒ `dst 逐字节不变` 的断言即红。
    #[test]
    fn unify_refuses_and_writes_nothing_when_backup_fails() {
        let dir = sync_tmp_dir("unify-nobackup");
        let src = dir.join("src.jsonl");
        let dst = dir.join("dst.jsonl");

        std::fs::write(&src, "{\"n\":1}\n{\"n\":2}\n").unwrap();
        std::fs::write(&dst, "{\"n\":1}\n{\"n\":9}\n").unwrap();
        let before = std::fs::read(&dst).unwrap();

        // 备份根目录的**父路径是个普通文件** ⇒ `create_dir_all` 必失败。
        let blocker = dir.join("blocker");
        std::fs::write(&blocker, b"not a dir").unwrap();

        let error = unify_replica_between_files(&src, &dst, "cid-a", "cid-a", &blocker.join("sub"))
            .expect_err("备份建不出来时必须报错");
        assert!(
            error.contains("创建备份目录失败"),
            "文案应指明原因：{error}"
        );
        assert_eq!(
            std::fs::read(&dst).unwrap(),
            before,
            "备份失败时目标必须一个字节都没变"
        );

        std::fs::remove_dir_all(dir).unwrap();
    }
}

/// ★ 会话页性能地基的单测：**索引 / 缓存 / 组状态算法**。
///
/// 全部用临时目录（**不碰真实 home**）—— 这是本仓踩过四次的坑（见模块头）。
#[cfg(test)]
mod content_status_tests {
    use super::*;

    /// 建一个临时 `projects/` 根 + 若干 jsonl（返回 `(根, 清理 guard)`）。
    fn temp_projects(tag: &str) -> (PathBuf, PathBuf) {
        let home = std::env::temp_dir().join(format!(
            "buddy-switch-status-{tag}-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let projects = home.join("projects");
        std::fs::create_dir_all(projects.join("ws-a")).unwrap();
        (home, projects)
    }

    /// 写一份会话正文（每行一条「消息」；含本会话 id —— 与真机形态对齐）。
    fn write_jsonl(projects: &Path, workspace: &str, cid: &str, lines: usize) {
        let mut body = String::new();
        for index in 0..lines {
            body.push_str(&format!(
                "{{\"type\":\"message\",\"sessionId\":\"{cid}\",\"n\":{index}}}\n"
            ));
        }
        let dir = projects.join(workspace);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(format!("{cid}.jsonl")), body).unwrap();
    }

    /// ★ 索引与「逐次查找」**逐条等价**（含「直立案优先于子目录」这条）。
    ///
    /// 可证伪：把 `build_projects_index_in` 里直立案的 `insert` 改成 `or_insert`
    /// ⇒ 子目录里的同名文件会赢 ⇒ 第二条断言红。
    #[test]
    fn projects_index_matches_per_lookup_path() {
        let (home, projects) = temp_projects("index-parity");
        write_jsonl(&projects, "ws-a", "cid-in-subdir", 3);
        // 直立案与子目录同名文件并存：**直立必须赢**（`find_project_jsonl_in` 的语义）。
        std::fs::write(
            projects.join("cid-direct.jsonl"),
            "{\"type\":\"message\"}\n",
        )
        .unwrap();
        write_jsonl(&projects, "ws-a", "cid-direct", 99);
        // 非 `.jsonl` 一律不参与（.meta.json / .file-rollback.ndjson 是边车）。
        std::fs::write(projects.join("ws-a").join("cid-meta.meta.json"), "{}").unwrap();

        let index = build_projects_index_in(&projects);
        for cid in [
            "cid-in-subdir",
            "cid-direct",
            "cid-missing",
            "cid-meta.meta",
        ] {
            assert_eq!(
                index.get(cid).cloned(),
                find_project_jsonl_in(&projects, cid),
                "索引与逐次查找必须给出同一条路径：{cid}"
            );
        }
        assert_eq!(
            index
                .get("cid-direct")
                .map(|p| p.parent().unwrap().to_path_buf()),
            Some(projects.clone()),
            "直立文件必须赢过子目录里的同名文件"
        );
        std::fs::remove_dir_all(home).unwrap();
    }

    /// ★★★ 「全体一致的公共前缀」== 「两两公共前缀的最小值」。
    ///
    /// 这是把 O(N²·512) 降成 O(N·512) 的**唯一依据**，所以必须拿**参考实现**
    /// （`common_prefix_len` 的两两版本）逐组对拍，不能只靠推导。
    ///
    /// 可证伪：把 `derive_content_status` 里「扫一遍找首个不一致」换成「只看第 0 个
    /// 成员与其他成员」之类的不完整实现 ⇒ 第三条（第 2 个成员先分叉的构造）红。
    #[test]
    fn all_agree_prefix_equals_min_pairwise_prefix() {
        // 构造：a 与 b 在前 4 行一致；c 在第 2 行就分叉（首个不一致 = 索引 2）。
        let a = SessionFingerprint {
            line_count: 9,
            prefix_hashes: vec![1, 2, 3, 4, 5],
            full_hash: 0,
        };
        let b = SessionFingerprint {
            line_count: 7,
            prefix_hashes: vec![1, 2, 3, 4, 6],
            full_hash: 0,
        };
        let c = SessionFingerprint {
            line_count: 6,
            prefix_hashes: vec![1, 2, 9, 9, 9],
            full_hash: 0,
        };
        let fps = vec![Some(a.clone()), Some(b.clone()), Some(c.clone())];

        let reference = [
            common_prefix_len(&a, &b),
            common_prefix_len(&a, &c),
            common_prefix_len(&b, &c),
        ]
        .into_iter()
        .min()
        .unwrap();
        assert_eq!(reference, 2, "参考实现：第 2 行即分叉");

        let members: Vec<(String, String)> = ["a", "b", "c"]
            .iter()
            .map(|cid| ("uid".to_string(), cid.to_string()))
            .collect();
        let status = derive_content_status(&members, &fps, None, &mut |_, _| None);
        assert_eq!(
            status["commonPrefix"],
            json!(2),
            "一遍扫过全体 = 两两最小值"
        );
        assert_eq!(status["deltas"]["a"], json!(7), "9 - 2");
        assert_eq!(status["deltas"]["b"], json!(5), "7 - 2");
        assert_eq!(status["deltas"]["c"], json!(4), "6 - 2");
        assert_eq!(status["latestLineCount"], json!(9));
        // 公共前缀（2）没覆盖到最短副本（6 行）⇒ 分叉。
        assert_eq!(status["status"], json!(STATUS_DIVERGE));

        // 全体一致 ⇒ latest；行数不等但前缀覆盖最短 ⇒ behind。
        // ⚠️ 构造必须**自洽**：`prefix_hashes.len() <= line_count`（超过 512 行时才截断），
        // 否则「公共前缀 < 行数」会把它判成分叉 —— 这正是本条断言要挡的假数据。
        let full = SessionFingerprint {
            line_count: 5,
            prefix_hashes: vec![1, 2, 3, 4, 5],
            full_hash: 0,
        };
        let same = vec![Some(full.clone()), Some(full)];
        let equal = derive_content_status(&members[..2], &same, None, &mut |_, _| None);
        assert_eq!(equal["status"], json!(STATUS_LATEST));
        assert_eq!(equal["commonPrefix"], json!(5));
        assert_eq!(equal["latestLineCount"], json!(5));
        let prefix_only = vec![
            Some(a.clone()),
            Some(SessionFingerprint {
                line_count: 3,
                prefix_hashes: vec![1, 2, 3],
                full_hash: 0,
            }),
        ];
        let behind = derive_content_status(&members[..2], &prefix_only, None, &mut |_, _| None);
        assert_eq!(behind["status"], json!(STATUS_BEHIND));
        assert_eq!(
            behind["commonPrefix"],
            json!(3),
            "公共前缀 = 3（覆盖到最短副本）"
        );

        // 缺一个指纹 ⇒ 整体 unknown（**不臆造**）。
        let missing = vec![Some(a), None];
        assert_eq!(
            derive_content_status(&members[..2], &missing, None, &mut |_, _| None)["status"],
            json!(STATUS_UNKNOWN)
        );
    }

    /// ★ 缓存按 **(size, mtime)** 失效：**只追加几行**就必须给出新读数。
    ///
    /// 可证伪：把 `FingerprintStore::valid` 里的判据删成「存在即命中」
    /// ⇒ 第二条断言红（会拿到追加前的行数）。
    #[test]
    fn fingerprint_store_invalidates_when_file_grows() {
        let (home, projects) = temp_projects("cache");
        write_jsonl(&projects, "ws-a", "cid-1", 5);
        let index = build_projects_index_in(&projects);
        let members = vec![("uid-a".to_string(), "cid-1".to_string())];

        let mut store = FingerprintStore::default();
        let first = fingerprints_for_members(&mut store, Region::Cn, &index, &members);
        assert_eq!(first[0].as_ref().unwrap().line_count, 5);

        // 命中：缓存里的读数被复用（内容未变 ⇒ 行数不变）。
        let cached = fingerprints_for_members(&mut store, Region::Cn, &index, &members);
        assert_eq!(cached[0].as_ref().unwrap().line_count, 5);

        // 追加一行（客户端继续对话）⇒ **必须**读到新行数。
        let path = projects.join("ws-a").join("cid-1.jsonl");
        let mut body = std::fs::read_to_string(&path).unwrap();
        body.push_str("{\"type\":\"message\",\"sessionId\":\"cid-1\",\"n\":5}\n");
        std::fs::write(&path, body).unwrap();

        let after = fingerprints_for_members(&mut store, Region::Cn, &index, &members);
        assert_eq!(
            after[0].as_ref().unwrap().line_count,
            6,
            "文件变长 ⇒ 缓存必须失效（否则会一直显示旧行数）"
        );
        std::fs::remove_dir_all(home).unwrap();
    }

    /// ★ 端到端（**不碰真实 home**）：目录里的 jsonl → 指纹 → 组状态。
    ///
    /// 这条覆盖「索引 + 并行读取 + 归一化 + 派生」的整条接线 —— 上面的用例各自
    /// 只覆盖一段，缺了它，「接线错了但每段都对」不会被发现。
    #[test]
    fn content_status_end_to_end_from_disk() {
        let (home, projects) = temp_projects("e2e");
        // 副本 a：12 行；副本 a-copy：内容相同（id 已归一化）⇒ 应当「已一致」。
        write_jsonl(&projects, "ws-a", "cid-a", 12);
        write_jsonl(&projects, "ws-b", "cid-a-copy", 12);
        // 副本 c：前缀 10 行 + 自己 2 行 ⇒ 相对最短副本（12）是**追加** ⇒ behind。
        write_jsonl(&projects, "ws-c", "cid-c", 14);

        let index = build_projects_index_in(&projects);
        let mut store = FingerprintStore::default();
        let members = vec![
            ("uid-a".to_string(), "cid-a".to_string()),
            ("uid-b".to_string(), "cid-a-copy".to_string()),
        ];
        let fingerprints = fingerprints_for_members(&mut store, Region::Cn, &index, &members);
        let status = derive_content_status(&members, &fingerprints, None, &mut |_, _| None);
        assert_eq!(
            status["status"],
            json!(STATUS_LATEST),
            "两份同名内容（换了 id）必须判「已一致」—— 归一化没生效就会误判分叉"
        );
        assert_eq!(status["commonPrefix"], json!(12));

        // 三副本：c 多 2 行 ⇒ behind（公共前缀仍是 12，覆盖到最短副本）。
        let mut three = members.clone();
        three.push(("uid-c".to_string(), "cid-c".to_string()));
        let fingerprints = fingerprints_for_members(&mut store, Region::Cn, &index, &three);
        let status = derive_content_status(&three, &fingerprints, None, &mut |_, _| None);
        assert_eq!(status["status"], json!(STATUS_BEHIND));
        assert_eq!(status["latestLineCount"], json!(14));
        assert_eq!(status["deltas"]["cid-c"], json!(2));
        assert_eq!(status["deltas"]["cid-a"], json!(0));

        // 目录里没有的 cid ⇒ `None`（**没有内容可读**不是错误）⇒ 整体 unknown。
        let ghost = vec![
            ("uid-a".to_string(), "cid-a".to_string()),
            ("uid-b".to_string(), "cid-ghost".to_string()),
        ];
        let fingerprints = fingerprints_for_members(&mut store, Region::Cn, &index, &ghost);
        assert!(fingerprints[1].is_none());
        assert_eq!(
            derive_content_status(&ghost, &fingerprints, None, &mut |_, _| None)["status"],
            json!(STATUS_UNKNOWN)
        );
        std::fs::remove_dir_all(home).unwrap();
    }

    // ------------------------------------------------------- issue #141：超窗口的长会话

    /// 写一份会话正文，并允许改写**指定行**（用来造「窗口之外才分叉」的现场）。
    fn write_jsonl_with(
        projects: &Path,
        workspace: &str,
        cid: &str,
        lines: usize,
        rewrite: impl Fn(usize, String) -> String,
    ) {
        let mut body = String::new();
        for index in 0..lines {
            let line = format!("{{\"type\":\"message\",\"sessionId\":\"{cid}\",\"n\":{index}}}");
            body.push_str(&rewrite(index, line));
            body.push('\n');
        }
        let dir = projects.join(workspace);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(format!("{cid}.jsonl")), body).unwrap();
    }

    /// 生产同款探针（缓存 → 只读到第 N 行 → 回填缓存）。
    fn probe_for<'a>(
        store: &'a mut FingerprintStore,
        index: &'a HashMap<String, PathBuf>,
        members: &'a [(String, String)],
        fingerprints: &'a [Option<SessionFingerprint>],
    ) -> impl FnMut(usize, usize) -> Option<u64> + 'a {
        move |position: usize, offset: usize| {
            let fingerprint = fingerprints.get(position)?.as_ref()?;
            if offset >= fingerprint.line_count {
                return Some(fingerprint.full_hash);
            }
            let (_uid, cid) = members.get(position)?;
            let path = index.get(cid)?;
            let key = fingerprint_key(Region::Cn, cid);
            if let Some(digest) = store.probe(&key, offset) {
                return Some(digest);
            }
            let digest = prefix_digest_of_file(path, cid, offset)?;
            store.record_probe(&key, offset, digest);
            Some(digest)
        }
    }

    /// ★ issue #141 的核心回归：**逐字节相同的长会话不许被判成「有分歧」**。
    ///
    /// 620 行 > [`PREFIX_HASH_LIMIT`] ⇒ 旧判据 `common(512) < min_lines(620)` **恒为真**，
    /// 会无条件报 `diverge`（用户点「处理分歧」又什么都不发生 —— 全量比对认为本来就一致）。
    ///
    /// 可证伪：把 `truncated` 分支里的 `full_hash` 比较删掉 / 恒为 `false` ⇒ 本条红。
    #[test]
    fn long_identical_copies_are_not_reported_as_divergent() {
        let (home, projects) = temp_projects("long-identical");
        write_jsonl(&projects, "ws-a", "cid-a", 620);
        write_jsonl(&projects, "ws-b", "cid-a-copy", 620);
        let index = build_projects_index_in(&projects);
        let mut store = FingerprintStore::default();
        let members = vec![
            ("uid-a".to_string(), "cid-a".to_string()),
            ("uid-b".to_string(), "cid-a-copy".to_string()),
        ];
        let fingerprints = fingerprints_for_members(&mut store, Region::Cn, &index, &members);
        // 行数相同 ⇒ 不该碰探针（零额外 IO）。
        let mut probe = |_: usize, _: usize| -> Option<u64> { panic!("行数相同不该读盘") };
        let status = derive_content_status(&members, &fingerprints, None, &mut probe);
        assert_eq!(
            status["status"],
            json!(STATUS_LATEST),
            "两份 620 行、内容逐字节相同（只换了 id）⇒ 必须「已一致」"
        );
        assert_eq!(status["commonPrefix"], json!(620));
        assert_eq!(status["commonPrefixExact"], json!(true));
        assert_eq!(status["deltas"]["cid-a"], json!(0));
        std::fs::remove_dir_all(home).unwrap();
    }

    /// ★ 超窗口的「只追加」必须仍判 `behind`（旧判据下 `behind` 对长会话**永不可达**）。
    ///
    /// 同时钉住探针：620 行那份是 660 行那份的**真前缀** ⇒ 公共前缀 = 620（不是 512）。
    #[test]
    fn long_copy_that_only_grew_is_behind_with_exact_prefix() {
        let (home, projects) = temp_projects("long-behind");
        write_jsonl(&projects, "ws-a", "cid-a", 620);
        write_jsonl(&projects, "ws-b", "cid-b", 660);
        let index = build_projects_index_in(&projects);
        let mut store = FingerprintStore::default();
        let members = vec![
            ("uid-a".to_string(), "cid-a".to_string()),
            ("uid-b".to_string(), "cid-b".to_string()),
        ];
        let fingerprints = fingerprints_for_members(&mut store, Region::Cn, &index, &members);
        let status = derive_content_status(
            &members,
            &fingerprints,
            None,
            &mut probe_for(&mut store, &index, &members, &fingerprints),
        );
        assert_eq!(
            status["status"],
            json!(STATUS_BEHIND),
            "短的那份是长的真前缀 ⇒ 只追加即可，不是分歧"
        );
        assert_eq!(
            status["commonPrefix"],
            json!(620),
            "公共前缀必须给出**真值**（不是窗口上限 512）"
        );
        assert_eq!(status["commonPrefixExact"], json!(true));
        assert_eq!(status["deltas"]["cid-a"], json!(0));
        assert_eq!(status["deltas"]["cid-b"], json!(40));
        // ★ 探针已记进缓存 ⇒ 第二次开页零 IO（`list_replica_groups_for` 的承诺）。
        let key = fingerprint_key(Region::Cn, "cid-b");
        assert!(
            store.probe(&key, 620).is_some(),
            "验证过的前缀摘要必须留在缓存里（否则每次开页都要重读）"
        );
        // ★ 而且必须**随缓存落盘**：不落盘的话，「重启后第一次开页」仍然要重读长文件，
        //   「命中零 IO」这条承诺就是空话。
        let cache = home.join("cache.bin");
        assert!(store.save_if_worthwhile(&cache), "有新条目 ⇒ 必须落盘");
        let reloaded = FingerprintStore::load_from(&cache);
        assert_eq!(
            reloaded.probe(&key, 620),
            store.probe(&key, 620),
            "探针没有往返 ⇒ 落盘格式漏了这一节"
        );
        std::fs::remove_dir_all(home).unwrap();
    }

    /// ★ 超窗口的**真分歧**：行数相同但正文在窗口之外改过 ⇒ `diverge`，且公共前缀只是下界。
    #[test]
    fn long_copies_with_rewritten_tail_are_divergent() {
        let (home, projects) = temp_projects("long-diverge-same");
        write_jsonl(&projects, "ws-a", "cid-a", 620);
        // 第 600 行（窗口之外）换掉一个字符 ⇒ 前 512 行仍然全同。
        write_jsonl_with(&projects, "ws-b", "cid-b", 620, |index, line| {
            if index == 600 {
                line.replace("\"n\":600", "\"n\":600,\"edited\":true")
            } else {
                line
            }
        });
        let index = build_projects_index_in(&projects);
        let mut store = FingerprintStore::default();
        let members = vec![
            ("uid-a".to_string(), "cid-a".to_string()),
            ("uid-b".to_string(), "cid-b".to_string()),
        ];
        let fingerprints = fingerprints_for_members(&mut store, Region::Cn, &index, &members);
        let status = derive_content_status(
            &members,
            &fingerprints,
            None,
            &mut probe_for(&mut store, &index, &members, &fingerprints),
        );
        assert_eq!(status["status"], json!(STATUS_DIVERGE));
        assert_eq!(
            status["commonPrefixExact"],
            json!(false),
            "窗口之外改过 ⇒ 512 只是**下界**，界面不许说「共同旧版 512 条」"
        );
        std::fs::remove_dir_all(home).unwrap();
    }

    /// ★ 超窗口 + 行数不等 + 窗口之外改过 ⇒ 前缀关系不成立 ⇒ `diverge`（不是 `behind`）。
    ///
    /// 这一条挡的是「为了修 #141 就把长会话一律当 `behind`」的偷懒修法 ——
    /// 那会让「只追加」把分叉行覆盖成一段混合历史。
    #[test]
    fn long_copy_with_divergent_middle_is_not_behind() {
        let (home, projects) = temp_projects("long-diverge-diff");
        write_jsonl(&projects, "ws-a", "cid-a", 620);
        write_jsonl_with(&projects, "ws-b", "cid-b", 660, |index, line| {
            if index == 590 {
                line.replace("\"n\":590", "\"n\":590,\"edited\":true")
            } else {
                line
            }
        });
        let index = build_projects_index_in(&projects);
        let mut store = FingerprintStore::default();
        let members = vec![
            ("uid-a".to_string(), "cid-a".to_string()),
            ("uid-b".to_string(), "cid-b".to_string()),
        ];
        let fingerprints = fingerprints_for_members(&mut store, Region::Cn, &index, &members);
        let status = derive_content_status(
            &members,
            &fingerprints,
            None,
            &mut probe_for(&mut store, &index, &members, &fingerprints),
        );
        assert_eq!(
            status["status"],
            json!(STATUS_DIVERGE),
            "短的那份不是长的前缀 ⇒ 只能走覆盖式统一"
        );
        std::fs::remove_dir_all(home).unwrap();
    }

    /// ★ 取不到前缀摘要 ⇒ `unknown`（**不把「读不到」当成「相同」**）。
    #[test]
    fn long_group_without_probe_stays_unknown() {
        let (home, projects) = temp_projects("long-unknown");
        write_jsonl(&projects, "ws-a", "cid-a", 620);
        write_jsonl(&projects, "ws-b", "cid-b", 660);
        let index = build_projects_index_in(&projects);
        let mut store = FingerprintStore::default();
        let members = vec![
            ("uid-a".to_string(), "cid-a".to_string()),
            ("uid-b".to_string(), "cid-b".to_string()),
        ];
        let fingerprints = fingerprints_for_members(&mut store, Region::Cn, &index, &members);
        let status = derive_content_status(&members, &fingerprints, None, &mut |_, _| None);
        assert_eq!(status["status"], json!(STATUS_UNKNOWN));
        assert_eq!(status["commonPrefix"], Value::Null);
        std::fs::remove_dir_all(home).unwrap();
    }

    // ---------------------------------------------------------------- 落盘缓存

    /// ★ 落盘缓存**往返**：写出去再读回来，读数与身份逐条一致。
    ///
    /// 可证伪：把 `save_if_worthwhile` 里的 `line_count` 改成不写（写 0）⇒ 第二条断言红。
    #[test]
    fn fingerprint_cache_round_trips() {
        let (home, projects) = temp_projects("cache-roundtrip");
        let cache = home.join("cache.bin");
        write_jsonl(&projects, "ws-a", "cid-1", 7);

        let mut store = FingerprintStore::default();
        store.put(
            fingerprint_key(Region::Cn, "cid-1"),
            1234,
            1_700_000_000_123,
            SessionFingerprint {
                line_count: 7,
                prefix_hashes: vec![10, 20, 30],
                full_hash: 0,
            },
        );
        assert!(store.save_if_worthwhile(&cache), "有改动就必须落盘");
        assert!(
            !store.save_if_worthwhile(&cache),
            "落盘后不再是 dirty（全命中的开页一个字节都不写）"
        );

        let reloaded = FingerprintStore::load_from(&cache);
        let entry = reloaded
            .entries
            .get(&fingerprint_key(Region::Cn, "cid-1"))
            .unwrap();
        assert_eq!(entry.size, 1234);
        assert_eq!(entry.mtime_ms, 1_700_000_000_123);
        assert_eq!(entry.fingerprint.line_count, 7);
        assert_eq!(entry.fingerprint.prefix_hashes, vec![10, 20, 30]);
        assert_eq!(
            entry.fingerprint.full_hash, 0,
            "整份摘要必须一起往返（#141 的超窗口判定全靠它）"
        );
        assert!(
            !reloaded.dirty,
            "载入不算改动（否则每次开页都会白写一份 106 MB）"
        );

        std::fs::remove_dir_all(home).unwrap();
    }

    /// ★★★ 跨「重启」复用：**没有重新读正文**也能拿到读数。
    ///
    /// 证明方式（唯一可靠的一种）：把文件内容**换成等长的另一份**、并把 mtime **改回去** ——
    /// 此时 (size, mtime) 与缓存一致 ⇒ 命中路径**必须**给出**旧**行数。
    /// 若读到的是新内容（行数变了），说明它根本没走缓存、偷偷读了盘。
    ///
    /// 这同时把「同尺寸同 mtime 改写会被漏掉」这条**已知边界**钉成断言：它是**刻意**接受的
    /// （客户端追加式写入必然改 size 或 mtime；为它每次开页重读 66 GB 是本末倒置）。
    #[test]
    fn persisted_cache_is_reused_without_reading_content() {
        let (home, projects) = temp_projects("cache-reuse");
        let cache = home.join("cache.bin");
        write_jsonl(&projects, "ws-a", "cid-1", 3);
        let path = projects.join("ws-a").join("cid-1.jsonl");
        let original_mtime = std::fs::metadata(&path).unwrap().modified().unwrap();

        // 第一趟：真读一遍并落盘。
        let mut store = FingerprintStore::default();
        let members = vec![("uid-a".to_string(), "cid-1".to_string())];
        let index = build_projects_index_in(&projects);
        let first = fingerprints_for_members(&mut store, Region::Cn, &index, &members);
        assert_eq!(first[0].as_ref().unwrap().line_count, 3);
        assert!(store.save_if_worthwhile(&cache));

        // 「重启」：新进程只有磁盘上的缓存。
        let mut store = FingerprintStore::load_from(&cache);

        // 等长改写 + mtime 改回去 ⇒ 按 (size, mtime) 判据**仍是命中**。
        let rewritten = "{\"type\":\"message\",\"sessionId\":\"cid-1\",\"n\":9}\n".repeat(3);
        assert_eq!(
            std::fs::metadata(&path).unwrap().len(),
            rewritten.len() as u64,
            "构造前提：两份内容必须等长"
        );
        std::fs::write(&path, &rewritten).unwrap();
        let file = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
        file.set_modified(original_mtime).unwrap();
        drop(file);

        let reused = fingerprints_for_members(&mut store, Region::Cn, &index, &members);
        // 行数仍是 3（两份都 3 行）⇒ 用 hash 证明：命中的是**旧**内容的前缀 hash。
        assert_eq!(
            reused[0].as_ref().unwrap().prefix_hashes,
            first[0].as_ref().unwrap().prefix_hashes,
            "同 (size, mtime) ⇒ 必须直接复用缓存读数；hash 变了说明它偷读了盘"
        );

        // 反向对照：mtime 变了（正常的追加）⇒ **必须**重读，hash 随之变化。
        let appended = format!("{rewritten}{{\"type\":\"message\"}}\n");
        std::fs::write(&path, &appended).unwrap();
        let refreshed = fingerprints_for_members(&mut store, Region::Cn, &index, &members);
        assert_eq!(
            refreshed[0].as_ref().unwrap().line_count,
            4,
            "尺寸变了必须重读"
        );
        assert_ne!(
            refreshed[0].as_ref().unwrap().prefix_hashes,
            first[0].as_ref().unwrap().prefix_hashes,
            "内容变了 hash 必须变（证明上面那条不是「hash 恒等」）"
        );

        std::fs::remove_dir_all(home).unwrap();
    }

    /// ★ 缓存文件损坏 / 版本不符 ⇒ **不 panic、不给定错误读数**（fail-safe 成空表）。
    ///
    /// ⚠️ 魔数不符必须**整体丢弃**：那是「布局变了」，按新布局读老字节会得到静默的错读数。
    #[test]
    fn corrupt_fingerprint_cache_degrades_to_empty() {
        let (home, projects) = temp_projects("cache-corrupt");
        let cache = home.join("cache.bin");
        write_jsonl(&projects, "ws-a", "cid-1", 2);

        let mut store = FingerprintStore::default();
        store.put(
            fingerprint_key(Region::Cn, "cid-1"),
            10,
            20,
            SessionFingerprint {
                line_count: 2,
                prefix_hashes: vec![1, 2],
                full_hash: 0,
            },
        );
        assert!(store.save_if_worthwhile(&cache));
        let good = std::fs::read(&cache).unwrap();

        // 魔数不符 ⇒ 空（不是「按新布局硬读」）。
        let mut wrong_magic = good.clone();
        wrong_magic[..8].copy_from_slice(b"BSFPC999");
        std::fs::write(&cache, &wrong_magic).unwrap();
        assert_eq!(
            FingerprintStore::load_from(&cache).entries.len(),
            0,
            "版本 / 魔数不符必须整体丢弃"
        );

        // 中途截断 ⇒ 保留已解析部分，**不 panic**。
        std::fs::write(&cache, &good[..good.len() - 5]).unwrap();
        let truncated = FingerprintStore::load_from(&cache);
        assert!(truncated.entries.len() <= 1, "截断最多留下完整解析的那几条");

        // 空文件 / 垃圾字节 ⇒ 空表。
        for payload in [Vec::new(), b"not-a-cache".to_vec(), vec![0u8; 64]] {
            std::fs::write(&cache, &payload).unwrap();
            assert_eq!(FingerprintStore::load_from(&cache).entries.len(), 0);
        }
        // 文件根本不存在 ⇒ 空表（首次运行）。
        assert_eq!(
            FingerprintStore::load_from(&home.join("nope.bin"))
                .entries
                .len(),
            0
        );

        std::fs::remove_dir_all(home).unwrap();
    }

    /// ★ 落盘**不是**「有改动就写」：条目集合没变、且新读数太少时**不写**（106 MB 整份重写）。
    ///
    /// 可证伪：把 `save_if_worthwhile` 里的 `structural` / `fresh_reads` 判据删掉
    /// （退回「dirty 就写」）⇒ 第一条「不该写」断言红。
    #[test]
    fn cache_write_is_skipped_for_tiny_updates() {
        let (home, projects) = temp_projects("cache-threshold");
        let cache = home.join("cache.bin");
        write_jsonl(&projects, "ws-a", "cid-1", 3);
        let path = projects.join("ws-a").join("cid-1.jsonl");

        // 模拟「上一次已经落过盘」：1 条条目，`entries_at_load` 与之一致。
        let mut store = FingerprintStore::default();
        store.put(
            fingerprint_key(Region::Cn, "cid-1"),
            1,
            1,
            SessionFingerprint {
                line_count: 3,
                prefix_hashes: vec![1],
                full_hash: 0,
            },
        );
        assert!(
            store.save_if_worthwhile(&cache),
            "首次构建（条目数变了）必须写"
        );
        let after_first = std::fs::read(&cache).unwrap();

        // 内容变了（模拟用户聊了几句）⇒ 只有 1 条新读数、条目数不变 ⇒ **不该重写 106 MB**。
        let fingerprint = fingerprint_of_file(&path, "cid-1").unwrap();
        store.put(fingerprint_key(Region::Cn, "cid-1"), 99, 99, fingerprint);
        assert!(
            !store.save_if_worthwhile(&cache),
            "只有 1 条新读数、条目数没变 ⇒ 不值得重写整份缓存"
        );
        assert_eq!(
            std::fs::read(&cache).unwrap(),
            after_first,
            "跳过落盘时文件必须**一个字节都没变**"
        );

        // 攒够 32 条 ⇒ 值得写。
        for index in 0..FINGERPRINT_CACHE_MIN_PERSIST_READS {
            store.put(
                fingerprint_key(Region::Cn, &format!("filler-{index}")),
                1,
                1,
                SessionFingerprint {
                    line_count: 1,
                    prefix_hashes: vec![index as u64],
                    full_hash: 0,
                },
            );
        }
        // ⚠️ 注意：上面这轮 **条目数变了** ⇒ `structural` 也为真；这里要单独验的是
        //    「fresh_reads 判据本身有效」—— 所以再补一次只有读数、没有新条目的场景。
        assert!(store.save_if_worthwhile(&cache), "条目数变了 ⇒ 必须写");

        let mut store = FingerprintStore::load_from(&cache);
        let before = std::fs::read(&cache).unwrap();
        for _ in 0..FINGERPRINT_CACHE_MIN_PERSIST_READS {
            store.put(
                fingerprint_key(Region::Cn, "cid-1"),
                2,
                2,
                SessionFingerprint {
                    line_count: 3,
                    prefix_hashes: vec![2],
                    full_hash: 0,
                },
            );
        }
        assert!(
            store.save_if_worthwhile(&cache),
            "同一个键重读 32 次 ⇒ fresh_reads 达标 ⇒ 必须写（证明该判据真的在起作用）"
        );
        assert_ne!(std::fs::read(&cache).unwrap(), before, "这次必须真的重写了");

        std::fs::remove_dir_all(home).unwrap();
    }

    /// ★ 剪枝：只清「本 region 且本次没请求」的条目 —— 别的 region 与本次请求过的必须留。
    ///
    /// 可证伪：把 `prune_region` 里的前缀判断去掉（退化成按 key 是否存在）
    /// ⇒ 别的 region 的条目会被连坐清掉 ⇒ 第一条保留断言红。
    #[test]
    fn prune_region_keeps_other_regions() {
        let mut store = FingerprintStore::default();
        for (region, cid) in [
            (Region::Cn, "keep"),
            (Region::Cn, "stale"),
            (Region::Global, "other-region"),
        ] {
            store.put(
                fingerprint_key(region, cid),
                1,
                1,
                SessionFingerprint {
                    line_count: 1,
                    prefix_hashes: vec![7],
                    full_hash: 0,
                },
            );
        }
        let mut keep = std::collections::HashSet::new();
        keep.insert(fingerprint_key(Region::Cn, "keep"));
        let removed = store.prune_region(Region::Cn, &keep);

        assert_eq!(removed, 1, "只该剪掉 CN 里没被请求的那一条");
        assert!(store
            .entries
            .contains_key(&fingerprint_key(Region::Cn, "keep")));
        assert!(!store
            .entries
            .contains_key(&fingerprint_key(Region::Cn, "stale")));
        assert!(
            store
                .entries
                .contains_key(&fingerprint_key(Region::Global, "other-region")),
            "别的 region 的条目与本次剪枝无关"
        );
    }

    /// ★ P14 S2：基线的**前缀验证**在 core（原先在前端）—— 验证不过 ⇒ `null`，不编数。
    #[test]
    fn since_baseline_requires_a_verified_prefix() {
        let baseline = GroupBaseline {
            a: "cid-a".to_string(),
            b: "cid-b".to_string(),
            created_at: 1,
            line_count: 3,
            prefix_hashes: vec![1, 2, 3],
        };
        let members = vec![
            ("uid-a".to_string(), "cid-a".to_string()),
            ("uid-b".to_string(), "cid-b".to_string()),
            ("uid-c".to_string(), "cid-c".to_string()),
        ];

        // 前缀仍是真前缀 ⇒ 给出诚实增量（5 - 3 = 2）。
        let good = vec![
            Some(SessionFingerprint {
                line_count: 5,
                prefix_hashes: vec![1, 2, 3, 4, 5],
                full_hash: 0,
            }),
            Some(SessionFingerprint {
                line_count: 3,
                prefix_hashes: vec![1, 2, 3],
                full_hash: 0,
            }),
            Some(SessionFingerprint {
                line_count: 3,
                prefix_hashes: vec![1, 2, 3],
                full_hash: 0,
            }),
        ];
        let status = derive_content_status(&members, &good, Some(&baseline), &mut |_, _| None);
        assert_eq!(status["sinceBaseline"]["cid-a"], json!(2));
        assert_eq!(status["sinceBaseline"]["cid-b"], json!(0));
        // 基线是**成对**的：与它无关的成员**不在表里**（不是 0）。
        assert!(status["sinceBaseline"].get("cid-c").is_none());

        // 改写了更早的内容（第 3 行变了）⇒ 验证不过 ⇒ `null`（「新增几条」不成立）。
        let rewritten = vec![
            Some(SessionFingerprint {
                line_count: 5,
                prefix_hashes: vec![1, 2, 9, 4, 5],
                full_hash: 0,
            }),
            Some(SessionFingerprint {
                line_count: 3,
                prefix_hashes: vec![1, 2, 3],
                full_hash: 0,
            }),
            Some(SessionFingerprint {
                line_count: 3,
                prefix_hashes: vec![1, 2, 3],
                full_hash: 0,
            }),
        ];
        let status = derive_content_status(&members, &rewritten, Some(&baseline), &mut |_, _| None);
        assert!(status["sinceBaseline"]["cid-a"].is_null());
    }
}
