/**
 * 文案域：**跨切面共享文案**（英文）。
 *
 * ⚠️ 不写类型注解：键名合法性由 `src/locales/en.ts` 单点校验。域文件保持零依赖。
 * 键必须与 `shared.zh.ts` **一一对应**（缺哪条就回落中文）。
 */
export const en = {
  // ---- Version regions (region.ts / trae-types.ts / trae-variant-status.ts) ----
  "shared.region.version.cn": "CN version",
  "shared.region.version.global": "Global version",
  "shared.region.gateway.cn": "CN version (WorkBuddy)",
  "shared.region.gateway.global": "Global version (WorkBuddy AI)",
  "shared.region.filter.all": "Combined",

  // ---- Statistics scope bar (region-bar.tsx) ----
  /* P3: `shared.region.scope.aria` removed (region selector moved to the sidebar). */
  "shared.region.combine.aria": "Combine both regions",
  "shared.region.combine.label": "Combine both regions",

  // ---- Gateway account pool status (gateway.ts — one card shared by both sides) ----
  "shared.poolStatus.available": "Available",
  "shared.poolStatus.cooling": "Cooling",
  "shared.poolStatus.disabled": "Session expired",
  "shared.poolStatus.expired": "Credits expired",
  "shared.poolStatus.noCredits": "No credits",

  // ---- Gateway account pool card (components/gateway/account-pool-card.tsx) ----
  "shared.gateway.pool.title": "Account pool",
  /* P3: snippet field labels are shared (previously one set per platform). */
  /* P11: `snippet.apiBase` / `snippet.apiKey` removed — the `cherry` snippet that used them was
     dropped with the tool-set change (see `shared.gateway.access.*`). "Base URL" / "API Key" are
     generic parameter names (English contracts), hardcoded in the snippet, not in the word list. */
  "shared.gateway.snippet.model": "Model",
  "shared.gateway.snippet.modelPlaceholder": "Pick a model from the list",

  // =====================================================================
  // ---- Access guide (P11 §1.7) — protocol × tool, shared by both platforms ----
  // =====================================================================
  // ⚠️ Snippet key names (`ANTHROPIC_BASE_URL` / `base_url` / `env_key` …) are contracts read by
  //    external tools and stay hardcoded in English; only natural-language labels live here.
  "shared.gateway.access.protocol.openai": "OpenAI-compatible",
  "shared.gateway.access.protocol.anthropic": "Anthropic-compatible",
  "shared.gateway.access.tool.genericOpenai": "Other OpenAI-compatible tools",
  "shared.gateway.access.tool.genericAnthropic": "Other Anthropic-compatible tools",
  // `{openai}` / `{anthropic}` are the English endpoint contracts.
  "shared.gateway.access.endpointNote":
    "Protocol endpoints: {openai} (OpenAI) · {anthropic} (Anthropic) — both are supported, including streaming.",
  "shared.gateway.access.authHeaderLabel": "Auth header",
  "shared.gateway.access.authNote": "Auth header: {header}",
  // Codex (OpenAI protocol) — snippet key names (`base_url` / `env_key`) stay English.
  "shared.gateway.access.codex.title": "Codex (OpenAI protocol) — custom model_provider",
  "shared.gateway.access.codex.envKeyNote": "environment variable name; set its value to your key",
  "shared.gateway.access.codex.docNote": "see the official Codex docs for the remaining fields",
  // opencode — snippet key names (`baseURL` / `apiKey`) stay English.
  "shared.gateway.access.opencode.title": "opencode — provider config",
  "shared.gateway.access.opencode.docNote": "see the official opencode docs for field names",
  // generic model line hint; `{url}` is the English endpoint contract (`…/v1/models`).
  "shared.gateway.access.generic.modelHint": "auto-discovered from GET {url}",
  /* Shared strings for the model-list "source bar" (components/gateway/catalog-source-bar.tsx).
     Only the parts identical across both platforms live here; source vocabulary and button
     labels are injected by the caller (WB three levels / Trae "client cache / not loaded"). */
  "shared.gateway.sourceBar.sourceLabel": "Source",
  "shared.gateway.sourceBar.identifiedAt": "Last identified {time}",
  "shared.gateway.sourceBar.neverIdentified": "Never identified",
  /* ⚠️ Only "identified" — never "N models available" (HTTP 200 ≠ usable; §3.17.3 pitfall 3 / D24). */
  "shared.gateway.sourceBar.count": "{n} models identified",
  "shared.gateway.sourceBar.autoHint": "Identified automatically after connecting — no need to fill in model names",
  "shared.gateway.sourceBar.justNow": "just now",
  "shared.gateway.sourceBar.minutesAgo": "{n} min ago",
  "shared.gateway.sourceBar.hoursAgo": "{n} h ago",
  "shared.gateway.sourceBar.daysAgo": "{n} d ago",
  /* Model table (P11 §1.6) — components/gateway/model-table.tsx, shared by both platforms.
     Columns = model / upstream / gateway exposure; no "scope" column (#1), no "protocol" column (#2). */
  "shared.gateway.models.colModel": "Model",
  "shared.gateway.models.colUpstream": "Upstream",
  "shared.gateway.models.colExposed": "Gateway exposure",
  "shared.gateway.models.upstreamAvailable": "Available",
  "shared.gateway.models.upstreamUnavailable": "Unavailable",
  "shared.gateway.models.exposed": "Exposed",
  "shared.gateway.models.disabled": "Disabled",
  "shared.gateway.models.disable": "Disable",
  "shared.gateway.models.enable": "Enable",
  "shared.gateway.models.count": "{served} of {total} exposed",
  /* ★ Both exits take effect together — otherwise the disable switch is fake (§3.18.1). */
  "shared.gateway.models.twoExits":
    "A disabled model does not appear in `/v1/models`, and calling it directly is rejected with an explanation — both exits take effect together, otherwise the switch is fake.",
  "shared.gateway.models.disabledToast": "Disabled “{name}”",
  "shared.gateway.models.enabledToast": "Enabled “{name}”",
  "shared.gateway.models.toggleFail": "Action failed",
  "shared.gateway.pool.totalRequests": "{count} requests",
  "shared.gateway.pool.tile.available": "Routable",
  "shared.gateway.pool.tile.cooling": "Cooling",
  "shared.gateway.pool.tile.disabled": "Session dead",
  "shared.gateway.pool.tile.expired": "Credits expired",
  "shared.gateway.pool.tile.zeroCredits": "No credits",
  "shared.gateway.pool.empty": "The pool is empty. Add accounts under Accounts first.",
  "shared.gateway.pool.credits": "{credits} credits",
  "shared.gateway.pool.diagnoseNote": "When a request reports \"no available account\", check these reasons one by one:",
  "shared.gateway.pool.preferredLabel": "Preferred account",
  "shared.gateway.pool.preferredAria": "Account to consume first",
  "shared.gateway.pool.preferredAuto": "Auto (best available)",
  // WorkBuddy only: the second non-account option (writes `AccountStrategy::MaxCredits`).
  "shared.gateway.pool.strategyMaxCredits": "Pick by live credits (queried per request)",
  "shared.gateway.pool.preferredMissing": "Preferred account (not in this pool)",
  // Preference, not a hard pin: falls back when the preferred account can't serve.
  "shared.gateway.pool.preferredHint": "A preference, not a hard pin: if it is cooling, dead, or out of credits, another account is used instead — requests are never rejected.",

  // ---- Clipboard (clipboard.ts) ----
  "shared.clipboard.copied": "Copied",
  "shared.clipboard.failed": "Copy failed — select the text and copy it manually",

  // ---- Desktop companion window (components/companion/*, a separate window) ----
  // The companion is a separate webview (`?window=companion`, see the entry split in
  // `main.tsx`) sharing this word list. State labels / read-only footnote / empty state /
  // a11y labels come in T03; T01 only adds the title key the skeleton actually uses.
  "shared.companion.title": "Session status",
  // Sidebar entry (App.tsx `CompanionToggle`): tooltip + accessible label.
  // It is one of the two faces of the same switch as the tray item (both converge on
  // `companion_set_visible`), so it shares the tray's wording.
  "shared.companion.entry": "Session companion",
  "shared.companion.badge.waiting": "{count} awaiting",
  "shared.companion.footnote": "Read-only · Click to return to the session · Hover to reveal the title",
  "shared.companion.close": "Hide the companion window",
  "shared.companion.state.running": "Running",
  "shared.companion.state.waiting": "Awaiting",
  "shared.companion.state.done": "Done",
  "shared.companion.state.failed": "Failed",
  "shared.companion.row.aria": "{source} · {state} · {hint}",
  "shared.companion.info.source": "Source",
  "shared.companion.info.time": "Time",
  "shared.companion.jump.ok": "Focused {source}",
  "shared.companion.jump.failed": "Couldn't focus the source: {message}",
  "shared.companion.empty.title": "No sessions",
  "shared.companion.empty.description": "Statuses appear here after you start a task in a client.",
  "shared.companion.empty.refresh": "Refresh",
  "shared.companion.error.title": "Couldn't load session status",
  "shared.companion.loading": "Loading session status",

  // ---- Demo mode guard (demo-action.tsx) ----
  "shared.demo.unavailable": "Not available in demo mode",

  "shared.api.unsupportedInWebui": "Not supported in webui mode: {cmd}",
  "shared.api.unreachable":
    "Cannot reach the Buddy Switch service ({base}). Start it with `buddy-switch` first.",
  "shared.api.requestFailed": "Request failed ({status})",
  "shared.api.webuiPermissionByProcess":
    "In webui mode permissions come from the service process (started in a terminal); no extra authorization is needed",
  "shared.api.accountNotFound": "Account not found",

  // =====================================================================
  // ---- Demo fixtures (screenshot-demo.ts: fake data for the demo/screenshot site) ----
  // =====================================================================
  // Model names, protocol keys, paths and uids stay out of the word list — they are contracts.
  // ---- Fake accounts ----
  "shared.demo.account.a": "Test A",
  "shared.demo.account.aRemark": "DS4.1 quota unlocks on 10/03",
  "shared.demo.account.b": "Test B",
  "shared.demo.account.c": "Test C",
  // ---- Credit package names ----
  "shared.demo.pack.fission": "CodeBuddy Personal CN Growth Pack",
  "shared.demo.pack.credits": "CodeBuddy Personal Credits Pack",
  "shared.demo.pack.trial": "CodeBuddy New User Pack",
  "shared.demo.pack.checkin": "CodeBuddy Check-in Bonus Credits",
  "shared.demo.pack.activity": "CodeBuddy Campaign Reward Credits",
  // ---- Auto travel ----
  "shared.demo.travel.cafe": "Cafe",
  "shared.demo.travel.gym": "Gym",
  // ---- Auto-rotate log ----
  "shared.demo.rotate.reasonPinned": "The current account is still the available one with the most urgent credit expiry",
  "shared.demo.rotate.reasonExpiring": "The target account's credits expire within 5 days",
  // ---- Token statistics ----
  "shared.demo.session.tokenDashboard": "Polish the token statistics dashboard and local usage analysis",
  "shared.demo.session.agentAcp": "Design Agent and ACP management settings",
  "shared.demo.session.characterAudio": "Complete the character idiom dual audio",
  "shared.demo.session.accountCard": "Unify account card visuals and interactions",
  "shared.demo.session.replicaAlpha": "Rework replica ownership computation",
  "shared.demo.session.replicaBeta": "Unify the cross-account session copy entry",
  // ---- Model catalog ----
  "shared.demo.catalog.limitedFree": "Limited-time free",
  "shared.demo.catalog.promo": "Promo",
  "shared.demo.catalog.globalStale": "The upstream API may have changed; showing the last successful cache",
  // ---- Account strategy ----
  "shared.demo.strategy.realtime": "Chosen at request time",
  // Demo data for the WorkBuddy gateway account pool (`demoGatewayStatus`).
  "shared.demo.pool.reasonRateLimited": "Rate limited",
  "shared.demo.pool.reasonSessionDead": "Session expired",
  "shared.demo.pool.diagnose": "{name} ({tail}: {reason}, credits={credits})",
  "shared.demo.pool.creditsUnknown": "unknown",
  // ---- Trae: fake account names / group / packages / cooldown ----
  "shared.demo.trae.name.main": "Primary",
  "shared.demo.trae.remark.main": "Primary: keep for check-in credits, not long tasks",
  "shared.demo.trae.name.altA": "Alt A",
  "shared.demo.trae.name.altB": "Alt B",
  "shared.demo.trae.group.spare": "Spare",
  "shared.demo.trae.pack.month": "Work Monthly Pack",
  "shared.demo.trae.pack.checkinBonus": "Check-in Bonus Pack",
  "shared.demo.trae.pack.cnPlan": "CN Plan Pack",
  "shared.demo.trae.pack.trial": "Trial Pack",
  "shared.demo.trae.cooldown.rateLimited": "Too many requests, please try again later",
  "shared.demo.trae.cooldown.sessionDead": "Session expired, please sign in again",
  "shared.demo.trae.cooldown.reasonRateLimited": "Rate limited",
  "shared.demo.trae.cooldown.reasonSessionDead": "Session expired",
  // ---- Trae: check-in ----
  "shared.demo.trae.checkin.ok": "Check-in succeeded",
  "shared.demo.trae.checkin.already": "Already checked in today",
  "shared.demo.trae.checkin.sessionDead": "Session expired, please sign in again",
  "shared.demo.trae.warn.altBSessionDead": "Alt B's session expired; sign in again before checking in",
  // ---- Trae: program display names (`label` / `nameAlias`) ----
  "shared.demo.trae.program.work": "TraeWork",
  "shared.demo.trae.program.workCn": "TraeWork CN",
  "shared.demo.trae.program.code": "TraeCode",
  "shared.demo.trae.program.codeCn": "TraeCode CN",
  "shared.demo.trae.program.workAi": "TraeWork AI",
  "shared.demo.trae.program.workGlobal": "TraeWork",
  "shared.demo.trae.program.codeAi": "Trae AI",
  "shared.demo.trae.program.codePending": "TraeCode (untested)",
  // ---- Trae: API key names / gateway diagnostics / logs ----
  "shared.demo.trae.key.cursorCn": "Cursor (TraeWork)",
  "shared.demo.trae.key.cherryGlobal": "Cherry (TraeWork AI)",
  "shared.demo.trae.key.continueCode": "Continue (TraeCode)",
  "shared.demo.trae.key.legacy": "Legacy key (migrated on upgrade)",
  "shared.demo.trae.diagnose.main": "Primary(7481920:available,credits=120)",
  "shared.demo.trae.diagnose.altA": "Alt A(7481999:cooling,credits=65)",
  "shared.demo.trae.diagnose.altB": "Alt B(7482044:session expired (sign in again),credits=8)",
  "shared.demo.trae.gatewayUpstreamError": "Account \"{name}\" failed upstream (HTTP {status})",
  "shared.demo.trae.tokenSource": "Trae API gateway",
  "shared.demo.trae.tokenNote": "Counts only calls through the local Trae gateway; chatting directly in the Trae IDE produces no records.",
  "shared.demo.trae.logNote": "Reads only plain-text runtime logs (app / checkin / switcher); gateway request logs live in the \"Gateway request logs\" tab.",
  "shared.demo.trae.log.app": "Runtime",
  "shared.demo.trae.log.checkin": "Check-in",
  "shared.demo.trae.log.switch": "Switch",
  // Runtime log lines (mirroring the exact `store::append_log` formats)
  "shared.demo.trae.log.checkinDone": "Check-in done: ok {ok}/already {already}/failed {failed}/total {total}",
  "shared.demo.trae.log.thawed": "Auto-thawed account {uid}: {credits} credits left, cooldown cleared",
  "shared.demo.trae.log.savedLogin": "Saved login state: user={uid} files={count}",
  "shared.demo.trae.log.jwtRefreshed": "JWT auto-refresh succeeded: user={uid} new expiry={hours}h",
  "shared.demo.trae.log.deviceReset": "Device identifier reset complete: {count} items applied",
  // ---- Trae: unsupported capabilities (greyed-out cards) ----
  "shared.demo.cap.travel": "Auto travel",
  "shared.demo.cap.travelReason": "The Trae client has no API for that activity, and this tool has no matching backend implementation.",
  "shared.demo.cap.cli": "CodeBuddy CLI / IDE integration",
  "shared.demo.cap.cliReason": "CodeBuddy belongs to the WorkBuddy ecosystem; the Trae section does not offer integration or switching for that client.",
  "shared.demo.cap.migration": "Session / memory / connector migration",
  "shared.demo.cap.migrationReason": "Trae's login state is a set of Cloud-IDE-JWT files; there are no session tree / memory / connector objects to migrate.",
  "shared.demo.cap.sessionTree": "Session list / copy session / switch progress stream",
  "shared.demo.cap.sessionTreeReason": "Trae account switching swaps a file-level snapshot; there is no session list or switch progress event stream.",
  "shared.demo.cap.officialByModel": "Official credit usage by model",
  "shared.demo.cap.officialByModelReason": "Trae credits come only from check-in snapshots; there is no data source for \"the request usage that produced these credits\".",
  "shared.demo.cap.cacheMetrics": "Cache read / write / hit rate",
  "shared.demo.cap.cacheMetricsReason": "Neither the Trae gateway logs nor the upload path has cache fields, and the upstream does not return them — there is nothing to record",
  "shared.demo.cap.projectDimension": "Statistics by project",
  "shared.demo.cap.projectDimensionReason": "The gateway log's project_id / session_id are freshly generated uuids per request and do not correspond to client projects",
  "shared.demo.cap.sessionCost": "Most expensive session",
  "shared.demo.cap.sessionCostReason": "No stable session identifier, so multiple requests cannot be merged into one session cost",
  // ---- Demo mode write-action replies ----
  "shared.demo.switchDone": "Demo switch completed",
  "shared.demo.updateTitle": "Update prompt demo",
  "shared.demo.error.accountMissing": "Account not found",
  "shared.demo.error.missingReadOnly": "Demo mode is missing read-only data: {command}",
  // ---- Punctuation & separators used for string concatenation ----
  "shared.punct.colon": ": ",
  "shared.punct.comma": ", ",
  "shared.punct.semicolon": "; ",
  "shared.punct.period": ". ",
  "shared.punct.openParen": " (",
  "shared.punct.closeParen": ")",

  // ---- Account detail drawer (components/account-detail-drawer.tsx, shared) ----
  "shared.drawer.close": "Close",
  "shared.drawer.loading": "Loading details…",
  "shared.drawer.remark.title": "Remark",
  "shared.drawer.remark.placeholder": "e.g. DS4.1 quota · unlocks 10/03",
  "shared.drawer.remark.aria": "Remark for {name}",
  "shared.drawer.remark.editAria": "Edit remark for {name}",
  "shared.drawer.remark.add": "Add remark",
  "shared.drawer.packs.title": "Credits / packs",
  // ★ B5: check-in credits (fields come from the **measured upstream response**).
  "shared.drawer.checkin.title": "Check-in",
  "shared.drawer.checkin.today": "Checked in today",
  "shared.drawer.checkin.streak": "{n}-day streak",
  "shared.drawer.checkin.total": "{n} credits total",
  "shared.drawer.programs.title": "Programs (read-only)",
  "shared.drawer.program.current": "Current account",
  "shared.drawer.program.installed": "Installed",
  "shared.drawer.program.noDataDir": "No data dir",
  "shared.drawer.program.missing": "Not detected",
  "shared.drawer.empty.title": "No details",
  "shared.drawer.empty.description": "No details to show for this account yet; re-check and try again.",

  // ---- Minimal batch bar (components/account-batch-bar.tsx, shared) ----
  "shared.batch.selected": "{n} selected",
  "shared.batch.checkin": "Check in",
  "shared.batch.refresh": "Refresh",
  "shared.batch.delete": "Delete",
  "shared.batch.clear": "Clear selection",
  "shared.batch.deleteTitle": "Delete accounts in bulk",
  "shared.batch.deleteDesc": "Delete the {n} selected accounts? You can restore them from the recycle bin.",
  "shared.batch.confirm": "Delete",
  "shared.batch.cancel": "Cancel",

  // ---- Account list toolbar (components/account-toolbar.tsx, shared) ----
  "shared.toolbar.searchPlaceholder": "Search accounts…",
  "shared.toolbar.searchAria": "Search accounts",
  "shared.toolbar.sortAria": "Sort by",
  "shared.toolbar.groupAria": "Group by",
  "shared.toolbar.viewAria": "Switch card / table view",
  "shared.toolbar.viewCard": "Cards",
  "shared.toolbar.viewTable": "Table",
  "shared.toolbar.viewTableDisabled": "Window is too narrow for the table view; widen it and try again",
  "shared.toolbar.selectAll": "Select all",
  "shared.toolbar.deselectAll": "Deselect all",

  // ---- Account list empty states (two distinct kinds — never mix) ----
  "shared.list.empty.none": "No accounts yet",
  "shared.list.empty.noneDesc": "Add or import accounts from this machine to manage them here.",
  "shared.list.empty.filtered": "No accounts match your filters",
  "shared.list.empty.filteredDesc": "Try another keyword, or clear the filters to see all accounts.",
  "shared.list.empty.clearFilters": "Clear filters",

  // ---- Account table (components/account-table.tsx, shared) ----
  "shared.table.selectAllAria": "Select all in list",
  "shared.table.selectRowAria": "Select {name}",
  "shared.table.openRowAria": "Open details for {name}",
};
