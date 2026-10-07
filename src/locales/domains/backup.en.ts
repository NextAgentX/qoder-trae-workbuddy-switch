/**
 * 文案域：**全局备份与恢复**（英文）。
 *
 * ⚠️ 这里**不写类型注解**：键名合法性由组合入口 `src/locales/en.ts` 单点校验。
 *
 * ★ 红线（§7 #2）：`backup.credentials.notice` **不得**译成「backup complete」；
 * 必须如实说「凭据不含在内，导入后需重新登录」（Credentials are not included…）。
 */
export const en = {
  // ---- Export ----
  "backup.export.title": "Export backup",
  "backup.export.intro":
    "Covers all scopes in one file. Credentials are excluded by default — no token field appears in the file.",
  "backup.export.scopeAria": "Select all {scope}",
  "backup.export.partAria": "{part} of {scope}",
  "backup.export.part.accounts": "accounts",
  "backup.export.part.config": "config",
  "backup.export.part.gateway": "gateway",
  "backup.export.credsLabel": "Include sign-in credentials (tokens)",
  "backup.export.credsDesc": "Anyone with the file can sign in to your accounts",
  "backup.export.credsWarning":
    "The file will contain plaintext credentials. Keep it somewhere trusted and never send it over chat.",
  "backup.export.credsConfirmTitle": "Export a backup with credentials?",
  "backup.export.credsConfirmBody":
    "The file will contain plaintext tokens; anyone who has it can sign in to your accounts. Keep it somewhere trusted.",
  "backup.export.credsConfirmOk": "Export anyway",
  "backup.export.credsConfirmCancel": "Cancel",
  "backup.export.estimate":
    "About ~{size} KB · {accounts} accounts · {scopes} scopes · {parts} sections",
  "backup.export.credentialsOn": "with credentials",
  "backup.export.credentialsOff": "without credentials",
  "backup.export.button": "Export to file…",
  "backup.export.buttonCreds": "Export (with credentials)…",
  "backup.export.emptyHint": "Select at least one section to export",
  "backup.export.saved": "Exported to: {path}",
  "backup.export.error": "Export failed",

  // ---- ★ Credentials red line (§7 #2): never "backup complete" ----
  "backup.credentials.notice": "Credentials are not included; you'll need to sign in again after import",

  // ---- Import ----
  "backup.import.title": "Import backup",
  "backup.import.intro":
    "Pick a file to preview the added / overwritten / skipped counts before anything is written. A snapshot is taken automatically first.",
  "backup.import.choose": "Choose backup file…",
  "backup.import.reading": "Reading…",
  "backup.import.previewError": "Preview failed",
  "backup.import.error": "Import failed",
  "backup.import.preview.head": "Importing {kind}",
  "backup.import.preview.meta": "formatVersion {version} · {exportedAt} · {credentials}",
  "backup.import.preview.credentialsIncluded": "with credentials",
  "backup.import.preview.credentialsExcluded": "without credentials",
  "backup.import.preview.counts":
    "Added {added} · Overwritten {overwritten} · Skipped {skipped} · Conflicts {conflicts}",
  "backup.import.preview.rejected": "Section rejected (unknown scope)",
  "backup.import.preview.configs": "Config: {count} items will be overwritten ({items})",
  "backup.import.preview.relogin":
    "This file has no credentials ⇒ {count} accounts will need to sign in again after import.",
  "backup.import.preview.noScopes": "This file contains no scope sections.",
  "backup.import.conflict.label": "Conflict handling",
  "backup.import.conflict.desc": "What to do when an account (matched by uid) already exists",
  "backup.import.conflict.keepLocal": "Keep local",
  "backup.import.conflict.overwrite": "Overwrite",
  "backup.import.conflict.keepBoth": "Keep both",
  "backup.import.cancel": "Cancel",
  "backup.import.run": "Start import",
  "backup.import.result.title": "Import result",
  "backup.import.result.summary": "Added {added} · Overwritten {overwritten} · Skipped {skipped}",
  "backup.import.result.failed": "{count} accounts failed to import",
  "backup.import.result.retry": "Retry failed items",
  "backup.import.result.snapshot":
    "A snapshot of the pre-import state was taken; roll back from “Restore from auto backup”.",
  "backup.import.result.done": "Done",

  // ---- Auto backup ----
  "backup.auto.title": "Auto backup",
  "backup.auto.enableLabel": "Enable scheduled auto backup",
  "backup.auto.enableDesc":
    "Backs up once a day and keeps the latest 7. Turning it off stops new backups; existing ones are kept.",
  "backup.auto.listTitle": "Backup history",
  "backup.auto.empty": "No auto backups yet",
  "backup.auto.emptyBody":
    "Scheduled backup creates one a day, and a snapshot is taken before each import; they appear here once created.",
  "backup.auto.kind.auto": "Scheduled",
  "backup.auto.kind.preImport": "Pre-import",
  "backup.auto.kind.other": "Other",
  "backup.auto.restore": "Restore from this backup",
  "backup.auto.restoreTitle": "Restore from backup?",
  "backup.auto.restoreBody":
    "Restoring overwrites current data. A snapshot is taken before import, so you can roll back again.",
  "backup.auto.restoreOk": "Restore",
  "backup.auto.restoreCancel": "Cancel",
  "backup.auto.saved": "Auto backup setting saved",
  "backup.auto.saveFailed": "Save failed",
  "backup.auto.loadFailed": "Load failed",
  "backup.auto.restored": "Restored from backup",
  "backup.auto.restoreFailed": "Restore failed",
  "backup.auto.refresh": "Refresh",
};
