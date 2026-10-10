//! Skill 管理（cc-switch 式）：**清单 + 启停 + 跨区搬迁 + 本地导入**。
//!
//! 完整背景与验收口径见 `.trellis/tasks/09-22-skill-manager/prd.md`。
//!
//! ## 三处刻意**不照搬** cc-switch（本模块的设计判断）
//!
//! 1. **不建 SQLite 第二真相源**：WorkBuddy 的 `{region}/skills/` 目录**本身就是 SSOT**，
//!    再建库必然漂移。目录即真相源，本模块只在需要时留备份。
//! 2. **默认 copy，不用 symlink**：本工具的搬运是**一次性数据复制**，而 Windows 未开
//!    开发者模式时 `symlink` 必然失败。
//! 3. **跨区搬迁不进 `MigrateScope`**：`migrate.rs` 是 **per-uid** 语义，而 skill 是
//!    **region 级、无 uid 层**的资产 —— 塞进去会让「切号」平白多搬一类数据。
//!
//! ## 启停：行级改写官方原生字段，**绝不重序列化 frontmatter**
//!
//! frontmatter 支持 `user-invocable: false` + `disable-model-invocation: true`（内置 skill
//! 大量在用）。本模块只**行级**改写这两行：任何「解析成结构 → 再序列化」的路径都会重写
//! 整个 frontmatter（缩进 / 引号风格 / 键顺序 / 注释全变），对一个我们只是「路过」的
//! 文件不可接受。因此**不引入 YAML 依赖**，手写只认顶层标量的行级解析。
//!
//! 必须处理的真实输入变体：CRLF / BOM / 无 frontmatter / `---` 后紧跟 EOF /
//! 键名带引号 / 值带 `#` 注释 / 嵌套 `metadata` 块。

use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use crate::modules::config;
use crate::modules::region::Region;
use crate::modules::session::session_data_dir;

/// skills 目录相对 region 数据根的目录名。
const SKILLS_DIR: &str = "skills";

/// skill 清单文件名（一个 skill 目录的**唯一**入口）。
const SKILL_FILE: &str = "SKILL.md";

/// 启用/停用用的两个官方字段（顺序即插入顺序，与内置 skill 的写法一致）。
const FIELD_USER_INVOCABLE: &str = "user-invocable";
const FIELD_DISABLE_MODEL: &str = "disable-model-invocation";

// ---------------------------------------------------------------------------
// 数据模型
// ---------------------------------------------------------------------------

/// 一个 skill 的视图（由 `SKILL.md` frontmatter + 目录推导）。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillEntry {
    /// 目录名（唯一键，启停 / 导入都以它定位）。
    pub dir_name: String,
    /// frontmatter `name`，缺失回落目录名。
    pub name: String,
    /// 优先 `description_zh`（中文界面）；缺失回落 `description`。
    pub description: String,
    /// frontmatter `version`。
    pub version: Option<String>,
    /// 推导值：**非全停用即启用**（见 [`derive_enabled`]）。
    pub enabled: bool,
    /// 两个启停字段只写了一半 ⇒ 状态不一致，界面给「修复」入口。
    pub inconsistent: bool,
    /// frontmatter `agent_created` 为真。
    pub agent_created: bool,
    /// 有无合法 frontmatter；为假时该目录**不可启停**（不擅自造 frontmatter）。
    pub has_frontmatter: bool,
}

/// 单次启停的结果。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillToggleResult {
    pub dir_name: String,
    /// 目标状态（已落盘）。
    pub enabled: bool,
    /// 本次是否真的改写了文件（幂等重跑为 `false`）。
    pub changed: bool,
    /// 改前原文的备份路径；**无改动**或**备份失败**时为 `None`。
    pub backup: Option<PathBuf>,
}

/// 跨 region 搬迁报告（合并补齐语义）。
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillCopyReport {
    pub source_region: String,
    pub target_region: String,
    /// 目标侧缺、已复制过去的 skill。
    pub copied: Vec<String>,
    /// 同名但内容不同 ⇒ **保留目标侧**并计入此列（绝不静默覆盖）。
    pub skipped_conflict: Vec<String>,
    /// 同名且内容一致 ⇒ 幂等跳过。
    pub skipped_identical: Vec<String>,
    /// `copied` 非空。
    pub changed: bool,
    /// 目标侧 skills 目录的整目录快照；仅 `changed` 时产生。
    pub backup: Option<PathBuf>,
}

/// 本地导入结果。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillImportResult {
    pub region: String,
    pub dir_name: String,
    /// 已复制进目标 skills 目录。
    pub imported: bool,
    /// 同名但内容不同 ⇒ 保留目标侧，未导入。
    pub conflict: bool,
    /// 同名且内容一致 ⇒ 幂等跳过。
    pub identical: bool,
}

// ---------------------------------------------------------------------------
// 路径
// ---------------------------------------------------------------------------

/// 该 region 的用户级 skills 目录（`~/.workbuddy/skills` / `~/.workbuddy-ai/skills`）。
///
/// 与 `memory_dir_for` 同构；**只列用户级目录**，程序目录里的内置 skill 不纳管。
pub fn skills_dir_for(region: Region) -> PathBuf {
    session_data_dir(region).join(SKILLS_DIR)
}

/// 解析 region 字符串参数（缺省 `cn`，与两宿主 `parse_region` 同约定）。
fn region_arg(region: Option<String>) -> Region {
    region.as_deref().and_then(Region::parse).unwrap_or(Region::Cn)
}

// ---------------------------------------------------------------------------
// frontmatter：行级解析（无 YAML 依赖）
// ---------------------------------------------------------------------------

/// 解析出的 frontmatter 视图。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SkillFrontmatter {
    /// 顶层标量键 → 值（已去引号、已去行内注释）。
    pub scalars: BTreeMap<String, String>,
    /// 存在顶层 `metadata:` 嵌套块（只记存在性，**不**把块内缩进行当标量）。
    pub has_metadata: bool,
    /// 闭分隔符 `---` 所在行号（0 基）。
    pub end_line: usize,
    /// 文件主行尾符（用于插入新行）。
    pub eol: String,
}

/// 按行切分，**保留每行的行尾符**（`\n` / `\r\n` / 末尾无行尾）。
///
/// 用「内容 + 行尾」二元组而不是 `str::lines()`：后者会吞掉 `\r`，于是行级改写
/// 会把 CRLF 文件整体改成 LF —— 那正是「保留其余字节」要防的事。
fn split_lines_keep_eol(text: &str) -> Vec<(String, String)> {
    let bytes = text.as_bytes();
    let mut out: Vec<(String, String)> = Vec::new();
    let mut start = 0usize;
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'\n' {
            let (content_end, eol) = if i > start && bytes[i - 1] == b'\r' {
                (i - 1, "\r\n")
            } else {
                (i, "\n")
            };
            out.push((text[start..content_end].to_string(), eol.to_string()));
            start = i + 1;
        }
        i += 1;
    }
    if start < text.len() {
        out.push((text[start..].to_string(), String::new()));
    }
    out
}

/// 去掉可能存在的 UTF-8 BOM（只用于**判定**，不改动原字节）。
fn strip_bom(line: &str) -> &str {
    line.strip_prefix('\u{feff}').unwrap_or(line)
}

/// 定位 frontmatter：首行（去 BOM 后）必须是 `---`，且**必须**有闭合 `---`。
///
/// `---` 后紧跟 EOF（或无闭合行）一律判为**无 frontmatter** —— 那是非法文件，
/// 我们不猜它的边界，也就不会去改它。
fn detect_frontmatter(lines: &[(String, String)]) -> Option<usize> {
    let first = strip_bom(&lines.first()?.0);
    if first.trim() != "---" {
        return None;
    }
    (1..lines.len()).find(|&i| lines[i].0.trim() == "---")
}

/// 拆出「键 + 冒号后原文」；无冒号 / 空键返回 `None`。
fn split_key_value(line: &str) -> Option<(String, &str)> {
    let colon = line.find(':')?;
    let key_raw = line[..colon].trim();
    if key_raw.is_empty() {
        return None;
    }
    let key = unquote(key_raw);
    if key.is_empty() {
        return None;
    }
    Some((key, &line[colon + 1..]))
}

/// 去掉成对的引号（单/双）。
fn unquote(raw: &str) -> String {
    let t = raw.trim();
    let bytes = t.as_bytes();
    if bytes.len() >= 2 {
        let first = bytes[0];
        let last = bytes[bytes.len() - 1];
        if (first == b'"' && last == b'"') || (first == b'\'' && last == b'\'') {
            return t[1..t.len() - 1].to_string();
        }
    }
    t.to_string()
}

