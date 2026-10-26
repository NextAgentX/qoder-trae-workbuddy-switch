/**
 * 文案域：**聚合账号面板**（中文，键的权威之一）。
 *
 * 归属文件：`src/pages/AggregateAccountsPage.tsx` + `src/components/aggregate/*`。
 *
 * ## 为什么单独成域
 *
 * 聚合页横跨三个平台，**不属于任何一个** `wbAccounts` / `traePages` / `qoderPages`。
 * 塞进其中任何一个都会让「这一页到底归谁」变得含糊，而且改一个平台的文案会误伤另外两个。
 *
 * ## 复用什么、不复用什么
 *
 * - **复用** `shared.batch.*`（批量条按钮）与 `shared.table.*`（无障碍名）：
 *   它们是同一批动作，两处各写一遍必然漂移成「账号页叫批量签到、聚合页叫签到」。
 * - **不复用** `wbAccounts.card.*` / `traeAccounts.card.*` 的卡片文案：
 *   那些由三张卡自己 `t()`，聚合页不参与（R2「同一套 props」的直接结果）。
 *
 * 键前缀：`aggregate.`
 */
export const zh = {
  // ---- 顶栏作用域菜单里的入口（D1：第一项，与 6 个作用域用分隔线隔开）----
  "aggregate.scopeMenu.group": "全部",
  "aggregate.scopeMenu.entry": "聚合",
  "aggregate.scopeMenu.entryName": "聚合 · 全部账号",
  "aggregate.scopeMenu.entrySub": "{n} 个账号 · 跨 {scopes} 个作用域",
  /* 总数还没取到时**不显示数字**（显示 0 是撒谎）。 */
  "aggregate.scopeMenu.entrySubUnknown": "跨 {scopes} 个作用域",
  "aggregate.scopeMenu.entryAttention": "{n} 个需处理",
  "aggregate.scopeMenu.hint":
    "作用域决定「我现在管的是哪一套账号」。选「聚合」看全部 {scopes} 个作用域的账号；选任一作用域回到它的账号页。",

  // ---- 顶栏运行态 ----
  "aggregate.topbar.runningScopes": "{n} 个作用域运行中",

  // ---- 工具栏 ----
  "aggregate.toolbar.searchPlaceholder": "筛选账号（昵称 / 备注 / 邮箱）",
  "aggregate.toolbar.searchAria": "在全部作用域里筛选账号",
  "aggregate.toolbar.filter": "筛选",
  "aggregate.toolbar.filterCount": "筛选 · {n}",
  "aggregate.toolbar.group": "按作用域分组",
  "aggregate.toolbar.flat": "平铺",
  "aggregate.toolbar.groupHint": "按作用域分组：找某个渠道的号、组内操作",
  "aggregate.toolbar.flatHint": "按紧迫度混排：扫全场的待处理项",
  "aggregate.toolbar.refresh": "刷新全部作用域",
  "aggregate.count.total": "{n} 个账号 · {scopes} 个作用域",
  "aggregate.count.sorted": "{n} 个账号 · 按紧迫度排序",
  "aggregate.count.filtered": "筛选中 · 显示 {shown} / {total}",
  "aggregate.count.showAll": "显示全部",

  // ---- 分组 ----
  "aggregate.group.count": "{n} 个账号",
  "aggregate.group.attention": "{n} 个需处理",
  "aggregate.group.failed": "加载失败",
  "aggregate.group.selectAll": "全选本组",
  "aggregate.note.noAccounts": "该作用域还没有账号",
  "aggregate.note.noClient": "这台机器没有检测到 {client} 客户端，因此这里没有可管理的账号",

  // ---- 加载失败（红线：一组挂了其余照常，且不拿旧数量冒充）----
  "aggregate.fail.title": "账号列表加载失败",
  "aggregate.fail.desc": "请求超时 · 其余作用域不受影响，可单独重试",
  "aggregate.fail.retry": "重试",

  // ---- 空态（三种「空」必须彼此可辨）----
  "aggregate.empty.filtered.title": "当前筛选下没有任何账号",
  "aggregate.empty.filtered.desc": "筛选条件把 {n} 个账号全部排除了。清空筛选即可恢复全量。",
  "aggregate.empty.filtered.action": "显示全部账号",
  "aggregate.empty.none.title": "还没有任何账号",
  "aggregate.empty.none.desc": "先在某个作用域里添加账号，聚合视图会把它们自动汇总到这里。",
  "aggregate.empty.none.action": "打开作用域菜单",
  "aggregate.empty.search.title": "没有匹配的账号",
  "aggregate.empty.search.desc": "试试换个关键词，或清空搜索框。",
  "aggregate.empty.search.action": "清空搜索",
  "aggregate.loading": "正在读取 {n} 个作用域的账号…",

  // ---- 筛选面板 ----
  "aggregate.filter.title": "筛选显示范围",
  "aggregate.filter.hint": "只改变这一屏显示哪些卡片，不删除、不修改任何账号。选满即等于「显示全部」。",
  "aggregate.filter.all": "全部账号",
  "aggregate.filter.selected": "{n} / {total} 已选",
  "aggregate.filter.selectedCount": "已选 {n} / {total}",
  "aggregate.filter.selectedAll": "已选 {n} / {total} · 等于显示全部",
  "aggregate.filter.expand": "展开",
  "aggregate.filter.collapse": "收起",
  "aggregate.filter.apply": "应用",
  "aggregate.filter.cancel": "取消",
  "aggregate.filter.noAccounts": "没有账号",
  "aggregate.filter.noClient": "未安装客户端",
  "aggregate.filter.toggleAria": "切换 {scope} 下的全部账号",
  "aggregate.filter.expandAria": "展开或收起 {scope} 的账号",

  // ---- Q3 方案 C：新增账号提示 ----
  "aggregate.newcomer.title": "{n} 个账号不在当前筛选中",
  "aggregate.newcomer.note": "本次新增",
  "aggregate.newcomer.add": "加入筛选",
  "aggregate.newcomer.ignore": "忽略",

  // ---- 批量条（按钮文案复用 `shared.batch.*`，这里只加聚合页特有的那两枚）----
  "aggregate.batch.selected": "已选 {n} 个账号",
  "aggregate.batch.acrossScopes": "跨 {n} 个作用域",
  "aggregate.batch.acrossScopesHint": "批量动作会作用到这些作用域",

  // ---- 卡片「更多」菜单头部 ----
  "aggregate.menu.head": "{name} · {scope}",

  // ---- toast ----
  "aggregate.toast.entered": "已进入聚合视图",
  "aggregate.toast.enteredDesc": "{n} 个账号 · {scopes} 个作用域铺在一屏",
  "aggregate.toast.left": "已离开聚合视图，进入 {scope}",
  "aggregate.toast.filtered": "已筛选：显示 {shown} / {total} 个账号",
  "aggregate.toast.filteredDesc": "关闭应用后仍会保留这次筛选",
  "aggregate.toast.showAll": "已显示全部账号",
  "aggregate.toast.filterCleared": "筛选偏好已清除",
  "aggregate.toast.newcomerAdded": "已加入筛选（原有筛选保持不变）",
  "aggregate.toast.retried": "已重新加载 {scope}",
  "aggregate.toast.refreshed": "已刷新 {n} 个作用域",
  "aggregate.toast.refreshedDesc": "{n} 个账号的状态已更新",
  /* ⚠️ 聚合页上**唯一会离开聚合页**的卡片动作（WB 程序位未装 ⇒ 去设置诊断）。
     文案必须标明，不能装作没这回事（issue #50 R3）。 */
  "aggregate.toast.guideLeavesAggregate": "⚠️ 这一项会离开聚合页（诊断区是单作用域页面）",

  // ---- 详情抽屉（装配见 `lib/aggregate-detail.ts`）----
  "aggregate.detail.checkin": "签到",
  "aggregate.detail.checkedIn": "今日已签到",
  "aggregate.detail.notCheckedIn": "今日未签到",
  "aggregate.detail.lastUsed": "最后使用",
  "aggregate.detail.sectionIdentity": "身份",
  "aggregate.detail.sectionCredential": "凭据",
  "aggregate.detail.email": "邮箱",
  "aggregate.detail.remark": "备注",
  "aggregate.detail.group": "分组",
  "aggregate.detail.domain": "域名",
  "aggregate.detail.device": "设备指纹",
  "aggregate.detail.status": "状态",
  "aggregate.detail.jwt": "登录态（JWT）",
  "aggregate.detail.cooldown": "冷却至",
  "aggregate.detail.expiresAt": "凭据到期",
  "aggregate.detail.refreshExpiresAt": "刷新令牌到期",
  "aggregate.detail.credentialSummary": "凭据摘要",
  "aggregate.detail.credentialOk": "正常",
  "aggregate.detail.credentialExpiring": "即将过期",
  "aggregate.detail.credentialExpired": "已过期",
  "aggregate.detail.credentialRelogin": "需重新登录",
  "aggregate.detail.credentialMissing": "凭据缺失",
  "aggregate.detail.unknown": "未知",
  "aggregate.detail.packFallbackName": "积分包",
  "aggregate.detail.groupSaved": "分组已保存",

  // ---- 导出（按作用域：导出命令是单 scope 的）----
  "aggregate.export.button": "导出",
  "aggregate.export.none": "没有可导出的账号",
  "aggregate.export.pickScope": "选择要导出的作用域",
  "aggregate.export.count": "{n} 个账号",
  "aggregate.export.platform.workbuddy": "WorkBuddy",
  "aggregate.export.platform.trae": "Trae",
  "aggregate.export.platform.qoder": "Qoder",
  "aggregate.export.done": "已导出 {n} 个账号",
  "aggregate.toast.switching": "正在切换…",
  // 删除确认（issue #89 · 2.3）：撤销 toast 只是事后补救，不能替代「删之前问一句」。
  "aggregate.delete.title": "删除账号",
  "aggregate.delete.body": "确定删除「{name}」？删除后可立即用弹出的「撤销」恢复。",
  "aggregate.delete.batchBody": "确定删除选中的 {n} 个账号？删除后可立即用弹出的「撤销」恢复。",
  "aggregate.delete.cancel": "取消",
  "aggregate.delete.confirm": "删除",

  // ---- 批量动作结果 ----
  "aggregate.toast.checkinDone": "已为 {n} 个账号签到",
  "aggregate.toast.checkinPartial": "签到完成：成功 {ok} · 失败 {fail}",
  "aggregate.toast.deletePartial": "有 {fail} 个账号删除失败（其余已移入回收站）",
};
