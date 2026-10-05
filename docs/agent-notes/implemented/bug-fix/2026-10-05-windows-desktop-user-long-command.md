# Agent Note: Stage long desktop-user commands as data

Status: implemented

## Problem

Hermes uninstall removed its launchers but failed while cleaning registrations. The desktop-user runner passed a generated registration command longer than the 1024 UTF-16-unit CreateProcessWithTokenW limit. The UI stopped its animation, kept the retryable installation and displayed only a generic failure. Tests launched the same registration script through ordinary Command, which does not have this limit.

## Decision

Keep short commands direct. Send long structured commands as JSON to a fixed PowerShell dispatcher running under the same verified desktop-user token. The executable, quoted argument tail and working directory remain data. Bound native command lengths, reject linked staging ancestors and create new staging files exclusively. Reclaim the exact staging files after the child exits. Capture the native error before destroying the environment block, close token handles on preparation failure, and reject failed wait or exit-code queries. Display the failure reason through localized UI status text.

## Alternatives considered

Running the command elevated would modify the wrong user context. Truncating the command loses verification steps. Interpolating arguments into generated script source makes quoting unsafe. Checking only an ordinary unelevated child misses the reported Windows API failure.

## Consequences

Long official calls and existing desktop-user shell calls share the dispatcher. Cleanup completion still depends on verified artifacts and registrations. Failed operations retain recovery records. The dispatcher requires Windows PowerShell and leaves no staged files after a normal completion; abrupt process termination can leave a private staging directory.

## Verification

- `src/platform/windows/source_install.rs::registration_cleanup_handles_quoted_paths_and_preserves_unrelated_values`
- `src/platform/windows/process.rs::long_desktop_command_preserves_arguments_cwd_exit_and_cleans_staging`
- `src/platform/windows/process.rs::token_command_rejects_nul_and_native_overflow`
- `src/platform/windows/app_discovery.rs::official_uninstall_process_preserves_data_and_completes_noop_with_verified_fallback`

Proved: temporarily bypassed staging for commands below 32767 units, then ran the registration fixture in an elevated test process. It failed with the user's exact native error -2147024809 and test exit 101. Restoring the 1024 threshold passes the same elevated fixture. The real child receipt verifies Unicode, quotes, shell metacharacters, working directory, exit 7 and staging reclamation. The external Python isolated-module test passes through the elevated desktop-user runner. No actual Hermes uninstall was executed.
