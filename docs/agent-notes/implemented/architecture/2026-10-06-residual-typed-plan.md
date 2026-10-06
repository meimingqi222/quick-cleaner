# Agent Note: Residual items map to typed operations and a cleanup plan

Status: implemented

## Problem

The "统一计划" matrix row wants every entry to express its targets through the typed
plan model (scope / operation / completion). The residual channel was the exception: the
residual scan produced `ResidualItem`s whose only typing was `ResidualKind`, and
"cleanup" was a `match` over that kind. There was no `Operation` for the native residue
kinds (registry / task / extension), so no residual could be expressed as a
`CleanupPlan` target — unlike source installs, registered-app uninstalls, smart cleanup
and the directory/version layouts.

## Decision

- `Operation` gains one native variant `Native { native: NativeKind, identifier: String }`
  with a new `NativeKind` enum (`RegistryKey`, `RegistryValue`, `ScheduledTask`,
  `SystemExtension`). The variant is wired through the plan model: `is_native_resource`,
  `target_key` (`native:<kind>:<id>`), `validate_target` (native residue is identified by
  its typed `identifier`, so it has no virtual-path requirement), `completion`
  (`RegistrationAbsent`) and `scope` (`NativeResource`).
- `ResidualKind::operation()` maps each residual kind to its typed `Operation`:
  file/dir → `File`/`Tree`, registry key/value → `Native{RegistryKey/Value}` with the
  `root\subpath[ → value]` id, task → `Native{ScheduledTask}` with the task path,
  extension → `Native{SystemExtension}` with `teamID/bundleID`.
- `ResidualScanResult::cleanup_plan()` builds a `CleanupPlan` with one typed target per
  item (falling back to the observed engine rule when the items carry no rule). It is
  diagnostic — the residual clean still performs its own platform checks (identity,
  occupancy, installs-before-delete) — but it gives residuals the same typed
  scope/operation/completion model as every other entry.

## Alternatives considered

Reusing `Operation::Registration` for all native kinds would lose the kind (key vs task
vs extension) and the identifier. Duplicating `ResidualKind` as a parallel
`ResidualOperation` enum would be a third name for the same fact. Moving the residual
deletion into the shared runner is a larger change deferred to a follow-up; this batch
establishes the typed model first. All considered.

## Consequences

Residuals now have typed operations and can be expressed as a plan, closing the last
"only a display kind" channel. The clean path is unchanged this batch (a follow-up can
route it through the runner using `Operation::Native`).

## Verification

- `src/core/apps.rs::residual_kinds_map_to_typed_operations`
- `src/core/apps.rs::residual_scan_result_builds_a_typed_plan`

Model migration, not an organic bug fix; no red-run proof claimed. Actual unified gate
results and limitations are recorded in RULES_REFACTOR_STATUS.
