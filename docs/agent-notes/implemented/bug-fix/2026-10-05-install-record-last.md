# Agent Note: Remove external runtime before verifying completion and removing records

Status: implemented

## Problem

The split installation's state directory contained both dependency environments and recovery facts. Deferring the whole directory treated runtime deletion as recovery-record cleanup, so verification ran while the external runtime still existed and a failing final tree delete could lose retry evidence too soon.

## Decision

Use the fixed core source lifecycle. Supplemental cleanup removes owned state children while preserving the record's containing child, including nested record layouts. Verify program artifacts and native registrations with facts retained. Recheck occupancy and shared references before final record removal. Earlier dependency failure stops later steps.

## Alternatives considered

Deleting the whole state early loses recovery evidence. Leaving runtime inside the final record deletion conflates substantive cleanup and retry bookkeeping. An app-name branch duplicates the same ownership algorithm for each new layout.

## Consequences

Hermes and the Atlas rule reuse the same lifecycle. Runtime and record deletion are separate phases; shared ownership and preserved user data remain enforced. The isolated fixtures never uninstall the actual local Hermes.

## Verification

- `src/platform/windows/source_install.rs::verification_runs_after_runtime_cleanup_and_before_record_removal`
- `src/platform/windows/source_install.rs::late_failure_keeps_install_record_for_recovery`
- `src/core/rules/execution.rs::lifecycle_keeps_retry_records_after_any_dependency_failure`

Proved: temporarily skipped remove_state_contents and ran the real split-layout fixture. It failed during registration verification because environments still existed, cargo exit 101. Restoring the separate cleanup passes while facts remain available during verification. The late-failure fixture retains facts and supports successful retry after source removal; reordered lifecycle steps are rejected before callbacks run.
