# Agent Note: Recycle-bin deletion verifies the item is gone before reporting Ok

Status: implemented

## Problem

`recycle_path` recorded `CleanResult::Ok` as soon as `platform::move_to_trash` returned
`Ok`. Every path that removes files through the trash — the disk-lens and single-path
entrances, declutter, and both residual channels' file items — therefore treated the OS
call's success as the completion fact. That is the same trap the repo already closed for
uninstallers and native registrations ("成功判据是产物/登记没了，不是退出码 0"): an API
that reports success while the item stays on disk would be counted as freed, and the
item's row would be dropped from the UI list because the report said `Ok`.

## Decision

`move_to_trash` success is verified before it counts: after the call, the path is
re-checked with `symlink_metadata`; if it still exists the result is `Failed` with
`FailReason::Unverified` and the reason `trash-reported-success-but-remains` recorded, and
the file count is not incremented. A path that is genuinely gone records `Ok` with the
item count only — trashing still does not count as freed bytes.

The logic lives in `recycle_path_with(path, progress, move_to_trash)`, an injection seam:
production passes `platform::move_to_trash`, tests pass fakes. That is what makes the
"reported success but the file remains" branch reachable on any host instead of requiring
a misbehaving trash API.

## Alternatives considered

Trusting the API on the grounds that `NSFileManager`/`SHFileOperation` rarely lie —
rejected: the repo's completion principle exists precisely because these APIs do lie in
edge cases (Finder fallback, sandboxed helpers), and the check is one `symlink_metadata`.
Checking existence only in the UI follow-up — rejected: the report is the shared fact;
entrances that do not run the follow-up (declutter, disk lens) would still see `Ok`.
Adding a platform-specific guard per OS — rejected: the verification is platform-neutral
and the seam keeps it testable without a macOS host.

## Consequences

A trash operation that leaves the item behind is now reported as a failure and the item
stays in the list; nothing silently disappears from the UI on a false success. Cost is
one extra `symlink_metadata` per trashed item. Both residual channels and all three
user-picked-path entrances get the verification through the shared `dispose` path.

## Verification

- `src/core/cleaner.rs::recycle_verification_rejects_reported_success_that_left_the_file`
- `src/core/cleaner.rs::recycle_verification_accepts_a_real_removal`
- `src/core/cleaner.rs::recycle_never_silently_destroys` (existing invariant, unchanged)

Proved: with the post-call existence check removed, the injected "reported Ok, file
still present" fake is recorded as `Ok` and the test fails
(`assert_eq!(result, CleanResult::Failed)` → left `Ok`); evidence
`docs/agent-notes-evidence/2026-10-06-recycle-verify-red.log`. Restoring the check makes
the same focused run green (`...-green.log`). Windows-only execution; macOS native
acceptance remains an external condition. Actual unified gate results are recorded in
RULES_REFACTOR_STATUS.
