# Agent Note: Cache-owner exit consent is narrower than arbitrary process termination

Status: implemented

## Problem

A known cache can remain busy indefinitely, but terminating processes by an inferred
application label is unsafe. Existing Busy only retained a label/booleans, and the
status page's Windows terminate_process is a forced termination, not normal Quit.

## Decision

Only originally rule-recommended application/browser cache or log contents with
frozen plans, stable root identity and permanent contents disposal offer this action.
Busy preserves the original recommendation when occupancy downgrades it. Unknown
occupancy, manual-only rows, assets, worktrees, arbitrary paths and whole-tree deletion
are excluded. Preparation queries real owners on demand and presents names, PIDs,
executables and affected paths before consent. Discovery remains read-only.

macOS uses bounded recursive lsof evidence and Darwin uniqueid. Windows uses bounded,
no-follow file registration with Restart Manager and exact creation FILETIME. Both
reject other users, the cleaner's process tree and protected system executables;
Windows additionally rejects service/critical/Explorer owners. Unknown evidence
prevents both termination and cleanup. Unidentified background services are manual.

Normal exit uses NSRunningApplication terminate on macOS and WM_CLOSE on Windows.
It never delegates to SIGTERM or TerminateProcess. CLI/no-window processes may not
respond. Continued occupancy offers a separate, explicitly worded force confirmation;
force uses stable-identity termination, and is never an automatic timeout fallback.
The approved owner set can shrink but cannot gain new processes or reused PIDs.

Before any exit and again before cleanup, the frozen rule plans and root identity
are revalidated. Vacancy is independently probed; sending an exit request is not
success. Final deletion still uses cleaner::clean_targets and its occupancy, live
SQLite, whitelist, identity and link protections. Cancellation stops subsequent work.
No application is automatically reopened after cleanup.

## Alternatives considered

Automatically killing every owner, killing by application names, reusing the status
page's terminate_process as normal Quit, interpreting probe failure as vacancy and
rebuilding deletion authorization after exit are rejected. Global installation
misclassification is addressed separately by 2026-10-20-build-installation-boundary.md.

## Consequences

Preparation and waiting run in background tasks; the UI is cancellable. A rejected,
partially failed or unknown operation retains the item. Frozen process identities
still have the usual macOS syscall race limit; Windows force verifies creation time
on the same handle used for termination. Windows traversal caps at 20,000 entries
and 30 seconds; incomplete or changing Restart Manager results fail closed. This is
an on-demand feature, not a claim of complete Windows scan-time handle detection.

## Verification

- `src/core/cache_release.rs::consent_never_covers_reused_pids_or_new_owners`
- `src/core/cache_release.rs::cache_release_waits_for_vacancy_and_preserves_the_scanned_plan`
- `src/core/cache_release.rs::timeout_unknown_new_owners_and_cancel_never_delete_or_auto_force`
- `src/core/cache_release.rs::only_rule_recommended_cache_contents_can_close_owners`
- `src/core/cache_release.rs::changed_cache_root_is_refused_before_any_exit`
- `src/platform/cache_release/macos.rs::lsof_evidence_retains_every_owner_and_respects_path_boundaries`
- `src/core/inuse.rs::apply_busy_downgrades_recommended`

Feature tests use injected exit/probe functions and temporary caches; no running user
application was terminated. Windows native compilation/runtime and manual Quit/save
prompt interaction remain unverified on this macOS host.
