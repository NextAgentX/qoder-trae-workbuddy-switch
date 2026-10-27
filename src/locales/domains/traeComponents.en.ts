/**
 * 文案域：**Trae 产品线 · 组件**（英文）。
 *
 * ⚠️ 不写类型注解：键名合法性由 `src/locales/en.ts` 单点校验。域文件保持零依赖。
 * 键必须与 `traeComponents.zh.ts` **一一对应**（缺哪条就回落中文）。
 */
export const en = {
  // =====================================================================
  // trae-types.ts —— program / variant display names
  // =====================================================================
  // Brand names are identical in both locales; they still go through the word list
  // so that adding a program triggers a compile-time reminder to add the key.
  // ★★ Naming rule (user-confirmed, corrected 2026-10-13): the **CN build carries a
  // ` CN` suffix, the international build carries none**. All six names are verbatim
  // the client's own strings (`nameAlias` / `nameShort`, measured from `product.json`),
  // so none of them is invented:
  //   TraeWork CN / TraeWork · TraeCode CN / TraeCode · Trae CN / Trae
  // (The earlier "international gets an `AI` suffix" rule was wrong: it invented names
  // the client never uses, and `Trae AI` matched both international TraeCode and the
  // international merged client.)
  // Merged client (issue #20): the client is literally named `Trae CN` / `Trae`.
  // They are **separate variants** (`trae_merged` / `global_trae_merged`) — the ids
  // and snapshot dirs differ from `traeCode*`.
  "trae.program.trae": "Trae",
  "trae.program.traeCn": "Trae CN",
  "trae.program.traeGlobal": "Trae",

  // Trae-side **region names** (user-confirmed 2026-10-13: use the client's own
  // product name — `Trae CN` for the CN build, `Trae` for the international one).
  // Distinct from `shared.region.version.*`, which is the cross-platform region
  // *word* used in **composed** labels (`scope-chip.tsx` = product + region word)
  // and is also read by WorkBuddy — so it must not become a Trae product name.
  "trae.region.cn": "Trae CN",
  "trae.region.global": "Trae",

  // =====================================================================
  // trae-account-card.tsx —— Trae account card
  // =====================================================================
  // ---- Duration / JWT status ----
  "trae.comp.card.hours.unknown": "Unknown",
  "trae.comp.card.hours.expired": "Expired",
  "trae.comp.card.hours.minutes": "{count} min",
  "trae.comp.card.hours.hours": "{count} h",
  "trae.comp.card.hours.days": "{count} d",
  "trae.comp.card.jwt.valid": "Valid {hours}",
  "trae.comp.card.jwt.warn": "Expiring {hours}",
  "trae.comp.card.jwt.expired": "Expired",
  "trae.comp.card.jwt.unparsable": "Unparsable",

  // ---- Expiry ----
  "trae.comp.card.expiry.permanent": "No expiry",
  "trae.comp.card.expiry.short": "Due {date}",
  // 2026-10-12: the in-card "big number row + expiring packages list" was removed to match
  // the prototype card, so the related keys are gone.

  // ---- Body row labels ----
  "trae.comp.card.label.device": "Device",
  "trae.comp.card.label.jwtExpiry": "JWT expiry",
  "trae.comp.card.label.addedAt": "Added",
  "trae.comp.card.label.updatedAt": "Last updated",
  "trae.comp.card.label.group": "Group",

  // ---- Status chips ----
  "trae.comp.card.chip.checked": "Checked in",
  "trae.comp.card.chip.toolsInUse": "{n} tools in use",
  "trae.comp.card.chip.expired": "Credentials expired",
  "trae.comp.card.chip.needRelogin": "Needs re-login",
  "trae.comp.card.chip.expiring": "{amount} credits expire within {days} days",
  // issue #83 · 3 (second half): per-package name / remaining / expiry must be visible,
  // not just the aggregate. Rendered in a tooltip — the card is a fixed 4-block layout.
  "trae.comp.card.packs.title": "Credit packages ({n})",
  "trae.comp.card.packs.line": "{name} · {remaining}/{total} left · expires {expire}",
  "trae.comp.card.packs.lineExpired": "{name} · {remaining}/{total} left · expired {expire}",
  "trae.comp.card.packs.unnamed": "Unnamed package",
  "trae.comp.card.chip.unchecked": "Not checked in today",
  "trae.comp.card.chip.cooling": "Cooling down",
  "trae.comp.card.chip.jwtAuto": "Auto-refresh JWT",
  "trae.comp.card.chip.jwtManual": "JWT refreshable",
  "trae.comp.card.cooldown.until": " · until {time}",
  "trae.comp.card.cooldown.fallback": "Cooling down ({type})",
  "trae.comp.card.chip.authRejected": "Credential rejected (self-healing)",
  // ---- Cooldown state → user action (single source, see src/lib/trae-cooldown.ts) ----
  "trae.comp.card.cooldown.hint.sessionDead":
    "The upstream invalidated this session (12153). Sign in again; the marker clears itself once the credential changes. You can also use \"Clear cooldown\" in the menu.",
  "trae.comp.card.cooldown.hint.authRejected":
    "The upstream rejected this access token but did not invalidate the session — usually expiry. The keep-alive refresh recovers it in about 60 seconds.",
  "trae.comp.card.cooldown.hint.planLimit": "Plan quota exhausted. Wait for the reset or use another account.",
  "trae.comp.card.cooldown.hint.transient":
    "Rate limited or upstream hiccup. Recovers when the cooldown expires; the first successful request clears it immediately.",
  "trae.comp.card.cooldown.age.justNow": "just now",
  "trae.comp.card.cooldown.age.minutes": "{minutes} min ago",
  "trae.comp.card.cooldown.age.hours": "{minutes} h ago",
  "trae.comp.card.cooldown.age.days": "{minutes} d ago",

  // ---- Program switch controls ----
  "trae.comp.card.currentOf": "Current account in {label}",
  "trae.comp.card.switch.tip": "Switch to this account in {label} (restarts {label})",
  "trae.comp.card.switch.missing": "{label} not detected",
  "trae.comp.card.switch.noDataDir": "{label} has no client data directory yet — start {label} once and sign in, then save the login state",
  "trae.comp.card.switch.aria": "Switch to {label}",
  "trae.comp.card.switch.ariaBusy": "Switching to {label}",
  "trae.comp.card.switch.busy": "Switching…",
  // 2026-10-10: the cross-region "switch region first" strings were deleted — the CN scope
  // no longer renders global program buttons at all (user request).

  // ---- Action menu ----
  "trae.comp.card.manage.aria": "Manage account {name}",
  "trae.comp.card.manage.title": "More account actions",
  "trae.comp.card.selectAria": "Select account {name}",
  "trae.comp.card.menu.save": "Save login state",
  "trae.comp.card.menu.refreshJwt": "Refresh JWT",
  "trae.comp.card.menu.refreshCredits": "Refresh credits",
  "trae.comp.card.menu.checkin": "Check in manually",
  "trae.comp.card.menu.thaw": "Clear cooldown",
  "trae.comp.card.menu.remark": "Edit remark",
  "trae.comp.card.menu.delete": "Delete account",

  // ---- Credits section ----
  "trae.comp.card.credits.remaining": "Credits left",
  "trae.comp.card.credits.notQueried": "No credits fetched yet",
  "trae.comp.card.credits.refresh": "Refresh credits",
  "trae.comp.card.section.info": "Account info",
  "trae.comp.card.package.fallback": "Credit package",
  // P4: "used / expiry" text for the drawer `DetailPack` (mirrors the WorkBuddy drawer).
  "trae.comp.card.package.used": "Used {n}",
  "trae.comp.card.package.expired": "Expired",

  // ---- Action explanations (used as menu-item titles) ----
  // P4: the footer "Save login state" / "Refresh JWT" buttons moved back into the menu;
  // their explanations live on as menu-item titles so the "why disabled" is not lost.
  "trae.comp.card.action.saveTip": "Save the client's current login state under this account (used to restore when switching); the client will be closed and reopened first",
  "trae.comp.card.action.saveNoLogin": "This account is not signed in on any Trae client — sign in on the client first, then save",
  "trae.comp.card.action.refreshJwtTip": "Exchange the refresh token for a new JWT",
  "trae.comp.card.action.refreshCreditsTip": "Re-query credits for this account only — no need to select all accounts and refresh in bulk",
  "trae.comp.card.footer.updatedAt": "Updated {time}",
  "trae.comp.card.footer.noUpdate": "No update time recorded",
  "trae.comp.card.footer.updatedTip": "Last time this account was updated in the local account library",

  // ---- Card head + metrics row (prototype `.acct-top` / `.acct-metrics`) ----
  // Same names and meaning as the WorkBuddy card: both product areas render one card shape.
  "trae.comp.card.currentLogin": "Current login",
  "trae.comp.card.metric.credits": "Credits",
  "trae.comp.card.metric.expiry": "Expires",

  // ---- Detail drawer sections (assembled by TraeAccountsPage) ----
  "trae.comp.card.detail.section.account": "Account",
  "trae.comp.card.detail.section.credits": "Credits",
  "trae.comp.card.detail.section.credential": "Credential",

  // ---- Details (P4: carried by the shared drawer `account-detail-drawer.tsx`) ----
  "trae.comp.card.detail.open": "View account details",
  "trae.comp.card.detail.subtitle": "{name} · UID {uid}",
  "trae.comp.card.detail.accountId": "Account ID",
  "trae.comp.card.detail.creditsExpiry": "Credits expiry",
  "trae.comp.card.detail.jwtStatus": "JWT status",
  "trae.comp.card.detail.jwtAutoRefresh": "Auto-refresh JWT",
  "trae.comp.card.detail.deviceId": "Device ID",
  "trae.comp.card.detail.cooldown": "Cooldown",
  "trae.comp.card.detail.none": "None",
  "trae.comp.card.detail.notQueried": "Not fetched",
  "trae.comp.card.detail.jwtAutoOn": "Due for renewal",
  "trae.comp.card.detail.jwtAutoOff": "Auto-renews before expiry",
  "trae.comp.card.detail.jwtAutoDisabled": "Off (manual refresh only)",
  "trae.comp.card.detail.noRefreshToken": "No refresh token",

  // =====================================================================
  // trae-oauth-login-dialog.tsx —— OAuth web login
  // =====================================================================
  "trae.comp.oauth.title": "OAuth web login · {variant}",
  "trae.comp.oauth.desc.lead": "Sign in to",
  "trae.comp.oauth.desc.mid": "in your browser and authorize. The app catches the callback automatically and adds the account to",
  "trae.comp.oauth.desc.tail": "'s account library — no token pasting needed.",
  "trae.comp.oauth.error.fallback": "Sign-in failed",
  "trae.comp.oauth.error.timeout": "Timed out waiting for authorization ({seconds}s). Confirm that you finished authorizing in the browser; if you did and it still timed out, the authorization page usually failed to send the callback back to the local listening port (see the port below). Close this dialog and try again; if it keeps timing out, make sure port 17388 on this machine is not taken by another program. You can also use “Import local accounts” instead.",
  "trae.comp.oauth.start": "Start {variant} web sign-in",
  "trae.comp.oauth.startBusy": "Starting sign-in for {variant}…",
  "trae.comp.oauth.waiting": "Waiting for authorization; please finish signing in in your browser…",
  "trae.comp.oauth.remaining": "{time} left",
  "trae.comp.oauth.callback": "Local callback listener: ",
  "trae.comp.oauth.result": "Account added: {name}",
  "trae.comp.oauth.close": "Close",
  "trae.comp.oauth.done": "Done",
  "trae.comp.oauth.retry": "Restart sign-in",
  "trae.comp.oauth.retryBusy": "Starting sign-in…",
  "trae.comp.oauth.launch": "Launch the {variant} client",
  "trae.comp.oauth.launchBusy": "Launching the {variant} client…",
  // ---- Remote / client-less deployment (issue #62): the authorize page probes 127.0.0.1:17388 ----
  "trae.comp.oauth.tunnel.title": "Remote deployment needs one extra step: the authorize page probes local port {port}",
  "trae.comp.oauth.tunnel.desc": "While stuck on \"authorizing\", the Trae authorize page probes 127.0.0.1:{port} to confirm the client is online. A server has no such port, and the browser you click in runs on your own machine — open a port forward there first, and keep the terminal open:",
  "trae.comp.oauth.tunnel.copy": "Copy command",
  "trae.comp.oauth.tunnel.copied": "Copied",
  "trae.comp.oauth.pasteCallback.title": "Callback never arrived? Paste the address bar here",
  "trae.comp.oauth.pasteCallback.hint": "After approving, the browser jumps to 127.0.0.1:{port}/authorize?… and fails to load — copy that whole URL here and the server finishes the exchange.",
  "trae.comp.oauth.pasteCallback.placeholder": "http://127.0.0.1:{port}/authorize?authCodeInfo=…",
  "trae.comp.oauth.pasteCallback.submit": "Submit callback",
  "trae.comp.oauth.pasteCallback.busy": "Exchanging…",
  "trae.comp.oauth.keyNote": "Also: Trae signs token exchange and refresh with the icube device private key stored in the client's storage.json. Without a client on this machine there is no such key — copy storage.json from the machine that has the client to the same path here, or use \"Import backup\" (backups carry no private key, so refresh will fail and the account only lasts until the access_token expires).",
  "trae.comp.oauth.launchOk": "The {variant} client has been launched. Wait for it to write its device credential (a first launch also shows a sign-in page — sign in there once), then click “Restart sign-in”.",

  // =====================================================================
  // trae-import-accounts-dialog.tsx —— Import accounts
  // =====================================================================
  "trae.comp.import.title": "Import accounts",
  "trae.comp.import.desc": "Choose a Trae account backup JSON, then select the accounts to import.",
  "trae.comp.import.uid": "UID · {uid}",
  "trae.comp.import.item": "Item {index}",
  "trae.comp.import.chooseFile": "Choose file",
  "trae.comp.import.parsing": "Parsing…",
  "trae.comp.import.summary": "{count} of {total} accounts selected",
  "trae.comp.import.selectAll": "Select all",
  "trae.comp.import.deselectAll": "Deselect all",
  "trae.comp.import.missingJwt": "Missing JWT",
  "trae.comp.import.cancel": "Cancel",
  "trae.comp.import.busy": "Importing…",
  "trae.comp.import.submit": "Import selected",

  // =====================================================================
  // trae-export-accounts-dialog.tsx —— Export accounts
  // =====================================================================
  "trae.comp.export.title": "Export accounts",
  "trae.comp.export.desc": "Select the Trae accounts to export into a JSON file.",
  "trae.comp.export.uid": "UID · {uid}",
  "trae.comp.export.reveal.windows": "Show in File Explorer",
  "trae.comp.export.reveal.linux": "Show in file manager",
  "trae.comp.export.reveal.mac": "Show in Finder",
  "trae.comp.export.saveTitle": "Export Trae accounts",
  "trae.comp.export.warningTitle": "Security notice",
  "trae.comp.export.warningBody": "The exported file contains a Cloud-IDE-JWT, which is equivalent to a password. Do not upload it to cloud storage or send it to anyone.",
  "trae.comp.export.empty": "There are no accounts to export.",
  "trae.comp.export.summary": "{count} of {total} accounts selected",
  "trae.comp.export.selectAll": "Select all",
  "trae.comp.export.deselectAll": "Deselect all",
  "trae.comp.export.successTitle": "Export complete",
  "trae.comp.export.done": "Done",
  "trae.comp.export.cancel": "Cancel",
  "trae.comp.export.submit": "Export selected",
};
