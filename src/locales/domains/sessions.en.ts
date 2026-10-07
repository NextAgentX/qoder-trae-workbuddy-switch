/**
 * Locale domain: **session workbench** (PLAN §3.15 / P8).
 *
 * See the composition entry `src/locales/en.ts`. Keep keys in sync with `sessions.zh.ts`.
 */
export const en = {
  "nav.sessions": "Sessions",

  "sessions.title": "Linked sessions",
  "sessions.subtitle": "Link and sync session copies across accounts",

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
