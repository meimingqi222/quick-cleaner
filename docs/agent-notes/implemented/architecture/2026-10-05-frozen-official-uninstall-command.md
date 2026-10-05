# Agent Note: Official uninstall commands are frozen into the scanned plan

Status: implemented

## Problem

`source_adapter` built the `OfficialUninstaller` during discovery, but `run_uninstaller_with_timeout` re-derived it at execution through `inspect_adapter` → `external_python`. Between scan and execution, the interpreter under `tools/` or the declared module file could be replaced while the plan still ran a re-discovered command, and a runtime that appeared later could silently switch a recovery-declared plan onto the interpreter route.

## Decision

`CleanupPlan` now carries an `OfficialOperation`: the exact command (provider, executable, arguments, working directory, frozen `installed_artifacts`) plus bounded file evidence — the interpreter and, when the interpreter route is chosen, the declared `module_file`. Windows evidence binds volume serial and file index; a missing or redirected module at scan freezes the no-op PowerShell recovery route instead of the interpreter route, so a module dropped after scanning is never executed. Execution takes the command exclusively from `scanned.official.command()`, which re-verifies every evidence artifact; `CleanupPlan::validate` re-checks it too, so early failures report the frozen step as Blocked instead of launching anything. The rebuilt adapter keeps only layout revalidation duty.

## Alternatives considered

Keeping the rebuilt command and only adding identity checks loses the scanned route decision: a runtime that arrives later would upgrade a recovery plan into an interpreter plan, and replacement of the interpreter between scan and execution would be silently adopted. Capturing only the executable without `module_file` lets a replaced module script run under the still-valid interpreter. Both approaches are rejected.

## Consequences

Plans whose official command was built by an older scan still fail safely: execution without a frozen command reports a missing scanned operation. Discovery hides an installation when frozen evidence cannot be captured, matching the fail-closed evidence contract. Registration scanning facts, full runtime/registration observation, parent-child range merging, remaining entry points, UI review, update failure matrix, golden baselines and macOS verification are still open in the GOAL matrix; this change is not full refactor acceptance.

## Verification

- `src/platform/windows/app_discovery.rs::frozen_official_command_rejects_replaced_interpreter`
- `src/platform/windows/app_discovery.rs::frozen_official_command_rejects_replaced_module`
- `src/platform/windows/app_discovery.rs::frozen_recovery_route_never_switches_to_late_runtime`
- `src/platform/windows/app_discovery.rs::missing_module_freezes_recovery_even_with_runtime`

A red run that reverted both the frozen command in `run_uninstaller_with_timeout` and the `CleanupPlan::validate` re-check made the replacement tests fail: the rebuilt path attempted to `CreateProcess` the swapped text fixture (os error 216), proving the tests bind the real guard. After restoring both guards the focused suite passed 17 tests with 2 ignored. No real Hermes or system registration was touched.
