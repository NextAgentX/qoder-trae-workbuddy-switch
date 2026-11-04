/**
 * 文案域：**Qoder 产品线 · API 网关**（中文，键的权威之一）。
 *
 * 归属文件：
 * `src/pages/QoderApiServicePage.tsx`、`src/components/gateway/qoder-api-key-table.tsx`、
 * `src/components/gateway/qoder-model-list.tsx`
 *
 * ⚠️ 命名空间用 `qoder.gateway.`（与 `trae.gateway.` 同款：页面与它的专用组件共用一个前缀）。
 * **可共用的文案一律不在这里**：账号池卡 / 模型表 / 来源条 / 日志卡的词条走 `shared.*`
 * （三平台同形），本域只放 Qoder 侧特有的那批。
 */
export const zh = {
  // ---- 页头 / 服务卡（QoderApiServicePage.tsx）----
  "qoder.gateway.title": "网关",
  // ⚠️ 页头副标题走 `shared` 域的 `pageHead.gateway` —— 与 WB / Trae 同一公式，不另立门户。

  "qoder.gateway.enable": "启用本地网关",
  "qoder.gateway.enableDesc": "在 127.0.0.1 上提供 OpenAI / Anthropic 兼容接口·上游为 {upstream}",
  "qoder.gateway.bindAddr": "监听地址",
  "qoder.gateway.bindLoopback": "仅本机（127.0.0.1）",
  "qoder.gateway.bindLan": "局域网（0.0.0.0）",
  "qoder.gateway.port": "端口",
  "qoder.gateway.statusLabel": "状态",
  "qoder.gateway.running": "运行中",
  "qoder.gateway.stopped": "已停用",
  // 四条网关都占 `/v1/chat/completions` ⇒ 端口必须错开；这句是把「为什么是 57895」讲明白。
  "qoder.gateway.noteLead": "各条网关都占用",
  "qoder.gateway.noteTail": "—— 端口必须错开（本页是 Qoder，默认 57895）。",
  "qoder.gateway.lanWarning": "当前已允许局域网访问 —— 请确认所处网络可信。",
  "qoder.gateway.lastError": "最近一次失败",
  "qoder.gateway.refresh": "刷新",
  "qoder.gateway.savedStarted": "已保存并启动监听",
  "qoder.gateway.savedDisabled": "已保存：监听已停止",
  "qoder.gateway.saveFailed": "保存失败",
  "qoder.gateway.portInvalid": "端口无效",
  "qoder.gateway.portRange": "端口需在 1–65535 之间",
  "qoder.gateway.loadFailed": "加载网关信息失败",
  "qoder.gateway.retry": "重试",

  // ---- 接入地址卡 ----
  "qoder.gateway.endpoint.title": "接入地址",
  "qoder.gateway.endpoint.baseUrl": "Base URL",
  "qoder.gateway.endpoint.keyPrefix": "API Key",
  "qoder.gateway.endpoint.noKey": "还没有 Key（去「密钥」分区创建）",
  "qoder.gateway.endpoint.copied": "地址已复制",
  "qoder.gateway.copy": "复制",

  // ---- 概览两行 ----
  "qoder.gateway.overview.models": "可用模型",
  "qoder.gateway.overview.modelsCount": "{count} 个",
  "qoder.gateway.overview.pool": "账号池",
  "qoder.gateway.overview.poolCount": "{count} 个条目",
  "qoder.gateway.overview.view": "查看",

  // ---- 局域网风险确认 ----
  "qoder.gateway.lanDialog.title": "允许局域网访问？",
  "qoder.gateway.lanDialog.desc":
    "监听 0.0.0.0 后，同一网络内的任何设备都可以用你的 Key 调用本网关（消耗你的 Qoder 额度）。仅在你确认网络可信时开启。",
  "qoder.gateway.lanDialog.cancel": "取消",
  "qoder.gateway.lanDialog.confirm": "确认开启",

  // ---- 接入指引（qoder 侧两处平台化文案；其余词条复用 `trae.gateway.guide.*`）----
  "qoder.gateway.guide.noKey": "sk-qoder-…（请先在上方创建 Key）",
  "qoder.gateway.guide.streamNote":
    "Qoder 上游只支持流式；请求 `stream: false` 时由本网关在本地聚合后一次性返回，首字节延迟较长。",

  // ---- 请求日志（日志卡与 Trae 共用；只有这两条 toast 是 Qoder 侧自己的）----
  "qoder.gateway.logsCleared": "日志已清空",
  "qoder.gateway.logsClearFailed": "清空日志失败",

  // ---- 模型清单（qoder-model-list.tsx）----
  "qoder.gateway.quota.title": "限额护栏",
  "qoder.gateway.quota.refreshFailed": "读取护栏状态失败",
  "qoder.gateway.quota.desc": "达到阈值即暂停使用该账号（0 = 关闭该条；未知一律放行）",
  "qoder.gateway.quota.refresh": "刷新",
  "qoder.gateway.quota.reserveLabel": "余额地板（credits）",
  "qoder.gateway.quota.reserveHint": "剩余 ≤ 该值即停；余额未知时不拦",
  "qoder.gateway.quota.dailyLabel": "当日 token 上限",
  "qoder.gateway.quota.dailyHint": "单账号今天经网关转发的 token 总量",
  "qoder.gateway.quota.creditLabel": "当日 credit 上限",
  "qoder.gateway.quota.creditHint": "单账号今天经网关消耗的 credits（可填小数）",
  "qoder.gateway.quota.creditsUsed": "已用 {n} credit",
  "qoder.gateway.quota.why.daily_credits": "当日 credit 已满",
  "qoder.gateway.quota.modelLabel": "单模型当日 token",
  "qoder.gateway.quota.modelHint": "只限该模型，不连坐同账号的其它模型",
  "qoder.gateway.quota.invalid": "只能填非负整数（0 = 关闭）",
  "qoder.gateway.quota.noteOn": "计数来自网关自己的记账；每天 10:00（UTC+8）重置",
  "qoder.gateway.quota.noteOff": "三条都是 0 ⇒ 护栏未启用，不会拦任何请求",
  "qoder.gateway.quota.save": "保存",
  "qoder.gateway.quota.loading": "正在读取护栏状态…",
  "qoder.gateway.quota.empty": "本区域还没有账号",
  "qoder.gateway.quota.today": "今日（{date}）· {count} 个账号",
  "qoder.gateway.quota.blockedCount": "{count} 个已被拦下",
  "qoder.gateway.quota.allClear": "当前没有账号被拦",
  "qoder.gateway.quota.noModelUsage": "今日暂无网关用量",
  "qoder.gateway.quota.credits": "余额 {n}",
  "qoder.gateway.quota.tokens": "{n} token",
  "qoder.gateway.quota.ok": "可用",
  "qoder.gateway.quota.why.parked": "余额耗尽停放",
  "qoder.gateway.quota.why.reserve": "触到余额地板",
  "qoder.gateway.quota.why.daily_tokens": "当日 token 已满",
  "qoder.gateway.quota.why.model_tokens": "模型额度已满",
  "qoder.gateway.quota.why.other": "已拦下",
  "qoder.gateway.models.title": "模型清单",
  "qoder.gateway.models.promotionWindow": "错峰时段 {range}（UTC+8）",
  "qoder.gateway.models.note":
    "清单直接来自 Qoder 官方（拉取需要账号凭据），10 分钟内不重复请求；「禁用」的模型不会出现在 /v1/models。",
  "qoder.gateway.models.refresh": "重新识别",
  "qoder.gateway.models.refreshFailed": "重新识别失败",
  "qoder.gateway.models.scanning": "识别中…",
  "qoder.gateway.models.sourceLive": "实时",
  "qoder.gateway.models.sourceCached": "上次成功",
  "qoder.gateway.models.sourceUnavailable": "暂无清单",
  "qoder.gateway.models.reasonCached": "本次拉取失败，展示的是上次成功的结果",
  "qoder.gateway.models.reasonUnavailable":
    "还没成功拉到过清单：请确认账号可调用，然后重新识别",
  "qoder.gateway.models.empty": "还没有可展示的模型",
  "qoder.gateway.models.emptyDesc":
    "模型清单需要账号凭据向官方拉取 —— 先在「账号」分区导入一个账号，再点「重新识别」。",

  // ---- 密钥表（qoder-api-key-table.tsx）----
  "qoder.gateway.key.create": "创建密钥",
  "qoder.gateway.key.createDesc": "密钥绑定一个区域：它决定这把 Key 走哪本账号库。",
  "qoder.gateway.key.name": "名称",
  "qoder.gateway.key.namePlaceholder": "例如：CI 自动化",
  "qoder.gateway.key.nameRequired": "请填写名称",
  "qoder.gateway.key.region": "归属区域",
  "qoder.gateway.key.regionHint": "两个区域是独立的账号库与端点，Key 不能跨区域使用。",
  "qoder.gateway.key.cancel": "取消",
  "qoder.gateway.key.createSubmit": "创建",
  "qoder.gateway.key.createFailed": "创建失败",
  "qoder.gateway.key.loadFailed": "密钥列表加载失败",
  "qoder.gateway.key.empty": "还没有密钥",
  "qoder.gateway.key.col.region": "归属",
  "qoder.gateway.key.col.prefix": "密钥",
  "qoder.gateway.key.col.createdAt": "创建于",
  "qoder.gateway.key.col.lastUsed": "最后使用",
  "qoder.gateway.key.col.status": "状态",
  "qoder.gateway.key.col.actions": "操作",
  "qoder.gateway.key.statusActive": "启用",
  "qoder.gateway.key.statusRevoked": "已吊销",
  "qoder.gateway.key.neverUsed": "从未使用",
  "qoder.gateway.key.revoke": "吊销",
  "qoder.gateway.key.revokeTitle": "吊销这把密钥？",
  "qoder.gateway.key.revokeDesc":
    "「{name}」将立即无法调用（可逆）：已吊销的行仍留在表里，随时可以「恢复」。",
  "qoder.gateway.key.revoked": "已吊销",
  "qoder.gateway.key.revokeFailed": "吊销失败",
  "qoder.gateway.key.restore": "恢复",
  "qoder.gateway.key.restoreTitle": "恢复这把密钥？",
  "qoder.gateway.key.restoreDesc":
    "「{name}」将重新可用 —— 库中存的是同一把 Key，明文未变，客户端无需改配置。",
  "qoder.gateway.key.restored": "已恢复",
  "qoder.gateway.key.restoreFailed": "恢复失败",
  "qoder.gateway.key.delete": "删除",
  "qoder.gateway.key.deleteTitle": "删除这把密钥？",
  "qoder.gateway.key.deleteDesc": "「{name}」的记录将被物理删除（不可逆）。使用它的客户端会立即 401。",
  "qoder.gateway.key.deleted": "已删除",
  "qoder.gateway.key.deleteFailed": "删除失败",
} as const;
