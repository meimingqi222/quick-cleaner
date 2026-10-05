# Agent Note: Discover linked agent worktrees and remove only their Git registration

Status: implemented

## Problem

The fixed agent worktrees directories were deleted as a whole, without cleaning Git registrations. Maka stores linked checkouts under the roaming workspace directory, which was not scanned. The real copilot-api repository has 19 such checkouts.

## Decision

Enumerate known agent containers to a maximum of three directory levels with a finite entry budget. Read bounded metadata and verify both links to an immediate child of the common Git directory's worktrees directory. Stop at repository roots and never start Git during discovery. Preserve the registration path as a typed operation in the scan and selection.

At cleanup time reject unknown, locked, dirty, untracked, ignored, submodule, changed, or protected registrations. Git status uses no optional locks and disables fsmonitor. The existing core cleaner performs permanent file deletion with its safety, live-database, identity and occupancy guards. After verified checkout removal, invoke Git worktree remove for that exact missing checkout without force and verify the admin entry is gone. Recycle-bin disposal is rejected because deleting the registration would break later restoration. Never run repository-wide prune or fall back to raw deletion after a failed check. Normalize Windows canonical paths to ordinary drive paths before safety matching and Git arguments.

## Alternatives considered

Searching every repository and spawning Git during scanning would add process and disk costs proportional to project count. Deleting the whole agent container hides independent checkouts and risks uncommitted data. Global prune would also remove registrations not selected by the user. Letting Git delete all checkout files would bypass the existing per-file safety and live-database guards.

## Consequences

Maka and the existing home-directory agent layouts show independent opt-in worktrees. Git runs only for selected cleanup targets. Dirty worktrees, including ignored build outputs, require the user to resolve their contents first. Partial file deletion retains registration and is reported as a failure; unknown metadata is not advertised as disposable. Already-missing checkouts and arbitrary project-local worktrees outside the known containers are not new scan targets. Windows validation does not substitute for macOS native CI.

## Verification

- `src/core/worktrees.rs::cleanup_removes_exact_registration_and_preserves_other_stale_entries`
- `src/core/worktrees.rs::dirty_locked_and_changed_registration_never_fall_back_to_deletion`
- `src/core/worktrees.rs::discovery_only_lists_linked_checkouts_and_stops_at_repository_roots`
- `src/core/worktrees.rs::unregister_requires_missing_checkout_and_unchanged_backlink`
- `src/core/categories/dev.rs::moved_catalogs_keep_the_previous_names`

Proved: temporarily returning success without deleting the registration made the exact-registration test fail with cargo exit 101 because the selected admin directory remained. Restoring the implementation passed the full library suite: 467 passed, 9 ignored. The isolated tests use actual Git repositories and compare the post-cleanup native worktree list, preserving unrelated live and stale entries. Read-only discovery on the real Maka container found all 19 copilot-api checkouts; no real project worktrees were deleted.

Timing: the ten warm discovery runs averaged 67.4 ms. The existing scanner's parallel size measurement took 1.116 seconds for 605.9 MB, using filesystem traversal without an MFT size tree. These are local measurements of the added worktree targets, not a before/after timing of every category. Reproduce with `cargo run --example worktrees -- --measure <agent-container>`.

Pitfalls checked: P1-P4, P8-P9 and P14-P18/P21 retain their existing command, UI, installation and rule contracts. P5-P7 retain the core readonly-directory, ACL and reboot-delete behavior. P19 preserves duplicate-plan constraints and P20 retains fail-closed owner results. The user-provided macOS dataless, skipped-index, dev/ino, lsof, vendored GPUI, cache-signature and first-frame constraints remain unchanged. P22 records the new worktree invariant.
