/**
 * Locale domain: **session workbench** (PLAN §3.15 / P8).
 *
 * See the composition entry `src/locales/en.ts`. Keep keys in sync with `sessions.zh.ts`.
 */
export const en = {
  "nav.sessions": "Sessions",

  "sessions.title": "Linked sessions",

  /* ★ Sessions toolbar (prototype `viewSessions`, :3183): title "{client} sessions" + count
     badge. Client tabs are NOT rendered (session copy is WorkBuddy-only, so a single-option
     switcher would be a fake control, §3.15.1): the title degrades to static text. */
  "sessions.toolbar.title": "{client} sessions",
  "sessions.toolbar.client": "WorkBuddy desktop",
  /* Card head (prototype `.sg-head`): status badge + "{count} replicas"; plus the a11y label. */
  "sessions.card.replicaCount": "{count} replicas",
  "sessions.card.openAria": "Open session group {title}",

  "sessions.empty.title": "No sessions copied with this app yet",
  "sessions.empty.description":
    "Session copies come from this app's copy records. Copy a session from the accounts page first, and its replicas will show up here.",
  "sessions.empty.action": "Copy on the accounts page",

  "sessions.error.retry": "Retry",

  /* ★ Read-only replica view (P8 T02). `partialNote` is a fixed string (hard constraint ③). */
  "sessions.replica.groupTitle": "{count} replicas in this group",
  "sessions.replica.partialNote": "Only sessions copied with this app are shown",
  // ★ P8: **content-level** status (derived from session jsonl fingerprints, **not** updatedAt).
  "sessions.replica.status.latest": "In sync",
  "sessions.replica.status.behind": "Behind",
  "sessions.replica.status.diverge": "Diverged",
  "sessions.replica.status.unknown": "Status unknown",
  "sessions.replica.commonPrefix": "{count} messages in common",
  "sessions.replica.delta": "+{count}",
  // Status filter (**on demand**: only rendered when >= 2 statuses are selectable).
  "sessions.filter.all": "All ({count})",
  "sessions.replica.hasHistory": "Has history",
  "sessions.replica.titleFallback": "Title unavailable",
  "sessions.replica.updatedUnknown": "—",
  "sessions.replica.unknownAccount": "{uid} (unknown account)",

  "sessions.unsupported.title": "{product} doesn't support session management yet",
  "sessions.unsupported.description":
    "Session copy relies on the platform's own session storage and cloud ownership; currently only WorkBuddy has this capability.",
  "sessions.unsupported.action": "Go to accounts",

  /* ★ P8 "safe sync" (incremental append, §3.15.4) — **additive** by design.
     The copy must state that nothing in the target is deleted or overwritten: being
     overwritten is the user's biggest fear about "sync" (PLAN.md §3.15.4). */
  "sessions.sync.action": "Sync safe items ({count})",
  "sessions.sync.all": "Sync all behind ({count})",
  "sessions.sync.hint": "Appends only what the source added; never deletes existing content",
  // ★ D2: persistent note (not hover-only). Must keep the word "additive" — users'
  // biggest fear about "sync" is losing/overwriting their own content.
  "sessions.sync.additiveNote":
    "Sync is additive: it only appends what the source added to the target; nothing already in the target is deleted or overwritten.",
  "sessions.sync.toast.done": "Sync finished",
  "sessions.sync.toast.failAll": "Sync failed",
  "sessions.sync.toast.ok": "Synced {n} item(s), appended {lines} line(s)",
  "sessions.sync.toast.failed": "{n} failed",
  "sessions.sync.toast.retryFailed": "Retry failed only",
  "sessions.sync.toast.error": "Sync request failed",

  /* ★ P8 "resolve divergence" (overwrite-based unify, §3.15.2) — the **opposite** of the
     "safe sync" block above: sync is additive, this **overwrites** the other replicas and
     discards their divergent lines. Copy must never read like a silent overwrite: with a
     divergence the user must first pick which copy to keep. */
  "sessions.unify.action": "Resolve divergence ({count})",
  "sessions.unify.hint": "Pick which copy to keep; the others are overwritten (auto-backed up first)",
  "sessions.unify.title": "Choose the copy to keep",
  "sessions.unify.desc":
    "These replicas each changed after their common version, so they can't be merged. Pick one to keep; the rest will be overwritten by it.",
  "sessions.unify.mostComplete": "Most complete",
  "sessions.unify.overwrite":
    "This overwrites {n} other replica(s), discarding {lost} line(s) unique to them (auto-backed up first).",
  "sessions.unify.overwriteUnknown":
    "This overwrites {n} other replica(s); the number of discarded lines is unknown (auto-backed up first).",
  "sessions.unify.cancel": "Cancel",
  "sessions.unify.confirm": "Unify into other replicas",
  "sessions.unify.toast.done": "Unified",
  "sessions.unify.toast.failAll": "Unify failed",
  "sessions.unify.toast.ok": "Unified {n} item(s), discarded {lost} line(s)",
  "sessions.unify.toast.failed": "{n} failed",
  "sessions.unify.toast.retryFailed": "Retry failed only",
  "sessions.unify.toast.error": "Unify request failed",

  /* ★ Unified restart confirmation (P8 T03 / §3.15.3). The non-CLI copy defaults to
     "reopen automatically" plus an explicit unsaved-files warning. The CLI copy is the
     exception: it must NOT promise an auto-reopen — it says the session will be interrupted. */
  "sessions.restart.title": "Restart {client}?",
  "sessions.restart.description": "Switching will close and reopen {client}.",
  "sessions.restart.autoReopen": "Reopen {client} automatically after switching",
  "sessions.restart.unsavedWarning": "Unsaved files may be lost; save them first.",
  "sessions.restart.confirm": "Continue and restart",
  "sessions.restart.cancel": "Cancel",
  "sessions.restart.cliTitle": "Before switching {client}",
  "sessions.restart.cliDescription":
    "Switching {client} only updates the auth config and does not reopen the client.",
  "sessions.restart.cliWarning": "The current CLI session will be interrupted; finish any running task first.",
  "sessions.restart.cliConfirm": "Switch anyway",
  "sessions.restart.resultNotRestarted": "Not restarted; start {client} manually",
  "sessions.restart.switchingNoRestart": "Switching {client} (no restart)",
} as const;
