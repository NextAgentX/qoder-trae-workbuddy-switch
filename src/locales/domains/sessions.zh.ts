/**
 * 文案域：**会话工作台**（PLAN §3.15 / P8）。
 *
 * 域划分见 `src/locales/zh.ts` 的组合入口。改动本文件请遵守那里的三条约定。
 *
 * ⚠️ **只保留被引用的键**：本仓没有「未使用键」检查，多余的键会静默腐坏。
 * 本域的键被 `App.tsx`（`DEST_META.sessions`）、`pages/SessionsPage` /
 * `pages/TraeSessionsPage`，以及 `components/restart-confirm-dialog`（P8 T03 统一重启确认）
 * 引用。
 */
export const zh = {
  /* 侧栏第 6 目的地标签（P8）。刻意放在本域而非 `shell.zh.ts`：
     它属 P8 功能词条，且 `shell` 域被多个并行任务编辑 —— 放这里避免重键与并发踩踏。 */
  "nav.sessions": "会话",

  /* 会话工作台页头标题（两平台共用）。作用域副标题在 `shell.zh.ts` 的 `pageHead.sessions`
     —— 所有页头的「作用域 · 说明」都住在那里，避免同一个约定散落两处。 */
  "sessions.title": "关联会话",

  /* ★ 会话页工具栏（原型 `viewSessions`，:3183）：标题 = 「{客户端} 会话」+ 计数徽标。
     ⚠️ **不渲染客户端 Tab**（会话复制只有 WorkBuddy ⇒ 单选项是假控件，§3.15.1）
     ⇒ 标题退化成静态文本，功能一点不少；客户端名走 `toolbar.client`（不写死在页面里）。 */
  "sessions.toolbar.title": "{client} 会话",
  "sessions.toolbar.client": "WorkBuddy 桌面",
  /* 卡片头部（原型 `.sg-head`）：状态徽标 +「N 个副本」；以及可点区的无障碍标签。 */
  "sessions.card.replicaCount": "{count} 个副本",
  "sessions.card.openAria": "打开会话组 {title}",

  /* WorkBuddy 空态（§1.5）：ledger 无登记 ⇒ 结构化空态 + **出口**（去账号页复制）。 */
  "sessions.empty.title": "尚未用本应用复制过会话",
  "sessions.empty.description": "会话副本来自本应用的复制记录。先在账号页复制一次会话，这里就会出现它的副本归属。",
  "sessions.empty.action": "去账号页复制",

  /* 加载失败：不静默、不假装空（给出原因 + 重试出口）。 */
  "sessions.error.retry": "重试",

  /* ★ 副本只读视图（P8 T02）。
     ⚠️ `partialNote` 是**固定文案**（§1.4 硬约束 ③）：ledger 只覆盖「经本应用复制过」的会话，
     **禁止**改成「全部副本」这类假承诺（§3.5）。 */
  "sessions.replica.groupTitle": "本组 {count} 个副本",
  "sessions.replica.partialNote": "仅显示经本应用复制过会话",
  // ★ P8：**内容级**状态（由会话 jsonl 的内容指纹派生，**不是** updatedAt）。
  "sessions.replica.status.latest": "已一致",
  "sessions.replica.status.behind": "待同步",
  "sessions.replica.status.diverge": "有分歧",
  "sessions.replica.status.unknown": "状态未知",
  "sessions.replica.commonPrefix": "共同旧版 {count} 条",
  "sessions.replica.delta": "+{count} 条",
  // 状态筛选（**按需出现**：至少 2 个可选状态才渲染整条）。
  "sessions.filter.all": "全部（{count}）",
  "sessions.replica.hasHistory": "含历史",
  "sessions.replica.titleFallback": "标题未知",
  "sessions.replica.updatedUnknown": "—",
  /* uid 不在账号库（已删除 / 从未保存）⇒ uid 前缀 + 「未知账号」。**不许**不显示（✅ #3）：
     缺一行会让用户以为「这条副本没有来源」，而实际是「来源账号已删」—— 含义完全不同。 */
  "sessions.replica.unknownAccount": "{uid}（未知账号）",

  /* Trae 能力边界（§1.3）：URL 直达 ⇒ 结构化空态 + **说明原因** + 出口。
     ⚠️ 标题带 `{product}` 占位（跨语言都不写死产品名）；**绝不**列出 WorkBuddy 的会话组。 */
  "sessions.unsupported.title": "{product} 暂不支持会话管理",
  "sessions.unsupported.description": "会话复制依赖该平台自身的会话存储与云端归属机制，当前只有 WorkBuddy 具备该能力。",
  "sessions.unsupported.action": "去账号页",

  /* ★ P8「安全同步」（增量追加，§3.15.4）—— **加法**语义。
     ⚠️ 文案必须写明「只追加、不删除目标已有内容」：用户对「同步」最大的恐惧就是**被覆盖**
     （`PLAN.md` §3.15.4 明确要求 UI 显式写出这一点）。
     ⚠️ 分歧组**没有**这条出口（要先人工决定保留哪一份）⇒ 本组文案不涉及分歧。 */
  "sessions.sync.action": "同步安全项（{count}）",
  "sessions.sync.all": "同步全部落后项（{count}）",
  "sessions.sync.hint": "只把来源的新增内容追加到目标，不删除目标已有内容",
  // ★ D2：常驻说明（不只靠 hover 的 title）。文案**必须含「加法」二字** ——
  //   用户对「同步」最大的恐惧是「我的内容被覆盖」，而这句是唯一的常驻承诺。
  "sessions.sync.additiveNote": "同步是加法：只往目标追加来源新增的内容，不会删除或覆盖目标已有内容。",
  "sessions.sync.toast.done": "同步完成",
  "sessions.sync.toast.failAll": "同步失败",
  "sessions.sync.toast.ok": "已同步 {n} 项，追加 {lines} 条",
  "sessions.sync.toast.failed": "{n} 项失败",
  "sessions.sync.toast.retryFailed": "只重试失败项",
  "sessions.sync.toast.error": "同步请求失败",

  /* ★ P8「分歧处理」（覆盖式统一，§3.15.2）—— 与上面的「安全同步」**语义相反**：
     同步是加法（只追加），这里是**覆盖**（目标的分歧内容会被丢弃）。
     ⚠️ 文案红线（§3.15.2 / §3.5）：**不许**出现「同步到全部副本」这类静默覆盖措辞 ——
     有分歧时**必须先让用户选保留哪一份**（`verify-behaviors.cjs` 有对应断言）。 */
  "sessions.unify.action": "处理分歧（{count}）",
  "sessions.unify.hint": "有分歧时必须先选保留哪一份；会覆盖其余副本（覆盖前自动备份）",
  "sessions.unify.title": "选择要保留的那一份",
  "sessions.unify.desc": "这些副本在共同旧版之后各自改过，无法合并。请选择保留哪一份，其余副本会被它覆盖。",
  "sessions.unify.mostComplete": "内容最全",
  "sessions.unify.overwrite": "将覆盖其余 {n} 个副本，丢弃 {lost} 条它们独有的内容（覆盖前已自动备份）。",
  "sessions.unify.overwriteUnknown": "将覆盖其余 {n} 个副本，丢弃的条数未知（覆盖前已自动备份）。",
  "sessions.unify.cancel": "取消",
  "sessions.unify.confirm": "统一到其他副本",
  "sessions.unify.toast.done": "已统一",
  "sessions.unify.toast.failAll": "统一失败",
  "sessions.unify.toast.ok": "已统一 {n} 项，丢弃 {lost} 条",
  "sessions.unify.toast.failed": "{n} 项失败",
  "sessions.unify.toast.retryFailed": "只重试失败项",
  "sessions.unify.toast.error": "统一请求失败",

  /* ★ 统一重启确认（P8 T03 / §3.15.3）—— 跨程序位**复用同一条**流程。
     非 CLI：默认勾选「自动重开」+ 显式「未保存文件可能丢失」提示。
     ⚠️ **CLI 例外**：CLI 切换**不重开**客户端，且**当前 CLI 会话会中断** ⇒ `cli*` 三键
     **禁止**出现「自动重开」字样（写它就是假承诺，§3.5 红线），如实写「会中断」。 */
  "sessions.restart.title": "需要重启 {client}",
  "sessions.restart.description": "切换账号会先关闭再重新打开 {client}。",
  "sessions.restart.autoReopen": "切换后自动重开 {client}",
  "sessions.restart.unsavedWarning": "未保存的文件可能丢失，请先保存。",
  "sessions.restart.confirm": "继续并重启",
  "sessions.restart.cancel": "取消",
  "sessions.restart.cliTitle": "切换 {client} 需注意",
  "sessions.restart.cliDescription": "切换 {client} 只更新认证配置，不会重开客户端。",
  "sessions.restart.cliWarning": "当前 CLI 会话会中断，请先结束正在进行的任务。",
  "sessions.restart.cliConfirm": "仍要切换",
  /* 切换完成后的**明确结果**（§3.15.3）：取消勾选「自动重开」时不得沿用「已重启」。 */
  "sessions.restart.resultNotRestarted": "未重启，需手动启动 {client}",
  /* 取消勾选「自动重开」时的**进行中**提示（不得写「并重启」）。 */
  "sessions.restart.switchingNoRestart": "正在切换 {client}（不重启）",
} as const;
