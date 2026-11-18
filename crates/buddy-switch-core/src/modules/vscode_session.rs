//! VS Code 内 CodeBuddy 插件 / CodeBuddy IDE 的**会话仓**（P14 S4a · **只读半**）。
//!
//! ## 布局
//!
//! 两棵会话树**同根同构**，只差客户端段目录名（[`ExtClient::client_dir`]）：
//!
//! ```text
//! <本地数据根>/CodeBuddyExtension/Data/<uid>/<客户端段>/<uid>/
//!   history/<工作区目录名>/
//!     index.json              # 工作区级会话索引 {conversations:[…], current}
//!     .index_bak.json         # 同结构备份（插件侧写；IDE 侧不写）
//!     <会话 id>/               # 32 位小写 hex
//!       index.json            # {messages:[…], requests:[…]}
//!       messages/<消息 id>.json
//!       assets/*
//! ```
//!
//! 工作区目录名是**不透明字符串**（扩展自己按 `md5(工作区)` 生成）——本模块**只枚举、
//! 不重算**，因为重算 md5 需要知道扩展对「工作区」的精确定义（路径大小写 / 归一化规则），
//! 猜错就会把会话写到**另一个工作区**目录里去。
//!
//! ## ★★ 这是本期唯一「老实现完全不能复用」的地方
//!
//! 桌面版会话是 **jsonl（逐行）**；这里是 **JSON 记录**（目录 + 索引数组）。
//! ⇒ [`crate::modules::session::append_delta_between_files`] 那套**按行 append**
//! 对它毫无意义（`P14-TASKS.md` S4 已写死这条）。
//!
//! ## 三条硬约束（写路径落地前必须守住，本文件先把边界钉住）
//!
//! 1. **uid 必须过 [`is_safe_uid`]**：拒空 / 含路径分隔符 / `.` / `..` / `default` / `Public`。
//!    没有这一层，一个脏 uid 就能让我们写到数据仓之外（或碰坏结构不同的 `Public\` 目录）。
//! 2. **id 一律 32 位小写 hex**（扩展 `generateId()` 的产物）。带连字符的 UUID 扩展认不出来。
//! 3. **只读当前实现**：本模块**不写任何文件**。写路径（复制 / 同步）是下一步 ——
//!    它要动用户客户端的真实数据，**不半懂就写**。
//!
//! ## 本期范围与下一步
//!
//! - 本期（S4a 读半）：路径解析 + 安全边界 + 会话枚举（只读）。
//! - 下一步（S4a 写半）：复制（id 重映射 + 加法写 + 原子提交）。
//! - 再下一步（S4b/S4c）：关联 / 增量同步；IDE 侧复用。
//!
//! ⚠️ **布局口径来自对同产品另一实现的取证**，本机**没有真实客户端数据可验证**。
//! ⇒ 所有出口一律 **fail-closed**：布局对不上就返回**结构化空态**（`dataRoot: null` 或
//! `sessions: []` + `skipped` 计数），**绝不臆造**一条会话出来。

use serde_json::{json, Value};
use std::path::{Path, PathBuf};

use crate::modules::config::home_dir;

/// 应用数据目录名（两棵树同根）。
const APP_DIR: &str = "CodeBuddyExtension";

/// 会话历史目录名。
const HISTORY_DIR: &str = "history";

/// 扩展侧的两个客户端（同根同构，只差客户端段目录名）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExtClient {
    /// VS Code 内 CodeBuddy 插件（`tencent-cloud.coding-copilot`）。
    Vscode,
    /// CodeBuddy IDE 桌面客户端（国内版）。
    CodebuddyIde,
}

impl ExtClient {
    /// 客户端段目录名（布局里的那个 `VSCode` / `CodeBuddyIDE`）。
    pub fn client_dir(self) -> &'static str {
        match self {
            ExtClient::Vscode => "VSCode",
            ExtClient::CodebuddyIde => "CodeBuddyIDE",
        }
    }

    /// 稳定字符串（进 JSON / 日志用；与前端 `client` 参数同一套取值）。
    pub fn as_str(self) -> &'static str {
        match self {
            ExtClient::Vscode => "vscode",
            ExtClient::CodebuddyIde => "codebuddy-ide",
        }
    }

    /// 该客户端的**副本 id 策略**（两棵同构的树，唯一的行为差异就在这里）。
    pub fn copy_id_policy(self) -> CopyIdPolicy {
        match self {
            ExtClient::Vscode => CopyIdPolicy::AlwaysNew,
            ExtClient::CodebuddyIde => CopyIdPolicy::KeepUnlessConflict,
        }
    }

    /// 是否把合并后的索引同步写进目标工作区的 `.index_bak.json`。
    ///
    /// - 插件侧**写**：扩展自用的索引备份需要与主索引一致；
    /// - IDE 侧**不写**：目标侧原有的 `.index_bak.json` 保持原样，**不存在也不新建**
    ///   （同产品另一实现的 codebuddy-ide-target 契约）。
    pub fn writes_workspace_index_backup(self) -> bool {
        matches!(self, ExtClient::Vscode)
    }
}

/// 副本 id 策略：决定复制体沿用源 id 还是取新 id。
///
/// ⚠️ **这是两个客户端之间唯一的语义差异**，别把它当成「配置项」随意改 ——
/// 两侧的取值都来自**真机实测**（见各变体的文档）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CopyIdPolicy {
    /// 副本**恒取新 id**（VS Code 插件）。
    ///
    /// 扩展按 id 归类会话 ⇒ 复制体必须与源区分，否则「同一份会话」在扩展看来是两条。
    AlwaysNew,
    /// **默认沿用源 id**，仅当目标已有同 id（索引条目或磁盘目录）时取新 id。
    ///
    /// CodeBuddy IDE：同产品另一实现**真机实测**——沿用 id 的目标账号**可直接加载**；
    /// 冲突时才重随机，避免覆盖目标既有会话。
    KeepUnlessConflict,
}

/// 数据根候选（按优先级，已去重）。
///
/// Windows 为 `%LOCALAPPDATA%\CodeBuddyExtension\Data`（同产品另一实现**实测**），
/// 次选 Roaming；macOS / Linux 按同一相对布局推导，**未实测**，仅作 best-effort 兜底
/// （与本仓 `auth_file::auth_candidates_for` 的候选口径一致）。
///
/// ⚠️ 用 `home_dir()` 而不是 `dirs::data_local_dir()`：本仓的 home 覆盖机制
/// （`BUDDY_SWITCH_HOME`）能让单测**完全不碰真实 home**，`dirs::*` 绕不过去。
pub fn data_root_candidates() -> Vec<PathBuf> {
    let home = home_dir();
    let bases: Vec<PathBuf> = {
        #[cfg(target_os = "windows")]
        {
            vec![home.join("AppData/Local"), home.join("AppData/Roaming")]
        }
        #[cfg(target_os = "macos")]
        {
            vec![
                home.join("Library/Application Support"),
                home.join("Library/Caches"),
            ]
        }
        #[cfg(not(any(target_os = "windows", target_os = "macos")))]
        {
            vec![home.join(".local/share"), home.join(".config")]
        }
    };
    let mut out: Vec<PathBuf> = Vec::new();
    for base in bases {
        let candidate = base.join(APP_DIR).join("Data");
        if !out.contains(&candidate) {
            out.push(candidate);
        }
    }
    out
}

/// 第一个**真实存在**的数据根；都不存在 ⇒ `None`。
///
/// 调用方据此区分「**没装**该客户端」（`None`）与「装了但该账号没有会话」（`Some` + 空列表）——
/// 这两种情况的界面文案完全不同，压成一个值就是在撒谎。
pub fn data_root() -> Option<PathBuf> {
    data_root_candidates()
        .into_iter()
        .find(|path| path.is_dir())
}

/// 账号数据目录：`<root>/<uid>/<客户端段>/<uid>`。
///
/// ⚠️ uid 出现**两次**是布局本身的形状（不是笔误）——调用前必须过 [`is_safe_uid`]。
pub fn uid_data_dir_in(root: &Path, client: ExtClient, uid: &str) -> PathBuf {
    root.join(uid).join(client.client_dir()).join(uid)
}

/// 账号的会话历史根：`…/<uid>/<客户端段>/<uid>/history`。
pub fn history_root_in(root: &Path, client: ExtClient, uid: &str) -> PathBuf {
    uid_data_dir_in(root, client, uid).join(HISTORY_DIR)
}

/// uid 白名单：非空、无路径分隔符、非 `.` / `..`、且不是 `default` / `Public`。
///
/// `default` / `Public` 是扩展自己的**兜底目录**（结构不同），必须显式排除 ——
/// 它们不是账号，碰它们等于破坏客户端的内部状态。
pub fn is_safe_uid(uid: &str) -> bool {
    let uid = uid.trim();
    !uid.is_empty()
        && uid != "."
        && uid != ".."
        && !uid.contains(['/', '\\'])
        && !uid.eq_ignore_ascii_case("default")
        && !uid.eq_ignore_ascii_case("public")
}

/// 是否为 32 位小写 hex（扩展 `generateId()` 的产物形态）。
///
/// 用途：**只对认得的 id 动手**。`messages/` 目录里可能躺着调试用的 `foo.json`，
/// 它既不是消息 id，也不该被改名 —— 复制时必须原样带过去。
pub fn is_hex32(text: &str) -> bool {
    text.len() == 32
        && text
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// 列出某账号在该客户端仓里的会话（**只读**）。
///
/// 数据根找不到 ⇒ `dataRoot: null`（前端据此渲染「未检测到客户端」而不是「没有会话」）。
pub fn list_sessions_for(client: ExtClient, uid: &str) -> Value {
    match data_root() {
        Some(root) => list_sessions_in(&root, client, uid),
        None => listing(uid, &[], 0, None),
    }
}

/// [`copy_conversations_in`] 的**生产入口**：自己解析数据根。
///
/// ⚠️ 数据根找不到 ⇒ **`Err`**，不是「复制了 0 条」——
/// 后者会让界面显示「复制成功 0 项」，把「客户端没装 / 数据目录不对」伪装成一次空操作
/// （「失败不得谎报成功」）。读路径可以用 `dataRoot: null` 表达「没装」，
/// **写路径不行**：用户点了复制就必须知道到底做没做。
pub fn copy_conversations(
    client: ExtClient,
    source_uid: &str,
    target_uid: &str,
    items: &[CopyItem],
) -> Result<Value, String> {
    use crate::modules::{
        codebuddy_ide_session_sync, linked_vscode_session,
        linked_workbuddy::{SessionPaths, WbVariant},
        session_link, vscode_session_sync,
    };
    let root = data_root().ok_or_else(|| "未检测到客户端数据目录，无法复制会话".to_string())?;
    let (paths, spec) = match client {
        ExtClient::Vscode => (
            SessionPaths::for_vscode_ext(),
            linked_vscode_session::VSCODE_STORE,
        ),
        ExtClient::CodebuddyIde => (
            SessionPaths::for_codebuddy_ide(),
            linked_vscode_session::CODEBUDDY_IDE_STORE,
        ),
    };
    let _guard = session_link::try_acquire_client_ops_lock(&paths)?;
    vscode_session_sync::initialize_link_store(spec, &root, &paths)?;
    let mut report = copy_conversations_in(&root, client, source_uid, target_uid, items);

    /* ★ 关联登记放在**生产入口**，不放进 `copy_conversations_in`。
    `copy_conversations_in` 只碰**客户端数据目录**（测试可以完全隔离）；
    而账本写在**工具自己的存储根**下 —— 塞进去会让「复制客户端数据」这个动作
    **隐式写工具存储**，单测就会污染用户的真实目录
    （本仓「单测不该依赖 `BUDDY_SWITCH_HOME`」这条纪律正是防这个）。 */
    let recorded = record_copy_links(
        &crate::modules::config::store_dir(),
        client,
        source_uid,
        target_uid,
        &report,
    );
    if let Some(object) = report.as_object_mut() {
        object.insert("linksRecorded".to_string(), json!(recorded));
    }
    if report["copied"]
        .as_array()
        .is_some_and(|rows| !rows.is_empty())
    {
        let variant = if crate::modules::account::find_account_for(
            crate::modules::region::Region::Cn,
            target_uid,
        )
        .is_some()
        {
            WbVariant::Cn
        } else if crate::modules::account::find_account_for(
            crate::modules::region::Region::Global,
            target_uid,
        )
        .is_some()
        {
            WbVariant::Ai
        } else {
            WbVariant::Cn
        };
        let errors = match client {
            ExtClient::Vscode => {
                vscode_session_sync::register_copied_sessions(&root, &paths, variant, &report)
            }
            ExtClient::CodebuddyIde => codebuddy_ide_session_sync::register_copied_sessions(
                &root, &paths, variant, &report,
            ),
        };
        report["linkErrors"] = json!(errors);
    }
    Ok(report)
}

/// [`list_sessions_for`] 的可测形态（显式数据根 ⇒ 单测只需临时目录树）。
///
/// 返回：
/// ```json
/// { "sourceUid": "…", "sessions": [ { "id", "workspaceHash", "title",
///   "updatedAt", "type", "hasHistory" } ], "skipped": 0, "dataRoot": "…" }
/// ```
/// `skipped` = 读不出来（缺失 / 损坏）的工作区索引数 —— **如实报数**，
/// 不用「少列几条」把问题吞掉（§3.5 不许假承诺）。
pub fn list_sessions_in(root: &Path, client: ExtClient, uid: &str) -> Value {
    let data_root = root.to_string_lossy().to_string();
    // ★ 脏 uid ⇒ 直接空列表（**不进文件系统**）。
    if !is_safe_uid(uid) {
        return listing(uid, &[], 0, Some(&data_root));
    }

    let history = history_root_in(root, client, uid);
    let mut sessions: Vec<Value> = Vec::new();
    let mut skipped = 0usize;

    if let Ok(entries) = std::fs::read_dir(&history) {
        for entry in entries.flatten() {
            let ws_dir = entry.path();
            if !ws_dir.is_dir() {
                continue;
            }
            // 工作区目录名原样透传（不重算 md5，见模块文档）。
            let workspace_hash = entry.file_name().to_string_lossy().to_string();
            let Some(index) = read_json(&ws_dir.join("index.json")) else {
                skipped += 1;
                continue;
            };
            let Some(conversations) = index.get("conversations").and_then(Value::as_array) else {
                skipped += 1;
                continue;
            };
            for conversation in conversations {
                let Some(id) = conversation
                    .get("id")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|text| !text.is_empty())
                else {
                    continue;
                };
                sessions.push(json!({
                    "id": id,
                    "workspaceHash": workspace_hash,
                    "title": conversation
                        .get("name")
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .filter(|text| !text.is_empty())
                        .unwrap_or("(无标题)"),
                    "updatedAt": conversation_updated_at(conversation),
                    "type": conversation.get("type").and_then(Value::as_str).unwrap_or(""),
                    "hasHistory": conversation_has_history(&ws_dir, id),
                }));
            }
        }
    }

    // 最近更新的排前面（时间缺失的排末尾，且**稳定**：同值保持原顺序）。
    sessions.sort_by_key(|session| {
        std::cmp::Reverse(
            session
                .get("updatedAt")
                .and_then(Value::as_i64)
                .unwrap_or(0),
        )
    });

    listing(uid, &sessions, skipped, Some(&data_root))
}

/// 统一出口形状（`dataRoot` 为 `None` ⇒ JSON `null`）。
fn listing(uid: &str, sessions: &[Value], skipped: usize, data_root: Option<&str>) -> Value {
    json!({
        "sourceUid": uid,
        "sessions": sessions,
        "skipped": skipped,
        "dataRoot": data_root,
    })
}

/// 会话的「最近活动」时刻（ms）：`lastMessageAt` 优先，回落 `createdAt`，都认不出 ⇒ 0。
fn conversation_updated_at(conversation: &Value) -> i64 {
    let last = time_to_ms(conversation.get("lastMessageAt"));
    if last > 0 {
        return last;
    }
    time_to_ms(conversation.get("createdAt"))
}

