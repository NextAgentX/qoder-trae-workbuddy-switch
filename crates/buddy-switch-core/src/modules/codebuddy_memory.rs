//! CodeBuddy IDE 长期记忆（`~/.codebuddy/memery/{uid}_memery.md`）的合并 —— issue #86。
//!
//! ## 为什么单独成模块，而不改进 `memory.rs`
//!
//! 两份记忆是**两个产品**的：WorkBuddy 桌面端在 `~/.workbuddy/memory/{uid}_memory.md`，
//! CodeBuddy IDE（Genie 扩展）在 `~/.codebuddy/memery/{uid}_memery.md`，文件名、目录名
//! （`memery` 是客户端的拼写）、信封结构都可能不同。塞进同一个文件会让「按 region 分家」
//! 与「按产品分家」两套轴混在一起 —— 那是本仓最常踩的漂移。
//!
//! ## ★ 结构**未经取证**，所以这里只做「认清才写」
//!
//! 2026-11-07 在本机核过：`~/.codebuddy/memery/`、`~/.workbuddy/memery/`、
//! `user-memery-state.json` **都不存在**（IDE 装过但没登录过），因此**没有样本**。
//! 拿不到样本还去写合并，最坏结果是「把目标账号的记忆写成乱码」——比不合并坏得多。
//!
//! 于是这里按**能认清才动**处理，三种形态：
//!
//! | 形态 | 判据 | 处理 |
//! |:--|:--|:--|
//! | 纯 markdown | 不以 `{` 开头 | 逐行去重合并（复用 [`crate::modules::memory::merge_memory_text`]） |
//! | JSON 信封 | 能解析成对象，且有一个已知正文键 | **只替换正文那一个键**，其余键原样保留 |
//! | 认不出 | 其它（JSON 数组 / 对象但没有正文键） | **拒绝写**，并在报错里列出实际键名 |
//!
//! 拒绝时列出键名不是刁难用户，而是把下一次报告变成**一次定死**：看到键名就知道该把
//! 哪个键加进 [`BODY_KEYS`]。
//!
//! ## `user-memery-state.json` 刻意**不碰**
//!
//! 它记录「记忆版本」。语义未知时改写它等于告诉客户端「记忆是脏的」或「已经同步过」，
//! 两种都可能让客户端丢掉这次合并或重复合并。**只合并正文**，让客户端自己决定何时刷新。
//!
//! ## 备份
//!
//! 与 `memory.rs` 同一条：改写目标前留原文到 `{store}/backups/memory/{时间戳}/`；
//! 备份失败不阻断迁移，但在报告里是 `backup: null`。

use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::{Map, Value};

use crate::modules::config;
use crate::modules::memory::{merge_memory_text, MemoryMergeResult};

/// 正文可能落在这些键里（**顺序即优先级**）。
///
/// `memoryBlock` 是 WorkBuddy 侧实测的键名（issue #148）；其余三个是按常见命名补的候选。
/// 命中哪个就只改哪个 —— 其余键一个字节都不动。
const BODY_KEYS: [&str; 4] = ["memoryBlock", "memory", "content", "text"];

/// 记录「这条记忆属于谁」的键（命中且等于源 uid 时才改成目标 uid）。
const UID_KEYS: [&str; 2] = ["uid", "userId"];

/// 记录「最后更新时间」的键（命中才写，不新增）。
const TIME_KEYS: [&str; 2] = ["updatedAt", "updateTime"];

/// 目录名（客户端的拼写就是 `memery`，不要"顺手纠正"成 `memory`）。
const MEMERY_DIR: &str = "memery";

/// 文件名后缀：`{uid}_memery.md`。
const MEMERY_SUFFIX: &str = "_memery.md";

