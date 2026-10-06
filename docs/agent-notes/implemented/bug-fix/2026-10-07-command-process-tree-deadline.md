# Agent Note: Command deadlines terminate descendants and bound pipe completion

Status: implemented

## Problem

A cmd shim starts the actual tool as a descendant. Killing only cmd leaves the tool
running, possibly modifying files, and inherited stdout/stderr keep read_to_end joins
blocked. A two-second deadline actually returned after the descendant's four-second sleep.
An exited parent has the same pipe problem even when its exit status is successful.

## Decision

The shared runner launches Windows children suspended, assigns a kill-on-close Job Object,
then resumes the sole initial thread through public Toolhelp/OpenThread/ResumeThread APIs.
Assignment or resumption failure kills and reaps the suspended child, with no uncontained
fallback. No tool code runs before containment. Unix commands use a fresh process group.

Both outputs are drained with bounded nonblocking reads, using PeekNamedPipe on Windows
and O_NONBLOCK on Unix. Process exit and both pipe EOFs share one deadline. Every return
closes the job or kills the group, kills and reaps the direct child. Output and exit code
remain available on normal completion; failures return None.

## Alternatives considered

Killing just the parent and detaching readers permits the actual uninstall to continue.
Assigning a job after an ordinary spawn leaves a race in which the tool can create an
uncontained child. Process enumeration plus taskkill is not atomic and requires another
potentially blocked command. These alternatives are rejected.

## Consequences

No mutation threads are detached. Descendants cannot deliberately break away from the
Windows job. Tools that leave background descendants are stopped when the command completes.
Existing owner and inventory commands share this containment; updater handoffs use their
separate launch path. macOS uses process groups and has not been executed on this host.

## Verification

- `src/core/proc.rs::review_timeout_kills_shim_descendants_and_closes_pipes`
- `src/core/proc.rs::review_exited_parent_does_not_bypass_the_pipe_deadline`
- `src/core/proc.rs::review_large_stdout_and_stderr_are_drained_without_deadlock`
- `src/core/proc.rs::windows_background_command_has_no_console_and_keeps_output_and_exit_code`

Proved: pre-fix real cmd/PowerShell descendant test failed the elapsed-time assertion,
returning after 4.5 seconds for a two-second deadline. Output is committed at
`docs/agent-notes-evidence/2026-10-07-review-removal-red.log`; the restored focused tests
are recorded at `docs/agent-notes-evidence/2026-10-07-review-removal-green.log`. Ready and
completion files prove the descendant actually started and cannot perform its delayed
write after timeout. Pitfalls checked: P20/P23/P40/P43/P46.