/// 把索引里的时间字段换算成 epoch 毫秒；认不出 ⇒ 0（**不臆造**一个时刻）。
///
/// 认两种形态：RFC3339 字符串（扩展的常见写法）与数字（秒或毫秒，按量级判）。
fn time_to_ms(value: Option<&Value>) -> i64 {
    match value {
        Some(Value::String(text)) => chrono::DateTime::parse_from_rfc3339(text.trim())
            .map(|dt| dt.timestamp_millis())
            .unwrap_or(0),
        Some(Value::Number(number)) => number
            .as_i64()
            .map(|raw| {
                if raw > 1_000_000_000_000 {
                    raw
                } else {
                    raw.saturating_mul(1000)
                }
            })
            .unwrap_or(0),
        _ => 0,
    }
}

/// 会话是否含正文：会话目录的 `index.json` 有非空 `messages`，或磁盘 `messages/` 下有文件。
///
/// ⚠️ 判据要**两条都看**：只有索引没落盘消息文件（或反之）在真实数据里都出现过，
/// 只看一条会把「有正文」误报成「空会话」。
fn conversation_has_history(ws_dir: &Path, conversation_id: &str) -> bool {
    let conversation_dir = ws_dir.join(conversation_id);
    if let Some(index) = read_json(&conversation_dir.join("index.json")) {
        if index
            .get("messages")
            .and_then(Value::as_array)
            .is_some_and(|messages| !messages.is_empty())
        {
            return true;
        }
    }
    std::fs::read_dir(conversation_dir.join("messages"))
        .map(|entries| entries.flatten().any(|entry| entry.path().is_file()))
        .unwrap_or(false)
}

/// 读取并解析 JSON 文件；缺失 / 不可读 / 损坏 ⇒ `None`（**不 panic**，由调用方计入 `skipped`）。
pub(crate) fn read_json(path: &Path) -> Option<Value> {
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

// ---------------------------------------------------------------------------
// P14 S4a · 写半：复制（id 重映射 + 原子提交 + 加法）
// ---------------------------------------------------------------------------

/// 一条待复制的会话引用。
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CopyItem {
    /// 工作区目录名（= 扩展自己算的 `md5(工作区)`，本模块**只透传不重算**）。
    pub workspace_hash: String,
    /// 会话 id（32 位小写 hex）。
    pub conversation_id: String,
}

/// 32 位小写 hex 生成（对齐扩展 `generateId()` 产物）。
///
/// ⚠️ **不是**带连字符的 UUID：扩展按这种形态归类会话，带连字符的 id 它认不出来。
fn gen_hex32() -> String {
    let bytes = uuid::Uuid::new_v4().into_bytes();
    let mut out = String::with_capacity(32);
    for byte in bytes {
        out.push(char::from_digit((byte >> 4) as u32, 16).unwrap_or('0'));
        out.push(char::from_digit((byte & 0x0f) as u32, 16).unwrap_or('0'));
    }
    out
}

/// 取一个**该工作区尚未出现过**的新 hex id（把磁盘目录名与索引条目都算进 `used`）。
fn unique_hex32(used: &mut std::collections::BTreeSet<String>) -> String {
    loop {
        let candidate = gen_hex32();
        if used.insert(candidate.clone()) {
            return candidate;
        }
    }
}

/// 一次复制的 **id 重映射表**：老 id → 新 id。
///
/// ## 为什么必须重映射
///
/// 副本要与源**区分开**（扩展按 id 归类会话），而消息之间**互相引用**：
/// `requests[].messages[]` 存的是消息 id 列表，消息的 `extra` 里还有
/// `requestId` / `responseId`。只换会话 id 会让副本内部的引用**指向源会话的 id**
/// ——扩展加载时找不到，表现为「会话能看见但点进去是空的」。
///
/// ## ★ 匹配规则是「**精确等于**」，不是子串替换
///
/// 只有**恰好等于**某个老 id 的字符串才会被换掉。正文里出现的 32 位 hex 片段
/// （代码、hash、base64 片段）**绝不能被改** —— 子串替换会**静默毁掉会话正文**，
/// 而且改完看不出来（文件还是合法 JSON）。这是本模块最危险的一处，单测钉着它。
struct RemapPlan {
    message_ids: std::collections::BTreeMap<String, String>,
    request_ids: std::collections::BTreeMap<String, String>,
    /// 两表的合并视图 —— `extra` 里不区分消息 / 请求 id，走同一套精确匹配。
    combined_ids: std::collections::BTreeMap<String, String>,
}

impl RemapPlan {
    /// 从**源会话索引**与磁盘 `messages/` 建立映射。
    ///
    /// 消息 id 有**两个来源，都要收**：
    /// ① 索引 `messages[].id`（权威引用）；
    /// ② 磁盘 `messages/<hex32>.json` 的文件名（索引可能漏记，而文件确实在）。
    ///
    /// ⚠️ 只收 **32 位小写 hex** 的文件名：`messages/` 下还可能有调试文件 / 锁文件，
    /// 它们**既不是消息 id，也不该被改名**（改名 = 扩展再也找不到那条消息）。
    fn build(source_index: &Value, source_dir: &Path) -> Self {
        let mut message_ids: std::collections::BTreeMap<String, String> =
            std::collections::BTreeMap::new();
        let mut request_ids: std::collections::BTreeMap<String, String> =
            std::collections::BTreeMap::new();
        let mut used: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();

        if let Some(messages) = source_index.get("messages").and_then(Value::as_array) {
            for message in messages {
                if let Some(id) = message.get("id").and_then(Value::as_str) {
                    insert_new_id(&mut message_ids, &mut used, id.trim());
                }
            }
        }
        if let Ok(entries) = std::fs::read_dir(source_dir.join("messages")) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if let Some(stem) = name.strip_suffix(".json") {
                    if is_hex32(stem) {
                        insert_new_id(&mut message_ids, &mut used, stem);
                    }
                }
            }
        }
        if let Some(requests) = source_index.get("requests").and_then(Value::as_array) {
            for request in requests {
                if let Some(id) = request.get("id").and_then(Value::as_str) {
                    insert_new_id(&mut request_ids, &mut used, id.trim());
                }
            }
        }

        // 合并视图：`messages` 优先（同一个老 id 同时出现在两张表时以消息表为准）。
        let mut combined_ids = request_ids.clone();
        for (old_id, new_id) in &message_ids {
            combined_ids.insert(old_id.clone(), new_id.clone());
        }

        Self {
            message_ids,
            request_ids,
            combined_ids,
        }
    }
}

/// 为一个老 id 生成并登记新 id（空串或已登记 ⇒ 跳过，**不重复消耗 id**）。
fn insert_new_id(
    map: &mut std::collections::BTreeMap<String, String>,
    used: &mut std::collections::BTreeSet<String>,
    old_id: &str,
) {
    if old_id.is_empty() || map.contains_key(old_id) {
        return;
    }
    let new_id = unique_hex32(used);
    map.insert(old_id.to_string(), new_id);
}

/// 递归遍历任意 JSON 值，把「**恰好等于**映射表里某个老 id」的字符串换成映射值。
///
/// ⚠️ 精确匹配是硬约束（见 [`RemapPlan`] 的文档）：这里**不做**子串替换，
/// 也不碰数字 / 布尔 / null。
fn replace_exact_ids(value: &mut Value, map: &std::collections::BTreeMap<String, String>) {
    match value {
        Value::String(text) => {
            if let Some(replacement) = map.get(text.as_str()) {
                *value = json!(replacement);
            }
        }
        Value::Array(items) => {
            for item in items.iter_mut() {
                replace_exact_ids(item, map);
            }
        }
        Value::Object(object) => {
            for item in object.values_mut() {
                replace_exact_ids(item, map);
            }
        }
        _ => {}
    }
}

/// 重映射消息的 `extra` —— **兼容「字符串化 JSON」与「对象」两种形态**。
///
/// ⚠️ `extra` 实测是**字符串化的 JSON**（内容是 `{requestId, responseId, modelId}`）。
/// 直接把整个字符串当 id 比一遍是**没用的**（它不等于任何一个 id）；必须**解析开**再改、
/// 改完再字符串化回去。对象形态也要支持（不同版本 / 不同客户端段有差异）。
fn remap_extra(extra: &Value, map: &std::collections::BTreeMap<String, String>) -> Value {
    let (mut object, stringified) = match extra {
        Value::String(text) => match serde_json::from_str::<Value>(text) {
            Ok(Value::Object(object)) => (object, true),
            // 解析不开 / 不是对象 ⇒ **原样返回**（宁可少改，也不猜着改）。
            _ => return extra.clone(),
        },
        Value::Object(object) => (object.clone(), false),
        _ => return extra.clone(),
    };
    for value in object.values_mut() {
        replace_exact_ids(value, map);
    }
    let value = Value::Object(object);
    if stringified {
        json!(value.to_string())
    } else {
        value
    }
}

/// 重映射**会话级索引**：`messages[].id`、`requests[].id`、`requests[].messages[]`。
fn remap_session_index(source_index: &Value, plan: &RemapPlan) -> Value {
    let mut out = source_index.clone();
    if let Some(messages) = out.get_mut("messages").and_then(Value::as_array_mut) {
        for message in messages.iter_mut() {
            let Some(id) = message
                .get("id")
                .and_then(Value::as_str)
                .map(str::to_string)
            else {
                continue;
            };
            if let (Some(new_id), Some(object)) =
                (plan.message_ids.get(&id), message.as_object_mut())
            {
                object.insert("id".to_string(), json!(new_id));
            }
        }
    }
    if let Some(requests) = out.get_mut("requests").and_then(Value::as_array_mut) {
        for request in requests.iter_mut() {
            let Some(object) = request.as_object_mut() else {
                continue;
            };
            if let Some(id) = object.get("id").and_then(Value::as_str).map(str::to_string) {
                if let Some(new_id) = plan.request_ids.get(&id) {
                    object.insert("id".to_string(), json!(new_id));
                }
            }
            // ★ `requests[].messages[]` 存的是**消息 id 列表** —— 漏改这里，
            //   扩展会按老 id 去找消息文件，找不到就显示成空会话。
            if let Some(messages) = object.get_mut("messages").and_then(Value::as_array_mut) {
                for message in messages.iter_mut() {
                    let Some(id) = message.as_str().map(str::to_string) else {
                        continue;
                    };
                    if let Some(new_id) = plan.message_ids.get(&id) {
                        *message = json!(new_id);
                    }
                }
            }
        }
    }
    out
}

/// 重映射**单条消息文件**：`id` 与 `extra` 内的引用（其余字段原样保留）。
///
/// 解析不开（非 JSON / 不是对象）⇒ **原样返回**：宁可少改一条，也不把文件写坏。
fn remap_message_file(text: &str, old_stem: &str, plan: &RemapPlan) -> String {
    let Ok(mut value) = serde_json::from_str::<Value>(text) else {
        return text.to_string();
    };
    let Some(object) = value.as_object_mut() else {
        return text.to_string();
    };
    if let Some(new_id) = plan.message_ids.get(old_stem) {
        object.insert("id".to_string(), json!(new_id));
    }
    if let Some(extra) = object.get("extra").cloned() {
        object.insert("extra".to_string(), remap_extra(&extra, &plan.combined_ids));
    }
    value.to_string()
}

/// 递归复制目录（保持相对结构与文件名 —— 附件可能有中文名）。
pub(crate) fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)?.flatten() {
        let path = entry.path();
        let target = dst.join(entry.file_name());
        if path.is_dir() {
            copy_dir_recursive(&path, &target)?;
        } else if path.is_file() {
            std::fs::copy(&path, &target)?;
        }
    }
    Ok(())
}

/// 尽力删除目录树（不存在 / 失败都忽略 —— 回退路径上不该再抛）。
pub(crate) fn remove_dir_all_if_exists(path: &Path) {
    if path.exists() {
        let _ = std::fs::remove_dir_all(path);
    }
}

/// 在目标目录写全一个会话：重映射后的 `index.json` + `messages/*` + 其余文件原样复制。
fn write_conversation(
    source_dir: &Path,
    dest_dir: &Path,
    source_index: &Value,
    plan: &RemapPlan,
) -> std::io::Result<()> {
    std::fs::create_dir_all(dest_dir)?;

    // 1) 会话索引（重映射后的）。
    std::fs::write(
        dest_dir.join("index.json"),
        remap_session_index(source_index, plan).to_string(),
    )?;

    // 2) messages/<oldId>.json → messages/<newId>.json，并重写文件内的 id / extra 引用。
    let source_messages = source_dir.join("messages");
    if source_messages.is_dir() {
        let target_messages = dest_dir.join("messages");
        std::fs::create_dir_all(&target_messages)?;
        for entry in std::fs::read_dir(&source_messages)?.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_string();
            let stem = name.strip_suffix(".json").unwrap_or(&name).to_string();
            // ⚠️ 只动 32 位小写 hex 的文件名；其它 json（调试文件 / 锁文件）**按字节原样复制**
            //    —— 既不改名也不重新序列化（重新序列化会改掉它的格式）。
            if !is_hex32(&stem) {
                std::fs::copy(&path, target_messages.join(&name))?;
                continue;
            }
            let new_name = match plan.message_ids.get(&stem) {
                Some(new_id) => format!("{new_id}.json"),
                None => name.clone(),
            };
            match std::fs::read_to_string(&path) {
                Ok(text) => {
                    let remapped = remap_message_file(&text, &stem, plan);
                    std::fs::write(target_messages.join(&new_name), remapped)?;
                }
                // 非 UTF-8 / 读取失败 ⇒ 按二进制原样复制（**不猜编码**）。
                Err(_) => {
                    std::fs::copy(&path, target_messages.join(&new_name))?;
                }
            }
        }
    }

    // 2b) 会话级 `.index_bak.json`（若存在）：同样重映射，避免老 messageId 残留在目标目录。
    let source_bak = source_dir.join(".index_bak.json");
    if source_bak.is_file() {
        match read_json(&source_bak) {
            Some(bak) => std::fs::write(
                dest_dir.join(".index_bak.json"),
                remap_session_index(&bak, plan).to_string(),
            )?,
            None => {
                std::fs::copy(&source_bak, dest_dir.join(".index_bak.json"))?;
            }
        }
    }

    // 3) 其余顶层文件 / 目录原样复制（跳过已单独处理的三个）。
    for entry in std::fs::read_dir(source_dir)?.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name == "index.json" || name == ".index_bak.json" || name == "messages" {
            continue;
        }
        let path = entry.path();
        if path.is_dir() {
            copy_dir_recursive(&path, &dest_dir.join(&name))?;
        } else if path.is_file() {
            std::fs::copy(&path, dest_dir.join(&name))?;
        }
    }
    Ok(())
}

/// 构造并入目标索引的会话条目：优先复用源条目（保住 `name` / `type` / 时间），**只换 id**。
fn conversation_entry(source_entry: Option<&Value>, new_id: &str) -> Value {
    match source_entry.and_then(Value::as_object) {
        Some(object) => {
            let mut entry = object.clone();
            entry.insert("id".to_string(), json!(new_id));
            Value::Object(entry)
        }
        None => json!({
            "id": new_id,
            "type": "craft",
            "name": "",
            "createdAt": Value::Null,
            "lastMessageAt": Value::Null,
        }),
    }
}

/// 把新会话条目并入工作区索引（追加到 `conversations[]`，并**修好 `current`**）。
///
/// ⚠️ `current` 不是可选装饰：**实测**（同产品另一实现）缺 `current` 的工作区索引
/// 会被扩展判定为**损坏** —— 它会改名成 `index.json.corrupted.<ms>` 并重建，
/// 顺带多出一条垃圾空会话。⇒ 目标原有的 `current` 仍有效就保留；缺失 / 悬空就指向本次并入的会话。
fn merge_workspace_index(index: &mut Value, entry: Value, new_id: &str) {
    if !index.is_object() {
        *index = json!({});
    }
    let Some(object) = index.as_object_mut() else {
        return;
    };
    let conversations = object
        .entry("conversations".to_string())
        .or_insert_with(|| json!([]));
    if !conversations.is_array() {
        *conversations = json!([]);
    }
    if let Some(array) = conversations.as_array_mut() {
        array.push(entry);
    }

    let current_ok = match object.get("current").and_then(Value::as_str) {
        Some(current) => object
            .get("conversations")
            .and_then(Value::as_array)
            .is_some_and(|array| {
                array
                    .iter()
                    .any(|item| item.get("id").and_then(Value::as_str) == Some(current))
            }),
        None => false,
    };
    if !current_ok {
        object.insert("current".to_string(), json!(new_id));
    }
}

