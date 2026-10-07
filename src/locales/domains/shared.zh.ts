/**
 * 文案域：**跨切面共享文案**（中文，键的权威之一）。
 *
 * 归属文件（`lib/` 与 `stores/` 里的**用户可见**文案，以及两个产品共用的展示组件）：
 * `src/lib/region.ts`、`src/lib/trae-variant-status.ts`、`src/lib/trae-client.ts`、
 * `src/lib/gateway.ts`、`src/lib/trae-gateway.ts`、`src/lib/use-cached-resource.ts`、
 * `src/lib/types.ts`、`src/lib/trae-types.ts`、`src/lib/demo-mode.ts`、
 * `src/lib/update.ts`、`src/lib/clipboard.ts`、`src/lib/stacked-bar-visuals.ts`、
 * `src/stores/resources.ts`、`src/stores/accounts.ts`、`src/stores/gateway.ts`、
 * `src/components/product-marks.tsx`、`src/components/region-bar.tsx`、
 * `src/components/demo-action.tsx`
 *
 * ⚠️ 这些文件大多是**模块级常量表**（如「状态 → 显示标签」的映射）。必须把
 * **键**存进表里（`labelKey: "shared.status.x"`），渲染处再 `t(...)` ——
 * 存中文会让整张表在语言切换时失效（表在模块加载时就定型了）。
 *
 * 键前缀：`shared.`
 */
