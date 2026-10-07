/**
 * 文案域：**回收站**（中文，键的权威之一）。
 *
 * 归属文件：`src/components/recycle-bin-sheet.tsx`（WorkBuddy 与 Trae **共用**的回收站抽屉）。
 *
 * ## 为什么单独成域，而不并入 `shared` / `wbAccounts` / `traePages`
 *
 * 回收站是**两平台共用**的独立组件（列 / 恢复 / 永久删除 / 清空），三处删除文案
 * （`wbAccounts.dialog.deleteDesc` / `shared.batch.deleteDesc` / `trae.page.accounts.deleteBody`）
 * 只负责**承诺**「删除后可在回收站恢复」，而**兑现承诺的入口**在本域。
 * 单独成域让「承诺 → 入口」的对应关系一眼可见，也避免与并行改动的域文件互相踩。
 *
 * ## 口径（与后端软删除对齐，§2.7）
 *
 * - 删除 = 移入回收站（可恢复），**保留 30 天**后自动清理；
 * - **永久删除（purge）/ 清空 = 唯一不可逆步** ⇒ 文案必须**明示不可恢复**，并说明
 *   「凭据 / 快照会一并删除」（否则「删了账号，token 还在」）。
 *
 * 键前缀：`recycleBin.`
 */
export const zh = {
  // ---- 抽屉头部 ----
  "recycleBin.title": "回收站",
  "recycleBin.description": "删除的账号会先移到这里，保留 30 天后自动清理；可随时恢复，或永久删除。",
  "recycleBin.close": "关闭",

  // ---- 区块状态（PLAN §3.6：加载用 Skeleton / 空态用 EmptyState）----
  "recycleBin.loading": "正在加载回收站…",
  "recycleBin.loadFailed": "回收站加载失败",
  "recycleBin.empty.title": "回收站是空的",
  "recycleBin.empty.description": "删除的账号会先移到这里，30 天内都可以恢复。",
  // 空态四要素里的**出口**（缺出口等于把用户丢在原地）。
  "recycleBin.empty.action": "关闭",

  // ---- 条目 ----
  "recycleBin.entry.unnamed": "未命名账号",
  "recycleBin.entry.deletedLabel": "删除于",
  // ★ 每条必须显示到期时间（§2.7：避免「以为还在」的假期待）。
  "recycleBin.entry.expiresLabel": "到期",

  // ---- 操作 ----
  "recycleBin.restore": "恢复",
  "recycleBin.purge": "永久删除",
  "recycleBin.clear": "清空回收站",
  "recycleBin.cancel": "取消",

  // ---- 永久删除的二次确认（唯一不可逆步）----
  "recycleBin.purge.title": "永久删除账号",
  "recycleBin.purge.desc": "将永久删除「{name}」，此操作不可恢复，并会一并删除其凭据 / 快照。",
  "recycleBin.purge.confirm": "永久删除",

  // ---- 清空的二次确认（批量 ⇒ 明示条数）----
  "recycleBin.clear.title": "清空回收站",
  "recycleBin.clear.desc": "将永久删除回收站中的 {n} 个账号，此操作不可恢复，并会一并删除它们的凭据 / 快照。",
  "recycleBin.clear.confirm": "清空",

  // ---- Toast ----
  "recycleBin.toast.restored": "账号已恢复",
  "recycleBin.toast.restoreFail": "恢复失败",
  "recycleBin.toast.purged": "已永久删除",
  "recycleBin.toast.purgeFail": "永久删除失败",
  "recycleBin.toast.cleared": "回收站已清空（{n} 项）",
  "recycleBin.toast.clearFail": "清空回收站失败",
} as const;