/// 从工作区索引里找一条会话条目（复用源条目时用）。
fn find_conversation(index: &Value, conversation_id: &str) -> Option<Value> {
    index
        .get("conversations")
        .and_then(Value::as_array)?
        .iter()
        .find(|entry| entry.get("id").and_then(Value::as_str) == Some(conversation_id))
        .cloned()
}

/// 收集目标工作区里**已占用**的名字（磁盘目录 + 索引条目）⇒ 新 id 不会撞上任何一边。
fn used_ids_in(target_ws_dir: &Path, index: &Value) -> std::collections::BTreeSet<String> {
    let mut used: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    if let Ok(entries) = std::fs::read_dir(target_ws_dir) {
        for entry in entries.flatten() {
            used.insert(entry.file_name().to_string_lossy().to_string());
        }
    }
    if let Some(conversations) = index.get("conversations").and_then(Value::as_array) {
        for conversation in conversations {
            if let Some(id) = conversation.get("id").and_then(Value::as_str) {
                used.insert(id.to_string());
            }
        }
    }
    used
}

/// 写回目标工作区索引（`index.json` + `.index_bak.json`），**写前先备份**。
///
/// 两条纪律（与桌面版会话的覆盖式统一同源）：
/// 1. **写前备份**，且**复核备份真的落盘、字节数一致** —— 只信 `copy` 的返回值不够
///    （本仓有过「吞掉错误、无条件返回备份路径」的事故）；
/// 2. 备份不成 ⇒ **绝不改写**索引。
fn write_workspace_index(
    target_ws_dir: &Path,
    index: &Value,
    backup_root: &Path,
    workspace_hash: &str,
    write_index_backup: bool,
) -> Result<Option<String>, String> {
    std::fs::create_dir_all(target_ws_dir).map_err(|e| format!("建不了目标工作区目录：{e}"))?;
    let index_path = target_ws_dir.join("index.json");

    let mut backup_path: Option<String> = None;
    if index_path.is_file() {
        std::fs::create_dir_all(backup_root).map_err(|e| format!("建不了备份目录：{e}"))?;
        let target = backup_root.join(format!("{workspace_hash}-index.json"));
        std::fs::copy(&index_path, &target)
            .map_err(|e| format!("备份目标索引失败（**未改写**）：{e}"))?;
        let backed_up = std::fs::metadata(&target).map(|m| m.len()).unwrap_or(0);
        let original = std::fs::metadata(&index_path).map(|m| m.len()).unwrap_or(0);
        if backed_up != original {
            return Err(format!(
                "备份校验失败（备份 {backed_up} 字节 / 原文件 {original} 字节）⇒ **未改写索引**"
            ));
        }
        backup_path = Some(target.to_string_lossy().into_owned());
    }

    let text = index.to_string();
    crate::modules::config::atomic_write(&index_path, &text)
        .map_err(|e| format!("写入目标索引失败：{e}"))?;
    // 插件侧要求 `.index_bak.json` 与主索引一致（扩展自用的索引备份）；
    // ⚠️ **IDE 侧不写** —— 目标侧原有的那份保持原样，**不存在也不新建**
    //（同产品另一实现的 codebuddy-ide-target 契约：凭空造一个索引备份会让 IDE 读到它不认识的状态）。
    if write_index_backup {
        let _ = crate::modules::config::atomic_write(&target_ws_dir.join(".index_bak.json"), &text);
    }
    Ok(backup_path)
}

/// 复制一组会话到目标账号（**加法**：目标已有内容一律不动）。
///
/// ## 语义
///
/// - **只写目标 uid 目录**；源目录**只读**；
/// - **副本恒取新 id**（[`gen_hex32`]）⇒ 与源区分、也**永不覆盖**目标已有会话；
/// - **逐条隔离**：一条失败不影响其余（记入 `errors[]`）；
/// - **工作区级原子性**：会话目录先写 `.tmp-<id>` 再 `rename`；索引在**全部条目都落盘之后**
///   才合并写回 —— 索引写失败则**删掉本次新建的目录**（回退），该工作区的条目全部记失败。
///
/// ## 返回
/// ```json
/// { "sourceUid": "…", "targetUid": "…", "copiedCount": 1, "failCount": 0,
///   "copied": [ { "workspaceHash", "sourceId", "newId", "backup" } ],
///   "errors": [ { "workspaceHash", "conversationId", "error" } ] }
/// ```
pub fn copy_conversations_in(
    root: &Path,
    client: ExtClient,
    source_uid: &str,
    target_uid: &str,
    items: &[CopyItem],
) -> Value {
    let mut copied: Vec<Value> = Vec::new();
    let mut errors: Vec<Value> = Vec::new();

    let fail = |errors: &mut Vec<Value>, item: &CopyItem, reason: String| {
        errors.push(json!({
            "workspaceHash": item.workspace_hash,
            "conversationId": item.conversation_id,
            "error": reason,
        }));
    };

    // ★ 两个 uid 都要过白名单；**源与目标相同 ⇒ 直接拒**（自我复制没有意义，
    //   而且会把「只写目标」这条纪律变成一个空操作，掩盖调用方的 bug）。
    if !is_safe_uid(source_uid) || !is_safe_uid(target_uid) {
        for item in items {
            fail(&mut errors, item, "账号 uid 不合法".to_string());
        }
        return copy_report(source_uid, target_uid, copied, errors);
    }
    if source_uid == target_uid {
        for item in items {
            fail(&mut errors, item, "源账号与目标账号相同".to_string());
        }
        return copy_report(source_uid, target_uid, copied, errors);
    }

    let backup_root = session_backup_root();
    let source_history = history_root_in(root, client, source_uid);
    let target_history = history_root_in(root, client, target_uid);
    /* ★ 两个客户端之间**唯一**的语义差异走这两条（取值都来自真机实测，见各自文档）。 */
    let policy = client.copy_id_policy();
    let writes_backup = client.writes_workspace_index_backup();

    // 按工作区聚合：索引整区一次读、一次写（逐条读写会让 N 条会话写 N 次索引）。
    let mut by_workspace: std::collections::BTreeMap<String, Vec<&CopyItem>> =
        std::collections::BTreeMap::new();
    for item in items {
        by_workspace
            .entry(item.workspace_hash.clone())
            .or_default()
            .push(item);
    }

    for (workspace_hash, workspace_items) in by_workspace {
        // ★ 工作区目录名是**扩展自己算的 md5**，本模块只透传 ⇒ 必须挡住路径穿越
        //   （`../` 会让我们写到别的账号甚至仓外）。
        if !is_hex32(&workspace_hash) {
            for item in &workspace_items {
                fail(&mut errors, item, "工作区标识不合法".to_string());
            }
            continue;
        }

        let source_ws_dir = source_history.join(&workspace_hash);
        let target_ws_dir = target_history.join(&workspace_hash);

        // 目标索引：不存在 ⇒ 空索引（**不臆造** conversations 字段，由 merge 补）。
        let mut target_index =
            read_json(&target_ws_dir.join("index.json")).unwrap_or_else(|| json!({}));
        let mut used = used_ids_in(&target_ws_dir, &target_index);
        let mut created: Vec<(PathBuf, Value, String, String)> = Vec::new();

        for item in &workspace_items {
            let conversation_id = item.conversation_id.trim();
            if !is_hex32(conversation_id) {
                fail(&mut errors, item, "会话标识不合法".to_string());
                continue;
            }
            let source_dir = source_ws_dir.join(conversation_id);
            let Some(source_index) = read_json(&source_dir.join("index.json")) else {
                fail(&mut errors, item, "源会话索引缺失或损坏".to_string());
                continue;
            };

            // ★ id 策略按客户端分家（见 [`CopyIdPolicy`]）：
            //   · `AlwaysNew`          ⇒ 恒取新 id，副本内部**全部** id 重映射；
            //   · `KeepUnlessConflict` ⇒ 默认**沿用源 id**（此时整目录**按字节复制**，
            //                            一个字节都不改）；仅当目标已有同 id 时取新 id。
            //     ⚠️ 冲突时走**全量重映射**（连消息 / 请求 id 一起换）是**安全**的：
            //        会话 id 都变了，副本内部引用跟着换才自洽；而外部对它的引用本来就已断。
            let keep_source_id =
                policy == CopyIdPolicy::KeepUnlessConflict && !used.contains(conversation_id);
            let new_id = if keep_source_id {
                used.insert(conversation_id.to_string());
                conversation_id.to_string()
            } else {
                unique_hex32(&mut used)
            };
            // 1) 先写临时目录，避免出现「半个会话」。
            let tmp_dir = target_ws_dir.join(format!(".tmp-{new_id}"));
            remove_dir_all_if_exists(&tmp_dir);
            let written = if keep_source_id {
                // 沿用源 id ⇒ **整目录按字节复制**：与源逐字节相同（真机实测可直接加载）。
                copy_dir_recursive(&source_dir, &tmp_dir)
            } else {
                let plan = RemapPlan::build(&source_index, &source_dir);
                write_conversation(&source_dir, &tmp_dir, &source_index, &plan)
            };
            if let Err(error) = written {
                remove_dir_all_if_exists(&tmp_dir);
                fail(&mut errors, item, format!("写入临时会话目录失败：{error}"));
                continue;
            }
            // 2) 原子提交：rename tmp → <newId>。
            let final_dir = target_ws_dir.join(&new_id);
            if let Err(error) = std::fs::rename(&tmp_dir, &final_dir) {
                remove_dir_all_if_exists(&tmp_dir);
                fail(&mut errors, item, format!("提交会话目录失败：{error}"));
                continue;
            }
            let entry = conversation_entry(
                find_conversation(&source_index, conversation_id).as_ref(),
                &new_id,
            );
            created.push((final_dir, entry, new_id, conversation_id.to_string()));
        }

        if created.is_empty() {
            continue;
        }

        // 3) 索引最后写：把所有条目并入后**一次**写回（写前备份）。
        for (_, entry, new_id, _) in &created {
            merge_workspace_index(&mut target_index, entry.clone(), new_id);
        }
        match write_workspace_index(
            &target_ws_dir,
            &target_index,
            &backup_root,
            &workspace_hash,
            writes_backup,
        ) {
            Ok(backup) => {
                for (_, _, new_id, source_id) in &created {
                    copied.push(json!({
                        "workspaceHash": workspace_hash,
                        "sourceId": source_id,
                        "newId": new_id,
                        "backup": backup,
                    }));
                }
            }
            Err(error) => {
                // ★ 索引没写成 ⇒ 本次新建的会话目录**全部删掉**（否则留下「索引里没有、
                //   但磁盘上躺着」的孤儿会话，扩展下次扫描会把它当垃圾或半成品）。
                for (dir, _, _, _) in &created {
                    remove_dir_all_if_exists(dir);
                }
                for item in &workspace_items {
                    fail(&mut errors, item, format!("写入目标索引失败：{error}"));
                }
            }
        }
    }

    copy_report(source_uid, target_uid, copied, errors)
}

/// 统一出口形状。
///
/// 同时是 `backups/vscode-sessions/<utc>/` 自动保留的执行点：本函数的三个出口
/// （两条早退 + 正常结束）覆盖 `copy_conversations_in` 的全部路径，放在这里
/// 只需一处。显式清理入口见 `cache::clear`。
fn copy_report(
    source_uid: &str,
    target_uid: &str,
    copied: Vec<Value>,
    errors: Vec<Value>,
) -> Value {
    crate::modules::cache::prune_session_backup_dirs(crate::modules::cache::SESSION_BACKUP_KEEP);
    json!({
        "sourceUid": source_uid,
        "targetUid": target_uid,
        "copied": copied,
        "errors": errors,
        "copiedCount": copied.len(),
        "failCount": errors.len(),
    })
}

// ---------------------------------------------------------------------------
// P14 S4b · 记录级 diff（**只读**）：这份会话能不能做增量追加
// ---------------------------------------------------------------------------

/// 一个会话在「记录级」上的构成。
///
/// ## ★★ 为什么插件侧**不能**照搬桌面版的「按行 append」
///
/// 桌面版的会话是 **jsonl**：行按时间**追加**，所以「把源里多出来的行 append 到目标尾部」
/// **天然保序**（`session::append_delta_between_files`）。
///
/// 插件侧是**有序数组 + 一堆文件**：
/// - 顺序由 `index.json` 的 `messages[]` **数组次序**决定，**不是**文件时间；
/// - 消息正文在 `messages/<id>.json` 里，一条一个文件。
///
/// ⇒ 「把源里多出来的记录补到目标」**不天然保序**：
/// 若目标缺的是**中间**那几条，追加到尾部会把会话读成**错乱的顺序**。
/// 这就是本模块必须先算 diff、再决定能不能同步的原因。
#[derive(Debug, Clone, PartialEq, Eq)]
struct ConversationShape {
    /// 有序的消息 id（`index.json` 的 `messages[].id`，**顺序即会话顺序**）。
    messages: Vec<String>,
    /// 有序的请求 id。
    requests: Vec<String>,
}

impl ConversationShape {
    /// 从会话目录读形状；目录 / 索引缺失或损坏 ⇒ `None`。
    fn read(conversation_dir: &Path) -> Option<Self> {
        let index = read_json(&conversation_dir.join("index.json"))?;
        let ids = |key: &str| -> Vec<String> {
            index
                .get(key)
                .and_then(Value::as_array)
                .map(|items| {
                    items
                        .iter()
                        .filter_map(|item| {
                            item.get("id")
                                .and_then(Value::as_str)
                                .map(str::trim)
                                .filter(|text| !text.is_empty())
                                .map(str::to_string)
                        })
                        .collect()
                })
                .unwrap_or_default()
        };
        Some(Self {
            messages: ids("messages"),
            requests: ids("requests"),
        })
    }
}

/// 单条记录的**内容身份** = `messages/<id>.json` 的**原始字节**哈希。
///
/// ⚠️ 刻意**不解析 JSON** 再比：`extra` 是字符串化的 JSON，解析 → 重新序列化会**抹平**
/// 键序 / 空白差异 ⇒ 「内容其实一样」被误判成「不同」，或反过来。
/// 比字节是**最保守**的：只要有一个字节不同就算「内容不同」（宁可少同步，不可误覆盖）。
fn record_digest(path: &Path) -> Option<u64> {
    let bytes = std::fs::read(path).ok()?;
    // FNV-1a 64：本仓既有口径（只在进程内比较，跨进程稳定性不是需求）。
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    Some(hash)
}

/// `candidate` 是否为 `full` 的**前缀**（逐位相等，且不长于）。
///
/// 这是「追加不会打乱顺序」的**最强、也最保守**的判据：目标现有的记录必须
/// **原样**是源的开头一段。少一条、多一条、换一条都算不满足。
fn is_prefix(candidate: &[String], full: &[String]) -> bool {
    candidate.len() <= full.len()
        && candidate
            .iter()
            .zip(full)
            .all(|(left, right)| left == right)
}

