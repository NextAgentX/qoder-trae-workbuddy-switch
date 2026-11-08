/**
 * 文案域：**Trae 产品线 · 统计与 API 服务页**（中文，键的权威之一）。
 *
 * 归属文件：
 * `src/pages/TraeTokenStatsPage.tsx`、`src/pages/TraeCreditsPage.tsx`、
 * `src/pages/TraeApiServicePage.tsx`
 *
 * ⚠️ 命名空间用 `trae`（不是 `traeStats`）：与另两个 Trae 域共用前缀、靠二级段区分
 * （`trae.page.*` / `trae.comp.*` / `trae.gateway.*` / `trae.stats.*`），
 * 因为它们本就是同一产品线的文案。
 *
 * 键前缀：`trae.stats.`
 */
export const zh = {
  // =====================================================================
  // TraeTokenStatsPage.tsx —— 「用量 · Token」视图（Trae 分区）：趋势 / 模型
  //
  // ⚠️ 页头 / 子 Tab / 视图参数条由 `UsagePage` 渲染，故本域**不再有** title / subtitle /
  //    时间窗口 / 版本范围条词条（那些控件已删除或上移）。
  // ⚠️ 两个视图的**统计卡与图表标题**全部走 `wbStats.usage.trend.*` / `wbStats.usage.models.*`
  //    / `wbStats.usage.range.*` —— 那是**原型用量视图的公共标签**（原型平台无关），
  //    两平台共用同一批键才能逐字一致。
  // =====================================================================
  "trae.stats.token.loadFailed": "无法读取 Token 统计",
  "trae.stats.token.retry": "重试",
  "trae.stats.token.refresh": "刷新",

  // ---- 数据源边界提示 ----
  "trae.stats.token.sourceTitle": "数据来源",
  "trae.stats.token.sourceFallback": "只统计经过本机 Trae 网关的调用。",
  // 账本口径（网关逐日落库，90 天）—— 与上面的「日志窗口」是两个观测点。
  "trae.stats.token.ledgerTitle": "数据来源 · 网关账本",
  "trae.stats.token.ledgerBody":
    "网关账本由后台每 2 分钟把请求日志增量记进本地账本，保留 90 天 —— 这一本自动就有。客户端那一本账落在 IDE 私有存储（SQLCipher 加密 + 二进制索引），本工具读不了，得用 sqlcipher 导出明文副本后从「客户端」档导入。",
  "trae.stats.token.heatmapLedgerUnavailable":
    "账本按日聚合，没有「周 × 小时」的时段槽位 —— 时段分布这张图在账本口径下画不出来，不画空图。",
  // 「按账本并排」专用：Trae 的 `all` 档是两本账并排，不是「按模型拆分」——
  // WB 的 `wbStats.usage.trend.stackedTitle` / `stackedAria` / `ledgerCaption` 都不适用
  // （后者结尾「见对账」在 Trae 上没有入口：无 `get_usage_reconcile`）。
  // ⚠️ 两本账不是等价账，也不是子集关系：客户端副本**含全部对话**；网关只含**经本机转发**
  //   的请求，其中还可能有第三方拿 API Key 直调网关、根本不在客户端库里 ⇒ **部分重叠**，
  //   不能相加；重叠量也测不出来（无逐条对账）。
  "trae.stats.token.ledgerStackedTitle": "每日 Token · 客户端 / 网关并排",
  "trae.stats.token.ledgerStackedCaption":
    "柱高是两本账各自的值，不合并、不扣减 —— 两本账会部分重叠（为什么、重叠多少见上方说明）。",
  "trae.stats.token.ledgerStackedAria": "每日 Token 柱状图：客户端与网关两本账并排对比（不叠加）",
  // 「全部」档顶部提示条专用。WB 的 `wbStats.usage.models.noMerge` / `noMergeBody` 说的是
  // 「同一请求两本账各记一次」（双计），Trae 不是那个形状 —— 这里是部分重叠 + 不可测。
  // ⚠️ 页面上它渲染为**加粗小标题**（一行），下一行才是 `noMergeBody` ⇒ 要短、像标签、不带句号。
  "trae.stats.token.noMerge": "两本账，不合并",
  "trae.stats.token.noMergeBody":
    "Trae 上两本账会部分重叠：客户端副本含全部对话，网关只含经本机转发的请求（其中还可能有第三方直调网关、不在客户端库里）。重叠多少算不出来，所以这里既不合并也不扣减 —— 想看清某一本，请切成单一账本。",

  // ---- 空态 ----
  "trae.stats.token.emptyTitle": "当前窗口内没有任何网关调用",
  "trae.stats.token.emptyBody": "到「API 服务」页启用网关，再把客户端的 Base URL 指过来，调用就会记在这里。",
  "trae.stats.token.ledgerEmptyTitle": "账本里还没有这段窗口的记录",
  "trae.stats.token.ledgerEmptyBody":
    "账本从启用后开始逐日累积（每 2 分钟记一轮）。如果网关一直没启用，这里会一直是空的 —— 未记录不等于当天没有用量。",

  // ---- 统计卡 ----
  // 标签走公共键；这里只有 aria 与 Trae 独有的第 4 枚（原型是「活跃账号」，
  // WB 无账号维度才用「模型数」替代 —— 见页面注释）。
  "trae.stats.token.summaryAria": "用量汇总统计",
  "trae.stats.token.summary.activeAccounts": "活跃账号",

  // ---- 平台不支持的维度 ----
  "trae.stats.token.unsupportedTitle": "平台不支持的维度",
  "trae.stats.token.unsupportedOnly": "（仅 {on}）",

  // ---- 日志解析失败（**诚实性提示**：数字不完整时必须说）----
  "trae.stats.token.parseErrors": "日志中有 {count} 条记录无法解析（缺少时间戳），已跳过。",

  // ---- 客户端账本（导入）----
  "trae.stats.token.client.title": "客户端账本（导入）",
  "trae.stats.token.client.body":
    "Trae 的对话库是 SQLCipher 加密的。填入口令解锁后，工具会在本机导出一份明文副本再读它 —— 原库不会被改动；不想让工具解密，也可以自己导出明文、直接把路径填在下面。",
  "trae.stats.token.client.pathLabel": "明文库路径",
  "trae.stats.token.client.pathPlaceholder": "例如 D:\\trae-plain\\database.db",
  "trae.stats.token.client.pick": "选择文件",
  "trae.stats.token.client.verify": "校验",
  "trae.stats.token.client.save": "保存并读取",
  "trae.stats.token.client.clear": "清除",
  "trae.stats.token.client.helpTitle": "口令从哪来 / 想自己导出",
  "trae.stats.token.client.helpBody":
    "口令在 Trae 的进程里、磁盘上没有副本，本工具也不内置提取手段 —— 你需要自己从客户端进程取（调试器断在 sqlite3_key 上读参数），这是有门槛的一步。拿到口令后，上面填进去点「解锁并读取」即可，工具会调 SQLCipher 的 sqlcipher_export 在本机导出明文副本。若你更想自己导出：在 sqlcipher 里 PRAGMA key='<口令>'; 然后 ATTACH DATABASE 'plain.db' AS plain KEY '';  SELECT sqlcipher_export('plain');  DETACH DATABASE plain; 并把 database.db-wal 一起处理，否则最近一段会丢。",
  "trae.stats.token.client.missing": "还没有指定明文库",
  "trae.stats.token.client.missingHint": "客户端账本需要先解锁一次；没解锁时这里只有网关一本账。",
  "trae.stats.token.client.notPlaintext": "这个文件不是明文 SQLite",
  "trae.stats.token.client.notPlaintextHint": "直接把加密的 database.db 指过来读不了。在上面填口令、点「解锁并读取」，工具会自己导出明文副本。",
  "trae.stats.token.client.unrecognized": "能打开，但认不出会话或用量结构",
  "trae.stats.token.client.unrecognizedHint": "客户端版本可能改了表结构。下方列出了库里实际的表名，便于适配。",
  "trae.stats.token.client.noUsage": "认出了会话结构，但没找到用量字段",
  "trae.stats.token.client.noUsageHint": "这个版本的客户端可能不回写 token 用量，只能统计会话条数。",
  "trae.stats.token.client.probeOk": "可读：{sessions} 个会话 · {messages} 条消息 · {withUsage} 条带用量",
  "trae.stats.token.client.probeTables": "库里的表",
  "trae.stats.token.client.importedAt": "上次读取：{time}",
  "trae.stats.token.client.loadFailed": "客户端账本读取失败",
  "trae.stats.token.client.retry": "重试",
  // ---- 内置解密（口令由用户输入，工具内部导出明文副本）----
  "trae.stats.token.client.passphraseLabel": "库口令",
  "trae.stats.token.client.passphrasePlaceholder": "Trae 私有存储的口令",
  "trae.stats.token.client.passphraseHint":
    "口令在 Trae 的进程里，磁盘上没有副本 —— 本工具只拿它解密一次，用完即弃，不落盘、不上传。",
  "trae.stats.token.client.unlock": "解锁并读取",
  "trae.stats.token.client.unlocking": "正在解锁…",
  "trae.stats.token.client.unlocked": "已解锁（兼容代 {compat}），明文副本已就绪",
  "trae.stats.token.client.unlockedPlain": "这个文件本来就是明文，已直接使用",
  "trae.stats.token.client.unlockHint":
    "解密产物写在工具的私有目录（大小约等于源库），原库不会被改动。",
  "trae.stats.token.client.wrongKey": "口令不对",
  "trae.stats.token.client.wrongKeyHint": "确认这是当前客户端版本的库口令。连续失败不会锁库，重试即可。",
  "trae.stats.token.client.alreadyPlaintext": "这个文件已经是明文库",
  "trae.stats.token.client.alreadyPlaintextHint": "不需要口令。直接点「保存并读取」把它设为数据源即可。",
  "trae.stats.token.client.notSqlite": "不是 SQLite 数据库",

  // =====================================================================
  // TraeCreditsPage.tsx —— 「用量 · 积分」视图（Trae 分区）
  //
  // ⚠️ 统计卡与图表标题尽量走 `wbStats.usage.credits.*`（原型积分视图的公共标签）；
  //    本域只留 Trae 独有的部分（双向柱的标题、两个环形、流水说明）。
  // =====================================================================
  "trae.stats.credits.subtitle": "查看每个 Trae 账号的剩余积分、每日变化与历史趋势。",
  "trae.stats.credits.sync": "同步积分",
  "trae.stats.credits.syncing": "同步中",
  "trae.stats.credits.refreshDone": "积分数据已刷新",
  "trae.stats.credits.refreshFailed": "刷新失败",
  "trae.stats.credits.loadFailed": "无法读取 Trae 积分数据",
  "trae.stats.credits.overviewAria": "积分总览",

  // ---- 统计卡的三条副标题（原型：签到奖励 / 净 ±Y / 最早 D+N 天后）----
  /* ★ issue #186：`total` 是部分和（额度不可解析的账号不计入），必须把未知数说出来。 */
  "trae.stats.credits.accountsHintPartial": "{count} 个账号 · 其中 {unknown} 个额度未知（未计入）",
  "trae.stats.credits.inHint": "签到入账",
  "trae.stats.credits.netHint": "净 {net}",
  "trae.stats.credits.soonestHint": "最早 {days} 天后",
  "trae.stats.credits.noExpiryHint": "30 天内无到期",

  // ---- 每日积分收支（Trae **两侧都有** ⇒ 双向柱；WB 只有消耗侧）----
  "trae.stats.credits.dailyTitle": "每日积分收支",
  "trae.stats.credits.dailyNote": "上 = 签到入账　下 = 对话消耗",
  "trae.stats.credits.dailyAria": "每日积分收支双向柱状图（上为入账、下为消耗）",
  "trae.stats.credits.expiryEmpty": "未来 30 天内没有积分到期。",

  // ---- 原型积分页的两个环形（WB 侧无数据源 ⇒ 不渲染）----
  "trae.stats.credits.accountShareTitle": "账号 Token 占比",
  "trae.stats.credits.shareNote": "累计",
  "trae.stats.credits.accountShareAria": "各账号累计 Token 占比环形图",
  "trae.stats.credits.accountShareEmpty": "还没有账号产生用量。",
  "trae.stats.credits.statusTitle": "网关状态码分布",
  "trae.stats.credits.statusNote": "当前窗口",
  "trae.stats.credits.statusAria": "网关请求状态码分布环形图",
  "trae.stats.credits.statusEmpty": "本作用域没有网关请求。",

  // ---- 积分流水 ----
  "trae.stats.credits.flowNote": "按日期倒序",
  "trae.stats.credits.recordEarned": "积分入账",
  "trae.stats.credits.recordSpent": "积分消耗",
  "trae.stats.credits.flowEmptyTitle": "暂无积分流水",
  "trae.stats.credits.flowEmptyBody": "执行一次签到或「同步积分」后，这里会按天记录积分变动。",

  // ---- 空态 ----
  "trae.stats.credits.trendEmptyAccounts": "尚无账号数据。添加账号后这里会展示积分统计。",
  "trae.stats.credits.trendEmptyData": "暂无积分数据。执行一次签到或「同步积分」后即可看到每日变化。",

  // ---- 平台不支持的维度 ----
  "trae.stats.credits.unsupportedTitle": "平台不支持的维度",
  "trae.stats.credits.unsupportedOnly": "（仅 {on}）",

  // =====================================================================
  // TraeApiServicePage.tsx —— 「API 服务」页
  // =====================================================================
  "trae.stats.api.title": "API 服务",
  "trae.stats.api.subtitle": "把 Trae 的模型额度以 OpenAI 兼容接口提供给本机工具。",
  "trae.stats.api.loadFailed": "无法读取网关状态",
  "trae.stats.api.retry": "重试",
  "trae.stats.api.refresh": "刷新",

  // ---- 网关卡 ----
  "trae.stats.api.gateway.enable": "启用 API 网关",
  "trae.stats.api.gateway.desc": "开启后本机 AI 工具可通过下方地址调用 Trae 模型（上游 {upstream}）。",
  "trae.stats.api.gateway.bindAddr": "监听地址",
  "trae.stats.api.gateway.bindLoopback": "127.0.0.1（仅本机）",
  "trae.stats.api.gateway.bindLan": "0.0.0.0（局域网）",
  "trae.stats.api.gateway.port": "监听端口",
  "trae.stats.api.gateway.statusLabel": "状态：",
  "trae.stats.api.gateway.running": "运行中",
  "trae.stats.api.gateway.stopped": "已停止",
  "trae.stats.api.gateway.openDataDir": "打开数据目录",
  "trae.stats.api.gateway.noteLead": "默认端口 57894，与 WorkBuddy 网关（57891）错开——两者都占用 ",
  "trae.stats.api.gateway.maxMode": "Max Mode（按各模型上限）",
  "trae.stats.api.gateway.maxModeDesc": "对支持 Max Mode 的模型注入 is_max_mode，清单按各模型自己的上限声明（不是统一 1M）；关闭后回到 __dev 通道的默认窗口。",
  "trae.stats.api.gateway.noteTail": "。",
  "trae.stats.api.gateway.lanWarning": "局域网模式下，同网段任何设备拿到 Key 都能消耗你的 Trae 积分。",
  "trae.stats.api.gateway.lastError": "最近一次错误",

  // ---- 接入地址卡 ----
  "trae.stats.api.endpoint.title": "接入地址",
  "trae.stats.api.endpoint.baseUrl": "Base URL",
  "trae.stats.api.copy": "复制",
  "trae.stats.api.endpoint.copied": "Base URL 已复制",
  "trae.stats.api.endpoint.keyPrefix": "最近 Key 前缀",
  "trae.stats.api.endpoint.noKey": "尚无可用 Key",
  /* 概览两行 —— 与 WB 侧 `wbStats.gateway.availableModels` / `modelCount` /
     `poolEntries` / `poolEntryCount` / `view` **逐字同文案**（两平台的概览行是同一种东西）。
     ⚠️ Trae **没有**「上游账号凭据 · 校验」那一行：WB 的探针是 `check_auth_permission`
     （写权限自检），Trae 侧不存在对应命令 —— 缺能力不画空壳（§3.4 规则 1）。 */
  "trae.stats.api.overview.models": "可用模型",
  "trae.stats.api.overview.modelsCount": "{count} 个",
  "trae.stats.api.overview.pool": "账号池",
  "trae.stats.api.overview.poolCount": "{count} 个条目",
  "trae.stats.api.overview.view": "查看",

  // ---- 操作结果提示 ----
  "trae.stats.api.toast.savedStarted": "网关已保存并启动",
  "trae.stats.api.toast.savedDisabled": "网关已保存（未启用监听）",
  "trae.stats.api.toast.saveFailed": "保存失败",
  "trae.stats.api.toast.portInvalid": "端口无效",
  "trae.stats.api.toast.portRange": "请输入 1–65535 之间的整数",
  "trae.stats.api.toast.logsCleared": "日志已清空",
  "trae.stats.api.toast.clearFailed": "清空失败",
  "trae.stats.api.toast.logsRefreshed": "已补入 {n} 条积分",
  "trae.stats.api.toast.logsRefreshedNone": "已向 Trae 查过账单，暂无新记录",
  "trae.stats.api.toast.logsRefreshCooling": "刚刚已查过账单，2 分钟后才能再查（避免触发上游限流）",
  "trae.stats.api.toast.logsRefreshTooFresh": "账单还没出（上游出账约需 6–8 秒），稍等再试",
  "trae.stats.api.toast.logsRefreshFailed": "刷新积分失败",
  "trae.stats.api.toast.unsupported": "当前平台不支持",
  "trae.stats.api.toast.unsupportedDesc": "该能力仅在 Windows 提供。",
  "trae.stats.api.toast.dirOpened": "已打开 Trae 数据目录",
  "trae.stats.api.toast.dirOpenFailed": "打开数据目录失败",

  // ---- 局域网风险确认 ----
  "trae.stats.api.lanDialog.title": "允许局域网访问？",
  "trae.stats.api.lanDialog.desc": "监听 0.0.0.0 后，同网段（含公共 Wi-Fi）的任何设备只要拿到 API Key，就能消耗你的 Trae 积分。请只在可信网络下开启。",
  "trae.stats.api.lanDialog.cancel": "取消",
  "trae.stats.api.lanDialog.confirm": "我已确认，开启",
} as const;