/// 合并报告（前端直接渲染）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemeryReport {
    pub source_uid: String,
    pub target_uid: String,
    /// 源记忆文件所在目录（诊断用：用户可能同时有两代目录）。
    pub source_dir: String,
    /// 目标记忆文件路径（合并后）。
    pub target_path: String,
    /// 识别到的形态：`markdown` / `envelope:<键名>`。
    pub shape: String,
    pub target_lines: usize,
    pub source_lines: usize,
    pub appended: usize,
    pub skipped_duplicate: usize,
    /// 是否真的改写了目标文件。
    pub changed: bool,
    /// 改前原文的备份路径；未改写时为 `null`。
    pub backup: Option<String>,
}

/// 候选目录（顺序即优先级）：IDE 自己的 home 优先，其次历史 `~/.workbuddy`。
///
/// 两代目录都查，是因为 issue #86 提到旧版兼容路径 `~/.workbuddy/memery` —— 用户的记忆
/// 可能躺在旧目录里，只看新目录会得出「没有记忆」的错结论。
pub fn memery_dirs() -> Vec<PathBuf> {
    let home = config::home_dir();
    vec![
        home.join(".codebuddy").join(MEMERY_DIR),
        home.join(".workbuddy").join(MEMERY_DIR),
    ]
}

/// 某 uid 的记忆文件（**第一个存在的**）；都没有 ⇒ `None`。
pub fn memery_file_for(uid: &str) -> Option<PathBuf> {
    let name = format!("{uid}{MEMERY_SUFFIX}");
    memery_dirs()
        .into_iter()
        .map(|dir| dir.join(&name))
        .find(|path| path.is_file())
}

