/**
 * 文案域：**清除缓存**（English）。
 *
 * ⚠️ 这里**不写类型注解**：键名合法性由组合入口 `src/locales/en.ts` 单点校验。
 *
 * ★ 红线：确认弹窗必须如实列出类别与体积，并说明账号 / 配置 / 账本不受影响 ——
 * 不得使用 "safe" / "no worries" 这类无法兑现的措辞。
 */
export const en = {
  "cache.title": "Clear cache",
  "cache.intro":
    "Renewable caches and backups under the data directory, cleared by category. Clearing is irreversible; accounts, settings and usage ledgers are never touched.",
  "cache.loadFailed": "Scan failed",
  "cache.refresh": "Rescan",
  "cache.total": "Total {size}",
  "cache.storePath": "Data directory: {path}",
  "cache.entries": "{count} files",
  "cache.clear": "Clear selected…",
  "cache.clearNone": "Select at least one category first",
  "cache.confirmTitle": "Clear the selected caches?",
  "cache.confirmBody":
    "This deletes {size} across the categories below. The action is irreversible; accounts, settings and usage ledgers are never touched.",
  "cache.confirmOk": "Clear",
  "cache.confirmCancel": "Cancel",
  "cache.clearing": "Clearing…",
  "cache.cleared": "Freed {size}",
  "cache.clearedPartial": "Freed {size}; {count} items could not be cleared",
  "cache.clearFailed": "Clear failed",
  "cache.aria": "Clear {category}",

  // ---- Categories (ids match core `cache.rs` constants — do not change unilaterally) ----
  "cache.category.sessionBackups": "Session copy backups",
  "cache.category.sessionBackupsDesc":
    "Whole-database snapshots taken before each session copy / divergence handling. Clearing removes the rollback points; current sessions and accounts are unaffected.",
  "cache.category.snapshots": "Backup snapshots",
  "cache.category.snapshotsDesc":
    "Auto backups, pre-import snapshots, and auth-file / client-settings copies. Clearing removes the matching restore entries.",
  "cache.category.caches": "Renewable caches",
  "cache.category.cachesDesc":
    "Session fingerprints, official usage, model catalogs and other API/scan caches. Data is re-fetched on next use.",
  "cache.category.logs": "Run logs",
  "cache.category.logsDesc":
    "Gateway, check-in and rotation logs. Clearing removes the history used for troubleshooting.",
};
