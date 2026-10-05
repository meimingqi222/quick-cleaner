# Agent Note: Plans expose capability scopes, dependencies and completion conditions

Status: implemented

## Problem

Plans captured rule evidence but did not explain exact scope, ordered dependencies or capability completion. Filesystem cleanup could report a successful action without an independent completion check.

## Decision

Each planned target derives a typed extent and completion condition from its immutable operation. Manifest membership remains a distinct boundary. Plans expose capability-owned dependencies: filesystem revalidation, mutation and verification; source installation retains the fixed five-step flow; worktree checkout absence precedes exact registration retirement. The filesystem executor consumes the batch occupancy result, stops dependent steps after failure, unknown or cancellation, and verifies absence or empty contents independently of the operation report. Core reports retain step outcomes and rule version, schema and sequence. Native conditions require native executors and never fall back through filesystem completion.

## Alternatives considered

A generic zero-exit completion condition cannot verify resources. Reprobing directory occupancy per step adds expensive duplicate work. Diagnostic steps alone do not prove execution, so the filesystem entry is wired into cleaner while native adaptation remains explicitly outstanding. A serialized diagnostic plan cannot be loaded as authority.

## Consequences

The rule explanation tool uses the same production plan projection and reports failed variable extraction. Existing compatibility callers, native execution reports, application entrances, parent/child normalization, preservation splitting and UI step details remain in the full goal. Filesystem completion uses one bounded directory read; it does not start a recursive scan. No real Hermes or native ecosystem resource was deleted.

## Verification

Source preflight failures occurring before the runner now retain the frozen plan header and five Blocked steps. The original cause belongs to the first step, and dependent steps record dependency failure; no frozen plan means no invented execution history. This is diagnostic output only. `src/platform/windows/app_discovery.rs::discovered_installation_plan_rejects_new_artifacts_and_keeps_snapshot` exercises scope expansion through the actual reported uninstall facade and checks frozen metadata, stopped actions and preserved executable.

- `src/core/rules/flow.rs::successful_action_without_artifact_removal_fails_completion`
- `src/core/rules/flow.rs::real_file_tree_and_contents_follow_plan_dependencies_and_completion`
- `src/core/rules/flow.rs::failed_cancelled_and_native_steps_never_run_dependents`
- `src/core/rules/flow.rs::shared_busy_or_unknown_probe_blocks_apply_without_reprobing`
- `src/core/rules/flow.rs::unreadable_contents_and_nonfilesystem_conditions_are_unknown`
- `src/core/rules/plan.rs::exported_scopes_and_completion_do_not_guess_from_display_paths`
- `src/core/rules/plan.rs::source_and_worktree_steps_preserve_verification_before_records`
- `src/core/rules/plan.rs::mixed_snapshot_explanation_retains_unknown_and_conflict_without_panicking`
- `src/core/cleaner.rs::scanned_filesystem_target_runs_reported_plan_lifecycle`

Docker and snapshots now share the capability runner. Mutation and read-only completion probes are separate; failed mutation blocks verification, Unknown cannot report success, and byte accounting requires confirmed completion and actual Docker layer deletion. Other native adaptation and native scan identity validation remain outstanding.

- `src/core/rules/flow.rs::native_executor_uses_typed_resources_and_reports_verification_and_accounting`
- `src/core/docker.rs::native_completion_probe_distinguishes_remaining_and_unknown_without_mutation`
- `src/core/rules/execution.rs::snapshot_completion_probe_preserves_unknown_and_only_queries_inventory`

The full Windows suite passed with 485 tests and 9 ignored. Native resources use isolated backends; no real images or snapshots were removed. This does not prove full cross-platform acceptance.

Homebrew now uses the same runner and a completion hook. Fixed cleanup and dry-run checks are separate; failed or unknown previews never update the throttle timestamp. Only recognized empty output confirms completion, and remaining resources return Absent. The fixture hook validates ordering without writing actual settings or invoking Homebrew. Estimated byte accounting remains the existing brew policy, distinct from Docker layer accounting.

- `src/core/rules/flow.rs::brew_records_throttle_and_estimate_only_after_confirmed_preview`
- `src/core/brew.rs::cleanup_completion_distinguishes_empty_remaining_and_unknown_preview`
- `src/core/brew.rs::cleanup_action_is_fixed_and_failure_retains_command_error`