/// 增量同步的可行性（**只读**，不写任何文件）。
///
/// ## 返回的 `status` 与它的**判据顺序**（顺序即优先级）
///
/// | status | 含义 | 后续动作 |
/// |:---|:---|:---|
/// | `error` | 源会话缺失 / 索引损坏 | 无可同步 |
/// | `absent` | 目标**没有**这份会话 | 走**整份复制**（不是增量） |
/// | `divergent` | 同 id 但**内容不同** | **拒绝**（同步会静默改写目标内容） |
/// | `unordered` | 目标不是源的**前缀** | **拒绝**（追加会**打乱会话顺序**） |
/// | `identical` | 逐条一致 | 无事可做 |
/// | `appendable` | 目标 = 源的前缀，且源只是尾部多几条 | **可以**增量追加 |
///
/// ## 为什么不「尽力同步」
///
/// `divergent` / `unordered` 两种情况下，**任何**自动动作要么改写目标已有内容、
/// 要么打乱会话顺序 —— 两者都是**静默**的（改完 JSON 依然合法）。
/// ⇒ 这里**只报告、不动作**，把选择权交回用户（与桌面版的 `diverge` 同一立场）。
pub fn diff_conversation_in(
    root: &Path,
    client: ExtClient,
    source_uid: &str,
    target_uid: &str,
    workspace_hash: &str,
    conversation_id: &str,
) -> Value {
    let report = |status: &str, reason: &str| {
        json!({
            "status": status,
            "reason": reason,
            "appendable": false,
            "commonMessages": 0,
            "missingMessages": [],
            "extraMessages": [],
            "divergentMessages": [],
        })
    };

    if !is_safe_uid(source_uid) || !is_safe_uid(target_uid) {
        return report("error", "账号 uid 不合法");
    }
    if !is_hex32(workspace_hash) || !is_hex32(conversation_id) {
        return report("error", "工作区或会话标识不合法");
    }

    let source_dir = history_root_in(root, client, source_uid)
        .join(workspace_hash)
        .join(conversation_id);
    let target_dir = history_root_in(root, client, target_uid)
        .join(workspace_hash)
        .join(conversation_id);

    let Some(source) = ConversationShape::read(&source_dir) else {
        return report("error", "源会话索引缺失或损坏");
    };
    if !target_dir.is_dir() {
        return report("absent", "目标账号没有这份会话（需要整份复制，不是增量）");
    }
    let Some(target) = ConversationShape::read(&target_dir) else {
        return report("error", "目标会话索引缺失或损坏");
    };

    let target_set: std::collections::BTreeSet<&String> = target.messages.iter().collect();
    let source_set: std::collections::BTreeSet<&String> = source.messages.iter().collect();

    // ① 同 id 但内容不同 ⇒ **拒绝**（同步会静默改写目标内容）。
    //
    //    ⚠️ **「目标缺正文文件」不是「内容不同」**：前者是**目标损坏**（索引说有、磁盘上没有），
    //    混成 `divergent` 会给出错误的处置建议（用户会以为「两边改过、需要选一份」）。
    let mut divergent: Vec<String> = Vec::new();
    let mut broken: Vec<String> = Vec::new();
    for id in source.messages.iter().filter(|id| target_set.contains(id)) {
        let from = record_digest(&source_dir.join("messages").join(format!("{id}.json")));
        let to = record_digest(&target_dir.join("messages").join(format!("{id}.json")));
        match (from, to) {
            (Some(left), Some(right)) if left != right => divergent.push(id.clone()),
            (Some(_), None) => broken.push(id.clone()),
            _ => {}
        }
    }
    if !broken.is_empty() {
        return json!({
            "status": "error",
            "reason": format!("目标有 {} 条消息的正文文件缺失（索引里有、磁盘上没有）⇒ 目标已损坏，先修好再同步", broken.len()),
            "appendable": false,
            "commonMessages": 0,
            "missingMessages": [],
            "extraMessages": [],
            "divergentMessages": [],
            "brokenMessages": broken,
        });
    }
    if !divergent.is_empty() {
        return json!({
            "status": "divergent",
            "reason": format!("有 {} 条同 id 但内容不同的消息 ⇒ 同步会**静默改写**目标已有内容", divergent.len()),
            "appendable": false,
            "commonMessages": target.messages.iter().filter(|id| source_set.contains(id)).count(),
            "missingMessages": [],
            "extraMessages": [],
            "divergentMessages": divergent,
        });
    }

    // ② 目标不是源的**前缀** ⇒ **拒绝**（追加会打乱会话顺序）。
    //    ⚠️ 消息与请求**两条序列都要判**：请求里存着消息 id 列表，只判一条会漏。
    let ordered = is_prefix(&target.messages, &source.messages)
        && is_prefix(&target.requests, &source.requests);
    let missing: Vec<String> = source
        .messages
        .iter()
        .filter(|id| !target_set.contains(id))
        .cloned()
        .collect();
    let extra: Vec<String> = target
        .messages
        .iter()
        .filter(|id| !source_set.contains(id))
        .cloned()
        .collect();
    let common = target
        .messages
        .iter()
        .filter(|id| source_set.contains(id))
        .count();

    if !ordered {
        return json!({
            "status": "unordered",
            "reason": "目标现有的记录**不是**源的开头一段（少的是中间几条 / 顺序不同）⇒ 追加会**打乱会话顺序**",
            "appendable": false,
            "commonMessages": common,
            "missingMessages": missing,
            "extraMessages": extra,
            "divergentMessages": [],
        });
    }

    let status = if missing.is_empty() && extra.is_empty() {
        "identical"
    } else {
        "appendable"
    };
    json!({
        "status": status,
        "reason": if status == "identical" {
            "逐条一致，无需同步".to_string()
        } else {
            format!("目标恰好是源的开头一段 ⇒ 可安全追加 {} 条（目标已有内容一字不动）", missing.len())
        },
        "appendable": status == "appendable",
        "commonMessages": common,
        "missingMessages": missing,
        "extraMessages": extra,
        "divergentMessages": [],
    })
}

/// [`diff_conversation_in`] 的**生产入口**：自己解析数据根。
///
/// ⚠️ 数据根找不到 ⇒ 回 `status: "error"`（**保持返回形状统一**）而不是 `Err`：
/// 这是**读**操作，前端拿到 `status` 就能统一渲染；而写操作（[`sync_conversation`]）
/// 才用 `Err` —— 用户点了「同步」就必须知道到底做没做。
pub fn diff_conversation(
    client: ExtClient,
    source_uid: &str,
    target_uid: &str,
    workspace_hash: &str,
    conversation_id: &str,
) -> Value {
    match data_root() {
        Some(root) => diff_conversation_in(
            &root,
            client,
            source_uid,
            target_uid,
            workspace_hash,
            conversation_id,
        ),
        None => json!({
            "status": "error",
            "reason": "未检测到客户端数据目录",
            "appendable": false,
            "commonMessages": 0,
            "missingMessages": [],
            "extraMessages": [],
            "divergentMessages": [],
        }),
    }
}

/// [`sync_conversation_in`] 的**生产入口**：自己解析数据根。
///
/// ⚠️ 数据根找不到 ⇒ **`Err`**（不是「同步了 0 条」）—— 后者会把「客户端没装」
/// 伪装成一次成功的空操作（「失败不得谎报成功」）。
pub fn sync_conversation(
    client: ExtClient,
    source_uid: &str,
    target_uid: &str,
    workspace_hash: &str,
    conversation_id: &str,
) -> Result<Value, String> {
    let root = data_root().ok_or_else(|| "未检测到客户端数据目录，无法同步会话".to_string())?;
    Ok(sync_conversation_in(
        &root,
        client,
        source_uid,
        target_uid,
        workspace_hash,
        conversation_id,
    ))
}

// ---------------------------------------------------------------------------
// P14 S4b · 关联（「这份会话在哪些账号里有副本」）
// ---------------------------------------------------------------------------

/// 插件侧会话**关联账本**的文件名。
///
/// ## ★ 为什么**另起一个文件**，不并进桌面版的 `buddy-switch-copy-ledger.json`
///
/// 1. 两个客户端的会话 id 是**两个 id 空间**，混进一个键空间迟早撞上；
/// 2. 桌面版账本是 P8 **去重机制**的一部分（`ledger_hit` 决定「复制过就不再重复复制」）——
///    往里塞插件侧的条目会**静默改变 P8 行为**，而且**不可逆**（清掉就再也恢复不了）。
///
/// ⇒ 插件侧有自己的账本，只服务一件事：「谁和谁是同一份会话」。
///
/// ⚠️ **不按 region 分文件**：桌面版按 region 分家（`~/.workbuddy` vs `~/.workbuddy-ai`），
/// 但**插件的数据根没有 region 维度** —— region 已经体现在 uid 里（不同 region 是不同账号）。
const LINK_LEDGER_FILE: &str = "vscode-session-links.json";

/// 关联账本路径（工具自己的存储根下，**不在客户端数据目录里**）。
fn link_ledger_path(store_root: &Path) -> PathBuf {
    store_root.join(LINK_LEDGER_FILE)
}

/// 关联键：`client ␟ workspaceHash ␟ sourceUid ␟ targetUid ␟ sourceCid`。
///
/// ★ **客户端与工作区都要进键**：同一个 `conversationId` 在 VS Code 树与 IDE 树里是
/// **两份不同的会话**，在不同工作区里也是不同会话。少一段就会把它们**当成同一对**。
fn link_key(
    client: ExtClient,
    workspace_hash: &str,
    source_uid: &str,
    target_uid: &str,
    source_cid: &str,
) -> String {
    const SEP: char = '\u{1f}';
    format!(
        "{}{SEP}{workspace_hash}{SEP}{source_uid}{SEP}{target_uid}{SEP}{source_cid}",
        client.as_str()
    )
}

/// 读关联账本（缺失 / 损坏 ⇒ 空表，**不 panic**）。
fn load_links(store_root: &Path) -> serde_json::Map<String, Value> {
    let Ok(text) = std::fs::read_to_string(link_ledger_path(store_root)) else {
        return serde_json::Map::new();
    };
    match serde_json::from_str::<Value>(&text) {
        Ok(Value::Object(map)) => map,
        _ => serde_json::Map::new(),
    }
}

pub(crate) struct LegacyCopyLink {
    pub workspace_hash: String,
    pub source_uid: String,
    pub target_uid: String,
    pub source_id: String,
    pub target_id: String,
    pub linked_at: Value,
}

pub(crate) fn legacy_copy_links(store_root: &Path, client: ExtClient) -> Vec<LegacyCopyLink> {
    let mut links: Vec<_> = load_links(store_root)
        .into_iter()
        .filter_map(|(key, value)| {
            let mut fields = key.split('\u{1f}');
            if fields.next()? != client.as_str() {
                return None;
            }
            let link = LegacyCopyLink {
                workspace_hash: fields.next()?.to_owned(),
                source_uid: fields.next()?.to_owned(),
                target_uid: fields.next()?.to_owned(),
                source_id: fields.next()?.to_owned(),
                target_id: value.get("targetCid")?.as_str()?.to_owned(),
                linked_at: value.get("at").cloned().unwrap_or(Value::Null),
            };
            fields.next().is_none().then_some(link)
        })
        .collect();
    links.sort_by_key(|link| std::cmp::Reverse(link.linked_at.as_i64().unwrap_or(0)));
    links
}

pub(crate) fn record_copy_links(
    store_root: &Path,
    client: ExtClient,
    source_uid: &str,
    target_uid: &str,
    report: &Value,
) -> bool {
    let entries: Vec<_> = report["copied"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|row| {
            let source_id = row.get("oldId").or_else(|| row.get("sourceId"))?.as_str()?;
            let workspace = row.get("workspaceHash")?.as_str()?;
            let target_id = row.get("newId")?.as_str()?;
            Some((
                link_key(client, workspace, source_uid, target_uid, source_id),
                target_id.to_owned(),
            ))
        })
        .collect();
    record_links(store_root, &entries)
}

/// 登记一批关联（**新记录覆盖旧记录** —— 指向**最新**的那份副本）。
///
/// ## ⚠️ 为什么是「覆盖」而不是「保留第一次」
///
/// 同一对账号**再复制一次**会在目标里生成**又一份**新会话（副本恒取新 id）。
/// 若保留第一次的键值，账本会一直指向**旧的那份** —— 用户看到的「对应会话」
/// 会是一份越来越过时的副本。**指向最新**才是可用的语义。
/// 代价：旧副本的关联会丢（它仍在目标账号里，只是不再被本账本串起来）。
///
/// 返回是否落盘成功。**失败不致命**（本次复制的产物仍然有效，只是「关联」这条信息丢了
/// —— 下次对同一对账号再复制就会补上），但**必须如实回报**，不能假装登记成功。
fn record_links(store_root: &Path, entries: &[(String, String)]) -> bool {
    if entries.is_empty() {
        return true;
    }
    if std::fs::create_dir_all(store_root).is_err() {
        return false;
    }
    let Ok(lock) = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(store_root.join("vscode-session-copy-ledger.lock"))
    else {
        return false;
    };
    if lock.lock().is_err() {
        return false;
    }
    let mut ledger = load_links(store_root);
    let at = crate::modules::config::now_ms();
    for (key, target_cid) in entries {
        ledger.insert(key.clone(), json!({ "targetCid": target_cid, "at": at }));
    }
    let path = link_ledger_path(store_root);
    let content = serde_json::to_string_pretty(&Value::Object(ledger)).unwrap_or_default();
    crate::modules::config::atomic_write(&path, &content).is_ok()
}

/// 查一条会话的**全部关联**（**双向**：它作为源、以及它作为目标的那些）。
///
/// 返回 `{ "links": [ { workspaceHash, sourceUid, targetUid, sourceCid, targetCid, at } ] }`，
/// 按 `at` **降序**（最近的关联排前面）。
///
/// 关联的用途：**不用让用户手工指名「哪两条是一对」** ——
/// 有了它，界面才能直接给出「这份会话在另一个账号里的对应副本」，
/// 进而调 [`diff_conversation`] / [`sync_conversation`]。
pub fn links_for(client: ExtClient, workspace_hash: &str, conversation_id: &str) -> Value {
    let links: Vec<_> = legacy_copy_links(&crate::modules::config::store_dir(), client)
        .into_iter()
        .filter(|link| {
            link.workspace_hash == workspace_hash
                && (link.source_id == conversation_id || link.target_id == conversation_id)
        })
        .map(|link| {
            json!({
                "workspaceHash": link.workspace_hash,
                "sourceUid": link.source_uid,
                "targetUid": link.target_uid,
                "sourceCid": link.source_id,
                "targetCid": link.target_id,
                "at": link.linked_at,
            })
        })
        .collect();
    json!({ "links": links })
}

/// 会话复制的备份根：`<工具存储根>/backups/vscode-sessions/<utc_iso>/`。
///
/// 插件侧与 IDE 侧**共用**这一个根（同一棵同构的树、同一套语义）；
/// 需要区分时靠文件名里的工作区 hash。
fn session_backup_root() -> PathBuf {
    crate::modules::config::backup_dir()
        .join("vscode-sessions")
        .join(crate::modules::config::utc_iso())
}

