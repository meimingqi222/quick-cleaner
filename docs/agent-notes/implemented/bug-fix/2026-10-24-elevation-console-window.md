# Agent Note: Elevated console relaunch stays hidden

Status: implemented

## Problem

Running as administrator from `go run` flashed a small window titled with the
temporary executable path, then the main window appeared. The window is the
terminal Windows opens for that elevated process.

## Decision

`internal/platform/security_windows.go` passes `elevationShowCmd` to
ShellExecute. A console program (subsystem 3) uses show value 0, so that
relaunch has no terminal window. A GUI program (subsystem 2) keeps show value
1, because that value is the real window's startup state. The UI process is
the later relaunch and does not inherit the flag.

## Alternatives considered

Hiding every elevation, including a GUI-subsystem build, would also hide the
real window. Leaving show value 1 matches the captured popup. Patching a copy
of the executable to change its subsystem was not used.

## Consequences

`go run` and other console-subsystem builds no longer open that terminal when
they elevate. Cancelling UAC still continues unelevated. A GUI-subsystem build
is unchanged.

## Verification

- `internal/platform/security_test.go::TestElevationShowCmdHidesOnlyConsolePrograms`

Proved: manufactured red — returning 1 for subsystem 3 failed
`TestElevationShowCmdHidesOnlyConsolePrograms` at `console elevation shows a window`;
see `docs/agent-notes-evidence/2026-10-24-elevation-console-red.log`. Restoring
show value 0 passes the same test; see
`docs/agent-notes-evidence/2026-10-24-elevation-console-green.log`.
