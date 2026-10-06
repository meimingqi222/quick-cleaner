# Agent Note: Native residuals run through the shared capability runner

Status: implemented

## Problem

The previous batch gave residuals typed operations (`Operation::Native`) and a
`cleanup_plan`, but the residual clean still drove the native deletions directly (a
kind-specific `match` calling `delete_registry_target(...)`), so `Operation::Native`
was never executed by the unified runner and the residual channel had no
`PlanExecution` step evidence like every other entry.

## Decision

- `flow::execute_native_residual(plan, index, progress, apply, verify)` runs a
  `Operation::Native` target through the shared capability runner (`execute_capability`)
  with a `NativeResidualExecutor` (`supports` = `Native`, no file occupancy). It
  produces the generic `Revalidate → Apply → Verify{RegistrationAbsent}` steps.
- `residuals::clean_native_residual` builds a single-target `CleanupPlan` for one native
  residual and calls the runner. `apply` reuses the existing `delete_registry_target`
  (three-state: absent-before → delete → absent-after, refusing on Unknown); `verify`
  maps its `Option<bool>` to `Confirmed`/`Absent`/`Unknown`.
- The three native branches of `clean_residuals` (registry key, registry value,
  scheduled task) now go through `clean_native_residual`, keeping the exact report
  accounting (`report.ok += 1` on success, `CleanFailure::Id(...)` on failure) and the
  installs-before-delete / service-stop / identity semantics.
- `residuals::clean_filesystem_residual` does the same for the file/dir branch via
  `flow::execute_filesystem`: the runner's `Revalidate` re-checks the plan (identity,
  protection — the replaced-target refusal stays), `Apply` disposes to the Recycle Bin,
  and `Verify` uses `PathAbsent`. The branch keeps only the pre-run pending-reboot check
  (records `ManualAction`) and merges the runner's report, so the whole residual clean —
  file, registry, task — now runs through the unified executor.

## Alternatives considered

Keeping the direct kind dispatch leaves `Operation::Native` unexecuted and the residual
channel without step evidence. A per-kind executor would reintroduce app-specific
branches. Letting the runner itself verify (rather than reusing `delete_registry_target`)
would duplicate the three-state logic. All rejected.

## Consequences

The residual native clean now yields the same typed step report as the other channels
(`Revalidate`, `Apply`, `Verify`), and a failed delete keeps its reason on the `Apply`
step. The registry/task clean regression tests exercise the reworked branches unchanged.

## Verification

- `src/core/rules/flow.rs::native_residual_runner_reports_revalidate_apply_verify_steps`
- `src/platform/windows/residuals.rs::registry_residual_clean_verifies_absence_after_delete`
- `src/platform/windows/residuals.rs::scheduled_task_delete_counts_absence_as_completion`
- `src/platform/windows/residuals.rs::residual_cleanup_rejects_path_replaced_after_scan`

Model migration, not an organic bug fix; no red-run proof claimed. Actual unified gate
results and limitations are recorded in RULES_REFACTOR_STATUS.