export const zh = {
  // ---- 版本区域（region.ts / trae-types.ts / trae-variant-status.ts） ----
  "shared.region.version.cn": "国内版",
  "shared.region.version.global": "国际版",
  "shared.region.gateway.cn": "国内版 (WorkBuddy)",
  "shared.region.gateway.global": "国际版 (WorkBuddy AI)",
  "shared.region.filter.all": "合并",

  // ---- 统计范围切换条（region-bar.tsx） ----
  /* P3：`shared.region.scope.aria` 已删除（区域选择器上移到侧栏）。
     页面内只剩「合并」这个**视图参数**开关。 */
  "shared.region.combine.aria": "合并国内版与国际版查看",
  "shared.region.combine.label": "合并国内版与国际版",

  // ---- 网关账号池状态（gateway.ts —— WorkBuddy 与 Trae **共用**同一张卡）----
  // ⚠️ `expired` 只有 Trae 侧会产出：WorkBuddy 池不存积分有效期，没有判据就不展示。
  "shared.poolStatus.available": "可用",
  "shared.poolStatus.cooling": "冷却中",
  "shared.poolStatus.disabled": "会话失效",
  "shared.poolStatus.expired": "积分过期",
  "shared.poolStatus.noCredits": "零积分",

  // ---- 网关账号池卡（components/gateway/account-pool-card.tsx，两侧共用）----
  "shared.gateway.pool.title": "账号池",
  /* P3：接入片段的**字段标签**统一在这里（原来 WB 与 Trae 各有一套）。
     ⚠️ 只有**自然语言标签**放这儿；片段里的键名（`apiProvider` 等）是外部工具读取的
     契约，保持英文硬编码，翻译了会粘贴失败。 */
  /* P11：`snippet.apiBase` / `snippet.apiKey` 两条已删除 —— 用它们的 `cherry` 片段随
     工具清单调整而移除（新工具集见下方 `shared.gateway.access.*`）。`Base URL` / `API Key`
     是通用参数名（英文契约），直接硬编码在片段里，不入词表。 */
  "shared.gateway.snippet.model": "模型",
  /* 模型清单还没加载时用占位 —— **不编造**一个看起来可用的模型名。 */
  "shared.gateway.snippet.modelPlaceholder": "请在模型列表中选择",

  // =====================================================================
  // ---- 接入说明（P11 §1.7）—— 协议 × 工具，两平台共用 ----
  // =====================================================================
  // ⚠️ 片段里的**键名**（`ANTHROPIC_BASE_URL` / `base_url` / `env_key` …）是外部工具读取的
  //    契约，保持英文硬编码；这里只放**自然语言标签**与**协议元数据的展示名**。
  "shared.gateway.access.protocol.openai": "OpenAI 兼容",
  "shared.gateway.access.protocol.anthropic": "Anthropic 兼容",
  "shared.gateway.access.tool.genericOpenai": "其它 OpenAI 兼容工具",
  "shared.gateway.access.tool.genericAnthropic": "其它 Anthropic 兼容工具",
  // 端点说明：两种协议**都已支持**（§3.18.2）；`{openai}` / `{anthropic}` 是**英文端点契约**。
  "shared.gateway.access.endpointNote":
    "协议端点：{openai}（OpenAI）· {anthropic}（Anthropic）—— 两者都已支持，含流式。",
  "shared.gateway.access.authHeaderLabel": "认证头",
  "shared.gateway.access.authNote": "认证头：{header}",
  // Codex（OpenAI 协议）—— 片段里的键名（`base_url` / `env_key`）保持英文。
  "shared.gateway.access.codex.title": "Codex（OpenAI 协议）—— 自定义 model_provider",
  "shared.gateway.access.codex.envKeyNote": "环境变量名；值填你的 Key",
  "shared.gateway.access.codex.docNote": "其余字段以 Codex 官方文档为准",
  // opencode —— 片段里的键名（`baseURL` / `apiKey`）保持英文。
  "shared.gateway.access.opencode.title": "opencode —— provider 配置",
  "shared.gateway.access.opencode.docNote": "字段名以 opencode 官方文档为准",
  // generic 的模型行提示：`{url}` 是**英文端点契约**（`…/v1/models`）。
  "shared.gateway.access.generic.modelHint": "从 GET {url} 自动发现",
  /* 模型清单「来源条」的**共享文案**（components/gateway/catalog-source-bar.tsx，两侧共用壳）。
     ⚠️ 只放**两平台同形**的部分：「来源」标签 / 上次识别 / 计数 / 自动识别提示 / 相对时间单位。
     来源**词汇**与按钮文案由调用方注入（WB 三级 / Trae「客户端缓存 / 未读取」）—— 词汇不同不硬统一。 */
  "shared.gateway.sourceBar.sourceLabel": "来源",
  "shared.gateway.sourceBar.identifiedAt": "上次识别 {time}",
  "shared.gateway.sourceBar.neverIdentified": "尚未识别过",
  /* ⚠️ 计数只说「已识别」，**禁止**「N 个模型可用」—— HTTP 200 ≠ 模型可用（§3.17.3 陷阱 3 / D24）。 */
  "shared.gateway.sourceBar.count": "已识别 {n} 个模型",
  "shared.gateway.sourceBar.autoHint": "接入后自动识别，无需手填模型名",
  "shared.gateway.sourceBar.justNow": "刚刚",
  "shared.gateway.sourceBar.minutesAgo": "{n} 分钟前",
  "shared.gateway.sourceBar.hoursAgo": "{n} 小时前",
  "shared.gateway.sourceBar.daysAgo": "{n} 天前",
  /* 模型表格（P11 §1.6）—— components/gateway/model-table.tsx，**两平台共用**。
     列集 = 模型 / 上游状态 / 网关暴露；**无归属列**（决策 #1）、**无协议列**（决策 #2）。
     ⚠️ 「上游状态」列只可能在 Trae 侧出现（WB 的 `available` 恒 true ⇒ 不渲染该列，决策 #3），
        但文案仍放共享域：它是**共用壳**的一部分，由调用方按 `showUpstream` 决定是否渲染。 */
  "shared.gateway.models.colModel": "模型",
  "shared.gateway.models.colUpstream": "上游状态",
  "shared.gateway.models.colExposed": "网关暴露",
  "shared.gateway.models.upstreamAvailable": "可用",
  "shared.gateway.models.upstreamUnavailable": "不可用",
  "shared.gateway.models.exposed": "已暴露",
  "shared.gateway.models.disabled": "已禁用",
  "shared.gateway.models.disable": "禁用",
  "shared.gateway.models.enable": "启用",
  /* 计数 = `available && !disabled`，与出口 ①（`/v1/models` 过滤）**同源**。 */
  "shared.gateway.models.count": "当前暴露 {served} / {total} 个",
  /* ★ 两个出口同时生效 —— 禁用**必须**两个出口都改，否则是假开关（§3.18.1）：
     只说「已从清单移除」会让用户以为调用还能过；必须把「直接调用也会被拒」一并说清。 */
  "shared.gateway.models.twoExits":
    "禁用的模型不会出现在 `/v1/models`，直接调用也会被拒绝并说明原因 —— 两个出口同时生效，否则禁用就是假开关。",
  "shared.gateway.models.disabledToast": "已禁用「{name}」",
  "shared.gateway.models.enabledToast": "已启用「{name}」",
  "shared.gateway.models.toggleFail": "操作失败",
  "shared.gateway.pool.totalRequests": "累计请求 {count}",
  "shared.gateway.pool.tile.available": "可路由",
  "shared.gateway.pool.tile.cooling": "冷却中",
  "shared.gateway.pool.tile.disabled": "会话失效",
  "shared.gateway.pool.tile.expired": "积分过期",
  "shared.gateway.pool.tile.zeroCredits": "零积分",
  "shared.gateway.pool.empty": "账号池为空。请先在「账号管理」中添加账号。",
  "shared.gateway.pool.credits": "{credits} 积分",
  "shared.gateway.pool.diagnoseNote": "请求报「没有可用账号」时，按下面的原因逐条排查：",
  "shared.gateway.pool.preferredLabel": "指定账号",
  "shared.gateway.pool.preferredAria": "指定优先消耗的账号",
  "shared.gateway.pool.preferredAuto": "不指定（自动择优）",
  // 仅 WorkBuddy：三档策略里「非指定账号」的第二档（写回 `AccountStrategy::MaxCredits`）。
  "shared.gateway.pool.strategyMaxCredits": "按实时积分择优（每次请求查询）",
  "shared.gateway.pool.preferredMissing": "已指定的账号（当前不在池中）",
  // 偏好语义：指定账号不可用时**回落**自动择优，而不是拒绝服务。
  "shared.gateway.pool.preferredHint": "偏好而非强制：指定账号冷却 / 失效 / 零积分时自动改用其他账号，不会拒绝服务。",

  // ---- 剪贴板（clipboard.ts） ----
  "shared.clipboard.copied": "已复制",
  "shared.clipboard.failed": "复制失败，请手动选择后复制",

  // =====================================================================
  // ---- 桌面悬浮窗（components/companion/*，独立窗口）----
  // =====================================================================
  // 悬浮窗是**独立 webview**（`?window=companion`，见 `main.tsx` 的入口分流），
  // 与主窗口共用同一份词表。四态文案 / 只读脚注 / 空态 / 无障碍标签由 T03 补；
  // T01 只放骨架**实际引用**的标题键（本仓没有「未使用键」检查，多余键会静默腐坏）。
  "shared.companion.title": "会话状态",
  // 侧栏左下角入口（`App.tsx` 的 `CompanionToggle`）：tooltip + 无障碍标签。
  // 它与托盘菜单项是**同一开关的两面**（都经 `companion_set_visible` 收敛），故与托盘同名。
  "shared.companion.entry": "会话悬浮窗",
  // 头部徽标：仅当有「待确认」项时出现（计数由快照数据决定，前端不臆断）。
  "shared.companion.badge.waiting": "{count} 待确认",
  // 只读脚注：三件事——只读、点击语义、标题默认隐藏的原因。
  "shared.companion.footnote": "只读 · 点击回到原会话 · 标题悬停显示",
  // 窗口内关闭按钮（关闭 = 隐藏，可从侧栏 / 托盘重开）。
  "shared.companion.close": "隐藏悬浮窗",
  // 四态文案（与后端 `CompanionState` 的 lowercase 序列化一一对应）。
  "shared.companion.state.running": "运行中",
  "shared.companion.state.waiting": "待确认",
  "shared.companion.state.done": "已完成",
  "shared.companion.state.failed": "失败",
  // ★ 无障碍描述：**脱敏**——只报来源 / 状态 / 时间，**不含标题**
  //（标题是敏感字段，默认不进 DOM；不能因为读屏就把明文塞回去，见 P9 ✅ #3）。
  "shared.companion.row.aria": "{source} · {state} · {hint}",
  // 悬停信息卡的两个字段标签（标题本身即信息卡主体，不设标签）。
  "shared.companion.info.source": "来源",
  "shared.companion.info.time": "时间",
  // 点击条目后的**明确反馈**（不静默失败；来源未运行 ⇒ 可读错误）。
  "shared.companion.jump.ok": "已聚焦「{source}」",
  "shared.companion.jump.failed": "无法聚焦来源：{message}",
  // 空态四要素（含出口「刷新」）——只读刷新，与轮询走同一条路径。
  "shared.companion.empty.title": "暂无会话",
  "shared.companion.empty.description": "在客户端发起任务后，状态会显示在这里。",
  "shared.companion.empty.refresh": "刷新",
  // 读取失败的空态（区别于「确实没有会话」）。
  "shared.companion.error.title": "读取会话状态失败",
  // 首次加载的读屏提示（视觉上是骨架屏）。
  "shared.companion.loading": "正在读取会话状态",

  // ---- 演示模式遮罩（demo-action.tsx） ----
  "shared.demo.unavailable": "演示模式下不可操作",

  // ---- HTTP 通道自身抛出的错误（api.ts） ----
  // 这些错误的产生者就是前端（拿不到路由 / 连不上本地服务 / 非 2xx），
  // 后端无从为它们提供码，因此直接走词表。
  "shared.api.unsupportedInWebui": "webui 模式暂不支持该操作: {cmd}",
  "shared.api.unreachable": "无法连接 Buddy Switch 服务（{base}），请先运行 `buddy-switch`",
  "shared.api.requestFailed": "请求失败 ({status})",
  // ---- 权限检测（api.ts） ----
  "shared.api.webuiPermissionByProcess": "webui 模式由服务进程（终端启动）的权限决定，无需额外授权",
  "shared.api.accountNotFound": "未找到账号",

  // =====================================================================
  // ---- 演示夹具（screenshot-demo.ts：演示站 / 截图站的假数据） ----
  // =====================================================================
  // ⚠️ 这些文案**只在演示模式下出现**，但演示站本身就是给人看的一站，
  // 因此同样要中英两版。模型名、协议键、路径、uid 刻意不入表（它们是契约）。
  // ---- 假账号 ----
  "shared.demo.account.a": "测试 A",
  "shared.demo.account.aRemark": "DS4.1 额度 10/03 解禁",
  "shared.demo.account.b": "测试 B",
  "shared.demo.account.c": "测试 C",
  // ---- 积分包名 ----
  "shared.demo.pack.fission": "CodeBuddy 个人版国内运营裂变包",
  "shared.demo.pack.credits": "CodeBuddy 个人版积分包",
  "shared.demo.pack.trial": "CodeBuddy 新用户体验包",
  "shared.demo.pack.checkin": "CodeBuddy 签到赠送积分",
  "shared.demo.pack.activity": "CodeBuddy 活动奖励积分",
  // ---- 自动旅行 ----
  "shared.demo.travel.cafe": "咖啡馆",
  "shared.demo.travel.gym": "健身房",
  // ---- 自动轮换日志 ----
  "shared.demo.rotate.reasonPinned": "当前账号仍是积分到期最紧迫的可用账号",
  "shared.demo.rotate.reasonExpiring": "目标账号积分将在 5 天内到期",
  // ---- Token 统计 ----
  "shared.demo.session.tokenDashboard": "完善 Token 统计仪表盘与本地用量分析",
  "shared.demo.session.agentAcp": "设计 Agent 与 ACP 管理设置",
  "shared.demo.session.characterAudio": "补全角色成语双音频",
  "shared.demo.session.accountCard": "统一账号卡片视觉和交互",
  // P8 T02 · 会话副本视图的演示会话标题（`list_sessions` / `list_session_replicas` 两个 case 共用）。
  "shared.demo.session.replicaAlpha": "重构会话副本视图的归属计算",
  "shared.demo.session.replicaBeta": "统一跨账号会话的复制入口",
  // ---- 模型目录 ----
  "shared.demo.catalog.limitedFree": "限时免费",
  "shared.demo.catalog.promo": "促销",
  "shared.demo.catalog.globalStale": "上游接口可能已变更，当前展示上次成功缓存",
  // ---- 账号策略 ----
  "shared.demo.strategy.realtime": "请求时实时择优",
  // ---- Trae：假账号名 / 分组 / 套餐 / 冷却 ----
  // WorkBuddy 网关账号池的演示数据（`screenshot-demo.ts` 的 `demoGatewayStatus`）。
  "shared.demo.pool.reasonRateLimited": "请求过于频繁",
  "shared.demo.pool.reasonSessionDead": "会话已失效",
  "shared.demo.pool.diagnose": "{name}（{tail}：{reason}，积分={credits}）",
  "shared.demo.pool.creditsUnknown": "未知",

  "shared.demo.trae.name.main": "主号",
  "shared.demo.trae.remark.main": "主号：签到攒额度，别用来跑长任务",
  "shared.demo.trae.name.altA": "小号 A",
  "shared.demo.trae.name.altB": "小号 B",
  "shared.demo.trae.group.spare": "备用",
  "shared.demo.trae.pack.month": "Work 月度包",
  "shared.demo.trae.pack.checkinBonus": "签到赠送包",
  "shared.demo.trae.pack.cnPlan": "国内套餐包",
  "shared.demo.trae.pack.trial": "试用包",
  "shared.demo.trae.cooldown.rateLimited": "请求过于频繁，请稍后再试",
  "shared.demo.trae.cooldown.sessionDead": "会话已失效，需重新登录",
  "shared.demo.trae.cooldown.reasonRateLimited": "请求过于频繁",
  "shared.demo.trae.cooldown.reasonSessionDead": "会话已失效",
  // ---- Trae：签到 ----
  "shared.demo.trae.checkin.ok": "签到成功",
  "shared.demo.trae.checkin.already": "今日已签到",
  "shared.demo.trae.checkin.sessionDead": "会话已失效，需重新登录",
  "shared.demo.trae.warn.altBSessionDead": "小号 B 会话已失效，请重新登录后再签到",
  // ---- Trae：程序位展示名（`label` / `nameAlias`） ----
  "shared.demo.trae.program.work": "TraeWork",
  "shared.demo.trae.program.workCn": "TraeWork CN",
  "shared.demo.trae.program.code": "TraeCode",
  "shared.demo.trae.program.codeCn": "TraeCode CN",
  "shared.demo.trae.program.workAi": "TraeWork AI",
  "shared.demo.trae.program.workGlobal": "TraeWork",
  "shared.demo.trae.program.codeAi": "Trae AI",
  // 「待实测」是**状态标注**，不是品牌名 —— 它是这条目唯一需要翻译的部分。
  "shared.demo.trae.program.codePending": "TraeCode（待实测）",
  // ---- Trae：API Key 名 / 网关诊断 / 日志 ----
  "shared.demo.trae.key.cursorCn": "Cursor (TraeWork)",
  "shared.demo.trae.key.cherryGlobal": "Cherry (TraeWork AI)",
  "shared.demo.trae.key.continueCode": "Continue (TraeCode)",
  "shared.demo.trae.key.legacy": "旧 Key（升级迁移）",
  "shared.demo.trae.diagnose.main": "主号(7481920:可用,积分=120)",
  "shared.demo.trae.diagnose.altA": "小号 A(7481999:冷却中,积分=65)",
  "shared.demo.trae.diagnose.altB": "小号 B(7482044:会话失效（需重新登录）,积分=8)",
  "shared.demo.trae.gatewayUpstreamError": "账号「{name}」上游失败（HTTP {status}）",
  "shared.demo.trae.tokenSource": "Trae API 网关",
  "shared.demo.trae.tokenNote": "只统计经过本机 Trae 网关的调用；直接在 Trae IDE 里对话不产生记录。",
  "shared.demo.trae.logNote": "只读取本机纯文本运行日志（app / checkin / switcher）；网关请求日志在「网关请求日志」标签页。",
  "shared.demo.trae.log.app": "运行",
  "shared.demo.trae.log.checkin": "签到",
  "shared.demo.trae.log.switch": "切换",
  // 运行日志正文（**逐字对齐 `store::append_log` 的真实格式**：演示行与真机行必须同形）
  "shared.demo.trae.log.checkinDone": "签到完成: 成功 {ok}/已签到 {already}/失败 {failed}/总计 {total}",
  "shared.demo.trae.log.thawed": "自动解冻账号 {uid}: 剩余积分 {credits}，冷却已清除",
  "shared.demo.trae.log.savedLogin": "保存登录态: user={uid} 文件数={count}",
  "shared.demo.trae.log.jwtRefreshed": "JWT 自动刷新成功: user={uid} 新到期={hours}h",
  "shared.demo.trae.log.deviceReset": "设备标识重置完成: {count} 项生效",
  // ---- Trae：平台做不到的维度（置灰卡） ----
  "shared.demo.cap.travel": "自动旅行",
  "shared.demo.cap.travelReason": "Trae 客户端没有该活动接口，本工具也无对应后端实现。",
  "shared.demo.cap.cli": "CodeBuddy CLI / IDE 接入",
  "shared.demo.cap.cliReason": "CodeBuddy 属 WorkBuddy 生态，Trae 分区不提供该客户端的接入与切换。",
  "shared.demo.cap.migration": "会话 / 记忆 / 连接器迁移",
  "shared.demo.cap.migrationReason": "Trae 登录态是一组 Cloud-IDE-JWT 文件，没有会话树 / 记忆 / 连接器对象可迁移。",
  "shared.demo.cap.sessionTree": "会话列表 / 复制会话 / 切换进度流",
  "shared.demo.cap.sessionTreeReason": "Trae 的账号切换是文件级快照替换，不存在会话列表与切换进度事件流。",
  "shared.demo.cap.officialByModel": "官方积分消耗按模型",
  "shared.demo.cap.officialByModelReason": "Trae 积分只来自签到快照，不存在「产生这些积分的请求用量」这一口径的数据源。",
  "shared.demo.cap.cacheMetrics": "缓存读取 / 写入 / 命中率",
  "shared.demo.cap.cacheMetricsReason": "Trae 网关日志与上传链路都没有 cache 字段，上游也不回传——无从记录",
  "shared.demo.cap.projectDimension": "按项目维度统计",
  "shared.demo.cap.projectDimensionReason": "网关日志的 project_id / session_id 是每请求新生成的 uuid，不对应客户端项目",
  "shared.demo.cap.sessionCost": "调用最贵的会话",
  "shared.demo.cap.sessionCostReason": "无稳定会话标识，无法把多次请求归并成一个会话成本",
  // ---- 演示模式的写操作应答（DemoAction 拦截后仍要给出「像真的」的回执） ----
  "shared.demo.switchDone": "演示切换已完成",
  "shared.demo.updateTitle": "更新提示演示",
  "shared.demo.error.accountMissing": "账号不存在",
  "shared.demo.error.missingReadOnly": "演示模式缺少只读数据: {command}",
  // ---- 拼接用的标点与分隔符 ----
  // ⚠️ 这些是「语言相关」的排版符号，不能硬编码：中文用全角（：，；（））、
  // 英文用半角（: , ; ( )）。硬编码的症状是「英文界面里冒出中文标点」，
  // 例如 "Primary：已登录"，且不会被「残留中文」扫描抓到（只占一两个字符）。
  "shared.punct.colon": "：",
  "shared.punct.comma": "，",
  "shared.punct.semicolon": "；",
  "shared.punct.period": "。",
  "shared.punct.openParen": "（",
  "shared.punct.closeParen": "）",

  // =====================================================================
  // ---- 账号详情抽屉（components/account-detail-drawer.tsx，两平台共用）----
  // =====================================================================
  // 抽屉是**共享壳**：平台差异由调用方装配成 sections / packs / programs 注入，
  // 因此抽屉自身的文案（标题 / 备注 / 只读程序位 / 加载）放这里，不各写一份。
  "shared.drawer.close": "关闭",
  "shared.drawer.loading": "正在加载详情…",
  // 备注区（就地编辑）—— 语义与卡片时代的备注编辑器一致，只是搬进了抽屉。
  "shared.drawer.remark.title": "备注",
  "shared.drawer.remark.placeholder": "例如：DS4.1 额度 · 10/03 解禁",
  "shared.drawer.remark.aria": "{name} 的备注",
  "shared.drawer.remark.editAria": "编辑 {name} 的备注",
  "shared.drawer.remark.add": "添加备注",
  // 积分 / 资源包区块：文本已由调用方格式化，抽屉只负责排版。
  "shared.drawer.packs.title": "积分 / 资源包",
  // 只读程序位徽标（**不是**切换控件）—— 抽屉里不允许出现第二个「看起来能切」的控件。
  // ★ B5：签到积分（字段来自**上游实测响应**；`+N` 的 N 取 `today_credit` / `daily_credit`）。
  "shared.drawer.checkin.title": "签到",
  "shared.drawer.checkin.today": "今日签到",
  "shared.drawer.checkin.streak": "已连签 {n} 天",
  "shared.drawer.checkin.total": "累计 {n} 积分",
  "shared.drawer.programs.title": "程序位（只读）",
  "shared.drawer.program.current": "当前账号",
  "shared.drawer.program.installed": "已安装",
  "shared.drawer.program.noDataDir": "无数据目录",
  "shared.drawer.program.missing": "未检测到",
  // 详情区块全空时的空态（四要素里的 title / description；出口由调用方以 ReactNode 注入）。
  "shared.drawer.empty.title": "暂无详情",
  "shared.drawer.empty.description": "该账号暂时没有可展示的详情；可重新检测后再试。",

  // =====================================================================
  // ---- 最小批量操作条（components/account-batch-bar.tsx，两平台共用）----
  // =====================================================================
  // §3.3 的「勾选 + 批量」本期只做**最小形态**：计数 + 三个动作 + 清除。
  "shared.batch.selected": "已选 {n} 项",
  "shared.batch.checkin": "批量签到",
  "shared.batch.refresh": "刷新",
  "shared.batch.delete": "删除",
  "shared.batch.clear": "清除选择",
  "shared.batch.deleteTitle": "批量删除账号",
  "shared.batch.deleteDesc": "确定删除选中的 {n} 个账号？删除后可在回收站恢复。",
  "shared.batch.confirm": "删除",
  "shared.batch.cancel": "取消",

  // =====================================================================
  // ---- 账号列表工具栏（components/account-toolbar.tsx，两平台共用）----
  // =====================================================================
  // 工具栏是**共享壳**：四个核心控件（搜索 / 排序 / 分组 / 视图）结构相同，
  // 平台差异由调用方以 options / children 注入，因此壳自身的文案放这里。
  "shared.toolbar.searchPlaceholder": "搜索账号…",
  "shared.toolbar.searchAria": "搜索账号",
  "shared.toolbar.sortAria": "排序方式",
  "shared.toolbar.groupAria": "分组方式",
  "shared.toolbar.viewAria": "切换卡片 / 表格视图",
  "shared.toolbar.viewCard": "卡片",
  "shared.toolbar.viewTable": "表格",
  // 窄窗（< 900px）下表格不可读 ⇒ 按钮**禁用并说明原因**（不是隐藏，隐藏会让用户以为没有表格）。
  "shared.toolbar.viewTableDisabled": "窗口太窄，表格视图不可用；请加宽窗口后重试",
  "shared.toolbar.selectAll": "全选",
  "shared.toolbar.deselectAll": "取消全选",

  // =====================================================================
  // ---- 账号列表空态（两种，绝不混用 —— §3.4 规则 3）----
  // =====================================================================
  // 混用会让用户以为**数据丢了**：明明是「被筛没了」，却显示「还没有账号」。
  "shared.list.empty.none": "还没有账号",
  "shared.list.empty.noneDesc": "添加或从本机导入账号后，即可在这里管理。",
  "shared.list.empty.filtered": "没有符合条件的账号",
  "shared.list.empty.filteredDesc": "换个关键词，或清除筛选后查看全部账号。",
  "shared.list.empty.clearFilters": "清除筛选",

  // =====================================================================
  // ---- 账号表格（components/account-table.tsx，两平台共用）----
  // =====================================================================
  "shared.table.selectAllAria": "全选当前列表",
  "shared.table.selectRowAria": "选择 {name}",
  "shared.table.openRowAria": "打开 {name} 的详情",
} as const;
