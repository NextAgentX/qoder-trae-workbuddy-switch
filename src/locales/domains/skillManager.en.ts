/**
 * 文案域：**Skill 管理（cc-switch 式）** —— English。
 *
 * 与 `skillManager.zh.ts` **逐键对齐**（只留被引用的键）。
 * 归属文件：`src/components/skill-manager.tsx`。
 *
 * 键前缀：`skillManager.`
 */
export const en = {
  "skillManager.entry": "Skills",
  "skillManager.title": "Skill manager",
  "skillManager.description":
    "Scan the CN / Global skills directory: enable, copy across regions, import locally.",

  "skillManager.loading": "Loading…",
  "skillManager.reload": "Refresh",
  "skillManager.openDir": "Open folder",
  "skillManager.error": "Failed to load: {message}",

  "skillManager.empty.title": "No skills in this version yet",
  "skillManager.empty.description":
    "The skills folder is empty. Import from a local folder / zip, or copy from the other region.",

  "skillManager.count": "{n} total",
  "skillManager.version": "v{version}",

  "skillManager.enabled": "Enabled",
  "skillManager.disabled": "Disabled",
  /** Accessible label for the toggle; `name` comes from the call site. */
  "skillManager.toggle.aria": "Enable / disable {name}",
  "skillManager.toggle.failed": "Toggle failed: {message}",

  "skillManager.badge.agentCreated": "Agent-created",
  "skillManager.badge.manual": "Manual",

  /** Only one of the two enable fields is present. */
  "skillManager.inconsistent": "Inconsistent — click to fix",
  "skillManager.inconsistent.hint":
    "Only one of the two enable fields is set; toggling writes both.",

  "skillManager.noFrontmatter": "No frontmatter — cannot toggle",

  "skillManager.copyCross": "Copy from the other region",
  "skillManager.copyCross.source": "Source: {source}",
  "skillManager.copyCross.running": "Copying…",
  "skillManager.copyCross.done": "Copy complete",
  "skillManager.copyCross.none": "Nothing to copy",
  "skillManager.copyCross.copied": "{n} added",
  "skillManager.copyCross.conflict": "{n} conflicts skipped",
  "skillManager.copyCross.identical": "{n} already present",
  "skillManager.copyCross.conflictNote":
    "Same name but different content — the target side was kept (not overwritten).",
  "skillManager.copyCross.failed": "Copy failed: {message}",

  "skillManager.import": "Import from local",
  "skillManager.import.placeholder": "Full path to a folder or .zip",
  "skillManager.import.running": "Importing…",
  "skillManager.import.done": "Imported {name}",
  "skillManager.import.identical": "{name} already exists with identical content — skipped",
  "skillManager.import.conflict": "{name} already exists with different content — target kept",
  "skillManager.import.failed": "Import failed: {message}",

  "skillManager.webuiNote":
    "Remote (webui) mode: this operates on the skills folder of the SERVER machine, not this one.",
} as const;