/// 解析一个顶层标量值：去引号；未加引号时按 YAML 规则剥行内注释（`#` 前须是空白或行首）。
fn parse_scalar_value(raw: &str) -> String {
    let t = raw.trim();
    let bytes = t.as_bytes();
    if bytes.len() >= 2 {
        let first = bytes[0];
        let last = bytes[bytes.len() - 1];
        if (first == b'"' && last == b'"') || (first == b'\'' && last == b'\'') {
            return t[1..t.len() - 1].to_string();
        }
    }
    let mut end = t.len();
    for (i, &c) in bytes.iter().enumerate() {
        if c == b'#' && (i == 0 || bytes[i - 1] == b' ' || bytes[i - 1] == b'\t') {
            end = i;
            break;
        }
    }
    t[..end].trim().to_string()
}

/// 解析 `SKILL.md` 的 frontmatter（纯函数，无副作用）。
///
/// 只认 `^key: value$` 形式的**顶层**标量：缩进行（嵌套块内容）一律跳过；
/// `metadata:` 这类「键 + 空值」行只记**存在性**。
pub fn parse_skill_frontmatter(text: &str) -> Option<SkillFrontmatter> {
    let lines = split_lines_keep_eol(text);
    let end_line = detect_frontmatter(&lines)?;
    let eol = if lines.iter().any(|(_, e)| e == "\r\n") {
        "\r\n"
    } else {
        "\n"
    }
    .to_string();

    let mut fm = SkillFrontmatter {
        end_line,
        eol,
        ..Default::default()
    };
    for (content, _) in lines.iter().take(end_line).skip(1) {
        // 缩进行 = 嵌套块内容，不是顶层标量。
        if content.starts_with(' ') || content.starts_with('\t') {
            continue;
        }
        let trimmed = content.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let Some((key, raw)) = split_key_value(content) else {
            continue;
        };
        if raw.trim().is_empty() {
            if key == "metadata" {
                fm.has_metadata = true;
            }
            continue;
        }
        fm.scalars.insert(key, parse_scalar_value(raw));
    }
    Some(fm)
}

/// 解析 YAML 布尔（只认 `true` / `false`，大小写不敏感）；其余返回 `None`。
fn parse_bool(value: &str) -> Option<bool> {
    let v = value.trim();
    if v.eq_ignore_ascii_case("true") {
        Some(true)
    } else if v.eq_ignore_ascii_case("false") {
        Some(false)
    } else {
        None
    }
}

/// 由两个字段推导「是否启用 / 是否状态不一致」。
///
/// | 状态 | frontmatter |
/// |:--|:--|
/// | 启用 | 两字段都不存在（或显式 `true` / `false`） |
/// | 停用 | `user-invocable: false` **且** `disable-model-invocation: true` |
/// | 不一致 | 只写了其中一个 ⇒ 按「非全停用即启用」推导 |
fn derive_enabled(scalars: &BTreeMap<String, String>) -> (bool, bool) {
    let ui = scalars.get(FIELD_USER_INVOCABLE).and_then(|v| parse_bool(v));
    let dmi = scalars
        .get(FIELD_DISABLE_MODEL)
        .and_then(|v| parse_bool(v));
    let disabled = ui == Some(false) && dmi == Some(true);
    let inconsistent = ui.is_some() != dmi.is_some();
    (!disabled, inconsistent)
}

// ---------------------------------------------------------------------------
// 启停：行级改写
// ---------------------------------------------------------------------------

/// 行级改写两个启停字段，返回（新文本, 是否确有改动）。
///
/// - 字段**已存在** ⇒ 就地替换其值（保留键名写法，含引号）；
/// - 字段**不存在** ⇒ 插在 frontmatter **末尾**（闭分隔符之前），不紧贴开头、不动既有行；
/// - **无 frontmatter** ⇒ 报错（不擅自造 frontmatter，避免把非法 skill 变成看似合法的）。
///
/// 其余字节**逐字节不变**（含 BOM、CRLF、缩进、注释、键顺序）。
pub fn rewrite_skill_toggle(text: &str, enabled: bool) -> Result<(String, bool), String> {
    let mut lines = split_lines_keep_eol(text);
    let end_line = detect_frontmatter(&lines).ok_or_else(|| {
        format!("该 skill 的 {SKILL_FILE} 缺少合法的 frontmatter（`---` 块），无法启停")
    })?;
    let eol = if lines.iter().any(|(_, e)| e == "\r\n") {
        "\r\n"
    } else {
        "\n"
    };

    let targets: [(&str, &str); 2] = if enabled {
        [(FIELD_USER_INVOCABLE, "true"), (FIELD_DISABLE_MODEL, "false")]
    } else {
        [(FIELD_USER_INVOCABLE, "false"), (FIELD_DISABLE_MODEL, "true")]
    };

    // 定位已有行（只看 frontmatter 范围内的顶层行）。
    let mut found: [Option<usize>; 2] = [None, None];
    for (idx, (content, _)) in lines.iter().enumerate().take(end_line).skip(1) {
        if content.starts_with(' ') || content.starts_with('\t') {
            continue;
        }
        if let Some((key, _)) = split_key_value(content) {
            for (ti, (target_key, _)) in targets.iter().enumerate() {
                if found[ti].is_none() && key == *target_key {
                    found[ti] = Some(idx);
                }
            }
        }
    }

    // ① 已存在 ⇒ 只换值。
    for (ti, (_, value)) in targets.iter().enumerate() {
        if let Some(idx) = found[ti] {
            let content = lines[idx].0.clone();
            let colon = content.find(':').unwrap_or(content.len());
            let key_part = &content[..=colon];
            lines[idx].0 = format!("{key_part} {value}");
        }
    }

    // ② 不存在 ⇒ 插在 frontmatter 末尾（闭分隔符之前）。
    let mut insert_at = end_line;
    for (ti, (key, value)) in targets.iter().enumerate() {
        if found[ti].is_none() {
            lines.insert(insert_at, (format!("{key}: {value}"), eol.to_string()));
            insert_at += 1;
        }
    }

    let rebuilt: String = lines.iter().map(|(c, e)| format!("{c}{e}")).collect();
    let changed = rebuilt != text;
    Ok((rebuilt, changed))
}

// ---------------------------------------------------------------------------
// 备份（模块内部，失败不阻断）
// ---------------------------------------------------------------------------

/// 备份单个 skill 的 `SKILL.md` 改前原文到 `{store}/backups/skills/{时间戳}/{目录名}/`。
///
/// 备份失败**不阻断**写入（返回 `None`）：与 `memory.rs::backup_target_memory` 同一规则 ——
/// 迁移的价值高于备份留存，失败会在报告里体现为 `backup: null`。
fn backup_skill_file(dir_name: &str, original: &str) -> Option<PathBuf> {
    let dir = config::backup_dir()
        .join("skills")
        .join(config::utc_iso())
        .join(dir_name);
    std::fs::create_dir_all(&dir).ok()?;
    let dest = dir.join(SKILL_FILE);
    config::atomic_write(&dest, original).ok()?;
    Some(dest)
}

/// 对**目标侧 skills 目录整体**做一次快照（仅跨区搬迁有实际复制时调用）。
fn backup_skills_dir(target: Region) -> Option<PathBuf> {
    let src = skills_dir_for(target);
    if !src.is_dir() {
        return None;
    }
    let dest = config::backup_dir()
        .join("skills")
        .join(config::utc_iso())
        .join(format!("copy-{}", target.as_str()));
    copy_dir_recursive(&src, &dest).ok()?;
    Some(dest)
}

// ---------------------------------------------------------------------------
// 目录工具
// ---------------------------------------------------------------------------

/// 递归复制目录（copy，非 symlink）。
fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir_recursive(&from, &to)?;
        } else {
            std::fs::copy(&from, &to)?;
        }
    }
    Ok(())
}

/// 一个文件的轻量指纹：相对路径 + 大小 + 首 4KB 哈希。
#[derive(Debug, Clone, PartialEq, Eq)]
struct FileSig {
    rel: String,
    size: u64,
    head: u64,
}

