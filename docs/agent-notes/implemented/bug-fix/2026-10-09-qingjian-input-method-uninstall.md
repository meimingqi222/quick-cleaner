# Agent Note: Qingjian input method discovery and retained data cleanup

Status: implemented

## Problem

Qingjian installs its macOS bundle in system or user Input Methods, outside the application roots. Its official uninstaller is a shell script rather than an Uninstall app. Windows uses an Inno Setup registration named 青简, while data directories use Qingjian; the short-name gate and display-name matching omit them.

## Decision

Include both Input Methods roots in application discovery and direct installed-owner probes. The shared reviewed-script executor requires its exact bundle identity and supported installation locations, validates both copies because the official script removes both, rejects redirected paths, and only executes reviewed SHA256 script bytes from bundled policy. Execution preserves data, pins PATH and the original user's HOME, quotes AppleScript arguments, requests elevation only for the system installation, and verifies both bundles are absent. Unknown scripts or failed operations never fall back to trash. Future upstream script changes require review and a new digest. The system input-source list still follows the upstream logout/login requirement; no shared Apple preferences are deleted.

Windows residual policy declares a bounded data alias requiring the exact Inno uninstall key, publisher and display name (including known version suffixes tied to DisplayVersion). This selects only the exact Qingjian child under the existing scan roots, rejects links/protected paths, and keeps the short-name fuzzy gate unchanged. These data items are Possible, not preselected. macOS Application Support for this bundle is also not preselected. Existing identity, occupancy, pending-reboot and verified-completion gates remain intact. Schema 10 declares the new alias shape; both residual rule versions increase.

CFPreferences defaults reads can fail on sandboxed temporary plists; canonicalize the path and use bounded plutil per-key reads when defaults fails. The fixture success test requires the actual parsed identity and script selection, so failure-path tests cannot silently pass because metadata was missing.

## Alternatives considered

Lowering the global short-name threshold or treating an English install-folder name as ownership would broaden unrelated cleanup. Executing arbitrary uninstall.sh by filename allows unreviewed commands; a reviewed digest is intentionally restrictive. Passing --purge would erase learning data and keys before the user selects residuals. Running the script as root with root's HOME would target the wrong user. Returning success on exit code 0 misses scripts that swallow deletion errors. These alternatives are rejected.

## Consequences

Normal Qingjian installations become discoverable on macOS. Windows data/log directories are available for explicit cleanup, including an empty ProgramData icon directory. No real input method is installed or removed during verification. macOS script quoting and preserve-data behavior use real system commands on dedicated fixtures; administrator authorization and actual input-source registration remain manual acceptance. Windows native residual integration requires Windows CI; cross-platform alias tests run locally.

## Verification

- `src/platform/macos/apps.rs::qingjian_discovery_includes_both_input_method_roots`
- `src/platform/macos/script_uninstall.rs::qingjian_script_uninstall_preserves_data_and_uses_verified_bytes`
- `src/platform/macos/script_uninstall.rs::qingjian_script_zero_exit_and_failure_do_not_grant_trash_fallback`
- `src/platform/macos/script_uninstall.rs::qingjian_script_rejects_unreviewed_content_and_redirected_paths`
- `src/platform/macos/script_uninstall.rs::qingjian_script_verifies_both_copies_after_elevated_operation`
- `src/platform/macos/script_uninstall.rs::qingjian_elevated_command_quotes_home_and_does_not_pass_purge`
- `src/platform/macos/residuals.rs::qingjian_learning_data_is_listed_but_not_preselected`
- `src/core/rules/app_data.rs::qingjian_data_alias_requires_complete_registration_identity`
- `src/core/rules/app_data.rs::qingjian_data_alias_rejects_links_and_path_escape`
- `src/platform/windows/residuals.rs::qingjian_short_name_residual_scan_lists_exact_data_without_preselection`

Proved: before adding the input-method roots and shipped alias, the focused qingjian run failed the root-presence assertion and the exact data-path assertion (empty result). The successful script fixture also exposed unreadable plist identity in the sandbox. Evidence: `docs/agent-notes-evidence/2026-10-09-qingjian-red.log`. Restored policy and metadata fallback pass the same focused run in `docs/agent-notes-evidence/2026-10-09-qingjian-green.log`. AppleScript standard additions require an unsandboxed test run on this host; no administrator privileges are requested by tests. Windows-native test is not executed on this macOS host.

Final local gates: cargo build, cargo fmt --check, cargo clippy --all-targets -- -D warnings, and strict note/pitfall validation pass. Full library suite: 721 passed, 1 failed, 11 ignored. The existing installed_bundle_ids_returns_known_apps assertion fails before direct-root probing because Spotlight returns a null Bundle ID for a local application bundle; the fail-closed inventory rule is retained. A Windows cross-check using the installed rustup target stops in the ring dependency because this macOS host lacks MSVC C headers (assert.h); Windows integration compilation/execution is not claimed.

Evidence logs redact host-specific workspace and temporary-directory prefixes with <workspace> and <tmp>; assertion contents and test results are retained.
