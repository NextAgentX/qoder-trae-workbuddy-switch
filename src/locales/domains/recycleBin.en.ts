/**
 * 文案域：**Recycle Bin**（English）—— 与 `recycleBin.zh.ts` **逐键对齐**。
 *
 * ⚠️ 本文件刻意**不写类型注解**（与其余 `domains/*.en.ts` 一致）：键的权威在 `zh.ts`，
 * 拼错键名会在 `en.ts` 组合处**编译报错**，避免静默丢翻译。
 *
 * 键前缀：`recycleBin.`
 */
export const en = {
  // ---- Drawer header ----
  "recycleBin.title": "Recycle Bin",
  "recycleBin.description":
    "Deleted accounts are moved here first and kept for 30 days before automatic cleanup. Restore them anytime, or delete them permanently.",
  "recycleBin.close": "Close",

  // ---- Section states ----
  "recycleBin.loading": "Loading recycle bin…",
  "recycleBin.loadFailed": "Failed to load the recycle bin",
  "recycleBin.empty.title": "Recycle bin is empty",
  "recycleBin.empty.description":
    "Deleted accounts are moved here first and can be restored within 30 days.",
  "recycleBin.empty.action": "Close",

  // ---- Entry ----
  "recycleBin.entry.unnamed": "Unnamed account",
  "recycleBin.entry.deletedLabel": "Deleted",
  "recycleBin.entry.expiresLabel": "Expires",

  // ---- Actions ----
  "recycleBin.restore": "Restore",
  "recycleBin.purge": "Delete permanently",
  "recycleBin.clear": "Empty recycle bin",
  "recycleBin.cancel": "Cancel",

  // ---- Permanent delete confirmation ----
  "recycleBin.purge.title": "Delete account permanently",
  "recycleBin.purge.desc":
    "\u201C{name}\u201D will be permanently deleted. This cannot be undone, and its credentials / snapshots will be removed as well.",
  "recycleBin.purge.confirm": "Delete permanently",

  // ---- Empty confirmation ----
  "recycleBin.clear.title": "Empty recycle bin",
  "recycleBin.clear.desc":
    "All {n} accounts in the recycle bin will be permanently deleted. This cannot be undone, and their credentials / snapshots will be removed as well.",
  "recycleBin.clear.confirm": "Empty",

  // ---- Toast ----
  "recycleBin.toast.restored": "Account restored",
  "recycleBin.toast.restoreFail": "Restore failed",
  "recycleBin.toast.purged": "Permanently deleted",
  "recycleBin.toast.purgeFail": "Permanent delete failed",
  "recycleBin.toast.cleared": "Recycle bin emptied ({n})",
  "recycleBin.toast.clearFail": "Failed to empty the recycle bin",
} as const;
