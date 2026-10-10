/**
 * 文案域：**上游客户端 settings.json 统一托管** —— English。
 *
 * 与 `clientSettings.zh.ts` **逐键对齐**（只留被引用的键）。
 * 归属文件：`src/components/client-settings.tsx`。
 *
 * 键前缀：`clientSettings.`
 */
export const en = {
  "clientSettings.entry": "Client settings",
  "clientSettings.title": "Upstream client settings",
  "clientSettings.description":
    "Manage the proxied clients' own settings.json: read, edit by key, back up, restore, and compare across slots.",
  "clientSettings.webuiNote":
    "Remote (webui) mode: this reads and writes the client config files on the SERVER machine, not this one.",

  "clientSettings.reload": "Refresh",
  "clientSettings.loading": "Loading…",
  "clientSettings.error": "Failed to load: {message}",

  "clientSettings.product.workbuddy": "WorkBuddy",
  "clientSettings.product.trae": "Trae",

  "clientSettings.slot.workbuddyCn": "WorkBuddy CN",
  "clientSettings.slot.workbuddyGlobal": "WorkBuddy Global",
  "clientSettings.slot.traeCodeCn": "Trae CN",
  "clientSettings.slot.traeCodeGlobal": "Trae Global",
  "clientSettings.slot.traeWorkCn": "TRAE SOLO CN",
  "clientSettings.slot.traeWorkGlobal": "TRAE SOLO",

  "clientSettings.status.installed": "Installed",
  "clientSettings.status.notInstalled": "Not installed",
  "clientSettings.status.running": "Running",
  "clientSettings.status.stopped": "Not running",

  "clientSettings.path": "Path",
  "clientSettings.format": "Formatting",
  "clientSettings.format.compact": "Compact (no newlines)",
  "clientSettings.format.flat": "No indent",
  "clientSettings.format.tab": "Tab",
  "clientSettings.format.spaces": "{n} spaces",
  "clientSettings.format.other": "Custom ({indent})",

  "clientSettings.notInstalled.title": "This client is not installed",
  "clientSettings.notInstalled.description":
    "No settings.json was found for this slot, so it cannot be read or edited. Install the client and run it at least once.",

  "clientSettings.keys": "{n} keys",
  "clientSettings.raw": "Raw text",
  "clientSettings.raw.show": "Show raw text",
  "clientSettings.raw.hide": "Hide raw text",

  "clientSettings.edit.title": "Edit by key",
  "clientSettings.edit.hint":
    "Keys accept dot paths (e.g. claw.channels.wechatmp.enabled). Writing preserves the original formatting, changes only the target key's line, and backs up automatically first.",
  "clientSettings.edit.key": "Key",
  "clientSettings.edit.keyPlaceholder": "Dot path, e.g. http.proxy",
  "clientSettings.edit.value": "New value",
  "clientSettings.edit.valuePlaceholder": "JSON value, e.g. \"\" or true or {\"a\":1}",
  "clientSettings.edit.save": "Write",
  "clientSettings.edit.saving": "Writing…",
  "clientSettings.edit.done": "Wrote {keys}",
  "clientSettings.edit.unchanged": "Value is identical to the current one — nothing written or backed up",
  "clientSettings.edit.invalidJson": "The new value is not valid JSON: {message}",
  "clientSettings.edit.emptyKey": "Enter the key to edit first",
  "clientSettings.edit.backup": "Previous content backed up to {path}",
  "clientSettings.edit.noBackup": "No backup was produced (nothing changed, or the backup failed)",
  "clientSettings.edit.failed": "Write failed: {message}",

  "clientSettings.backups.title": "Backups",
  "clientSettings.backups.count": "{n} total",
  "clientSettings.backups.empty": "No backups yet. One is created automatically before every real change.",
  "clientSettings.backups.restore": "Restore",
  "clientSettings.backups.restoring": "Restoring…",
  "clientSettings.backups.done": "Restored to {id} (the current file was backed up first)",
  "clientSettings.backups.failed": "Restore failed: {message}",

  "clientSettings.diff.title": "Compare slots",
  "clientSettings.diff.hint":
    "Compare any two slots and get three buckets: only in A / only in B / different value (nested objects are flattened to dot paths). Across products, proxy fields are merged into one group via the semantic mapping.",
  "clientSettings.diff.a": "Slot A",
  "clientSettings.diff.b": "Slot B",
  "clientSettings.diff.run": "Compare",
  "clientSettings.diff.running": "Comparing…",
  "clientSettings.diff.onlyA": "Only in A",
  "clientSettings.diff.onlyB": "Only in B",
  "clientSettings.diff.different": "Different",
  "clientSettings.diff.same": "{n} same",
  "clientSettings.diff.empty": "The two slots have no differences.",
  "clientSettings.diff.failed": "Compare failed: {message}",
  "clientSettings.diff.proxy": "Proxy (merged across products)",
  "clientSettings.diff.proxySame": "equivalent",
  "clientSettings.diff.proxyDifferent": "different",
  "clientSettings.diff.proxyAbsent": "not set",
} as const;