# Agent Note: Complete Hermes split-install removal while preserving retry evidence

Status: implemented

## Problem

The audited Hermes bootstrap installs source, venv generations and tools in separate directories. Its lite uninstall can return zero after warnings while modern dependency stores and Windows registrations remain. Missing launchers also hid half-uninstalled installations from the application list.

## Decision

Validate the source signature or the exact canonical source-path hash and recorded venv scope. Recover discovery when launchers, source or Python are missing. Use the official lite command when available, then remove only proven program artifacts through core cleaner. Keep install/tool facts until other deletion and registration verification succeed so failed cleanup remains discoverable and retryable. Recheck live installers/processes and shared owners before deletion. Preserve configurations, credentials, sessions, unknown files and tools referenced by another installation; named profiles block checkout removal. Registration cleanup runs as the real user and requires exact installation paths in task actions, Startup scripts and environment values.

## Alternatives considered

Deleting the entire Hermes home would remove user data. Trusting exit zero or disappearance of uninstall.py misses external dependencies. Deleting all tools or tasks by name breaks other installations. Keeping only an in-memory plan loses retry evidence after a partial failure.

## Consequences

Uninstall completion is artifact based. A no-op official command can succeed only after independently verified cleanup. Sealed installations retain conservative capabilities. Unrecorded files and empty directory shells may remain intentionally; this is program removal, not erasure of the data home. Unsafe or unreadable ownership evidence blocks removal.

## Verification

- `src/platform/windows/app_discovery.rs::half_uninstalled_hermes_and_dead_shortcuts_remain_discoverable`
- `src/platform/windows/app_discovery.rs::dependency_record_recovers_without_source_or_runtime`
- `src/platform/windows/app_discovery.rs::stale_residual_scan_cannot_remove_newly_shared_tools`
- `src/platform/windows/source_install.rs::split_runtime_cleanup_preserves_data_and_unrecorded_files`
- `src/platform/windows/source_install.rs::late_failure_keeps_install_record_for_recovery`
- `src/platform/windows/source_install.rs::other_installations_and_profiles_do_not_lose_shared_tools`
- `src/platform/windows/source_install.rs::gateway_task_cleanup_requires_exact_installation_action`
- `src/platform/windows/source_install.rs::registration_cleanup_handles_quoted_paths_and_preserves_unrelated_values`

Proved: before recovery discovery was implemented, the missing-launcher test expected one app but got zero (cargo exit 101). The fixed test passes using real dead Windows shortcuts. Isolated physical-directory tests verify external runtime removal and retained data; isolated HKCU tests verify quoting and unrelated values; disabled temporary Task Scheduler entries verify action ownership. The real external Python runner test also passes. No real Hermes uninstall was performed.