/// uid 能否拼成文件名：拒绝空、`.`/`..`、含路径分隔符与 Windows 保留字符。
///
/// 与 `trae::paths::safe_slot_name` 同一条判据，但**不借道**它 —— 那个函数的语义是
/// 「Trae 快照槽位」，拿它校验 CodeBuddy 的文件名会把两件事绑在一起。
fn safe_uid(uid: &str) -> bool {
    !uid.is_empty()
        && uid != "."
        && uid != ".."
        && !uid
            .chars()
            .any(|c| matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') || c == '\0')
}

/// 记忆正文的形态。
#[derive(Debug, Clone, PartialEq, Eq)]
enum Shape {
    Markdown,
    /// JSON 信封，正文在这个键里。
    Envelope {
        key: String,
    },
}

impl Shape {
    fn label(&self) -> String {
        match self {
            Self::Markdown => "markdown".to_string(),
            Self::Envelope { key } => format!("envelope:{key}"),
        }
    }
}

/// 判断形态。认不出时返回 `Err`，文案里带**实际存在的键名**（下一次报告可一次定死）。
fn detect_shape(text: &str) -> Result<Shape, String> {
    let trimmed = text.trim_start();
    if trimmed.is_empty() {
        return Ok(Shape::Markdown);
    }
    // 以 `{` / `[` 开头就当成 JSON（`[` 是数组：不是我们认识的形态，也要拒绝而不是
    // 当 markdown 追加 —— 那会把 JSON 文本拼进正文）。
    if !trimmed.starts_with('{') {
        if trimmed.starts_with('[') {
            return Err("记忆文件是 JSON 数组；未做合并（避免写坏）".to_string());
        }
        return Ok(Shape::Markdown);
    }
    let Ok(Value::Object(object)) = serde_json::from_str::<Value>(trimmed) else {
        // 以 `{` 开头却不是 JSON 对象：不是我们认识的任何形态，**不猜**。
        return Err("记忆文件看起来是 JSON，但解析不出对象；未做合并（避免写坏）".to_string());
    };
    for key in BODY_KEYS {
        if object.get(key).and_then(Value::as_str).is_some() {
            return Ok(Shape::Envelope {
                key: key.to_string(),
            });
        }
    }
    Err(format!(
        "记忆文件是 JSON，但里面没有已知的正文键（现有键：{}）。请把这份结构贴到 issue #86，\
         加入 BODY_KEYS 后即可合并 —— 先把键名认准，比猜着写安全",
        if object.is_empty() {
            "（空对象）".to_string()
        } else {
            object.keys().cloned().collect::<Vec<_>>().join(", ")
        }
    ))
}

/// 取出正文。
fn body_of(text: &str, shape: &Shape) -> String {
    match shape {
        Shape::Markdown => text.to_string(),
        Shape::Envelope { key } => serde_json::from_str::<Value>(text.trim_start())
            .ok()
            .and_then(|value| value.get(key).and_then(Value::as_str).map(str::to_string))
            .unwrap_or_default(),
    }
}

/// 合并两份文本，返回**目标文件的完整新内容**。
///
/// `target = None` 表示目标文件还不存在（第一次给这个账号合并）。
///
/// 信封形态只在**原本就有** `uid` / 更新时间键时才改它们 —— 不新增未知键（那才是"编"）。
fn merge_texts(
    target: Option<&str>,
    source: &str,
    target_uid: &str,
    label: &str,
) -> Result<(String, Shape, MemoryMergeResult), String> {
    let source_shape = detect_shape(source)?;
    let target_shape = match target {
        Some(text) => detect_shape(text)?,
        None => source_shape.clone(),
    };
    // 目标已存在时以**目标**的形态为准（它是客户端真正会读的那份）。
    let shape = target_shape;
    let source_body = body_of(source, &source_shape);
    let target_body = target.map(|text| body_of(text, &shape)).unwrap_or_default();

    let (merged_body, result) = merge_memory_text(&target_body, &source_body, label);

    let rendered = match &shape {
        Shape::Markdown => merged_body,
        Shape::Envelope { key } => {
            let original = target.unwrap_or(source);
            render_envelope(original, key, &merged_body, target_uid)
        }
    };
    Ok((rendered, shape, result))
}

/// 把正文写回信封：只动正文键 + 已存在的 `uid` / 更新时间键，其余键**逐字保留**。
///
/// 解析失败时退回「原对象 + 替换正文」的做法 —— 但 `detect_shape` 已经保证它是对象，
/// 所以这里的失败只可能来自调用方传错文本，返回原文更安全。
fn render_envelope(original: &str, key: &str, body: &str, target_uid: &str) -> String {
    let Ok(Value::Object(mut object)) = serde_json::from_str::<Value>(original.trim_start()) else {
        return body.to_string();
    };
    object.insert(key.to_string(), Value::String(body.to_string()));
    // 只改「已经存在」的键：新增未知键等于编结构。
    for uid_key in UID_KEYS {
        if object.get(uid_key).is_some() {
            object.insert(uid_key.to_string(), Value::String(target_uid.to_string()));
        }
    }
    for time_key in TIME_KEYS {
        if object.get(time_key).is_some() {
            object.insert(
                time_key.to_string(),
                Value::String(config::utc_iso().to_string()),
            );
        }
    }
    serde_json::to_string_pretty(&Value::Object(object)).unwrap_or_else(|_| body.to_string())
}

/// 备份目标文件改前原文（失败不阻断，返回 `None`）。
fn backup_target(path: &Path, original: &str) -> Option<PathBuf> {
    let file_name = path.file_name()?.to_string_lossy().to_string();
    let dir = config::backup_dir().join("memory").join(config::utc_iso());
    std::fs::create_dir_all(&dir).ok()?;
    let dest = dir.join(file_name);
    config::atomic_write(&dest, original).ok()?;
    Some(dest)
}

/// 把 `source_uid` 的 CodeBuddy IDE 记忆合并进 `target_uid`（**加法**，不覆盖目标已有内容）。
///
/// 语义与 `memory.rs` 的 `merge_memory_files` 对齐：逐行去重、目标为空时整份取源、
/// 改写前备份。差异只在「文件位置与信封键名」。
pub fn merge_memery(source_uid: &str, target_uid: &str) -> Result<MemeryReport, String> {
    let source_uid = source_uid.trim();
    let target_uid = target_uid.trim();
    if !safe_uid(source_uid) {
        return Err(format!("非法的源账号 ID: {source_uid}"));
    }
    if !safe_uid(target_uid) {
        return Err(format!("非法的目标账号 ID: {target_uid}"));
    }
    if source_uid == target_uid {
        return Err("源账号与目标账号相同".to_string());
    }

    let Some(source_path) = memery_file_for(source_uid) else {
        // 没有源记忆**不是错误**（用户可能没让 IDE 存过记忆）—— 但也不能报「成功合并 0 条」，
        // 那会让「没做」和「做了没内容」分不开。
        return Err(format!(
            "源账号没有 CodeBuddy IDE 记忆文件（已查：{}）",
            memery_dirs()
                .iter()
                .map(|dir| dir.display().to_string())
                .collect::<Vec<_>>()
                .join("、")
        ));
    };
    let dir = source_path
        .parent()
        .ok_or_else(|| format!("记忆文件没有父目录：{}", source_path.display()))?
        .to_path_buf();
    let target_path = dir.join(format!("{target_uid}{MEMERY_SUFFIX}"));

    let source_text = std::fs::read_to_string(&source_path)
        .map_err(|e| format!("读取源记忆失败（{}）：{e}", source_path.display()))?;
    let target_text = if target_path.is_file() {
        Some(
            std::fs::read_to_string(&target_path)
                .map_err(|e| format!("读取目标记忆失败（{}）：{e}", target_path.display()))?,
        )
    } else {
        None
    };

    let (rendered, shape, result) = merge_texts(
        target_text.as_deref(),
        &source_text,
        target_uid,
        &shorten(source_uid),
    )?;

    let mut backup = None;
    let changed = result.changed() || target_text.is_none();
    if changed {
        // 目标原本存在才备份（不存在就没有原文可留）。
        if let Some(original) = target_text.as_deref() {
            backup = backup_target(&target_path, original);
        }
        if let Some(parent) = target_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("创建目标目录失败（{}）：{e}", parent.display()))?;
        }
        config::atomic_write(&target_path, &rendered)
            .map_err(|e| format!("写入目标记忆失败（{}）：{e}", target_path.display()))?;
    }

    Ok(MemeryReport {
        source_uid: source_uid.to_string(),
        target_uid: target_uid.to_string(),
        source_dir: dir.display().to_string(),
        target_path: target_path.display().to_string(),
        shape: shape.label(),
        target_lines: result.target_lines,
        source_lines: result.source_lines,
        appended: result.appended,
        skipped_duplicate: result.skipped_duplicate,
        changed,
        backup: backup.map(|path| path.display().to_string()),
    })
}

