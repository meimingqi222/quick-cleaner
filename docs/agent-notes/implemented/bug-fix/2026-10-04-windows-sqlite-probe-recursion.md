# Agent Note: Keep Windows raw occupancy evidence outside database safety policy

Status: implemented

## Problem

Running the Windows library tests exposed a recursive stale SQLite check: live-database policy called is_open, which called the platform spot-check fallback, which called live-database policy again. The stale nested SQLite deletion test overflowed the stack, even with a 16 MiB Rust test stack.

## Decision

The Windows raw is_open probe uses a bounded, no-follow file traversal and exclusive-sharing CreateFileW probe. Sharing/lock violations mean occupied; a successful open means vacant at that instant; other failures and symlinks mean unknown. It never calls safety predicates. Existing deletion and live SQLite gates remain authoritative.

## Alternatives considered

Increasing the thread stack does not terminate recursion. Returning vacant for unsupported or inaccessible files would permit unsafe deletion. Removing the crash-leftover exception would permanently block stale SQLite remnants.

## Consequences

Stale unlocked databases can be cleaned without recursion, while a genuinely open shared handle still blocks cleaning. The probe is momentary evidence, not a lock held through deletion. General Windows batch handle enumeration remains outside this change.

## Verification

- `src/core/cleaner.rs::delete_tree_cleans_stale_nested_sqlite_family`
- `src/platform/windows/inuse.rs::stale_sqlite_raw_probe_is_nonrecursive_and_detects_shared_handles`

Proved: before the raw-probe fix, the existing stale nested SQLite deletion test terminated with stack overflow, including after setting RUST_MIN_STACK=16777216. After the fix the full library suite passed. The new regression holds an actual Windows shared read handle, checks occupied while held, checks vacant after close, and checks that missing paths remain unknown.
