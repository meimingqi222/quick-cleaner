# Agent Note: Native residual deletions verify completion instead of trusting API success

Status: implemented

## Problem

Windows native residual deletion had three gaps in the same code path
(`platform/windows/residuals.rs::clean_residuals`):

1. **Registry key/value deletion had no completion verification.** `RegDeleteTreeW` /
   `RegDeleteValueW` returning success was recorded as `ok` without re-checking that the
   registration really disappeared — the frozen principle is that the criterion is the
   entry being gone, not the API call reporting success.
2. **`delete_scheduled_task` accepted `schtasks` exit code 0 as success** (`status.success()
   || !exists`). A successful exit code with the task file still on disk was counted as
   cleaned — the exact "exit code 0 is not a success criterion" trap the repo already
   documented for uninstallers.
3. **`delete_reg_tree` ignored WOW64 view flags.** 32-bit installs are scanned through the
   32-bit view (`KEY_WOW64_32KEY`), but the deletion opened the predefined root without any
   view flag, so a 64-bit process deleted the 64-bit view's same-named path: it either
   permanently reported "delete failed" for every 32-bit registration or hit the wrong view.

## Decision

`delete_registry_target` makes verified absence the single completion criterion for
registration keys, registry values and scheduled tasks:

- tri-state pre-check (verified absent / still present / Unknown);
- verified absent → idempotent completion (repeat cleans and races are not fabricated
  failures);
- still present → delete, then verify absence again; only verified absence counts as `ok`;
- Unknown (access denied and other unexplained failures) → refuse to delete and record the
  failure — an unreadable state is not deletion authorization.

`delete_reg_tree` gains a `sam` parameter and deletes through the parent key opened with
the declared view (`delete_sam_of` mirrors `sam_of` minus read bits), so deletions land in
the same WOW64 view the scan used. `reg_key_absent` / `reg_value_absent` only accept
`ERROR_FILE_NOT_FOUND` / `ERROR_PATH_NOT_FOUND` as "verified absent"; everything else is
Unknown. The `CleanFailure::Id` strings are unchanged because
`residual_clean_follow_up` matches unresolved items against `display_label`.
`scheduled_task_exists` (the Tasks-folder source the scanner enumerates) is the task
completion source.

## Alternatives considered

Treating API success as sufficient — rejected, it is the failure mode this batch exists to
close. Treating Unknown as success after a successful delete call — rejected, fail-closed
is the repo rule for unexplained states. Reporting a different failure string for the new
"verified still present" case — rejected, it would break the follow-up matching contract;
the reason goes to the log instead.

## Consequences

A 32-bit app's uninstall registration is now deleted in the view it was scanned in.
A registration that survives the delete call (or whose state cannot be verified) is
reported as failed with the reason in the log, never as success. Repeat cleaning of an
already-absent registration counts as completion instead of a spurious failure. macOS and
Windows native residual channels now share the same shape: per-item identity gate for
filesystem paths, verified-absence completion for registration resources.

## Verification

- `src/platform/windows/residuals.rs::registry_residual_clean_verifies_absence_after_delete`
- `src/platform/windows/residuals.rs::scheduled_task_delete_counts_absence_as_completion`
- `src/platform/windows/registry.rs::delete_then_verify_reports_verified_absence`
- `src/platform/windows/registry.rs::deleting_a_missing_tree_reports_failure`

Proved: with the `delete_registry_target` call in the registry-key branch reverted to the
old "delete call success = ok" logic, `registry_residual_clean_verifies_absence_after_delete`
fails — the idempotent re-clean of an already-absent fixture key is reported as `Failed`
(exit 101); evidence
`docs/agent-notes-evidence/2026-10-06-native-residual-verify-red.log`. Restoring the guard
makes the same focused run green (`...-verify-green.log`). The registry fixture is a
self-created key under the HKCU hive (`Software` / `QuickCleanerTest` / process id) —
self-created and self-cleaned, never user resources.

Limitation: the WOW64 view defect itself cannot be reproduced on this host without a real
32-bit HKLM registration and admin rights, so that part is fixed by construction (delete
through the parent opened with the scan's view flags) and covered by the shared
delete/verify round-trip rather than a red-run. This is recorded honestly as not
red-run-proven.
