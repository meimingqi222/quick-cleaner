# Agent Note: Completion verification includes the settle window inside the lifecycle

Status: implemented

## Problem

`run_uninstaller_with_timeout` polled artifact disappearance and install idleness only after `plan.execute` returned. That out-of-band check was invisible to the step report: an uninstall that never stabilized still produced a five-step report of all Succeeded, and the "official uninstall incomplete" failure was attributed to no step. Recovery-record retirement was also gated by a check outside the declared lifecycle.

## Decision

The stability inventory excludes only the exact retained installation-state directory and tool-facts file, using the same helper as single-pass verification. Their children and prefix siblings are not exempt. Checking the entire original inventory before retirement deadlocks real split installs; all other frozen artifacts still must disappear. Each stability sample also rechecks shared owners. The final retirement step continues to verify the complete artifact set after removing records.

The settle poll moved into `VerifyCompletion`. `SourceInstallPlan::execute` takes a settle budget and the frozen command; after `verify_with` succeeds, `await_stable` samples the frozen `installed_artifacts` and the idle probe until four consecutive clean samples or budget expiry. An unstable completion reports `VerifyCompletion: Failed`, keeps recovery records, and returns the original "Official uninstall incomplete" reason as the step's failure. `run_uninstaller_with_timeout` passes its timeout straight through and no longer polls after the runner returns. Test helpers call `execute_with` with `settle: None` and keep the single-pass verification.

## Alternatives considered

Keeping the poll outside the runner preserves a lie in the plan report — a target can look fully executed while the caller rejects it. Folding the poll into `verify_with`'s single snapshot cannot observe resurrection over time. Adding a sixth declared step would change the fixed lifecycle contract and rule validation for every source install rule. These approaches are rejected.

## Consequences

`VerifyCompletion` may now take up to the settle budget (~seconds) instead of returning immediately; cancellation and dependency-failure semantics are unchanged. The step report honestly attributes instability, and `RemoveRecoveryRecords` cannot run on an unstabilized completion. Registration scan facts, remaining entry convergence, parent-child merging, UI plan review, update failure matrix, golden baselines and macOS verification stay open in the GOAL matrix; this change is not full refactor acceptance.

## Verification

- `src/platform/windows/source_install.rs::settle_exempts_only_exact_recovery_records_and_rechecks_shared_owners`

Proved: Replacing the positive fixture's artificial never-existed inventory with the actual scanned plan.paths caused settle_verification_succeeds_and_retires_recovery_records to fail before the fix with Official uninstall incomplete, while retained recovery records were still present. Saved TDD red output: `docs/agent-notes-evidence/2026-10-05-settle-recovery-red.log`. The fixture now retains the real inventory; it is not weakened to force completion.

- `src/platform/windows/source_install.rs::settle_verification_reports_failure_and_keeps_recovery_records`
- `src/platform/windows/source_install.rs::settle_verification_succeeds_and_retires_recovery_records`

A red run that skipped the settle check inside `VerifyCompletion` made the negative test fail (it received `Ok` where the frozen artifact list still claimed a preserved file), proving the test binds the real gate. The full Windows library suite passed with 506 tests and 9 ignored; strict clippy, `cargo build`, `cargo fmt --check` and `git diff --check` passed. No real Hermes or system registration was touched.
