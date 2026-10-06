# Agent Note: Windows residual cleanup rechecks the scanned identity

Status: implemented

## Problem

The residual channels delete leftovers of an app that is no longer installed. macOS
`clean_residuals` re-checks each item's scanned identity immediately before deleting
(`item.identity.recheck(path)`), so a path that was replaced between scan and click —
same name, different object — is refused instead of deleted. The Windows
`clean_residuals` never did this: for a `File`/`Directory` residual it went straight
from the pending-reboot check to `dispose(path, RecycleBin)`. A live app that
recreated a same-named file at a scanned path in that window (or a reinstall) would
have its fresh data moved to the Recycle Bin.

The asymmetry was silent because both platforms looked "complete": macOS had the gate,
Windows compiled and passed its tests without it.

## Decision

Windows `clean_residuals` gains the same gate, immediately after the pending-reboot
check and before `dispose`: `item.identity.is_some_and(|identity| identity.recheck(path))`
must hold. On failure it records a `CleanResult::Failed` (and bumps the failed counter)
and skips the item — it never falls back to a bare delete. A missing identity is also a
refusal, matching the fail-closed rule ("读取失败是 Unknown，不能当作删除授权").

The gate uses the same `TargetIdentity` the scan captured. On Windows that identity now
carries the stable object id (see the Windows stable-identity note), so a same-second,
same-length delete-and-recreate is caught here too.

## Alternatives considered

Trusting the earlier `validate_discovered_residual_clean` snapshot check is not enough:
that runs once for the whole batch before the clean, not per item at delete time, and it
is Windows-discovery-specific. Relying on the app-gone check is also not enough — it
proves the owner is gone, not that this specific path still holds the scanned object.
Skipping only when the file is missing would leave the replacement deleted. All rejected.

## Consequences

A Windows residual whose path changed after the scan is refused and reported as failed
instead of being trashed; the untouched replacement stays on disk. No behavior change
for unchanged items. macOS and Windows now share the same per-item identity gate.

## Verification

- `src/platform/windows/residuals.rs::residual_cleanup_rejects_path_replaced_after_scan`
- `src/platform/macos/residuals.rs::residual_cleanup_rejects_path_replaced_after_scan`

Bug-fix note. The Windows test creates a residual, replaces the file after capture, and
asserts `clean_residuals` reports `CleanFailure::Path` and leaves the replacement intact.

Proved: with the new gate removed, `cargo test --lib residual_cleanup_rejects_path_replaced_after_scan`
fails — `report.failed` is empty and the replacement is trashed (assertion `left: []` vs
the expected `Path(...)`); evidence
`docs/agent-notes-evidence/2026-10-05-windows-residual-identity-red.log`.
Restoring the gate makes the same focused run green
(`docs/agent-notes-evidence/2026-10-05-windows-residual-identity-green.log`). Actual
unified gate results and limitations are recorded in RULES_REFACTOR_STATUS.
