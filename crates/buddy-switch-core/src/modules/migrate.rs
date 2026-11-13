//! 账号数据迁移编排：把源账号的 Memory / Connector 合并进目标账号（带去重）。
//!
//! 对照参考实现 `workbuddy-account-migrate/scripts/migrate.py::migrate` 的 Phase 3，
//! 但只负责**可去重的本机文件类数据**：
//!
//! - Memory：`{user_id}_memory.md` 追加合并（去重见 [`crate::modules::memory`]）
//! - Connector：`connectors/{user_id}/{mcp.json,connector-states.json}` 深合并
//!   （数组元素级去重见 [`crate::modules::connector`]）
//!
//! 会话（session）**不在本模块**：它需要改写 `workbuddy.db`，必须先在关闭 WorkBuddy
//! 之后执行，已有独立入口 [`crate::modules::session::copy_sessions_for_switch_for`]。
//! 本模块只碰普通文件，因此**不需要关进程**，也**不改变切换流程的语义**。
//!
//! 设计约束：
//! 1. 每个范围（memory / connectors）**独立成败**：一个失败不阻断另一个，报告里各自给出
//!    `error`，与参考实现「分阶段执行、逐阶段报告」的做法一致。
//! 2. 幂等：二次执行不得产生新增内容（memory 追加 0 行、connector `changed=false`）。
//! 3. 源账号与目标账号相同直接拒绝，避免自我覆盖。

use serde_json::{json, Value};

use crate::modules::connector;
use crate::modules::memory;
use crate::modules::region::Region;

/// 迁移范围开关。默认两者都做。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MigrateScope {
    /// 是否合并 Memory。
    pub memory: bool,
    /// 是否合并 Connector 配置。
    pub connectors: bool,
}

impl Default for MigrateScope {
    fn default() -> Self {
        Self {
            memory: true,
            connectors: true,
        }
    }
}

impl MigrateScope {
    /// 二者都不做时无需进入迁移流程。
    pub fn is_empty(&self) -> bool {
        !self.memory && !self.connectors
    }
}

/// 按 region 把 `source_uid` 的 Memory / Connector 合并到 `target_uid`（同一版本内）。
pub fn migrate_account_data_for(
    region: Region,
    source_uid: &str,
    target_uid: &str,
    scope: MigrateScope,
) -> Result<Value, String> {
    migrate_account_data_cross(region, source_uid, region, target_uid, scope)
}