/// 迁移标注里的 uid 缩写（前 12 字符）。
fn shorten(uid: &str) -> String {
    if uid.chars().count() <= 12 {
        return uid.to_string();
    }
    format!("{}…", uid.chars().take(12).collect::<String>())
}

/// 仅供测试与诊断：把一段 JSON 信封的键名报出来（不写盘）。
pub fn envelope_keys(text: &str) -> Vec<String> {
    serde_json::from_str::<Value>(text.trim_start())
        .ok()
        .and_then(|value| value.as_object().cloned())
        .map(|object: Map<String, Value>| object.keys().cloned().collect())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markdown_shape_is_plain_text() {
        assert_eq!(
            detect_shape("## 记忆\n- 偏好：中文").unwrap(),
            Shape::Markdown
        );
        assert_eq!(detect_shape("").unwrap(), Shape::Markdown);
    }

    #[test]
    fn envelope_body_key_is_detected_in_priority_order() {
        let text = r#"{"uid":"u1","memoryBlock":"正文","updatedAt":"2026-11-01"}"#;
        assert_eq!(
            detect_shape(text).unwrap(),
            Shape::Envelope {
                key: "memoryBlock".into()
            }
        );
        let other = r#"{"content":"正文"}"#;
        assert_eq!(
            detect_shape(other).unwrap(),
            Shape::Envelope {
                key: "content".into()
            }
        );
    }

    /// ★ 认不出时**必须拒绝**，且报错里要带真实键名 —— 那是下一次报告的答案。
    #[test]
    fn unknown_envelope_is_refused_with_its_keys() {
        let err = detect_shape(r#"{"foo":1,"bar":"x"}"#).unwrap_err();
        assert!(err.contains("foo"), "{err}");
        assert!(err.contains("bar"), "{err}");
        assert!(err.contains("issue #86"), "要指向取证入口: {err}");
        // JSON 数组 / 坏 JSON 同样拒绝（不猜）。
        assert!(detect_shape("[1,2]").is_err());
        assert!(detect_shape("{not json").is_err());
    }

    #[test]
    fn merge_markdown_appends_only_new_lines() {
        let target = "## 记忆\n- 偏好：中文";
        let source = "## 记忆\n- 偏好：中文\n- 时区：+08:00";
        let (merged, shape, result) = merge_texts(Some(target), source, "u2", "u1").unwrap();
        assert_eq!(shape, Shape::Markdown);
        assert!(merged.contains("时区：+08:00"), "{merged}");
        assert_eq!(
            merged.matches("偏好：中文").count(),
            1,
            "重复行不得追加两次"
        );
        assert_eq!(result.appended, 1);
        // 源里的标题行与「偏好」那行都已在目标中 ⇒ 跳 2 条（标题也算重复，别把它当头衔）。
        assert_eq!(result.skipped_duplicate, 2);
    }

    /// 信封：只改正文 + 已存在的 uid / 更新时间，**其余键逐字保留**（不新增未知键）。
    #[test]
    fn envelope_merge_preserves_other_keys() {
        let target =
            r#"{"uid":"u2","memoryBlock":"- A","updatedAt":"2026-01-01","extra":{"keep":true}}"#;
        let source = r#"{"uid":"u1","memoryBlock":"- A\n- B"}"#;
        let (merged, shape, _) = merge_texts(Some(target), source, "u2", "u1").unwrap();
        assert_eq!(
            shape,
            Shape::Envelope {
                key: "memoryBlock".into()
            }
        );
        let value: Value = serde_json::from_str(&merged).unwrap();
        assert_eq!(value["uid"], "u2", "uid 必须改成目标账号");
        assert_eq!(value["extra"]["keep"], true, "未知键必须原样保留");
        let body = value["memoryBlock"].as_str().unwrap();
        assert!(body.contains("- A") && body.contains("- B"), "{body}");
        assert_eq!(body.matches("- A").count(), 1, "重复行不得追加两次");
        assert!(value["updatedAt"].is_string());
    }

    /// 目标不存在时按源的形态新建；不含 uid 键的信封**不得**凭空加一个。
    #[test]
    fn missing_target_uses_source_shape_without_inventing_keys() {
        let source = r#"{"memoryBlock":"- A"}"#;
        let (merged, _, result) = merge_texts(None, source, "u2", "u1").unwrap();
        let value: Value = serde_json::from_str(&merged).unwrap();
        assert_eq!(value["memoryBlock"], "- A");
        assert!(value.get("uid").is_none(), "原本没有的键不许加");
        assert_eq!(result.appended, 1);
    }

    #[test]
    fn unsafe_uids_are_rejected() {
        for bad in ["", "..", "a/b", "a\\b", "a:b"] {
            assert!(merge_memery(bad, "u2").is_err(), "{bad} 应被拒绝");
        }
        assert!(merge_memery("u1", "u1").is_err(), "源与目标相同应被拒绝");
    }

    #[test]
    fn missing_source_reports_every_checked_dir() {
        let err = merge_memery("u-does-not-exist", "u2").unwrap_err();
        assert!(err.contains("没有 CodeBuddy IDE 记忆文件"), "{err}");
        assert!(err.contains(".codebuddy"), "要列出查过的目录: {err}");
    }
}
