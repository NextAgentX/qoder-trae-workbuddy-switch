/**
 * 文案域：**应用外壳**（英文）。
 *
 * ⚠️ 这里**不写类型注解**：键名合法性由组合入口 `src/locales/en.ts` 单点校验
 * （它把整体约束为 `Partial<Record<TranslationKey, string>>`）。域文件保持零依赖，
 * 既避免循环导入，也让「拼错键名」只报一处。
 *
 * 译文约定（全仓一致）：
 * - 术语与客户端一致：account / check-in / credits / token / gateway / profile。
 * - **产品名不译**（WorkBuddy / Trae / Buddy Switch）。
 * - 中文的顿号、书名号按英文习惯改为逗号与引号 —— 照搬标点是最常见的「机翻味」。
 * - 语态用祈使/陈述，不堆砌 Please；长度尽量控制在中文的两倍内，避免挤破 220px 侧栏。
 */
export const en = {
  // ---- App shell / sidebar ----
  "app.name": "Buddy Switch",
  "app.demoBadge": "Demo",
  "product.workbuddy": "WorkBuddy",
  /* issue #44: "TraeWork" is the retired legacy program line (each region now ships a single
     merged "Trae" client). The sidebar must use the merged name. */
  "product.trae": "Trae",
  "product.qoder": "Qoder",
  "product.switchAria": "Switch product",
  "product.navAria": "{product} navigation",

  "nav.overview": "Overview",
  /* Short labels, mirroring the prototype's `DESTINATIONS` (buddy-switch-prototype.html:1139-1146). */
  "nav.accounts": "Accounts",
  "nav.usage": "Usage",
  "nav.tokenStats": "Token usage",
  "nav.credits": "Credits",
  "nav.apiService": "Gateway",
  "nav.settings": "Settings",

  "sidebar.version": "Version",
  // Brand block tagline (prototype `.brand-text i`, `buddy-switch-prototype.html:163`).
  "sidebar.brand.tagline": "Multi-platform account manager",
  "sidebar.running": "{product} running",
  "sidebar.notRunning": "{product} not running",
  "sidebar.update": "Update",

  // ---- Top app bar (redesign L1 scope + global actions) ----
  "topbar.scope.aria": "Switch scope",
  "topbar.scope.menuLabel": "Scopes (platform × region)",
  "topbar.scope.sub.loggedIn": "Signed in as {name} · {n} accounts",
  "topbar.scope.sub.loggedOut": "Not signed in · {n} accounts",
  "topbar.scope.sub.missing": "Not detected · {n} accounts",
  // Appended when a scope has accounts needing action (prototype `scope-menu`).
  "topbar.scope.sub.attention": " · {n} need action",
  // Footer note of the scope menu (prototype `scope-menu`): what a scope switch actually affects.
  "topbar.scope.hint": "The scope decides which set of accounts you manage. Switching updates the sidebar, content and stats.",
  "topbar.run.running": "Running",
  "topbar.run.stopped": "Not running",
  "topbar.run.missing": "Not detected",
  "topbar.search": "Search accounts, pages, or actions",
  "topbar.refresh": "Refresh current scope",
  // Refresh-complete toast (prototype `doRefresh` success note). Wording follows what the
  // top-bar refresh actually rereads: the account list + running status (no credits).
  "topbar.refreshDone": "Refreshed",
  "topbar.refreshDoneDesc": "{scope}: accounts and status updated",
  "topbar.refreshFailed": "Refresh failed",
  "topbar.theme": "Switch light/dark theme",
  "topbar.appMenu": "App settings",

  // ---- Page-head scope subtitles (redesign §3.2) ----
  // Read-only: the only scope switcher lives in the top bar (region is part of L1 scope).
  "pageHead.overview.signedIn": "{scope} · Signed in as {name}",
  "pageHead.overview.signedOut": "{scope} · Not signed in",
  "pageHead.accounts": "{scope} · {n} accounts",
  "pageHead.accounts.pending": "{scope} · {n} accounts · {m} need action",
  "pageHead.usage": "{scope} · Token and credit stats in one workbench",
  "pageHead.gateway": "{scope} · Expose the account pool as OpenAI / Anthropic compatible APIs",
  "pageHead.settings": "{scope} · Grouped by topic instead of one long scroll",
  "pageHead.sessions": "{scope} · One session with a copy in each account — who lags, who diverged, sync to whom",

  // ---- Platform capability labels (overview "Capabilities" card; from `Capability` in registry) ----
  "capability.checkin": "Auto check-in",
  "capability.travel": "Auto travel",
  "capability.credits": "Credits",
  "capability.oauth": "OAuth sign-in",
  "capability.gateway": "API gateway",
  "capability.autoRotate": "Auto rotate",
  "capability.sessionCopy": "Session copy",

  // ---- Command palette (⌘K, P6 §3.7) ----
  "palette.entry": "Search or jump",
  "palette.title": "Search or jump",
  "palette.description": "Search accounts, pages, and actions across scopes; press Enter to go.",
  "palette.aria": "Command palette",
  "palette.placeholder": "Search accounts, pages, or actions…",
  "palette.loading": "Loading accounts…",
  "palette.results": "{count} results",
  "palette.empty.title": "No matches",
  "palette.empty.description": "Try a different keyword, or clear the search to see everything.",
  "palette.empty.clear": "Clear search",
  "palette.group.page": "Pages",
  "palette.group.scope": "Scopes",
  "palette.group.account": "Accounts",
  "palette.group.action": "Actions",
  "palette.scope.current": "Current",
  "palette.account.inScope": "in {scope}",
  "palette.action.refresh": "Refresh current scope",
  "palette.action.addAccount": "Add account",
  "palette.action.openDataDir": "Open data folder",

  // ---- Keyboard shortcuts settings (P6 §3.1) ----
  "shortcuts.settings.title": "Keyboard shortcuts",
  "shortcuts.settings.label": "Enable scope shortcuts",
  "shortcuts.settings.description": "Use {mod} + 1…{n} to switch directly to a scope.",
  "shortcuts.settings.degraded": "More than {limit} scopes are available, so number shortcuts are disabled.",

  // ---- First-run hint (P6 §1.5) ----
  "firstRun.title": "Quick start",
  "firstRun.openPalette": "Press {shortcut} to open the command palette and search across scopes.",
  "firstRun.scopeShortcut": "Press {shortcut} to switch directly to a scope.",
  "firstRun.listSearch": "Press / on the accounts page to focus search.",
  "firstRun.dismiss": "Got it",

  // ---- Undo toast (P6 T04 §1.4) ----
  // Deletion is a soft delete (the account moves to the recycle bin) => "can undo".
  // Never "permanently deleted" (that is the `recycleBin.purge` semantics), and never
  // imply that a timeout means permanent deletion — the account stays in the recycle bin.
  "undo.deletedTitle": "Deleted — can undo",
  "undo.batchTitle": "{n} accounts deleted — can undo",
  "undo.action": "Undo",
  "undo.undone": "Restored",
  "undo.failedTitle": "Undo failed",

  // ---- App-level settings (settings page "General" + top-bar gear drawer; one implementation, two shells) ----
  "appSettings.drawer.more": "More",

  "appSettings.appearance.title": "Appearance",
  "appSettings.appearance.theme.label": "Theme",
  "appSettings.appearance.theme.description":
    "Use light, dark, or follow the system appearance",
  "appSettings.appearance.theme.aria": "Theme",
  "appSettings.theme.system": "System",
  "appSettings.theme.light": "Light",
  "appSettings.theme.dark": "Dark",

  "appSettings.language.title": "Language",
  "appSettings.language.label": "Interface language",
  "appSettings.language.description":
    "Applies immediately; the preference is stored on this device only.",
  "appSettings.language.zh": "简体中文",
  "appSettings.language.en": "English",

  "appSettings.startup.title": "Startup & tray",
  "appSettings.startup.silent.label": "Start minimized to tray at login",
  "appSettings.startup.silent.description":
    "Reflects the system login item directly; reopen from the tray later",
  "appSettings.startup.enabled": "Launch at login enabled",
  "appSettings.startup.disabled": "Launch at login disabled",
  "appSettings.tray.closeToTray.label": "Keep running in the tray when the window is closed",
  "appSettings.tray.closeToTray.description":
    "Closing the main window hides it to the tray and keeps running; turn this off to quit the app on close",
  "appSettings.tray.saveMismatch":
    "Setting not applied: the read-back value differs from the requested one. Try again.",

  "appSettings.update.title": "Updates",
  "appSettings.update.auto.label": "Check for updates automatically",
  "appSettings.update.auto.description":
    "Checks every 30 minutes; you can still check manually below when this is off",
  "appSettings.update.current": "Current version: ",
  "appSettings.update.source": "Public update source",
  "appSettings.update.openRelease": "Open GitHub Releases",
  "appSettings.update.openReleaseTitle": "Open GitHub Releases",
  "appSettings.update.proxy.label": "Update proxy",
  "appSettings.update.proxy.description":
    "Used only for GitHub update checks and installer downloads; leave empty to disable an explicit proxy.",
  "appSettings.update.proxy.placeholder": "e.g. http://127.0.0.1:7897",
  "appSettings.update.proxy.save": "Save proxy",
  "appSettings.update.proxy.saved": "Update proxy saved",
  "appSettings.update.proxy.cleared": "Update proxy disabled",
  "appSettings.update.proxy.invalid":
    "Invalid proxy address. Enter an HTTP/HTTPS URL, e.g. http://127.0.0.1:7897",
  "appSettings.update.check": "Check for updates",
  "appSettings.update.checkFailed": "Update check failed",
  "appSettings.update.foundTitle": "New version available",
  "appSettings.update.doneTitle": "Update check complete",
  "appSettings.update.foundDetail": "Version v{latest} is available (current v{current})",
  "appSettings.update.upToDate": "You are on the latest version v{current}",
  "appSettings.update.installNow": "Update now",
  /* The settings entry opens the notes dialog first instead of downloading (see UPDATE-NOTES-ARCH.md). */
  "appSettings.update.viewNotes": "View release notes",

  // ---- Settings sub-nav (P7 §3.9: 9 items, the 9th is "Backup & restore") ----
  "settings.subnav.aria": "Settings sections",
  "settings.subnav.general": "General",
  "settings.subnav.account": "Accounts & sign-in",
  "settings.subnav.checkin": "Check-in & credits",
  "settings.subnav.gateway": "Gateway",
  "settings.subnav.schedule": "Scheduled tasks",
  "settings.subnav.client": "Client",
  "settings.subnav.diagnostics": "Permissions & diagnostics",
  "settings.subnav.logs": "Logs",
  "settings.subnav.backup": "Backup & restore",

  // ---- Gateway workspace subnav (PLAN §3.9: 6 items, shared by both product lines) ----
  "gateway.subnav.aria": "Gateway sections",
  "gateway.subnav.overview": "Overview",
  "gateway.subnav.keys": "Keys",
  "gateway.subnav.models": "Models",
  "gateway.subnav.pool": "Account pool",
  "gateway.subnav.proxy": "Request proxy",
  "gateway.subnav.logs": "Request logs",

  // ---- 统一网关（第三个产品线 · 跨作用域）----
  "gateway.subnav.unified": "Unified gateway",
  /* Gateway **type** switch (the two types are orthogonal to the section list). */
  "gateway.type.label": "Gateway type",
  "gateway.type.independent": "Standalone",
  "gateway.type.unified": "Unified",
  "unifiedGateway.subnav.overview": "Overview",
  "unifiedGateway.subnav.namespaces": "Namespaces",
  "unifiedGateway.subnav.keys": "Keys",
  "unifiedGateway.subnav.pool": "Account pool",
  "gateway.keysBar.total": "{count} keys",
  "gateway.keysBar.hint": "{live} active · plaintext is shown once at creation",
  "unifiedGateway.verdict.ready": "Ready",
  "unifiedGateway.verdict.partial": "Partly unusable · {count}",
  "unifiedGateway.verdict.dead": "All unusable",
  "unifiedGateway.verdict.disabled": "Disabled",
  "unifiedGateway.verdict.noNamespace": "No namespace enabled",
  "unifiedGateway.verdict.readyHint": "All {count} namespaces have usable accounts behind them.",
  "unifiedGateway.verdict.partialHint": "No usable accounts behind: {list}",
  "unifiedGateway.verdict.deadHint": "None of the enabled namespaces has a usable account.",
  "unifiedGateway.verdict.disabledHint": "The unified gateway is off and nothing is listening. Settings below are still saved and take effect once enabled.",
  "unifiedGateway.verdict.noNamespaceHint": "Enable at least one namespace, or every call returns 400.",
  "unifiedGateway.verdict.enabled": "Enabled",
  "unifiedGateway.verdict.enabledHint": "Configuration is on; pool data is still loading, so no availability verdict yet.",
  "unifiedGateway.whichGateway": "One platform only? Use Standalone, which follows the scope in the top bar. One provider across every platform and version? Use this unified gateway.",
  "unifiedGateway.guideTitle": "Integration",
  "unifiedGateway.guideOpenai": "OpenAI compatible",
  "unifiedGateway.guideAnthropic": "Anthropic compatible",
  "unifiedGateway.guideBase": "Base URL: {openai} for OpenAI, {anthropic} for Anthropic (no /v1 — the client appends it).",
  "unifiedGateway.guideModels": "Model names must carry a namespace prefix (for example {sample}); that is exactly what /v1/models discovery returns.",
  "unifiedGateway.guideResponses": "/v1/responses is available for WorkBuddy and Qoder namespaces (Qoder translates it to chat inside the gateway); Trae namespaces return 501.",
  "unifiedGateway.guideKey": "One unified key can authorise several namespaces — see the Keys section.",
  "unifiedGateway.bareModel": "Bare model name",
  "unifiedGateway.bareModelHint": "A model name without a prefix is rejected, with the list of usable prefixes",
  "unifiedGateway.showErrors": "See error responses",
  "unifiedGateway.errorsTitle": "Unified gateway error responses",
  "unifiedGateway.errorsHint": "Every error carries a way out — a bare not supported leaves people guessing.",
  "unifiedGateway.err.bareModel.title": "Bare model name rejected",
  "unifiedGateway.err.bareModel.body": "The unified gateway requires a namespace prefix on the model name. Enabled here: wb-cn: / wb-global: / trae-cn: / trae-global: / qoder-cn: / qoder-global:.\nYou sent deepseek-v4.1-flash — use something like wb-cn:deepseek-v4.1-flash.",
  "unifiedGateway.err.forbidden.title": "Namespace not authorised",
  "unifiedGateway.err.forbidden.body": "This API key is not authorised for namespace trae-global (it only covers wb-cn and wb-global).\nTick it for that key under Gateway > Unified > Keys, or create another key.",
  "unifiedGateway.err.responses.title": "Trae namespace with /v1/responses",
  "unifiedGateway.err.responses.body": "Trae has no /v1/responses endpoint. Use /v1/chat/completions instead,\nor switch the prefix to wb-cn: or wb-global:.",
  "unifiedGateway.err.notEnabled.title": "Namespace not enabled",
  "unifiedGateway.err.notEnabled.body": "Namespace traework-cn is not enabled on this gateway (it is not ticked in the Namespaces section).",
  "unifiedGateway.err.oldPort.title": "Namespace prefix on the old port",
  "unifiedGateway.err.oldPort.body": "The model name wb-cn:deepseek-v4.1-flash looks like a unified gateway namespace prefix,\nbut this port (57891) does not parse namespaces. Use the unified gateway at http://127.0.0.1:57892/v1,\nor drop the prefix and use cn: / global: instead.",
  "unifiedGateway.notEnabled": "Currently disabled: settings below are still saved and take effect once enabled; nothing is listening right now.",
  "unifiedGateway.listening": "Listening",
  "unifiedGateway.notListening": "Not listening",
  "unifiedGateway.listeningHint": "Something is accepting connections on this port, so clients can connect now.",
  "unifiedGateway.notListeningHint": "The config is enabled but nothing is listening. Toggle the switch off and on again, or restart the app.",
  "unifiedGateway.addr": "Base URL",
  "unifiedGateway.copyBaseUrl": "Copy base URL",
  "unifiedGateway.loopback": "Loopback 127.0.0.1",
  "unifiedGateway.lan": "LAN 0.0.0.0",
  "unifiedGateway.allowLanTitle": "Allow LAN access?",
  "unifiedGateway.allowLanDesc": "On 0.0.0.0, any device on your network can call models through this machine and spend your account credits. The default is loopback only.",
  "unifiedGateway.confirmOpen": "Expose on LAN",
  "unifiedGateway.confirm": "Confirm",
  "unifiedGateway.nsCount": "{count} of {total} enabled",
  "unifiedGateway.pool.empty": "No namespace enabled",
  "unifiedGateway.pool.emptyHint": "Pools are picked by namespace — enable one in the Namespaces section first.",
  "unifiedGateway.pool.count": "{count} pools",
  "unifiedGateway.pool.totals": "{available} usable accounts / {total} total",
  "unifiedGateway.pool.readOnly": "Read-only summary — selection, stickiness and revival are set per platform",
  "unifiedGateway.pool.account": "Account",
  "unifiedGateway.pool.credits": "Credits",
  "unifiedGateway.pool.state": "State",
  "unifiedGateway.pool.available": "{count} usable",
  "unifiedGateway.pool.total": "{count} total",
  "unifiedGateway.pool.cooling": "{count} cooling",
  "unifiedGateway.pool.disabled": "{count} disabled",
  "unifiedGateway.pool.expired": "{count} expired",
  "unifiedGateway.pool.zeroCredits": "{count} zero-credit",
  "unifiedGateway.pool.modelCooling": "{count} model-cooling",
  // Orthogonal count (already included in "{count} available"): the account is fine,
  // it is just at its concurrency cap right now.
  "unifiedGateway.pool.inFlightFull": "{count} at in-flight cap",
  "unifiedGateway.pool.inFlightFullHint":
    "These accounts are healthy but at their concurrency cap (earlier requests are still running). They recover on their own in seconds; a request right now would get 503 pool_busy.",
  "unifiedGateway.pool.noAccount": "Empty pool — add an account on the platform that owns it before it can serve.",
  "unifiedGateway.pool.note": "The unified gateway has no pool of its own — it routes by the namespace prefix in the model name: wb-* to WorkBuddy, trae-* / traework-* / traecode-* to Trae, qoder-* to Qoder. This is a cross-platform summary; selection policy is per product line, so change it on the gateway page of that platform.",
  "unifiedGateway.nsPoolOf": "{total} total · pool {target}",
  "unifiedGateway.pool.show": "View pool",
  "unifiedGateway.pool.hide": "Hide pool",
  "unifiedGateway.health.ready": "Ready",
  "unifiedGateway.health.readyHint": "Usable accounts behind this segment, and models have been discovered.",
  "unifiedGateway.health.noAccount": "No account",
  "unifiedGateway.health.noAccountHint": "The upstream pool has no account at all — add one on the platform accounts page.",
  "unifiedGateway.health.noAvailable": "No usable account",
  "unifiedGateway.health.noAvailableHint": "The pool has accounts, but none is usable right now (cooling, disabled or zero credits).",
  "unifiedGateway.health.noModels": "No models discovered",
  "unifiedGateway.health.noModelsHint": "No model list discovered yet — clients get nothing from /v1/models.",
  "unifiedGateway.health.disabled": "Disabled",
  "unifiedGateway.health.disabledHint": "This gateway does not enable the namespace; calls return 400.",
  "unifiedGateway.title": "Unified gateway",
  "unifiedGateway.running": "Running",
  "unifiedGateway.stopped": "Stopped",
  "unifiedGateway.crossScope": "This gateway spans all scopes; the scope selector above does not affect it.",
  "unifiedGateway.summary": "{namespaces} namespaces · {keys} active keys · listening on {addr}",
  "unifiedGateway.enable": "Enable the unified gateway",
  "unifiedGateway.enableHint": "One namespaced entry — every platform and version through a single provider",
  "unifiedGateway.bindAddr": "Bind address",
  "unifiedGateway.bindLan": "Allow LAN",
  "unifiedGateway.bindLoopback": "Loopback only",
  "unifiedGateway.port": "Port",
  "unifiedGateway.portHint": "Restart required. Avoid the ports already taken: UI 57890 · WorkBuddy 57891 · unified 57892 · hook 57893 · Trae 57894 · Qoder 57895",
  "unifiedGateway.portInvalid": "Invalid port: enter an integer between 1 and 65535",
  "unifiedGateway.nsTitle": "Enabled namespaces",
  "unifiedGateway.nsHint": "Disabled ones are not parsed or listed, and calls return 400. Keep at least one enabled.",
  "unifiedGateway.showMore": "Show more program lines",
  "unifiedGateway.showLess": "Hide extra program lines",
  "unifiedGateway.keepOne": "Keep at least one namespace enabled",
  "unifiedGateway.nsOff": "Disabled on this gateway — calls return 400",
  "unifiedGateway.keysTitle": "Unified keys",
  "unifiedGateway.createKey": "Create key",
  "unifiedGateway.noKeys": "No unified keys yet",
  "unifiedGateway.noKeysHint": "One key can authorize several namespaces at once (e.g. WorkBuddy CN plus Trae CN).",
  "unifiedGateway.colName": "Name",
  "unifiedGateway.colNamespaces": "Authorized namespaces",
  "unifiedGateway.colMasked": "Key",
  "unifiedGateway.colCreated": "Created",
  "unifiedGateway.colLastUsed": "Last used",
  "unifiedGateway.colStatus": "Status",
  "unifiedGateway.colActions": "Actions",
  "unifiedGateway.neverUsed": "Never used",
  "unifiedGateway.revoked": "Revoked",
  "unifiedGateway.active": "Active",
  "unifiedGateway.restore": "Restore",
  "unifiedGateway.delete": "Delete",
  "unifiedGateway.revoke": "Revoke",
  "unifiedGateway.loadFailed": "Failed to load the unified gateway config",
  "unifiedGateway.saveFailed": "Save failed",
  "unifiedGateway.created": "Key created",
  "unifiedGateway.createdHint": "The plaintext is shown only once (the store keeps sha256 only). Copy it now — it cannot be retrieved later.",
  "unifiedGateway.copy": "Copy",
  "unifiedGateway.savedClose": "I've saved it, close",
  "unifiedGateway.keyName": "Name",
  "unifiedGateway.keyNamePlaceholder": "e.g. My main key",
  "unifiedGateway.authorize": "Authorized namespaces (at least one)",
  "unifiedGateway.authorizeRequired": "Select at least one. Disabled namespaces may be authorized too — they take effect once enabled.",
  "unifiedGateway.bizTitle": "Business-forwarding namespace",
  "unifiedGateway.bizNone": "No business forwarding",
  "unifiedGateway.bizHint": "Business requests carry no model name, so the namespace can only come from the key; Trae namespaces are invalid.",
  "unifiedGateway.cancel": "Cancel",
  "unifiedGateway.create": "Create",
  "unifiedGateway.unnamed": "Untitled",
  "unifiedGateway.previewTitle": "Namespaces and model preview",
  "unifiedGateway.previewTotal": "{count} models in total",
  "unifiedGateway.previewEmpty": "No namespace is enabled — pick one under \"Enabled namespaces\" above.",
  "unifiedGateway.previewCount": "{count} models",
  "unifiedGateway.showModels": "Show models",
  "unifiedGateway.hideModels": "Hide",
  "unifiedGateway.previewNoModels": "No catalog for this segment yet",
  "unifiedGateway.source.live": "Live",
  "unifiedGateway.source.cached": "Cached",
  "unifiedGateway.source.builtin": "Built-in",

  // ---- Settings "Logs" section ----
  // ⚠️ Corrected after measuring on 2026-10-14: WorkBuddy DOES keep its own runtime logs
  //    (`~/.workbuddy/logs/`: per-day folders plus `automation.log`). The earlier wording
  //    pointed users at the gateway page, which shows THIS tool's request log — a different thing.
  "settings.logs.emptyTitle": "No runtime log in this section",
  "settings.logs.emptyBody":
    "This section only holds this tool's settings and backup entries; WorkBuddy's own runtime logs live in a logs subdirectory of its data dir (not surfaced here yet).",
  "settings.logs.emptyAction": "Open gateway settings",

  // ---- Scope visibility (app-settings.tsx) ----
  "appSettings.scopeVisibility.groupTitle": "Scope Visibility",
  "appSettings.scopeVisibility.toggleAria": "Toggle visibility for {scope}",
  "appSettings.scopeVisibility.hint": "Hidden scopes disappear from the top bar and shortcuts; direct URLs remain accessible.",
  "appSettings.scopeVisibility.onlyOneLeft": "Only one scope left visible — cannot hide more",

  // ---- Shared fallbacks ----
  "common.unknownError": "Unknown error",

  // ---- Top-level error boundary (see components/error-boundary.tsx) ----
  "app.error.title": "Something went wrong",
  "app.error.description":
    "A rendering error was caught. The UI stopped here to avoid showing a garbled screen. Copy the details below and report them; reloading usually gets you back in.",
  "app.error.reload": "Reload",
  "app.error.copy": "Copy error details",
  "app.error.copied": "Copied",
};