/// 增量追加：把源**尾部多出来**的记录补到目标。
///
/// ## ★★ 三条硬纪律
///
/// 1. **不信任调用方**：进来先**自己重算** [`diff_conversation_in`]，只有 `appendable` 才动手。
///    调用方传来的「我算过了，可以同步」不算数 —— 两件事之间源可能又变了。
/// 2. **只加不改**：目标已有的消息文件**一个字节都不碰**（只补缺失的）；索引只**追加**尾部。
/// 3. **索引是提交点**：先把消息文件补齐，**最后**才写索引（写前备份）。
///    索引写失败 ⇒ **删掉本次新拷的消息文件**回退 —— 否则会留下「索引里没有、磁盘上躺着」
///    的孤儿消息（扩展下次扫描会把它当半成品）。
///
/// ## 为什么 `unordered` / `divergent` 一律拒绝
///
/// 那两种情况下「同步」必然要么**打乱会话顺序**、要么**静默改写目标已有内容**，
/// 且都看不出来。⇒ 拒绝并**如实说原因**，把选择权交回用户（与桌面版 `diverge` 同一立场）。
pub fn sync_conversation_in(
    root: &Path,
    client: ExtClient,
    source_uid: &str,
    target_uid: &str,
    workspace_hash: &str,
    conversation_id: &str,
) -> Value {
    // ★ 纪律 1：自己重算，不信调用方。
    let diff = diff_conversation_in(
        root,
        client,
        source_uid,
        target_uid,
        workspace_hash,
        conversation_id,
    );
    if diff["status"] != json!("appendable") {
        return json!({
            "ok": false,
            "status": diff["status"],
            "reason": diff["reason"],
            "addedMessages": [],
            "backup": Value::Null,
        });
    }

    let source_dir = history_root_in(root, client, source_uid)
        .join(workspace_hash)
        .join(conversation_id);
    let target_dir = history_root_in(root, client, target_uid)
        .join(workspace_hash)
        .join(conversation_id);

    let missing: Vec<String> = diff["missingMessages"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();

    // ① 先补消息文件（**已存在的一律跳过** ⇒ 目标已有内容不会被改写）。
    let mut copied: Vec<PathBuf> = Vec::new();
    let mut added: Vec<String> = Vec::new();
    std::fs::create_dir_all(target_dir.join("messages")).ok();
    for id in &missing {
        let from = source_dir.join("messages").join(format!("{id}.json"));
        let to = target_dir.join("messages").join(format!("{id}.json"));
        if to.is_file() {
            // 理论上不会发生（diff 说它缺）—— 真发生时**以目标为准**，绝不覆盖。
            continue;
        }
        if to.exists() {
            // ★ 存在但**不是文件**（目录 / 别的什么）⇒ **不能**当成「已经有了」：
            //   那样索引会声称这条消息存在，而正文其实不可用 —— **静默损坏**。
            for path in &copied {
                let _ = std::fs::remove_file(path);
            }
            return json!({
                "ok": false,
                "status": "io-error",
                "reason": format!("目标已有同名的非文件条目，拒绝覆盖：{}", to.display()),
                "addedMessages": [],
                "backup": Value::Null,
            });
        }
        if let Err(error) = std::fs::copy(&from, &to) {
            for path in &copied {
                let _ = std::fs::remove_file(path);
            }
            return json!({
                "ok": false,
                "status": "io-error",
                "reason": format!("复制消息 {id} 失败：{error}"),
                "addedMessages": [],
                "backup": Value::Null,
            });
        }
        copied.push(to);
        added.push(id.clone());
    }

    // ② 索引是**提交点**：写前备份（并复核真的落盘）。
    let Some(source_index) = read_json(&source_dir.join("index.json")) else {
        for path in &copied {
            let _ = std::fs::remove_file(path);
        }
        return json!({ "ok": false, "status": "io-error", "reason": "源会话索引读不到",
                       "addedMessages": [], "backup": Value::Null });
    };
    let mut merged = read_json(&target_dir.join("index.json")).unwrap_or_else(|| json!({}));
    if let Some(object) = merged.as_object_mut() {
        // 只**替换这两条序列**（它们就是「会话内容」）；目标索引里其它字段原样保留。
        for key in ["messages", "requests"] {
            if let Some(value) = source_index.get(key) {
                object.insert(key.to_string(), value.clone());
            }
        }
    }

    let index_path = target_dir.join("index.json");
    let backup_root = session_backup_root();
    let mut backup: Option<String> = None;
    if index_path.is_file() {
        if std::fs::create_dir_all(&backup_root).is_err() {
            for path in &copied {
                let _ = std::fs::remove_file(path);
            }
            return json!({ "ok": false, "status": "io-error", "reason": "建不了备份目录（**未改写**）",
                           "addedMessages": [], "backup": Value::Null });
        }
        // ⚠️ 备份文件名**必须带工作区 hash**：只写 `<conversationId>-index.json` 时，
        //    两个不同工作区（或两次并发调用）里同名会话会**互相覆盖备份** ——
        //    备份被覆盖比没有备份更坏（用户以为有退路，其实没有）。
        let target = backup_root.join(format!("{workspace_hash}-{conversation_id}-index.json"));
        if std::fs::copy(&index_path, &target).is_err() {
            for path in &copied {
                let _ = std::fs::remove_file(path);
            }
            return json!({ "ok": false, "status": "io-error", "reason": "备份目标索引失败（**未改写**）",
                           "addedMessages": [], "backup": Value::Null });
        }
        backup = Some(target.to_string_lossy().into_owned());
    }
    if let Err(error) = crate::modules::config::atomic_write(&index_path, &merged.to_string()) {
        // ③ 纪律 3：索引没写成 ⇒ 删掉本次新拷的消息文件（不留孤儿）。
        for path in &copied {
            let _ = std::fs::remove_file(path);
        }
        return json!({
            "ok": false,
            "status": "io-error",
            "reason": format!("写入目标索引失败：{error}"),
            "addedMessages": [],
            "backup": backup,
        });
    }

    json!({
        "ok": true,
        "status": "synced",
        "reason": format!("已追加 {} 条（目标已有内容一字未动）", added.len()),
        "addedMessages": added,
        "backup": backup,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::config::HomeOverrideGuard;

    /// 建一个临时数据根 + 一棵会话树；返回 `(home, root)`。
    ///
    /// ⚠️ **刻意不在这里取 `HomeOverrideGuard`**：那把锁是**进程级独占**的，
    /// 每个用例都拿一次会让所有用例**串行**（本仓踩过「guard 自锁死 / 假失败」）。
    /// 本模块的判据函数全部是 `_in(root, …)` 形态、**显式吃路径** ⇒ 大多数用例
    /// **根本不需要** home 覆盖。只有真要调 `data_root()` / `data_root_candidates()`
    /// 的用例才自己取一次（见 `missing_data_root_…`）。
    fn fixture(tag: &str) -> (PathBuf, PathBuf) {
        let home = std::env::temp_dir().join(format!(
            "buddy-switch-vssess-{tag}-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let root = home.join("AppData/Local").join(APP_DIR).join("Data");
        std::fs::create_dir_all(&root).unwrap();
        (home, root)
    }

    /// 在数据根下写一个工作区目录 + 其 `index.json`。
    fn seed_workspace(
        root: &Path,
        client: ExtClient,
        uid: &str,
        workspace_hash: &str,
        conversations: Value,
    ) -> PathBuf {
        let ws_dir = history_root_in(root, client, uid).join(workspace_hash);
        std::fs::create_dir_all(&ws_dir).unwrap();
        std::fs::write(
            ws_dir.join("index.json"),
            serde_json::to_string(&json!({ "conversations": conversations, "current": null }))
                .unwrap(),
        )
        .unwrap();
        ws_dir
    }

    /// ★ 路径布局：`<root>/<uid>/<客户端段>/<uid>/history`（uid 出现**两次**是布局本身的形状）。
    #[test]
    fn layout_puts_the_uid_twice_and_differs_only_by_client_segment() {
        let root = Path::new("/data");
        assert_eq!(
            history_root_in(root, ExtClient::Vscode, "u1"),
            Path::new("/data")
                .join("u1")
                .join("VSCode")
                .join("u1")
                .join("history")
        );
        assert_eq!(
            history_root_in(root, ExtClient::CodebuddyIde, "u1"),
            Path::new("/data")
                .join("u1")
                .join("CodeBuddyIDE")
                .join("u1")
                .join("history")
        );
    }

    /// ★ 数据根候选：必须落在**本仓 home 覆盖**之下（否则单测会读真实 home）。
    ///
    /// 可证伪：把 `data_root_candidates` 里的 `home_dir()` 换成 `dirs::data_local_dir()`
    /// ⇒ 候选不再以临时 home 开头 ⇒ 红。
    #[test]
    fn data_root_candidates_are_under_the_overridden_home() {
        let (home, _root) = fixture("roots");
        // ★ 只有这一条用例真需要 home 覆盖（`data_root_candidates()` 走 `home_dir()`）。
        let guard = HomeOverrideGuard::set(&home);
        for candidate in data_root_candidates() {
            assert!(
                candidate.starts_with(&home),
                "候选必须落在被覆盖的 home 下：{}",
                candidate.display()
            );
            assert!(candidate.ends_with(Path::new(APP_DIR).join("Data")));
        }
        drop(guard);
        std::fs::remove_dir_all(home).unwrap();
    }

    /// ★ uid 白名单：脏 uid 一律拒（否则一个 `..` 就能让我们写到仓外）。
    ///
    /// 可证伪：去掉 `contains(['/', '\\'])` 与 `default`/`Public` 判定 ⇒ 红。
    #[test]
    fn unsafe_uids_are_rejected() {
        for good in ["uid-a", "a1b2c3", "UPPER", "with space"] {
            assert!(is_safe_uid(good), "{good} 应当被接受");
        }
        for bad in [
            "", "  ", ".", "..", "a/b", "a\\b", "default", "DEFAULT", "Public", "public",
        ] {
            assert!(!is_safe_uid(bad), "{bad:?} 必须被拒绝");
        }
    }

    /// ★ 脏 uid ⇒ **空列表且不进文件系统**（不是「抛错」，也不是「碰运气找目录」）。
    #[test]
    fn dirty_uid_yields_an_empty_listing_without_touching_the_filesystem() {
        let (home, root) = fixture("dirty-uid");
        // 故意造一个「如果拼路径就会命中」的目录：uid = ".." 会退到 root 上一级。
        let out = list_sessions_in(&root, ExtClient::Vscode, "..");
        assert_eq!(out["sessions"], json!([]));
        assert_eq!(out["skipped"], json!(0));
        assert_eq!(out["sourceUid"], json!(".."));
        std::fs::remove_dir_all(home).unwrap();
    }

    /// ★★ 只读枚举：按工作区分桶列出会话，字段逐个对上。
    ///
    /// 可证伪：把 `history_root_in` 的客户端段写死成 `VSCode` ⇒ 第二条（IDE 树）红。
    #[test]
    fn listing_reads_both_client_trees_by_their_own_segment() {
        let (home, root) = fixture("listing");

        seed_workspace(
            &root,
            ExtClient::Vscode,
            "uid-a",
            "ws-aaa",
            json!([
                { "id": "aaaa1111aaaa1111aaaa1111aaaa1111", "name": "旧会话",
                  "createdAt": "2026-10-01T00:00:00Z", "lastMessageAt": "2026-10-02T00:00:00Z" },
                { "id": "bbbb2222bbbb2222bbbb2222bbbb2222", "name": "新会话",
                  "createdAt": "2026-10-05T00:00:00Z" }
            ]),
        );
        seed_workspace(
            &root,
            ExtClient::CodebuddyIde,
            "uid-a",
            "ws-bbb",
            json!([{ "id": "cccc3333cccc3333cccc3333cccc3333", "name": "IDE 侧会话" }]),
        );

        let vscode = list_sessions_in(&root, ExtClient::Vscode, "uid-a");
        let sessions = vscode["sessions"].as_array().unwrap();
        assert_eq!(sessions.len(), 2, "只该看到插件侧那棵树");
        // 最近更新的排前面：新会话（10-05）在旧会话（10-02）之前。
        assert_eq!(sessions[0]["id"], json!("bbbb2222bbbb2222bbbb2222bbbb2222"));
        assert_eq!(sessions[0]["title"], json!("新会话"));
        assert_eq!(sessions[0]["workspaceHash"], json!("ws-aaa"));
        assert_eq!(sessions[1]["id"], json!("aaaa1111aaaa1111aaaa1111aaaa1111"));
        assert_eq!(vscode["skipped"], json!(0));
        assert_eq!(vscode["sourceUid"], json!("uid-a"));
        assert_eq!(vscode["dataRoot"].as_str().unwrap(), root.to_string_lossy());

        let ide = list_sessions_in(&root, ExtClient::CodebuddyIde, "uid-a");
        let ide_sessions = ide["sessions"].as_array().unwrap();
        assert_eq!(ide_sessions.len(), 1);
        assert_eq!(
            ide_sessions[0]["id"],
            json!("cccc3333cccc3333cccc3333cccc3333")
        );
        assert_eq!(ide_sessions[0]["workspaceHash"], json!("ws-bbb"));

        std::fs::remove_dir_all(home).unwrap();
    }

    /// ★ 损坏的工作区索引 ⇒ **计入 `skipped` 并继续**（不吞、不炸、不伪造会话）。
    ///
    /// 可证伪：把 `skipped += 1` 去掉（静默跳过）⇒ 第二条断言红。
    #[test]
    fn corrupt_workspace_index_is_counted_and_does_not_abort_the_rest() {
        let (home, root) = fixture("corrupt");

        seed_workspace(
            &root,
            ExtClient::Vscode,
            "uid-a",
            "ws-good",
            json!([{ "id": "dddd4444dddd4444dddd4444dddd4444", "name": "好会话" }]),
        );
        // 损坏的工作区索引（不是 JSON）。
        let broken = history_root_in(&root, ExtClient::Vscode, "uid-a").join("ws-broken");
        std::fs::create_dir_all(&broken).unwrap();
        std::fs::write(broken.join("index.json"), b"{ not json").unwrap();
        // 结构不符（没有 conversations 数组）——与「损坏」同款处理。
        let odd = history_root_in(&root, ExtClient::Vscode, "uid-a").join("ws-odd");
        std::fs::create_dir_all(&odd).unwrap();
        std::fs::write(odd.join("index.json"), b"{\"conversations\": 1}").unwrap();

        let out = list_sessions_in(&root, ExtClient::Vscode, "uid-a");
        assert_eq!(
            out["sessions"].as_array().unwrap().len(),
            1,
            "好的那条必须还在"
        );
        assert_eq!(out["skipped"], json!(2), "两个坏索引都要如实报数");

        std::fs::remove_dir_all(home).unwrap();
    }

    /// ★ `hasHistory` 判据要**索引与磁盘都看**（只看一条会把有正文的误报成空会话）。
    ///
    /// 可证伪：把 `conversation_has_history` 里的磁盘 `messages/` 分支删掉 ⇒ 第二条红。
    #[test]
    fn has_history_checks_both_the_index_and_the_disk() {
        let (home, root) = fixture("history");

        let ws_dir = seed_workspace(
            &root,
            ExtClient::Vscode,
            "uid-a",
            "ws-1",
            json!([
                { "id": "eeee5555eeee5555eeee5555eeee5555", "name": "索引里有消息" },
                { "id": "ffff6666ffff6666ffff6666ffff6666", "name": "只有磁盘文件" },
                { "id": "9999aaaa9999aaaa9999aaaa9999aaaa", "name": "空的" }
            ]),
        );
        // ① 索引里有非空 messages。
        let first = ws_dir.join("eeee5555eeee5555eeee5555eeee5555");
        std::fs::create_dir_all(&first).unwrap();
        std::fs::write(first.join("index.json"), b"{\"messages\":[{\"id\":\"x\"}]}").unwrap();
        // ② 索引里 messages 为空，但磁盘 messages/ 下有文件。
        let second = ws_dir.join("ffff6666ffff6666ffff6666ffff6666");
        std::fs::create_dir_all(second.join("messages")).unwrap();
        std::fs::write(second.join("index.json"), b"{\"messages\":[]}").unwrap();
        std::fs::write(second.join("messages").join("m1.json"), b"{}").unwrap();
        // ③ 两边都空。
        let third = ws_dir.join("9999aaaa9999aaaa9999aaaa9999aaaa");
        std::fs::create_dir_all(&third).unwrap();
        std::fs::write(third.join("index.json"), b"{\"messages\":[]}").unwrap();

        let out = list_sessions_in(&root, ExtClient::Vscode, "uid-a");
        let by_id = |id: &str| {
            out["sessions"]
                .as_array()
                .unwrap()
                .iter()
                .find(|session| session["id"] == json!(id))
                .unwrap()
                .clone()
        };
        assert_eq!(
            by_id("eeee5555eeee5555eeee5555eeee5555")["hasHistory"],
            json!(true)
        );
        assert_eq!(
            by_id("ffff6666ffff6666ffff6666ffff6666")["hasHistory"],
            json!(true),
            "只有磁盘 messages/ 文件时也算有正文"
        );
        assert_eq!(
            by_id("9999aaaa9999aaaa9999aaaa9999aaaa")["hasHistory"],
            json!(false)
        );

        std::fs::remove_dir_all(home).unwrap();
    }

    /// ★ 时间归一：RFC3339 字符串 / 秒 / 毫秒三种形态都要认；认不出 ⇒ 0（**不臆造**）。
    #[test]
    fn time_fields_are_normalized_or_zero() {
        assert_eq!(
            time_to_ms(Some(&json!("2026-10-02T00:00:00Z"))),
            1_790_899_200_000
        );
        assert_eq!(
            time_to_ms(Some(&json!(1_790_899_200i64))),
            1_790_899_200_000,
            "秒 ⇒ 毫秒"
        );
        assert_eq!(
            time_to_ms(Some(&json!(1_790_899_200_000i64))),
            1_790_899_200_000,
            "毫秒原样"
        );
        for junk in [json!(""), json!("not-a-time"), json!(null), json!({})] {
            assert_eq!(time_to_ms(Some(&junk)), 0, "{junk} 认不出 ⇒ 0");
        }
        assert_eq!(time_to_ms(None), 0);
    }

    /// ★ `is_hex32`：只认 32 位**小写** hex —— 大写 / 带连字符的 UUID / 短串都不认。
    ///
    /// 用途见模块文档：`messages/` 目录里的非 hex 文件（调试文件）必须**原样复制**，
    /// 不能被当成消息 id 改名。
    #[test]
    fn hex32_predicate_is_strict() {
        assert!(is_hex32("aaaa1111aaaa1111aaaa1111aaaa1111"));
        assert!(is_hex32("0123456789abcdef0123456789abcdef"));
        assert!(!is_hex32("AAAA1111AAAA1111AAAA1111AAAA1111"), "大写不认");
        assert!(
            !is_hex32("aaaa1111-aaaa-1111-aaaa-111111111111"),
            "带连字符的 UUID 不认"
        );
        assert!(!is_hex32("aaaa1111aaaa1111aaaa1111aaaa111"), "31 位不认");
        assert!(
            !is_hex32("gggg1111gggg1111gggg1111gggg1111"),
            "非 hex 字符不认"
        );
    }

    // ---------------------------------------------------------------- 写半：复制

    const WS: &str = "0123456789abcdef0123456789abcdef";
    const CONV: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const MSG_1: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    const MSG_2: &str = "cccccccccccccccccccccccccccccccc";
    const REQ_1: &str = "dddddddddddddddddddddddddddddddd";
    const SRC_UID: &str = "uid-src";
    const DST_UID: &str = "uid-dst";

    /// 递归取目录快照（相对路径 + 原始字节，已排序）—— 用来断言「逐字节未变」。
    fn tree_snapshot(dir: &Path) -> Vec<(String, Vec<u8>)> {
        fn walk(base: &Path, dir: &Path, out: &mut Vec<(String, Vec<u8>)>) {
            let Ok(entries) = std::fs::read_dir(dir) else {
                return;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    walk(base, &path, out);
                } else if path.is_file() {
                    let rel = path
                        .strip_prefix(base)
                        .unwrap_or(&path)
                        .to_string_lossy()
                        .into_owned();
                    out.push((rel, std::fs::read(&path).unwrap_or_default()));
                }
            }
        }
        let mut out = Vec::new();
        walk(dir, dir, &mut out);
        out.sort();
        out
    }

    /// 在源工作区里造一个「有正文」的会话：索引（messages + requests）+ 两条消息文件。
    ///
    /// ⚠️ `MSG_1` 的消息正文里**故意**塞了它自己作为**子串**（`see <MSG_1>ff`）——
    /// 精确匹配规则若被写成子串替换，那条断言会立刻红。
    fn seed_conversation(
        root: &Path,
        client: ExtClient,
        uid: &str,
        ws: &str,
        conv: &str,
    ) -> PathBuf {
        let ws_dir = seed_workspace(
            root,
            client,
            uid,
            ws,
            json!([{ "id": conv, "name": "源会话", "type": "craft",
                     "lastMessageAt": "2026-10-02T00:00:00Z" }]),
        );
        let conv_dir = ws_dir.join(conv);
        std::fs::create_dir_all(conv_dir.join("messages")).unwrap();
        std::fs::write(
            conv_dir.join("index.json"),
            json!({
                "messages": [{ "id": MSG_1 }, { "id": MSG_2 }],
                "requests": [{ "id": REQ_1, "messages": [MSG_1, MSG_2] }],
            })
            .to_string(),
        )
        .unwrap();
        std::fs::write(
            conv_dir.join("messages").join(format!("{MSG_1}.json")),
            json!({
                "id": MSG_1,
                "role": "user",
                "message": format!("see {MSG_1}ff and prefix{MSG_1} for details"),
                // ★ `note` 里塞了 `REQ_1` 的**子串**：`extra` 是**真的会被重映射**的地方，
                //   所以只有这里才能测出「精确匹配 vs 子串替换」（见 substring 用例）。
                "extra": json!({
                    "requestId": REQ_1,
                    "responseId": REQ_1,
                    "modelId": "gpt-4",
                    "note": format!("see {REQ_1}ff and prefix{REQ_1}"),
                })
                .to_string(),
            })
            .to_string(),
        )
        .unwrap();
        std::fs::write(
            conv_dir.join("messages").join(format!("{MSG_2}.json")),
            json!({ "id": MSG_2, "role": "assistant" }).to_string(),
        )
        .unwrap();
        conv_dir
    }

    fn item(ws: &str, conv: &str) -> CopyItem {
        CopyItem {
            workspace_hash: ws.to_string(),
            conversation_id: conv.to_string(),
        }
    }

    /// ★★ S4a 写半：复制把**四种 id 引用**全部重映射，且**源目录逐字节未变**。
    ///
    /// 四种引用：① 会话 id（目录名 + 工作区索引条目）② `messages[].id` / 文件名
    /// ③ `requests[].id` ④ `requests[].messages[]` 与消息 `extra` 里的 `requestId`。
    ///
    /// 可证伪：把 `remap_session_index` 里 `requests[].messages[]` 那段删掉 ⇒
    /// 「请求里的消息 id 也换了」红；把 `write_conversation` 改成直接 `copy_dir_recursive`
    /// （不重映射）⇒ 新 id 断言与 extra 断言都红。
    #[test]
    fn copy_remaps_every_id_reference_and_leaves_the_source_untouched() {
        let (home, root) = fixture("copy-remap");
        let source_conv = seed_conversation(&root, ExtClient::Vscode, SRC_UID, WS, CONV);
        let source_before = tree_snapshot(&source_conv);

        let report = copy_conversations_in(
            &root,
            ExtClient::Vscode,
            SRC_UID,
            DST_UID,
            &[item(WS, CONV)],
        );
        assert_eq!(report["copiedCount"], json!(1), "报告：{report}");
        assert_eq!(report["failCount"], json!(0));
        let new_id = report["copied"][0]["newId"].as_str().unwrap().to_string();
        assert_ne!(new_id, CONV, "副本必须取新 id");
        assert!(is_hex32(&new_id), "新 id 必须是 32 位小写 hex：{new_id}");

        let target_conv = history_root_in(&root, ExtClient::Vscode, DST_UID)
            .join(WS)
            .join(&new_id);
        let index = read_json(&target_conv.join("index.json")).expect("副本索引");

        // ② messages[].id 换了，且**一一对应**（两个老 id → 两个不同新 id）。
        let new_msg_1 = index["messages"][0]["id"].as_str().unwrap().to_string();
        let new_msg_2 = index["messages"][1]["id"].as_str().unwrap().to_string();
        assert!(is_hex32(&new_msg_1) && is_hex32(&new_msg_2));
        assert_ne!(new_msg_1, MSG_1);
        assert_ne!(new_msg_2, MSG_2);
        assert_ne!(new_msg_1, new_msg_2, "两条消息不能映射到同一个新 id");

        // ③ requests[].id 换了。
        let new_req_1 = index["requests"][0]["id"].as_str().unwrap().to_string();
        assert!(is_hex32(&new_req_1));
        assert_ne!(new_req_1, REQ_1);

        // ④ requests[].messages[] 指向**新的**消息 id（漏改这里 ⇒ 扩展显示空会话）。
        assert_eq!(
            index["requests"][0]["messages"][0].as_str().unwrap(),
            new_msg_1
        );
        assert_eq!(
            index["requests"][0]["messages"][1].as_str().unwrap(),
            new_msg_2
        );

        // 消息文件已按新 id 改名，且文件内的 id / extra 引用同步。
        let msg_file = read_json(
            &target_conv
                .join("messages")
                .join(format!("{new_msg_1}.json")),
        )
        .expect("副本消息文件");
        assert_eq!(msg_file["id"].as_str().unwrap(), new_msg_1);
        let extra: Value =
            serde_json::from_str(msg_file["extra"].as_str().expect("extra 仍是字符串化 JSON"))
                .expect("extra 可解析");
        assert_eq!(extra["requestId"].as_str().unwrap(), new_req_1);
        assert_eq!(extra["responseId"].as_str().unwrap(), new_req_1);
        assert_eq!(
            extra["modelId"].as_str().unwrap(),
            "gpt-4",
            "modelId 不是 id ⇒ 绝不能被改"
        );

        // 工作区索引：新条目在，且 `current` 指向它（缺 `current` 会被扩展判为损坏）。
        let ws_index = read_json(
            &history_root_in(&root, ExtClient::Vscode, DST_UID)
                .join(WS)
                .join("index.json"),
        )
        .expect("目标工作区索引");
        assert_eq!(ws_index["current"].as_str().unwrap(), new_id);
        assert!(ws_index["conversations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| entry["id"].as_str() == Some(new_id.as_str())));

        // ★ 源目录**逐字节未变**（复制是只读源）。
        assert_eq!(tree_snapshot(&source_conv), source_before, "源目录不许被动");

        std::fs::remove_dir_all(home).unwrap();
    }

    /// ★★ 只有「**恰好等于**老 id」的字符串才重映射；**子串一律不动**。
    ///
    /// 这是本模块最危险的一处 —— 子串替换会**静默毁掉会话内容**，而且改完看不出来
    /// （文件还是合法 JSON）。
    ///
    /// ## ⚠️ 断言必须落在**真的会被重映射**的字段上
    ///
    /// 本用例初版把子串塞进 `message`（正文），结果**在子串变异下照样通过** ——
    /// 因为 `remap_message_file` 只碰 `id` 与 `extra`，**正文根本不经过
    /// [`replace_exact_ids`]** ⇒ 那条断言是**构造上恒真**的（典型的「永远绿」假护栏）。
    /// ⇒ 子串要塞进 **`extra`**（`note` 字段）里才测得到。
    ///
    /// 可证伪：把 `replace_exact_ids` 的 `map.get(text.as_str())` 改成子串匹配
    /// （`text.contains(old_id)`）⇒ 本用例立刻红（`note` 会被改掉）。
    #[test]
    fn remap_never_touches_ids_that_are_only_substrings_of_content() {
        let (home, root) = fixture("copy-substring");
        seed_conversation(&root, ExtClient::Vscode, SRC_UID, WS, CONV);

        let report = copy_conversations_in(
            &root,
            ExtClient::Vscode,
            SRC_UID,
            DST_UID,
            &[item(WS, CONV)],
        );
        let new_id = report["copied"][0]["newId"].as_str().unwrap().to_string();
        let target_conv = history_root_in(&root, ExtClient::Vscode, DST_UID)
            .join(WS)
            .join(&new_id);
        let index = read_json(&target_conv.join("index.json")).unwrap();
        let new_msg_1 = index["messages"][0]["id"].as_str().unwrap().to_string();
        let new_req_1 = index["requests"][0]["id"].as_str().unwrap().to_string();

        let msg_file = read_json(
            &target_conv
                .join("messages")
                .join(format!("{new_msg_1}.json")),
        )
        .expect("副本消息文件");
        // ① 正文完全不被重映射（连子串也不该动）。
        assert_eq!(
            msg_file["message"].as_str().unwrap(),
            format!("see {MSG_1}ff and prefix{MSG_1} for details"),
            "正文里的 id 子串必须原样保留"
        );

        let extra: Value =
            serde_json::from_str(msg_file["extra"].as_str().expect("extra")).expect("extra 可解析");
        // ② `extra` 里**整串等于**老 id 的字段被换掉……
        assert_eq!(extra["requestId"].as_str().unwrap(), new_req_1);
        // ③ ……而**只是包含**老 id 的字段**一个字都不许动**（← 真正的护栏）。
        assert_eq!(
            extra["note"].as_str().unwrap(),
            format!("see {REQ_1}ff and prefix{REQ_1}"),
            "`extra` 里只是**包含**老 id 的字符串绝不能被改（精确匹配，不是子串替换）"
        );

        std::fs::remove_dir_all(home).unwrap();
    }

    /// ★ 复制是**加法**：目标已有同 id 的会话 ⇒ 副本取新 id，目标那条**一字未动**。
    ///
    /// 可证伪：把 `unique_hex32` 换成「直接用源 id」⇒ 目标那条会被覆盖 ⇒ 红。
    #[test]
    fn copy_never_overwrites_existing_target_content() {
        // 本用例读进程级 HOME / 网关配置：必须与「改 HOME 的用例」串行，
        // 否则并行跑时会读到别人临时改过的家目录（曾因此 flaky）。
        let _env = crate::modules::config::env_lock();
        let (home, root) = fixture("copy-additive");
        seed_conversation(&root, ExtClient::Vscode, SRC_UID, WS, CONV);
        // 目标账号在同一工作区里**已经有**一条 id 恰好等于源 id 的会话（内容不同）。
        let target_ws = history_root_in(&root, ExtClient::Vscode, DST_UID).join(WS);
        std::fs::create_dir_all(target_ws.join(CONV)).unwrap();
        std::fs::write(
            target_ws.join(CONV).join("index.json"),
            json!({ "messages": [], "requests": [] }).to_string(),
        )
        .unwrap();
        std::fs::write(
            target_ws.join("index.json"),
            json!({ "conversations": [{ "id": CONV, "name": "目标原有" }], "current": CONV })
                .to_string(),
        )
        .unwrap();
        let target_before = tree_snapshot(&target_ws.join(CONV));

        let report = copy_conversations_in(
            &root,
            ExtClient::Vscode,
            SRC_UID,
            DST_UID,
            &[item(WS, CONV)],
        );
        assert_eq!(report["copiedCount"], json!(1), "报告：{report}");
        let new_id = report["copied"][0]["newId"].as_str().unwrap();
        assert_ne!(new_id, CONV, "撞上目标已有 id 时必须换新 id");

        assert_eq!(
            tree_snapshot(&target_ws.join(CONV)),
            target_before,
            "目标原有会话必须一字未动"
        );
        // 目标原有的 `current` 仍有效 ⇒ **保留**（不被本次并入改写）。
        let ws_index = read_json(&target_ws.join("index.json")).unwrap();
        assert_eq!(ws_index["current"].as_str().unwrap(), CONV);
        assert_eq!(ws_index["conversations"].as_array().unwrap().len(), 2);

        std::fs::remove_dir_all(home).unwrap();
    }

    /// ★ `messages/` 下的**非 hex 文件**（调试文件 / 锁文件）必须**按字节原样**复制：
    /// 既不改名，也不重新序列化（重新序列化会改掉它的格式）。
    #[test]
    fn non_hex_files_in_messages_are_copied_verbatim() {
        let (home, root) = fixture("copy-verbatim");
        let source_conv = seed_conversation(&root, ExtClient::Vscode, SRC_UID, WS, CONV);
        // 非规范 JSON（宽松空格）⇒ 一旦被重新序列化就会变形。
        let raw = b"{  \"debug\" :  1  ,\n  \"note\":\"keep me\" }";
        std::fs::write(source_conv.join("messages").join("debug.json"), raw).unwrap();

        let report = copy_conversations_in(
            &root,
            ExtClient::Vscode,
            SRC_UID,
            DST_UID,
            &[item(WS, CONV)],
        );
        let new_id = report["copied"][0]["newId"].as_str().unwrap();
        let copied = history_root_in(&root, ExtClient::Vscode, DST_UID)
            .join(WS)
            .join(new_id)
            .join("messages")
            .join("debug.json");
        assert_eq!(
            std::fs::read(&copied).unwrap(),
            raw,
            "非 hex 文件必须按字节原样复制（不改名、不重排格式）"
        );

        std::fs::remove_dir_all(home).unwrap();
    }

    /// ★ 索引写失败 ⇒ **回退**：本次新建的会话目录必须被删掉，不留孤儿。
    ///
    /// 可证伪：把 `copy_conversations_in` 里 `Err(error)` 分支的 `remove_dir_all_if_exists`
    /// 删掉 ⇒ 目标工作区里会多出一个「索引里没有、磁盘上躺着」的目录 ⇒ 红。
    #[test]
    fn failed_index_write_rolls_back_the_created_directories() {
        let (home, root) = fixture("copy-rollback");
        seed_conversation(&root, ExtClient::Vscode, SRC_UID, WS, CONV);
        // 用**目录**占住 `index.json` ⇒ 写索引必失败。
        let target_ws = history_root_in(&root, ExtClient::Vscode, DST_UID).join(WS);
        std::fs::create_dir_all(target_ws.join("index.json")).unwrap();

        let report = copy_conversations_in(
            &root,
            ExtClient::Vscode,
            SRC_UID,
            DST_UID,
            &[item(WS, CONV)],
        );
        assert_eq!(report["copiedCount"], json!(0), "报告：{report}");
        assert_eq!(report["failCount"], json!(1));
        assert!(
            report["errors"][0]["error"]
                .as_str()
                .unwrap_or_default()
                .contains("索引"),
            "失败原因要指明是索引：{}",
            report["errors"][0]["error"]
        );

        let names: Vec<String> = std::fs::read_dir(&target_ws)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            names,
            vec!["index.json".to_string()],
            "回退后不得留下孤儿会话目录：{names:?}"
        );

        std::fs::remove_dir_all(home).unwrap();
    }

    /// ★ 守卫：脏 uid / 同 uid / 脏工作区标识 / 脏会话标识 ⇒ 逐条记失败，**不碰文件系统**。
    #[test]
    fn unsafe_inputs_are_rejected_per_item() {
        let (home, root) = fixture("copy-guards");

        // 脏 uid（`..`）⇒ 全部失败。
        let report =
            copy_conversations_in(&root, ExtClient::Vscode, "..", DST_UID, &[item(WS, CONV)]);
        assert_eq!(report["copiedCount"], json!(0));
        assert!(report["errors"][0]["error"]
            .as_str()
            .unwrap()
            .contains("uid 不合法"));

        // 源 = 目标 ⇒ 全部失败。
        let report = copy_conversations_in(
            &root,
            ExtClient::Vscode,
            SRC_UID,
            SRC_UID,
            &[item(WS, CONV)],
        );
        assert!(report["errors"][0]["error"]
            .as_str()
            .unwrap()
            .contains("相同"));

        // 脏工作区标识（`../` 会写到别的账号甚至仓外）⇒ 该条失败。
        let report = copy_conversations_in(
            &root,
            ExtClient::Vscode,
            SRC_UID,
            DST_UID,
            &[item("../escape", CONV)],
        );
        assert!(report["errors"][0]["error"]
            .as_str()
            .unwrap()
            .contains("工作区标识不合法"));

        // 脏会话标识 ⇒ 该条失败。
        let report = copy_conversations_in(
            &root,
            ExtClient::Vscode,
            SRC_UID,
            DST_UID,
            &[item(WS, "not-hex")],
        );
        assert!(report["errors"][0]["error"]
            .as_str()
            .unwrap()
            .contains("会话标识不合法"));

        std::fs::remove_dir_all(home).unwrap();
    }

    /// ★★ S4c：**IDE 侧沿用源 id**，且内容与源**逐字节相同**（不重映射 ⇒ 不重新序列化）。
    ///
    /// 依据：同产品另一实现**真机实测** —— 沿用 id 的目标账号可直接加载。
    ///
    /// 可证伪：把 `ExtClient::copy_id_policy` 的 `CodebuddyIde` 改成 `AlwaysNew` ⇒
    /// 「必须沿用源 id」与「逐字节相同」两条都红。
    #[test]
    fn ide_copy_keeps_the_source_id_and_copies_bytes_verbatim() {
        let (home, root) = fixture("ide-keep");
        let source_conv = seed_conversation(&root, ExtClient::CodebuddyIde, SRC_UID, WS, CONV);
        let source_before = tree_snapshot(&source_conv);

        let report = copy_conversations_in(
            &root,
            ExtClient::CodebuddyIde,
            SRC_UID,
            DST_UID,
            &[item(WS, CONV)],
        );
        assert_eq!(report["copiedCount"], json!(1), "报告：{report}");
        assert_eq!(
            report["copied"][0]["newId"].as_str().unwrap(),
            CONV,
            "IDE 侧必须**沿用源 id**（真机实测：沿用 id 的目标账号可直接加载）"
        );

        let target_ws = history_root_in(&root, ExtClient::CodebuddyIde, DST_UID).join(WS);
        assert_eq!(
            tree_snapshot(&target_ws.join(CONV)),
            source_before,
            "沿用 id ⇒ 内容必须与源**逐字节相同**（走字节复制，不重映射、不重新序列化）"
        );
        let ws_index = read_json(&target_ws.join("index.json")).expect("目标工作区索引");
        assert_eq!(ws_index["current"].as_str().unwrap(), CONV);

        std::fs::remove_dir_all(home).unwrap();
    }

    /// ★★ S4c：**仅在冲突时**才取新 id —— 且此时走全量重映射（副本内部引用跟着换才自洽）。
    ///
    /// 可证伪：把冲突判定去掉（`KeepUnlessConflict` 直接沿用 id）⇒ 目标那条会被**覆盖** ⇒ 红。
    #[test]
    fn ide_copy_takes_a_new_id_only_on_conflict() {
        // 本用例读进程级 HOME / 网关配置：必须与「改 HOME 的用例」串行，
        // 否则并行跑时会读到别人临时改过的家目录（曾因此 flaky）。
        let _env = crate::modules::config::env_lock();
        let (home, root) = fixture("ide-conflict");
        seed_conversation(&root, ExtClient::CodebuddyIde, SRC_UID, WS, CONV);

        // 目标账号在同一工作区里**已经有**一条 id 恰好等于源 id 的会话（内容不同）。
        let target_ws = history_root_in(&root, ExtClient::CodebuddyIde, DST_UID).join(WS);
        std::fs::create_dir_all(target_ws.join(CONV)).unwrap();
        std::fs::write(
            target_ws.join(CONV).join("index.json"),
            json!({ "messages": [], "requests": [] }).to_string(),
        )
        .unwrap();
        std::fs::write(
            target_ws.join("index.json"),
            json!({ "conversations": [{ "id": CONV, "name": "目标原有" }], "current": CONV })
                .to_string(),
        )
        .unwrap();
        let target_before = tree_snapshot(&target_ws.join(CONV));

        let report = copy_conversations_in(
            &root,
            ExtClient::CodebuddyIde,
            SRC_UID,
            DST_UID,
            &[item(WS, CONV)],
        );
        assert_eq!(report["copiedCount"], json!(1), "报告：{report}");
        let new_id = report["copied"][0]["newId"].as_str().unwrap().to_string();
        assert_ne!(new_id, CONV, "撞上目标已有 id 时必须换新 id");
        assert!(is_hex32(&new_id));

        assert_eq!(
            tree_snapshot(&target_ws.join(CONV)),
            target_before,
            "目标原有会话必须一字未动"
        );
        // 冲突 ⇒ 全量重映射：副本内部的 messages[].id 与源**不同**（引用自洽）。
        let new_index = read_json(&target_ws.join(&new_id).join("index.json")).expect("副本索引");
        let new_msg_1 = new_index["messages"][0]["id"].as_str().unwrap();
        assert_ne!(new_msg_1, MSG_1, "换 id 时必须重映射副本内部的引用");
        assert_eq!(
            new_index["requests"][0]["messages"][0].as_str().unwrap(),
            new_msg_1,
            "`requests[].messages[]` 要指向新的消息 id"
        );

        std::fs::remove_dir_all(home).unwrap();
    }

    /// ★★ S4c：**索引备份（`.index_bak.json`）按客户端分家** —— 插件侧写、IDE 侧不写。
    ///
    /// 可证伪：把 `writes_workspace_index_backup` 改成恒 `true` ⇒ 第二条（IDE 侧不存在）红；
    /// 改成恒 `false` ⇒ 第一条（插件侧存在）红。**两个方向都钉住**。
    #[test]
    fn workspace_index_backup_is_written_for_the_extension_but_not_for_the_ide() {
        let (home, root) = fixture("index-backup");
        /* ⚠️ **两棵树都要播种**：复制只在**同一个客户端的树**里找源
        （`history_root_in(root, client, uid)` 带客户端段）—— 只种 vscode 那棵，
        用 IDE 客户端去复制会「找不到源会话」而静默复制 0 条。 */
        seed_conversation(&root, ExtClient::Vscode, SRC_UID, WS, CONV);
        seed_conversation(&root, ExtClient::CodebuddyIde, SRC_UID, WS, CONV);

        // ① 插件侧：必须写。
        copy_conversations_in(
            &root,
            ExtClient::Vscode,
            SRC_UID,
            DST_UID,
            &[item(WS, CONV)],
        );
        let ext_ws = history_root_in(&root, ExtClient::Vscode, DST_UID).join(WS);
        assert!(
            ext_ws.join(".index_bak.json").is_file(),
            "插件侧必须写 `.index_bak.json`（扩展自用的索引备份要与主索引一致）"
        );
        let ext_index = read_json(&ext_ws.join("index.json")).unwrap();
        let ext_backup = read_json(&ext_ws.join(".index_bak.json")).unwrap();
        assert_eq!(ext_backup, ext_index, "索引备份必须与主索引一致");

        // ② IDE 侧：**不写**（凭空造一个会让 IDE 读到它不认识的状态）。
        let ide_dst = "uid-dst-ide";
        copy_conversations_in(
            &root,
            ExtClient::CodebuddyIde,
            SRC_UID,
            ide_dst,
            &[item(WS, CONV)],
        );
        let ide_ws = history_root_in(&root, ExtClient::CodebuddyIde, ide_dst).join(WS);
        assert!(
            ide_ws.join("index.json").is_file(),
            "IDE 侧的主索引必须写出来（否则下面那条断言没意义）"
        );
        assert!(
            !ide_ws.join(".index_bak.json").exists(),
            "IDE 侧**不得**凭空造 `.index_bak.json`"
        );

        std::fs::remove_dir_all(home).unwrap();
    }

    // ---------------------------------------------------------------- S4b：记录级 diff

    /// 在指定账号 / 工作区里造一个会话：**有序**消息 id + 每条正文（原样字节）。
    fn seed_ordered(
        root: &Path,
        client: ExtClient,
        uid: &str,
        ws: &str,
        conv: &str,
        messages: &[(&str, &str)],
    ) {
        let ws_dir = history_root_in(root, client, uid).join(ws);
        std::fs::create_dir_all(ws_dir.join(conv).join("messages")).unwrap();
        let ids: Vec<Value> = messages.iter().map(|(id, _)| json!({ "id": id })).collect();
        std::fs::write(
            ws_dir.join(conv).join("index.json"),
            json!({
                "messages": ids,
                "requests": [{ "id": REQ_1, "messages": messages.iter().map(|(id, _)| *id).collect::<Vec<_>>() }],
                /* ★ 一个**无关字段**：用来断言「同步只替换 messages / requests 两条序列，
                   索引里其它字段必须原样保留」。 */
                "note": "keep-me",
            })
            .to_string(),
        )
        .unwrap();
        for (id, content) in messages {
            std::fs::write(
                ws_dir
                    .join(conv)
                    .join("messages")
                    .join(format!("{id}.json")),
                content,
            )
            .unwrap();
        }
        std::fs::write(
            ws_dir.join("index.json"),
            json!({ "conversations": [{ "id": conv }], "current": conv }).to_string(),
        )
        .unwrap();
    }

    fn diff_of(root: &Path, client: ExtClient) -> Value {
        diff_conversation_in(root, client, SRC_UID, DST_UID, WS, CONV)
    }

    /// ★★ S4b：目标**恰好是源的开头一段** ⇒ `appendable`（可安全追加，且已有内容一字不动）。
    ///
    /// 可证伪：把 `is_prefix` 改成恒 `true` ⇒ 下面「缺中间那条」的用例会变成 `appendable` ⇒ 红。
    #[test]
    fn diff_is_appendable_only_when_the_target_is_a_prefix_of_the_source() {
        let (home, root) = fixture("diff-appendable");
        seed_ordered(
            &root,
            ExtClient::Vscode,
            SRC_UID,
            WS,
            CONV,
            &[
                (MSG_1, "{\"id\":\"m1\"}"),
                (MSG_2, "{\"id\":\"m2\"}"),
                (REQ_1, "{\"id\":\"m3\"}"),
            ],
        );
        seed_ordered(
            &root,
            ExtClient::Vscode,
            DST_UID,
            WS,
            CONV,
            &[(MSG_1, "{\"id\":\"m1\"}"), (MSG_2, "{\"id\":\"m2\"}")],
        );

        let out = diff_of(&root, ExtClient::Vscode);
        assert_eq!(out["status"], json!("appendable"), "报告：{out}");
        assert_eq!(out["appendable"], json!(true));
        assert_eq!(out["commonMessages"], json!(2));
        assert_eq!(out["missingMessages"], json!([REQ_1]));

        std::fs::remove_dir_all(home).unwrap();
    }

    /// ★★ S4b：目标缺的是**中间**那几条 ⇒ `unordered`，**拒绝**（追加会打乱会话顺序）。
    ///
    /// 这是本模块存在的理由：桌面版按行 append **天然保序**，插件侧**不保序**。
    #[test]
    fn diff_refuses_when_the_missing_record_is_in_the_middle() {
        let (home, root) = fixture("diff-unordered");
        seed_ordered(
            &root,
            ExtClient::Vscode,
            SRC_UID,
            WS,
            CONV,
            &[
                (MSG_1, "{\"id\":\"m1\"}"),
                (MSG_2, "{\"id\":\"m2\"}"),
                (REQ_1, "{\"id\":\"m3\"}"),
            ],
        );
        // 目标缺**中间**那条（`m2`），却有后面那条 ⇒ 不是前缀。
        seed_ordered(
            &root,
            ExtClient::Vscode,
            DST_UID,
            WS,
            CONV,
            &[(MSG_1, "{\"id\":\"m1\"}"), (REQ_1, "{\"id\":\"m3\"}")],
        );

        let out = diff_of(&root, ExtClient::Vscode);
        assert_eq!(out["status"], json!("unordered"), "报告：{out}");
        assert_eq!(out["appendable"], json!(false));
        assert_eq!(out["missingMessages"], json!([MSG_2]));
        assert!(
            out["reason"].as_str().unwrap().contains("打乱"),
            "拒绝理由必须说清后果：{}",
            out["reason"]
        );

        std::fs::remove_dir_all(home).unwrap();
    }

    /// ★★ S4b：**同 id 但内容不同** ⇒ `divergent`，**拒绝**（同步会静默改写目标内容）。
    ///
    /// ⚠️ 判据是**字节**比较，不是解析后比较：`extra` 是字符串化 JSON，
    /// 解析 → 重新序列化会抹平键序 / 空白 ⇒ 把「内容其实不同」判成「相同」。
    #[test]
    fn diff_refuses_when_the_same_id_has_different_bytes() {
        let (home, root) = fixture("diff-divergent");
        seed_ordered(
            &root,
            ExtClient::Vscode,
            SRC_UID,
            WS,
            CONV,
            &[(MSG_1, "{\"id\":\"m1\",\"message\":\"A\"}")],
        );
        // 同一个 id，只差一个字节。
        seed_ordered(
            &root,
            ExtClient::Vscode,
            DST_UID,
            WS,
            CONV,
            &[(MSG_1, "{\"id\":\"m1\",\"message\":\"B\"}")],
        );

        let out = diff_of(&root, ExtClient::Vscode);
        assert_eq!(out["status"], json!("divergent"), "报告：{out}");
        assert_eq!(out["divergentMessages"], json!([MSG_1]));
        assert_eq!(out["appendable"], json!(false));
        assert!(
            out["reason"].as_str().unwrap().contains("静默改写"),
            "理由：{}",
            out["reason"]
        );

        std::fs::remove_dir_all(home).unwrap();
    }

    /// ★ S4b：目标**没有**这份会话 ⇒ `absent`（要走**整份复制**，不是增量）。
    /// 与 `appendable` 分开报，是因为**后续动作完全不同**。
    #[test]
    fn diff_reports_absent_when_the_target_has_no_such_conversation() {
        let (home, root) = fixture("diff-absent");
        seed_ordered(
            &root,
            ExtClient::Vscode,
            SRC_UID,
            WS,
            CONV,
            &[(MSG_1, "{}")],
        );

        let out = diff_of(&root, ExtClient::Vscode);
        assert_eq!(out["status"], json!("absent"), "报告：{out}");
        assert_eq!(out["appendable"], json!(false));

        std::fs::remove_dir_all(home).unwrap();
    }

    /// ★ S4b：逐条一致 ⇒ `identical`（无事可做，**不是** `appendable`）。
    #[test]
    fn diff_reports_identical_when_nothing_is_missing() {
        let (home, root) = fixture("diff-identical");
        let rows = [(MSG_1, "{\"id\":\"m1\"}"), (MSG_2, "{\"id\":\"m2\"}")];
        seed_ordered(&root, ExtClient::Vscode, SRC_UID, WS, CONV, &rows);
        seed_ordered(&root, ExtClient::Vscode, DST_UID, WS, CONV, &rows);

        let out = diff_of(&root, ExtClient::Vscode);
        assert_eq!(out["status"], json!("identical"), "报告：{out}");
        assert_eq!(
            out["appendable"],
            json!(false),
            "一致 ⇒ 没有可追加的，别报成可同步"
        );

        std::fs::remove_dir_all(home).unwrap();
    }

    /// ★ S4b：**源**缺失 / 标识非法 ⇒ `error`，且**不碰文件系统**（脏工作区标识不得当路径用）。
    #[test]
    fn diff_reports_error_for_missing_source_or_dirty_ids() {
        let (home, root) = fixture("diff-error");
        // 源不存在。
        let out = diff_of(&root, ExtClient::Vscode);
        assert_eq!(out["status"], json!("error"), "报告：{out}");

        // 脏工作区标识（`../` 会退到别的目录）。
        let out = diff_conversation_in(
            &root,
            ExtClient::Vscode,
            SRC_UID,
            DST_UID,
            "../escape",
            CONV,
        );
        assert_eq!(out["status"], json!("error"));
        assert!(out["reason"].as_str().unwrap().contains("不合法"));

        std::fs::remove_dir_all(home).unwrap();
    }

    // ---------------------------------------------------------------- S4b：增量追加

    /// ★★ S4b：`appendable` ⇒ **只补尾部**，目标已有消息文件**逐字节未动**，索引也只追加。
    ///
    /// 可证伪：把 `sync_conversation_in` 里「自己重算 diff」换成信任调用方 / 去掉判定 ⇒
    /// 下面「拒绝」那两条会变成写成功 ⇒ 红。
    #[test]
    fn sync_appends_the_missing_tail_and_leaves_the_target_untouched() {
        let (home, root) = fixture("sync-append");
        seed_ordered(
            &root,
            ExtClient::Vscode,
            SRC_UID,
            WS,
            CONV,
            &[
                (MSG_1, "{\"id\":\"m1\"}"),
                (MSG_2, "{\"id\":\"m2\"}"),
                (REQ_1, "{\"id\":\"m3\"}"),
            ],
        );
        seed_ordered(
            &root,
            ExtClient::Vscode,
            DST_UID,
            WS,
            CONV,
            &[(MSG_1, "{\"id\":\"m1\"}"), (MSG_2, "{\"id\":\"m2\"}")],
        );
        let target_dir = history_root_in(&root, ExtClient::Vscode, DST_UID)
            .join(WS)
            .join(CONV);
        /* ★ 给**目标**的会话索引加一个**源没有的**字段。
        ⚠️ 两侧夹具的 `note` 若**值相同**，「整份换成源的索引」与「只替换两条序列」
        就**区分不出来** —— 那条断言会变成永远绿（本用例初版正是如此，变异 S3 没打红）。 */
        let mut target_index = read_json(&target_dir.join("index.json")).unwrap();
        target_index["targetOnly"] = json!("survive-me");
        std::fs::write(target_dir.join("index.json"), target_index.to_string()).unwrap();
        let target_before = tree_snapshot(&target_dir);

        let out = sync_conversation_in(&root, ExtClient::Vscode, SRC_UID, DST_UID, WS, CONV);
        assert_eq!(out["ok"], json!(true), "报告：{out}");
        assert_eq!(out["status"], json!("synced"));
        assert_eq!(out["addedMessages"], json!([REQ_1]));

        // 目标已有的**消息文件**逐字节未动（`index.json` 是**提交点**，它本来就该变 —— 见下）。
        let after = tree_snapshot(&target_dir);
        for (rel, bytes) in &target_before {
            if rel == "index.json" {
                continue;
            }
            let now = after.iter().find(|(name, _)| name == rel).map(|(_, b)| b);
            assert_eq!(now, Some(bytes), "{rel} 必须逐字节未动");
        }
        // 索引里 messages 序列变成源的（尾部补齐），且**没有丢**已有字段。
        let index = read_json(&target_dir.join("index.json")).unwrap();
        assert_eq!(
            index["messages"].as_array().unwrap().len(),
            3,
            "索引应补齐到源的长度：{index}"
        );
        assert_eq!(
            index["targetOnly"].as_str().unwrap(),
            "survive-me",
            "★ 只替换 messages / requests 两条序列 —— 目标索引里**它自己**的字段必须原样保留"
        );
        assert!(out["backup"].as_str().is_some(), "改写索引前必须备份");

        std::fs::remove_dir_all(home).unwrap();
    }

    /// ★★ S4b：**不可追加时一个字都不许写** —— `unordered` 与 `divergent` 都要拒绝。
    ///
    /// 可证伪：把 `sync_conversation_in` 开头的 diff 判定删掉（无条件往下走）⇒
    /// 两条都变成写成功 ⇒ 红。
    #[test]
    fn sync_refuses_and_writes_nothing_when_not_appendable() {
        let (home, root) = fixture("sync-refuse");

        // ① 缺中间那条（unordered）。
        seed_ordered(
            &root,
            ExtClient::Vscode,
            SRC_UID,
            WS,
            CONV,
            &[(MSG_1, "{}"), (MSG_2, "{}"), (REQ_1, "{}")],
        );
        seed_ordered(
            &root,
            ExtClient::Vscode,
            DST_UID,
            WS,
            CONV,
            &[(MSG_1, "{}"), (REQ_1, "{}")],
        );
        let target_dir = history_root_in(&root, ExtClient::Vscode, DST_UID)
            .join(WS)
            .join(CONV);
        let before = tree_snapshot(&target_dir);

        let out = sync_conversation_in(&root, ExtClient::Vscode, SRC_UID, DST_UID, WS, CONV);
        assert_eq!(out["ok"], json!(false), "报告：{out}");
        assert_eq!(out["status"], json!("unordered"));
        assert_eq!(
            tree_snapshot(&target_dir),
            before,
            "拒绝时**一个字都不许写**"
        );

        std::fs::remove_dir_all(home).unwrap();
    }

    /// ★ S4b：**同 id 不同内容**也拒绝（`divergent`）—— 同步会静默改写目标已有内容。
    #[test]
    fn sync_refuses_when_the_same_id_differs() {
        let (home, root) = fixture("sync-divergent");
        seed_ordered(
            &root,
            ExtClient::Vscode,
            SRC_UID,
            WS,
            CONV,
            &[(MSG_1, "{\"v\":\"A\"}")],
        );
        seed_ordered(
            &root,
            ExtClient::Vscode,
            DST_UID,
            WS,
            CONV,
            &[(MSG_1, "{\"v\":\"B\"}")],
        );
        let target_dir = history_root_in(&root, ExtClient::Vscode, DST_UID)
            .join(WS)
            .join(CONV);
        let before = tree_snapshot(&target_dir);

        let out = sync_conversation_in(&root, ExtClient::Vscode, SRC_UID, DST_UID, WS, CONV);
        assert_eq!(out["status"], json!("divergent"), "报告：{out}");
        assert_eq!(out["ok"], json!(false));
        assert_eq!(
            tree_snapshot(&target_dir),
            before,
            "拒绝时**一个字都不许写**"
        );

        std::fs::remove_dir_all(home).unwrap();
    }

    /// ★★ S4b：**中途失败要回退** —— 先拷成功的那条必须被删掉，索引一字未动。
    ///
    /// 夹具：待补两条（`MSG_2`、`REQ_1`，按源顺序），把**第二条**的目标路径占成一个**目录**
    /// ⇒ 第一条先拷成功、第二条失败 ⇒ 走到回退分支。
    ///
    /// 可证伪：把 `sync_conversation_in` 里 `to.exists()` 的非文件分支改成 `continue`
    /// （当成「已经有了」）⇒ 它会「成功」并写索引，而那条消息的正文其实不可用 ⇒ 红。
    #[test]
    fn sync_rolls_back_copied_messages_when_a_later_copy_fails() {
        let (home, root) = fixture("sync-rollback");
        seed_ordered(
            &root,
            ExtClient::Vscode,
            SRC_UID,
            WS,
            CONV,
            &[(MSG_1, "{}"), (MSG_2, "{}"), (REQ_1, "{}")],
        );
        seed_ordered(
            &root,
            ExtClient::Vscode,
            DST_UID,
            WS,
            CONV,
            &[(MSG_1, "{}")],
        );

        let target_dir = history_root_in(&root, ExtClient::Vscode, DST_UID)
            .join(WS)
            .join(CONV);
        let index_before = std::fs::read(target_dir.join("index.json")).unwrap();
        std::fs::create_dir_all(target_dir.join("messages").join(format!("{REQ_1}.json"))).unwrap();

        let out = sync_conversation_in(&root, ExtClient::Vscode, SRC_UID, DST_UID, WS, CONV);
        assert_eq!(out["ok"], json!(false), "报告：{out}");
        assert_eq!(out["status"], json!("io-error"));
        assert_eq!(out["addedMessages"], json!([]), "失败时不许报出「已追加」");
        assert!(
            !target_dir
                .join("messages")
                .join(format!("{MSG_2}.json"))
                .exists(),
            "★ 回退必须删掉本次已拷进去的消息文件（不留孤儿）"
        );
        assert_eq!(
            std::fs::read(target_dir.join("index.json")).unwrap(),
            index_before,
            "索引必须**一字未动**"
        );

        std::fs::remove_dir_all(home).unwrap();
    }

    /// ★ S4b：生产入口的**数据根缺失**分支 —— diff 回 `status: "error"`、sync 回 **`Err`**。
    ///
    /// 两者刻意**不同**：diff 是**读**，保持返回形状统一（前端按 `status` 统一渲染）；
    /// sync 是**写**，用户点了「同步」就必须知道到底做没做 —— 回「同步了 0 条」会把
    /// 「客户端没装」伪装成一次成功的空操作。
    ///
    /// ⚠️ 本用例**必须**取 `HomeOverrideGuard`（`data_root()` 走 `home_dir()`）；
    /// 本模块只有 3 条用例需要它（见夹具上的说明）。
    #[test]
    fn production_entries_fail_closed_without_a_data_root() {
        let home = std::env::temp_dir().join(format!(
            "buddy-switch-vssess-noroot-{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&home).unwrap();
        let guard = HomeOverrideGuard::set(&home);

        let diff = diff_conversation(ExtClient::Vscode, SRC_UID, DST_UID, WS, CONV);
        assert_eq!(diff["status"], json!("error"), "报告：{diff}");
        assert_eq!(diff["appendable"], json!(false));
        assert!(diff["reason"].as_str().unwrap().contains("数据目录"));

        let sync = sync_conversation(ExtClient::Vscode, SRC_UID, DST_UID, WS, CONV);
        let error = sync.expect_err("数据根缺失时写路径必须 Err（不是「同步了 0 条」）");
        assert!(
            error.contains("未检测到客户端数据目录"),
            "错误文案：{error}"
        );

        drop(guard);
        std::fs::remove_dir_all(home).unwrap();
    }

    /// ★★ S4b：复制会在**生产入口**登记关联，且**双向**可查。
    ///
    /// ⚠️ 本用例**必须**取 `HomeOverrideGuard`：关联账本写在工具存储根
    /// （`store_dir()` = `<home>/.buddy-switch`）下 —— **不覆盖 home 就会污染用户的真实目录**。
    ///
    /// 可证伪：把 `copy_conversations` 里的 `record_links` 调用删掉 ⇒ 两条查询都空 ⇒ 红；
    /// 把 `link_key` 里的客户端段去掉 ⇒ 阴性对照那条红。
    #[test]
    fn copy_records_a_link_that_reads_back_both_ways() {
        let home = std::env::temp_dir().join(format!(
            "buddy-switch-vssess-links-{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&home).unwrap();
        let guard = HomeOverrideGuard::set(&home);

        // 在**被覆盖的 home** 下造出数据根（`data_root()` 取第一个存在的候选）。
        let root = data_root_candidates().into_iter().next().unwrap();
        std::fs::create_dir_all(&root).unwrap();
        seed_conversation(&root, ExtClient::Vscode, SRC_UID, WS, CONV);

        let report =
            copy_conversations(ExtClient::Vscode, SRC_UID, DST_UID, &[item(WS, CONV)]).unwrap();
        assert_eq!(report["copiedCount"], json!(1), "报告：{report}");
        assert_eq!(report["linksRecorded"], json!(true), "关联没落盘：{report}");
        let new_id = report["copied"][0]["newId"].as_str().unwrap().to_string();

        // ① 以**源** id 查 ⇒ 找得到目标那份。
        let from_source = links_for(ExtClient::Vscode, WS, CONV);
        assert_eq!(
            from_source["links"].as_array().unwrap().len(),
            1,
            "报告：{from_source}"
        );
        assert_eq!(
            from_source["links"][0]["targetCid"].as_str().unwrap(),
            new_id
        );
        assert_eq!(
            from_source["links"][0]["sourceUid"].as_str().unwrap(),
            SRC_UID
        );

        // ② 以**目标** id 查 ⇒ **同样**找得到（双向）。
        let from_target = links_for(ExtClient::Vscode, WS, &new_id);
        assert_eq!(
            from_target["links"][0]["sourceCid"].as_str().unwrap(),
            CONV,
            "关联必须**双向**可查（它既可能是源、也可能是目标）"
        );

        // ③ 阴性对照：**另一棵树**（IDE）里同名会话不该被串起来。
        assert_eq!(
            links_for(ExtClient::CodebuddyIde, WS, CONV)["links"]
                .as_array()
                .unwrap()
                .len(),
            0,
            "客户端段必须进关联键 —— 两棵树里的同名会话是两份不同的会话"
        );
        // ④ 阴性对照：**别的工作区**里的同名会话也不该被串起来。
        assert_eq!(
            links_for(ExtClient::Vscode, "ffffffffffffffffffffffffffffffff", CONV)["links"]
                .as_array()
                .unwrap()
                .len(),
            0,
            "工作区段必须进关联键"
        );

        drop(guard);
        std::fs::remove_dir_all(home).unwrap();
    }

    /// ★ 数据根不存在 ⇒ `dataRoot: null`（前端据此区分「没装」与「没有会话」）。
    ///
    /// 可证伪：把 `list_sessions_for` 的 `None` 分支改成回一个空字符串 ⇒ 红。
    #[test]
    fn missing_data_root_is_reported_as_null_not_as_empty() {
        let home = std::env::temp_dir().join(format!(
            "buddy-switch-vssess-missing-{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&home).unwrap();
        let guard = HomeOverrideGuard::set(&home);

        let out = list_sessions_for(ExtClient::Vscode, "uid-a");
        assert_eq!(out["dataRoot"], Value::Null, "找不到数据根必须是 null");
        assert_eq!(out["sessions"], json!([]));
        assert_eq!(out["sourceUid"], json!("uid-a"));

        drop(guard);
        std::fs::remove_dir_all(home).unwrap();
    }
}
