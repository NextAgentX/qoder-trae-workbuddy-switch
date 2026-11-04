/**
 * 文案域：**Qoder API gateway**（英文）。
 *
 * ⚠️ 不写类型注解：键名合法性由 `src/locales/en.ts` 单点校验。域文件保持零依赖。
 * 键必须与 `qoderGateway.zh.ts` **一一对应**（缺哪条就回落中文）。
 */
export const en = {
  // ---- Page head / service card (QoderApiServicePage.tsx) ----
  "qoder.gateway.title": "Gateway",
  "qoder.gateway.enable": "Enable local gateway",
  "qoder.gateway.enableDesc": "Serve OpenAI / Anthropic compatible endpoints on 127.0.0.1 · upstream {upstream}",
  "qoder.gateway.bindAddr": "Listen address",
  "qoder.gateway.bindLoopback": "Local only (127.0.0.1)",
  "qoder.gateway.bindLan": "LAN (0.0.0.0)",
  "qoder.gateway.port": "Port",
  "qoder.gateway.statusLabel": "Status",
  "qoder.gateway.running": "Running",
  "qoder.gateway.stopped": "Stopped",
  "qoder.gateway.noteLead": "Every gateway serves",
  "qoder.gateway.noteTail": "— the ports must differ (this page is Qoder, default 57895).",
  "qoder.gateway.lanWarning": "LAN access is enabled — make sure this network is trusted.",
  "qoder.gateway.lastError": "Last error",
  "qoder.gateway.refresh": "Refresh",
  "qoder.gateway.savedStarted": "Saved and listening",
  "qoder.gateway.savedDisabled": "Saved: listener stopped",
  "qoder.gateway.saveFailed": "Save failed",
  "qoder.gateway.portInvalid": "Invalid port",
  "qoder.gateway.portRange": "Port must be between 1 and 65535",
  "qoder.gateway.loadFailed": "Failed to load gateway information",
  "qoder.gateway.retry": "Retry",

  // ---- Endpoint card ----
  "qoder.gateway.endpoint.title": "Endpoint",
  "qoder.gateway.endpoint.baseUrl": "Base URL",
  "qoder.gateway.endpoint.keyPrefix": "API key",
  "qoder.gateway.endpoint.noKey": "No key yet (create one on the Keys tab)",
  "qoder.gateway.endpoint.copied": "Endpoint copied",
  "qoder.gateway.copy": "Copy",

  // ---- Overview rows ----
  "qoder.gateway.overview.models": "Available models",
  "qoder.gateway.overview.modelsCount": "{count}",
  "qoder.gateway.overview.pool": "Account pool",
  "qoder.gateway.overview.poolCount": "{count} entries",
  "qoder.gateway.overview.view": "View",

  // ---- LAN risk dialog ----
  "qoder.gateway.lanDialog.title": "Allow LAN access?",
  "qoder.gateway.lanDialog.desc":
    "Binding 0.0.0.0 lets any device on this network call the gateway with your key (spending your Qoder quota). Only enable it on a trusted network.",
  "qoder.gateway.lanDialog.cancel": "Cancel",
  "qoder.gateway.lanDialog.confirm": "Enable",

  // ---- Integration guide (the two Qoder-specific strings; the rest reuses trae.gateway.guide.*) ----
  "qoder.gateway.guide.noKey": "sk-qoder-… (create a key above first)",
  "qoder.gateway.guide.streamNote":
    "The Qoder upstream is streaming-only; when you send `stream: false` this gateway aggregates it locally and returns one response, so the first byte takes longer.",

  // ---- Request log (the log card is shared with Trae; only these two toasts are Qoder's) ----
  "qoder.gateway.logsCleared": "Logs cleared",
  "qoder.gateway.logsClearFailed": "Failed to clear logs",

  // ---- Model list (qoder-model-list.tsx) ----
  "qoder.gateway.quota.title": "Quota guard",
  "qoder.gateway.quota.refreshFailed": "Couldn't read quota status",
  "qoder.gateway.quota.desc": "Pause an account once a threshold is hit (0 = off; unknown values always pass)",
  "qoder.gateway.quota.refresh": "Refresh",
  "qoder.gateway.quota.reserveLabel": "Balance floor (credits)",
  "qoder.gateway.quota.reserveHint": "Stops when remaining ≤ this; unknown balance never blocks",
  "qoder.gateway.quota.dailyLabel": "Daily token limit",
  "qoder.gateway.quota.dailyHint": "Tokens this account relayed through the gateway today",
  "qoder.gateway.quota.creditLabel": "Daily credit limit",
  "qoder.gateway.quota.creditHint": "Credits this account spent via the gateway today (decimals ok)",
  "qoder.gateway.quota.creditsUsed": "{n} credits used",
  "qoder.gateway.quota.why.daily_credits": "Daily credits used up",
  "qoder.gateway.quota.modelLabel": "Per-model daily tokens",
  "qoder.gateway.quota.modelHint": "Only that model is limited; other models keep working",
  "qoder.gateway.quota.invalid": "Non-negative integers only (0 = off)",
  "qoder.gateway.quota.noteOn": "Counted by the gateway itself; resets daily at 10:00 (UTC+8)",
  "qoder.gateway.quota.noteOff": "All three are 0 ⇒ guard is off, nothing is blocked",
  "qoder.gateway.quota.save": "Save",
  "qoder.gateway.quota.loading": "Reading quota status…",
  "qoder.gateway.quota.empty": "No accounts in this region yet",
  "qoder.gateway.quota.today": "Today ({date}) · {count} accounts",
  "qoder.gateway.quota.blockedCount": "{count} blocked",
  "qoder.gateway.quota.allClear": "Nothing blocked right now",
  "qoder.gateway.quota.noModelUsage": "No gateway usage today",
  "qoder.gateway.quota.credits": "balance {n}",
  "qoder.gateway.quota.tokens": "{n} tokens",
  "qoder.gateway.quota.ok": "Available",
  "qoder.gateway.quota.why.parked": "Out of balance (parked)",
  "qoder.gateway.quota.why.reserve": "Balance floor hit",
  "qoder.gateway.quota.why.daily_tokens": "Daily tokens used up",
  "qoder.gateway.quota.why.model_tokens": "Model quota used up",
  "qoder.gateway.quota.why.other": "Blocked",
  "qoder.gateway.models.title": "Model list",
  "qoder.gateway.models.promotionWindow": "Off-peak {range} (UTC+8)",
  "qoder.gateway.models.note":
    "This list comes straight from Qoder (fetching it needs account credentials) and is cached for 10 minutes; disabled models disappear from /v1/models.",
  "qoder.gateway.models.refresh": "Re-detect",
  "qoder.gateway.models.refreshFailed": "Re-detect failed",
  "qoder.gateway.models.scanning": "Detecting…",
  "qoder.gateway.models.sourceLive": "Live",
  "qoder.gateway.models.sourceCached": "Last success",
  "qoder.gateway.models.sourceUnavailable": "No list yet",
  "qoder.gateway.models.reasonCached": "This fetch failed — showing the last successful result",
  "qoder.gateway.models.reasonUnavailable":
    "No list has ever been fetched: check that an account is usable, then re-detect.",
  "qoder.gateway.models.empty": "No models to show yet",
  "qoder.gateway.models.emptyDesc":
    "Fetching the list needs account credentials — import an account on the Accounts tab, then re-detect.",

  // ---- Key table (qoder-api-key-table.tsx) ----
  "qoder.gateway.key.create": "Create key",
  "qoder.gateway.key.createDesc": "A key is bound to one region: it decides which account pool the key uses.",
  "qoder.gateway.key.name": "Name",
  "qoder.gateway.key.namePlaceholder": "e.g. CI automation",
  "qoder.gateway.key.nameRequired": "Please enter a name",
  "qoder.gateway.key.region": "Region",
  "qoder.gateway.key.regionHint": "The two regions are separate account pools and endpoints; a key cannot cross regions.",
  "qoder.gateway.key.cancel": "Cancel",
  "qoder.gateway.key.createSubmit": "Create",
  "qoder.gateway.key.createFailed": "Create failed",
  "qoder.gateway.key.loadFailed": "Failed to load keys",
  "qoder.gateway.key.empty": "No keys yet",
  "qoder.gateway.key.col.region": "Region",
  "qoder.gateway.key.col.prefix": "Key",
  "qoder.gateway.key.col.createdAt": "Created",
  "qoder.gateway.key.col.lastUsed": "Last used",
  "qoder.gateway.key.col.status": "Status",
  "qoder.gateway.key.col.actions": "Actions",
  "qoder.gateway.key.statusActive": "Active",
  "qoder.gateway.key.statusRevoked": "Revoked",
  "qoder.gateway.key.neverUsed": "Never used",
  "qoder.gateway.key.revoke": "Revoke",
  "qoder.gateway.key.revokeTitle": "Revoke this key?",
  "qoder.gateway.key.revokeDesc":
    "\"{name}\" stops working immediately (reversible): revoked rows stay in the table and can be restored at any time.",
  "qoder.gateway.key.revoked": "Revoked",
  "qoder.gateway.key.revokeFailed": "Revoke failed",
  "qoder.gateway.key.restore": "Restore",
  "qoder.gateway.key.restoreTitle": "Restore this key?",
  "qoder.gateway.key.restoreDesc":
    "\"{name}\" becomes usable again — it is the same key (plaintext unchanged), so clients need no reconfiguration.",
  "qoder.gateway.key.restored": "Restored",
  "qoder.gateway.key.restoreFailed": "Restore failed",
  "qoder.gateway.key.delete": "Delete",
  "qoder.gateway.key.deleteTitle": "Delete this key?",
  "qoder.gateway.key.deleteDesc": "The record for \"{name}\" is deleted permanently. Clients using it get 401 immediately.",
  "qoder.gateway.key.deleted": "Deleted",
  "qoder.gateway.key.deleteFailed": "Delete failed",
};
