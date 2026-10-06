# Agent Note: Disk-lens and path deletions freeze their typed operation at confirmation

Status: implemented

## Problem

The disk-lens and single-path entrances (`clean_arbitrary_items`) were the last cleanup
paths whose operation was re-derived at delete time from the filesystem type: the
confirmation captured only a path and an optional identity, and `clean_path` decided
delete-as-directory vs delete-as-file when it ran. GOAL requires every entrance to carry
a typed operation rather than guessing it from the object at hand.

The identity gate covers same-path replacement when a snapshot exists, so the practical
hole was the no-snapshot case (network volumes, metadata unreadable at confirmation):
a path confirmed as a directory and replaced by a same-named file would be deleted as a
plain file — an operation the user never approved.

## Decision

`ArbitraryTarget` gains a frozen `operation` captured in `capture()` (directory → `Tree`,
otherwise `File`) alongside the identity. `clean_arbitrary_items` re-checks the current
type against the frozen operation immediately before disposal and records
`FailReason::Changed` on a mismatch — it never switches to a different operation. A path
that disappeared between confirmation and cleanup stays a `Skipped` record (handled by
`dispose`), not a refusal.

## Alternatives considered

Constructing a full `CleanupPlan` for user-picked paths — rejected: no rule governs a
hand-picked path, and inventing a rule reference would make the display source dishonest
(the residual channel binds a real rule, but arbitrary picks have none). Relying on the
identity gate alone — rejected: it is skipped when no snapshot exists, which is where the
wrong-operation delete could happen. Re-deriving the type but only for logging — rejected,
logging is not a guard.

## Consequences

A type swap after confirmation is refused with `Changed` and the replacement stays on
disk, even when no identity snapshot exists. Deletes whose type did not change behave
exactly as before. The disk-lens confirmation now states the disposal method and the
operation granularity, and the executor acts on the same frozen operation.

## Verification

- `src/core/cleaner.rs::arbitrary_target_freezes_the_operation_at_capture`
- `src/core/cleaner.rs::arbitrary_clean_refuses_a_type_swap_after_capture`

Proved: with the frozen-operation recheck removed, the type-swap test fails — the
same-named replacement file is deleted (assertion `dir.is_file()` fails); evidence
`docs/agent-notes-evidence/2026-10-06-arbitrary-operation-red.log`. Restoring the guard
makes the same focused run green
(`docs/agent-notes-evidence/2026-10-06-arbitrary-operation-green.log`). An earlier draft
of the test passed even without the guard because the identity gate happened to cover the
snapshot-present case; the shipped test intentionally removes the identity to pin the
guard's unique contribution, and the red run is against that hardened test.

Windows-only evidence; macOS native acceptance remains an external condition. Actual
unified gate results are recorded in RULES_REFACTOR_STATUS.
