/**
 * 文案域：**Qoder 产品线 · 页面**（中文，键的权威之一）。
 *
 * 归属文件：
 * `src/pages/QoderAccountsPage.tsx`、`src/pages/QoderPendingPage.tsx`、
 * `src/pages/QoderUsagePage.tsx`
 *
 * ⚠️ 命名空间用 `qoder`（与 `trae` 同款：页面与组件共用一个前缀、靠二级段区分），
 * 键前缀：`qoder.page.`
 */
export const zh = {
  "qoder.page.accounts.title": "账号",
  // ⚠️ 页头副标题走 `shared` 域的 `pageHead.accounts` / `pageHead.accounts.pending`
  //    （「{scope} · {n} 个账号 · {m} 个需处理」）—— 与 WB / Trae 同一公式，不另立门户。

  "qoder.page.accounts.sectionAria": "Qoder 账号列表（{count} 个账号）",
  "qoder.page.accounts.loadFailed": "账号列表加载失败",
  "qoder.page.accounts.recheck": "重新检测",

  "qoder.page.accounts.empty.title": "这个区域还没有 Qoder 账号",
  "qoder.page.accounts.empty.description":
    "可以从本机已登录的 Qoder 客户端导入，或粘贴一个 PAT。",
  "qoder.page.accounts.empty.action": "添加账号",
  "qoder.page.accounts.empty.sources": "本机来源",

  "qoder.page.accounts.import.description":
    "只读探测桌面版与 CLI 的登录态。不会修改客户端里的任何文件。",
  "qoder.page.accounts.import.result": "新增 {imported} 个，更新 {updated} 个",
  "qoder.page.accounts.import.none": "两处来源都没有可导入的账号",
  "qoder.page.accounts.addDrawer.notInstalled":
    "两处本机来源都未检测到。可以先用官方客户端登录后导入，或直接粘贴 PAT。",

  "qoder.page.accounts.source.desktop": "Qoder 桌面版",
  "qoder.page.accounts.source.cli": "Qoder CLI",
  "qoder.page.accounts.source.missing": "未检测到",
  "qoder.page.accounts.source.unreadable": "读不出来",
  "qoder.page.accounts.source.empty": "里面没有账号",
  "qoder.page.accounts.source.ready": "可导入",

  "qoder.page.accounts.pat.title": "粘贴 PAT",
  "qoder.page.accounts.pat.description":
    "个人访问令牌（以 pt- 开头）。适合没有装客户端、或客户端登录态读不出来的情况。",
  "qoder.page.accounts.pat.placeholder": "pt-…",
  "qoder.page.accounts.pat.action": "导入",
  "qoder.page.accounts.pat.invalid": "前缀不对：PAT 应以 pt- 开头",
  "qoder.page.accounts.pat.cancel": "取消",

  // ---- 卡片 / 表格 / 工具栏（原型 `accountCard` / `tableView` / `viewAccounts` 的槽位） ----
  "qoder.page.accounts.chip.ok": "凭据可用",
  "qoder.page.accounts.chip.noCredential": "无可用凭据",
  "qoder.page.accounts.chip.unrecognized": "凭据未识别",
  "qoder.page.accounts.chip.foreign": "归属异常",
  "qoder.page.accounts.chip.foreignTip": "这条账号的归属域名不属于当前区域（可能被错误导入）",

  "qoder.page.accounts.card.manageAria": "{name} 的更多操作",
  "qoder.page.accounts.card.moreActions": "更多操作",
  "qoder.page.accounts.card.viewDetail": "查看详情",
  "qoder.page.accounts.card.checkin": "手动签到",
  "qoder.page.accounts.card.checkinUnknownHint": "今日签到状态尚未确认 —— 点一下即向上游查询并领取（已领过会提示「今日已签到」）",
  "qoder.page.accounts.card.editRemark": "编辑备注",
  "qoder.page.accounts.card.deleteAccount": "删除账号",
  "qoder.page.accounts.card.selectAria": "选择 {name}",

  "qoder.page.accounts.toolbar.sort.name": "名称",
  "qoder.page.accounts.toolbar.sort.recent": "最近更新",
  "qoder.page.accounts.toolbar.group.none": "不分组",
  "qoder.page.accounts.toolbar.group.status": "按状态",
  "qoder.page.accounts.toolbar.statusBucket.normal": "正常",
  "qoder.page.accounts.toolbar.statusBucket.noCredential": "无可用凭据",
  "qoder.page.accounts.toolbar.statusBucket.unrecognized": "凭据未识别",

  "qoder.page.accounts.table.account": "账号",
  "qoder.page.accounts.table.status": "状态",
  "qoder.page.accounts.table.source": "来源",
  "qoder.page.accounts.table.remark": "备注",

  // ---- 详情抽屉（原型 `drawerAccount`：账号信息 + 备注**在抽屉里**编辑） ----
  "qoder.page.accounts.drawer.subtitle": "{name} · UID {uid}",
  "qoder.page.accounts.drawer.info": "账号信息",
  "qoder.page.accounts.drawer.uid": "UID",
  "qoder.page.accounts.drawer.domain": "域名",
  "qoder.page.accounts.drawer.source": "来源",
  "qoder.page.accounts.drawer.addedAt": "添加时间",
  "qoder.page.accounts.drawer.updatedAt": "最近更新",
  "qoder.page.accounts.drawer.credential": "凭据",
  "qoder.page.accounts.drawer.accessToken": "访问令牌",

  // ---- 删除（**软删除**：进回收站可恢复 ⇒ 文案如实说「可恢复」，别再写「永久删除」） ----
  "qoder.page.accounts.removed": "已删除 {name}",
  "qoder.page.accounts.domainMismatch": "归属不属于当前区域",
  "qoder.page.accounts.delete.title": "删除账号",
  "qoder.page.accounts.delete.desc": "确定删除「{name}」？删除后会进入回收站，可以恢复。",
  "qoder.page.accounts.delete.batchTitle": "批量删除账号",
  "qoder.page.accounts.delete.batchDesc": "确定删除选中的 {n} 个账号？删除后会进入回收站，可以恢复。",
  "qoder.page.accounts.delete.confirm": "删除",
  "qoder.page.accounts.delete.cancel": "取消",
  "qoder.page.accounts.delete.done": "已删除 {n} 个账号",
  "qoder.page.accounts.delete.partial": "已删除 {ok} 个，{fail} 个失败",

  "qoder.page.accounts.remark.saved": "备注已保存",
  "qoder.page.accounts.remark.missing": "这条账号已不在库里 —— 备注未保存",

  // 来源 id 是**后端数据**（`qoder::account::SOURCE_*`）；表外取值原样显示。
  "qoder.page.accounts.origin.pat": "PAT 导入",
  "qoder.page.accounts.origin.desktopApp": "桌面版导入",
  "qoder.page.accounts.origin.cli": "CLI 导入",
  "qoder.page.accounts.origin.oauth": "设备码登录",

  // ── 程序位 / 切换 / 昵称 / 分组（2026-10-15 补齐）──
  "qoder.page.accounts.program.desktop": "Qoder 桌面版",
  "qoder.page.accounts.program.cli": "Qoder CLI",
  "qoder.page.accounts.program.current": "当前登录",
  "qoder.page.accounts.program.switchTo": "切换到此账号",
  "qoder.page.accounts.program.running": "客户端正在运行",
  "qoder.page.accounts.program.runningHint": "请先退出该客户端再切换 —— 否则它退出时会把内存里的登录态写回，覆盖这次切换。",
  "qoder.page.accounts.program.missing": "未检测到",
  "qoder.page.accounts.program.unreadable": "读不出来",
  "qoder.page.accounts.program.empty": "未登录",
  "qoder.page.accounts.switch.title": "切换账号",
  "qoder.page.accounts.switch.desc":
    "把「{name}」的凭据写入 {program} 的登录态。原登录态会先备份，写入后校验失败会自动还原。",
  "qoder.page.accounts.switch.confirm": "切换",
  "qoder.page.accounts.switch.cancel": "取消",
  "qoder.page.accounts.switch.done": "已切换到 {name}",
  "qoder.page.accounts.switch.backup": "原登录态已备份到 {path}",
  "qoder.page.accounts.group.label": "分组",
  "qoder.page.accounts.group.placeholder": "留空表示未分组",
  "qoder.page.accounts.group.saved": "分组已保存",
  "qoder.page.accounts.group.missing": "这条账号已不在库里 —— 分组未保存",
  "qoder.page.accounts.toolbar.group.group": "按分组",
  "qoder.page.accounts.toolbar.group.program": "按程序位",
  "qoder.page.accounts.toolbar.programBucket.none": "未登录到任何程序位",
  "qoder.page.accounts.toolbar.groupBucket.ungrouped": "未分组",
  "qoder.page.accounts.card.setGroup": "设置分组",
  "qoder.page.accounts.refresh.batchPartial": "刷新成功 {ok} 个，失败 {fail} 个",

  // ── 用量（单位是 credits，不是 token）──
  "qoder.page.usage.title": "用量",
  "qoder.page.usage.subtitle": "{scope} · 近 {days} 天 · 单位 {unit}",
  "qoder.page.usage.unit.credits": "积分（credits）",
  "qoder.page.usage.sourceNote":
    "数据来自本机 Qoder 客户端的会话记录。Qoder 按 credits 计费，客户端记录的 token 计数恒为 0 —— 因此本视图统计的是 credits，不是 token。",
  "qoder.page.usage.totals.credits": "合计积分",
  "qoder.page.usage.totals.requests": "请求数",
  "qoder.page.usage.totals.sessions": "会话数",
  "qoder.page.usage.totals.models": "模型数",
  "qoder.page.usage.totals.sessionsCustomHint": "自定义区间不支持会话去重计数",
  "qoder.page.usage.totals.sessionsGatewayHint": "网关账本没有会话标识 —— 切到「会话」视图看账号 / 端点分布",
  // ---- 账本（issue #63 平台补齐）----
  "qoder.page.usage.ledger.gatewayNote":
    "当前看的是网关账：只统计经本机 Qoder 网关转发的调用（后台每 2 分钟增量落库，保留 90 天）。直接在 IDE 里对话、不走网关的消耗不在这本账里。",
  "qoder.page.usage.ledger.bothNote":
    "「全部」把两本账按日相加 —— 同一笔调用若既在客户端记录里又经网关转发，会被两边各记一次；要看准确消耗请切到单一账本。",
  "qoder.page.usage.trend": "每日积分",
  "qoder.page.usage.byModel": "按模型拆分",
  "qoder.page.usage.empty.title": "这段时间没有用量记录",
  "qoder.page.usage.empty.description": "本机客户端的会话记录里没有落在该窗口内的请求。",
  "qoder.page.usage.missing.title": "没有找到客户端会话记录",
  "qoder.page.usage.missing.description":
    "本机没有 Qoder 客户端的会话目录（{path}）—— 装好客户端并至少对话一次后这里会有数据。",
  "qoder.page.usage.loadFailed": "用量读取失败",
  "qoder.page.usage.recheck": "重新读取",
  "qoder.page.usage.window.label": "窗口",
  "qoder.page.usage.window.d7": "近 7 天",
  "qoder.page.usage.window.d30": "近 30 天",
  "qoder.page.usage.window.d90": "近 90 天",
  "qoder.page.usage.window.all": "全部",
  "qoder.page.usage.models.count": "模型数",
  "qoder.page.usage.models.top": "头部模型",
  "qoder.page.usage.models.topShare": "头部占比",
  "qoder.page.usage.models.stacked": "每日模型构成",
  "qoder.page.usage.models.ranking": "模型排行",
  "qoder.page.usage.models.other": "其他",
  "qoder.page.usage.models.empty": "这段时间没有模型维度的用量",
  "qoder.page.usage.sessions.count": "会话数",
  "qoder.page.usage.sessions.top": "头部会话",
  "qoder.page.usage.sessions.avg": "平均每会话",
  "qoder.page.usage.sessions.list": "会话排行",
  "qoder.page.usage.sessions.empty": "这段时间没有会话记录",
  "qoder.page.usage.sessions.untitled": "会话 {id}",
  "qoder.page.usage.sessions.requests": "{n} 次请求",
  "qoder.page.usage.sessions.range": "{from} ~ {to}",
  "qoder.page.usage.sessions.capped": "按积分降序，最多显示 {n} 个会话",
  "qoder.page.usage.sessions.note": "Qoder 没有会话复制能力：这里只读展示本机客户端会话的用量，跨设备与经网关的调用不在其中。",
  "qoder.page.usage.creditsView.remaining": "剩余合计",
  "qoder.page.usage.creditsView.total": "额度总量",
  "qoder.page.usage.creditsView.okAccounts": "可用账号",
  "qoder.page.usage.creditsView.failedAccounts": "读取失败",
  "qoder.page.usage.creditsView.perAccount": "逐账号余额",
  "qoder.page.usage.creditsView.value": "{remaining} / {total}",
  "qoder.page.usage.creditsView.exceeded": "已超额",
  "qoder.page.usage.creditsView.plan": "套餐 {plan}",
  "qoder.page.usage.creditsView.note": "这是**实时余额快照**（向上游查询），不是逐日用量；额度按上游规则重置。",
  "qoder.page.usage.creditsView.empty": "本区域还没有账号：先去「账号」页导入，余额才有数据可查。",
  "qoder.page.usage.creditsView.loadFailed": "余额读取失败",
  "qoder.page.usage.creditsView.rowFailed": "读取失败：{error}",
  "qoder.page.usage.creditsView.partial": "{n} 个账号读取失败（其余照常显示）",
  "qoder.page.usage.creditsView.reload": "重新查询",
  "qoder.page.usage.creditsValue": "{value} credits",
  "qoder.page.usage.caption": "合计 {total} credits · 单日峰值 {peak}",

  // ── OAuth 设备授权（2026-10-15 补齐：能力声明里早就有 oauth，此前是空头支票） ──
  "qoder.page.accounts.oauth.title": "设备码登录",
  "qoder.page.accounts.oauth.description":
    "在浏览器里完成 Qoder 授权，回到这里会自动采集账号（双区域的授权页不同，由当前作用域决定）。",
  "qoder.page.accounts.oauth.start": "开始登录",
  "qoder.page.accounts.oauth.starting": "正在发起…",
  "qoder.page.accounts.oauth.waiting": "等待浏览器完成授权…",
  "qoder.page.accounts.oauth.collected": "已采集账号：{name}",
  "qoder.page.accounts.oauth.failed": "登录失败",
  "qoder.page.accounts.oauth.close": "关闭",
  "qoder.page.accounts.oauth.entry": "设备码登录",
  "qoder.page.accounts.oauth.entryDesc": "用浏览器授权，适合本机还没装 Qoder 客户端的情况",

  // ── 回收站（软删除）──
  "qoder.page.accounts.trash.restored": "已恢复 {name}",
  "qoder.page.accounts.trash.undo": "撤销",

  // ── 签到 / 额度（后端早已就绪，此前无 UI 入口）──
  "qoder.page.accounts.chip.checkedIn": "今日已签",
  "qoder.page.accounts.chip.notCheckedIn": "今日未签",
  "qoder.page.accounts.chip.checkinUnavailable": "签到不可用",
  "qoder.page.accounts.metric.credits": "积分",
  "qoder.page.accounts.metric.creditsValue": "{remaining} / {total}",

  // ---- 网页端逐包明细（issue #111）----
  "qoder.page.accounts.web.loginMenu": "登录 Qoder 网页端（看逐包明细）",
  "qoder.page.accounts.web.loginToast": "已打开登录窗：登录完成后回到这里刷新即可看到逐包明细",
  "qoder.page.accounts.web.sourcePlan": "套餐额度",
  "qoder.page.accounts.web.sourceBonus": "赠送资源包",
  "qoder.page.accounts.web.sourceDedicated": "专用资源包",
  "qoder.page.accounts.web.sourceOther": "资源包",
  "qoder.page.accounts.web.neverExpires": "长期有效",
  "qoder.page.accounts.web.expired": "已过期",
  "qoder.page.accounts.web.packName": "{source} · {n}",
  "qoder.page.accounts.web.triggerUnsupported": "仅桌面版",
  "qoder.page.accounts.web.triggerLoading": "读取中…",
  "qoder.page.accounts.web.triggerPacks": "资源包 {n}",
  "qoder.page.accounts.web.triggerNeutral": "资源包",
  "qoder.page.accounts.web.triggerNoSession": "资源包 · 未登录",
  "qoder.page.accounts.web.triggerHint": "查看逐资源包明细",
  "qoder.page.accounts.web.title": "逐资源包明细",
  "qoder.page.accounts.web.summary": "{n} 个 · 共剩 {remaining}",
  "qoder.page.accounts.web.loading": "正在读取…",
  "qoder.page.accounts.web.used": "已用 {n}",
  "qoder.page.accounts.web.expiresAt": "{date} 到期",
  "qoder.page.accounts.web.state.noSession": "还没有登录 Qoder 网页端",
  "qoder.page.accounts.web.state.noSessionHint": "逐包明细只存在于网页端接口，需要在网页端登录一次（之后会记住，不必重复登录）。",
  "qoder.page.accounts.web.state.expired": "网页登录已失效",
  "qoder.page.accounts.web.state.expiredHint": "重新登录一次即可，账号本身没有受影响。",
  "qoder.page.accounts.web.state.mismatch": "网页登录的不是这个账号",
  "qoder.page.accounts.web.state.mismatchHint": "登录窗里登的是另一个账号，为避免把别人的额度显示到你这里，已丢弃该会话。请重新登录。",
  "qoder.page.accounts.web.state.upstream": "上游暂时没返回",
  "qoder.page.accounts.web.state.upstreamHint": "这不是登录问题，稍后重试即可；登录态已保留。",
  "qoder.page.accounts.web.state.unknown": "暂未取到逐包明细",
  "qoder.page.accounts.web.state.unknownHint": "登录网页端后即可查看每个积分包各剩多少、何时到期。",
  "qoder.page.accounts.web.state.unsupported": "当前形态看不到逐包",
  "qoder.page.accounts.web.state.unsupportedHint": "逐包需要内嵌浏览器采集网页会话，仅桌面版支持；聚合额度在任意形态都可用。",


  "qoder.page.accounts.metric.plan": "套餐",
  "qoder.page.accounts.credits.loading": "额度查询中…",
  "qoder.page.accounts.checkin.action": "签到",
  "qoder.page.accounts.checkin.already": "{name} 今日已签到",
  "qoder.page.accounts.checkin.done": "{name} 签到成功",
  "qoder.page.accounts.checkin.batchDone": "已签到 {n} 个账号",
  "qoder.page.accounts.checkin.batchPartial": "成功 {ok} 个，失败 {fail} 个",
  "qoder.page.accounts.checkin.resultOk": "签到成功 {n} 个",
  "qoder.page.accounts.checkin.resultFail": "签到失败 {n} 个",
  "qoder.page.accounts.pro.title": "Pro 升级包",
  "qoder.page.accounts.pro.desc": "一次性福利：领取 +1800 积分",
  "qoder.page.accounts.pro.claim": "领取",
  "qoder.page.accounts.pro.claiming": "领取中…",
  "qoder.page.accounts.pro.claimed": "已领取",
  "qoder.page.accounts.checkin.noticeUnreadable": "读不到签到状态：{error}",
  "qoder.page.accounts.checkin.noticeNoMachine": "未检测到官方客户端的机器身份，本区域的签到活动可能不可见（只影响签到，不影响模型中转）。",
  "qoder.page.accounts.checkin.noticeNoCampaign": "当前没有可领取的签到活动（每日 10:00 刷新）。",
  "qoder.page.accounts.refresh.done": "{name} 的令牌已刷新",
  "qoder.page.accounts.refresh.batchDone": "已刷新 {n} 个账号的令牌",

  "qoder.page.accounts.toolbar.sort.credits": "积分余额",
  "qoder.page.accounts.toolbar.group.source": "按来源",
  "qoder.page.accounts.toolbar.group.checkin": "按签到",
  "qoder.page.accounts.toolbar.checkinBucket.checkedIn": "今日已签",
  "qoder.page.accounts.toolbar.checkinBucket.notCheckedIn": "今日未签",
  "qoder.page.accounts.toolbar.checkinBucket.unknown": "签到状态未知",

  "qoder.page.pending.title": "这个页面尚未接入 Qoder",
  "qoder.page.pending.description":
    "Qoder 平台的账号管理、概览与设置已经可用；用量 / 网关依赖网关请求日志与用量台账，尚未落地。",
  "qoder.page.pending.action": "去账号页",

  // ---- 概览 ----
  "qoder.page.overview.title": "概览",
  "qoder.page.overview.subtitle": "{scope} · 账号与本机客户端；用量与网关尚未落地",
  "qoder.page.overview.error": "读取失败：{error}",
  "qoder.page.overview.kpi.aria": "Qoder 概览统计",
  "qoder.page.overview.kpi.accounts": "账号",
  "qoder.page.overview.kpi.accountsHint": "全部归属本区域",
  "qoder.page.overview.kpi.accountsForeign": "其中 {n} 条归属不属于本区域",
  "qoder.page.overview.kpi.accountsTitle": "本区域账号库里的条数",
  "qoder.page.overview.kpi.clients": "本机客户端",
  "qoder.page.overview.kpi.clientsReady": "有可导入的登录态",
  "qoder.page.overview.kpi.clientsNone": "两处都没检测到",
  "qoder.page.overview.kpi.clientsHint": "桌面版 / CLI 的探测结果",
  "qoder.page.overview.kpi.clientsTitle": "两处官方凭据存储里「读到了可用账号」的来源数",
  "qoder.page.overview.kpi.domain": "区域域名",
  "qoder.page.overview.kpi.domainHint": "账号库与请求都按它分家",
  "qoder.page.overview.kpi.domainTitle": "该区域的官方域名 —— 双区域是两本互不相通的库",
  "qoder.page.overview.kpi.usage": "今日已记录积分",
  "qoder.page.overview.kpi.usageHint": "本机客户端会话记录（不含其它设备与第三方调用）",
  "qoder.page.overview.kpi.usageTitle":
    "逐日 Token 与额度来自网关请求日志与本地额度缓存，两者尚未落地；未知用「—」而不是 0",
  "qoder.page.overview.clients.title": "本机凭据来源",
  "qoder.page.overview.clients.description": "只读探测，不落库、不改客户端里的任何文件。",
  "qoder.page.overview.clients.action": "去账号页",
  "qoder.page.overview.accounts.title": "账号",
  "qoder.page.overview.accounts.description": "按区域分家的两本账号库之一。",
  "qoder.page.overview.accounts.action": "去账号页",
  "qoder.page.overview.accounts.more": "另有 {n} 个",
  "qoder.page.overview.accounts.empty": "这个区域还没有账号 —— 去账号页从本机导入或粘贴 PAT。",
  "qoder.page.overview.pending.title": "用量与网关尚未落地",
  "qoder.page.overview.pending.description":
    "逐日 Token、额度与请求日志都来自网关；网关未接入前这里不摆空壳 KPI。",
  "qoder.page.overview.pending.action": "去账号页",

  // ---- 设置 ----
  "qoder.page.settings.title": "设置",
  "qoder.page.settings.section.client": "客户端检测",
  "qoder.page.settings.client.description": "本机两处官方凭据存储的探测结果（只读，不落库）。",
  "qoder.page.settings.client.error": "探测失败：{error}",
  // ★ 计划任务（Qoder 的两类排程；排程是全局单份 `schedule_config.json`）
  "qoder.page.settings.schedule.checkinGroup": "自动签到",
  "qoder.page.settings.schedule.checkinEnable": "按排程自动签到",
  "qoder.page.settings.schedule.checkinDesc":
    "到点自动领取活动平台的积分奖励（默认关闭 —— 替你去领东西，需要你明确打开）。",
  "qoder.page.settings.schedule.checkinTimesLabel": "签到时点",
  "qoder.page.settings.schedule.checkinTimesDesc":
    "每天在这些时刻（本地时区）尝试一次；已有随机抖动，不会与官方风控撞在同一秒。",
  "qoder.page.settings.schedule.keepaliveGroup": "令牌保活",
  "qoder.page.settings.schedule.keepaliveEnable": "自动续期 refresh token",
  "qoder.page.settings.schedule.keepaliveDesc":
    "在令牌到期前 48 小时内续期（默认开启：refresh token 的唯一用途就是续期，它本来就是你在网页登录时交出来的）。窗口必须大于巡检间隔（默认每天 1 次），否则每轮巡检都判「还不需要刷」，账号会在两轮之间过期。",
  "qoder.page.settings.schedule.keepaliveTimesLabel": "保活时点",
  "qoder.page.settings.schedule.keepaliveTimesDesc": "每天在这些时刻检查一次是否临近到期。",
  "qoder.page.settings.schedule.saveFailed": "保存计划任务失败",
  "qoder.page.settings.filtered":
    "这里只列已有内容的分区；签到 / 网关 / 权限诊断 / 日志等设置项会随对应功能落地后出现（不预先摆空壳）。",
} as const;