/// 目录内容指纹（文件清单 + 每个文件的大小 + 首 4KB 哈希），按相对路径排序。
///
/// **刻意不做全量逐字节哈希**：60 个目录逐个全量读开销大，且可能撞上 `.git` 之类的大目录。
/// 判据写成独立纯函数（`dir_signature`）以便单测；任一项不同即判为冲突。
fn dir_signature(root: &Path) -> Option<Vec<FileSig>> {
    let mut out = Vec::new();
    collect_sigs(root, root, &mut out)?;
    out.sort_by(|a, b| a.rel.cmp(&b.rel));
    Some(out)
}

fn collect_sigs(root: &Path, dir: &Path, out: &mut Vec<FileSig>) -> Option<()> {
    let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .collect();
    entries.sort();
    for path in entries {
        let meta = std::fs::symlink_metadata(&path).ok()?;
        if meta.is_dir() {
            collect_sigs(root, &path, out)?;
        } else if meta.is_file() {
            let rel = path
                .strip_prefix(root)
                .ok()?
                .to_string_lossy()
                .replace('\\', "/");
            out.push(FileSig {
                rel,
                size: meta.len(),
                head: head_hash(&path),
            });
        }
    }
    Some(())
}

/// 首 4KB 的 FNV-1a 64 位哈希（读不到时返回 0）。
fn head_hash(path: &Path) -> u64 {
    let mut file = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(_) => return 0,
    };
    let mut buf = [0u8; 4096];
    let n = file.read(&mut buf).unwrap_or(0);
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in &buf[..n] {
        hash ^= b as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// 两个目录内容是否一致（任一侧读不到即判**不一致**，宁可报冲突也不覆盖）。
fn dirs_identical(a: &Path, b: &Path) -> bool {
    match (dir_signature(a), dir_signature(b)) {
        (Some(x), Some(y)) => x == y,
        _ => false,
    }
}

/// 目录名白名单：拒绝空、`.`/`..`、含路径分隔符或盘符的输入（防目录穿越）。
fn sanitize_dir_name(raw: &str) -> Result<String, String> {
    let name = raw.trim();
    if name.is_empty() || name == "." || name == ".." {
        return Err("skill 目录名不合法（空或相对路径）".to_string());
    }
    if name.contains('/') || name.contains('\\') || name.contains(':') {
        return Err(format!("skill 目录名不合法：{name}"));
    }
    Ok(name.to_string())
}

/// 该目录是否是一个可纳管的 skill（含 `SKILL.md`）。
fn is_skill_dir(dir: &Path) -> bool {
    dir.is_dir() && dir.join(SKILL_FILE).is_file()
}

// ---------------------------------------------------------------------------
// L1：清单
// ---------------------------------------------------------------------------

/// 读一个 skill 目录 → 条目；缺 `SKILL.md` 返回 `None`。
fn read_skill_entry(dir: &Path) -> Option<SkillEntry> {
    let dir_name = dir.file_name()?.to_str()?.to_string();
    let text = std::fs::read_to_string(dir.join(SKILL_FILE)).ok()?;
    let fm = parse_skill_frontmatter(&text);
    let empty = BTreeMap::new();
    let scalars = fm.as_ref().map(|f| &f.scalars).unwrap_or(&empty);
    let (enabled, inconsistent) = derive_enabled(scalars);

    let name = scalars
        .get("name")
        .filter(|v| !v.is_empty())
        .cloned()
        .unwrap_or_else(|| dir_name.clone());
    let description = scalars
        .get("description_zh")
        .or_else(|| scalars.get("description"))
        .cloned()
        .unwrap_or_default();
    let version = scalars.get("version").filter(|v| !v.is_empty()).cloned();
    let agent_created = scalars
        .get("agent_created")
        .and_then(|v| parse_bool(v))
        .unwrap_or(false);

    Some(SkillEntry {
        dir_name,
        name,
        description,
        version,
        enabled,
        inconsistent,
        agent_created,
        has_frontmatter: fm.is_some(),
    })
}

/// 列出该 region 用户级 skills 目录下的全部 skill（按目录名排序）。
///
/// 目录不存在 ⇒ 空清单（不是错误）；无 `SKILL.md` 的目录不是 skill，跳过。
pub fn list_skills_for(region: Region) -> Result<Vec<SkillEntry>, String> {
    let dir = skills_dir_for(region);
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut entries: Vec<SkillEntry> = std::fs::read_dir(&dir)
        .map_err(|e| format!("读取 skills 目录失败（{}）：{e}", dir.display()))?
        .flatten()
        .filter_map(|e| read_skill_entry(&e.path()))
        .collect();
    entries.sort_by(|a, b| a.dir_name.cmp(&b.dir_name));
    Ok(entries)
}

// ---------------------------------------------------------------------------
// L1：启停
// ---------------------------------------------------------------------------

/// 设置一个 skill 的启用状态（行级改写 + 原子写 + 模块内备份）。
pub fn set_skill_enabled_for(
    region: Region,
    dir_name: &str,
    enabled: bool,
) -> Result<SkillToggleResult, String> {
    let name = sanitize_dir_name(dir_name)?;
    let path = skills_dir_for(region).join(&name).join(SKILL_FILE);
    if !path.is_file() {
        return Err(format!("skill 不存在或缺少 {SKILL_FILE}：{name}"));
    }
    let original = std::fs::read_to_string(&path)
        .map_err(|e| format!("读取 {SKILL_FILE} 失败（{}）：{e}", path.display()))?;
    let (rewritten, changed) = rewrite_skill_toggle(&original, enabled)?;

    let mut backup = None;
    if changed {
        // 备份**仅**在「目标原本存在」且「本次确有改动」时发生（与 memory.rs 同一规则）。
        backup = backup_skill_file(&name, &original);
        config::atomic_write(&path, &rewritten)
            .map_err(|e| format!("写入 {SKILL_FILE} 失败（{}）：{e}", path.display()))?;
    }

    Ok(SkillToggleResult {
        dir_name: name,
        enabled,
        changed,
        backup,
    })
}

// ---------------------------------------------------------------------------
// L2：跨 region 搬迁（合并补齐）
// ---------------------------------------------------------------------------

/// 把 `source` 区域的 skill **合并补齐**到 `target` 区域。
///
/// | 目标侧情况 | 行为 |
/// |:--|:--|
/// | 无同名目录 | 复制（copy，非 symlink） |
/// | 同名且内容一致 | 跳过，计入 `skippedIdentical` |
/// | 同名但内容不同 | **保留目标侧**，计入 `skippedConflict`（绝不静默覆盖） |
///
/// 因此天然**幂等**：重复执行不产生新内容。
pub fn copy_skills_cross_for(source: Region, target: Region) -> Result<SkillCopyReport, String> {
    if source == target {
        return Err("源区域与目标区域相同".to_string());
    }
    let src_dir = skills_dir_for(source);
    let dst_dir = skills_dir_for(target);
    let mut report = SkillCopyReport {
        source_region: source.as_str().to_string(),
        target_region: target.as_str().to_string(),
        ..Default::default()
    };

    if src_dir.is_dir() {
        let mut names: Vec<String> = std::fs::read_dir(&src_dir)
            .map_err(|e| format!("读取源 skills 目录失败（{}）：{e}", src_dir.display()))?
            .flatten()
            .filter(|e| is_skill_dir(&e.path()))
            .filter_map(|e| e.file_name().to_str().map(str::to_string))
            .collect();
        names.sort();

        for name in names {
            let from = src_dir.join(&name);
            let to = dst_dir.join(&name);
            if to.is_dir() {
                if dirs_identical(&from, &to) {
                    report.skipped_identical.push(name);
                } else {
                    report.skipped_conflict.push(name);
                }
            } else if to.exists() {
                // 同名但不是目录 ⇒ 同样按冲突处理，保留目标侧。
                report.skipped_conflict.push(name);
            } else {
                std::fs::create_dir_all(&dst_dir).map_err(|e| {
                    format!("创建目标 skills 目录失败（{}）：{e}", dst_dir.display())
                })?;
                copy_dir_recursive(&from, &to)
                    .map_err(|e| format!("复制 skill 失败（{name}）：{e}"))?;
                report.copied.push(name);
            }
        }
    }

    report.changed = !report.copied.is_empty();
    if report.changed {
        report.backup = backup_skills_dir(target);
    }
    Ok(report)
}

// ---------------------------------------------------------------------------
// L3a：本地导入（目录 / zip）
// ---------------------------------------------------------------------------

/// 定位 skill 根目录：自身含 `SKILL.md`，或**唯一**顶层子目录含之。
///
/// 返回 `(根目录, 是否就是传入的根)` —— 后者决定目录名取「传入路径的名字」还是「子目录名」。
fn resolve_skill_source_dir(root: &Path) -> Result<(PathBuf, bool), String> {
    if root.join(SKILL_FILE).is_file() {
        return Ok((root.to_path_buf(), true));
    }
    let mut candidates: Vec<PathBuf> = std::fs::read_dir(root)
        .map_err(|e| format!("读取源目录失败（{}）：{e}", root.display()))?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir() && p.join(SKILL_FILE).is_file())
        .collect();
    candidates.sort();
    match candidates.len() {
        0 => Err(format!("源目录中找不到 {SKILL_FILE}")),
        1 => Ok((candidates.remove(0), false)),
        n => Err(format!(
            "源目录里有 {n} 个 skill，无法确定导入哪一个（请只导入单个 skill）"
        )),
    }
}

