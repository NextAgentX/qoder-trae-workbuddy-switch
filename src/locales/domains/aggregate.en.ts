/**
 * 文案域：**聚合账号面板**（英文，与 `aggregate.zh.ts` 键位一一对应）。
 *
 * ⚠️ 键位对齐由 `scripts/check-scope-registry.cjs` 钉死 —— 少一个键 `npm run build` 直接红。
 * 措辞取向：英文不逐字直译中文的破折号长句，改成分号 / 短句（英文长句更难读）；
 * 但**语义强度必须一致**（「不删除、不修改任何账号」这种承诺不能省）。
 */
export const en = {
  // ---- Entry inside the top-bar scope menu (D1) ----
  "aggregate.scopeMenu.group": "All",
  "aggregate.scopeMenu.entry": "Aggregate",
  "aggregate.scopeMenu.entryName": "Aggregate · All accounts",
  "aggregate.scopeMenu.entrySub": "{n} accounts · across {scopes} scopes",
  /* The total isn't known yet ⇒ **don't show a number** (showing 0 would be a lie). */
  "aggregate.scopeMenu.entrySubUnknown": "across {scopes} scopes",
  "aggregate.scopeMenu.entryAttention": "{n} need attention",
  "aggregate.scopeMenu.hint":
    "The scope decides which account set you are managing. Pick “Aggregate” to see all {scopes} scopes at once; pick any scope to go back to its accounts page.",

  // ---- Top bar run state ----
  "aggregate.topbar.runningScopes": "{n} scopes running",

  // ---- Toolbar ----
  "aggregate.toolbar.searchPlaceholder": "Filter accounts (name / remark / email)",
  "aggregate.toolbar.searchAria": "Filter accounts across all scopes",
  "aggregate.toolbar.filter": "Filter",
  "aggregate.toolbar.filterCount": "Filter · {n}",
  "aggregate.toolbar.group": "Group by scope",
  "aggregate.toolbar.flat": "Flat",
  "aggregate.toolbar.groupHint": "Group by scope: find accounts of one channel, then act in place",
  "aggregate.toolbar.flatHint": "Sort by urgency: sweep everything that needs attention",
  "aggregate.toolbar.refresh": "Refresh all scopes",
  "aggregate.count.total": "{n} accounts · {scopes} scopes",
  "aggregate.count.sorted": "{n} accounts · sorted by urgency",
  "aggregate.count.filtered": "Filtered · showing {shown} / {total}",
  "aggregate.count.showAll": "Show all",

  // ---- Groups ----
  "aggregate.group.count": "{n} accounts",
  "aggregate.group.attention": "{n} need attention",
  "aggregate.group.failed": "Failed to load",
  "aggregate.group.selectAll": "Select group",
  "aggregate.note.noAccounts": "This scope has no accounts yet",
  "aggregate.note.noClient": "No {client} client detected on this machine, so there is nothing to manage here",

  // ---- Load failure (red line: one group failing must not take down the page) ----
  "aggregate.fail.title": "Failed to load accounts",
  "aggregate.fail.desc": "Request timed out · other scopes are unaffected; retry this one alone",
  "aggregate.fail.retry": "Retry",

  // ---- Empty states (the three kinds of "empty" must stay distinguishable) ----
  "aggregate.empty.filtered.title": "No accounts match the current filter",
  "aggregate.empty.filtered.desc": "The filter excludes all {n} accounts. Clear it to see everything again.",
  "aggregate.empty.filtered.action": "Show all accounts",
  "aggregate.empty.none.title": "No accounts yet",
  "aggregate.empty.none.desc": "Add an account in any scope and the aggregate view will pick it up automatically.",
  "aggregate.empty.none.action": "Open scope menu",
  "aggregate.empty.search.title": "No matching accounts",
  "aggregate.empty.search.desc": "Try another keyword, or clear the search box.",
  "aggregate.empty.search.action": "Clear search",
  "aggregate.loading": "Reading accounts from {n} scopes…",

  // ---- Filter panel ----
  "aggregate.filter.title": "Filter what is shown",
  "aggregate.filter.hint": "Only changes which cards are rendered here. No account is deleted or modified. Selecting everything equals “show all”.",
  "aggregate.filter.all": "All accounts",
  "aggregate.filter.selected": "{n} / {total} selected",
  "aggregate.filter.selectedCount": "Selected {n} / {total}",
  "aggregate.filter.selectedAll": "Selected {n} / {total} · equals showing all",
  "aggregate.filter.expand": "Expand",
  "aggregate.filter.collapse": "Collapse",
  "aggregate.filter.apply": "Apply",
  "aggregate.filter.cancel": "Cancel",
  "aggregate.filter.noAccounts": "No accounts",
  "aggregate.filter.noClient": "Client not installed",
  "aggregate.filter.toggleAria": "Toggle every account in {scope}",
  "aggregate.filter.expandAria": "Expand or collapse the accounts in {scope}",

  // ---- Q3 option C: newly added accounts ----
  "aggregate.newcomer.title": "{n} accounts are not in the current filter",
  "aggregate.newcomer.note": "newly added",
  "aggregate.newcomer.add": "Add to filter",
  "aggregate.newcomer.ignore": "Ignore",

  // ---- Batch bar (button labels reuse `shared.batch.*`) ----
  "aggregate.batch.selected": "{n} accounts selected",
  "aggregate.batch.acrossScopes": "across {n} scopes",
  "aggregate.batch.acrossScopesHint": "Batch actions apply to these scopes",

  // ---- Account "more" menu header ----
  "aggregate.menu.head": "{name} · {scope}",

  // ---- Toasts ----
  "aggregate.toast.entered": "Aggregate view opened",
  "aggregate.toast.enteredDesc": "{n} accounts from {scopes} scopes on one screen",
  "aggregate.toast.left": "Left the aggregate view · now in {scope}",
  "aggregate.toast.filtered": "Filtered: showing {shown} / {total} accounts",
  "aggregate.toast.filteredDesc": "This filter is kept after you restart the app",
  "aggregate.toast.showAll": "Showing all accounts",
  "aggregate.toast.filterCleared": "Filter preference cleared",
  "aggregate.toast.newcomerAdded": "Added to the filter (your existing filter is kept)",
  "aggregate.toast.retried": "Reloaded {scope}",
  "aggregate.toast.refreshed": "Refreshed {n} scopes",
  "aggregate.toast.refreshedDesc": "Status of {n} accounts updated",
  "aggregate.toast.guideLeavesAggregate": "⚠️ This leaves the aggregate view (diagnostics is a per-scope page)",

  // ---- Detail drawer (assembly lives in `lib/aggregate-detail.ts`) ----
  "aggregate.detail.checkin": "Check-in",
  "aggregate.detail.checkedIn": "Checked in today",
  "aggregate.detail.notCheckedIn": "Not checked in today",
  "aggregate.detail.lastUsed": "Last used",
  "aggregate.detail.sectionIdentity": "Identity",
  "aggregate.detail.sectionCredential": "Credential",
  "aggregate.detail.email": "Email",
  "aggregate.detail.remark": "Note",
  "aggregate.detail.group": "Group",
  "aggregate.detail.domain": "Domain",
  "aggregate.detail.device": "Device fingerprint",
  "aggregate.detail.status": "Status",
  "aggregate.detail.jwt": "Login state (JWT)",
  "aggregate.detail.cooldown": "Cooling down until",
  "aggregate.detail.expiresAt": "Credential expires",
  "aggregate.detail.refreshExpiresAt": "Refresh token expires",
  "aggregate.detail.credentialSummary": "Credential summary",
  "aggregate.detail.credentialOk": "Valid",
  "aggregate.detail.credentialExpiring": "Expiring soon",
  "aggregate.detail.credentialExpired": "Expired",
  "aggregate.detail.credentialRelogin": "Sign-in required",
  "aggregate.detail.credentialMissing": "Credential missing",
  "aggregate.detail.unknown": "Unknown",
  "aggregate.detail.packFallbackName": "Credit pack",
  "aggregate.detail.groupSaved": "Group saved",

  // ---- Export (per scope: the export command is single-scope) ----
  "aggregate.export.button": "Export",
  "aggregate.export.none": "No accounts to export",
  "aggregate.export.pickScope": "Pick the scope to export",
  "aggregate.export.count": "{n} accounts",
  "aggregate.export.platform.workbuddy": "WorkBuddy",
  "aggregate.export.platform.trae": "Trae",
  "aggregate.export.platform.qoder": "Qoder",
  "aggregate.export.done": "Exported {n} accounts",
  "aggregate.toast.switching": "Switching…",
  // Delete confirmation (issue #89 · 2.3): the undo toast is a remedy, not a substitute
  // for asking first.
  "aggregate.delete.title": "Delete account",
  "aggregate.delete.body": "Delete “{name}”? You can restore it right away with the “Undo” action in the toast.",
  "aggregate.delete.batchBody": "Delete the {n} selected accounts? You can restore them right away with the “Undo” action in the toast.",
  "aggregate.delete.cancel": "Cancel",
  "aggregate.delete.confirm": "Delete",

  // ---- Batch action results ----
  "aggregate.toast.checkinDone": "Checked in {n} accounts",
  "aggregate.toast.checkinPartial": "Check-in finished: {ok} succeeded · {fail} failed",
  "aggregate.toast.deletePartial": "{fail} accounts could not be deleted (the rest are in the recycle bin)",
};
