# Agent Note: Batch index mutations retain directory lookup caches

Status: implemented

## Problem

The macOS 0.0.13 session on 2026-10-15 applied 11,251 file updates and 9,612
removals between 08:44:28.658 and 08:46:25.879 (117 seconds). The following
1,057 directory rescans took only 1.187 seconds. The 0.0.12 session also stalled
in this phase; this is not evidence of a new 0.0.13-only regression. Sampling
after the operation completed showed a responsive main event loop, not a
captured deadlock. Consumers share the index build lock and wait for this work. The selected live
log lines are preserved in
`docs/agent-notes-evidence/2026-10-15-index-live-stall.log`.

In `src/platform/macos/disk_tree.rs`, every file upsert, tombstone and overlay
insertion cleared the entire child-name lookup cache. Looking up the next file
rebuilt wide ancestor directories, and existing-file updates unnecessarily
appended a replacement node. Small semantic fixtures did not detect this
multiplication of work.

## Decision

Metadata-only updates retain the existing file slot and update ancestor sizes
through the existing copy-on-write override mechanism. Name changes (including
case-only renames) still replace the node: mapped base names come from an
immutable base name pool, so a new name offset cannot be placed in a base
override. Tombstones retain cached
name mappings; lookups validate the cached node and repair only a stale name,
preserving first-live ASCII case-insensitive alias ordering. Removed directories
discard their own caches. mmap overlay insertion maintains only the parent's
cached name entry. Heap additions retain their existing CSR-rebuild visibility.
Whole-tree CSR/delta reconstruction can still invalidate the entire cache.

A deterministic regression checks cache retention throughout updates, removals
and additions, stable slot count for metadata changes, size/count/mtime results,
and deletion/recreation correctness. It also verifies case-only renames, immutable
shared base state and persistence round trips. A separate correctness test covers aliases,
recursive removal, subtree replacement and directory-to-file replacement on both
heap and mmap trees. The optional timing probe measures the same batch before and
after the change; elapsed time is evidence, not a flaky CI assertion.

## Alternatives considered

Clearing only the parent directory on every deletion still rebuilds a wide
same-parent directory once per event. Removing invalidation without checking
cached tombstones returns ghost nodes. Always choosing the newly appended node
breaks first-live case-insensitive alias ordering. Raising scan timeouts or
skipping FSEvents paths conceals the work or loses scan correctness.

## Consequences

Batch updates reuse lookup work and avoid unnecessary delta growth. A lookup of
a deleted cached name may scan that parent's children once to preserve alias
semantics; this is deferred until that exact name is queried. This fix does not
claim to eliminate every filesystem or UI stall. The synthetic batch is not a
replay of the entire original live event stream.

Pitfalls reviewed: P1-P9/P14-P25 and P28/P31-P41/P43-P61 retain command,
uninstall, safety, identity, rule and UI contracts. P26/P27 retain dataless and
skipped-index semantics; cache absence is never filesystem absence. P29/P30/P34
retain occupancy timeouts, vendored GPUI and first-frame guards. P42's parallel
measurement remains unchanged. P55 local toolchain validation is reported with
its actual version. P62 points to this decision and its regression tests.

## Verification

- `src/platform/macos/disk_tree.rs::batch_file_changes_reuse_directory_lookups`
- `src/platform/macos/disk_tree.rs::cached_tombstones_keep_alias_order_and_subtree_replacements_correct`
- Optional timing: `src/platform/macos/disk_tree.rs::batch_file_changes_timing`

Proved: the pre-fix focused run failed at "single-file removals must retain warmed
directory lookups"; output is saved in
`docs/agent-notes-evidence/2026-10-15-index-batch-red.log`. The unchanged cache
retention assertion guards the original global invalidation. Pre-fix timing of
20,000 sibling files, 4,000 updates and 4,000 removals was 15.977594542 seconds in
`docs/agent-notes-evidence/2026-10-15-index-batch-before.log`.

The same timing probe after the final fix took 21.575916 milliseconds in
`docs/agent-notes-evidence/2026-10-15-index-batch-after.log` (the first fixed run
was about 11 milliseconds; the recorded final run overlapped the native suite).
The focused tree suite passed 8 tests with 1 optional timing probe ignored in
`docs/agent-notes-evidence/2026-10-15-index-batch-green.log`.

Native macOS validation with rustc 1.95.0: the full library suite passed 819 tests
with 12 ignored; build and clippy with warnings denied passed. Summaries are saved
in `docs/agent-notes-evidence/2026-10-15-index-batch-gates.log`. The initial sandbox
suite could not access Spotlight/FSEvents/processes/user logs, so the full suite
was rerun outside the sandbox after correcting the new case-only rename check.
Formatting, strict note anchors and pitfall pointers also passed (the pointer
checker retains 8 pre-existing warnings for older entries without notes).
The installed app has not been replaced and the original live event stream has
not been replayed. No Windows runtime result is claimed for this macOS change.