/// 把 skill 目录装进目标 region（同名按合并补齐的冲突规则处理）。
fn install_skill_dir(
    region: Region,
    skill_dir: &Path,
    name: &str,
) -> Result<SkillImportResult, String> {
    let dst_dir = skills_dir_for(region);
    let target = dst_dir.join(name);
    let mut result = SkillImportResult {
        region: region.as_str().to_string(),
        dir_name: name.to_string(),
        imported: false,
        conflict: false,
        identical: false,
    };
    if target.is_dir() {
        if dirs_identical(skill_dir, &target) {
            result.identical = true;
        } else {
            result.conflict = true;
        }
    } else if target.exists() {
        result.conflict = true;
    } else {
        std::fs::create_dir_all(&dst_dir)
            .map_err(|e| format!("创建 skills 目录失败（{}）：{e}", dst_dir.display()))?;
        copy_dir_recursive(skill_dir, &target)
            .map_err(|e| format!("复制 skill 失败（{name}）：{e}"))?;
        result.imported = true;
    }
    Ok(result)
}

/// 从**本地目录或 zip** 导入一个 skill。
///
/// 校验源（或 zip 解压后的唯一顶层目录）顶层含 `SKILL.md`；同名冲突**保留目标侧**。
pub fn import_skill_into(region: Region, source: &Path) -> Result<SkillImportResult, String> {
    if !source.exists() {
        return Err(format!("源路径不存在：{}", source.display()));
    }

    let mut temp: Option<PathBuf> = None;
    let resolved = if source.is_dir() {
        resolve_skill_source_dir(source)
    } else if source.is_file() {
        let dir = std::env::temp_dir().join(format!(
            "buddy-switch-skill-zip-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let extracted = extract_zip_to(source, &dir).and_then(|_| {
            temp = Some(dir.clone());
            resolve_skill_source_dir(&dir)
        });
        if extracted.is_err() {
            let _ = std::fs::remove_dir_all(&dir);
        }
        extracted
    } else {
        return Err(format!("源路径不是目录也不是 zip 文件：{}", source.display()));
    };
    let (skill_dir, is_root) = resolved?;

    let fallback = source
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_string();
    let raw_name = if is_root {
        fallback
    } else {
        skill_dir
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string()
    };
    let name = sanitize_dir_name(&raw_name)?;

    let outcome = install_skill_dir(region, &skill_dir, &name);
    if let Some(dir) = temp {
        let _ = std::fs::remove_dir_all(&dir);
    }
    outcome
}

// ---------------------------------------------------------------------------
// zip：极简读取（不引第三方依赖）
// ---------------------------------------------------------------------------

/// 一个 zip 中央目录条目。
struct ZipEntry {
    name: String,
    method: u16,
    comp_size: usize,
    local_offset: usize,
}

/// zip 中央目录 / 本地头签名（小端字节序）。
const ZIP_EOCD: [u8; 4] = [0x50, 0x4b, 0x05, 0x06];
const ZIP_CENTRAL: [u8; 4] = [0x50, 0x4b, 0x01, 0x02];
const ZIP_LOCAL: [u8; 4] = [0x50, 0x4b, 0x03, 0x04];

fn u16_at(data: &[u8], at: usize) -> Option<u16> {
    let bytes = data.get(at..at + 2)?;
    Some(u16::from_le_bytes([bytes[0], bytes[1]]))
}

fn u32_at(data: &[u8], at: usize) -> Option<u32> {
    let bytes = data.get(at..at + 4)?;
    Some(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

/// 解析 zip 的中央目录（不支持 ZIP64）。
fn read_zip_entries(data: &[u8]) -> Result<Vec<ZipEntry>, String> {
    if data.len() < 22 {
        return Err("zip 文件过小，不是有效的 zip".to_string());
    }
    // EOCD 在最末尾（可能带注释），从后往前找签名。
    let search_from = data.len().saturating_sub(22 + 0xFFFF);
    let eocd = (search_from..=data.len() - 22)
        .rev()
        .find(|&i| data[i..i + 4] == ZIP_EOCD)
        .ok_or_else(|| "zip 中找不到中央目录（不是有效 zip）".to_string())?;
    let count = u16_at(data, eocd + 10).ok_or("zip EOCD 截断")? as usize;
    let cd_offset = u32_at(data, eocd + 16).ok_or("zip EOCD 截断")? as usize;

    let mut entries = Vec::new();
    let mut p = cd_offset;
    for _ in 0..count {
        if data.get(p..p + 4) != Some(&ZIP_CENTRAL[..]) {
            return Err("zip 中央目录条目损坏".to_string());
        }
        let method = u16_at(data, p + 10).ok_or("zip 条目截断")?;
        let comp_size = u32_at(data, p + 20).ok_or("zip 条目截断")? as usize;
        let name_len = u16_at(data, p + 28).ok_or("zip 条目截断")? as usize;
        let extra_len = u16_at(data, p + 30).ok_or("zip 条目截断")? as usize;
        let comment_len = u16_at(data, p + 32).ok_or("zip 条目截断")? as usize;
        let local_offset = u32_at(data, p + 42).ok_or("zip 条目截断")? as usize;
        let name_bytes = data
            .get(p + 46..p + 46 + name_len)
            .ok_or("zip 条目文件名截断")?;
        entries.push(ZipEntry {
            name: String::from_utf8_lossy(name_bytes).to_string(),
            method,
            comp_size,
            local_offset,
        });
        p += 46 + name_len + extra_len + comment_len;
    }
    Ok(entries)
}

/// 解出单个 zip 条目的内容（`0` = 存储，`8` = deflate）。
fn extract_zip_entry(data: &[u8], entry: &ZipEntry) -> Result<Vec<u8>, String> {
    let p = entry.local_offset;
    if data.get(p..p + 4) != Some(&ZIP_LOCAL[..]) {
        return Err(format!("zip 本地头损坏：{}", entry.name));
    }
    let name_len = u16_at(data, p + 26).ok_or("zip 本地头截断")? as usize;
    let extra_len = u16_at(data, p + 28).ok_or("zip 本地头截断")? as usize;
    let start = p + 30 + name_len + extra_len;
    let raw = data
        .get(start..start + entry.comp_size)
        .ok_or_else(|| format!("zip 数据截断：{}", entry.name))?;
    match entry.method {
        0 => Ok(raw.to_vec()),
        8 => inflate_raw(raw),
        m => Err(format!("zip 使用了不支持的压缩方式（{m}）：{}", entry.name)),
    }
}

/// 把 zip 解压到 `dest`（目录条目跳过；**拒绝**任何绝对路径 / `..` 条目）。
fn extract_zip_to(zip_path: &Path, dest: &Path) -> Result<(), String> {
    let data =
        std::fs::read(zip_path).map_err(|e| format!("读取 zip 失败（{}）：{e}", zip_path.display()))?;
    let entries = read_zip_entries(&data)?;
    std::fs::create_dir_all(dest).map_err(|e| format!("创建解压目录失败：{e}"))?;
    for entry in entries {
        if entry.name.ends_with('/') {
            continue;
        }
        let rel = safe_relative_path(&entry.name)
            .ok_or_else(|| format!("zip 内含不安全路径，已拒绝：{}", entry.name))?;
        let out = dest.join(&rel);
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("创建解压子目录失败：{e}"))?;
        }
        let bytes = extract_zip_entry(&data, &entry)?;
        std::fs::write(&out, &bytes)
            .map_err(|e| format!("写出解压文件失败（{}）：{e}", out.display()))?;
    }
    Ok(())
}

