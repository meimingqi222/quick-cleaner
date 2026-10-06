# Agent Note: Registered-app uninstall reports typed steps through the shared runner

Status: implemented

## Problem

`run_uninstaller_reported` gave the UI typed step evidence only for source installs
(`app.discovery.is_some()` → `run_discovered_uninstaller_reported`). A registered
(registry) app — the common case — returned `plan_executions: Vec::new()`, so the
uninstall confirmation and result had no steps at all: no `Revalidate`, no
`official_operation`, no completion check surfaced. GOAL's "统一计划" row wants every
entry to produce a plan and an execution chain.

The blocker was structural: the shared runner's generic `Revalidate` step requires
`target.operation.validate_target(...)`, which is `false` for `OfficialUninstall` (a
command operation has no filesystem scope), so a registered app's plan would be blocked
at Revalidate before the command ever ran.

## Decision

- `flow::execute_capability`'s `Revalidate` step now skips the filesystem target check
  for `Operation::OfficialUninstall` only: a command operation has no filesystem scope,
  so its capability's `supports()` is the authority. Everything else still must pass the
  typed target check. The source channel is unaffected (it uses `SourceLifecycle` steps,
  not the generic `Revalidate`).
- Added `flow::execute_registered(plan, index, progress, run, verify)` and a
  `RegisteredExecutor` (`supports` = `OfficialUninstall` + `Permanent`,
  `requires_file_occupancy` = false). It runs the plan's generic
  `Revalidate → Apply{OfficialUninstall} → Verify{InstallationArtifactsAndRegistrationsAbsent}`
  steps through the same runner.
- `apps::run_uninstaller_and_wait_reported` builds a `CleanupPlan` (engine rule, one
  `OfficialUninstall` target anchored at the install location, or the registry subpath
  when there is none) and calls `execute_registered`. `run` is the unchanged
  `run_uninstaller_and_wait`; `verify` reports `Confirmed` once the app is no longer
  registered. `platform::run_uninstaller_reported` routes Windows registered apps here.

The target's `identity` is the install location's identity when present; the plan's
`validate()` intentionally skips filesystem checks for `OfficialUninstall`, so an app
with no install location still validates.

## Alternatives considered

Duplicating the runner loop for command operations would fork the "统一执行器".
Making `OfficialUninstall.validate_target` return true for any real path would weaken
the check for every caller (including the cleaner's `validate_target` gate). Modelling
the app as `Operation::Registration` needs a virtual registry scheme `is_virtual_path`
does not know and a new executor. All rejected.

## Consequences

A registered-app uninstall now reports the same `Revalidate → official_operation →
completion` steps as a source install, and a failure keeps its original reason on the
`Apply` step (`run_uninstaller_and_wait`'s message is preserved via the executor's
`failure_reason`). Completion that is not met makes the `Verify` step fail. The actual
uninstall mechanics are unchanged.

## Verification

- `src/core/rules/flow.rs::registered_runner_reports_revalidate_apply_verify_steps`

Model migration, not an organic bug fix; no red-run proof claimed. Actual unified gate
results and limitations are recorded in RULES_REFACTOR_STATUS.
