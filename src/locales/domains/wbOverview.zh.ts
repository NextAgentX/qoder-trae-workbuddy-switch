/** 概览工作台（PLAN §3.9 / P3 新增）。 */
export const zh = {
  "wbOverview.title": "概览",
  "wbOverview.refresh": "刷新",

  "wbOverview.tile.accounts": "账号",
  "wbOverview.tile.pending": "待处理",
  "wbOverview.tile.client": "客户端",
  "wbOverview.running": "运行中",
  "wbOverview.stopped": "未运行",

  "wbOverview.pendingTitle": "需要你处理",
  "wbOverview.pending.relogin": "{count} 个账号需要重新登录",
  "wbOverview.pending.reloginHint": "登录态已失效，切到该账号时会失败。去账号管理里重新登录。",
  "wbOverview.pending.expiring": "{count} 个账号的登录态即将过期",
  "wbOverview.pending.expiringHint": "24 小时内到期。到期后会退化成「需要重新登录」。",

  /* 没有待处理项是**好消息**，所以空态不是「暂无数据」而是「一切正常」+ 出口。 */
  "wbOverview.allGood": "一切正常",
  "wbOverview.allGoodHint": "没有需要处理的账号。",
  "wbOverview.allGoodAction": "去账号管理",

  // ---- 今日用量摘要（P3 新增，与用量页**同源**）----
  "wbOverview.today.title": "今日用量",
  "wbOverview.today.tokens": "今日 Token",
  "wbOverview.today.records": "今日调用",
  "wbOverview.today.vsYesterday": "对比昨日",
  "wbOverview.today.up": "较昨日 +{pct}%",
  "wbOverview.today.down": "较昨日 -{pct}%",
  "wbOverview.today.flat": "与昨日持平",
  "wbOverview.today.noBaseline": "昨日无数据",
  "wbOverview.today.empty": "今日暂无用量",
  "wbOverview.today.emptyHint": "产生调用后，这里会显示今日 Token 与对比。",
  "wbOverview.today.goUsage": "查看用量",
} as const;
