# Agent Note: Windows stable object identity extended to ordinary targets

Status: implemented

## Problem

`TargetIdentity` is the TOCTOU gate every filesystem target is re-checked against
before deletion. On Unix it stores `dev + ino` and ignores `mtime`/`len`; on
Windows it only stored `mtime + len` — a weak check. The doc admitted the gap:
`mtime + len` "挡不住精确保持这两项的定向攻击" — delete a target, recreate it
within the same second with the same length, and the weak check passes.

The strong Windows identity already existed (`platform::windows::identity::object_id`,
volume serial + file index via `GetFileInformationByHandle`) but was wired **only**
into installation artifacts (`InstallationArtifact.object_id` / `OfficialOperation`).
Everything else — smart-cleanup targets, discovered build dirs, residual items —
stayed on the weak check. GOAL asks to promote the Windows stable identity beyond
install artifacts.

## Decision

`TargetIdentity` gains a Windows-only `stable: Option<(u64, u64)>` (volume serial,
file index). Two capture paths, deliberately different:

- `from_metadata` stays weak (`stable: None`): it is the hot path that reuses an
  already-taken `Metadata` (scan, `read_dir`, the cleaner's per-child identities)
  and must not open a handle per file.
- `capture_identity` (one syscall already, per target) additionally fills `stable`
  from `object_id`. Everywhere a target identity is frozen — scanner targets,
  plan targets, residual items, worktrees, discovered dirs — goes through it.

`recheck` on Windows now requires **weak AND strong**: `mtime + len` must match
(detects a changed length or modification time) *and*, when `stable` is present,
the freshly read `object_id` must match. This keeps the documented weak-check
behavior (an in-place rewrite that changes `mtime`/`len` is still rejected) while
closing the same-second / same-length swap the weak check missed. When `stable`
is `None` (metadata-only capture) it falls back to the weak check alone.

`object_id` itself is unchanged and remains the install-artifact identity too.

## Alternatives considered

Switching Windows `recheck` to `object_id` alone (mirroring Unix `dev+ino`) would
make an in-place rewrite pass — contradicting the deliberate
`identity_recheck_inplace_write_verdict_is_platform_split` behavior and its
documented rationale. Filling `stable` inside `from_metadata` is impossible
(`Metadata` carries no file index on stable Rust) and would add a handle open to
the per-file hot path. Making `capture_identity` skip the handle open when an
ancestor is a reparse point is already handled inside `object_id` (returns `None`).
All rejected or already covered.

## Consequences

Every target captured via `capture_identity` now carries the strong Windows
identity, so a same-second/same-length delete-and-recreate is caught at the
identity gate instead of only by the frozen installation instance. Cost: one
`GetFileInformationByHandle` (plus the ancestor reparse scan) per target capture —
bounded by the target count, not by files walked. The existing identity tests
(unchanged file passes, delete+recreate fails, path vanishes fails, in-place write
still rejected on Windows) all keep passing.

`src/platform/windows/source_install.rs::supplement_retains_same_content_startup_replacement`
had a fixture assertion `identity.recheck(&startup)` that relied on the weak check
passing after a rename-and-rewrite; the rewritten file now gets a different file
index (the original still exists as `saved-startup.cmd`), so the assertion was
updated to `!identity.recheck(&startup)`. The frozen-instance detection the test
actually exercises is unchanged — the startup loop calls `scanned_identity` first,
which still returns the "Scanned installation artifact changed" error.

## Verification

- `src/core/model.rs::identity_recheck_rejects_a_different_stable_object`
- `src/core/model.rs::identity_recheck_passes_for_unchanged_file`
- `src/core/model.rs::identity_recheck_fails_after_delete_and_recreate`
- `src/core/model.rs::identity_recheck_fails_when_path_vanishes`
- `src/core/model.rs::identity_recheck_inplace_write_verdict_is_platform_split`
- `src/platform/windows/source_install.rs::supplement_retains_same_content_startup_replacement`

Model migration, not an organic bug fix; no red-run proof claimed. Actual unified
gate results and limitations are recorded in RULES_REFACTOR_STATUS.
