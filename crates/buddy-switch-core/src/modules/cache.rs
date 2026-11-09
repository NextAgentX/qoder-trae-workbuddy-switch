//! 缓存清理（「清除缓存」功能）—— 分类扫描 + 按类清除。
//!
//! ## 它解决什么
//!
//! `~/.buddy-switch` 是应用的自有数据目录。其中**会话复制备份**
//! （`backups/sessions/<utc>/workbuddy.db`，每批复制整库拍一份）在加自动保留之前
//! **无界增长**——现场曾累积 3042 份、18GB。本模块给用户一个**显式、按类、
//! 先扫描体积再清除**的清理入口，并对会话备份根补上自动保留
//! （[`SESSION_BACKUP_KEEP`] / [`prune_session_backup_dirs`]），清完不再复发。
//!
//! ## 分类（id 即前后端契约，勿随意改）
//!
//! | id | 内容 | 清除后果 |
//! |:--|:--|:--|
//! | `session_backups` | `backups/sessions`、`sessions-unify`、`vscode-sessions` | 失去会话复制/分歧处理的回滚点 |
//! | `snapshots` | 自动备份、导入前快照、`*.info` 认证快照、`backups/skills\|client-settings\|memory` | 失去对应恢复入口 |
//! | `caches` | `session-fingerprints.bin`、`official_usage_cache*`、`gateway_models.*`、扫描缓存、`travel_cache.json` | 下次扫描/查询重新取数 |
//! | `logs` | `gateway_logs.json`、签到/轮换日志、`trae/logs` | 失去排查用的历史日志 |
//!
//! ## 性能（2026-10-19 并行化）
//!
//! 扫描与清除都是 IO 密集（几千个目录的枚举 / 几十 GB 的卸载），故两者都按
//! **一层 fan-out + 有界线程池**（`std::thread::scope` 工作窃取，线程数
//! = `min(available_parallelism, 8)`）并行；条目数 < 8 或单核时自动落回顺序版
//! （行为与改造前逐字一致）。server / 桌面两侧的命令另用 `spawn_blocking`
//! 包住，避免长时间占住异步运行时 / UI 线程。
//!
//! ## 硬约束
//!
//! - **绝不清**的数据不进任何类别：账号库、`state.json`、`token_daily` 账本、
//!   各类 config、网关密钥、`credit_usage_snapshots.json`  usage 历史；
//! - **fail-closed**：未知类别 id / 空类别列表 ⇒ 整次调用 `Err`，不清一半；
//! - 所有目标路径必须落在 `store_dir()` 之内（目录穿越兜底，正常不可能触发）；
//! - 部分失败不中断：逐目标收集 `errors`，成功的照常释放空间。

use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use crate::modules::config;

/// 会话复制备份的自动保留份数：`backups/{sessions,sessions-unify,vscode-sessions}`
/// 每个根只保留最新 N 份，更旧的在下一次复制流程里被清掉。
///
/// 选 10 的理由：回滚需求集中在**最近几次**操作；每份约 10MB ⇒ 上限约 30MB/根，
/// 相比无界增长（现场 18GB）是可接受的回滚深度。
pub const SESSION_BACKUP_KEEP: usize = 10;

/// 类别 id：会话复制备份（`backups/sessions`、`sessions-unify`、`vscode-sessions`）。
pub const CATEGORY_SESSION_BACKUPS: &str = "session_backups";
/// 类别 id：备份快照（自动备份 / 导入前快照 / 认证文件快照 / 客户端设置·技能·记忆备份）。
pub const CATEGORY_SNAPSHOTS: &str = "snapshots";
/// 类别 id：可再生缓存（指纹、官方用量、模型目录、扫描缓存、旅行缓存）。
pub const CATEGORY_CACHES: &str = "caches";
/// 类别 id：运行日志。
pub const CATEGORY_LOGS: &str = "logs";

