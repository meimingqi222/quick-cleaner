# Agent Note: Managed worktrees require owner reference retirement

Status: implemented

## Problem

Deleting clean Maka child checkouts and their exact Git registrations left live child Session workspace bindings behind. Host startup treats a missing live binding as fatal (`Live subagent worktree is unavailable`), so the application could no longer connect. Merged branches and clean status prove neither reference retirement nor safe Session resumption.

## Decision

Bare filesystem cleanup refuses workspace-owned `subagent-worktrees` identified by adjacent Maka storage/composition markers or `runtime.sqlite`. The shared safety policy also protects descendants, including individual source files and checkouts whose `.git` marker disappears. Worktree preflight and top-level cleaner report `WorktreeManaged` before deletion. Detection depends on workspace evidence, not a fixed project directory, user name, or install path.

The currently inspected Maka version makes `subagentWorkspace` immutable and prohibits relocation of managed child Sessions. Its supported resource release is `session.remove`: commit Session tombstones first, then retire the worktree, lease branch, and Git registration. Archive alone does not release the workspace. Removing child conversations requires explicit user authorization; QuickCleaner's ordinary directory confirmation does not grant that authorization. Automatic native Session deletion is not yet integrated into QuickCleaner. The managed refusal is the guard for that unsupported route, not evidence that merged worktrees can never be retired.

## Alternatives considered

- Restore checkouts: useful only to get the failed Host running, not the requested final cleanup.
- Remove only Git registrations: leaves live application references and reproduces startup failure.
- Clear JSON fields directly in the running database: bypasses immutable bindings, concurrency control and Session retirement side effects.
- Treat archive or terminal execution as ownership release: Maka deliberately retains workspaces for resume and follow-up.

## Consequences

Until the owner route has its own explicit conversation-deletion confirmation and completion checks, managed worktrees show a reason instead of being bare-deleted. Ordinary linked-worktree cleanup remains available, including exact registration removal and dirty/locked protections. Scan still reads bounded metadata and does not launch Git or inspect transcript rows.

The incident cleanup used the installed same-version native client after the user explicitly authorized removing the 14 clean child conversations. The 5 dirty children and their parent remain outside the selection. Filesystem and persistent references must be checked after native asynchronous cleanup; a command receipt alone is insufficient. Two retired checkouts remained until their own Git fsmonitor daemons were stopped and native `session.remove` was retried; no directory-delete fallback was used.

## Verification

- `src/core/worktrees.rs::managed_clean_checkout_cannot_be_deleted_without_reference_retirement`
- `src/core/worktrees.rs::cleanup_removes_exact_registration_and_preserves_other_stale_entries`
- `src/core/worktrees.rs::dirty_locked_and_changed_registration_never_fall_back_to_deletion`

Proved: temporarily made the managed-owner policy return false for nonempty paths; the regression failed with `Ok(())` instead of `Err(WorktreeManaged)`. Restored the policy, then the complete library suite passed (481 passed, 9 ignored).

Live validation: all 14 selected paths, branches, registrations and live Session metadata disappeared; all 14 Session tombstones existed. The parent and 5 dirty children remained. The installed Maka executor's `recover` passed with the 5 remaining live bindings, and the connected Host reported `ready`. Format, clippy, build and strict notes verification passed.