/// zip 条目名 → 安全相对路径；含 `..` / 盘符 / 绝对路径返回 `None`。
fn safe_relative_path(name: &str) -> Option<PathBuf> {
    let normalized = name.replace('\\', "/");
    let mut out = PathBuf::new();
    for part in normalized.split('/') {
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." || part.contains(':') {
            return None;
        }
        out.push(part);
    }
    if out.as_os_str().is_empty() {
        None
    } else {
        Some(out)
    }
}

// ---------------------------------------------------------------------------
// raw DEFLATE（RFC 1951）：手写解压，**不引第三方依赖**
// ---------------------------------------------------------------------------

/// 位读取器（LSB 优先，与 DEFLATE 一致）。
struct BitReader<'a> {
    data: &'a [u8],
    byte: usize,
    bitbuf: u32,
    bitcnt: u32,
}

impl<'a> BitReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self {
            data,
            byte: 0,
            bitbuf: 0,
            bitcnt: 0,
        }
    }

    fn fill(&mut self, n: u32) -> Result<(), String> {
        while self.bitcnt < n {
            let b = *self.data.get(self.byte).ok_or("deflate 数据不足")? as u32;
            self.byte += 1;
            self.bitbuf |= b << self.bitcnt;
            self.bitcnt += 8;
        }
        Ok(())
    }

    fn bits(&mut self, n: u32) -> Result<u32, String> {
        if n == 0 {
            return Ok(0);
        }
        self.fill(n)?;
        let value = self.bitbuf & ((1u32 << n) - 1);
        self.bitbuf >>= n;
        self.bitcnt -= n;
        Ok(value)
    }

    /// 丢弃当前字节内的剩余位（存储块用）。
    fn align_to_byte(&mut self) {
        let drop = self.bitcnt % 8;
        self.bitbuf >>= drop;
        self.bitcnt -= drop;
    }
}

/// 规范 Huffman 表（`puff.c` 同款 counts/symbols 表示）。
struct Huffman {
    counts: [u16; 16],
    symbols: Vec<u16>,
}

impl Huffman {
    fn new(lengths: &[u8]) -> Self {
        let mut counts = [0u16; 16];
        for &l in lengths {
            counts[l as usize] += 1;
        }
        counts[0] = 0;
        let mut offsets = [0u16; 16];
        for i in 1..16 {
            offsets[i] = offsets[i - 1] + counts[i - 1];
        }
        let mut symbols = vec![0u16; lengths.len()];
        for (sym, &l) in lengths.iter().enumerate() {
            if l != 0 {
                symbols[offsets[l as usize] as usize] = sym as u16;
                offsets[l as usize] += 1;
            }
        }
        Self { counts, symbols }
    }

    fn decode(&self, br: &mut BitReader) -> Result<u16, String> {
        let mut code: i32 = 0;
        let mut first: i32 = 0;
        let mut index: i32 = 0;
        for len in 1..16 {
            code |= br.bits(1)? as i32;
            let count = self.counts[len] as i32;
            if code - count < first {
                let at = index + (code - first);
                return self
                    .symbols
                    .get(at as usize)
                    .copied()
                    .ok_or_else(|| "deflate 符号越界".to_string());
            }
            index += count;
            first = (first + count) << 1;
            code <<= 1;
        }
        Err("deflate 非法 Huffman 码".to_string())
    }
}

const LEN_BASE: [u16; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131,
    163, 195, 227, 258,
];
const LEN_EXTRA: [u8; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];
const DIST_BASE: [u16; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
    2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];
const DIST_EXTRA: [u8; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13,
    13,
];

/// 固定 Huffman 表（BTYPE=01）。
fn fixed_tables() -> (Huffman, Huffman) {
    let mut lit = [0u8; 288];
    for (i, l) in lit.iter_mut().enumerate() {
        *l = match i {
            0..=143 => 8,
            144..=255 => 9,
            256..=279 => 7,
            _ => 8,
        };
    }
    let dist = [5u8; 32];
    (Huffman::new(&lit), Huffman::new(&dist))
}

/// 读动态 Huffman 表（BTYPE=10）。
fn read_dynamic_tables(br: &mut BitReader) -> Result<(Huffman, Huffman), String> {
    const ORDER: [usize; 19] = [
        16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
    ];
    let hlit = br.bits(5)? as usize + 257;
    let hdist = br.bits(5)? as usize + 1;
    let hclen = br.bits(4)? as usize + 4;
    let mut code_lengths = [0u8; 19];
    for &slot in ORDER.iter().take(hclen) {
        code_lengths[slot] = br.bits(3)? as u8;
    }
    let code_table = Huffman::new(&code_lengths);

    let total = hlit + hdist;
    let mut lengths = vec![0u8; total];
    let mut i = 0usize;
    while i < total {
        let sym = code_table.decode(br)?;
        match sym {
            0..=15 => {
                lengths[i] = sym as u8;
                i += 1;
            }
            16 => {
                if i == 0 {
                    return Err("deflate 码长重复码出现在开头".to_string());
                }
                let prev = lengths[i - 1];
                let repeat = 3 + br.bits(2)? as usize;
                for _ in 0..repeat {
                    if i >= total {
                        return Err("deflate 码长表越界".to_string());
                    }
                    lengths[i] = prev;
                    i += 1;
                }
            }
            17 | 18 => {
                let repeat = if sym == 17 {
                    3 + br.bits(3)? as usize
                } else {
                    11 + br.bits(7)? as usize
                };
                for _ in 0..repeat {
                    if i >= total {
                        return Err("deflate 码长表越界".to_string());
                    }
                    lengths[i] = 0;
                    i += 1;
                }
            }
            _ => return Err("deflate 非法码长符号".to_string()),
        }
    }
    Ok((
        Huffman::new(&lengths[..hlit]),
        Huffman::new(&lengths[hlit..]),
    ))
}

fn inflate_block(
    br: &mut BitReader,
    out: &mut Vec<u8>,
    lit: &Huffman,
    dist: &Huffman,
) -> Result<(), String> {
    loop {
        let sym = lit.decode(br)?;
        match sym {
            0..=255 => out.push(sym as u8),
            256 => return Ok(()),
            257..=285 => {
                let idx = (sym - 257) as usize;
                let len = LEN_BASE[idx] as usize + br.bits(LEN_EXTRA[idx] as u32)? as usize;
                let dsym = dist.decode(br)? as usize;
                if dsym >= DIST_BASE.len() {
                    return Err("deflate 非法距离码".to_string());
                }
                let distance =
                    DIST_BASE[dsym] as usize + br.bits(DIST_EXTRA[dsym] as u32)? as usize;
                if distance > out.len() {
                    return Err("deflate 距离越界".to_string());
                }
                let start = out.len() - distance;
                for k in 0..len {
                    let byte = out[start + k];
                    out.push(byte);
                }
            }
            _ => return Err("deflate 非法长度码".to_string()),
        }
    }
}

