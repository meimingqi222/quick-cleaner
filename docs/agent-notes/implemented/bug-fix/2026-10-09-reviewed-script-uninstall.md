# Agent Note: Reviewed scripts share an executor and internal GUI uninstallers never fall back to trash

Status: implemented

## Problem

The Qingjian adapter hardcoded a bundle identity, script path and two installation paths. Another reviewed script could not reuse the existing execution guarantees. MSIME has an internal GUI uninstall action rather than an external command; treating it as an ordinary app would trash only one bundle and bypass its input-source precondition. Its Windows uninstaller also deletes owned user data before residual selection.

## Decision

Schema 11 adds bounded script_uninstallers and uninstall_data_warnings to existing bundled rules. The macOS executor reads one cloned script specification, validates the identity and confinement of every declared copy, applies core safety, freezes reviewed bytes, passes literal arguments to a fixed sh/bash interpreter, preserves the original user's HOME, clears shell startup environment and elevates only when a system copy exists. Completion requires every declared bundle to be NotFound. Failures never grant fallback deletion. The required preserves_user_data flag records the reviewed argument combination and adds a pre-execution warning when false. Only self-contained scripts are supported; script-path, resource, working-directory and terminal-dependent scripts are excluded.

manual_uninstall_bundle_ids explicitly guards applications with only internal GUI actions. The UI returns guidance before scanning or requesting confirmation; the platform also refuses direct removal. MSIME's settings and input-method identities are listed exactly for six editions; no prefix or vendor-family match is used. No process is automatically opened because starting the settings app can refresh/reinstall the input method. Users complete the official action and remove the companion app manually, then refresh.

Windows uninstall_data_warnings use exact ARP identity, publisher and version-bound display-name matching. This matching is shared with data aliases instead of duplicated. The confirmation explains that the official MSIME uninstaller deletes edition-owned data, including custom data directories. Existing Windows native execution and completion checks remain authoritative.

## Alternatives considered

A plugin system or workflow state machine would duplicate native uninstall paths. Filename-based shell execution would turn discovery into execution authority. Launching an internal Tauri command externally is not a supported interface. Automatically opening MSIME risks reinstalling the bundle. Deferring data warnings to residual review is too late because the official uninstaller has already deleted data. These alternatives are rejected.

## Consequences

A second fictional product is discovered and uninstalled using configuration only, with a different script path, Bash and literal arguments. Qingjian behavior remains covered by the moved regression tests. MSIME macOS receives guidance rather than unattended uninstall; automatic Homebrew routing and real input-source interaction are not implemented. Tests use dedicated fixtures, and AppleScript tests remove administrator authorization. Windows identity policies run cross-platform; Windows-native execution needs Windows acceptance.

## Verification

- `src/core/rules/uninstall.rs::msime_manual_uninstall_matches_exact_editions`
- `src/core/rules/uninstall.rs::msime_data_warning_requires_complete_registration_identity`
- `src/core/rules/uninstall.rs::script_data_warning_follows_reviewed_arguments_policy`
- `src/core/rules/uninstall.rs::script_uninstall_policy_rejects_ambiguous_or_unbounded_commands`
- `src/platform/macos/script_uninstall.rs::script_uninstall_config_only_extension_runs_bash_and_literal_arguments`
- `src/platform/macos/script_uninstall.rs::script_uninstall_elevated_arguments_are_literal`
- `src/platform/macos/script_uninstall.rs::msime_manual_uninstall_does_not_delete_either_bundle`
- `src/platform/macos/script_uninstall.rs::qingjian_script_verifies_both_copies_after_elevated_operation`

Proved: temporarily disabled the manual-uninstall identity guard; the exact-edition regression failed for app.msime.macos without running any native uninstall. Restoring the guard passes the same focused test. Logs: `docs/agent-notes-evidence/2026-10-09-reviewed-script-uninstall-red.log` and `docs/agent-notes-evidence/2026-10-09-reviewed-script-uninstall-green.log`.

The initial unsandboxed native fixture exposed an incorrect root discriminator: macOS parse_app_bundle uses Hkcu, not Unregistered. Corrected matching follows this native representation; the fixture asserts the parsed identity, root and manual decision before invoking the platform entry. Its actual failure is appended to the red evidence. A sandbox-only deletion error must never stand in for a manual-uninstall guard.

Final validation: cargo fmt --check, cargo build, cargo clippy --all-targets -- -D warnings, bundled rules check (schema 11), strict notes, pitfall pointers and UI localization pass. The unsandboxed uninstall-focused suite passes 16 tests. Full library suite: 728 passed, 1 failed, 11 ignored; the remaining pre-existing installed_bundle_ids_returns_known_apps failure reports unavailable Spotlight inventory on this host. No real input method is uninstalled and no administrator authorization is requested by tests. Windows-native execution remains unverified.

Pitfall audit: reviewed the full list. P1's official Windows command parsing is untouched; P10/installed-owner fail-closed behavior is retained, including the Spotlight failure; P46's bounded process-tree runner is reused; P49's exact identity, frozen bytes, original HOME, retained data and multi-copy completion remain covered. P50 records the internal GUI and data-removal constraints plus the native Hkcu root representation. Other listed deletion, path, residual and UI protections are not weakened.

Evidence logs redact host-specific workspace and temporary-directory prefixes with <workspace> and <tmp>; assertion contents and test results are retained.