/// 全部类别 id（顺序即前端展示顺序）。
pub const CATEGORY_IDS: [&str; 4] = [
    CATEGORY_SESSION_BACKUPS,
    CATEGORY_SNAPSHOTS,
    CATEGORY_CACHES,
    CATEGORY_LOGS,
];

/// 并行工作线程数上限。取 `available_parallelism` 与 8 的较小值：
/// 元数据 / 删除都是 IO 密集，超线程再多只会争抢盘（与 `session.rs` 的
/// `MAX_JSONL_COPY_WORKERS` 同思路，本模块的形状更偏「一层宽」，上限略放宽）。
fn worker_count() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        .min(8)
}

/// 条目数少于该值时不启并行：为三五个文件起线程只会更慢（线程创建 + join
/// 的固定成本压倒了收益）。实测场景（3042 个备份目录）远超该阈值。
const PARALLEL_MIN_ENTRIES: usize = 8;

/// 某类别的**全部目标路径**（文件或目录；不存在就跳过，不算错误）。
///
/// 每条路径都从 `store_dir()` / `backup_dir()` 派生，**不接受调用方传入路径** ——
/// 类别 id 是本模块与前端之间唯一的自由文本通道。
fn category_targets(id: &str) -> Result<Vec<PathBuf>, String> {
    let store = config::store_dir();
    let backups = config::backup_dir();
    let trae = store.join("trae");
    let qoder = store.join("qoder");
    let out = match id {
        CATEGORY_SESSION_BACKUPS => vec![
            backups.join("sessions"),
            backups.join("sessions-unify"),
            backups.join("vscode-sessions"),
        ],
        CATEGORY_SNAPSHOTS => {
            let mut paths = config::managed_backup_files();
            // `*.info` 认证文件快照（客户端认证文件写前的留底，见 `auth_file.rs`）。
            if let Ok(entries) = std::fs::read_dir(&backups) {
                paths.extend(entries.flatten().map(|e| e.path()).filter(|p| {
                    p.is_file() && p.extension().and_then(|e| e.to_str()) == Some("info")
                }));
            }
            paths.extend([
                backups.join("skills"),
                backups.join("client-settings"),
                backups.join("memory"),
            ]);
            paths
        }
        CATEGORY_CACHES => {
            let mut paths = vec![
                store.join("session-fingerprints.bin"),
                store.join("token-scan-cache.json"),
                store.join("client-limits-scan-cache.json"),
                store.join("travel_cache.json"),
            ];
            // 名字带规律的一类（CN/Global 各一份）：`official_usage_cache*`、`gateway_models.*`。
            if let Ok(entries) = std::fs::read_dir(&store) {
                paths.extend(entries.flatten().map(|e| e.path()).filter(|p| {
                    p.is_file()
                        && matches!(
                            p.file_name().and_then(|n| n.to_str()).unwrap_or_default(),
                            name if name.starts_with("official_usage_cache")
                                || name.starts_with("gateway_models")
                        )
                }));
            }
            paths
        }
        CATEGORY_LOGS => vec![
            store.join("gateway_logs.json"),
            store.join("auto_checkin_logs.json"),
            store.join("auto_rotate_logs.json"),
            // Qoder 自动签到台账（诊断用；与 WB 的 `auto_checkin_logs` 分开落盘）。
            qoder.join("checkin_logs.cn.json"),
            qoder.join("checkin_logs.global.json"),
            trae.join("api_gateway_logs.json"),
            trae.join("logs"),
        ],
        other => return Err(format!("未知的缓存类别：{other}")),
    };
    Ok(out)
}

/// 递归量一个路径：`(总字节, 文件数)`。目录不可读按 0 计（扫描尽力而为，
/// 不因个别条目失败就让整个 scan 报错）。
fn measure(path: &Path) -> (u64, usize) {
    let meta = match std::fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(_) => return (0, 0),
    };
    if meta.is_file() {
        return (meta.len(), 1);
    }
    if !meta.is_dir() {
        return (0, 0);
    }
    let mut bytes = 0u64;
    let mut files = 0usize;
    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.flatten() {
            let (b, f) = measure(&entry.path());
            bytes += b;
            files += f;
        }
    }
    (bytes, files)
}