/// 解压 raw DEFLATE（RFC 1951）流。
fn inflate_raw(data: &[u8]) -> Result<Vec<u8>, String> {
    let mut br = BitReader::new(data);
    let mut out: Vec<u8> = Vec::new();
    loop {
        let last = br.bits(1)?;
        let btype = br.bits(2)?;
        match btype {
            0 => {
                br.align_to_byte();
                let len = br.bits(16)? as usize;
                let nlen = br.bits(16)? as usize;
                if len != (!nlen & 0xFFFF) {
                    return Err("deflate 存储块长度校验失败".to_string());
                }
                for _ in 0..len {
                    out.push(br.bits(8)? as u8);
                }
            }
            1 => {
                let (lit, dist) = fixed_tables();
                inflate_block(&mut br, &mut out, &lit, &dist)?;
            }
            2 => {
                let (lit, dist) = read_dynamic_tables(&mut br)?;
                inflate_block(&mut br, &mut out, &lit, &dist)?;
            }
            _ => return Err("deflate 非法块类型".to_string()),
        }
        if last == 1 {
            break;
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// 字符串入口（供两宿主；返回**同一** JSON 形状）
// ---------------------------------------------------------------------------

/// `GET /api/skills` —— 列出该 region 的 skill 清单。
pub fn list_skills(region: Option<String>) -> Result<Value, String> {
    let region = region_arg(region);
    let skills = list_skills_for(region)?;
    Ok(json!({ "region": region.as_str(), "skills": skills }))
}

/// `POST /api/skills/enabled` —— 启停一个 skill。
pub fn set_skill_enabled(
    region: Option<String>,
    dir_name: &str,
    enabled: bool,
) -> Result<Value, String> {
    let region = region_arg(region);
    let result = set_skill_enabled_for(region, dir_name, enabled)?;
    serde_json::to_value(&result).map_err(|e| format!("序列化启停结果失败：{e}"))
}

/// `POST /api/skills/copy-cross` —— 跨 region 合并补齐。
///
/// 两个 region **都必须显式给出**：这是跨区命令，缺省回落 `cn` 会让「搬去对侧」
/// 静默变成「源与目标相同」的报错或空操作，不如直接报缺参。
pub fn copy_skills_cross(
    source_region: Option<String>,
    target_region: Option<String>,
) -> Result<Value, String> {
    let source = source_region
        .as_deref()
        .and_then(Region::parse)
        .ok_or_else(|| "缺少或无法识别 sourceRegion".to_string())?;
    let target = target_region
        .as_deref()
        .and_then(Region::parse)
        .ok_or_else(|| "缺少或无法识别 targetRegion".to_string())?;
    let report = copy_skills_cross_for(source, target)?;
    serde_json::to_value(&report).map_err(|e| format!("序列化搬迁报告失败：{e}"))
}

/// `POST /api/skills/import` —— 从本地目录或 zip 导入一个 skill。
pub fn import_skill(region: Option<String>, source_path: &str) -> Result<Value, String> {
    let path = source_path.trim();
    if path.is_empty() {
        return Err("缺少 sourcePath".to_string());
    }
    let region = region_arg(region);
    let result = import_skill_into(region, Path::new(path))?;
    serde_json::to_value(&result).map_err(|e| format!("序列化导入结果失败：{e}"))
}

/// `POST /api/skills/open-dir` —— 确保 skills 目录存在，并返回**两宿主共用**的响应载荷。
///
/// 「在文件管理器里打开」是平台相关动作，留在各宿主（`reveal_dir`）；本函数只负责
/// 目录存在性与**响应形状**，避免两侧各拼一份 JSON 而漂移。
pub fn prepare_skills_dir(region: Option<String>) -> Result<(PathBuf, Value), String> {
    let region = region_arg(region);
    let dir = skills_dir_for(region);
    if !dir.is_dir() {
        std::fs::create_dir_all(&dir)
            .map_err(|e| format!("创建 skills 目录失败（{}）：{e}", dir.display()))?;
    }
    let payload = json!({
        "ok": true,
        "region": region.as_str(),
        "dir": dir.to_string_lossy(),
    });
    Ok((dir, payload))
}

// ---------------------------------------------------------------------------
// 单测
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// 建一个隔离的临时 home（已存在），避免单测触碰真实用户数据。
    fn temp_home(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "buddy-switch-skill-{label}-{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&dir).expect("创建临时 home");
        dir
    }

    /// 在临时 home 下写一个 skill 目录。
    fn write_skill(region: Region, dir_name: &str, body: &str) {
        let dir = skills_dir_for(region).join(dir_name);
        std::fs::create_dir_all(&dir).expect("创建 skill 目录");
        std::fs::write(dir.join(SKILL_FILE), body).expect("写 SKILL.md");
    }

    fn read_skill(region: Region, dir_name: &str) -> String {
        std::fs::read_to_string(skills_dir_for(region).join(dir_name).join(SKILL_FILE))
            .expect("读 SKILL.md")
    }

    // ---- frontmatter 解析 ----

    #[test]
    fn frontmatter_parses_crlf_bom_and_nested_metadata() {
        let text = "\u{feff}---\r\nname: demo\r\nmetadata:\r\n  agent: explore\r\n  nested: 1\r\ndescription: 中文描述 # 行内注释\r\n---\r\n正文\r\n";
        let fm = parse_skill_frontmatter(text).expect("应解析出 frontmatter");
        assert_eq!(fm.scalars.get("name").map(String::as_str), Some("demo"));
        assert_eq!(
            fm.scalars.get("description").map(String::as_str),
            Some("中文描述"),
            "未加引号的值应剥掉 ` # ` 之后的行内注释"
        );
        assert!(fm.has_metadata, "顶层 metadata 块必须被识别");
        assert!(
            !fm.scalars.contains_key("agent"),
            "嵌套块内的缩进行不得被误读为顶层标量"
        );
        assert_eq!(fm.eol, "\r\n");
    }

    #[test]
    fn frontmatter_absent_when_missing_or_unclosed() {
        assert!(parse_skill_frontmatter("没有 frontmatter\n").is_none());
        assert!(
            parse_skill_frontmatter("---\nname: x\n").is_none(),
            "`---` 后没有闭合行 ⇒ 判为无 frontmatter（不猜边界）"
        );
        assert!(parse_skill_frontmatter("---").is_none(), "`---` 紧跟 EOF");
        let empty = parse_skill_frontmatter("---\n---\n").expect("空 frontmatter 合法");
        assert!(empty.scalars.is_empty());
    }

    #[test]
    fn frontmatter_handles_quoted_keys_and_values_with_hash() {
        let text = "---\n\"name\": \"demo # 1\"\nversion: 1.2.3 # 版本\n---\n";
        let fm = parse_skill_frontmatter(text).expect("frontmatter");
        assert_eq!(fm.scalars.get("name").map(String::as_str), Some("demo # 1"));
        assert_eq!(fm.scalars.get("version").map(String::as_str), Some("1.2.3"));
    }

    // ---- 行级改写 ----

    #[test]
    fn rewrite_preserves_all_other_bytes() {
        let text = "\u{feff}---\r\nname: demo\r\n# 注释行\r\nmetadata:\r\n  agent: explore\r\ndescription: 描述\r\n---\r\n\r\n正文（不得改动）\r\n";
        let (out, changed) = rewrite_skill_toggle(text, false).expect("改写成功");
        assert!(changed);

        // 除新增的两行外，其余字节逐字节不变。
        let old_body = text.split("\r\n---\r\n").nth(1).expect("正文");
        let new_body = out.split("\r\n---\r\n").nth(1).expect("正文");
        assert_eq!(old_body, new_body, "闭分隔符之后的正文必须逐字节不变");
        assert!(out.contains("# 注释行\r\n"), "注释行必须保留");
        assert!(out.contains("metadata:\r\n  agent: explore\r\n"), "嵌套块必须保留");
        assert!(out.contains("name: demo\r\n"), "既有 name 行必须保留");
        assert!(out.starts_with("\u{feff}---\r\n"), "BOM 与首行分隔符必须保留");
        assert_eq!(
            out.matches("\r\n").count(),
            out.matches('\n').count(),
            "CRLF 文件不得被改成 LF"
        );
        assert!(out.contains("user-invocable: false\r\n"));
        assert!(out.contains("disable-model-invocation: true\r\n"));
    }

    #[test]
    fn rewrite_replaces_existing_values_in_place() {
        let text = "---\nname: demo\nuser-invocable: false\ndisable-model-invocation: true\n---\n";
        let (out, changed) = rewrite_skill_toggle(text, true).expect("改写成功");
        assert!(changed);
        assert!(out.contains("user-invocable: true\n"));
        assert!(out.contains("disable-model-invocation: false\n"));
        assert_eq!(out.matches("user-invocable").count(), 1, "不得重复插入字段");
        assert_eq!(out.matches("disable-model-invocation").count(), 1);
    }

    #[test]
    fn rewrite_rejects_missing_frontmatter() {
        assert!(rewrite_skill_toggle("没有 frontmatter\n", true).is_err());
    }

    #[test]
    fn toggle_round_trip_is_idempotent() {
        let original = "---\nname: demo\ndescription: 描述\n---\n正文\n";
        let (enabled, _) = rewrite_skill_toggle(original, true).expect("启用");
        let (disabled, _) = rewrite_skill_toggle(&enabled, false).expect("停用");
        let (re_enabled, _) = rewrite_skill_toggle(&disabled, true).expect("再启用");
        assert_eq!(re_enabled, enabled, "启用 → 停用 → 启用 必须往返幂等");

        let (again, changed) = rewrite_skill_toggle(&re_enabled, true).expect("重复启用");
        assert!(!changed, "已是目标状态时不得再改文件");
        assert_eq!(again, re_enabled);
    }

    // ---- 状态推导 ----

    #[test]
    fn inconsistent_detected_when_only_one_field_written() {
        let only_user = "---\nuser-invocable: false\n---\n";
        let fm = parse_skill_frontmatter(only_user).unwrap();
        assert_eq!(derive_enabled(&fm.scalars), (true, true), "只写一半 ⇒ 不一致");

        let only_disable = "---\ndisable-model-invocation: true\n---\n";
        let fm = parse_skill_frontmatter(only_disable).unwrap();
        assert_eq!(derive_enabled(&fm.scalars), (true, true));

        let stopped = "---\nuser-invocable: false\ndisable-model-invocation: true\n---\n";
        let fm = parse_skill_frontmatter(stopped).unwrap();
        assert_eq!(derive_enabled(&fm.scalars), (false, false), "两字段齐备 ⇒ 停用");

        let none = "---\nname: demo\n---\n";
        let fm = parse_skill_frontmatter(none).unwrap();
        assert_eq!(derive_enabled(&fm.scalars), (true, false), "都不写 ⇒ 默认启用");
    }

    // ---- 清单 ----

    #[test]
    fn list_skills_reads_entries_and_skips_non_skills() {
        let home = temp_home("list");
        let _guard = config::HomeOverrideGuard::set(&home);

        write_skill(
            Region::Cn,
            "beta",
            "---\nname: beta\ndescription: en only\nversion: 2.0.0\nagent_created: true\n---\n",
        );
        write_skill(
            Region::Cn,
            "alpha",
            "---\nname: alpha\ndescription: en\ndescription_zh: 中文\n---\n",
        );
        // 非 skill：无 SKILL.md 的目录 + 一个散文件。
        std::fs::create_dir_all(skills_dir_for(Region::Cn).join("not-a-skill")).unwrap();
        std::fs::write(skills_dir_for(Region::Cn).join("stray.txt"), "x").unwrap();

        let list = list_skills_for(Region::Cn).expect("列出清单");
        assert_eq!(
            list.iter().map(|s| s.dir_name.as_str()).collect::<Vec<_>>(),
            vec!["alpha", "beta"],
            "按目录名排序，且只列含 SKILL.md 的目录"
        );
        assert_eq!(list[0].description, "中文", "优先 description_zh");
        assert_eq!(list[1].version.as_deref(), Some("2.0.0"));
        assert!(list[1].agent_created);
        assert!(list[0].has_frontmatter);
        assert!(list[0].enabled);

        drop(_guard);
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn list_skills_returns_empty_when_dir_missing() {
        let home = temp_home("list-missing");
        let _guard = config::HomeOverrideGuard::set(&home);
        assert!(list_skills_for(Region::Global).unwrap().is_empty());
        drop(_guard);
        let _ = std::fs::remove_dir_all(&home);
    }

    // ---- 启停 + 备份 ----

    #[test]
    fn backup_only_when_target_existed_and_changed() {
        let home = temp_home("backup");
        let _guard = config::HomeOverrideGuard::set(&home);
        let backup_root = config::backup_dir().join("skills");

        // ① 目标不存在 ⇒ 报错，且不得产生备份目录。
        assert!(set_skill_enabled_for(Region::Cn, "ghost", false).is_err());
        assert!(!backup_root.exists(), "目标不存在时不得产生备份");

        // ② 目标存在且确有改动 ⇒ 产生备份。
        write_skill(Region::Cn, "demo", "---\nname: demo\n---\n正文\n");
        let first = set_skill_enabled_for(Region::Cn, "demo", false).expect("停用");
        assert!(first.changed);
        let backup = first.backup.expect("有改动时必须备份");
        assert!(backup.is_file(), "备份文件必须存在：{}", backup.display());
        assert!(
            backup.starts_with(&backup_root),
            "备份必须落在 backups/skills/ 下"
        );

        // ③ 已是目标状态 ⇒ 无改动、无备份。
        let second = set_skill_enabled_for(Region::Cn, "demo", false).expect("重复停用");
        assert!(!second.changed);
        assert!(second.backup.is_none(), "无改动时不得备份");

        drop(_guard);
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn toggle_rejects_skill_without_frontmatter() {
        let home = temp_home("toggle-nofm");
        let _guard = config::HomeOverrideGuard::set(&home);
        write_skill(Region::Cn, "raw", "没有 frontmatter 的 SKILL.md\n");
        let err = set_skill_enabled_for(Region::Cn, "raw", false).unwrap_err();
        assert!(err.contains("frontmatter"), "错误文案应指出缺 frontmatter：{err}");
        assert_eq!(read_skill(Region::Cn, "raw"), "没有 frontmatter 的 SKILL.md\n");
        drop(_guard);
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn toggle_rejects_path_traversal_dir_name() {
        let home = temp_home("toggle-traversal");
        let _guard = config::HomeOverrideGuard::set(&home);
        assert!(set_skill_enabled_for(Region::Cn, "../escape", false).is_err());
        assert!(set_skill_enabled_for(Region::Cn, "a/b", false).is_err());
        drop(_guard);
        let _ = std::fs::remove_dir_all(&home);
    }

    // ---- 跨区搬迁 ----

    #[test]
    fn copy_cross_is_idempotent_and_keeps_target_on_conflict() {
        let home = temp_home("copy");
        let _guard = config::HomeOverrideGuard::set(&home);

        write_skill(Region::Global, "a", "---\nname: a\n---\n");
        write_skill(Region::Global, "b", "---\nname: b\n---\n源侧内容\n");
        write_skill(Region::Cn, "b", "---\nname: b\n---\n目标侧内容\n");

        let first = copy_skills_cross_for(Region::Global, Region::Cn).expect("首次搬迁");
        assert_eq!(first.copied, vec!["a".to_string()]);
        assert_eq!(first.skipped_conflict, vec!["b".to_string()]);
        assert!(first.skipped_identical.is_empty());
        assert!(first.changed);
        assert!(first.backup.is_some(), "有实际复制时必须快照目标侧");
        assert!(
            read_skill(Region::Cn, "b").contains("目标侧内容"),
            "冲突必须保留目标侧，绝不静默覆盖"
        );

        let second = copy_skills_cross_for(Region::Global, Region::Cn).expect("再次搬迁");
        assert!(second.copied.is_empty(), "二次执行必须复制 0 个（幂等）");
        assert_eq!(second.skipped_identical, vec!["a".to_string()]);
        assert_eq!(second.skipped_conflict, vec!["b".to_string()]);
        assert!(!second.changed);
        assert!(second.backup.is_none(), "无复制时不得快照");

        drop(_guard);
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn copy_cross_rejects_same_region() {
        let home = temp_home("copy-same");
        let _guard = config::HomeOverrideGuard::set(&home);
        assert!(copy_skills_cross_for(Region::Cn, Region::Cn).is_err());
        drop(_guard);
        let _ = std::fs::remove_dir_all(&home);
    }

    // ---- 导入 ----

    #[test]
    fn import_from_directory_copies_skill() {
        let home = temp_home("import-dir");
        let _guard = config::HomeOverrideGuard::set(&home);

        let src = std::env::temp_dir().join(format!(
            "buddy-switch-skill-src-{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(src.join("my-skill")).unwrap();
        std::fs::write(src.join("my-skill/SKILL.md"), "---\nname: my-skill\n---\n").unwrap();

        // 顶层是唯一含 SKILL.md 的子目录 ⇒ 用子目录名作 skill 名。
        let result = import_skill_into(Region::Cn, &src).expect("导入");
        assert!(result.imported);
        assert_eq!(result.dir_name, "my-skill");
        assert!(skills_dir_for(Region::Cn)
            .join("my-skill/SKILL.md")
            .is_file());

        // 再次导入 ⇒ 内容一致，幂等跳过。
        let again = import_skill_into(Region::Cn, &src).expect("重复导入");
        assert!(again.identical && !again.imported);

        drop(_guard);
        let _ = std::fs::remove_dir_all(&home);
        let _ = std::fs::remove_dir_all(&src);
    }

    #[test]
    fn import_conflict_keeps_target_side() {
        let home = temp_home("import-conflict");
        let _guard = config::HomeOverrideGuard::set(&home);
        write_skill(Region::Cn, "dup", "---\nname: dup\n---\n目标侧\n");

        // 源用「容器目录 + 同名子目录」形态：skill 名取子目录名（`dup`），
        // 才会与目标侧已有的 `dup` 撞名 —— 直接对根目录写 SKILL.md 时名字取的是
        // 随机容器目录名，永远不会撞上 `dup`，冲突分支根本进不去。
        let src = std::env::temp_dir().join(format!(
            "buddy-switch-skill-dup-{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(src.join("dup")).unwrap();
        std::fs::write(src.join("dup/SKILL.md"), "---\nname: dup\n---\n源侧\n").unwrap();

        let result = import_skill_into(Region::Cn, &src).expect("导入");
        assert!(result.conflict && !result.imported);
        assert_eq!(result.dir_name, "dup");
        assert!(read_skill(Region::Cn, "dup").contains("目标侧"));

        drop(_guard);
        let _ = std::fs::remove_dir_all(&home);
        let _ = std::fs::remove_dir_all(&src);
    }

    #[test]
    fn import_rejects_missing_source() {
        let home = temp_home("import-missing");
        let _guard = config::HomeOverrideGuard::set(&home);
        assert!(import_skill_into(Region::Cn, Path::new("/definitely/not/here")).is_err());
        drop(_guard);
        let _ = std::fs::remove_dir_all(&home);
    }

    // ---- zip + deflate ----

    /// 构造一个只含**存储（method 0）**条目的 zip。
    fn build_stored_zip(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut out: Vec<u8> = Vec::new();
        let mut central: Vec<(usize, usize, usize, String)> = Vec::new();
        for (name, body) in files {
            let offset = out.len();
            let name_bytes = name.as_bytes();
            out.extend_from_slice(&ZIP_LOCAL);
            out.extend_from_slice(&20u16.to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes()); // method = 0（存储）
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(&0u32.to_le_bytes()); // crc（本读取器不校验）
            out.extend_from_slice(&(body.len() as u32).to_le_bytes());
            out.extend_from_slice(&(body.len() as u32).to_le_bytes());
            out.extend_from_slice(&(name_bytes.len() as u16).to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(name_bytes);
            out.extend_from_slice(body);
            central.push((offset, body.len(), name_bytes.len(), (*name).to_string()));
        }

        let cd_offset = out.len();
        for (offset, size, name_len, name) in &central {
            let name_bytes = name.as_bytes();
            out.extend_from_slice(&ZIP_CENTRAL);
            out.extend_from_slice(&20u16.to_le_bytes());
            out.extend_from_slice(&20u16.to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(&0u32.to_le_bytes());
            out.extend_from_slice(&(*size as u32).to_le_bytes());
            out.extend_from_slice(&(*size as u32).to_le_bytes());
            out.extend_from_slice(&(*name_len as u16).to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(&0u32.to_le_bytes());
            out.extend_from_slice(&(*offset as u32).to_le_bytes());
            out.extend_from_slice(name_bytes);
        }
        let cd_size = out.len() - cd_offset;
        out.extend_from_slice(&ZIP_EOCD);
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&(central.len() as u16).to_le_bytes());
        out.extend_from_slice(&(central.len() as u16).to_le_bytes());
        out.extend_from_slice(&(cd_size as u32).to_le_bytes());
        out.extend_from_slice(&(cd_offset as u32).to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out
    }

    #[test]
    fn zip_stored_entry_round_trips() {
        let zip = build_stored_zip(&[("my-skill/SKILL.md", b"---\nname: z\n---\n")]);
        let entries = read_zip_entries(&zip).expect("解析中央目录");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "my-skill/SKILL.md");
        assert_eq!(
            extract_zip_entry(&zip, &entries[0]).expect("解出条目"),
            b"---\nname: z\n---\n".to_vec()
        );
    }

    /// 构造一个 fixed-Huffman、仅字面量的 deflate 块（用于验证 inflate 的 Huffman 路径）。
    struct BitWriter {
        out: Vec<u8>,
        acc: u32,
        bits: u32,
    }

    impl BitWriter {
        fn new() -> Self {
            Self {
                out: Vec::new(),
                acc: 0,
                bits: 0,
            }
        }
        /// 写入 `n` 位（LSB 优先，用于块头 / 长度等字段）。
        fn put(&mut self, value: u32, n: u32) {
            self.acc |= (value & ((1u32 << n) - 1)) << self.bits;
            self.bits += n;
            while self.bits >= 8 {
                self.out.push((self.acc & 0xFF) as u8);
                self.acc >>= 8;
                self.bits -= 8;
            }
        }
        /// 写入一个 Huffman 码（**MSB 优先**，逐位进流）。
        fn put_code(&mut self, code: u32, len: u32) {
            for i in (0..len).rev() {
                self.put((code >> i) & 1, 1);
            }
        }
        fn finish(mut self) -> Vec<u8> {
            if self.bits > 0 {
                self.out.push((self.acc & 0xFF) as u8);
            }
            self.out
        }
    }

    /// fixed Huffman 下某个字面量符号的 (码, 位数)。
    fn fixed_literal_code(sym: u32) -> (u32, u32) {
        match sym {
            0..=143 => (0x30 + sym, 8),
            144..=255 => (0x190 + (sym - 144), 9),
            256..=279 => (sym - 256, 7),
            _ => (0xC0 + (sym - 280), 8),
        }
    }

    #[test]
    fn inflate_decodes_stored_block() {
        let mut w = BitWriter::new();
        w.put(1, 1); // BFINAL
        w.put(0, 2); // BTYPE = 00（存储）
        w.put(0, 5); // 补齐到字节边界
        let payload = b"SKILL";
        w.put(payload.len() as u32, 16);
        w.put(!(payload.len() as u32) & 0xFFFF, 16);
        for &b in payload {
            w.put(b as u32, 8);
        }
        assert_eq!(inflate_raw(&w.finish()).unwrap(), payload.to_vec());
    }

    #[test]
    fn inflate_decodes_fixed_huffman_literals() {
        let mut w = BitWriter::new();
        w.put(1, 1); // BFINAL
        w.put(1, 2); // BTYPE = 01（固定 Huffman）
        for &b in b"SKILL" {
            let (code, len) = fixed_literal_code(b as u32);
            w.put_code(code, len);
        }
        let (eob, eob_len) = fixed_literal_code(256);
        w.put_code(eob, eob_len);
        assert_eq!(inflate_raw(&w.finish()).unwrap(), b"SKILL".to_vec());
    }

    #[test]
    fn import_from_zip_extracts_and_installs() {
        let home = temp_home("import-zip");
        let _guard = config::HomeOverrideGuard::set(&home);

        let zip_path = std::env::temp_dir().join(format!(
            "buddy-switch-skill-{}.zip",
            uuid::Uuid::new_v4().simple()
        ));
        let zip = build_stored_zip(&[
            ("zipped-skill/SKILL.md", b"---\nname: zipped\n---\n"),
            ("zipped-skill/extra.txt", b"hello"),
        ]);
        std::fs::write(&zip_path, &zip).unwrap();

        let result = import_skill_into(Region::Cn, &zip_path).expect("从 zip 导入");
        assert!(result.imported);
        assert_eq!(result.dir_name, "zipped-skill");
        let dir = skills_dir_for(Region::Cn).join("zipped-skill");
        assert!(dir.join(SKILL_FILE).is_file(), "解压后的 SKILL.md 必须落到目标目录");
        assert!(dir.join("extra.txt").is_file(), "zip 内的附属文件必须一并导入");

        drop(_guard);
        let _ = std::fs::remove_dir_all(&home);
        let _ = std::fs::remove_file(&zip_path);
    }

    #[test]
    fn zip_extraction_rejects_path_traversal() {
        let home = temp_home("zip-traversal");
        let _guard = config::HomeOverrideGuard::set(&home);
        let zip_path = std::env::temp_dir().join(format!(
            "buddy-switch-skill-evil-{}.zip",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::write(&zip_path, build_stored_zip(&[("../evil.txt", b"x")])).unwrap();
        let dest = std::env::temp_dir().join(format!(
            "buddy-switch-skill-dest-{}",
            uuid::Uuid::new_v4().simple()
        ));
        assert!(extract_zip_to(&zip_path, &dest).is_err(), "必须拒绝 `..` 条目");
        drop(_guard);
        let _ = std::fs::remove_dir_all(&home);
        let _ = std::fs::remove_file(&zip_path);
        let _ = std::fs::remove_dir_all(&dest);
    }

    #[test]
    fn zip_extraction_skips_directory_entries() {
        let zip = build_stored_zip(&[("dir/", b"")]);
        let entries = read_zip_entries(&zip).unwrap();
        assert_eq!(entries.len(), 1);
        // 目录条目名以 `/` 结尾 ⇒ `extract_zip_to` 会跳过，不写空文件。
        assert!(entries[0].name.ends_with('/'));
    }
}