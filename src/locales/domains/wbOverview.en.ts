/** 概览工作台（PLAN §3.9 / P3 新增）。 */
export const en = {
  "wbOverview.title": "Overview",
  "wbOverview.refresh": "Refresh",

  "wbOverview.pendingTitle": "Needs your attention",
  "wbOverview.pending.count": "{n} items",
  "wbOverview.pending.relogin": "{count} account(s) need to sign in again",
  "wbOverview.pending.reloginHint": "The session expired; switching to them will fail. Sign in again from Accounts.",
  // ★ New in the prototype `viewOverview` (`buddy-switch-prototype.html:2065-2067`).
  // ⚠️ Kept distinct from "need to sign in again": expired = the **credential** is dead,
  //    relogin = the **session** was rejected.
  "wbOverview.pending.expired": "{count} account(s) have expired credentials",
  "wbOverview.pending.expiredHint": "Credentials expired; switching to them fails. Sign in again from Accounts.",
  "wbOverview.pending.notChecked": "{count} account(s) not checked in today",
  "wbOverview.pending.notCheckedHint": "Not checked in yet today. Check them in right here, or handle them one by one in Accounts.",
  "wbOverview.pending.expiringCredits": "{count} account(s) have credits expiring soon",
  "wbOverview.pending.expiringCreditsHint": "{amount} credits expire soon and become unusable afterwards.",
  "wbOverview.pending.expiring": "{count} account(s) expire soon",
  "wbOverview.pending.expiringHint": "Expires within 24 hours. After that it becomes “needs sign-in”.",
  "wbOverview.pending.notRunning": "The client is not running",
  "wbOverview.pending.notRunningHint": "Switching accounts requires the client to be running.",
  "wbOverview.pending.noAccounts": "This scope has no accounts yet",
  "wbOverview.pending.noAccountsHint": "Add an account to get started.",
  "wbOverview.pending.action.handle": "Fix",
  "wbOverview.pending.action.settings": "Open settings",
  "wbOverview.pending.action.addAccount": "Add account",
  // The "not checked in today" row runs an action in place (prototype `run: "batch-checkin"`).
  "wbOverview.pending.action.checkin": "Check in all",
  "wbOverview.toast.checkinLoading": "Checking in…",
  "wbOverview.toast.checkinOk": "Check-in complete",
  "wbOverview.toast.checkinPartial": "Some accounts failed to check in",
  "wbOverview.toast.checkinFail": "Check-in failed",
  "wbOverview.toast.checkinSummary": "{ok} succeeded · {already} already done · {failed} failed",
  "wbOverview.toast.checkinRetryFailed": "Retry failed only",

  "wbOverview.otherScopes.title": "Other scopes",
  "wbOverview.otherScopes.loggedIn": "Signed in as {name} · {n} accounts",
  "wbOverview.otherScopes.loggedOut": "Not signed in · {n} accounts",
  "wbOverview.otherScopes.missing": "Not detected · {n} accounts",
  "wbOverview.capabilities.title": "Capabilities",

  "wbOverview.allGood": "All good",
  "wbOverview.allGoodHint": "Nothing needs your attention.",
  "wbOverview.allGoodAction": "Go to Accounts",

  "wbOverview.stat.aria": "Overview statistics",
  "wbOverview.stat.accounts": "Accounts",
  "wbOverview.stat.accountsAttention": "{n} need attention",
  "wbOverview.stat.accountsOk": "All healthy",
  // ★ Q3: this tile counts **client-side** tokens (`get_token_statistics` over session logs),
//   while the account card's "Tokens today" also includes the **gateway**
//   (`get_account_token_today`). The two sources are **disjoint** (the client talks to the
//   upstream directly), so both numbers are correct but mean different things.
//   Same name ⇒ users assume they should match ⇒ renamed to "client".
  "wbOverview.stat.tokens": "Client tokens (today)",
  "wbOverview.stat.tokensHintClientOnly": "Client's own usage only; excludes requests via this gateway",
  "wbOverview.stat.credits": "Credits spent today",
  "wbOverview.stat.creditsRemaining": "{n} remaining",
  "wbOverview.stat.gateway": "Gateway requests (today)",
  "wbOverview.stat.gatewayHint": "Counted from local request logs",
  "wbOverview.stat.gatewayTruncated": "Logs were trimmed; the real count is higher",
  "wbOverview.stat.unavailable": "Unavailable",
  "wbOverview.today.up": "+{pct}% vs yesterday",
  "wbOverview.today.down": "-{pct}% vs yesterday",
  "wbOverview.today.flat": "Flat vs yesterday",
  "wbOverview.today.noBaseline": "No data yesterday",

  "wbOverview.trend.title": "Token usage, last 14 days",
  "wbOverview.trend.caption": "Last 14 days total {total} · daily peak {peak}",
  "wbOverview.trend.aria": "Daily token usage for the last 14 days",
  "wbOverview.trend.barTitle": "{date} · {tokens} tokens · {records} calls",
  "wbOverview.trend.empty": "No usage in the last 14 days",
  "wbOverview.trend.emptyHint": "Once calls are made, daily tokens appear here.",
  "wbOverview.trend.goUsage": "View usage",
} as const;