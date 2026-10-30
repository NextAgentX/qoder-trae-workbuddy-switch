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
  /* 密钥表下方的说明段 —— **三个平台的密钥表共用同一段话**（差异只有「归属」那一列）。
     ★ 抽取理由：改造前这段文案在 WB / Trae / Qoder 三个组件里各写一遍，措辞已经漂移过。 */
  "shared.gateway.keys.note":
    "密钥明文只在创建时展示一次；此处永远只显示掩码。请求数为该密钥创建以来的累计值，与概览的「今日」口径不同。吊销可逆：已吊销的密钥仍留在表里，可「恢复」回启用态（库中存的是同一把 Key，明文未变，客户端无需改配置），也可「删除」清掉记录。",
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
  /* P3：合并态由「开关」改为 `.seg` 双按钮（原型 `viewUsage` 的 params 段）。 */
  "shared.region.combine.only": "仅本作用域",
  "shared.region.combine.all": "合并全部作用域",

  // ---- 网关账号池状态（gateway.ts —— WorkBuddy 与 Trae **共用**同一张卡）----
  // ⚠️ `expired` 只有 Trae 侧会产出：WorkBuddy 池不存积分有效期，没有判据就不展示。
  "shared.poolStatus.available": "可用",
  "shared.poolStatus.cooling": "冷却中",
  "shared.poolStatus.disabled": "会话失效",
  "shared.poolStatus.expired": "积分过期",
  "shared.poolStatus.noCredits": "零积分",
  "shared.poolStatus.excluded": "已排除",

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
  "shared.gateway.sourceBar.yesterday": "昨天",
  "shared.gateway.sourceBar.daysAgo": "{n} 天前",
  /* 模型表格（P11 §1.6）—— components/gateway/model-table.tsx，**两平台共用**。
     列集 = 模型 / 上游状态 / 网关暴露；**无归属列**（决策 #1）、**无协议列**（决策 #2）。
     ⚠️ 「上游状态」列只可能在 Trae 侧出现（WB 的 `available` 恒 true ⇒ 不渲染该列，决策 #3），
        但文案仍放共享域：它是**共用壳**的一部分，由调用方按 `showUpstream` 决定是否渲染。 */
  "shared.gateway.models.colModel": "模型",
  "shared.gateway.models.colContext": "上下文",
  "shared.gateway.models.contextTitle": "上下文上限",
  "shared.gateway.models.contextFixedDesc": "开启：清单与请求使用上游的上限窗口（如 1M）；关闭：回到默认窗口（如 300k）。数字来自官方目录，不是写死的 1M。",
  "shared.gateway.models.contextFromCatalog": "按目录声明",
  "shared.gateway.models.colUpstream": "上游状态",
  "shared.gateway.models.colExposed": "网关暴露",
  /* ★ issue #65：模型池里每个模型的「思考档位 / 积分倍率」两列。
     - 档位空数组有**两种成因**（明确不支持 / 该程序位没下发），文案必须分开，
       否则 TraeWork 侧整列「—」会被读成「这些模型都不能调档」。
     - 倍率是**倍数**不是金额（`×0.2` = 基准价的 0.2 倍），说成「价格」会误导。 */
  "shared.gateway.models.colEfforts": "思考档位",
  "shared.gateway.models.colCredits": "积分倍率",
  "shared.gateway.models.effortDefault": "默认档：{effort}",
  "shared.gateway.models.effortNoneUnknown": "该程序位的清单未下发思考档位信息",
  "shared.gateway.models.effortNoneUnsupported": "该模型不支持选档",
  "shared.gateway.models.effortNoneUndeclared": "上游未声明档位，内置档位表也没有对应条目",
  "shared.gateway.models.effortSourceUpstream": "档位来自上游目录声明",
  "shared.gateway.models.effortSourceStatic": "档位来自网关内置档位表（上游未声明，可能与官方客户端不一致）",
  "shared.gateway.models.creditsFree": "免费",
  "shared.gateway.models.creditsNone": "上游未提供积分倍率",
  "shared.gateway.models.creditsHint": "倍率：按基准价的多少倍计费，不是金额",
  /* ★ issue #101：图片输入能力（三态）。
     「未声明」≠「不支持」—— 上游清单（Trae / Qoder 根本没有这个字段）没说时，
     客户端按自己的配置处理，网关**不替它断言**。三态文案必须分开，否则用户会以为
     网关在说「这个模型不能读图」。 */
  "shared.gateway.models.colImages": "图片输入",
  "shared.gateway.models.imageYes": "支持",
  "shared.gateway.models.imageNo": "不支持",
  "shared.gateway.models.imageUndeclared": "未声明",
  "shared.gateway.models.imageSourceUpstream": "上游目录声明支持图片输入",
  "shared.gateway.models.imageSourceOverride": "你在本页手动指定（覆盖上游声明）",
  "shared.gateway.models.imageUndeclaredHint":
    "上游清单没有声明这个模型能否读图。客户端连上来时默认按「不支持」处理（图片会被它自己挡掉，请求根本不到网关），需要时可在此手动标记。",
  "shared.gateway.models.imageMarkYes": "标记为支持图片",
  "shared.gateway.models.imageMarkNo": "标记为不支持图片",
  "shared.gateway.models.imageReset": "还原上游声明",
  "shared.gateway.models.imageYesToast": "已标记「{name}」支持图片输入",
  "shared.gateway.models.imageNoToast": "已标记「{name}」不支持图片输入",
  "shared.gateway.models.imageResetToast": "已还原「{name}」的图片能力为上游声明",
  "shared.gateway.models.upstreamAvailable": "可用",
  "shared.gateway.models.upstreamUnavailable": "不可用",
  "shared.gateway.models.exposed": "已暴露",
  "shared.gateway.models.disabled": "已禁用",
  "shared.gateway.models.disable": "禁用",
  "shared.gateway.models.enable": "启用",
  /* 模型 ID 复制（2026-10-13）：Codex / Trae 自定义模型 / dsh 自定义 provider 都要求
     **手填模型 id**，而表格此前只把 id 当纯文本渲染 ⇒ 用户只能手抄。
     ⚠️ 复制的是 **id**（上游标识），不是展示名 —— 填展示名不生效。 */
  "shared.gateway.models.copyId": "复制模型 ID",
  "shared.gateway.models.idCopied": "模型 ID 已复制",
  /* 计数 = `available && !disabled`，与出口 ①（`/v1/models` 过滤）**同源**。 */
  "shared.gateway.models.count": "当前暴露 {served} / {total} 个",
  /* ★ 两个出口同时生效 —— 禁用**必须**两个出口都改，否则是假开关（§3.18.1）：
     只说「已从清单移除」会让用户以为调用还能过；必须把「直接调用也会被拒」一并说清。 */
  "shared.gateway.models.twoExits":
    "禁用的模型不会出现在 `/v1/models`，直接调用也会被拒绝并说明原因 —— 两个出口同时生效，否则禁用就是假开关。",
  "shared.gateway.models.disabledToast": "已禁用「{name}」",
  "shared.gateway.models.enabledToast": "已启用「{name}」",
  "shared.gateway.models.toggleFail": "操作失败",
  // ---- 限流探测（components/gateway/model-probe-card.tsx + model-list.tsx 新增列）----
  // 语义红线：只有「模型限流／账号限流／模型封禁」算限流；积分不足 / 登录失效**分列**。
  // ★ 安全审计 S1：notice 恒定可见（探测是真实上游请求，不是本地操作）。
  // ★ 安全审计 S2：reason（上游原文）只许纯文本渲染。
  "shared.gateway.models.colProbe": "限流（探测）",
  // ── 模型降级链（P13）──
  // 文案纪律：只说「明确失败」「不回退」「不把失败改写成成功」，
  // 不用「智能容错」这类让人以为能兜住一切的词。
  "shared.gateway.fallback.title": "模型降级链",
  "shared.gateway.fallback.badgeOff": "未配置",
  "shared.gateway.fallback.badgeOn": "已启用 · {count} 条",
  "shared.gateway.fallback.badgeClosed": "已关闭",
  "shared.gateway.fallback.limit": "同作用域 · 最多 {max} 跳",
  "shared.gateway.fallback.notice": "仅在上游明确失败时触发。已开始向客户端输出的流不会回退，也不会把失败改写成成功 —— 客户端可通过响应头 X-Buddy-Fallback-To 得知实际使用的模型。",
  "shared.gateway.fallback.enable": "启用降级",
  "shared.gateway.fallback.enableHint": "关掉后配置保留，但一次都不触发",
  "shared.gateway.fallback.new": "新建降级链",
  "shared.gateway.fallback.emptyTitle": "还没有降级链",
  "shared.gateway.fallback.emptyDesc": "当某个模型在上游明确失败（整体限流、被封禁、5xx）时，网关可以自动改用你指定的备用模型重发一次，而不是把错误直接抛给客户端。默认关闭，配置后才生效。",
  "shared.gateway.fallback.addTarget": "+ 备选",
  "shared.gateway.fallback.chainOff": "已停用",
  "shared.gateway.fallback.chainDisable": "停用",
  "shared.gateway.fallback.chainEnable": "启用",
  "shared.gateway.fallback.footNote": "状态点读的是实时闸门与冷却（与账号池同源），不是配置快照。降级成功后主模型的闸门不解除 —— 它确实失败了；闸门到期后请求自然回到主模型，无需「回切」开关。",
  "shared.gateway.fallback.diag.gated": "第 {hop} 跳 {model} 正在闸门暂停中（剩 {secs}s）。期间该链会直接跳到下一跳；若都不行，返回原始错误，不会无限等待。",
  "shared.gateway.fallback.diag.disabled": "第 {hop} 跳 {model} 已被禁用，该跳会被跳过。",
  "shared.gateway.fallback.diag.noVision": "第 {hop} 跳 {model} 不支持图片输入，而主模型支持。带图片的请求会跳过这一跳，继续下一个备选 —— 而不是发出一个必然失败的请求。",
  "shared.gateway.fallback.stats": "近 {days} 天：降级 {total} 次（成功 {ok} · 失败 {fail}）",
  "shared.gateway.fallback.events": "降级流水",
  "shared.gateway.fallback.eventsHide": "收起流水",
  "shared.gateway.fallback.expand": "展开",
  "shared.gateway.fallback.collapse": "收起",
  "shared.gateway.fallback.event.time": "时间",
  "shared.gateway.fallback.event.path": "实际路径",
  "shared.gateway.fallback.event.reason": "触发原因",
  "shared.gateway.fallback.event.account": "账号",
  "shared.gateway.fallback.event.cost": "耗时",
  "shared.gateway.fallback.event.result": "结果",
  "shared.gateway.fallback.event.degraded": "已降级",
  "shared.gateway.fallback.event.failed": "备选也失败",
  "shared.gateway.fallback.event.blocked": "按规则不降级",
  "shared.gateway.fallback.event.empty": "最近还没有降级事件。",
  "shared.gateway.fallback.event.note": "只有最近 {count} 条，且不参与任何统计口径：请求日志是环形缓冲（默认 200 条），繁忙时只覆盖十几分钟。需要「近 7 天降级率」时必须从账本另开一个维度，不能拿这张表算。",
  "shared.gateway.fallback.drawer.new": "新建降级链",
  "shared.gateway.fallback.drawer.edit": "编辑降级链 · {model}",
  "shared.gateway.fallback.field.from": "主模型",
  "shared.gateway.fallback.field.fromLocked": "主模型不可改。要换主模型请删掉这条链重配 —— 改 from 等于换了一条链。",
  "shared.gateway.fallback.field.crossModelPlaceholder": "填目标平台的模型 id",
  "shared.gateway.fallback.field.thisScope": "本区域",
  "shared.gateway.fallback.field.crossPlatformPending": "跨平台仅 WorkBuddy 支持",
  "shared.gateway.fallback.field.targets": "备选模型（按顺序尝试）",
  "shared.gateway.fallback.field.targetsHint": "最多 {max} 个（含主模型共 {hops} 跳）。已禁用 / 上游不可用的模型不会出现在这里。",
  "shared.gateway.fallback.capsNote": "带图片的请求会自动跳过不支持图片输入的备选；工具调用 / 结构化输出 / 上下文窗口本版不校验，由备选自己决定接受还是拒绝。",
  "shared.gateway.fallback.field.triggers": "触发条件",
  "shared.gateway.fallback.trigger.gate": "模型闸门暂停",
  "shared.gateway.fallback.trigger.gateDesc": "该模型近期在多个账号上失败、已被网关整体暂停。零成本直接跳下一跳。",
  "shared.gateway.fallback.trigger.model_rate": "模型级限流（6004）",
  "shared.gateway.fallback.trigger.model_rateDesc": "上游明确表示是这个模型被限，换号无用。",
  "shared.gateway.fallback.trigger.model_blocked": "模型被封禁（11102）",
  "shared.gateway.fallback.trigger.model_blockedDesc": "上游对该模型返回封禁；短时间不会恢复。",
  "shared.gateway.fallback.trigger.server": "上游 5xx",
  "shared.gateway.fallback.trigger.serverDesc": "服务端故障。开启后故障期间流量会涌向备选，可能把备选也打挂。",
  "shared.gateway.fallback.trigger.timeout": "请求超时",
  "shared.gateway.fallback.trigger.timeoutDesc": "超过 600s 未响应。开启后长任务可能被重复计费。",
  // 卡片上的紧凑标签（只标「超出基线」的增量，完整清单进 title）。
  "shared.gateway.fallback.trigger.not_found": "模型不存在（404）",
  "shared.gateway.fallback.trigger.not_foundDesc": "上游说这个模型不存在：可能是该账号的清单里没有它（换号有用），也可能是上游整体下线了（换模型有用）。",
  "shared.gateway.fallback.short.gate": "闸门",
  "shared.gateway.fallback.short.model_rate": "6004",
  "shared.gateway.fallback.short.model_blocked": "11102",
  "shared.gateway.fallback.short.server": "5xx",
  "shared.gateway.fallback.short.timeout": "超时",
  "shared.gateway.fallback.short.not_found": "404",
  "shared.gateway.fallback.never.title": "固定不降级",
  "shared.gateway.fallback.never.desc": "参数错误 4xx：换模型救不了，只会把同一个错误翻译成更陌生的模型名；内容审核拒绝：换模型等于换一套审核尺度，效果上就是帮调用方绕过审核；402 余额不足：账号额度问题，换模型同样 402；Key 配额用尽：按 Key 计量，与模型无关。",
  "shared.gateway.fallback.sim.title": "模拟路径 · 按当前闸门与冷却预演",
  "shared.gateway.fallback.sim.primary": "主模型",
  "shared.gateway.fallback.sim.target": "备选 {n}",
  "shared.gateway.fallback.sim.try": "先试它",
  "shared.gateway.fallback.sim.willUse": "可用 → 本次会用它",
  "shared.gateway.fallback.sim.skipGated": "仍在暂停 → 跳过，不发请求",
  "shared.gateway.fallback.sim.skipDisabled": "不可用 → 跳过",
  "shared.gateway.fallback.sim.allFail": "若所有跳都不可用，返回的是原始错误，不会无限等待，也不会把失败改写成成功。",
  "shared.gateway.fallback.save": "保存",
  "shared.gateway.fallback.cancel": "取消",
  "shared.gateway.fallback.saveOk": "降级链已保存",
  "shared.gateway.fallback.saveOkDesc": "立即生效，无需重启网关",
  "shared.gateway.fallback.needTarget": "至少选一个备选模型",
  "shared.gateway.fallback.needTargetDesc": "只配主模型不构成降级链",
  "shared.gateway.fallback.enabledToast": "已启用降级",
  "shared.gateway.fallback.disabledToast": "已关闭降级",
  "shared.gateway.probe.title": "限流探测",
  "shared.gateway.probe.badge": "账号 × 模型 实探",
  "shared.gateway.probe.expand": "展开",
  "shared.gateway.probe.collapse": "收起",
  "shared.gateway.probe.summaryAvailable": "{accounts} 账号 · {models} 模型可测",
  "shared.gateway.probe.summaryLast": "上次 {time} · 回填 +{added}/-{cleared}",
  // 不支持回填的平台（Trae）：头部**不提回填**，避免「回填 +0/-0」被读成「回填跑了但没结果」。
  "shared.gateway.probe.summaryLastPlain": "上次 {time}",
  // 本次**刻意没回填**（账号页发起的查证 / 用户关了开关）：显式说明，别让「置 0 / 清 0」被误读。
  "shared.gateway.probe.summaryLastNoAdopt": "上次 {time} · 未回填",
  "shared.gateway.probe.notice":
    "将向每个账号各发一次最小请求，消耗极少积分、会在上游留痕，可能触发风控。",
  "shared.gateway.probe.stepModels": "要测哪些模型",
  "shared.gateway.probe.stepAccounts": "在这些账号上测",
  "shared.gateway.probe.selected": "已选 {n}/{total}",
  "shared.gateway.probe.selectAll": "全选",
  "shared.gateway.probe.clearAll": "清空",
  "shared.gateway.probe.noModels": "当前作用域暂无可测模型，请先识别模型清单。",
  "shared.gateway.probe.noAccounts": "当前作用域账号池为空，请先添加账号。",
  "shared.gateway.probe.adoptTitle": "把结果回填账号池",
  "shared.gateway.probe.adoptDesc":
    "可用 ⇒ 清掉该模型的冷却；限流／封禁 ⇒ 置该模型的冷却。只动冷却，不推进退避计数。",
  "shared.gateway.probe.adoptAria": "回填账号池",
  // ★ Trae（T1）：Trae 池**没有「模型级冷却」** ⇒ 回填只能写账号级冷却、会误伤同账号其它模型
  //   （方案 §4.5.4 的 T3，明确不建议）。故开关**禁用**并给出这条原因文案。
  "shared.gateway.probe.adoptUnsupportedTitle": "本平台不支持回填",
  "shared.gateway.probe.adoptUnsupportedDesc":
    "Trae 账号池没有「模型级冷却」：回填只能写账号级冷却，会把该账号的其它模型一起冷却。本页只呈现探测结果，不改动账号池。",
  "shared.gateway.probe.footCells": "{accounts} 账号 × {models} 模型 = {cells} 格 · 消耗极少积分",
  "shared.gateway.probe.pickModelFirst": "请先选择要测试的模型",
  "shared.gateway.probe.start": "开始探测",
  "shared.gateway.probe.startFail": "启动探测失败",
  "shared.gateway.probe.cancel": "取消剩余",
  "shared.gateway.probe.adoptOn": "结束后自动回填池冷却",
  "shared.gateway.probe.adoptOff": "不回填（只出报告）",
  "shared.gateway.probe.runningCurrent": "正在探测：{current}",
  "shared.gateway.probe.runningNote":
    "账号内串行（连发会把好号探成 429）· 跨账号并发 2 · 格间 150ms · 零重试。探测自身 429 是信号不是失败。",
  "shared.gateway.probe.doneCells": "共 {n} 格",
  "shared.gateway.probe.done": "已完成",
  "shared.gateway.probe.cancelled": "已取消剩余（下面是部分结果）",
  "shared.gateway.probe.adoptResult": "回填 置 {added} / 清 {cleared}",
  // issue #88：探出限流却「回填 0 条」是最需要解释的组合，旧实现一个字都不渲染。
  "shared.gateway.probe.adoptNone":
    "本次判出 {n} 格限流，但没有写入任何冷却：这些账号可能已不在池中、已被禁用，或已有更晚的冷却（不会被缩短）。",
  // 回填明细（§5.3「回填反馈」完成条 → 可展开）；账号 / 模型 / 置或清 / 到什么时候。
  "shared.gateway.probe.adoptedDetail": "已回填 {n} 条冷却 · 看明细",
  "shared.gateway.probe.adoptedAdded": "置冷却",
  "shared.gateway.probe.adoptedCleared": "已清除",
  "shared.gateway.probe.adoptedUntil": "至 {time}",
  "shared.gateway.probe.legendUnknown": "未探测",
  // 被动已知按**来源**分两条（§5.3 角标「真实流量」/「客户端日志」—— 二者必须能区分）。
  "shared.gateway.probe.legendPassiveFlow": "被动已知 · 真实流量",
  "shared.gateway.probe.legendPassiveLedger": "被动已知 · 客户端日志",
  "shared.gateway.probe.matrixEmpty": "没有可探测的组合：请至少各选一个账号与模型。",
  "shared.gateway.probe.colAccount": "账号 \\ 模型",
  "shared.gateway.probe.sumLimit": "限流数",
  "shared.gateway.probe.rowLimitAccounts": "限流账号数",
  "shared.gateway.probe.resultProbed": "本次实测命中 {probed} 格限流／封禁。",
  "shared.gateway.probe.resultPassive": "另有 {passive} 格为被动已知（不是这次探出来的）。",
  "shared.gateway.probe.resultHint":
    "虚线 = 尚未探测；带内描边 = 被动已知（真实流量／客户端台账）。只有「模型限流／账号限流／模型封禁」算限流，402 积分不足与登录失效已分列。",
  "shared.gateway.probe.exportCsv": "导出 CSV",
  "shared.gateway.probe.exportDone": "已导出探测结果",
  "shared.gateway.probe.exportRows": "{n} 行",
  "shared.gateway.probe.retry": "重探",
  /* 单元格详情里的单格入口：`uids:[uid] models:[model]`（恒 1 格）、`adopt:false`（查证不写池）。 */
  "shared.gateway.probe.probeCell": "实测这一格",
  "shared.gateway.probe.reprobeCell": "重测这一格",
  "shared.gateway.probe.probedAt": "探测于 {time}",
  "shared.gateway.probe.cellTitle": "{account} · {model}\n状态：{state}\n来源：{source}\n{reason}",
  "shared.gateway.probe.sourceProbed": "本次探测",
  // 被动底色的**两个来源**必须能区分：flow = 池冷却（真实流量）；ledger = 客户端台账（客户端日志）。
  "shared.gateway.probe.sourcePassiveFlow": "被动底色（真实流量）",
  "shared.gateway.probe.sourcePassiveLedger": "被动底色（客户端日志）",
  "shared.gateway.probe.sourcePassiveBoth": "被动底色（真实流量 + 客户端日志，以真实流量为准）",
  "shared.gateway.probe.sourceUnknown": "尚未探测",
  "shared.gateway.probe.csv.account": "账号",
  "shared.gateway.probe.csv.model": "模型",
  "shared.gateway.probe.csv.state": "状态",
  "shared.gateway.probe.csv.source": "来源",
  "shared.gateway.probe.csv.note": "说明",
  // 状态标签（与后端 ProbeState 的 snake_case 序列化名逐字对应）。
  "shared.gateway.probe.state.ok": "可用",
  "shared.gateway.probe.state.rate_limited_model": "模型限流",
  "shared.gateway.probe.state.rate_limited_account": "账号限流",
  "shared.gateway.probe.state.model_blocked": "模型封禁",
  "shared.gateway.probe.state.no_credits": "积分不足",
  "shared.gateway.probe.state.auth_dead": "登录失效",
  "shared.gateway.probe.state.model_unavailable": "无此模型",
  "shared.gateway.probe.state.upstream_error": "上游异常",
  "shared.gateway.probe.state.upstream_queued": "上游排队",
  "shared.gateway.probe.state.transport_error": "网络错误",
  "shared.gateway.probe.state.skipped": "跳过",
  "shared.gateway.probe.state.unknown": "未探测",
  // 状态说明（悬停详情；纯文本）。
  "shared.gateway.probe.reason.ok": "上游 200，可正常调用",
  "shared.gateway.probe.reason.rate_limited_model": "上游 429 · 6004／IsModelRateLimit ⇒ 只封锁该模型，换其它模型可用",
  "shared.gateway.probe.reason.rate_limited_account": "上游 429 且无模型标记 ⇒ 该账号所有模型暂不可用",
  "shared.gateway.probe.reason.model_blocked": "上游 11102 ⇒ 该模型对此账号被封禁，退避逐次加倍（6h→12h→24h）",
  "shared.gateway.probe.reason.no_credits": "上游 402 ⇒ 积分不足（不是限流！等重置没用，要补积分）",
  "shared.gateway.probe.reason.auth_dead": "上游 12153／401 ⇒ 登录态失效，需重新登录或导入",
  "shared.gateway.probe.reason.model_unavailable": "上游 404 ⇒ 该账号看不到／不支持这个模型",
  "shared.gateway.probe.reason.upstream_error": "上游 5xx／4xx ⇒ 与账号无关",
  "shared.gateway.probe.reason.upstream_queued": "上游排队（10605／isQueued）⇒ 该模型此刻在排队，等一下就好 —— 不是限流、也不是账号问题",
  "shared.gateway.probe.reason.transport_error": "连接失败 ⇒ 网络层问题，不是账号问题",
  "shared.gateway.probe.reason.skipped": "未探测：无凭据（信封）／账号未登录／已排除",
  "shared.gateway.probe.reason.unknown": "尚未探测（虚线 = 未探测；带内描边 = 被动已知）",
  // 「限流（探测）」列的单元格（点徽章 = 下钻）。
  "shared.gateway.probe.colAllOk": "全部可用",
  "shared.gateway.probe.colLimited": "{limited}/{total} 限流",
  "shared.gateway.probe.colUnavailable": "{unavailable}/{total} 不可用",
  "shared.gateway.probe.colPassive": "被动 {n}",
  "shared.gateway.probe.colNone": "—",
  "shared.gateway.probe.colTooltipAllOk": "本次实测：{total} 个账号全部可用 —— 点击看矩阵",
  "shared.gateway.probe.colTooltipLimited": "本次实测：{total} 个账号中 {limited} 个被限流／封禁 —— 点击看矩阵",
  "shared.gateway.probe.colTooltipUnavailable": "本次实测：{total} 个账号中 {unavailable} 个当前不可用（上游排队／上游异常等，非限流）—— 点击看矩阵",
  "shared.gateway.probe.colTooltipPassive": "被动已知：{flow} 个来自真实流量、{ledger} 个来自客户端日志（共 {n} 个账号）—— 尚未主动探测，点击去探测",
  "shared.gateway.probe.colTooltipNone": "尚未探测这个模型 —— 在上方「限流探测」卡里选它",
  // 排序（§5.3：可点表头排序，默认按「限流数」降序；用 Radix Select 不用 details/summary）。
  "shared.gateway.probe.sortLabel": "排序",
  "shared.gateway.probe.sortAria": "矩阵排序方式",
  "shared.gateway.probe.sortThrottle": "按限流账号数",
  "shared.gateway.probe.sortName": "按模型名",
  // 详情气泡（§5.3：点单元格 → 状态 / HTTP / 上游原文 / resetAt / 耗时 + 「所以我能怎么办」）。
  "shared.gateway.probe.detailSource": "来源",
  "shared.gateway.probe.detailHttp": "HTTP",
  "shared.gateway.probe.detailLatency": "耗时",
  "shared.gateway.probe.detailReset": "重置时刻",
  "shared.gateway.probe.detailReason": "上游原文",
  // 空态（§5.3：全 ok ⇒「全部可用」；全 skipped ⇒ 去登录 / 导入账号指引）。
  "shared.gateway.probe.emptyAllOk": "全部可用：本次实测的所有格都通过了（上游 200），当前没有限流。",
  "shared.gateway.probe.emptyAllSkippedTitle": "全部跳过：没有任何账号发出请求",
  "shared.gateway.probe.emptyAllSkippedDesc":
    "这些账号都没有可用凭据——可能没登录、凭据是加密信封、或已被移出账号库。请到「账号」页重新登录或导入账号；若本机没有对应桌面客户端（Docker / headless），请在「网关 → 账号池」用扫码方式添加；若用的是「指定账号」策略，请确认该账号仍在本机账号库中。",
  "shared.gateway.pool.totalRequests": "累计请求 {count}",
  // ★ issue #129：账号池顶部「账号故障时自动切换」开关。
  "shared.gateway.pool.autoFailover": "账号故障时自动切换",
  "shared.gateway.pool.autoFailoverHint":
    "开启后，一个账号在回复中途挂掉时，网关会把这次失败变成「可重试错误」，客户端自动换号重跑整个步骤，你看到的是从头完整重写的回复。代价：这一次回答的已输出部分作废、会再消耗一次额度。关闭时保持原有行为（上游报错按原样透出）。默认关闭：它改变对外报错方式，属于行为变更。",
  "shared.gateway.pool.tile.available": "可路由",
  // 「可路由」格里的小字注：其中几个只是**并发占满**（账号正常、几秒自愈）。
  // 不做第七格的理由见 `GatewayPoolSummary.inFlightFull` 的注释 —— 瞬时态与故障态不能并排。
  "shared.gateway.pool.tile.availableBusy": "其中 {count} 个在途已满",
  "shared.gateway.pool.tile.availableBusyHint":
    "这些账号状态正常，只是并发额度占满（在途请求还没结束）；几秒后自动恢复，无需任何操作。",
  "shared.gateway.pool.tile.cooling": "冷却中",
  "shared.gateway.pool.tile.disabled": "会话失效",
  "shared.gateway.pool.tile.expired": "积分过期",
  "shared.gateway.pool.tile.zeroCredits": "零积分",
  "shared.gateway.pool.tile.excluded": "已排除",
  // Qoder 独有的一格：上游 429 按**模型**报 ⇒ 账号本身可用、但有模型级冷却。
  "shared.gateway.pool.tile.modelCooling": "模型冷却",
  // 模型冷却徽标（issue #88）：**必须带解冻时刻**，否则「N 个模型限流」等于没说。
  //
  // ★ 2026-10-29：冷却已经按上游声明的时刻走（`Pool::adopt_probe_result`），所以默认就是
  //   「{time} 解冻」。只有**上游什么都没说**、走本仓兜底时才是「约 … 解冻」——
  //   那个时刻是猜的，加「约」是为了不让人拿它当承诺。
  "shared.gateway.pool.modelCoolingBadge": "{count} 个模型限流 · {time} 解冻",
  "shared.gateway.pool.modelCoolingBadgeApprox": "{count} 个模型限流 · 约 {time} 解冻",
  "shared.gateway.pool.modelCoolingHint":
    "{models} 处于模型级冷却（只影响这些模型，其它模型仍可用），最早 {time} 解冻。",
  // 徽标 tooltip 的逐模型行（issue #88 缺口 4.2）：名称 · 剩余 · 是否兜底 · 上游声明时刻。
  "shared.gateway.pool.modelCoolingRow": "{model} · 剩 {remaining}{fallback}{upstream}",
  // 上游**文案**里声明的恢复时刻（只在与实际冷却截止不同时出现 —— 例如被 24h 截断）。
  "shared.gateway.pool.modelCoolingRowUpstream": " · 上游称 {time} 重置",
  // 「兜底」= 上游没给墙钟，本仓按 probe_fallback_ms 猜的（默认 10 分钟）。
  "shared.gateway.pool.modelCoolingFallback": "（兜底）",
  "shared.gateway.pool.empty": "账号池为空。请先在「账号管理」中添加账号。",
  "shared.gateway.pool.credits": "{credits} 积分",
  "shared.gateway.pool.diagnoseNote": "请求报「没有可用账号」时，按下面的原因逐条排查：",
  "shared.gateway.pool.preferredLabel": "指定账号",
  "shared.gateway.pool.preferredAria": "指定优先消耗的账号",
  "shared.gateway.pool.preferredAuto": "不指定（自动择优）",
  // 仅 WorkBuddy：三档策略里「非指定账号」的第二档（写回 `AccountStrategy::MaxCredits`）。
  "shared.gateway.pool.strategyMaxCredits": "按实时积分择优（每次请求查询）",
  "shared.gateway.pool.strategyRandom": "随机（在合格账号里均匀抽签）",
  // issue #102：粘住当前账号、限流才换 —— 目的是复用上游前缀缓存，降低 token 消耗。
  "shared.gateway.pool.strategySequential": "顺序（固定一个号，限流才换）",
  "shared.gateway.pool.preferredMissing": "已指定的账号（当前不在池中）",
  // 偏好语义：指定账号不可用时**回落**自动择优，而不是拒绝服务。
  "shared.gateway.pool.preferredHint": "偏好而非强制：指定账号冷却 / 失效 / 零积分时自动改用其他账号，不会拒绝服务。",
  // 表格列与「指定」列（原型 viewGateway :3057-3069：账号 / 积分 / 状态 / 选号）。
  "shared.gateway.pool.colAccount": "账号",
  "shared.gateway.pool.colCredits": "积分",
  "shared.gateway.pool.colStatus": "状态",
  "shared.gateway.pool.colPick": "选号",
  "shared.gateway.pool.pin": "指定",
  "shared.gateway.pool.unpin": "取消指定",
  "shared.gateway.pool.pinned": "已指定",
  "shared.gateway.pool.revive": "恢复",
  // 「恢复」出口的结果文案 —— **两平台共用**（同一件事同一做法）：WorkBuddy 与 Trae
  // 的禁用都是终态，都要靠这个出口解除。放在 shared 而不是各自的页面命名空间。
  "shared.gateway.pool.reviveDone": "已恢复：该账号重新参与选号",
  "shared.gateway.pool.reviveNoop": "该账号本来可用，无需恢复",
  "shared.gateway.pool.reviveFail": "恢复失败",
  // 选号优先级与排他性说明（原型 :3069）。
  "shared.gateway.pool.exclusiveHint": "选号优先级：显式指定 > 粘性会话 > 自动择优。指定是排他的——同一时刻只有一个账号被指定。",
  // 「恢复」出口说明：禁用是终态，没有出口该账号就永久不可用。
  // ⚠️ 文案**不得**提「熔断」：Trae 侧没有熔断这一维，两边共用同一句。
  "shared.gateway.pool.exclude": "排除",
  "shared.gateway.pool.unexclude": "取消排除",
  "shared.gateway.pool.excludeDone": "已排除该账号",
  "shared.gateway.pool.unexcludeDone": "已取消排除",
  "shared.gateway.pool.excludeFail": "排除失败",
  "shared.gateway.pool.excludeHint": "「排除」= 网关不再用这个账号（自动选号、兜底、粘性、指定全都跳过），但它**不是禁用** —— 账号本身仍可在客户端正常使用。适合「把额度留给客户端自己用」这类场景。",
  "shared.gateway.pool.reviveHint": "已禁用的账号不会自动解除——点「恢复」会立即解除禁用与冷却，让它重新参与选号；若它仍然不可用，会再次被禁用。",

  // ---- 剪贴板（clipboard.ts） ----
  "shared.clipboard.copied": "已复制",
  "shared.clipboard.failed": "复制失败，请手动选择后复制",

  // =====================================================================
  // ---- 桌面悬浮窗（components/companion/，独立窗口）----
  // ⚠️ 这里**不要**写 `companion/*` 这种通配：`check-scope-registry.cjs` 的 `stripComments`
  //   是**朴素正则**（`/\/\*[\s\S]*?\*\//`），行注释里的 `/*` 会被当成块注释起点，
  //   一直吞到文件里**下一个 `*/`** —— 实测因此丢掉过 250 行词条（门禁报「词条缺 …」）。
  //   词条文件里任何注释都别出现裸 `/*`。
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
  // 来源 id → 展示名（id 是**后端数据**，见 companion-row 的 SOURCE_LABEL_KEY）。
  // ★ 必须覆盖 `agent_watch::HOOK_SOURCES` 的每一项，门禁 `check-agent-sources.cjs` 会校验。
  "shared.companion.source.workbuddyCn": "WorkBuddy 国内版",
  "shared.companion.source.workbuddyGlobal": "WorkBuddy 国际版",
  "shared.companion.source.claude": "Claude Code",
  "shared.companion.source.codebuddy": "CodeBuddy",
  "shared.companion.source.codex": "Codex",
  "shared.companion.source.traeCn": "Trae 国内版",
  "shared.companion.source.traeGlobal": "Trae 国际版",
  "shared.companion.source.qoderCn": "Qoder 国内版",
  "shared.companion.source.qoderGlobal": "Qoder 国际版",
  // 本工具自己的长操作（签到等，`agent_watch::SOURCE_THIS_TOOL`）—— 与客户端来源分开。
  "shared.companion.source.thisTool": "本工具",
  // 会话没有标题时的兜底（客户端只在部分事件里带标题，空标题是常态）。
  "shared.companion.untitled": "未命名会话",
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
  // ⚠️ 这句话必须与**实际数据源**相符：窗口只显示「切号进度」与「已开启监听的客户端会话」，
  //    它**不会**自动读取客户端里的历史任务。此前这里写「在客户端发起任务后，状态会显示
  //    在这里」，而当时的实现里客户端任务根本进不来（provider 只读 `SWITCH_RUNNING`）——
  //    那是一句**没实现就写下的承诺**，照着它排查会白费功夫。
  "shared.companion.empty.description":
    "这里显示正在进行的账号切换，以及在「设置 → 客户端」开启会话监听后、客户端里发起的任务。",
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
  // ★ 2026-10-12：演示账号名与原型 `SCOPES["wb-cn"].accounts`（`buddy-switch-prototype.html:1229`）
  //   **逐一对应** ⇒ 演示站的账号页可以与原型逐卡对照（1:1 核验）。
  //   ⚠️ 这些是**用户名（数据）**，中英两语言下写法相同 —— **不翻译**。
  "shared.demo.account.a": "jackey",
  "shared.demo.account.aRemark": "主号 · 长期",
  "shared.demo.account.b": "team-shared",
  "shared.demo.account.c": "backup-01",
  "shared.demo.account.d": "qa-bot",
  "shared.demo.account.e": "design-lab",
  "shared.demo.account.f": "ops-02",
  "shared.demo.account.g": "spare-global",
  "shared.demo.account.h": "demo",
  // ---- 积分包名 ----
  "shared.demo.pack.fission": "CodeBuddy 个人版国内运营裂变包",
  "shared.demo.pack.credits": "CodeBuddy 个人版积分包",
  "shared.demo.pack.trial": "CodeBuddy 新用户体验包",
  "shared.demo.pack.checkin": "CodeBuddy 签到赠送积分",
  "shared.demo.pack.activity": "CodeBuddy 活动奖励积分",
  // ---- 自动旅行 ----
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

  // ★ 2026-10-12：Trae 演示账号名与原型 `SCOPES["trae-cn"].accounts`（`buddy-switch-prototype.html:1292`）
  //   **逐账号对应** ⇒ 演示站可与原型逐卡对照。⚠️ 用户名是**数据**，中英两语言写法相同 —— 不翻译。
  "shared.demo.trae.name.jackey": "jackey",
  "shared.demo.trae.remark.jackey": "TraeWork 主号",
  "shared.demo.trae.name.codeLab": "code-lab",
  "shared.demo.trae.name.soloTest": "solo-test",
  "shared.demo.trae.name.idle": "idle-01",
  "shared.demo.trae.name.snap": "snap-acc",
  "shared.demo.trae.group.common": "常用",
  "shared.demo.trae.group.test": "测试",
  "shared.demo.trae.group.idle": "闲置",
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
  // 合并后的客户端（issue #20）：客户端自己就叫 `Trae`。
  "shared.demo.trae.program.trae": "Trae CN",
  "shared.demo.trae.program.traeGlobalLabel": "Trae",
  // 「待实测」是**状态标注**，不是品牌名 —— 它是这条目唯一需要翻译的部分。
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
  // ---- Qoder：演示账号（只读两条；名字是**数据**，中英同形） ----
  "shared.demo.qoder.name.jackey": "jackey",
  "shared.demo.qoder.name.team": "team-qoder",
  "shared.demo.qoder.name.intl": "jackey-global",
  "shared.demo.qoder.name.trial": "trial-01",
  "shared.demo.qoder.remark.main": "主号 · 长期",
  "shared.demo.qoder.remark.team": "团队共用",
  "shared.demo.qoder.remark.intl": "国际版主号",
  // ---- Qoder：网关演示（池状态原因 / 清单来源 / 日志错误 / Key 名） ----
  // 原因文案与真机同义（真机的两处分别来自 `apply_error` 与 `unusable_reason`）。
  "shared.demo.qoder.gateway.reasonSessionDead": "凭据失效（需刷新令牌）",
  "shared.demo.qoder.gateway.reasonCooling": "上游暂时不可用（冷却中）",
  "shared.demo.qoder.gateway.catalogCached": "本次探测超时（8s），使用上次成功的结果",
  "shared.demo.qoder.gateway.modelRateLimited": "模型「MiniMax-M3」触及频率上限，已切到其它模型",
  "shared.demo.qoder.key.defaultCn": "Qoder 默认 Key",
  "shared.demo.qoder.key.cliCn": "Qoder CLI",
  "shared.demo.qoder.key.global": "Qoder 国际版 Key",
  // 凭据摘要的**格式**与 core `CredentialShape::describe` 逐字一致（演示站与真机同形）。
  "shared.demo.qoder.cred.device": "访问令牌(device_access) · 刷新令牌(device_refresh)",
  "shared.demo.qoder.cred.deviceOnly": "访问令牌(device_access) · 无刷新令牌",
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
  // 卡片「更多」菜单：四项刷新 + 账号动作分组（三平台共用同一套文案，避免各自造词）。
  "shared.cardMenu.refreshGroup": "刷新",
  "shared.cardMenu.refresh.credits": "刷新积分",
  "shared.cardMenu.refresh.tokens": "刷新今日 Token",
  "shared.cardMenu.refresh.checkin": "刷新签到状态",
  "shared.cardMenu.refresh.credential": "刷新凭据状态",
  "shared.cardMenu.accountGroup": "账号",
  // 四项刷新各自的说明（菜单项的 title）：说清「读的是哪一层」，失败时用户才知道卡在哪。
  "shared.cardMenu.refresh.creditsHint": "向上游查询剩余积分与到期时间（最慢，可能被限流）",
  "shared.cardMenu.refresh.tokensHint": "重读本机客户端会话日志 + 网关日志（本地，快）",
  "shared.cardMenu.refresh.checkinHint": "重读今天这个账号的签到结果（本地）",
  "shared.cardMenu.refresh.credentialHint": "重读本机登录凭据的有效期（凭据失效时只有这项还能用）",
  "shared.cardMenu.export": "导出账号",
  "shared.cardMenu.note": "备注与登录态在账号详情里（点卡片打开）",
  "shared.cardMenu.checkedToday": "今日已签到",
  "shared.cardMenu.checkedTodayHint": "今天已经签过，无需重复",
  "shared.cardMenu.needLogin": "需先登录",
  "shared.cardMenu.needLoginHint": "凭据已失效，先重新登录才能签到",
  "shared.cardMenu.busy": "刷新中…",
  "shared.cardMenu.done": "已更新",
  "shared.cardMenu.checkinDone": "今日已签到",
  "shared.cardMenu.checkinNeedsLogin": "需先登录",
  // 单项刷新的结果（四项各自一句，别用「已刷新」糊住 —— 用户要知道刷的是哪一层）。
  "shared.refreshPart.credits": "积分已刷新",
  "shared.refreshPart.tokens": "今日 Token 已刷新",
  "shared.refreshPart.checkin": "签到状态已刷新",
  "shared.refreshPart.credential": "凭据状态已刷新",
  "shared.refreshPart.fail": "刷新失败",
  // 详情抽屉顶部的四个指标块（文本在页面侧格式化，这里只提供标签与固定短语）。
  "shared.stat.credits": "积分",
  "shared.stat.tokens": "今日 Token",
  "shared.stat.checkin": "今日签到",
  "shared.stat.credential": "凭据",
  "shared.stat.checkedIn": "已签到",
  "shared.stat.notCheckedIn": "未签到",
  // issue #47：Trae / Qoder 的签到与 Token 两个块多出来的**诚实**措辞。
  // 「读不到」「未开放」「按本地台账」三档必须分开 —— 合成一句就是把「不知道」
  // 说成「没有」，用户会反复去点一个注定无效的按钮。
  "shared.stat.checkinUnreadable": "读不到",
  "shared.stat.checkinUnavailable": "本区域未开放",
  "shared.stat.checkinFromLedger": "按本地台账（刷新签到状态可向上游确认）",
  "shared.stat.tokensGatewayOnly": "仅经本网关的调用",
  "shared.stat.credentialOk": "有效",
  "shared.stat.credentialExpired": "已过期",
  "shared.stat.credentialRelogin": "需重登",
  "shared.stat.credentialOkSub": "可正常使用",
  "shared.stat.credentialBadSub": "先重新登录",
  "shared.stat.noExpiring": "无临期积分",
  "shared.stat.expiringSoon": "{n} 积分 {d} 天内到期",
  "shared.stat.total": "累计 {n}",
  "shared.stat.expiresAt": "到期 {when}",
  "shared.stat.noExpiry": "无到期时间",
  "shared.stat.plan": "{plan} 套餐",
  "shared.drawer.close": "关闭",
  "shared.drawer.loading": "正在加载详情…",
  // 详情页签：把 7 段长滚动切成四块（概览 / 积分 / 签到 / 凭据）。只切「看」的部分，
  // 备注 / 分组 / 程序位切换固定在上方，不随页签隐藏。
  "shared.drawer.tab.overview": "概览",
  "shared.drawer.tab.credits": "积分",
  "shared.drawer.tab.checkin": "签到",
  "shared.drawer.tab.credential": "凭据",
  "shared.drawer.tab.snapshot": "快照",
  "shared.drawer.snapshot.title": "登录态快照",
  "shared.drawer.snapshot.time": "最近修改",
  "shared.drawer.snapshot.size": "大小",
  "shared.drawer.snapshot.files": "{n} 个文件",
  "shared.drawer.snapshot.empty": "该账号还没有快照",
  // 备注区（就地编辑）—— 语义与卡片时代的备注编辑器一致，只是搬进了抽屉。
  "shared.drawer.remark.title": "备注",
  "shared.drawer.remark.placeholder": "例如：DS4.1 额度 · 10/03 解禁",
  "shared.drawer.remark.aria": "{name} 的备注",
  "shared.drawer.remark.editAria": "编辑 {name} 的备注",
  "shared.drawer.remark.add": "添加备注",
  // 分组区（就地编辑，与备注区**同构**；仅在调用方注入 `onSaveGroup` 时渲染）。
  "shared.drawer.group.title": "分组",
  "shared.drawer.group.placeholder": "输入自定义分组名（留空即移出分组）",
  "shared.drawer.group.aria": "{name} 的分组",
  "shared.drawer.group.editAria": "编辑 {name} 的分组",
  "shared.drawer.group.add": "设置分组",
  // 分组选择器（可新建）：既有的组列出来、最后一枚是「新建分组」。
  "shared.drawer.group.picker.aria": "{name} 的分组",
  "shared.drawer.group.picker.unassigned": "未分组",
  "shared.drawer.group.picker.create": "新建分组",
  "shared.drawer.group.picker.newPlaceholder": "新分组名",
  "shared.drawer.group.picker.confirm": "创建",
  "shared.drawer.group.picker.cancel": "取消",
  "shared.drawer.group.picker.exists": "该分组已存在，将直接归入",
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
  // 可点「切换到此账号」程序位 + 向导入口（原型 drawerAccount :3480-3486）。
  // ⚠️ 只在调用方注入 `switchTargets` 时渲染 —— 未注入时仍走上面的只读徽标（Trae）。
  "shared.drawer.switch.title": "切换到此账号",
  "shared.drawer.switch.wizard": "使用向导切换（可选迁移内容）",
  "shared.drawer.switch.available": "可切换",
  // 头部「当前登录」徽章（原型 :3475）—— 与只读徽标 `program.current` 区分：后者是「该程序位的当前账号」。
  "shared.drawer.currentLogin": "当前登录",
  // 操作区（原型 drawerAccount :3518-3527）：刷新 / 手动签到 / 编辑备注 / 导出 / 权限诊断 / 删除。
  "shared.drawer.actions.refresh": "刷新 Token",
  "shared.drawer.actions.export": "导出",
  "shared.drawer.actions.permission": "切换权限诊断",
  // 凭据不可用时的说明（原型 `.proto-note` :3526）—— 过期 / 需重登不给签到入口。
  "shared.drawer.actions.credentialNote": "凭据已过期或需重新登录，不提供签到入口，请先处理凭据。",
  // 签到历史（近 7 天）：日期 / 结果 / 获得积分（原型 checkinHistoryHtml :3428-3439）。
  // ⚠️ 获得积分取自**积分统计的签到事件**（账号 + 日期）；取不到时渲染「—」，绝不臆造固定奖励。
  "shared.drawer.checkin.historyTitle": "签到历史（近 7 天）",
  "shared.drawer.checkin.historyDate": "日期",
  "shared.drawer.checkin.historyResult": "结果",
  "shared.drawer.checkin.historyAmount": "获得积分",
  "shared.drawer.checkin.result.success": "成功",
  "shared.drawer.checkin.result.already": "已签到",
  "shared.drawer.checkin.result.fail": "失败",
  "shared.drawer.checkin.result.none": "未签到",
  "shared.drawer.checkin.historyEmpty": "暂无签到记录",
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
  // 原型 `batch-export`：批量条上导出**所选**账号（不是全部 —— 全部有工具栏那个入口）。
  "shared.batch.export": "导出",
  "shared.batch.delete": "删除",
  "shared.batch.clear": "清除选择",
  "shared.batch.deleteTitle": "批量删除账号",
  "shared.batch.deleteDesc": "确定删除选中的 {n} 个账号？删除后可在回收站恢复。",
  "shared.batch.confirm": "删除",
  "shared.batch.cancel": "取消",
  // ★ 批量签到结果条（原型 `checkinResultHtml`，`buddy-switch-prototype.html:2339-2354`）。
  // ⚠️ 必须**同时**给出「成功 N · 失败 M」，只报成功就是撒谎（批量操作必然部分失败）。
  "shared.checkinResult.summary": "成功 {ok} · 失败 {fail}",
  "shared.checkinResult.failedNames": "失败账号：{names}",
  "shared.checkinResult.allOk": "全部账号已签到",
  "shared.checkinResult.retryFailed": "重试失败项",
  "shared.checkinResult.summary4": "成功 {ok} · 未开放 {skip} · 失败 {fail}",
  "shared.checkinResult.skippedNames": "以下账号的签到通道未开放（桌面端网页活动页才可领）：{names}",
  // 本次真领到的额度合计（活动通道）。**可选**：不传时结果条不显示这一枚徽标，
  // WB / Trae 的渲染逐字不变（它们没有活动通道，也不该凭空多一行）。
  "shared.checkinResult.earned": "本次共 +{n} Credits",
  "shared.checkinResult.dismiss": "知道了",
  // 分组标题右侧的数量（原型 `.tiny muted`，`buddy-switch-prototype.html:2471`）。
  "shared.list.groupCount": "{n} 个",

  // =====================================================================
  // ---- 账号列表工具栏（components/account-toolbar.tsx，两平台共用）----
  // =====================================================================
  // 工具栏是**共享壳**：四个核心控件（搜索 / 排序 / 分组 / 视图）结构相同，
  // 平台差异由调用方以 options / addAction 注入，因此壳自身的文案放这里。
  "shared.toolbar.searchPlaceholder": "搜索账号名、邮箱、备注…（按 / 聚焦）",
  "shared.toolbar.searchAria": "搜索账号",
  "shared.toolbar.sort": "排序",
  "shared.toolbar.sortAria": "排序方式",
  "shared.toolbar.group": "分组",
  "shared.toolbar.groupAria": "分组方式",
  "shared.toolbar.viewAria": "切换卡片 / 表格视图",
  "shared.toolbar.viewCard": "卡片视图",
  "shared.toolbar.viewTable": "表格视图",
  // 窄窗（< 900px）下表格不可读 ⇒ 按钮**禁用并说明原因**（不是隐藏，隐藏会让用户以为没有表格）。
  "shared.toolbar.viewTableDisabled": "窗口太窄，表格视图不可用；请加宽窗口后重试",
  "shared.toolbar.selectAll": "全选",
  "shared.toolbar.deselectAll": "取消全选",

  // =====================================================================
  // ---- 「添加账号」抽屉（components/add-account-drawer.tsx，两平台共用）----
  // =====================================================================
  // ★ 抽屉是**共享组件**（WorkBuddy 与 Trae 是同一个），且同时承载原工具栏「⋯」菜单的内容 ⇒
  //   文案放这里，而不是 `wbAccounts.*` / `trae.page.*` 各写一份 —— 两平台的「添加账号」
  //   必须逐字一致（用户要求「交互保持一致」）。
  "shared.addDrawer.title": "添加账号",
  "shared.addDrawer.scopePrefix": "当前作用域：",
  "shared.addDrawer.scopeSuffix": "。选择一种方式添加账号。",
  "shared.addDrawer.oauthTitle": "OAuth 登录",
  "shared.addDrawer.oauthDesc": "打开官方授权页，用浏览器登录后自动导入",
  "shared.addDrawer.localTitle": "从本机导入",
  "shared.addDrawer.localDesc": "读取当前客户端的登录态，把正在用的账号加进来",
  "shared.addDrawer.backupTitle": "导入备份文件",
  "shared.addDrawer.backupDesc": "从导出的 JSON 备份批量恢复账号",
  "shared.addDrawer.notInstalled": "当前作用域未检测到客户端，「从本机导入」不可用。可先用 OAuth 登录。",
  // 「更多操作」= 原工具栏「⋯」菜单的分区标题。
  "shared.addDrawer.moreTools": "更多操作",

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

  // =====================================================================
  // ---- 授权链接面板（components/auth-link-panel.tsx，三平台共用）----
  // =====================================================================
  // 为什么三平台共用：交互链同构（发起 → 展示链接 → 轮询结果），平台差异只在
  // 「额外说明块」。**不能只靠自动开浏览器** —— 服务器 / 容器里没有系统浏览器，
  // 旧实现整个流程就断在 `openInBrowser` 那一行。
  "shared.authPanel.scanHint": "手机扫码，或复制链接在任意浏览器打开",
  "shared.authPanel.copy": "复制链接",
  "shared.authPanel.copied": "已复制",
  "shared.authPanel.authorized": "已授权",
  "shared.authPanel.waiting": "等待授权…",
  "shared.authPanel.remaining": "剩余 {time}",
  "shared.authPanel.expired": "已超时",
  "shared.authPanel.expiredDesc": "这条链接已作废，重新生成一条",
  "shared.authPanel.regenerate": "重新生成",
  "shared.authPanel.step1": "用手机或任意电脑的浏览器打开上面的链接。",
  "shared.authPanel.step2": "登录并点「授权」—— 不需要在这台机器上装客户端。",
  "shared.authPanel.step3": "本窗口自动收到结果，账号直接进账号库。",
  "shared.authPanel.noCallback": "不需要本机回调：服务端轮询官方接口取结果，服务器 / 容器里也能完成登录。",
  "shared.authPanel.qrAlt": "授权二维码",

  // =====================================================================
  // ---- 无客户端形态（服务器 / 容器部署，issue #62）----
  // =====================================================================
  // 缺客户端不是故障，是**部署形态**：账号与网关只依赖凭据本身。
  // 旧文案把它渲染成红字 + 只留「重新检测 / 从本机导入」，等于告诉服务器用户「你用不了」。
  "shared.srv.badge": "无客户端",
  "shared.srv.help": "降级说明",
  "shared.srv.strip.accounts": "未检测到 {scope} 客户端 —— 账号与网关不受影响：缺的只是程序位切换、客户端用量扫描与会话浏览。",
  "shared.srv.strip.gateway": "未检测到 {scope} 客户端 —— 网关照常可用：它只依赖账号池里的凭据。",
  "shared.srv.card.title": "本机没有 {platform} 客户端 —— 也能登录，也能开网关",
  "shared.srv.card.desc": "账号库里存的是凭据本身；客户端只是「现成的登录态来源」与「程序位开关」。选一条路登录一次，之后与装了客户端的机器没有区别。",
  "shared.srv.capOk": "没有客户端也能用",
  "shared.srv.capOff": "需要客户端在场",
  "shared.srv.capOk.oauth": "浏览器授权登录（链接 / 二维码，任意设备）",
  "shared.srv.capOk.accounts": "账号库、凭据刷新、签到与积分",
  "shared.srv.capOk.gateway": "API 网关、密钥管理、请求日志",
  "shared.srv.capOk.context": "上下文档位：用内置默认值（不再向上游探测）",
  "shared.srv.capOff.local": "从本机导入 —— 需要读客户端目录里的登录态",
  "shared.srv.capOff.programs": "程序位切换 —— 要改客户端自己的配置",
  "shared.srv.capOff.usage": "客户端用量扫描 —— 数据源是客户端会话文件",
  "shared.srv.capOff.sessions": "会话浏览与复制 —— 同上",
  "shared.srv.traeNote": "Trae 例外：授权与续期都依赖客户端里的设备私钥，纯浏览器登录走不通 —— 点「浏览器授权登录」看两条现实路径。",
  "shared.srv.help.title": "无客户端说明",
  "shared.srv.help.whyTitle": "为什么「没客户端」不是故障",
  "shared.srv.help.why1": "账号库里存的是凭据（access / refresh token），网关转发、签到、积分统计都只用它。客户端在不在同一台机器上，对这三件事没有影响。",
  "shared.srv.help.why2": "旧版把「未检测到客户端」渲染成红字 + 只留「重新检测 / 从本机导入」，等于告诉服务器用户「你用不了」—— 那是把一种部署形态当成了错误。",
  "shared.srv.help.moveTitle": "想把另一台机器上的账号搬过来",
  "shared.srv.help.moveDesc": "在那台机器上导出备份（JSON），回到这里用「导入备份文件」。备份里含凭据，导入后刷新与网关立即生效，不需要再登录一次。",
  "shared.srv.paste.title": "粘贴授权凭据",
  "shared.srv.paste.desc": "从任意一台装了客户端的机器复制凭据，或在浏览器授权后手工粘贴。本机不需要装客户端。",
  "shared.srv.paste.whyTitle": "为什么可以手工粘贴",
  "shared.srv.paste.whyDesc": "账号库存的就是凭据本身，「从本机导入」只是替你去客户端目录里读一次。入库之后，刷新、签到、网关走的是同一条路。",
  "shared.srv.paste.verify": "验证并添加",
  "shared.srv.paste.empty": "凭据是空的",
  "shared.srv.paste.emptyDesc": "把 auth 文件内容或 refresh_token 粘进来",
  "shared.srv.paste.failed": "凭据校验失败",
  "shared.srv.paste.ok": "已添加 {name}",
  "shared.srv.paste.okDesc": "凭据已入库 · 刷新与网关立即生效",
  "shared.srv.paste.hint.wb": "粘贴 auth/workbuddy-desktop-ai.info 里的 JSON（access_token / refresh_token）",
  "shared.srv.paste.hint.trae": "粘贴 refresh_token，或授权页跳回的那串回调链接",
  "shared.srv.paste.hint.qoder": "粘贴 Qoder 的个人访问令牌（PAT）",
  "shared.srv.localUnavailable": "本机没有客户端可读",
  "shared.srv.localUnavailableDesc": "改用「浏览器授权登录」或「粘贴授权凭据」；也可以在装了客户端的机器上导出备份再导入",
  "shared.srv.oauthTitle": "浏览器授权登录",
  "shared.srv.oauthDesc": "生成链接与二维码，用任意设备浏览器授权，服务端自己收结果",
} as const;