/// 并行量体积：把根的**直接子条目**均摊到有界线程池（工作窃取），每个子树的
/// 内部遍历仍走顺序版 [`measure`]。
///
/// ## 为什么只并行一层
///
/// 目标形状是「一层宽、两层浅」——`backups/sessions/<utc>/{db,-wal,-shm}`，
/// 3042 个时间戳目录是绝对主力。对这一形状，一层 fan-out 就把 `read_dir` /
/// 元数据系统调用均摊到了所有线程；再往深里并行只会让线程数随树形爆炸，
/// 而收益趋近于零。深树场景（`trae/logs` 等）条目少，自动落回顺序版。
///
/// 小于 [`PARALLEL_MIN_ENTRIES`] 或单核机器 ⇒ 直接顺序，行为与改造前逐字一致。
fn measure_parallel(path: &Path) -> (u64, usize) {
    let meta = match std::fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(_) => return (0, 0),
    };
    if meta.is_file() {
        return (meta.len(), 1);
    }
    if !meta.is_dir() {
        return (0, 0);
    }
    let items: Vec<PathBuf> = match std::fs::read_dir(path) {
        Ok(entries) => entries.flatten().map(|e| e.path()).collect(),
        Err(_) => return (0, 0),
    };
    let workers = worker_count().min(items.len());
    if items.len() < PARALLEL_MIN_ENTRIES || workers <= 1 {
        return measure(path);
    }
    let next = AtomicUsize::new(0);
    let items = &items;
    let sums: Vec<(u64, usize)> = std::thread::scope(|scope| {
        let next = &next;
        let handles: Vec<_> = (0..workers)
            .map(|_| {
                scope.spawn(move || {
                    let mut bytes = 0u64;
                    let mut files = 0usize;
                    loop {
                        let index = next.fetch_add(1, Ordering::Relaxed);
                        let Some(item) = items.get(index) else { break };
                        let (b, f) = measure(item);
                        bytes += b;
                        files += f;
                    }
                    (bytes, files)
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|handle| handle.join().unwrap_or((0, 0)))
            .collect()
    });
    sums.into_iter()
        .fold((0, 0), |acc, s| (acc.0 + s.0, acc.1 + s.1))
}

/// 顺序递归删（并行版的工作线程内部用它 —— 线程数只在一层上有界）。
fn remove_tree(path: &Path) -> std::io::Result<()> {
    let meta = std::fs::symlink_metadata(path)?;
    if meta.is_file() {
        return std::fs::remove_file(path);
    }
    if !meta.is_dir() {
        return Ok(()); // 不存在 ⇒ 无需清除，不算失败
    }
    for entry in std::fs::read_dir(path)?.flatten() {
        remove_tree(&entry.path())?;
    }
    std::fs::remove_dir(path)
}

/// 并行删一棵树：根的**直接子条目**均摊到有界线程池，子树内部顺序删。
///
/// 与 [`measure_parallel`] 同构：目标形状「一层宽」时收益最大（删 3042 个
/// 备份目录 ≈ 把 18GB 的卸载均摊到所有线程）；条目少 / 单核 ⇒ 顺序删。
/// 部分子条目失败**不中断**其余删除，首个错误在收尾时返回。
fn remove_tree_parallel(path: &Path) -> std::io::Result<()> {
    let meta = match std::fs::symlink_metadata(path) {
        Ok(m) => m,
        // 不存在 ⇒ 无需清除（与内部分支同语义），不算失败。
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
    };
    if meta.is_file() {
        return std::fs::remove_file(path);
    }
    if !meta.is_dir() {
        return Ok(());
    }
    let items: Vec<PathBuf> = std::fs::read_dir(path)?
        .flatten()
        .map(|e| e.path())
        .collect();
    if items.is_empty() {
        return std::fs::remove_dir(path);
    }
    let workers = worker_count().min(items.len());
    if items.len() < PARALLEL_MIN_ENTRIES || workers <= 1 {
        return remove_tree(path);
    }
    let next = AtomicUsize::new(0);
    let items = &items;
    let failure: Mutex<Option<std::io::Error>> = Mutex::new(None);
    std::thread::scope(|scope| {
        for _ in 0..workers {
            let next = &next;
            let failure = &failure;
            scope.spawn(move || loop {
                let index = next.fetch_add(1, Ordering::Relaxed);
                let Some(item) = items.get(index) else { break };
                if let Err(error) = remove_tree(item) {
                    let mut slot = failure.lock().unwrap_or_else(|e| e.into_inner());
                    if slot.is_none() {
                        *slot = Some(error);
                    }
                }
            });
        }
    });
    let root_removed = std::fs::remove_dir(path);
    match failure.into_inner().unwrap_or_else(|e| e.into_inner()) {
        Some(error) => Err(error),
        None => root_removed,
    }
}

/// 并行删**一批**根路径（保留策略用：一次可能删掉上千个旧备份目录）。
/// 返回成功删除的数量；失败不计入、不中断。
fn remove_many_parallel(paths: &[PathBuf]) -> usize {
    let workers = worker_count().min(paths.len());
    if paths.len() < PARALLEL_MIN_ENTRIES || workers <= 1 {
        return paths
            .iter()
            .filter(|path| remove_tree_parallel(path).is_ok())
            .count();
    }
    let next = AtomicUsize::new(0);
    let removed = AtomicUsize::new(0);
    let paths = &paths;
    std::thread::scope(|scope| {
        for _ in 0..workers {
            let next = &next;
            let removed = &removed;
            scope.spawn(move || loop {
                let index = next.fetch_add(1, Ordering::Relaxed);
                let Some(path) = paths.get(index) else { break };
                if remove_tree_parallel(path).is_ok() {
                    removed.fetch_add(1, Ordering::Relaxed);
                }
            });
        }
    });
    removed.load(Ordering::Relaxed)
}

/// 扫描全部类别体积（**只读**，不清任何东西）。
///
/// 返回 `{ storePath, totalBytes, categories: [{ id, bytes, entries }] }` ——
/// `entries` 是文件数（给 UI 做「N 个文件」的次要信息）；`bytes` 为 0 的类别
/// 前端按「无数据」处理，不渲染清除入口。
pub fn scan() -> Value {
    let store = config::store_dir();
    let mut categories = Vec::new();
    let mut total = 0u64;
    for id in CATEGORY_IDS {
        let targets = category_targets(id).unwrap_or_default();
        let mut bytes = 0u64;
        let mut entries = 0usize;
        for target in &targets {
            let (b, f) = measure_parallel(target);
            bytes += b;
            entries += f;
        }
        total += bytes;
        categories.push(json!({ "id": id, "bytes": bytes, "entries": entries }));
    }
    json!({
        "storePath": store.to_string_lossy(),
        "totalBytes": total,
        "categories": categories,
    })
}

/// 按类别清除（**不可逆**）。返回 `{ cleared, freedBytes, errors }`。
///
/// ## fail-closed 顺序
///
/// **先校验全部类别 id、再动一个字节**：中途才发现未知 id 会留下「清了一半」
/// 的半成品状态，那比整体失败更难向用户解释。
pub fn clear(categories: &[String]) -> Result<Value, String> {
    if categories.is_empty() {
        return Err("未选择任何要清除的类别".to_string());
    }
    // 先解析全部（未知 id ⇒ 整体 Err，尚未删除任何东西）。
    let mut plans: Vec<(&str, Vec<PathBuf>)> = Vec::with_capacity(categories.len());
    for id in categories {
        if plans.iter().any(|(known, _)| *known == id.as_str()) {
            continue; // 去重：同一类别传两遍只清一次（也避免 freedBytes 双计）。
        }
        plans.push((id, category_targets(id)?));
    }

    let store = config::store_dir();
    let mut cleared = Vec::new();
    let mut errors = Vec::new();
    let mut freed = 0u64;
    for (id, targets) in &plans {
        let mut bytes = 0u64;
        for target in targets {
            // 兜底：目标必须落在数据目录内（正常由派生逻辑保证，触发了说明有 bug）。
            if !target.starts_with(&store) {
                errors.push(json!({
                    "id": id,
                    "error": format!("目标路径越出数据目录，已跳过：{}", target.display()),
                }));
                continue;
            }
            let (b, _) = measure_parallel(target);
            let removed = if target.is_dir() {
                remove_tree_parallel(target)
            } else if target.is_file() {
                std::fs::remove_file(target)
            } else {
                Ok(()) // 不存在 ⇒ 无需清除，不算失败
            };
            match removed {
                Ok(()) => bytes += b,
                Err(error) => errors.push(json!({
                    "id": id,
                    "error": format!("{}：{error}", target.display()),
                })),
            }
        }
        freed += bytes;
        cleared.push(json!({ "id": id, "bytes": bytes }));
    }
    Ok(json!({ "cleared": cleared, "freedBytes": freed, "errors": errors }))
}

/// 会话复制备份根的自动保留：每个根只留最新 `keep` 份，返回删除的目录数。
///
/// 排序依据是**目录名**（`YYYY-MM-DDTHH-MM-SSZ`，字典序即时序）—— 与
/// [`config::prune_managed_backups` 用 mtime 不同：那里的两类前缀混排会乱序，
/// 而这里三个根的目录名全是同一格式的 UTC 戳，名字排序更稳（不依赖 FS 时间戳）。
///
/// `keep == 0` 是**空操作**（不删任何东西）：0 会被误读成「删光」，而保留策略的
/// 本意绝不是清空备份（与 `prune_managed_backups` 同义）。
pub fn prune_session_backup_dirs(keep: usize) -> usize {
    if keep == 0 {
        return 0;
    }
    let backups = config::backup_dir();
    let mut removed = 0;
    for root in ["sessions", "sessions-unify", "vscode-sessions"] {
        let dir = backups.join(root);
        let mut stamps: Vec<PathBuf> = match std::fs::read_dir(&dir) {
            Ok(entries) => entries
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.is_dir())
                .collect(),
            Err(_) => continue,
        };
        if stamps.len() <= keep {
            continue;
        }
        stamps.sort();
        let stale = &stamps[..stamps.len() - keep];
        removed += remove_many_parallel(stale);
    }
    removed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::config::HomeOverrideGuard;

    /// 造一个隔离的 `~/.buddy-switch`（`BUDDY_SWITCH_HOME` 覆盖 + 一次性目录）。
    ///
    /// ⚠️ 覆盖值必须是**已存在的绝对目录**（`home_dir_override` 的护栏）⇒ 先建目录
    /// 再拿 `HomeOverrideGuard` 钉住。返回的 guard 必须活到断言结束。
    fn fixture(tag: &str) -> (tempfile::TempDir, HomeOverrideGuard) {
        let root = tempfile::TempDir::with_prefix(format!("buddy-switch-cache-{tag}-"))
            .expect("建不了临时目录");
        let guard = config::HomeOverrideGuard::set(root.path());
        (root, guard)
    }

    fn write(path: &Path, bytes: usize) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, vec![b'x'; bytes]).unwrap();
    }

    fn bytes_of(value: &Value, id: &str) -> u64 {
        value["categories"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["id"] == id)
            .expect("类别必须出现在 scan 结果里")["bytes"]
            .as_u64()
            .unwrap()
    }

    /// 未知类别 / 空列表一律 `Err`（fail-closed：不清一半）。
    #[test]
    fn clear_rejects_unknown_category_and_empty_selection() {
        let (_root, _guard) = fixture("reject");
        assert!(clear(&[]).is_err(), "空类别列表必须报错");
        assert!(
            clear(&["nope".to_string()]).is_err(),
            "未知类别 id 必须报错"
        );
        // 先合法再非法：**一个字节都不该动**（校验先于执行）。
        write(&config::store_dir().join("accounts.json"), 16);
        let plan = vec![CATEGORY_LOGS.to_string(), "not-a-category".to_string()];
        assert!(clear(&plan).is_err());
        assert!(
            config::store_dir().join("accounts.json").exists(),
            "整体校验失败时不得删除任何东西"
        );
    }

    /// 类别目标**永不越出数据目录**（目录穿越兜底的机检）。
    #[test]
    fn category_targets_never_escape_store_dir() {
        let (_root, _guard) = fixture("escape");
        let store = config::store_dir();
        for id in CATEGORY_IDS {
            for target in category_targets(id).unwrap() {
                assert!(
                    target.starts_with(&store),
                    "{id} 的目标越出了数据目录：{}",
                    target.display()
                );
            }
        }
    }

    /// 扫描按类别报体积；`snapshots` 能捞到托管备份与 `*.info` 快照。
    #[test]
    fn scan_reports_bytes_per_category() {
        let (_root, _guard) = fixture("scan");
        let store = config::store_dir();
        write(
            &store.join("backups/sessions/2026-10-13T01-06-16Z/workbuddy.db"),
            4096,
        );
        write(&store.join("backups/auto-backup-2026-10-19.json"), 2048);
        write(
            &store.join("backups/workbuddy-desktop.2026-10-16T03-59-31Z.info"),
            512,
        );
        write(&store.join("session-fingerprints.bin"), 1024);
        write(&store.join("trae/logs/a.log"), 8);

        let report = scan();
        assert_eq!(bytes_of(&report, CATEGORY_SESSION_BACKUPS), 4096);
        assert_eq!(bytes_of(&report, CATEGORY_SNAPSHOTS), 2048 + 512);
        assert_eq!(bytes_of(&report, CATEGORY_CACHES), 1024);
        assert_eq!(bytes_of(&report, CATEGORY_LOGS), 8);
        assert_eq!(
            report["totalBytes"].as_u64().unwrap(),
            4096 + 2048 + 512 + 1024 + 8
        );
    }

    /// 清除**只动选定类别**：别的类别与受保护文件原样留着。
    #[test]
    fn clear_session_backups_only_removes_that_category() {
        let (_root, _guard) = fixture("clear-one");
        let store = config::store_dir();
        write(
            &store.join("backups/sessions/2026-10-13T01-06-16Z/workbuddy.db"),
            4096,
        );
        write(
            &store.join("backups/sessions/2026-10-13T01-06-15Z/workbuddy.db"),
            4096,
        );
        write(&store.join("backups/auto-backup-2026-10-19.json"), 2048);
        write(&store.join("session-fingerprints.bin"), 1024);
        write(&store.join("accounts.json"), 16);
        write(&store.join("state.json"), 16);

        let out = clear(&[CATEGORY_SESSION_BACKUPS.to_string()]).unwrap();
        assert_eq!(out["freedBytes"].as_u64().unwrap(), 8192);
        assert!(out["errors"].as_array().unwrap().is_empty());
        assert!(
            !store.join("backups/sessions").exists(),
            " sessions 备份必须被清掉"
        );
        assert!(
            store.join("backups/auto-backup-2026-10-19.json").exists(),
            "未选中的类别不得被动"
        );
        assert!(
            store.join("session-fingerprints.bin").exists(),
            "未选中的类别不得被动"
        );
        assert!(store.join("accounts.json").exists(), "账号库永不受影响");
        assert!(store.join("state.json").exists(), "运行状态永不受影响");
    }

    /// 全量清除后，账号 / 状态 / 账本 / 配置 / usage 历史**必须还在**。
    #[test]
    fn clear_never_touches_accounts_ledger_and_config() {
        let (_root, _guard) = fixture("clear-all");
        let store = config::store_dir();
        write(
            &store.join("backups/sessions/2026-10-13T01-06-16Z/workbuddy.db"),
            4096,
        );
        write(&store.join("backups/auto-backup-2026-10-19.json"), 2048);
        write(
            &store.join("backups/skills/2026-10-13T07-20-31Z/skill.json"),
            64,
        );
        write(&store.join("session-fingerprints.bin"), 1024);
        write(&store.join("official_usage_cache.json"), 32);
        write(&store.join("gateway_models.cn.json"), 32);
        write(&store.join("gateway_logs.json"), 16);
        write(&store.join("trae/api_gateway_logs.json"), 16);
        for protected in [
            "accounts.json",
            "accounts.global.json",
            "state.json",
            "token_daily.json",
            "token_daily.global.json",
            "credit_usage_snapshots.json",
            "switch_config.json",
            "gateway_keys.json",
            "gateway_config.json",
        ] {
            write(&store.join(protected), 8);
        }

        let ids: Vec<String> = CATEGORY_IDS.iter().map(|s| s.to_string()).collect();
        let out = clear(&ids).unwrap();
        assert_eq!(
            out["freedBytes"].as_u64().unwrap(),
            4096 + 2048 + 64 + 1024 + 32 + 32 + 16 + 16
        );
        for protected in [
            "accounts.json",
            "accounts.global.json",
            "state.json",
            "token_daily.json",
            "token_daily.global.json",
            "credit_usage_snapshots.json",
            "switch_config.json",
            "gateway_keys.json",
            "gateway_config.json",
        ] {
            assert!(
                store.join(protected).exists(),
                "受保护文件被误删：{protected}"
            );
        }
        assert!(!store.join("backups/sessions").exists());
        assert!(!store.join("session-fingerprints.bin").exists());
        assert!(!store.join("gateway_logs.json").exists());
    }

    /// 并行度量与顺序度量**逐字节一致**（走真正的并行分支：条目数 ≥ 阈值）。
    ///
    /// 可证伪：把 `measure_parallel` 的工作窃取索引改成 `fetch_sub` 或漏加某个
    /// worker 的局部和 ⇒ 总量对不上，本断言即红。
    #[test]
    fn parallel_measure_matches_sequential() {
        let (_root, _guard) = fixture("par-measure");
        let store = config::store_dir();
        // 12 个子目录 × 3 个文件（≥ PARALLEL_MIN_ENTRIES ⇒ 必走并行路径）。
        let mut expected = 0u64;
        for i in 1..=12 {
            for j in 1..=3 {
                let size = 1024 * i + j;
                write(
                    &store.join(format!("backups/sessions/stamp-{i:02}/f{j}.db")),
                    size,
                );
                expected += size as u64;
            }
        }
        let (bytes, files) = measure_parallel(&store.join("backups/sessions"));
        assert_eq!(bytes, expected, "并行度量必须与写入总量一致");
        assert_eq!(files, 36, "文件数必须一个不漏");
        assert_eq!(
            measure(&store.join("backups/sessions")),
            (bytes, files),
            "并行与顺序口径必须一致"
        );
        // 单文件 / 不存在两种边界也必须与顺序版一致。
        let file = store.join("single.bin");
        write(&file, 100);
        assert_eq!(measure_parallel(&file), (100, 1));
        assert_eq!(measure_parallel(&store.join("nope")), (0, 0));
    }

    /// 并行删除把整棵树删干净（含超过并行阈值的宽目录）。
    #[test]
    fn parallel_remove_clears_everything() {
        let (_root, _guard) = fixture("par-remove");
        let store = config::store_dir();
        let root = store.join("backups/sessions");
        for i in 1..=12 {
            write(
                &root.join(format!("2026-10-{i:02}T00-00-00Z/workbuddy.db")),
                4096,
            );
            write(
                &root.join(format!("2026-10-{i:02}T00-00-00Z/workbuddy.db-wal")),
                64,
            );
        }
        remove_tree_parallel(&root).expect("并行删除必须成功");
        assert!(!root.exists(), "整棵树必须被删干净");
        // 文件目标与不存在目标都必须可接受。
        let file = store.join("single.bin");
        write(&file, 8);
        remove_tree_parallel(&file).expect("文件也必须能删");
        assert!(!file.exists());
        remove_tree_parallel(&store.join("nope")).expect("不存在不是失败");
    }

    /// 并行收益实证（`--ignored`：构造 3000 个目录，本地手动跑，不进常规测试）。
    ///
    /// 用法：`cargo test -p buddy-switch-core --lib cache::tests::bench_parallel_measure -- --ignored --nocapture`
    ///
    /// 断言只钉「并行不慢于顺序的 90%」——并行是为了在大目录上更快，不是为了让
    /// 小目录变慢；CI 机器负载不可控，故不做绝对时长断言。
    #[test]
    #[ignore]
    fn bench_parallel_measure() {
        let (_root, _guard) = fixture("bench");
        let store = config::store_dir();
        let root = store.join("backups/sessions");
        for i in 0..3000 {
            let stamp = format!("2026-10-13T{i:05}Z");
            for name in ["workbuddy.db", "workbuddy.db-wal", "workbuddy.db-shm"] {
                write(&root.join(&stamp).join(name), 4096);
            }
        }
        let started = std::time::Instant::now();
        let sequential = measure(&root);
        let sequential_ms = started.elapsed().as_millis();
        let started = std::time::Instant::now();
        let parallel = measure_parallel(&root);
        let parallel_ms = started.elapsed().as_millis();
        assert_eq!(sequential, parallel, "两种口径必须一致");
        println!(
            "3000 目录 × 3 文件：顺序 {sequential_ms}ms，并行({} 线程) {parallel_ms}ms，提速 {:.1}×",
            worker_count(),
            sequential_ms as f64 / (parallel_ms.max(1) as f64),
        );
        assert!(
            parallel_ms * 10 <= sequential_ms * 9 + 50,
            "并行不应显著慢于顺序：seq={sequential_ms}ms par={parallel_ms}ms"
        );
    }

    /// 自动保留：只留最新 N 份；`keep == 0` 是空操作。
    #[test]
    fn prune_session_backup_dirs_keeps_newest_and_zero_is_noop() {
        let (_root, _guard) = fixture("prune");
        let store = config::store_dir();
        let root = store.join("backups/sessions");
        let mut stamps: Vec<String> = (1..=12)
            .map(|i| format!("2026-10-{i:02}T01-06-16Z"))
            .collect();
        stamps.reverse(); // 乱序写入，验证排序不依赖创建顺序
        for stamp in &stamps {
            write(&root.join(stamp).join("workbuddy.db"), 4096);
        }
        // sessions-unify 同步建几份，验证三个根都被管到。
        for i in 1..=3 {
            write(
                &store.join(format!(
                    "backups/sessions-unify/2026-10-{i:02}T00-00-00Z/workbuddy.db"
                )),
                4096,
            );
        }

        assert_eq!(prune_session_backup_dirs(0), 0, "keep=0 是空操作，不是删光");

        let removed = prune_session_backup_dirs(SESSION_BACKUP_KEEP);
        assert_eq!(removed, 2, "12 份留 10 ⇒ 删 2（unify 只有 3 份，不动）");
        assert!(
            !root.join("2026-10-01T01-06-16Z").exists(),
            "最旧的必须被删"
        );
        assert!(
            !root.join("2026-10-02T01-06-16Z").exists(),
            "第二旧的必须被删"
        );
        assert!(
            root.join("2026-10-03T01-06-16Z").exists(),
            "第 3 旧起必须保留"
        );
        assert!(root.join("2026-10-12T01-06-16Z").exists(), "最新的必须保留");
        assert_eq!(
            std::fs::read_dir(&root).unwrap().count(),
            10,
            "留下的必须正好是保留份数"
        );
        assert!(
            store
                .join("backups/sessions-unify/2026-10-01T00-00-00Z")
                .exists(),
            "unify 未超量不得被删"
        );
    }
}
