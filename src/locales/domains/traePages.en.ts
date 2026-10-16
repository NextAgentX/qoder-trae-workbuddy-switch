/**
 * 文案域：**Trae 产品线 · 页面**（英文）。
 *
 * ⚠️ 不写类型注解：键名合法性由 `src/locales/en.ts` 单点校验。域文件保持零依赖。
 * 键必须与 `traePages.zh.ts` **一一对应**（缺哪条就回落中文）。
 */
export const en = {
  // =====================================================================
  // TraeAccountsPage.tsx — Accounts page
  // =====================================================================
  "trae.page.accounts.title": "Accounts",
  "trae.page.accounts.subtitle": "Manage credentials, check-in and sign-in state for {line} accounts. The account store is kept separate from WorkBuddy's.",
  "trae.page.accounts.loadFailed": "Cannot read Trae data",

  // ---- Generic action results ----
  "trae.page.accounts.actionDone": "{label} done",
  "trae.page.accounts.actionFailed": "{label} failed",
  "trae.page.accounts.actionCheckinAll": "Check in and refresh credits",
  "trae.page.accounts.actionSwitch": "Switch {line} account",
  "trae.page.accounts.actionSaveLogin": "Save sign-in state",
  "trae.page.accounts.saveLoginNoProgram": "This account is not signed in on any Trae client, so its sign-in state cannot be saved",
  "trae.page.accounts.actionRefreshJwt": "Refresh JWT",
  "trae.page.accounts.actionThaw": "Clear cooldown",
  "trae.page.accounts.actionDelete": "Delete account",
  // P4: labels for the minimal batch bar (composed via actionDone / actionFailed).
  "trae.page.accounts.actionBatchCheckin": "Batch check-in",
  "trae.page.accounts.actionRefreshCredits": "Refresh credits",
  "trae.page.accounts.remarkSaved": "Remark saved",
  "trae.page.accounts.remarkCleared": "Remark cleared",
  "trae.page.accounts.remarkSaveFailed": "Failed to save remark",

  // ---- Merging the legacy product-line account store ----
  "trae.page.accounts.mergeDone": "Legacy product-line account store merged",
  "trae.page.accounts.mergeSummary": "{added} accounts merged in ({kept} existing accounts kept)",
  "trae.page.accounts.mergeGroups": "{count} groups",
  "trae.page.accounts.mergeBackup": "Old data backed up to {path}",
  "trae.page.accounts.mergeBackupFailed": "⚠️ Backup failed — copy the data directory manually first",
  "trae.page.accounts.mergeFailed": "Failed to merge the legacy account store",
  "trae.page.accounts.mergeSnapshotsDone": "Legacy sign-in snapshots carried over",
  "trae.page.accounts.mergeGuide.title": "Upgrade notice: Trae's two clients have merged",
  "trae.page.accounts.mergeGuide.desc": "Trae Work and Trae Code have been merged into a single client. This app follows: one Trae client per region, with legacy data migrated automatically on this launch.",
  "trae.page.accounts.mergeGuide.copied": "Carried over sign-in snapshots for {count} account(s) saved before the merge. The original folders are kept untouched.",
  "trae.page.accounts.mergeGuide.kept": "{count} account(s) already had a newer snapshot in the merged client; the newer one was kept.",
  "trae.page.accounts.mergeGuide.none": "There were no legacy snapshots to carry over on this machine.",
  "trae.page.accounts.mergeGuide.errors": "{count} account(s) could not be carried over; switching them still reports a missing snapshot.",
  "trae.page.accounts.mergeGuide.adviceTitle": "Recommended: sign in and save each account once",
  "trae.page.accounts.mergeGuide.advice": "Old snapshots carry the old client's device identity. If switching an account leaves you signed out (or signed in as someone else), sign in to that account in Trae first, then use Save sign-in state here to rebuild its snapshot.",
  "trae.page.accounts.mergeGuide.close": "Got it",
  "trae.page.accounts.mergeSnapshots": "Carried over sign-in snapshots for {count} account(s) (saved before the client merged)",
  "trae.page.accounts.mergeSnapshotsFailed": "⚠️ {count} snapshot(s) could not be carried over — see the run log",

  // ---- Check-in (all / single) ----
  "trae.page.accounts.creditsRefreshFailed": "Failed to refresh credits",
  "trae.page.accounts.noNeedCheckin": "No accounts need a check-in",
  "trae.page.accounts.checkinNotRun": "Check-in not performed",
  "trae.page.accounts.checkinSkippedDesc": "{name} was skipped (already checked in today, cooling down, or credentials expired)",
  "trae.page.accounts.alreadyCheckedToday": "Already checked in today",
  "trae.page.accounts.checkinFailed": "Check-in failed",
  "trae.page.accounts.checkinFailedDesc": "{name}: {reason}",
  "trae.page.accounts.checkinSuccess": "Check-in succeeded",
  "trae.page.accounts.checkinSuccessDesc": "{name}: +{delta} credits",
  // ---- Batch check-in summary (mirrors WorkBuddy's `wbAccounts.toast.checkinBatch*`) ----
  "trae.page.accounts.checkinBatchSuccess": "{n} checked in",
  "trae.page.accounts.checkinBatchAlready": "{n} already checked in",
  "trae.page.accounts.checkinBatchFailed": "{n} failed",
  // ★ B4: toast action on partial failure — retry only the failed accounts.
  "trae.page.accounts.checkinRetryFailed": "Retry failed",

  // ---- Import local account ----
  "trae.page.accounts.importLocalDone": "Local account imported",
  "trae.page.accounts.importLocalUid": "{name} · UID {uid}",
  "trae.page.accounts.importLocalFailed": "Failed to import local account",

  // ---- "Add & migrate accounts" banner ----
  "trae.page.accounts.addTitle": "Add & migrate accounts",
  "trae.page.accounts.addSubtitle": "Add an account quickly, or restore from an existing environment",
  "trae.page.accounts.oauthLogin": "OAuth web sign-in",
  "trae.page.accounts.pasteJwt": "Paste JWT",
  "trae.page.accounts.pasteJwtTitle": "Paste Cloud-IDE-JWT manually (fallback — prefer the web sign-in above)",
  "trae.page.accounts.exportTitle": "Export account backup",

  // ---- First-run empty state (`EmptyTraeCard`) ----
  // 2026-10-10: the old "client environment line" keys were deleted along with that strip
  // (the user asked to remove it, and the prototype's skeleton has no such row).
  // Only the key still referenced by `EmptyTraeCard` remains.
  "trae.page.accounts.dataDirMissing": "(Not created yet — start Trae once and sign in first)",

  // ---- Accounts toolbar ----
  "trae.page.accounts.sectionTitle": "Accounts",
  "trae.page.accounts.countAria": "{count} accounts",
  /* The list section no longer renders a visible heading (the prototype's account list has
     none); the count lives in the page-head subtitle, and semantics come from aria-label. */
  "trae.page.accounts.sectionAria": "Account list ({count})",
  // 2026-10-10: the toolbar's "filter by group" dropdown was removed so the toolbar matches
  // WorkBuddy's control set; its four strings went with it.
  "trae.page.accounts.fallbackGroup": "Group",
  // P4 toolbar: sort / group options (driven by `availableSorts` / `availableGroups` data; no fake options)
  "trae.page.accounts.toolbar.sort.priority": "Credit priority",
  "trae.page.accounts.toolbar.sort.name": "Name",
  "trae.page.accounts.toolbar.sort.expiry": "Expiry",
  "trae.page.accounts.toolbar.group.none": "No grouping",
  "trae.page.accounts.toolbar.group.program": "By program",
  "trae.page.accounts.toolbar.group.status": "By status",
  "trae.page.accounts.toolbar.group.group": "By custom group",
  "trae.page.accounts.toolbar.statusBucket.normal": "Normal",
  "trae.page.accounts.toolbar.statusBucket.notCheckedIn": "Not checked in today",
  "trae.page.accounts.toolbar.statusBucket.expired": "Credential expired",
  "trae.page.accounts.toolbar.statusBucket.cooling": "Cooling down",
  "trae.page.accounts.toolbar.programBucket.unbound": "No program bound",
  "trae.page.accounts.toolbar.groupBucket.default": "Ungrouped",
  // P4 table headers
  "trae.page.accounts.table.account": "Account",
  "trae.page.accounts.table.status": "Status",
  "trae.page.accounts.table.credits": "Credits",
  "trae.page.accounts.table.expiry": "Expiry",
  "trae.page.accounts.table.programs": "Programs",
  "trae.page.accounts.table.current": "Current",
  "trae.page.accounts.autoCheckin": "Auto check-in",
  "trae.page.accounts.autoCheckinSaving": "Saving auto check-in setting",
  "trae.page.accounts.skipChecked": "Skip checked-in",
  "trae.page.accounts.skipCheckedAria": "Skip accounts already checked in today",
  "trae.page.accounts.checkinSettingsSaving": "Saving check-in setting",
  "trae.page.accounts.checkinRefreshAll": "Check in and refresh all accounts' credits",
  "trae.page.accounts.autoCheckinSaveFailed": "Failed to save auto check-in setting",
  "trae.page.accounts.settingsSaveFailed": "Failed to save settings",
  "trae.page.accounts.switchIncomplete": "Switch did not complete",

  // ---- Check-in result card ----
  "trae.page.accounts.resultTitle": "Check-in results",
  "trae.page.accounts.resultProgress": "Processing {index} / {total}",
  "trae.page.accounts.resultLine": "{name}: {status}",
  "trae.page.accounts.resultSkipped": "Checked in",
  "trae.page.accounts.resultSuccess": "Succeeded",
  "trae.page.accounts.resultFailed": "Failed",
  "trae.page.accounts.resultEmpty": "No account needed a check-in this round (already checked in today, cooling down, or credentials expired).",

  // 2026-10-10: the "last check-in card" keys were deleted along with that card.
  // Batch check-in results are shown inline by the shared `CheckinResultBar` instead.

  // ---- Account add / import / delete feedback ----
  "trae.page.accounts.accountAdded": "Account added",
  "trae.page.accounts.autoRenewSuffix": " · auto-renew supported",
  "trae.page.accounts.exportedCount": "Exported {count} accounts",
  "trae.page.accounts.importDone": "Import finished",
  "trae.page.accounts.importSummary": "Added {added} · overwritten {overwritten} · skipped {skipped}",
  "trae.page.accounts.deleteTitle": "Delete account",
  "trae.page.accounts.deleteBody": "Delete the account “{name}”? Its sign-in snapshot is kept and can be cleaned up separately under Settings → Sign-in snapshots. You can restore it from the recycle bin.",
  "trae.page.accounts.cancel": "Cancel",
  "trae.page.accounts.delete": "Delete",
  "trae.page.accounts.unknownAccount": "Unknown account",

  // ---- Add account by pasting JWT ----
  "trae.page.accounts.addDialogTitle": "Add account by pasting JWT",
  "trae.page.accounts.addDialogDesc": "A fallback for when web sign-in is unavailable. Paste the `Cloud-IDE-JWT …` token (grab it from the proxy log or the plaintext storage of older clients). If the name is left empty, the account is named after the UID suffix.",
  "trae.page.accounts.addDialogNote": "Note: accounts added this way have no refresh_token, so the JWT must be pasted again once it expires; accounts added via web sign-in renew automatically.",
  "trae.page.accounts.addDialogName": "Account name (optional)",
  "trae.page.accounts.addDialogNamePlaceholder": "e.g. Main account",
  "trae.page.accounts.addDialogGroup": "Group (optional)",
  "trae.page.accounts.ungroupedPlaceholder": "Ungrouped",
  "trae.page.accounts.needJwt": "Please paste the JWT",
  "trae.page.accounts.addFailed": "Failed to add",
  "trae.page.accounts.add": "Add",

  // ---- Empty state card ----
  "trae.page.accounts.emptyTitle": "No Trae accounts yet",
  "trae.page.accounts.emptyReasons": "Possible causes:",
  "trae.page.accounts.emptyReasonNotInstalled": "The Trae desktop client is not installed",
  "trae.page.accounts.emptyReasonNoSession": "The client is installed, but there is no usable sign-in state",
  "trae.page.accounts.emptyReasonEncryptedLead": "You are signed in to the client, but the credentials are stored encrypted",
  "trae.page.accounts.emptyReasonEncryptedMid": "— since Trae 1.107.x the plaintext JWT is no longer written to",
  "trae.page.accounts.emptyReasonEncryptedTail": ", so importing from this machine cannot read it. This is the most common cause on current versions.",
  "trae.page.accounts.emptyReasonEmpty": "The account store is empty and no account has been added yet via web sign-in or a backup file",
  "trae.page.accounts.emptyRecommendLabel": "Recommended:",
  "trae.page.accounts.emptyRecommendBody": "Sign in and authorize once in your browser with “OAuth web sign-in”. The app catches the callback itself — no local file to hunt for and no token to paste. It is the only path unaffected by encrypted storage.",
  "trae.page.accounts.emptyRecheck": "Re-detect",
  "trae.page.accounts.emptyImportHint": "Only for older clients or installs that still keep credentials in plaintext; usually unreadable since 1.107.x",
  "trae.page.accounts.emptyImportTry": "Try importing from this machine",
  "trae.page.accounts.emptyDetectedDir": "Detected sign-in directory:",
  "trae.page.accounts.emptyExpectedDir": "Client data directory (expected location):",
  "trae.page.accounts.emptyNoDataDir": "No client data directory detected yet",
  "trae.page.accounts.emptyDirNote": "This file no longer stores plaintext credentials in newer clients; it only confirms where the client data directory is.",
  "trae.page.accounts.emptyLaunchClient": "Launch the client",

  // =====================================================================
  // TraeSettingsPage.tsx — Settings page
  // =====================================================================
  "trae.page.settings.title": "Settings",
  "trae.page.settings.subtitle": "Locate the client, set local ports and the check-in policy, and review sign-in snapshots and runtime logs. Trae settings are stored apart from WorkBuddy's and neither affects the other.",
  "trae.page.settings.loadFailed": "Cannot read Trae settings",
  "trae.page.settings.cancel": "Cancel",
  "trae.page.settings.refresh": "Refresh",
  "trae.page.settings.copy": "Copy",
  "trae.page.settings.exportCsv": "Export CSV",
  "trae.page.settings.save": "Save",
  "trae.page.settings.saveFailed": "Failed to save",
  "trae.page.settings.unknown": "Unknown",
  "trae.page.settings.running": "Running",
  "trae.page.settings.notRunning": "Not running",
  /* `trae.page.settings.loading` removed (P3): replaced by a `<Skeleton>` placeholder. */
  "trae.page.settings.actionDone": "{label} done",
  "trae.page.settings.actionFailed": "{label} failed",

  // ---- Trae client ----
  "trae.page.settings.clientGroup": "Trae client",
  "trae.page.settings.installLabel": "Install",
  "trae.page.settings.detected": "Detected",
  "trae.page.settings.detectedVersion": "Detected v{version}",
  "trae.page.settings.notDetected": "Not detected",
  "trae.page.settings.runLabel": "Status",
  "trae.page.settings.platform": "Platform {name}",
  "trae.page.settings.autoDetectPath": "Auto-detected install",
  "trae.page.settings.autoDetectDataDir": "Auto-detected data directory",
  "trae.page.settings.dirMissing": "Directory missing",
  "trae.page.settings.multiLead": "With several Trae product lines installed (",
  "trae.page.settings.multiSep": ", ",
  "trae.page.settings.multiTail": "), the most recently active one is selected automatically.",
  "trae.page.settings.pathLabel": "Client executable path",
  "trae.page.settings.pathDesc": "Applies to [{region}] only. Leave empty for auto-detection, which scans common install locations on the system drive and other drives — no need to fill this in. Only set it if Trae sits in an unusual location. Domestic and international keep separate values and never overwrite each other.",
  "trae.page.settings.pathPlaceholder": "Empty = auto-detect common locations",
  "trae.page.settings.pathStale": "The path set for [{region}] no longer exists (the client was upgraded or merged into a new location). Currently using the auto-detected one: {path}. Click “Save” with this field empty to fall back to auto-detection.",

  // ---- Ports & network ----
  "trae.page.settings.networkGroup": "Ports & network",
  "trae.page.settings.proxyPortLabel": "Local proxy port",
  "trae.page.settings.proxyPortDesc": "The listening port of the local MITM proxy; capturing the sign-in state depends on it.",
  "trae.page.settings.apiPortLabel": "API gateway port",
  "trae.page.settings.apiPortDesc": "Defaults to 7864, deliberately clear of the WorkBuddy gateway (57891) — both serve /v1/chat/completions.",
  "trae.page.settings.proxyDomainsLabel": "Proxy decryption domains",
  "trae.page.settings.proxyDomainsDesc": "Comma-separated. Only these domains are decrypted by the local proxy to capture the sign-in state; all other traffic stays on the encrypted tunnel.",

  // ---- Auto check-in (schedule) ----
  "trae.page.settings.autoCheckinGroup": "Auto check-in",
  "trae.page.settings.autoCheckinEnable": "Enable auto check-in",
  "trae.page.settings.autoCheckinDesc": "At the scheduled time the background scheduler runs one round across every Trae account on both the CN and global lines; it also catches up on launch if today's round has not run. Off by default — once enabled this app actively contacts Trae upstream.",
  "trae.page.settings.checkinHoursLabel": "Trigger times",
  "trae.page.settings.checkinHoursDesc": "Run one check-in round at each of these hours (0-23, several allowed). Enable the switch above first; to run a round by hand use the refresh button in the accounts page toolbar.",
  "trae.page.settings.autoCheckinSaveFailed": "Failed to save auto check-in settings",
  "trae.page.settings.jwtRefreshGroup": "Auto-refresh JWT",
  "trae.page.settings.jwtRefreshEnable": "Enable auto-renewal",
  "trae.page.settings.jwtRefreshDesc": "Once a JWT has under 24 hours left, exchange the account's refresh_token for a new one automatically. The background scheduler does this at the times below and catches up on launch. Only accounts added via web sign-in have a refresh_token. On by default; turn it off and the only way left is the manual \"Refresh JWT\" item in the account card menu.",
  "trae.page.settings.jwtRefreshHoursLabel": "Check times",
  "trae.page.settings.jwtRefreshHoursDesc": "Run one check at each of these hours (0-23, several allowed). Only accounts close to expiry are contacted; the rest are skipped.",
  "trae.page.settings.jwtRefreshSaveFailed": "Failed to save JWT renewal settings",

  // ---- Check-in policy ----
  "trae.page.settings.checkinGroup": "Check-in policy",
  "trae.page.settings.skipCheckedLabel": "Skip accounts already checked in today",
  "trae.page.settings.skipCheckedDesc": "Avoids duplicate claim requests for the same account, cutting quota waste and risk-control exposure.",
  "trae.page.settings.skipExpiredLabel": "Skip accounts with an expired JWT",
  "trae.page.settings.skipExpiredDesc": "Invalid credentials always return 401; skipping them keeps the account out of permanent cooldown.",
  "trae.page.settings.retryLabel": "Network failure retries",
  "trae.page.settings.retryDesc": "Only network-level errors are retried; business failures (such as quota limits) are not.",
  "trae.page.settings.logRetentionGroup": "Log retention",
  "trae.page.settings.logRetentionLabel": "Log retention days",
  "trae.page.settings.logRetentionDesc": "Log rows past the window are trimmed at startup; external output without a date prefix is always kept.",

  // ---- Device identity ----
  "trae.page.settings.deviceGroup": "Device identity",
  "trae.page.settings.resetLabel": "Reset device identity",
  "trae.page.settings.resetDesc": "Processes six layers of client device identity in order: the machineid file, telemetry and device IDs in storage.json, aha/TinyStorage, the registry MachineGuid (Windows only), and WebView tracking data. Make sure a sign-in snapshot is saved first.",
  "trae.page.settings.reset": "Reset",
  "trae.page.settings.resetResultPrefix": "Last result: ",
  "trae.page.settings.resetLayers": "{done} / {total} applied",
  "trae.page.settings.resetConfirm": "Confirm reset",
  "trae.page.settings.resetDialogDesc": "This rewrites the Trae client's machine code and device identity files. Close Trae first and confirm that a sign-in snapshot is saved. On non-Windows platforms the registry MachineGuid layer is plainly reported as “unsupported on this platform”, never faked as successful.",
  "trae.page.settings.resetDone": "Device identity reset finished",
  "trae.page.settings.resetFailed": "Reset failed",
  "trae.page.settings.stepOk": "Handled",
  "trae.page.settings.stepUnsupported": "Unsupported on this platform",
  "trae.page.settings.stepSkipped": "Skipped",

  // ---- Platform capabilities ----
  "trae.page.settings.capabilitiesGroup": "Platform capabilities",
  "trae.page.settings.capClientDetection": "Client detection",
  "trae.page.settings.capProcessControl": "Process control",
  "trae.page.settings.capScheduledTask": "Scheduled tasks",
  "trae.page.settings.capMachineGuid": "MachineGuid reset",
  "trae.page.settings.capabilityUnsupported": " (unsupported)",
  "trae.page.settings.capabilitiesNote": "Capabilities marked “unsupported” plainly state where they are supported and why they fail here, instead of faking success.",
  "trae.page.settings.capabilityRow": "{label} ({on} only): {reason}",

  // ---- About ----
  "trae.page.settings.aboutGroup": "About",
  "trae.page.settings.aboutTitle": "Trae support project",
  "trae.page.settings.aboutDesc": "Account switching, check-in and device identity features draw on open-source implementations and are rewritten as native Rust for this tool's architecture.",
  "trae.page.settings.aboutUnknownSite": "Site address unknown",

  // ---- Runtime logs ----
  "trae.page.settings.logsGroup": "Runtime logs",
  "trae.page.settings.kindAll": "All",
  "trae.page.settings.kindApp": "Runtime",
  "trae.page.settings.kindCheckin": "Check-in",
  "trae.page.settings.kindSwitch": "Switch",
  "trae.page.settings.allDates": "All dates",
  "trae.page.settings.searchPlaceholder": "Search keywords…",
  "trae.page.settings.search": "Search",
  "trae.page.settings.logsReset": "Reset",
  "trae.page.settings.autoRefreshAria": "Auto-refresh logs",
  "trae.page.settings.autoRefresh": "Auto-refresh",
  "trae.page.settings.totalCount": "{count} entries",
  "trae.page.settings.kindCount": "{label} {count}",
  "trae.page.settings.logsLoadFailed": "Failed to read logs",
  "trae.page.settings.detailLabel": "Log entries",
  "trae.page.settings.showing": "Showing {shown} / {total}",
  "trae.page.settings.logsEmptyTitle": "No logs yet",
  "trae.page.settings.logsEmptyBody": "The log file does not exist yet, or nothing matches the current filters. Entries start appearing here once you run a check-in or switch accounts.",
  "trae.page.settings.noTime": "(no timestamp)",
  "trae.page.settings.noteFallback": "Reads plain-text runtime logs from this machine only.",
  "trae.page.settings.sourceExists": "Exists",
  "trae.page.settings.sourceMissing": "Not created",
  "trae.page.settings.logDir": "Log directory: {dir}",
  "trae.page.settings.csvHeader": "Time\tType\tMessage",

  // ---- Gateway request logs ----
  "trae.page.settings.gatewayGroup": "Gateway request logs",
  "trae.page.settings.statusAll": "All statuses",
  "trae.page.settings.statusOk": "Success (2xx)",
  "trae.page.settings.statusClient": "Client errors (4xx)",
  "trae.page.settings.statusServer": "Server errors (5xx)",
  "trae.page.settings.gwSearchPlaceholder": "Search account / model / endpoint / error…",
  "trae.page.settings.gwClear": "Clear",
  "trae.page.settings.gwCleared": "Logs cleared",
  "trae.page.settings.gwLoadFailed": "Failed to read gateway logs",
  "trae.page.settings.gwSummary": "{count} entries{filtered}{tokens}. This list stays empty unless the gateway is enabled — only it writes request logs.",
  "trae.page.settings.gwFiltered": " (filtered from {total})",
  "trae.page.settings.gwTokens": " · {tokens} tokens in total",
  "trae.page.settings.colTime": "Time",
  "trae.page.settings.colAccount": "Account",
  "trae.page.settings.colModel": "Model",
  "trae.page.settings.colStatus": "Status",
  "trae.page.settings.colLatency": "Latency",
  "trae.page.settings.colToken": "Token",
  "trae.page.settings.gwEmptyTitle": "No gateway request logs yet",
  "trae.page.settings.gwEmptyBody": "Once the Trae gateway is enabled on the API service page, every request from external clients is recorded here.",
  "trae.page.settings.gwFooter": "Click any row to see the full fields. By default the gateway records metadata only (model, status, latency, token counts).",
  "trae.page.settings.gwDetailTitle": "Request details",
  "trae.page.settings.gwDetailDesc": "The gateway records metadata only. To capture bodies, enable “Log request bodies” on the API service page.",
  "trae.page.settings.gwDetailCopied": "Details copied",

  // ---- Sign-in snapshots ----
  "trae.page.settings.profilesGroup": "Sign-in snapshots",
  "trae.page.settings.clientLabel": "Client",
  "trae.page.settings.currentAccount": "Current account",
  "trae.page.settings.programLabel": "Program",
  "trae.page.settings.snapshotCount": "{count} snapshots",
  "trae.page.settings.dataDirLabel": "Client data directory: ",
  "trae.page.settings.backupLabel": "Back up current sign-in state",
  "trae.page.settings.backupDesc": "Saves the client's current sign-in state into the given slot; one snapshot per account, copying {count} core sign-in files exactly. Name the slot after the account UID; “last” is the fallback slot the switching flow uses automatically.",
  "trae.page.settings.slotPlaceholder": "Slot name (account UID)",
  "trae.page.settings.backup": "Back up",
  "trae.page.settings.profilesEmpty": "No snapshots yet. One is saved for the current account automatically whenever you switch accounts.",
  "trae.page.settings.fallbackSlot": "Fallback slot",
  "trae.page.settings.fileCount": "{count} files",
  "trae.page.settings.updatedAt": "Updated {time}",
  "trae.page.settings.restore": "Restore",
  "trae.page.settings.delete": "Delete",
  "trae.page.settings.profilesFooter": "Snapshots live in Buddy Switch's own data directory, separate from the Trae client directory, so deleting one never touches the sign-in state currently in use.",
  "trae.page.settings.restoreTitle": "Restore this snapshot",
  "trae.page.settings.restoreBodyLead": "This overwrites the Trae client's current sign-in state with the “{slot}” snapshot. ",
  "trae.page.settings.restoreBodyStrong": "The current sign-in state is not saved automatically",
  "trae.page.settings.restoreBodyTail": "; back it up above first if you want to keep it. Close the Trae client beforehand.",
  "trae.page.settings.restoreConfirm": "Confirm restore",
  "trae.page.settings.deleteTitle": "Delete snapshot",
  "trae.page.settings.deleteBody": "This deletes {count} files ({size}) in the “{slot}” slot. The account record is unaffected, but this slot can no longer be restored.",
};
