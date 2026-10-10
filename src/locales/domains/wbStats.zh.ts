/**
 * 文案域：**WorkBuddy · 统计与 API 网关**（中文，键的权威之一）。
 *
 * 归属文件：
 * `src/pages/TokenStatsPage.tsx`、`src/pages/CreditStatsPage.tsx`、
 * `src/pages/ApiServicePage.tsx`、
 * `src/components/gateway/{model-list,api-key-table,request-log,account-strategy-card,integration-guide}.tsx`
 *
 * 键前缀：`wbStats.`
 */
export const zh = {
  // ---- Token 统计 ----
  "wbStats.token.pageTitle": "Token 统计",
  "wbStats.token.bothRegions": "（国内版 + 国际版）",
  "wbStats.token.refresh": "刷新统计",
  /* P3：`wbStats.token.scopeAria` 已删除 —— 区域选择器上移到侧栏后不再需要（按 zh.ts 约定 3）。 */
  "wbStats.token.loadFailed": "统计加载失败",
  "wbStats.token.retry": "重试",

  // ---- 积分统计 ----
  "wbStats.credit.unnamedResource": "未命名资源包",
  "wbStats.credit.noCurrentResource": "尚未采集当前资源包。",
  // 请求用量明细：表头排序 + 分页
  "wbStats.credit.creditDetailTitle": "积分明细",
  "wbStats.credit.pageTitle": "积分统计",
  /* P3：`wbStats.credit.scopeAria` 已删除 —— 同上。 */
  "wbStats.credit.overviewAria": "积分总览",

  // ---- API 网关 ----
  "wbStats.gateway.saveFail": "保存失败",
  "wbStats.gateway.portRange": "端口需为 1-65535 之间的整数",
  "wbStats.gateway.title": "API 服务",
  "wbStats.gateway.desc": "把 WorkBuddy 的模型额度以 OpenAI / Anthropic 兼容接口提供给本机工具。",
  "wbStats.gateway.opFail": "操作失败",
  "wbStats.gateway.enableGateway": "启用 API 网关",
  "wbStats.gateway.enableDesc": "开启后本机 AI 工具可通过下方地址调用 WorkBuddy 模型。",
  "wbStats.gateway.enableAria": "启用 API 网关",
  "wbStats.gateway.bindAddr": "监听地址",
  "wbStats.gateway.bindAddrAria": "监听地址",
  "wbStats.gateway.loopback": "127.0.0.1（仅本机）",
  "wbStats.gateway.lan": "0.0.0.0（局域网）",
  "wbStats.gateway.port": "监听端口",
  "wbStats.gateway.portAria": "监听端口",
  "wbStats.gateway.status": "状态：",
  "wbStats.gateway.running": "运行中",
  "wbStats.gateway.stopped": "已停止",
  "wbStats.gateway.addr": "接入地址",
  "wbStats.gateway.regionSwitchAria": "切换国内版 / 国际版",
  "wbStats.gateway.copy": "复制",
  "wbStats.gateway.baseUrlCopied": "Base URL 已复制",
  "wbStats.gateway.allowLanTitle": "允许局域网访问",
  "wbStats.gateway.allowLanDesc": "局域网内任何设备都可消耗你的额度，请确认可信网络后再开启。网关仍要求携带有效 API Key。",
  "wbStats.gateway.cancel": "取消",
  "wbStats.gateway.confirmOpen": "确认开启",
  "wbStats.gateway.keyPlaceholder": "sk-wb-…（在下方 Key 列表创建）",
  /* 概览卡三行（原型 viewGateway :3013-3018）：上游凭据校验 / 可用模型 / 账号池。 */
  "wbStats.gateway.upstreamCreds": "上游账号凭据",
  "wbStats.gateway.upstreamCredsDesc": "账号池读取上游凭据的有效性",
  "wbStats.gateway.verify": "校验",
  "wbStats.gateway.verifyOk": "凭据校验通过",
  "wbStats.gateway.verifyFail": "凭据校验未通过",
  "wbStats.gateway.availableModels": "可用模型",
  "wbStats.gateway.modelCount": "{count} 个",
  "wbStats.gateway.poolEntries": "账号池",
  "wbStats.gateway.poolEntryCount": "{count} 个条目",
  "wbStats.gateway.view": "查看",
  "wbStats.gateway.modelList": "模型列表",
  /* 来源徽标（三级）—— `lib/catalog-source.ts` 的 `BADGE_KEY` 逐字引用。
     徽标前的「来源」标签本身走 `shared.gateway.sourceBar.sourceLabel`（两平台同形）。 */
  "wbStats.gateway.sourceLive": "实时",
  "wbStats.gateway.sourceCached": "已保存",
  "wbStats.gateway.sourceBuiltin": "内置",
  /* 非实时来源的**兜底原因**（后端 `note` 缺失时用）—— 契约：`kind !== "live"` ⇒ 原因恒非空。
     `reasonBuiltin` 必须含「尚未成功识别过」：`builtin` 且从未识别成功时，用户最需要知道的是
     「这不是实时清单」，而不是一个灰徽标。 */
  "wbStats.gateway.reasonCached": "本次未拉到，当前展示上次成功识别的清单",
  "wbStats.gateway.reasonBuiltin": "尚未成功识别过（上游未响应）",
  "wbStats.gateway.rescan": "重新识别",
  "wbStats.gateway.scanning": "识别中…",
  "wbStats.gateway.refreshed": "模型列表已刷新",
  "wbStats.gateway.refreshFail": "刷新失败",
  "wbStats.gateway.refresh": "刷新",
  "wbStats.gateway.retry": "重试",
  "wbStats.gateway.lastError": "最近一次错误",
  "wbStats.gateway.openDataDir": "打开数据目录",
  "wbStats.gateway.openDataDirAria": "在文件管理器中打开{version}的 WorkBuddy 数据目录",
  "wbStats.gateway.toast.dirOpened": "已打开 WorkBuddy 数据目录",
  "wbStats.gateway.toast.dirOpenFailed": "打开数据目录失败",
  "wbStats.gateway.toast.unsupported": "当前平台不支持",
  "wbStats.gateway.toast.unsupportedDesc": "该能力仅在 Windows 提供。",
  "wbStats.gateway.noModels": "暂无模型数据。",
  "wbStats.gateway.noModelsDesc": "该版本还没有识别到模型清单；上游未响应或尚未创建 API Key 时都会如此，可点「重新识别」重试。",
  /* P11：`modelTitle` / `free` 两条已删除 —— 模型页从 chips 改为**表格**后不再渲染
     「上下文 / 最大输出」的悬停提示与「免费」徽标（列集固定为 模型 / 网关暴露）。 */
  "wbStats.gateway.createKey": "创建 API Key",
  "wbStats.gateway.noKeys": "尚未创建 API Key。",
  "wbStats.gateway.revoked": "已吊销",
  "wbStats.gateway.enabled": "启用",
  "wbStats.gateway.delete": "删除",
  "wbStats.gateway.revoke": "吊销",
  "wbStats.gateway.nameRequired": "请填写名称",
  "wbStats.gateway.createdNoPlaintext": "创建成功但未返回明文，请重试",
  "wbStats.gateway.createFail": "创建失败",
  "wbStats.gateway.revokedToast": "已吊销",
  "wbStats.gateway.revokeFail": "吊销失败",
  "wbStats.gateway.deletedToast": "已删除",
  "wbStats.gateway.deleteFail": "删除失败",
  "wbStats.gateway.createTitle": "创建 API Key",
  "wbStats.gateway.createDesc": "每个 Key 只能访问其归属版本的模型与账号。",
  "wbStats.gateway.nameLabel": "名称",
  "wbStats.gateway.namePlaceholder": "例如 Cursor",
  "wbStats.gateway.regionLabel": "归属版本",
  "wbStats.gateway.regionAria": "归属版本",
  "wbStats.gateway.create": "创建",
  "wbStats.gateway.createdTitle": "API Key 已创建",
  "wbStats.gateway.createdDesc": "完整 Key 只显示这一次，请立即复制保存。",
  "wbStats.gateway.keyCopied": "API Key 已复制",
  "wbStats.gateway.savedClose": "我已保存，关闭",
  "wbStats.gateway.revokeTitle": "吊销 API Key",
  "wbStats.gateway.revokeDesc": "吊销后「{name}」立即失效（401），列表中保留为「已吊销」状态。",
  "wbStats.gateway.deleteTitle": "删除 API Key",
  "wbStats.gateway.deleteDesc": "确定删除已吊销的「{name}」？此操作不可撤销。",
  "wbStats.gateway.requestLog": "请求日志（最近 50 条）",
  "wbStats.gateway.clear": "清空",
  "wbStats.gateway.cleared": "日志已清空",
  "wbStats.gateway.clearFail": "清空失败",
  "wbStats.gateway.noRequestsLog": "暂无请求记录",
  // 「账号策略」卡已被**两侧共用的账号池卡**取代（策略三档并入卡上的「指定账号」
  // 下拉），下列键随之作废并删除；保存结果的两条仍在使用。
  "wbStats.gateway.strategySaved": "策略已保存",
  "wbStats.gateway.strategySaveFail": "策略保存失败",
  "wbStats.gateway.guide": "接入指引",
  "wbStats.gateway.copyCode": "复制代码",
  "wbStats.gateway.codeCopied": "代码已复制",
  "wbStats.gateway.streamNote": "建议开启流式（stream）；非流式请求会由网关聚合后一次性返回，首字节延迟较长。",
  /* 自动发现说明（P10 §1.7）：只说「客户端会自己读清单」，不承诺清单里的模型一定可用。 */
  "wbStats.gateway.autoDiscover": "客户端会自动从 `/v1/models` 读取模型清单，不需要手填模型名；若你的工具不支持自动发现，从「模型」页复制即可。",
  "wbStats.gateway.noKeyHint": "sk-wb-…（请先在上方创建 Key）",
  /* 「原始请求（调试用）」标签（原型 `:2929`）：下面是可直接粘贴的 curl 片段。 */
  "wbStats.gateway.rawRequest": "原始请求（调试用）",
  // 可复制配置片段里的字段标签（各客户端片段共用；片段本身是给外部工具粘贴的，故键名保持英文习惯）
  /* P3：`wbStats.gateway.snippet.*` 四条已删除 —— 字段标签统一到
     `shared.gateway.snippet.*`（两平台共用）。其中 `modelPick` 的值里还写死过
     「如 GLM-5.3」，合并后 `cursor` 片段改用真实模型名，那条文案随之不再需要。 */

  // ---- 表格表头 ----
  "wbStats.table.name": "名称",
  "wbStats.table.version": "版本",
  "wbStats.table.prefix": "前缀",
  "wbStats.table.createdAt": "创建时间",
  "wbStats.table.status": "状态",
  "wbStats.table.actions": "操作",

  // ---- 积分统计（补全） ----
  "wbStats.credit.bothRegions": "（国内版 + 国际版）",
  "wbStats.credit.refresh": "刷新统计",
  "wbStats.credit.headBarHint": "积分余额来自上游，可手动刷新",
  "wbStats.credit.refreshCredits": "刷新积分",
  "wbStats.credit.loadFailed": "统计加载失败",
  "wbStats.credit.retry": "重试",

  // ---- 用量四视图（P3：趋势 / 模型 / 会话 / 积分）----
  // 四视图标题（UsagePage 子 Tab）
  "wbStats.usage.view.trend": "趋势",
  "wbStats.usage.view.models": "模型",
  "wbStats.usage.view.sessions": "会话",
  "wbStats.usage.view.credits": "积分",

  // 视图参数条（每页唯一：范围 + 合并）
  "wbStats.usage.param.title": "视图参数",
  "wbStats.usage.param.aria": "时间范围",
  "wbStats.usage.range.7": "近 7 天",
  "wbStats.usage.range.14": "近 14 天",
  "wbStats.usage.range.30": "近 30 天",

  // 图表外壳（ChartFrame）
  "wbStats.usage.chart.chart": "图表",
  "wbStats.usage.chart.table": "表格",
  "wbStats.usage.chart.viewToggleAria": "图表与表格切换",
  "wbStats.usage.chart.export": "导出为图片",

  // 趋势视图
  "wbStats.usage.trend.total": "区间总 Token",
  "wbStats.usage.trend.avg": "日均",
  "wbStats.usage.trend.avgHint": "按自然日平均",
  "wbStats.usage.trend.peak": "峰值",
  "wbStats.usage.trend.models": "模型数",
  "wbStats.usage.trend.stackedTitle": "每日 Token · 按模型拆分",
  "wbStats.usage.trend.stackedDesc": "悬停查看当日各模型明细",
  "wbStats.usage.trend.stackedAria": "按模型拆分的每日 Token 堆叠柱状图",
  "wbStats.usage.trend.cumulativeTitle": "累计消耗曲线",
  "wbStats.usage.trend.donutTitle": "模型占比",
  "wbStats.usage.trend.heatmapTitle": "使用时段分布",
  "wbStats.usage.trend.heatmapDesc": "越亮 = 该时段越忙（相对强度，非绝对量）",
  "wbStats.usage.trend.heatmapAria": "一周七天 × 24 小时的使用强度热力图",
  "wbStats.usage.trend.totalSegment": "合计",

  // 模型视图
  "wbStats.usage.models.count": "模型数",
  "wbStats.usage.models.top": "头部模型",
  "wbStats.usage.models.total": "模型总 Token",
  "wbStats.usage.models.avg": "单模型均值",
  "wbStats.usage.models.donutTitle": "模型 Token 占比",
  "wbStats.usage.models.rankTitle": "模型用量排行",
  "wbStats.usage.models.rankDesc": "条形 = 占比 · 迷你线 = 区间内走势",

  // 会话视图（★ 横轴 = 记录数，不是消息数）
  "wbStats.usage.sessions.count": "会话数",
  "wbStats.usage.sessions.total": "会话总 Token",
  "wbStats.usage.sessions.avg": "单会话均值",
  "wbStats.usage.sessions.max": "最高消耗",
  "wbStats.usage.sessions.rankTitle": "会话 Token 排行",
  "wbStats.usage.sessions.scatterTitle": "记录数 × Token",
  "wbStats.usage.sessions.scatterDesc": "气泡越大 = 单次消耗越高",
  "wbStats.usage.sessions.scatterAria": "会话的「记录数 × Token」散点图",
  "wbStats.usage.sessions.unnamed": "未命名会话",
  "wbStats.usage.sessions.colTitle": "会话标题",
  "wbStats.usage.sessions.colRecords": "记录数",
  "wbStats.usage.sessions.colTokens": "Token",
  "wbStats.usage.sessions.colShare": "占比",

  // 积分视图
  "wbStats.usage.credits.available": "可用积分合计",
  "wbStats.usage.credits.in": "区间入账",
  "wbStats.usage.credits.out": "区间消耗",
  "wbStats.usage.credits.expiring": "即将过期",
  "wbStats.usage.credits.dailyTitle": "每日积分消耗",
  "wbStats.usage.credits.dailyAria": "每日积分消耗柱状图",
  "wbStats.usage.credits.balanceTitle": "积分余额变化",
  "wbStats.usage.credits.expiryTitle": "积分到期分布",
  "wbStats.usage.credits.expiryDesc": "未来 30 天",
  "wbStats.usage.credits.expiryAria": "未来 30 天积分到期分布",
  "wbStats.usage.credits.flowTitle": "积分流水",
  "wbStats.usage.credits.colTime": "时间",
  "wbStats.usage.credits.colAccount": "账号",
  "wbStats.usage.credits.colAmount": "变动",
  "wbStats.usage.credits.colNote": "说明",
  "wbStats.usage.credits.checkinSuccess": "签到成功",
  "wbStats.usage.credits.checkinAlready": "已签到",
  "wbStats.usage.credits.checkinError": "签到失败",
  "wbStats.usage.credits.usageNote": "观察到积分消耗",
  "wbStats.usage.credits.unknownAccount": "未知账号",

  // 图注（★ 只由 `lib/chart-caption.ts` 产出）
  "wbStats.usage.caption.cumulative": "区间累计 {total}（单日峰值 {peak}）",
  "wbStats.usage.caption.balance": "{first} → {last}（回推值；基线 {baseline}，非 0）",
  "wbStats.usage.caption.balanceZero": "{first} → {last}（回推值）",
  "wbStats.usage.caption.scatterAxis": "横轴 = 记录数",
  "wbStats.usage.caption.singleSide": "入账明细不可得，仅画消耗侧",

  // 空态（四要素含出口）
  "wbStats.usage.empty.noData": "还没有产生用量",
  "wbStats.usage.empty.noDataHint": "该作用域还没有产生任何调用记录。",
  "wbStats.usage.empty.filtered": "所选范围内没有数据",
  "wbStats.usage.empty.filteredHint": "换一个更大的时间范围试试。",
  "wbStats.usage.empty.viewAll": "看近 30 天",
  "wbStats.usage.empty.sessions": "暂无会话记录",
  "wbStats.usage.empty.sessionsHint": "该作用域还没有产生会话。",
  "wbStats.usage.empty.models": "暂无模型用量",
  "wbStats.usage.empty.modelsHint": "该作用域还没有产生调用记录。",
  "wbStats.usage.empty.credits": "暂无积分数据",
  "wbStats.usage.empty.creditsHint": "首次成功采集积分快照后，这里会开始累计。",
  "wbStats.usage.empty.noSource": "该范围下暂无可用数据来源",
  "wbStats.usage.empty.noSourceHint": "请点击刷新重试。",
  "wbStats.usage.empty.refresh": "刷新",
} as const;
