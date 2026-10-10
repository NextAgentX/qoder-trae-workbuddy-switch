/**
 * 文案域：**Skill 管理（cc-switch 式）** —— 中文（键的权威之一）。
 *
 * 归属文件：`src/components/skill-manager.tsx`。
 *
 * ## 为什么单独成域，而不并入 `wbSettings`
 *
 * Skill 管理是**跨产品分区共用**的应用级入口（与「通用设置」同区，见 `App.tsx`），
 * 且自带一整套语义（清单 / 启停 / 跨区搬迁 / 导入）。混进某个产品的设置域会让
 * 「哪些键属于 skill」不可查。完整背景见 `.trellis/tasks/09-22-skill-manager/prd.md`。
 *
 * 键前缀：`skillManager.`
 */
export const zh = {
  "skillManager.entry": "Skill 管理",
  "skillManager.title": "Skill 管理",
  "skillManager.description": "扫描国内版 / 国际版的 skills 目录：启停、跨区搬迁、从本地导入。",

  "skillManager.loading": "加载中…",
  "skillManager.reload": "刷新",
  "skillManager.openDir": "打开目录",
  "skillManager.error": "加载失败：{message}",

  "skillManager.empty.title": "该版本还没有 skill",
  "skillManager.empty.description":
    "skills 目录为空。可以从本地目录 / zip 导入，或从对侧区域搬迁。",

  "skillManager.count": "共 {n} 个",
  "skillManager.version": "v{version}",

  "skillManager.enabled": "已启用",
  "skillManager.disabled": "已停用",
  /** 开关的无障碍标签；`name` 由调用点传入。 */
  "skillManager.toggle.aria": "启用 / 停用 {name}",
  "skillManager.toggle.failed": "切换失败：{message}",

  "skillManager.badge.agentCreated": "Agent 创建",
  "skillManager.badge.manual": "手动",

  /** 两个启停字段只写了一半。 */
  "skillManager.inconsistent": "状态不一致，点击修复",
  "skillManager.inconsistent.hint": "两个启停字段只写了一半，点击开关会一并写齐。",

  "skillManager.noFrontmatter": "缺少 frontmatter，不可启停",

  "skillManager.copyCross": "从对侧区域搬迁",
  "skillManager.copyCross.source": "源：{source}",
  "skillManager.copyCross.running": "搬迁中…",
  "skillManager.copyCross.done": "搬迁完成",
  "skillManager.copyCross.none": "没有需要搬迁的 skill",
  "skillManager.copyCross.copied": "新增 {n}",
  "skillManager.copyCross.conflict": "冲突跳过 {n}",
  "skillManager.copyCross.identical": "已存在 {n}",
  "skillManager.copyCross.conflictNote": "同名但内容不同，已保留目标侧（不覆盖）。",
  "skillManager.copyCross.failed": "搬迁失败：{message}",

  "skillManager.import": "从本地导入",
  "skillManager.import.placeholder": "目录或 .zip 的完整路径",
  "skillManager.import.running": "导入中…",
  "skillManager.import.done": "已导入 {name}",
  "skillManager.import.identical": "{name} 已存在且内容一致，已跳过",
  "skillManager.import.conflict": "{name} 已存在且内容不同，已保留目标侧",
  "skillManager.import.failed": "导入失败：{message}",

  "skillManager.webuiNote": "当前为远程（webui）模式：操作的是服务端机器上的 skills 目录，不是本机。",
} as const;