# Agent Note: Windows startup stays cloaked until the first frame

Status: implemented

## Problem

On Windows the main window flashed white, title bar and all, before the shell
appeared. The captured first screen of that window was a blank client.

## Decision

`internal/app/run.go` calls `prepareStartupWindow` before `NewWindow` and
`finishStartupWindow` after the close handlers. `internal/app/startup_windows.go`
creates the window hidden and fills it with `internal/ui/theme.go`'s
`ShellBackground`. `presentStartupWindow` cloaks the HWND, shows it so the
surface can paint, and removes the cloak from `OnReadyToShow` after `DwmFlush`.
A later show is not sent if the cloaked show already happened. If no frame
arrives within two seconds, the same reveal runs so the window cannot stay
invisible. Other platforms are unchanged.

## Alternatives considered

Setting only `BackgroundColor` does not paint the client: the surface child
covers it and erases nothing, so DWM still composites white. Leaving the window
hidden until `OnReadyToShow`, without showing it cloaked, may never paint a
content window, so the reveal never runs. `STARTUPINFO` `SW_HIDE` and an
elevation `nShowCmd` of 0 were not used; they can hide the real GUI window.

## Consequences

The first image the desktop shows is the frame MyGo already presented. The
taskbar can list the window while it is still cloaked. The two-second fallback
can still show a white client if the first frame never arrives. Closing the
window before the frame skips the reveal.

## Verification

- `internal/app/startup_windows_test.go::TestStartupWindowStaysCloakedUntilFirstFrame`

Proved: manufactured red — removing only the cloak-before-show call failed
`TestStartupWindowStaysCloakedUntilFirstFrame` at `before frame cloaks=[] shows=1`;
see `docs/agent-notes-evidence/2026-10-24-startup-white-flash-red.log`. Restoring
that call passes the focused startup tests; see
`docs/agent-notes-evidence/2026-10-24-startup-white-flash-green.log`.
