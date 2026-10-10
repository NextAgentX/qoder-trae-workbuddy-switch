/** 概览工作台（PLAN §3.9 / P3 新增）。 */
export const zh = {
  "wbOverview.title": "概览",
  "wbOverview.refresh": "刷新",

  "wbOverview.pendingTitle": "待处理",
  "wbOverview.pending.count": "{n} 项",
  "wbOverview.pending.relogin": "{count} 个账号需要重新登录",
  "wbOverview.pending.reloginHint": "登录态已失效，切到该账号时会失败。去账号管理里重新登录。",
  // ★ 对齐新原型 `viewOverview`（`buddy-switch-prototype.html:2065-2067`）新增的三项。
  // ⚠️ 与上面的「需要重新登录」**分开**：过期是**凭据层面**已失效，重登是**会话层面**被踢。
  "wbOverview.pending.expired": "{count} 个账号凭据已过期",
  "wbOverview.pending.expiredHint": "凭据已过期，切到该账号会失败。去账号管理重新登录后再用。",
  "wbOverview.pending.notChecked": "{count} 个账号今日未签到",
  "wbOverview.pending.notCheckedHint": "今天还没签到。可直接在这里一键补齐，或去账号管理逐个处理。",
  "wbOverview.pending.expiringCredits": "{count} 个账号有即将过期的积分",
  "wbOverview.pending.expiringCreditsHint": "共 {amount} 积分即将过期，过期后不可再用。",
  "wbOverview.pending.expiring": "{count} 个账号的登录态即将过期",
  "wbOverview.pending.expiringHint": "24 小时内到期。到期后会退化成「需要重新登录」。",
  "wbOverview.pending.notRunning": "客户端当前未运行",
  "wbOverview.pending.notRunningHint": "账号切换需要客户端在运行状态。",
  "wbOverview.pending.noAccounts": "这个作用域还没有账号",
  "wbOverview.pending.noAccountsHint": "先添加一个账号才能使用。",
  // 行内动作（原型 `.srow` 右侧按钮）：一律指向**承接该动作的目的地**，不做悬浮提示。
  "wbOverview.pending.action.handle": "去处理",
  "wbOverview.pending.action.settings": "去设置",
  "wbOverview.pending.action.addAccount": "添加账号",
  // 「今日未签到」的行内动作是**可执行**的（原型 `run: "batch-checkin"`），不是跳转。
  "wbOverview.pending.action.checkin": "一键签到",
  "wbOverview.toast.checkinLoading": "正在签到…",
  "wbOverview.toast.checkinOk": "签到完成",
  "wbOverview.toast.checkinPartial": "部分账号签到失败",
  "wbOverview.toast.checkinFail": "签到失败",
  "wbOverview.toast.checkinSummary": "成功 {ok} 个 · 已签 {already} 个 · 失败 {failed} 个",
  "wbOverview.toast.checkinRetryFailed": "只重试失败项",

  // ---- 概览侧栏（原型 `.ov-side`：其他作用域 + 能力）----
  "wbOverview.otherScopes.title": "其他作用域",
  "wbOverview.otherScopes.loggedIn": "已登录 {name} · {n} 个账号",
  "wbOverview.otherScopes.loggedOut": "未登录 · {n} 个账号",
  "wbOverview.otherScopes.missing": "未检测到 · {n} 个账号",
  "wbOverview.capabilities.title": "能力",

  /* 没有待处理项是**好消息**，所以空态不是「暂无数据」而是「一切正常」+ 出口。 */
  "wbOverview.allGood": "一切正常",
  "wbOverview.allGoodHint": "没有需要处理的账号。",
  "wbOverview.allGoodAction": "去账号管理",

  // ---- 统计行（原型 `viewOverview` 的 `.grid.g4` 四块）----
  // 「读不到」一律由 `wbOverview.stat.unavailable` 表达 —— **不回落 0**（0 是「确实是 0」）。
  "wbOverview.stat.aria": "概览统计",
  "wbOverview.stat.accounts": "账号总数",
  "wbOverview.stat.accountsAttention": "{n} 个需处理",
  "wbOverview.stat.accountsOk": "全部正常",
  // ★ Q3：概览这一格统计的是**客户端自身**的 Token（走 `get_token_statistics` 扫会话日志），
//   账号卡的「今日 Token」则**含网关**（`get_account_token_today` 额外读请求日志）。
//   两个来源是**互斥**的（`MEMORY-10`：客户端直连上游，不流经网关）⇒ 两个数字都对，
//   但含义不同。同名会让人以为是同一个数、进而互相“对不上”⇒ 这里改名为「客户端」。
  "wbOverview.stat.tokens": "客户端 Token（今日）",
  "wbOverview.stat.tokensHintClientOnly": "仅客户端自身用量，不含经本网关的请求",
  "wbOverview.stat.credits": "今日积分消耗",
  "wbOverview.stat.creditsRemaining": "剩余合计 {n}",
  "wbOverview.stat.gateway": "网关请求（今日）",
  "wbOverview.stat.gatewayHint": "统计自本机请求日志",
  "wbOverview.stat.gatewayTruncated": "日志已裁剪，实际更多",
  "wbOverview.stat.unavailable": "读不到数据",
  // 「对比昨日」**真算**；昨日无数据时**不给百分比**（不编假增长）。
  "wbOverview.today.up": "较昨日 +{pct}%",
  "wbOverview.today.down": "较昨日 -{pct}%",
  "wbOverview.today.flat": "与昨日持平",
  "wbOverview.today.noBaseline": "昨日无数据",

  // ---- 近 14 天 Token 用量（原型 `.split-main`，与统计行**同源**）----
  "wbOverview.trend.title": "近 14 天 Token 用量",
  "wbOverview.trend.caption": "近 14 天合计 {total} · 单日峰值 {peak}",
  "wbOverview.trend.aria": "近 14 天逐日 Token 用量柱状图",
  "wbOverview.trend.barTitle": "{date} · {tokens} Token · {records} 次调用",
  "wbOverview.trend.empty": "近 14 天没有用量记录",
  "wbOverview.trend.emptyHint": "产生调用后，这里会显示逐日 Token。",
  "wbOverview.trend.goUsage": "查看详情",
} as const;