/// 跨版本迁移：把 `source_region` 下 `source_uid` 的 Memory / Connector
/// 合并到 `target_region` 下 `target_uid`。
///
/// 返回报告（形状与前端 `MigrateResult` 契约一致）：
///
/// ```json
/// {
///   "sourceUid": "…", "targetUid": "…",
///   "sourceRegion": "cn", "targetRegion": "global",
///   "memory":     { "targetLines":0,"sourceLines":0,"appended":0,"skippedDuplicate":0,
///                   "changed":false,"backup":null },
///   "connectors": { "skipped":false,"addedKeys":0,"droppedDuplicateElements":0,
///                   "changed":false,"files":[…],"backup":null },
///   "changed": false
/// }
/// ```
///
/// `backup` 为改前原文的备份路径（`memory` 指向文件本身，`connectors` 指向该轮备份目录）；
/// 未改写或备份失败时为 `null`。**备份失败不阻断迁移**。
///
/// 单项失败时该项为 `{ "error": "…" }`，另一项照常执行。
pub fn migrate_account_data_cross(
    source_region: Region,
    source_uid: &str,
    target_region: Region,
    target_uid: &str,
    scope: MigrateScope,
) -> Result<Value, String> {
    let source_uid = source_uid.trim();
    let target_uid = target_uid.trim();
    if source_uid.is_empty() {
        return Err("缺少源账号 uid".to_string());
    }
    if target_uid.is_empty() {
        return Err("缺少目标账号 uid".to_string());
    }
    // 只有同版本同 uid 才是自我覆盖；跨版本 uid 同文属巧合（两版 uid 不同源）。
    if source_region == target_region && source_uid == target_uid {
        return Err("源账号与目标账号相同".to_string());
    }

    let mut report = json!({
        "sourceUid": source_uid,
        "targetUid": target_uid,
        "sourceRegion": source_region.as_str(),
        "targetRegion": target_region.as_str(),
    });

    let mut changed = false;

    if scope.memory {
        match memory::merge_memory_files_cross(source_region, source_uid, target_region, target_uid)
        {
            Ok(r) => {
                changed |= r.changed();
                report["memory"] = json!({
                    "targetLines": r.target_lines,
                    "sourceLines": r.source_lines,
                    "appended": r.appended,
                    "skippedDuplicate": r.skipped_duplicate,
                    "changed": r.changed(),
                    "backup": r
                        .backup
                        .as_ref()
                        .map(|p| p.to_string_lossy().to_string()),
                });
            }
            Err(e) => report["memory"] = json!({ "error": e }),
        }

        // issue #198：当前客户端的智能体长期记忆在个人工作区里，旧档案常是空壳。
        // 两项分开报告，旧的失败不得遮住新的；新项缺失/空壳必须明确 skipped。
        match memory::merge_personal_memory_files_cross(
            source_region,
            source_uid,
            target_region,
            target_uid,
        ) {
            Ok(r) => {
                changed |= r.changed();
                report["memory"]["personal"] = json!({
                    "targetLines": r.merge.target_lines,
                    "sourceLines": r.merge.source_lines,
                    "appended": r.merge.appended,
                    "skippedDuplicate": r.merge.skipped_duplicate,
                    "skipped": r.skipped,
                    "skippedReason": r.skipped_reason,
                    "changed": r.changed(),
                    "backup": r.merge.backup
                        .as_ref()
                        .map(|p| p.to_string_lossy().to_string()),
                });
            }
            Err(e) => report["memory"]["personal"] = json!({ "error": e }),
        }
    }

    if scope.connectors {
        match connector::merge_connectors_cross(
            source_region,
            source_uid,
            target_region,
            target_uid,
        ) {
            Ok(r) => {
                changed |= r.changed();
                let files: Vec<Value> = r
                    .files
                    .iter()
                    .map(|(name, f)| {
                        json!({
                            "file": name,
                            "addedKeys": f.added_keys,
                            "keptTargetKeys": f.kept_target_keys,
                            "droppedDuplicateElements": f.dropped_duplicate_elements,
                            "changed": f.changed,
                        })
                    })
                    .collect();
                report["connectors"] = json!({
                    "skipped": r.skipped,
                    "addedKeys": r.added_keys(),
                    "droppedDuplicateElements": r.dropped_duplicate_elements(),
                    "changed": r.changed(),
                    "files": files,
                    "backup": r
                        .backup
                        .as_ref()
                        .map(|p| p.to_string_lossy().to_string()),
                });
            }
            Err(e) => report["connectors"] = json!({ "error": e }),
        }
    }

    report["changed"] = json!(changed);
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_scope_covers_both_targets() {
        let s = MigrateScope::default();
        assert!(s.memory);
        assert!(s.connectors);
        assert!(!s.is_empty());
    }

    #[test]
    fn empty_scope_is_recognized() {
        let s = MigrateScope {
            memory: false,
            connectors: false,
        };
        assert!(s.is_empty());
    }

    #[test]
    fn same_uid_is_rejected_before_any_file_work() {
        let err =
            migrate_account_data_for(Region::Cn, "uid-same", "uid-same", MigrateScope::default())
                .unwrap_err();
        assert!(err.contains("相同"), "unexpected error: {err}");
    }

    #[test]
    fn blank_uids_are_rejected() {
        let err = migrate_account_data_for(Region::Cn, "   ", "uid-b", MigrateScope::default())
            .unwrap_err();
        assert!(err.contains("源账号"), "unexpected error: {err}");

        let err = migrate_account_data_for(Region::Cn, "uid-a", "  ", MigrateScope::default())
            .unwrap_err();
        assert!(err.contains("目标账号"), "unexpected error: {err}");
    }

    /// uids are trimmed before the equality check, so `" uid-a "` and `"uid-a"` collide.
    #[test]
    fn uids_are_trimmed_before_comparison() {
        let err = migrate_account_data_for(Region::Cn, " uid-a ", "uid-a", MigrateScope::default())
            .unwrap_err();
        assert!(err.contains("相同"), "unexpected error: {err}");
    }

    /// 源账号在真实 HOME 下不存在任何文件 → 报告仍须完整成形、changed=false。
    /// 这两个 uid 是刻意构造的，配合 `BUDDY_SWITCH_HOME` 之外的默认路径不会命中真实数据。
    #[test]
    fn missing_source_yields_shaped_report_without_change() {
        let report = migrate_account_data_for(
            Region::Cn,
            "migrate-test-source-absent",
            "migrate-test-target-absent",
            MigrateScope::default(),
        )
        .expect("migration should not error when source simply has no data");

        assert_eq!(report["sourceUid"], "migrate-test-source-absent");
        assert_eq!(report["targetUid"], "migrate-test-target-absent");
        assert_eq!(report["changed"], false);
        // memory：源文件不存在 → 全 0
        assert_eq!(report["memory"]["appended"], 0);
        assert_eq!(report["memory"]["changed"], false);
        assert_eq!(report["memory"]["sourceLines"], 0);
        // connectors：源目录不存在 → skipped
        assert_eq!(report["connectors"]["skipped"], true);
        assert_eq!(report["connectors"]["changed"], false);
        assert_eq!(report["connectors"]["addedKeys"], 0);
    }

    /// 跨版本**不得**因 uid 同文被拒绝：两版 uid 不同源，同文只是巧合，
    /// 且两侧是两个不同文件，不构成自我覆盖。
    #[test]
    fn cross_region_migration_allows_identical_uids() {
        let report = migrate_account_data_cross(
            Region::Cn,
            "migrate-test-cross-same-uid",
            Region::Global,
            "migrate-test-cross-same-uid",
            MigrateScope::default(),
        )
        .expect("跨版本同文 uid 应照常执行");
        assert_eq!(report["sourceRegion"], "cn");
        assert_eq!(report["targetRegion"], "global");
    }

    /// 只启用一个范围时，另一个范围不应出现在报告里（调用方据此判断是否执行过）。
    #[test]
    fn disabled_scope_omits_its_section() {
        let report = migrate_account_data_for(
            Region::Cn,
            "migrate-test-source-absent",
            "migrate-test-target-absent",
            MigrateScope {
                memory: true,
                connectors: false,
            },
        )
        .unwrap();
        assert!(report.get("memory").is_some());
        assert!(report.get("connectors").is_none());
    }
}
