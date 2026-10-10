/**
 * 文案域：**Trae gateway management**（英文）。
 *
 * ⚠️ 不写类型注解：键名合法性由 `src/locales/en.ts` 单点校验。域文件保持零依赖。
 * 键必须与 `traeGateway.zh.ts` **一一对应**（缺哪条就回落中文）。
 */
export const en = {
  // =====================================================================
  // gateway/trae-model-list.tsx —— model list
  // =====================================================================
  "trae.gateway.models.title": "Model list",
  "trae.gateway.models.summary": "{count} total · default {model}",
  "trae.gateway.models.note": "This list is read from the Trae client's local cache (pushed by upstream), so it follows the client's refresh; the exposed list (/v1/models) follows the API key's owned client, and the two clients differ.",
  "trae.gateway.models.empty": "No model data yet.",
  "trae.gateway.models.emptyDesc": "This is empty when the client has not started, is not signed in, or has not fetched a list yet; after refreshing the client's model list, click \"Reload\".",
  "trae.gateway.models.refresh": "Reload",
  "trae.gateway.models.scanning": "Reading…",
  "trae.gateway.models.program": "Select target client",
  "trae.gateway.models.programTip": "Read the model list of the {label} client",
  /* Source badge (two levels — deliberately NOT unified into WB's three levels:
     the mechanism differs, and unifying would make the UI lie; see P10 open item #3). */
  "trae.gateway.models.sourceCache": "Client cache",
  "trae.gateway.models.sourceMissing": "Not loaded",
  /* Fallback reasons (used when the backend `note` is missing) — contract: reason is always non-empty. */
  "trae.gateway.models.reasonCache": "Read from the client's local cache (pushed by upstream), not live from upstream",
  "trae.gateway.models.reasonMissing": "No list has been read from the client yet",
  "trae.gateway.models.readFrom": "Read from the {label} client",
  "trae.gateway.models.gatewayCount": "{count} exposed by gateway",
  "trae.gateway.models.gatewayCountFor": "{count} exposed by gateway ({program})",
  /* P11: group chips became a table; these keys are dropped (see traeGateway.zh.ts). */

  // =====================================================================
  // gateway/trae-request-log.tsx —— request log
  // =====================================================================
  "trae.gateway.log.title": "Request log (last {count})",
  "trae.gateway.log.clear": "Clear",
  "trae.gateway.log.empty": "No requests yet. Once the gateway is running, every call from a client is recorded here (metadata only by default, no bodies).",
  "trae.gateway.log.col.time": "Time",
  "trae.gateway.log.col.account": "Account",
  "trae.gateway.log.col.model": "Model",
  "trae.gateway.log.col.status": "Status",
  "trae.gateway.log.col.latency": "Latency",
  "trae.gateway.log.col.tokens": "Token",

  // =====================================================================
  // gateway/trae-integration-guide.tsx —— integration guide
  // =====================================================================
  "trae.gateway.guide.title": "Integration guide",
  "trae.gateway.guide.copy": "Copy code",
  "trae.gateway.guide.copied": "Code copied",
  "trae.gateway.guide.noKey": "sk-trae-… (create a key above first)",
  "trae.gateway.guide.streamNote": "The Trae upstream only supports streaming; when a request sets `stream: false`, this gateway aggregates locally and returns it at once, with longer first-byte latency.",
  "trae.gateway.guide.autoDiscover": "The client reads the model list from `/v1/models` automatically — you do not need to fill in model names; if your tool does not support auto-discovery, copy one from the \"Models\" page.",
  // Field labels inside the copyable snippet
  /* P3: `trae.gateway.guide.snippet.*` removed — labels are now shared. */

  // =====================================================================
  // The account pool card is now **shared by both sides**
  // (`gateway/account-pool-card.tsx`); its strings moved to
  // `shared.gateway.pool.*` / `shared.poolStatus.*` (see `shared.en.ts`).
  // =====================================================================

  // =====================================================================
  // gateway/trae-api-key-table.tsx —— API Key list / create / revoke / delete
  // =====================================================================
  // ---- Toasts ----
  "trae.gateway.key.loadFailed": "Failed to load the API Key list",
  "trae.gateway.key.nameRequired": "Please enter a name",
  "trae.gateway.key.noPlaintext": "Created successfully but no plaintext returned; please retry",
  "trae.gateway.key.createFailed": "Create failed",
  "trae.gateway.key.revoked": "Revoked",
  "trae.gateway.key.revokeFailed": "Revoke failed",
  "trae.gateway.key.deleted": "Deleted",
  "trae.gateway.key.deleteFailed": "Delete failed",
  "trae.gateway.key.copied": "API Key copied",

  // ---- List ----
  "trae.gateway.key.create": "Create API Key",
  "trae.gateway.key.loading": "Loading…",
  "trae.gateway.key.empty": "No API Keys created yet.",
  "trae.gateway.key.neverUsed": "Never used",
  "trae.gateway.key.statusRevoked": "Revoked",
  "trae.gateway.key.statusActive": "Enabled",
  "trae.gateway.key.revoke": "Revoke",
  "trae.gateway.key.delete": "Delete",

  // ---- Table headers ----
  "trae.gateway.key.name": "Name",
  "trae.gateway.key.col.variant": "Owned client",
  "trae.gateway.key.col.prefix": "Prefix",
  "trae.gateway.key.col.createdAt": "Created",
  "trae.gateway.key.col.lastUsed": "Last used",
  "trae.gateway.key.col.status": "Status",
  "trae.gateway.key.col.actions": "Actions",

  // ---- Create dialog ----
  "trae.gateway.key.createDesc": "Each key can only reach the models and account pool of its owned client — TraeWork and TraeCode offer different models.",
  "trae.gateway.key.namePlaceholder": "e.g. Cursor",
  "trae.gateway.key.variant": "Owned client",
  "trae.gateway.key.cancel": "Cancel",
  "trae.gateway.key.createSubmit": "Create",

  // ---- One-time plaintext ----
  "trae.gateway.key.createdTitle": "API Key created",
  "trae.gateway.key.createdDesc": "The full key is shown only this once; copy and save it immediately.",
  "trae.gateway.key.copy": "Copy",
  "trae.gateway.key.saved": "I have saved it, close",

  // ---- Revoke / delete confirmations ----
  "trae.gateway.key.revokeTitle": "Revoke API Key",
  "trae.gateway.key.revokeDesc": "After revocation, \"{name}\" becomes invalid immediately (401) and remains listed as \"revoked\".",
  "trae.gateway.key.deleteTitle": "Delete API Key",
  "trae.gateway.key.deleteDesc": "Delete the revoked key \"{name}\"? This cannot be undone.",
};