Go and pnpm now reuse the same capability runner. Preflight resolves the fixed owner scope after shared occupancy, identity and rule checks. A preflight miss retains the existing authorized contents route, with root preservation and an explicit ContentsEmpty condition in the execution report. Failed or timed-out mutation never enters that fallback; Unknown verification earns no estimated bytes. Legacy public wrappers reuse PreparedOwner instead of maintaining a second command implementation. The version-suffix fixture injects command output instead of querying installed tools.

- `src/core/rules/flow.rs::owner_runner_records_fallback_extent_and_preserves_root`
- `src/core/rules/flow.rs::owner_runner_never_falls_back_after_attempt_and_accounts_only_confirmed`
- `src/core/rules/flow.rs::owner_runner_blocks_preflight_on_occupancy_identity_and_preserve`
- `src/core/owner.rs::prepared_owner_rejects_sibling_scope_and_invalid_utf8`
- `src/core/owner.rs::prepared_owner_uses_fixed_commands_and_retains_mutation_errors`
- `src/core/owner.rs::pnpm_store_prune_tolerates_version_suffix`
- `src/core/owner.rs::attempted_owner_timeout_never_grants_filesystem_fallback`

These fixtures do not mutate real ecosystem resources. Source lifecycle adaptation, conditional route display before confirmation, and the complete migration acceptance matrix remain outstanding.

Linked worktree cleanup now executes its five fixed steps through the same runner: ownership/readiness validation, core checkout deletion, independent checkout absence, exact Git unregister, and independent checkout/registration absence. The retained Registration owns the precise backlink/common path checked before deletion. The original public unregister wrapper reuses the split action and completion functions. Failed/no-op deletion, changed registration, unknown occupancy or cancellation stops dependents and retains registration. Git errors remain in step reasons and typed readiness reasons continue feeding the existing UI failure summary.

- `src/core/worktrees.rs::cleanup_removes_exact_registration_and_preserves_other_stale_entries`
- `src/core/worktrees.rs::worktree_runner_retains_registration_after_failed_or_unverified_checkout_cleanup`
- `src/core/worktrees.rs::dirty_locked_and_changed_registration_never_fall_back_to_deletion`
- `src/core/worktrees.rs::managed_clean_checkout_cannot_be_deleted_without_reference_retirement`
- `src/core/worktrees.rs::unregister_requires_missing_checkout_and_unchanged_backlink`

Fixtures create their own local Git repositories; no actual user checkout or application-owned session was cleaned. Scan-time full registration identity/shared references and the remaining migration matrix still need acceptance.

Source discovery now carries Arc<CleanupPlan> through InstalledApp cloning and confirmation. InstallationInstance captures exact program/startup artifact paths, object identity and confirmed absence; current layout remains a verification input and cannot widen that frozen scope. Existing with_snapshot already pins the scanned rule bundle and is retained. Confirmed missing artifacts permit half-uninstall recovery, while newly appearing artifacts or unknown identities block. Shortcut and native registration observations remain acceptance work.

- `src/core/rules/plan.rs::frozen_installation_instances_reject_expansion_appearance_and_replacement`
- `src/platform/windows/app_discovery.rs::discovered_installation_plan_rejects_new_artifacts_and_keeps_snapshot`
- `src/platform/windows/app_discovery.rs::half_uninstalled_hermes_and_dead_shortcuts_remain_discoverable`
- `src/platform/windows/app_discovery.rs::dependency_record_recovers_without_source_or_runtime`

The source fixed lifecycle now uses the shared capability runner and retains five PlanExecution steps even after callback failure or cancellation. The first step validates frozen rule/instance authority before invoking native actions. Original platform callbacks still own idle/shared checks, the official user-context invocation, supplemental core cleanup, registration verification and final recovery retirement. UninstallOutcome carries both legacy Result and the diagnostic steps through the platform facade; the UI retains steps on success and failure and displays the completed count. Registered/native adapters still return their legacy result with no invented step history until migration. The existing post-lifecycle settlement probe continues to guard overall success.

- `src/core/rules/flow.rs::source_runner_reports_fixed_dependencies_and_preserves_failure_reasons`
- `src/platform/windows/source_install.rs::late_failure_keeps_install_record_for_recovery`
- `src/platform/windows/source_install.rs::verification_runs_after_runtime_cleanup_and_before_record_removal`
- `src/platform/windows/source_install.rs::failed_registration_or_new_owner_keeps_retry_evidence`

The source fixture finish helper also uses the same runner; it does not maintain a second five-step execution implementation. Expanded shortcut/registration evidence, detailed UI step inspection and integration of final settlement into the plan remain outstanding.
