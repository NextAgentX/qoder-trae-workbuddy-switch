/**
 * 文案域：**Trae 网关管理**（中文，键的权威之一）。
 *
 * 归属文件：
 * `src/components/gateway/trae-model-list.tsx`、`src/components/gateway/trae-request-log.tsx`、
 * `src/components/gateway/trae-integration-guide.tsx`、
 * `src/components/gateway/trae-api-key-table.tsx`
 * （账号池卡已两侧共用，文案在 `shared.zh.ts`）
 *
 * ⚠️ 命名空间用 `trae`（不是 `traeGateway`）：与另两个 Trae 域共用前缀、靠二级段区分。
 *
 * ⚠️ `trae.variant.*`（产品线状态条 / 切换器）已随组件删除 —— 区域切换统一上移到顶栏
 * `ScopeSelector`，页头只留只读 `ScopeChip`（文案在 `shell.zh.ts`）。
 *
 * 键前缀：`trae.gateway.`
 */
export const zh = {
  // =====================================================================
  // gateway/trae-model-list.tsx —— 模型清单
  // =====================================================================
  "trae.gateway.models.title": "模型清单",
  "trae.gateway.models.summary": "共 {count} 个 · 默认 {model}",
  "trae.gateway.models.note": "清单读自 Trae 客户端的本地缓存（上游下发），客户端刷新后这里会跟着变；网关对外清单（/v1/models）按 API Key 的归属程序位取，两条程序位的内容不同。",
  "trae.gateway.models.empty": "暂无模型数据。",
  "trae.gateway.models.emptyDesc": "客户端未启动 / 未登录 / 尚未拉取清单时这里为空；在客户端刷新模型列表后点「重新读取」即可。",
  "trae.gateway.models.refresh": "重新读取",
  "trae.gateway.models.scanning": "读取中…",
  "trae.gateway.models.program": "选择目标客户端",
  "trae.gateway.models.programTip": "读取【{label}】客户端的模型清单",
  /* 来源徽标（两级，**不**统一成 WB 的三级 —— 机制不同，统一会让 UI 撒谎，见 P10 待明确 #3）。
     徽标前的「来源」标签走 `shared.gateway.sourceBar.sourceLabel`。 */
  "trae.gateway.models.sourceCache": "客户端缓存",
  "trae.gateway.models.sourceMissing": "未读取",
  /* 兜底原因（后端 `note` 缺失时用）：契约 `kind !== "live"` ⇒ 原因恒非空。
     「客户端缓存」也**不是**实时，故同样要说明它读自哪里。 */
  "trae.gateway.models.reasonCache": "读取自客户端本地缓存（上游下发），非上游实时",
  "trae.gateway.models.reasonMissing": "尚未从客户端读取到清单",
  "trae.gateway.models.readFrom": "读取自 {label} 客户端",
  "trae.gateway.models.gatewayCount": "网关对外 {count} 个",
  "trae.gateway.models.gatewayCountFor": "网关对外 {count} 个（{program}）",
  /* P11：分组 chips 改为**表格**（列集 = 模型 / 上游状态 / 网关暴露），下列键随之作废并删除：
     `groupCount`（分组标题）、`badgeDefault` / `badgeNew` / `badgeBeta` / `badgeCustom`
     （模型徽标）、`badgeNotServed`（旧「网关不提供」标记 —— 现由**上游状态列**承担）、
     `context`（上下文窗口，原在悬停提示里）。 */

  // =====================================================================
  // gateway/trae-request-log.tsx —— 请求日志
  // =====================================================================
  "trae.gateway.log.title": "请求日志（最近 {count} 条）",
  "trae.gateway.log.clear": "清空",
  "trae.gateway.log.empty": "暂无请求。网关启动后，客户端发来的每次调用都会记在这里（默认只记元数据，不记正文）。",
  "trae.gateway.log.col.time": "时间",
  "trae.gateway.log.col.account": "账号",
  "trae.gateway.log.col.model": "模型",
  "trae.gateway.log.col.status": "状态",
  "trae.gateway.log.col.latency": "耗时",
  "trae.gateway.log.col.tokens": "Token",

  // =====================================================================
  // gateway/trae-integration-guide.tsx —— 接入指引
  // =====================================================================
  "trae.gateway.guide.title": "接入指引",
  "trae.gateway.guide.copy": "复制代码",
  "trae.gateway.guide.copied": "代码已复制",
  "trae.gateway.guide.noKey": "sk-trae-…（请先在上方创建 Key）",
  "trae.gateway.guide.streamNote": "Trae 上游只支持流式；请求 `stream: false` 时由本网关在本地聚合后一次性返回，首字节延迟较长。",
  /* 自动发现说明（P10 §1.7）：与 WB 侧同构 —— 外部客户端会从本网关的 `/v1/models` 自动读清单。 */
  "trae.gateway.guide.autoDiscover": "客户端会自动从 `/v1/models` 读取模型清单，不需要手填模型名；若你的工具不支持自动发现，从「模型」页复制即可。",
  // 可复制配置片段里的字段标签（给外部工具粘贴用）
  /* P3：`trae.gateway.guide.snippet.*` 已删除 —— 字段标签统一到 `shared.gateway.snippet.*`。 */

  // =====================================================================
  // 账号池卡已**两侧共用**（`gateway/account-pool-card.tsx`），文案随之迁到
  // `shared.gateway.pool.*` / `shared.poolStatus.*`（见 `shared.zh.ts`）。
  // =====================================================================

  // =====================================================================
  // gateway/trae-api-key-table.tsx —— API Key 列表 / 创建 / 吊销 / 删除
  // =====================================================================
  // ---- toast ----
  "trae.gateway.key.loadFailed": "读取 API Key 列表失败",
  "trae.gateway.key.nameRequired": "请填写名称",
  "trae.gateway.key.noPlaintext": "创建成功但未返回明文，请重试",
  "trae.gateway.key.createFailed": "创建失败",
  "trae.gateway.key.revoked": "已吊销",
  "trae.gateway.key.revokeFailed": "吊销失败",
  "trae.gateway.key.deleted": "已删除",
  "trae.gateway.key.deleteFailed": "删除失败",
  "trae.gateway.key.copied": "API Key 已复制",

  // ---- 列表 ----
  "trae.gateway.key.create": "创建 API Key",
  "trae.gateway.key.loading": "读取中…",
  "trae.gateway.key.empty": "尚未创建 API Key。",
  "trae.gateway.key.neverUsed": "从未使用",
  "trae.gateway.key.statusRevoked": "已吊销",
  "trae.gateway.key.statusActive": "启用",
  "trae.gateway.key.revoke": "吊销",
  "trae.gateway.key.delete": "删除",

  // ---- 表头 ----
  "trae.gateway.key.name": "名称",
  "trae.gateway.key.col.variant": "归属程序位",
  "trae.gateway.key.col.prefix": "前缀",
  "trae.gateway.key.col.createdAt": "创建时间",
  "trae.gateway.key.col.lastUsed": "最近使用",
  "trae.gateway.key.col.status": "状态",
  "trae.gateway.key.col.actions": "操作",

  // ---- 创建对话框 ----
  "trae.gateway.key.createDesc": "每个 Key 只能访问其归属程序位的模型与账号池 —— TraeWork 与 TraeCode 的可调模型不同。",
  "trae.gateway.key.namePlaceholder": "例如 Cursor",
  "trae.gateway.key.variant": "归属程序位",
  "trae.gateway.key.cancel": "取消",
  "trae.gateway.key.createSubmit": "创建",

  // ---- 一次性明文 ----
  "trae.gateway.key.createdTitle": "API Key 已创建",
  "trae.gateway.key.createdDesc": "完整 Key 只显示这一次，请立即复制保存。",
  "trae.gateway.key.copy": "复制",
  "trae.gateway.key.saved": "我已保存，关闭",

  // ---- 吊销 / 删除 确认 ----
  "trae.gateway.key.revokeTitle": "吊销 API Key",
  "trae.gateway.key.revokeDesc": "吊销后「{name}」立即失效（401），列表中保留为「已吊销」状态。",
  "trae.gateway.key.deleteTitle": "删除 API Key",
  "trae.gateway.key.deleteDesc": "确定删除已吊销的「{name}」？此操作不可撤销。",
} as const;
