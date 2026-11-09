/**
 * 文案域：**Trae 产品线 · 统计与 API 服务页**（英文）。
 *
 * ⚠️ 不写类型注解：键名合法性由 `src/locales/en.ts` 单点校验。域文件保持零依赖。
 * 键必须与 `traeStats.zh.ts` **一一对应**（缺哪条就回落中文）。
 */
export const en = {
  // =====================================================================
  // TraeTokenStatsPage.tsx — Usage · Token views (Trae section): trend / models
  //
  // ⚠️ The page header / sub-tabs / view-parameter bar are rendered by `UsagePage`,
  //    so this domain no longer carries title / subtitle / time-window / scope-bar keys.
  // ⚠️ Both views' stat tiles and chart titles reuse `wbStats.usage.trend.*` /
  //    `wbStats.usage.models.*` / `wbStats.usage.range.*` (the prototype's
  //    platform-agnostic usage labels) so both platforms read identically.
  // =====================================================================
  "trae.stats.token.loadFailed": "Cannot read token statistics",
  "trae.stats.token.retry": "Retry",
  "trae.stats.token.refresh": "Refresh",

  // ---- Data-source boundary ----
  "trae.stats.token.sourceTitle": "Data source",
  "trae.stats.token.sourceFallback": "Only calls that went through the local Trae gateway are counted.",
  // Ledger reading (gateway daily ledger, 90 days) — a different observation point than the log window.
  "trae.stats.token.ledgerTitle": "Data source · gateway ledger",
  "trae.stats.token.ledgerBody":
    "Every 2 minutes the background task folds the request log into a local ledger kept for 90 days — that book comes for free. Trae's client-side book lives in the IDE's private store (SQLCipher-encrypted, binary-indexed), which this tool cannot read; export an unencrypted copy with sqlcipher and import it from the Client tab.",
  "trae.stats.token.heatmapLedgerUnavailable":
    "The ledger aggregates by day and has no weekday × hour slots, so the hourly heatmap cannot be drawn from it — we do not render an empty chart.",
  // For the side-by-side ledger chart only: Trae's `all` view compares two books, it is not
  // "by model" — so WB's `stackedTitle` / `stackedAria` / `ledgerCaption` all misfit here
  // (the last one ends with "see Reconcile", which Trae has no entry point for).
  // ⚠️ The two books are neither equivalent nor a clean subset: the client copy holds **every**
  //   conversation, while the gateway records only requests **relayed through this machine** —
  //   and it may also include third-party calls made straight to the gateway with an API key,
  //   which never reach the client database. So they **partially overlap**, cannot be added up,
  //   and the overlap size cannot be measured (no per-record reconcile).
  "trae.stats.token.ledgerStackedTitle": "Daily tokens · client vs gateway",
  "trae.stats.token.ledgerStackedCaption":
    "Bar height is each book's own value — never merged, never netted; the two books overlap partially (see the note above).",
  "trae.stats.token.ledgerStackedAria": "Daily token bar chart comparing the client and gateway books side by side (not stacked)",
  // For the "All" tier's notice only. WB's `noMerge` / `noMergeBody` describe the *same request
  // counted twice*; Trae is not that shape — it is a partial, unmeasurable overlap.
  // ⚠️ On the page this renders as a **bold sub-heading** (one line) with `noMergeBody` below it,
  //   so it must read as a label: short, no full stop, no repeated "the two books" with the body.
  "trae.stats.token.noMerge": "Two books, not merged",
  "trae.stats.token.noMergeBody":
    "On Trae the two books overlap only partially: the client copy holds every conversation, while the gateway records only requests relayed through this machine (and may even include third-party calls made straight to the gateway, which never appear in the client database). How much they overlap cannot be measured, so nothing is merged or netted here — to look at one book on its own, switch to a single ledger.",

  // ---- Empty state ----
  "trae.stats.token.emptyTitle": "No gateway calls in the current window",
  "trae.stats.token.emptyBody": "Enable the gateway on the API service page and point your client's Base URL at it; calls will then be recorded here.",
  "trae.stats.token.ledgerEmptyTitle": "No ledger records for this window yet",
  "trae.stats.token.ledgerEmptyBody":
    "The ledger accumulates day by day once enabled (one round every 2 minutes). If the gateway was never enabled this stays empty — an unrecorded day is not a zero-usage day.",

  // ---- Stat tiles (labels come from the shared keys) ----
  "trae.stats.token.summaryAria": "Usage summary",
  "trae.stats.token.summary.activeAccounts": "Active accounts",

  // ---- Dimensions the platform cannot provide ----
  "trae.stats.token.unsupportedTitle": "Dimensions not supported by the platform",
  "trae.stats.token.unsupportedOnly": " ({on} only)",

  // ---- Log parse failures (honesty notice: incomplete numbers must be stated) ----
  "trae.stats.token.parseErrors": "{count} records in the log could not be parsed (missing timestamp) and were skipped.",

  // ---- Client ledger (import) ----
  "trae.stats.token.client.title": "Client ledger (import)",
  "trae.stats.token.client.body":
    "Trae's conversation database is SQLCipher-encrypted. Enter the passphrase and unlock: the tool exports an unencrypted copy on this machine and reads that — the original database is never modified. If you prefer not to let the tool decrypt, export the copy yourself and paste its path below.",
  "trae.stats.token.client.pathLabel": "Unencrypted copy path",
  "trae.stats.token.client.pathPlaceholder": "e.g. D:\\trae-plain\\database.db",
  "trae.stats.token.client.pick": "Choose file",
  "trae.stats.token.client.verify": "Verify",
  "trae.stats.token.client.save": "Save and load",
  "trae.stats.token.client.clear": "Clear",
  "trae.stats.token.client.helpTitle": "How to export an unencrypted copy",
  "trae.stats.token.client.helpBody":
    "This step needs **the database passphrase, which you must obtain yourself** — this tool neither provides it nor decrypts for you. Once you have it, open the source database in sqlcipher (PRAGMA key='<passphrase>';), then run: ATTACH DATABASE 'plain.db' AS plain KEY '';  SELECT sqlcipher_export('plain');  DETACH DATABASE plain;  and finally deal with database.db-wal too — otherwise the most recent stretch is lost. The passphrase has to be pulled from the client process with a debugger, which is a non-trivial step; if you don't already have an unencrypted copy, this ledger stays unavailable.",
  "trae.stats.token.client.missing": "No unencrypted copy selected yet",
  "trae.stats.token.client.missingHint": "The client ledger needs a one-off unlock; until then only the gateway book is available here.",
  "trae.stats.token.client.notPlaintext": "This file is not an unencrypted SQLite database",
  "trae.stats.token.client.notPlaintextHint": "Pointing it straight at the encrypted database.db will not work. Enter the passphrase above and click \"Unlock and read\" — the tool exports the copy for you.",
  "trae.stats.token.client.unrecognized": "It opens, but no session or usage structure is recognisable",
  "trae.stats.token.client.unrecognizedHint": "The client version may have changed its schema. The actual table names in the database are listed below so this can be adapted.",
  "trae.stats.token.client.noUsage": "Session structure recognised, but no usage fields found",
  "trae.stats.token.client.noUsageHint": "This client version may not write token usage back; only session counts can be measured.",
  "trae.stats.token.client.probeOk": "Readable: {sessions} sessions · {messages} messages · {withUsage} with usage",
  "trae.stats.token.client.probeTables": "Tables in the database",
  "trae.stats.token.client.importedAt": "Last read: {time}",
  "trae.stats.token.client.loadFailed": "Failed to read the client ledger",
  "trae.stats.token.client.retry": "Retry",
  // ---- Built-in decryption (you supply the passphrase; the tool exports a copy) ----
  "trae.stats.token.client.passphraseLabel": "Database passphrase",
  "trae.stats.token.client.passphrasePlaceholder": "Passphrase of Trae's private store",
  "trae.stats.token.client.passphraseHint":
    "The passphrase lives inside Trae's process and has no copy on disk — we use it once to decrypt, then drop it. It is never written to disk or sent anywhere.",
  "trae.stats.token.client.unlock": "Unlock and read",
  "trae.stats.token.client.unlocking": "Unlocking…",
  "trae.stats.token.client.unlocked": "Unlocked (compatibility {compat}); the unencrypted copy is ready",
  "trae.stats.token.client.unlockedPlain": "That file is already unencrypted — using it directly",
  "trae.stats.token.client.unlockHint":
    "The unencrypted copy lands in this tool's private directory (roughly the size of the source); the source database is never modified.",
  "trae.stats.token.client.wrongKey": "Wrong passphrase",
  "trae.stats.token.client.wrongKeyHint":
    "Check that this is the passphrase for the current client version. Repeated failures do not lock anything — just try again.",
  "trae.stats.token.client.alreadyPlaintext": "That file is already an unencrypted database",
  "trae.stats.token.client.alreadyPlaintextHint":
    "No passphrase needed. Just click \"Save and read\" to use it as the data source.",
  "trae.stats.token.client.notSqlite": "Not a SQLite database",

  // =====================================================================
  // TraeCreditsPage.tsx — Usage · Credits view (Trae section)
  //
  // ⚠️ Stat tiles and chart titles reuse `wbStats.usage.credits.*` (the prototype's
  //    platform-agnostic credits labels); only Trae-specific strings live here.
  // =====================================================================
  "trae.stats.credits.subtitle": "Remaining credits, daily change and history for each Trae account.",
  "trae.stats.credits.sync": "Sync credits",
  "trae.stats.credits.syncing": "Syncing",
  "trae.stats.credits.refreshDone": "Credits refreshed",
  "trae.stats.credits.refreshFailed": "Failed to refresh",
  "trae.stats.credits.loadFailed": "Cannot read Trae credits",
  "trae.stats.credits.overviewAria": "Credits overview",

  // ---- Stat-tile captions (prototype: check-in reward / net ±Y / earliest in D+N days) ----
  /* issue #186: `total` is a partial sum (unparseable balances are excluded). */
  "trae.stats.credits.accountsHintPartial": "{count} accounts · {unknown} with unknown balance (excluded)",
  "trae.stats.credits.inHint": "Check-in rewards",
  "trae.stats.credits.netHint": "Net {net}",
  "trae.stats.credits.soonestHint": "Earliest in {days} days",
  "trae.stats.credits.noExpiryHint": "Nothing expires within 30 days",

  // ---- Daily credits flow (Trae has both sides ⇒ two-way bars; WB only has spend) ----
  "trae.stats.credits.dailyTitle": "Daily credits flow",
  "trae.stats.credits.dailyNote": "Up = check-in rewards　Down = chat spend",
  "trae.stats.credits.dailyAria": "Two-way bar chart of daily credits flow (rewards up, spend down)",
  "trae.stats.credits.expiryEmpty": "Nothing expires within the next 30 days.",

  // ---- The prototype's two donuts (WB has no data source ⇒ not rendered) ----
  "trae.stats.credits.accountShareTitle": "Token share by account",
  "trae.stats.credits.shareNote": "Cumulative",
  "trae.stats.credits.accountShareAria": "Donut chart of cumulative token share per account",
  "trae.stats.credits.accountShareEmpty": "No account has produced usage yet.",
  "trae.stats.credits.statusTitle": "Gateway status codes",
  "trae.stats.credits.statusNote": "Current window",
  "trae.stats.credits.statusAria": "Donut chart of gateway response status codes",
  "trae.stats.credits.statusEmpty": "No gateway requests in this scope.",

  // ---- Credits ledger ----
  "trae.stats.credits.flowNote": "Newest first",
  "trae.stats.credits.recordEarned": "Credits earned",
  "trae.stats.credits.recordSpent": "Credits spent",
  "trae.stats.credits.flowEmptyTitle": "No credits ledger yet",
  "trae.stats.credits.flowEmptyBody": "Run a check-in or “Sync credits” once and the daily credits changes will be listed here.",

  // ---- Empty states ----
  "trae.stats.credits.trendEmptyAccounts": "No account data yet. Add an account and the credits statistics show up here.",
  "trae.stats.credits.trendEmptyData": "No credits data yet. Run a check-in or “Sync credits” once to see the daily change.",

  // ---- Dimensions the platform cannot provide ----

  // =====================================================================
  // TraeApiServicePage.tsx — API service page
  // =====================================================================
  "trae.stats.api.title": "API service",
  "trae.stats.api.loadFailed": "Cannot read gateway status",
  "trae.stats.api.retry": "Retry",
  "trae.stats.api.refresh": "Refresh",

  // ---- Gateway card ----
  "trae.stats.api.gateway.enable": "Enable API gateway",
  "trae.stats.api.gateway.desc": "Once enabled, local AI tools can call Trae models through the address below (upstream {upstream}).",
  "trae.stats.api.gateway.bindAddr": "Listen address",
  "trae.stats.api.gateway.bindLoopback": "127.0.0.1 (this machine only)",
  "trae.stats.api.gateway.bindLan": "0.0.0.0 (LAN)",
  "trae.stats.api.gateway.port": "Listen port",
  "trae.stats.api.gateway.statusLabel": "Status:",
  "trae.stats.api.gateway.running": "Running",
  "trae.stats.api.gateway.stopped": "Stopped",
  "trae.stats.api.gateway.openDataDir": "Open data directory",
  "trae.stats.api.gateway.noteLead": "The default port is 57894, deliberately clear of the WorkBuddy gateway (57891) — both serve ",
  "trae.stats.api.gateway.maxMode": "Max Mode (per-model limits)",
  "trae.stats.api.gateway.maxModeDesc": "Inject is_max_mode for models that support it and advertise each model's own limit (not a fixed 1M). Turn off to use the __dev channel's default window.",
  "trae.stats.api.gateway.noteTail": ".",
  "trae.stats.api.gateway.lanWarning": "In LAN mode, any device on the same network that gets hold of a key can spend your Trae credits.",
  "trae.stats.api.gateway.lastError": "Most recent error",

  // ---- Endpoint card ----
  "trae.stats.api.endpoint.title": "Endpoint",
  "trae.stats.api.endpoint.baseUrl": "Base URL",
  "trae.stats.api.copy": "Copy",
  "trae.stats.api.endpoint.copied": "Base URL copied",
  "trae.stats.api.endpoint.keyPrefix": "Latest key prefix",
  "trae.stats.api.endpoint.noKey": "No usable key yet",
  "trae.stats.api.overview.models": "Available models",
  "trae.stats.api.overview.modelsCount": "{count}",
  "trae.stats.api.overview.pool": "Account pool",
  "trae.stats.api.overview.poolCount": "{count} entries",
  "trae.stats.api.overview.view": "View",

  // ---- Action feedback ----
  "trae.stats.api.toast.savedStarted": "Gateway saved and started",
  "trae.stats.api.toast.savedDisabled": "Gateway saved (listening disabled)",
  "trae.stats.api.toast.saveFailed": "Failed to save",
  "trae.stats.api.toast.portInvalid": "Invalid port",
  "trae.stats.api.toast.portRange": "Enter a whole number between 1 and 65535",
  "trae.stats.api.toast.logsCleared": "Logs cleared",
  "trae.stats.api.toast.clearFailed": "Failed to clear",
  "trae.stats.api.toast.logsRefreshFailed": "Failed to refresh request logs",
  "trae.stats.api.toast.unsupported": "Not supported on this platform",
  "trae.stats.api.toast.unsupportedDesc": "This capability is only available on Windows.",
  "trae.stats.api.toast.dirOpened": "Trae data directory opened",
  "trae.stats.api.toast.dirOpenFailed": "Failed to open the data directory",

  // ---- LAN risk confirmation ----
  "trae.stats.api.lanDialog.title": "Allow LAN access?",
  "trae.stats.api.lanDialog.desc": "Once listening on 0.0.0.0, any device on the same network — public Wi-Fi included — can spend your Trae credits as long as it holds an API key. Only enable this on a network you trust.",
  "trae.stats.api.lanDialog.cancel": "Cancel",
  "trae.stats.api.lanDialog.confirm": "I understand, enable it",
};
