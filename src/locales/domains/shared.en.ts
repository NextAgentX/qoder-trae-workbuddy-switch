/**
 * 文案域：**跨切面共享文案**（英文）。
 *
 * ⚠️ 不写类型注解：键名合法性由 `src/locales/en.ts` 单点校验。域文件保持零依赖。
 * 键必须与 `shared.zh.ts` **一一对应**（缺哪条就回落中文）。
 */
export const en = {
  "shared.gateway.keys.note":
    "The plaintext key is shown once at creation; only the mask is kept here. Request counts are lifetime totals for that key, not the “today” figure on the overview. Revocation is reversible: a revoked key stays in the list and can be restored (the same key is stored, so clients need no changes) or deleted.",
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
  /* P3: the merge toggle is a `.seg` of two buttons (prototype `viewUsage` params). */
  "shared.region.combine.only": "This scope only",
  "shared.region.combine.all": "Merge all scopes",

  // ---- Gateway account pool status (gateway.ts — one card shared by both sides) ----
  "shared.poolStatus.available": "Available",
  "shared.poolStatus.cooling": "Cooling",
  "shared.poolStatus.disabled": "Session expired",
  "shared.poolStatus.expired": "Credits expired",
  "shared.poolStatus.noCredits": "No credits",
  "shared.poolStatus.excluded": "Excluded",

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
  "shared.gateway.sourceBar.yesterday": "yesterday",
  "shared.gateway.sourceBar.daysAgo": "{n} d ago",
  /* Model table (P11 §1.6) — components/gateway/model-table.tsx, shared by both platforms.
     Columns = model / upstream / gateway exposure; no "scope" column (#1), no "protocol" column (#2). */
  "shared.gateway.models.colModel": "Model",
  "shared.gateway.models.colContext": "Context",
  "shared.gateway.models.contextTitle": "Context limit",
  "shared.gateway.models.contextFixedDesc": "On: lists and requests use the upstream maximum window (e.g. 1M). Off: fall back to the default window (e.g. 300k). Numbers come from the official catalog.",
  "shared.gateway.models.contextFromCatalog": "From catalog",
  "shared.gateway.models.colImages": "Image input",
  "shared.gateway.models.imageYes": "Supported",
  "shared.gateway.models.imageNo": "Not supported",
  "shared.gateway.models.imageUndeclared": "Not declared",
  "shared.gateway.models.imageSourceUpstream": "Declared image-capable by the upstream catalog",
  "shared.gateway.models.imageSourceOverride": "Manually set on this page (overrides upstream)",
  "shared.gateway.models.imageUndeclaredHint":
    "The upstream catalog does not say whether this model accepts images. Clients default to \"not supported\" and drop images before the request reaches the gateway — mark it here if you know better.",
  "shared.gateway.models.imageMarkYes": "Mark as image-capable",
  "shared.gateway.models.imageMarkNo": "Mark as not image-capable",
  "shared.gateway.models.imageReset": "Restore upstream declaration",
  "shared.gateway.models.imageYesToast": "Marked \"{name}\" as image-capable",
  "shared.gateway.models.imageNoToast": "Marked \"{name}\" as not image-capable",
  "shared.gateway.models.imageResetToast": "Restored \"{name}\" to the upstream declaration",
  "shared.gateway.models.colUpstream": "Upstream",
  "shared.gateway.models.colExposed": "Gateway exposure",
  /* issue #65: thinking levels + credit multiplier columns. */
  "shared.gateway.models.colEfforts": "Thinking",
  "shared.gateway.models.colCredits": "Credits",
  "shared.gateway.models.effortDefault": "Default: {effort}",
  "shared.gateway.models.effortNoneUnknown": "This program's model list does not declare thinking levels",
  "shared.gateway.models.effortNoneUnsupported": "This model has no selectable thinking level",
  "shared.gateway.models.effortNoneUndeclared": "Not declared upstream, and the built-in table has no entry",
  "shared.gateway.models.effortSourceUpstream": "Levels come from the upstream catalog",
  "shared.gateway.models.effortSourceStatic": "Levels come from the gateway's built-in table (not declared upstream; may lag the official client)",
  "shared.gateway.models.creditsFree": "Free",
  "shared.gateway.models.creditsNone": "Upstream provides no credit multiplier",
  "shared.gateway.models.creditsHint": "Multiplier of the base price, not an amount",
  "shared.gateway.models.upstreamAvailable": "Available",
  "shared.gateway.models.upstreamUnavailable": "Unavailable",
  "shared.gateway.models.exposed": "Exposed",
  "shared.gateway.models.disabled": "Disabled",
  "shared.gateway.models.disable": "Disable",
  "shared.gateway.models.enable": "Enable",
  "shared.gateway.models.copyId": "Copy model ID",
  "shared.gateway.models.idCopied": "Model ID copied",
  "shared.gateway.models.count": "{served} of {total} exposed",
  /* ★ Both exits take effect together — otherwise the disable switch is fake (§3.18.1). */
  "shared.gateway.models.twoExits":
    "A disabled model does not appear in `/v1/models`, and calling it directly is rejected with an explanation — both exits take effect together, otherwise the switch is fake.",
  "shared.gateway.models.disabledToast": "Disabled “{name}”",
  "shared.gateway.models.enabledToast": "Enabled “{name}”",
  "shared.gateway.models.toggleFail": "Action failed",
  // ---- Rate-limit probe (components/gateway/model-probe-card.tsx + the new model-list column) ----
  // Red line: only "model rate limit / account rate limit / model blocked" count as throttling;
  // "no credits" and "auth dead" are separate.
  // ★ Security S1: the notice stays visible at all times (probes hit the real upstream).
  // ★ Security S2: `reason` (raw upstream text) must be rendered as plain text only.
  "shared.gateway.models.colProbe": "Rate limit (probe)",
  // ── Model fallback (P13) ──
  "shared.gateway.fallback.title": "Model fallback",
  "shared.gateway.fallback.badgeOff": "Not configured",
  "shared.gateway.fallback.badgeOn": "Enabled · {count}",
  "shared.gateway.fallback.badgeClosed": "Off",
  "shared.gateway.fallback.limit": "Same scope · up to {max} hops",
  "shared.gateway.fallback.notice": "Triggered only on explicit upstream failures. A stream that has already reached the client is never rolled back, and a failure is never rewritten as success — clients can read X-Buddy-Fallback-To to see the model actually used.",
  "shared.gateway.fallback.enable": "Enable fallback",
  "shared.gateway.fallback.enableHint": "Config is kept when off, but nothing triggers",
  "shared.gateway.fallback.new": "New fallback chain",
  "shared.gateway.fallback.emptyTitle": "No fallback chains yet",
  "shared.gateway.fallback.emptyDesc": "When a model fails explicitly upstream (global rate limit, blocked, 5xx), the gateway can resend with a backup model you choose instead of handing the error back. Off by default.",
  "shared.gateway.fallback.addTarget": "+ Backup",
  "shared.gateway.fallback.chainOff": "Disabled",
  "shared.gateway.fallback.chainDisable": "Disable",
  "shared.gateway.fallback.chainEnable": "Enable",
  "shared.gateway.fallback.footNote": "Status dots read live gate and cooldown state (same source as the account pool), not the config snapshot. A successful fallback does not clear the primary model's gate — it really did fail; once the gate expires, requests return to the primary on their own, with no switch-back toggle.",
  "shared.gateway.fallback.diag.gated": "Hop {hop} {model} is gated ({secs}s left). The chain skips to the next hop; if none work, the original error is returned instead of waiting.",
  "shared.gateway.fallback.diag.disabled": "Hop {hop} {model} is disabled and will be skipped.",
  "shared.gateway.fallback.diag.noVision": "Hop {hop} {model} does not accept image input while the primary does. Requests with images skip this hop and move to the next backup instead of sending a request that is bound to fail.",
  "shared.gateway.fallback.stats": "Last {days} days: {total} fallbacks ({ok} succeeded · {fail} failed)",
  "shared.gateway.fallback.events": "Fallback log",
  "shared.gateway.fallback.eventsHide": "Hide log",
  "shared.gateway.fallback.expand": "Expand",
  "shared.gateway.fallback.collapse": "Collapse",
  "shared.gateway.fallback.event.time": "Time",
  "shared.gateway.fallback.event.path": "Actual path",
  "shared.gateway.fallback.event.reason": "Trigger",
  "shared.gateway.fallback.event.account": "Account",
  "shared.gateway.fallback.event.cost": "Duration",
  "shared.gateway.fallback.event.result": "Result",
  "shared.gateway.fallback.event.degraded": "Degraded",
  "shared.gateway.fallback.event.failed": "Backups failed",
  "shared.gateway.fallback.event.blocked": "Not degraded by rule",
  "shared.gateway.fallback.event.empty": "No fallback events yet.",
  "shared.gateway.fallback.event.note": "Only the latest {count} entries, and they never feed any aggregate: the request log is a ring buffer (200 by default) covering minutes on a busy gateway. A 7-day fallback rate must come from a new ledger dimension, not this table.",
  "shared.gateway.fallback.drawer.new": "New fallback chain",
  "shared.gateway.fallback.drawer.edit": "Edit fallback · {model}",
  "shared.gateway.fallback.field.from": "Primary model",
  "shared.gateway.fallback.field.fromLocked": "The primary cannot be changed. Delete and recreate to switch it — changing from is a different chain.",
  "shared.gateway.fallback.field.crossModelPlaceholder": "target platform model id",
  "shared.gateway.fallback.field.thisScope": "this scope",
  "shared.gateway.fallback.field.crossPlatformPending": "cross-platform only from WorkBuddy",
  "shared.gateway.fallback.field.targets": "Backup models (tried in order)",
  "shared.gateway.fallback.field.targetsHint": "Up to {max} (with the primary, {hops} hops total). Disabled or unavailable models are not listed.",
  "shared.gateway.fallback.capsNote": "Requests with images automatically skip backups that do not accept image input; tools, structured output and context window are not checked in this version — the backup accepts or rejects them itself.",
  "shared.gateway.fallback.field.triggers": "Triggers",
  "shared.gateway.fallback.trigger.gate": "Model gate paused",
  "shared.gateway.fallback.trigger.gateDesc": "The model has been failing across accounts and is paused gateway-wide. Free to skip — no upstream request is sent.",
  "shared.gateway.fallback.trigger.model_rate": "Model-level rate limit (6004)",
  "shared.gateway.fallback.trigger.model_rateDesc": "Upstream says this model specifically is limited; another account will not help.",
  "shared.gateway.fallback.trigger.model_blocked": "Model blocked (11102)",
  "shared.gateway.fallback.trigger.model_blockedDesc": "Upstream blocks this model; it will not recover soon.",
  "shared.gateway.fallback.trigger.server": "Upstream 5xx",
  "shared.gateway.fallback.trigger.serverDesc": "Server fault. Enabling this pushes traffic to backups during an outage, which may take them down too.",
  "shared.gateway.fallback.trigger.timeout": "Request timeout",
  "shared.gateway.fallback.trigger.timeoutDesc": "No response within 600s. Long jobs may be billed twice.",
  "shared.gateway.fallback.trigger.not_found": "Model not found (404)",
  "shared.gateway.fallback.trigger.not_foundDesc": "Upstream says the model does not exist: it may be missing from that account's list (another account helps) or removed upstream entirely (another model helps).",
  "shared.gateway.fallback.short.gate": "gate",
  "shared.gateway.fallback.short.model_rate": "6004",
  "shared.gateway.fallback.short.model_blocked": "11102",
  "shared.gateway.fallback.short.server": "5xx",
  "shared.gateway.fallback.short.timeout": "timeout",
  "shared.gateway.fallback.short.not_found": "404",
  "shared.gateway.fallback.never.title": "Never falls back",
  "shared.gateway.fallback.never.desc": "Bad request 4xx: another model cannot fix it and only makes the error more confusing. Content moderation refusal: switching models means switching moderation thresholds, which effectively helps callers bypass review. 402 out of credits: an account-level quota problem. Key quota exhausted: metered per key, unrelated to the model.",
  "shared.gateway.fallback.sim.title": "Simulated path · using current gate and cooldown state",
  "shared.gateway.fallback.sim.primary": "Primary",
  "shared.gateway.fallback.sim.target": "Backup {n}",
  "shared.gateway.fallback.sim.try": "tried first",
  "shared.gateway.fallback.sim.willUse": "available → will be used",
  "shared.gateway.fallback.sim.skipGated": "still paused → skipped, no request",
  "shared.gateway.fallback.sim.skipDisabled": "unavailable → skipped",
  "shared.gateway.fallback.sim.allFail": "If no hop works, the original error is returned — no endless waiting, and no rewriting a failure as success.",
  "shared.gateway.fallback.save": "Save",
  "shared.gateway.fallback.cancel": "Cancel",
  "shared.gateway.fallback.saveOk": "Fallback chain saved",
  "shared.gateway.fallback.saveOkDesc": "Takes effect immediately, no gateway restart",
  "shared.gateway.fallback.needTarget": "Pick at least one backup model",
  "shared.gateway.fallback.needTargetDesc": "A primary alone is not a fallback chain",
  "shared.gateway.fallback.enabledToast": "Fallback enabled",
  "shared.gateway.fallback.disabledToast": "Fallback disabled",
  "shared.gateway.probe.title": "Rate-limit probe",
  "shared.gateway.probe.badge": "Account × model live probe",
  "shared.gateway.probe.expand": "Expand",
  "shared.gateway.probe.collapse": "Collapse",
  "shared.gateway.probe.summaryAvailable": "{accounts} accounts · {models} models probeable",
  "shared.gateway.probe.summaryLast": "Last {time} · adopted +{added}/-{cleared}",
  "shared.gateway.probe.summaryLastPlain": "Last {time}",
  "shared.gateway.probe.summaryLastNoAdopt": "Last {time} · not adopted",
  "shared.gateway.probe.notice":
    "Sends one minimal request to each account — almost no credits, but it does leave a trace upstream and may trigger rate limiting.",
  "shared.gateway.probe.stepModels": "Which models to test",
  "shared.gateway.probe.stepAccounts": "Test on these accounts",
  "shared.gateway.probe.selected": "{n}/{total} selected",
  "shared.gateway.probe.selectAll": "Select all",
  "shared.gateway.probe.clearAll": "Clear",
  "shared.gateway.probe.noModels": "No probeable models in this scope yet — identify the model list first.",
  "shared.gateway.probe.noAccounts": "The account pool is empty in this scope — add an account first.",
  "shared.gateway.probe.adoptTitle": "Adopt results into the account pool",
  "shared.gateway.probe.adoptDesc":
    "Available ⇒ clears that model's cooldown; rate-limited / blocked ⇒ sets that model's cooldown. Cooldowns only — backoff counters are not advanced.",
  "shared.gateway.probe.adoptAria": "Adopt into the account pool",
  "shared.gateway.probe.adoptUnsupportedTitle": "Refill is not supported on this platform",
  "shared.gateway.probe.adoptUnsupportedDesc":
    "Trae's account pool has no per-model cooldown — refilling could only cool the whole account and would take every other model of it down too. This page only shows the probe results; it never changes the account pool.",
  "shared.gateway.probe.footCells": "{accounts} accounts × {models} models = {cells} cells · almost no credits",
  "shared.gateway.probe.pickModelFirst": "Select the models to test first",
  "shared.gateway.probe.start": "Start probe",
  "shared.gateway.probe.startFail": "Failed to start the probe",
  "shared.gateway.probe.cancel": "Cancel remaining",
  "shared.gateway.probe.adoptOn": "Adopts into pool cooldowns when finished",
  "shared.gateway.probe.adoptOff": "No adoption (report only)",
  "shared.gateway.probe.runningCurrent": "Probing: {current}",
  "shared.gateway.probe.runningNote":
    "Serial within an account (bursting would trip a good account into 429) · up to 2 accounts in parallel · 150ms between cells · zero retries. A 429 from the probe is a signal, not a failure.",
  "shared.gateway.probe.doneCells": "{n} cells",
  "shared.gateway.probe.done": "Done",
  "shared.gateway.probe.cancelled": "Remaining cancelled (partial results below)",
  "shared.gateway.probe.adoptResult": "adopted set {added} / cleared {cleared}",
  // issue #88: "N cells rate-limited but 0 adopted" is exactly the case that needs
  // an explanation — the old build rendered nothing at all.
  "shared.gateway.probe.adoptNone":
    "{n} cell(s) came back rate-limited, but no cooldown was written: those accounts may no longer be in the pool, may be disabled, or already have a later cooldown (it is never shortened).",
  // Adoption details (§5.3 "adoption feedback" bar → expandable): account / model / set-or-cleared / until when.
  "shared.gateway.probe.adoptedDetail": "{n} cooldowns adopted · details",
  "shared.gateway.probe.adoptedAdded": "Cooldown set",
  "shared.gateway.probe.adoptedCleared": "Cleared",
  "shared.gateway.probe.adoptedUntil": "until {time}",
  "shared.gateway.probe.legendUnknown": "Not probed",
  // Passive knowledge split by source (§5.3 tag "real traffic" / "client ledger" — must be distinguishable).
  "shared.gateway.probe.legendPassiveFlow": "Passive · real traffic",
  "shared.gateway.probe.legendPassiveLedger": "Passive · client ledger",
  "shared.gateway.probe.matrixEmpty": "No probeable combination — select at least one account and one model.",
  "shared.gateway.probe.colAccount": "Account \\ model",
  "shared.gateway.probe.sumLimit": "Throttled",
  "shared.gateway.probe.rowLimitAccounts": "Throttled accounts",
  "shared.gateway.probe.resultProbed": "{probed} cells hit a rate limit / block in this run.",
  "shared.gateway.probe.resultPassive": "{passive} more are passive knowledge (not found by this run).",
  "shared.gateway.probe.resultHint":
    "Dashed = not probed; inset ring = passive knowledge (real traffic / client ledger). Only \"model rate limit / account rate limit / model blocked\" count as throttling — 402 no-credits and dead auth are listed separately.",
  "shared.gateway.probe.exportCsv": "Export CSV",
  "shared.gateway.probe.exportDone": "Probe results exported",
  "shared.gateway.probe.exportRows": "{n} rows",
  "shared.gateway.probe.retry": "Re-probe",
  "shared.gateway.probe.probeCell": "Probe this cell",
  "shared.gateway.probe.reprobeCell": "Re-probe this cell",
  "shared.gateway.probe.probedAt": "Probed at {time}",
  "shared.gateway.probe.cellTitle": "{account} · {model}\nStatus: {state}\nSource: {source}\n{reason}",
  "shared.gateway.probe.sourceProbed": "This probe",
  // The two passive sources must be distinguishable: flow = pool cooldowns (real traffic); ledger = client ledger.
  "shared.gateway.probe.sourcePassiveFlow": "Passive (real traffic)",
  "shared.gateway.probe.sourcePassiveLedger": "Passive (client ledger)",
  "shared.gateway.probe.sourcePassiveBoth": "Passive (real traffic + client ledger; real traffic takes precedence)",
  "shared.gateway.probe.sourceUnknown": "Not probed yet",
  "shared.gateway.probe.csv.account": "Account",
  "shared.gateway.probe.csv.model": "Model",
  "shared.gateway.probe.csv.state": "State",
  "shared.gateway.probe.csv.source": "Source",
  "shared.gateway.probe.csv.note": "Note",
  // State labels (mirroring the backend's snake_case serialized names).
  "shared.gateway.probe.state.ok": "Available",
  "shared.gateway.probe.state.rate_limited_model": "Model throttled",
  "shared.gateway.probe.state.rate_limited_account": "Account throttled",
  "shared.gateway.probe.state.model_blocked": "Model blocked",
  "shared.gateway.probe.state.no_credits": "No credits",
  "shared.gateway.probe.state.auth_dead": "Auth dead",
  "shared.gateway.probe.state.model_unavailable": "No such model",
  "shared.gateway.probe.state.upstream_error": "Upstream error",
  "shared.gateway.probe.state.upstream_queued": "Upstream queued",
  "shared.gateway.probe.state.transport_error": "Network error",
  "shared.gateway.probe.state.skipped": "Skipped",
  "shared.gateway.probe.state.unknown": "Not probed",
  // State explanations (hover detail; plain text).
  "shared.gateway.probe.reason.ok": "Upstream 200 — callable",
  "shared.gateway.probe.reason.rate_limited_model": "Upstream 429 · 6004 / IsModelRateLimit ⇒ only that model is throttled; switch to another",
  "shared.gateway.probe.reason.rate_limited_account": "Upstream 429 with no model marker ⇒ all models on this account are temporarily unavailable",
  "shared.gateway.probe.reason.model_blocked": "Upstream 11102 ⇒ this model is blocked for the account; backoff doubles (6h→12h→24h)",
  "shared.gateway.probe.reason.no_credits": "Upstream 402 ⇒ out of credits (not throttling! waiting won't help — top up)",
  "shared.gateway.probe.reason.auth_dead": "Upstream 12153 / 401 ⇒ session invalid, sign in again or import",
  "shared.gateway.probe.reason.model_unavailable": "Upstream 404 ⇒ this account cannot see / use that model",
  "shared.gateway.probe.reason.upstream_error": "Upstream 5xx / 4xx ⇒ unrelated to the account",
  "shared.gateway.probe.reason.upstream_queued": "Upstream queueing (10605 / isQueued) ⇒ this model is queued right now — not throttling, not an account issue",
  "shared.gateway.probe.reason.transport_error": "Connection failed ⇒ a network-layer issue, not an account issue",
  "shared.gateway.probe.reason.skipped": "Not probed: no credential (envelope) / account signed out / excluded",
  "shared.gateway.probe.reason.unknown": "Not probed yet (dashed = not probed; inset ring = passive knowledge)",
  // "Rate limit (probe)" column cells (clicking a badge drills down).
  "shared.gateway.probe.colAllOk": "All available",
  "shared.gateway.probe.colLimited": "{limited}/{total} throttled",
  "shared.gateway.probe.colUnavailable": "{unavailable}/{total} unavailable",
  "shared.gateway.probe.colPassive": "Passive {n}",
  "shared.gateway.probe.colNone": "—",
  "shared.gateway.probe.colTooltipAllOk": "This probe: all {total} accounts available — click to see the matrix",
  "shared.gateway.probe.colTooltipLimited": "This probe: {limited} of {total} accounts throttled / blocked — click to see the matrix",
  "shared.gateway.probe.colTooltipUnavailable": "This probe: {unavailable} of {total} accounts currently unavailable (upstream queueing / errors, not throttling) — click to see the matrix",
  "shared.gateway.probe.colTooltipPassive": "Passive knowledge: {flow} from real traffic, {ledger} from the client ledger ({n} accounts) — not actively probed yet; click to probe",
  "shared.gateway.probe.colTooltipNone": "This model has not been probed — pick it in the \"Rate-limit probe\" card above",
  // Sorting (§5.3: clickable header sort, default by throttled cells descending; Radix Select, not details/summary).
  "shared.gateway.probe.sortLabel": "Sort",
  "shared.gateway.probe.sortAria": "Matrix sort order",
  "shared.gateway.probe.sortThrottle": "By throttled accounts",
  "shared.gateway.probe.sortName": "By model name",
  // Cell detail popover (§5.3: click a cell → state / HTTP / upstream message / resetAt / latency + "what you can do").
  "shared.gateway.probe.detailSource": "Source",
  "shared.gateway.probe.detailHttp": "HTTP",
  "shared.gateway.probe.detailLatency": "Latency",
  "shared.gateway.probe.detailReset": "Resets at",
  "shared.gateway.probe.detailReason": "Upstream message",
  // Empty states (§5.3: all ok ⇒ "all available"; all skipped ⇒ sign-in / import guidance).
  "shared.gateway.probe.emptyAllOk": "All available: every probed cell returned 200 — nothing is throttled right now.",
  "shared.gateway.probe.emptyAllSkippedTitle": "All skipped: no account could send a request",
  "shared.gateway.probe.emptyAllSkippedDesc":
    "None of these accounts have a usable credential — they may be signed out, hold an encrypted-envelope credential, or have been removed from the account store. Sign in again or import an account on the \"Accounts\" page; if there is no desktop client on this machine (Docker / headless), add an account by QR scan under \"Gateway → Account pool\"; if the \"Pinned account\" strategy is in use, make sure that account is still in the local account store.",
  "shared.gateway.pool.totalRequests": "{count} requests",
  "shared.gateway.pool.tile.available": "Routable",
  // Sub-line inside the "Routable" tile: some of them are merely **at the in-flight cap**
  // (account is fine, clears by itself in seconds). See `GatewayPoolSummary.inFlightFull`
  // for why this is a sub-line and not a seventh tile.
  "shared.gateway.pool.tile.availableBusy": "{count} at in-flight cap",
  "shared.gateway.pool.tile.availableBusyHint":
    "These accounts are healthy; they are just at their concurrency cap because earlier requests are still running. They recover on their own in seconds — nothing to do.",
  "shared.gateway.pool.tile.cooling": "Cooling",
  "shared.gateway.pool.tile.disabled": "Session dead",
  "shared.gateway.pool.tile.expired": "Credits expired",
  "shared.gateway.pool.tile.zeroCredits": "No credits",
  "shared.gateway.pool.tile.excluded": "Excluded",
  // Qoder-only tile: upstream 429 is reported per model, so an account can be usable
  // while individual models cool down.
  "shared.gateway.pool.tile.modelCooling": "Model cooling",
  // Model-cooling badge (issue #88): the thaw time is mandatory — "N models
  // rate-limited" without a time tells the user nothing actionable.
  //
  // 2026-10-29: the cooldown now follows upstream's declared reset time
  // (`Pool::adopt_probe_result`), so the default wording is "thaw at {time}".
  // Only when upstream said nothing and we fall back to our own guess do we prefix
  // with "~", so nobody reads a guess as a promise.
  "shared.gateway.pool.modelCoolingBadge": "{count} model(s) rate-limited · thaw at {time}",
  "shared.gateway.pool.modelCoolingBadgeApprox":
    "{count} model(s) rate-limited · thaw ~{time}",
  "shared.gateway.pool.modelCoolingHint":
    "{models} is/are cooling at the model level (only those models are affected; the account still serves others). Earliest thaw at {time}.",
  // Per-model tooltip rows (issue #88 gap 4.2): name · remaining · fallback marker ·
  // upstream-declared reset time.
  "shared.gateway.pool.modelCoolingRow": "{model} · {remaining} left{fallback}{upstream}",
  // Upstream's own reset time, shown only when it differs from the actual cooldown end
  // (e.g. clipped by the 24h cap).
  "shared.gateway.pool.modelCoolingRowUpstream": " · upstream resets {time}",
  // "fallback" = upstream gave no wall clock; we guessed probe_fallback_ms (10 min).
  "shared.gateway.pool.modelCoolingFallback": " (fallback)",
  "shared.gateway.pool.empty": "The pool is empty. Add accounts under Accounts first.",
  "shared.gateway.pool.credits": "{credits} credits",
  "shared.gateway.pool.diagnoseNote": "When a request reports \"no available account\", check these reasons one by one:",
  "shared.gateway.pool.preferredLabel": "Preferred account",
  "shared.gateway.pool.preferredAria": "Account to consume first",
  "shared.gateway.pool.preferredAuto": "Auto (best available)",
  // WorkBuddy only: the second non-account option (writes `AccountStrategy::MaxCredits`).
  "shared.gateway.pool.strategyMaxCredits": "Pick by live credits (queried per request)",
  "shared.gateway.pool.strategyRandom": "Random (uniform draw among eligible accounts)",
  // issue #102: stick to one account, switch only when it is limited — reuses the
  // upstream prefix cache, which is what keeps token usage down.
  "shared.gateway.pool.strategySequential": "Sequential (stick to one account until limited)",
  "shared.gateway.pool.preferredMissing": "Preferred account (not in this pool)",
  // Preference, not a hard pin: falls back when the preferred account can't serve.
  "shared.gateway.pool.preferredHint": "A preference, not a hard pin: if it is cooling, dead, or out of credits, another account is used instead — requests are never rejected.",
  // Table columns and the "pick" column (prototype viewGateway :3057-3069).
  "shared.gateway.pool.colAccount": "Account",
  "shared.gateway.pool.colCredits": "Credits",
  "shared.gateway.pool.colStatus": "Status",
  "shared.gateway.pool.colPick": "Pick",
  "shared.gateway.pool.pin": "Pin",
  "shared.gateway.pool.unpin": "Unpin",
  "shared.gateway.pool.pinned": "Pinned",
  "shared.gateway.pool.revive": "Restore",
  // Result strings for the "Restore" exit — **shared by both platforms** (same action, same wording):
  // disabled is terminal on both WorkBuddy and Trae, and this is the only way out.
  "shared.gateway.pool.reviveDone": "Restored — the account is back in rotation",
  "shared.gateway.pool.reviveNoop": "That account was already usable — nothing to restore",
  "shared.gateway.pool.reviveFail": "Restore failed",
  // Selection priority and exclusivity note (prototype :3069).
  "shared.gateway.pool.exclusiveHint": "Priority: explicit pin > sticky session > auto-best. Pinning is exclusive — only one account is pinned at a time.",
  // The "Restore" exit: disabled is terminal, so without an exit the account stays dead forever.
  // ⚠️ Must NOT mention a "breaker": Trae has no such dimension and the string is shared.
  "shared.gateway.pool.exclude": "Exclude",
  "shared.gateway.pool.unexclude": "Un-exclude",
  "shared.gateway.pool.excludeDone": "Account excluded",
  "shared.gateway.pool.unexcludeDone": "Exclusion removed",
  "shared.gateway.pool.excludeFail": "Failed to exclude",
  "shared.gateway.pool.excludeHint": "Exclude = the gateway will never use this account (auto-pick, fallback, stickiness and pinning all skip it). It is NOT a disable - the account keeps working in the desktop client. Useful for reserving quota for your own use.",
  "shared.gateway.pool.reviveHint": "A disabled account never recovers on its own — Restore clears the disable and the cooldown at once so it rejoins rotation. If it is still unusable it will be disabled again.",

  // ---- Clipboard (clipboard.ts) ----
  "shared.clipboard.copied": "Copied",
  "shared.clipboard.failed": "Copy failed — select the text and copy it manually",

  // ---- Desktop companion window (components/companion/, a separate window) ----
  // ⚠️ Do NOT write a bare `/*` in a comment here: the scope gate strips comments with a
  //   naive regex, so `companion/*` inside a line comment swallows everything up to the next
  //   `*/` (it once dropped 250 lines of entries).
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
  // Source id → display name (ids are backend data; see SOURCE_LABEL_KEY in companion-row).
  // ★ Must cover every entry of `agent_watch::HOOK_SOURCES`; enforced by check-agent-sources.cjs.
  "shared.companion.source.workbuddyCn": "WorkBuddy (CN)",
  "shared.companion.source.workbuddyGlobal": "WorkBuddy (Global)",
  "shared.companion.source.claude": "Claude Code",
  "shared.companion.source.codebuddy": "CodeBuddy",
  "shared.companion.source.codex": "Codex",
  "shared.companion.source.traeCn": "Trae (CN)",
  "shared.companion.source.traeGlobal": "Trae (Global)",
  "shared.companion.source.qoderCn": "Qoder (CN)",
  "shared.companion.source.qoderGlobal": "Qoder (Global)",
  // This tool's own long-running operations (check-in etc., `agent_watch::SOURCE_THIS_TOOL`).
  "shared.companion.source.thisTool": "This tool",
  // Fallback when a session has no title (clients only send one on some events).
  "shared.companion.untitled": "Untitled session",
  "shared.companion.row.aria": "{source} · {state} · {hint}",
  "shared.companion.info.source": "Source",
  "shared.companion.info.time": "Time",
  "shared.companion.jump.ok": "Focused {source}",
  "shared.companion.jump.failed": "Couldn't focus the source: {message}",
  "shared.companion.empty.title": "No sessions",
  // ⚠️ Must match the ACTUAL data sources: this window shows account-switch progress plus
  //    sessions from clients you explicitly enabled monitoring for. It does NOT read a
  //    client's past tasks on its own (the previous wording promised exactly that).
  "shared.companion.empty.description":
    "Shows account switches in progress, plus tasks from clients once you turn on session monitoring in Settings → Clients.",
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
  // Demo account names mirror the prototype's `SCOPES["wb-cn"].accounts` 1:1.
  // ⚠️ These are usernames (data) — identical in both languages, never translated.
  "shared.demo.account.a": "jackey",
  "shared.demo.account.aRemark": "Primary · long-term",
  "shared.demo.account.b": "team-shared",
  "shared.demo.account.c": "backup-01",
  "shared.demo.account.d": "qa-bot",
  "shared.demo.account.e": "design-lab",
  "shared.demo.account.f": "ops-02",
  "shared.demo.account.g": "spare-global",
  "shared.demo.account.h": "demo",
  // ---- Credit package names ----
  "shared.demo.pack.fission": "CodeBuddy Personal CN Growth Pack",
  "shared.demo.pack.credits": "CodeBuddy Personal Credits Pack",
  "shared.demo.pack.trial": "CodeBuddy New User Pack",
  "shared.demo.pack.checkin": "CodeBuddy Check-in Bonus Credits",
  "shared.demo.pack.activity": "CodeBuddy Campaign Reward Credits",
  // ---- Auto travel ----
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
  // Trae demo accounts mirror the prototype's `SCOPES["trae-cn"].accounts` 1:1.
  // ⚠️ Usernames are data — identical in both languages.
  "shared.demo.trae.name.jackey": "jackey",
  "shared.demo.trae.remark.jackey": "TraeWork primary",
  "shared.demo.trae.name.codeLab": "code-lab",
  "shared.demo.trae.name.soloTest": "solo-test",
  "shared.demo.trae.name.idle": "idle-01",
  "shared.demo.trae.name.snap": "snap-acc",
  "shared.demo.trae.group.common": "Common",
  "shared.demo.trae.group.test": "Test",
  "shared.demo.trae.group.idle": "Idle",
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
  // Merged client (issue #20): the client is literally named `Trae`.
  "shared.demo.trae.program.trae": "Trae CN",
  "shared.demo.trae.program.traeGlobalLabel": "Trae",
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
  // ---- Qoder: demo accounts (read-only pair; names are data, same in both languages) ----
  "shared.demo.qoder.name.jackey": "jackey",
  "shared.demo.qoder.name.team": "team-qoder",
  "shared.demo.qoder.name.intl": "jackey-global",
  "shared.demo.qoder.name.trial": "trial-01",
  // ---- Qoder: gateway demo data (pool reasons / catalog source / log error / key names) ----
  "shared.demo.qoder.gateway.reasonSessionDead": "credentials expired (refresh token required)",
  "shared.demo.qoder.gateway.reasonCooling": "upstream temporarily unavailable (cooling down)",
  "shared.demo.qoder.gateway.catalogCached": "this probe timed out (8s) — showing the last successful result",
  "shared.demo.qoder.gateway.modelRateLimited": "model \"MiniMax-M3\" hit its rate limit, rotated to another model",
  "shared.demo.qoder.key.defaultCn": "Qoder default key",
  "shared.demo.qoder.key.cliCn": "Qoder CLI",
  "shared.demo.qoder.key.global": "Qoder global key",
  "shared.demo.qoder.remark.main": "Main · long-term",
  "shared.demo.qoder.remark.team": "Shared by team",
  "shared.demo.qoder.remark.intl": "Main account (global)",
  // Credential summary format matches core `CredentialShape::describe` verbatim (demo mirrors the real shape).
  "shared.demo.qoder.cred.device": "access token (device_access) · refresh token (device_refresh)",
  "shared.demo.qoder.cred.deviceOnly": "access token (device_access) · no refresh token",
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
  // Card "more" menu: the four single-item refreshes + the account action group.
  "shared.cardMenu.refreshGroup": "Refresh",
  "shared.cardMenu.refresh.credits": "Refresh credits",
  "shared.cardMenu.refresh.tokens": "Refresh today's tokens",
  "shared.cardMenu.refresh.checkin": "Refresh check-in status",
  "shared.cardMenu.refresh.credential": "Refresh credential status",
  "shared.cardMenu.accountGroup": "Account",
  "shared.cardMenu.refresh.creditsHint": "Query remaining credits and expiry from upstream (slowest, may be rate-limited)",
  "shared.cardMenu.refresh.tokensHint": "Re-read local client session logs + gateway logs (fast)",
  "shared.cardMenu.refresh.checkinHint": "Re-read today's check-in result for this account (local)",
  "shared.cardMenu.refresh.credentialHint": "Re-read the local credential validity (the only one that still works when expired)",
  "shared.cardMenu.export": "Export account",
  "shared.cardMenu.note": "Remark and login state live in the account details (click the card)",
  "shared.cardMenu.checkedToday": "Checked in today",
  "shared.cardMenu.checkedTodayHint": "Already checked in today",
  "shared.cardMenu.needLogin": "Sign in first",
  "shared.cardMenu.needLoginHint": "Credential expired — sign in before checking in",
  "shared.cardMenu.busy": "Refreshing…",
  "shared.cardMenu.done": "Updated",
  "shared.cardMenu.checkinDone": "Checked in today",
  "shared.cardMenu.checkinNeedsLogin": "Sign in first",
  "shared.refreshPart.credits": "Credits refreshed",
  "shared.refreshPart.tokens": "Today's tokens refreshed",
  "shared.refreshPart.checkin": "Check-in status refreshed",
  "shared.refreshPart.credential": "Credential status refreshed",
  "shared.refreshPart.fail": "Refresh failed",
  "shared.stat.credits": "Credits",
  "shared.stat.tokens": "Today's tokens",
  "shared.stat.checkin": "Check-in",
  "shared.stat.credential": "Credential",
  "shared.stat.checkedIn": "Checked in",
  "shared.stat.notCheckedIn": "Not checked in",
  // issue #47: the honest wording Trae / Qoder need for check-in and token tiles.
  // "Unreadable" / "not available" / "from local ledger" must stay three separate
  // states — collapsing them turns "don't know" into "no", and the user keeps
  // retrying a button that can never work.
  "shared.stat.checkinUnreadable": "Unreadable",
  "shared.stat.checkinUnavailable": "Not available here",
  "shared.stat.checkinFromLedger": "From local ledger (refresh to confirm upstream)",
  "shared.stat.tokensGatewayOnly": "Requests via this gateway only",
  "shared.stat.credentialOk": "Valid",
  "shared.stat.credentialExpired": "Expired",
  "shared.stat.credentialRelogin": "Sign in again",
  "shared.stat.credentialOkSub": "Usable",
  "shared.stat.credentialBadSub": "Sign in first",
  "shared.stat.noExpiring": "No expiring credits",
  "shared.stat.expiringSoon": "{n} credits expire within {d} days",
  "shared.stat.total": "Total {n}",
  "shared.stat.expiresAt": "Expires {when}",
  "shared.stat.noExpiry": "No expiry",
  "shared.stat.plan": "{plan} plan",
  "shared.drawer.close": "Close",
  "shared.drawer.loading": "Loading details…",
  "shared.drawer.tab.overview": "Overview",
  "shared.drawer.tab.credits": "Credits",
  "shared.drawer.tab.checkin": "Check-in",
  "shared.drawer.tab.credential": "Credential",
  "shared.drawer.tab.snapshot": "Snapshot",
  "shared.drawer.snapshot.title": "Login-state snapshot",
  "shared.drawer.snapshot.time": "Last modified",
  "shared.drawer.snapshot.size": "Size",
  "shared.drawer.snapshot.files": "{n} files",
  "shared.drawer.snapshot.empty": "No snapshot for this account yet",
  "shared.drawer.remark.title": "Remark",
  "shared.drawer.remark.placeholder": "e.g. DS4.1 quota · unlocks 10/03",
  "shared.drawer.remark.aria": "Remark for {name}",
  "shared.drawer.remark.editAria": "Edit remark for {name}",
  "shared.drawer.remark.add": "Add remark",
  "shared.drawer.group.title": "Group",
  "shared.drawer.group.picker.aria": "Group for {name}",
  "shared.drawer.group.picker.unassigned": "Ungrouped",
  "shared.drawer.group.picker.create": "New group",
  "shared.drawer.group.picker.newPlaceholder": "Group name",
  "shared.drawer.group.picker.confirm": "Create",
  "shared.drawer.group.picker.cancel": "Cancel",
  "shared.drawer.group.picker.exists": "This group already exists — it will be reused",
  "shared.drawer.group.placeholder": "Enter a custom group name (leave blank to ungroup)",
  "shared.drawer.group.aria": "Group for {name}",
  "shared.drawer.group.editAria": "Edit group for {name}",
  "shared.drawer.group.add": "Set group",
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
  "shared.drawer.switch.title": "Switch to this account",
  "shared.drawer.switch.wizard": "Switch via wizard (optionally migrate content)",
  "shared.drawer.switch.available": "Switchable",
  "shared.drawer.currentLogin": "Signed in",
  "shared.drawer.actions.refresh": "Refresh token",
  "shared.drawer.actions.export": "Export",
  "shared.drawer.actions.permission": "Switch permission diagnostics",
  "shared.drawer.actions.credentialNote": "Credentials are expired or need re-login; no check-in entry until that is resolved.",
  "shared.drawer.checkin.historyTitle": "Check-in history (last 7 days)",
  "shared.drawer.checkin.historyDate": "Date",
  "shared.drawer.checkin.historyResult": "Result",
  "shared.drawer.checkin.historyAmount": "Credits earned",
  "shared.drawer.checkin.result.success": "Success",
  "shared.drawer.checkin.result.already": "Already checked in",
  "shared.drawer.checkin.result.fail": "Failed",
  "shared.drawer.checkin.result.none": "Not checked in",
  "shared.drawer.checkin.historyEmpty": "No check-in records yet",
  "shared.drawer.empty.title": "No details",
  "shared.drawer.empty.description": "No details to show for this account yet; re-check and try again.",

  // ---- Minimal batch bar (components/account-batch-bar.tsx, shared) ----
  "shared.batch.selected": "{n} selected",
  "shared.batch.checkin": "Check in",
  "shared.batch.refresh": "Refresh",
  // Prototype `batch-export`: exports the **selected** accounts (all-accounts export lives in the toolbar).
  "shared.batch.export": "Export",
  "shared.batch.delete": "Delete",
  "shared.batch.clear": "Clear selection",
  "shared.batch.deleteTitle": "Delete accounts in bulk",
  "shared.batch.deleteDesc": "Delete the {n} selected accounts? You can restore them from the recycle bin.",
  "shared.batch.confirm": "Delete",
  "shared.batch.cancel": "Cancel",
  // ★ Bulk check-in result bar (prototype `checkinResultHtml`). Must report BOTH counts —
  // a bulk operation always partially fails, so reporting only successes would be a lie.
  "shared.checkinResult.summary": "{ok} ok · {fail} failed",
  "shared.checkinResult.failedNames": "Failed: {names}",
  "shared.checkinResult.allOk": "All accounts checked in",
  "shared.checkinResult.retryFailed": "Retry failed",
  "shared.checkinResult.summary4": "{ok} ok · {skip} unavailable · {fail} failed",
  "shared.checkinResult.skippedNames": "Check-in channel unavailable for: {names}",
  "shared.checkinResult.earned": "+{n} Credits this run",
  "shared.checkinResult.dismiss": "Got it",
  "shared.list.groupCount": "{n}",

  // ---- Account list toolbar (components/account-toolbar.tsx, shared) ----
  "shared.toolbar.searchPlaceholder": "Search name, email, remark… (press / to focus)",
  "shared.toolbar.searchAria": "Search accounts",
  "shared.toolbar.sort": "Sort",
  "shared.toolbar.sortAria": "Sort by",
  "shared.toolbar.group": "Group",
  "shared.toolbar.groupAria": "Group by",
  "shared.toolbar.viewAria": "Switch card / table view",
  "shared.toolbar.viewCard": "Card view",
  "shared.toolbar.viewTable": "Table view",
  "shared.toolbar.viewTableDisabled": "Window is too narrow for the table view; widen it and try again",
  "shared.toolbar.selectAll": "Select all",
  "shared.toolbar.deselectAll": "Deselect all",

  // =====================================================================
  // ---- "Add account" drawer (components/add-account-drawer.tsx, shared) ----
  // =====================================================================
  // Shared by WorkBuddy and Trae, and it now also carries what used to live in the toolbar's
  // "…" overflow menu — so the copy lives here instead of once per platform.
  "shared.addDrawer.title": "Add account",
  "shared.addDrawer.scopePrefix": "Current scope: ",
  "shared.addDrawer.scopeSuffix": ". Pick a way to add an account.",
  "shared.addDrawer.oauthTitle": "OAuth login",
  "shared.addDrawer.oauthDesc": "Open the official authorization page; the account is imported after you sign in",
  "shared.addDrawer.localTitle": "Import from this machine",
  "shared.addDrawer.localDesc": "Read this client's signed-in session and add the account in use",
  "shared.addDrawer.backupTitle": "Import a backup file",
  "shared.addDrawer.backupDesc": "Restore accounts in bulk from an exported JSON backup",
  "shared.addDrawer.notInstalled": "No client detected in this scope, so \"Import from this machine\" is unavailable. Use OAuth login instead.",
  "shared.addDrawer.moreTools": "More actions",

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

  // ---- Auth link panel (components/auth-link-panel.tsx, shared by all platforms) ----
  "shared.authPanel.scanHint": "Scan with your phone, or copy the link into any browser",
  "shared.authPanel.copy": "Copy link",
  "shared.authPanel.copied": "Copied",
  "shared.authPanel.authorized": "Authorized",
  "shared.authPanel.waiting": "Waiting for authorization…",
  "shared.authPanel.remaining": "{time} left",
  "shared.authPanel.expired": "Expired",
  "shared.authPanel.expiredDesc": "This link is no longer valid — generate a new one",
  "shared.authPanel.regenerate": "Regenerate",
  "shared.authPanel.step1": "Open the link above on your phone or any other computer.",
  "shared.authPanel.step2": "Sign in and approve — no client needs to be installed on this machine.",
  "shared.authPanel.step3": "This window receives the result and the account lands in the library.",
  "shared.authPanel.noCallback": "No local callback needed: the server polls the official API, so this works inside a container too.",
  "shared.authPanel.qrAlt": "Authorization QR code",

  // ---- Client-less form (server / container deployment, issue #62) ----
  "shared.srv.badge": "No client",
  "shared.srv.help": "What degrades",
  "shared.srv.strip.accounts": "No {scope} client found — accounts and gateway are unaffected; only program-slot switching, client usage scanning and session browsing are missing.",
  "shared.srv.strip.gateway": "No {scope} client found — the gateway works as usual: it only needs credentials from the pool.",
  "shared.srv.card.title": "No {platform} client on this machine — you can still sign in and run the gateway",
  "shared.srv.card.desc": "The library stores credentials themselves; the client is only a ready-made source of a login and a program switch. Sign in once with any path below and everything behaves like a machine that has the client.",
  "shared.srv.capOk": "Works without a client",
  "shared.srv.capOff": "Needs the client",
  "shared.srv.capOk.oauth": "Browser authorization (link / QR, any device)",
  "shared.srv.capOk.accounts": "Account library, token refresh, check-in and credits",
  "shared.srv.capOk.gateway": "API gateway, key management, request logs",
  "shared.srv.capOk.context": "Context tiers: built-in defaults (no upstream probing)",
  "shared.srv.capOff.local": "Import from this machine — reads the client's login state",
  "shared.srv.capOff.programs": "Program-slot switching — writes the client's own config",
  "shared.srv.capOff.usage": "Client usage scan — sourced from client session files",
  "shared.srv.capOff.sessions": "Session browsing and copy — same source",
  "shared.srv.traeNote": "Trae is the exception: both authorization and refresh need the client's device private key, so pure browser login cannot complete — open \"Browser authorization\" to see the two realistic paths.",
  "shared.srv.help.title": "No-client notes",
  "shared.srv.help.whyTitle": "Why \"no client\" is not a fault",
  "shared.srv.help.why1": "The library stores credentials (access / refresh token); gateway forwarding, check-in and credit stats only use them. Whether the client sits on the same machine does not matter for these three.",
  "shared.srv.help.why2": "The old UI rendered \"no client detected\" in red with only \"Re-detect / Import from this machine\", which told server users \"you can't use this\" — that treated a deployment form as an error.",
  "shared.srv.help.moveTitle": "Moving accounts from another machine",
  "shared.srv.help.moveDesc": "Export a backup (JSON) there, then use \"Import backup\" here. The backup carries credentials, so refresh and the gateway work immediately — no second sign-in.",
  "shared.srv.paste.title": "Paste credentials",
  "shared.srv.paste.desc": "Copy credentials from any machine that has the client, or paste them after a browser authorization. This machine needs no client.",
  "shared.srv.paste.whyTitle": "Why pasting works",
  "shared.srv.paste.whyDesc": "The library stores the credential itself; \"Import from this machine\" merely reads it from the client directory for you. Once stored, refresh, check-in and the gateway take the same path.",
  "shared.srv.paste.verify": "Verify and add",
  "shared.srv.paste.empty": "Nothing pasted",
  "shared.srv.paste.emptyDesc": "Paste the auth file contents or a refresh_token",
  "shared.srv.paste.failed": "Credential verification failed",
  "shared.srv.paste.ok": "Added {name}",
  "shared.srv.paste.okDesc": "Credential stored · refresh and gateway are live",
  "shared.srv.paste.hint.wb": "Paste the JSON from auth/workbuddy-desktop-ai.info (access_token / refresh_token)",
  "shared.srv.paste.hint.trae": "Paste a refresh_token, or the callback URL the authorize page redirected to",
  "shared.srv.paste.hint.qoder": "Paste a Qoder personal access token (PAT)",
  "shared.srv.localUnavailable": "No client on this machine to read",
  "shared.srv.localUnavailableDesc": "Use \"Browser authorization\" or \"Paste credentials\" instead; or export a backup on a machine that has the client and import it here",
  "shared.srv.oauthTitle": "Browser authorization",
  "shared.srv.oauthDesc": "Generates a link and QR code; authorize on any device, the server collects the result",
};
