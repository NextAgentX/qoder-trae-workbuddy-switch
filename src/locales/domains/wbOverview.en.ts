/** 概览工作台（PLAN §3.9 / P3 新增）。 */
export const en = {
  "wbOverview.title": "Overview",
  "wbOverview.refresh": "Refresh",

  "wbOverview.tile.accounts": "Accounts",
  "wbOverview.tile.pending": "Needs action",
  "wbOverview.tile.client": "Client",
  "wbOverview.running": "Running",
  "wbOverview.stopped": "Not running",

  "wbOverview.pendingTitle": "Needs your attention",
  "wbOverview.pending.relogin": "{count} account(s) need to sign in again",
  "wbOverview.pending.reloginHint": "The session expired; switching to them will fail. Sign in again from Accounts.",
  "wbOverview.pending.expiring": "{count} account(s) expire soon",
  "wbOverview.pending.expiringHint": "Expires within 24 hours. After that it becomes “needs sign-in”.",

  "wbOverview.allGood": "All good",
  "wbOverview.allGoodHint": "Nothing needs your attention.",
  "wbOverview.allGoodAction": "Go to Accounts",

  "wbOverview.today.title": "Today's usage",
  "wbOverview.today.tokens": "Tokens today",
  "wbOverview.today.records": "Calls today",
  "wbOverview.today.vsYesterday": "vs yesterday",
  "wbOverview.today.up": "+{pct}% vs yesterday",
  "wbOverview.today.down": "-{pct}% vs yesterday",
  "wbOverview.today.flat": "Flat vs yesterday",
  "wbOverview.today.noBaseline": "No data yesterday",
  "wbOverview.today.empty": "No usage today",
  "wbOverview.today.emptyHint": "Once calls are made, today's tokens and comparison appear here.",
  "wbOverview.today.goUsage": "View usage",
} as const;
