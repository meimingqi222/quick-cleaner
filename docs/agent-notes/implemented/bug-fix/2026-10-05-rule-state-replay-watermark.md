# Agent Note: Rule cache corruption cannot reset replay protection

Status: implemented

## Problem

Malformed or unreadable state files were treated as a fresh installation, resetting the accepted sequence to zero and allowing an older signed package to be accepted. An interrupted package publication could also block retry or erase the distinction between accepted state and retention failure.

## Decision

Empty state is legitimate only when no state files exist. Corrupt-only or unreadable state fails closed. Recover the latest valid pointer while preserving the highest valid watermark. Write synced immutable packages and atomic state generations; allow authenticated retry after package publication but before pointer publication. Retain current, previous and built-in rules, and reclaim only exact authenticated cache files. Retention errors after a committed pointer do not undo acceptance.

## Alternatives considered

Defaulting every read error to an empty state loses replay history. Replacing an immutable package permits sequence equivocation. Recursive deletion of stale cache directories can destroy unknown files or follow redirected paths.

## Consequences

Offline clients continue with a valid cached or built-in snapshot. Updates stay blocked when replay history cannot be confirmed. Unknown cache entries survive cleanup. Rollback retains the accepted watermark and does not change an already pinned scan.

## Verification

- `src/core/rules/update.rs::corrupt_state_never_resets_replay_protection`
- `src/core/rules/update.rs::interrupted_publication_retries_and_corrupt_updates_leave_active_state`
- `src/core/rules/update.rs::cache_retains_current_previous_and_embedded_without_deleting_unknown_files`

Proved: temporarily restored unwrap_or_default for corrupt-only state and ran the first test. It failed at state(root).is_err() with cargo exit 101. Restoring the fail-closed read passes. Signed fixture tests also cover replay after rollback, interrupted publication retry, corrupted active-package fallback, retention and pinned snapshots. Production keys were not provisioned.
