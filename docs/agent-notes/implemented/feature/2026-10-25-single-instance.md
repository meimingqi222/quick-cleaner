# Agent Note: One instance per session; a second launch wakes it

Status: implemented

## Problem

Every launch started another QuickCleaner, each with its own tray icon,
scans and monitor. Rust had no guard either. MyGo's
`RequestSingleInstanceLock` uses a Unix socket in the temporary directory,
which does not fit Windows here:

- An elevated instance's socket carries a High integrity label, so a plain
  launch cannot connect, judges the socket stale, and starts again.
- Elevation under another admin account uses a different temporary
  directory, so the two instances never meet.
- A check made after elevating costs a UAC prompt for every repeat launch.

## Decision

Windows: `internal/platform/single_instance_windows.go` holds a
`Local\QuickCleaner.Instance` mutex (per logon session) and an auto-reset
`Local\QuickCleaner.Wake` event. Their SDDL grants authenticated users
synchronize and signal rights and lowers the mandatory label to Medium.
Handles are not inheritable, so the update helper cannot keep the lock
alive.

`cmd/quick-cleaner/main.go` checks `InstanceRunning` before
`RelaunchAsAdminIfNeeded` and only wakes the instance. The elevated (or
`--no-elevate`) process calls `AcquireInstance`; a launch that loses the
race also wakes the winner. The second launch grants foreground rights
(`AllowSetForegroundWindow`) before signalling. The app's `OnInstanceWake`
handler calls `showMain`, which now also restores a minimized window.

macOS and Linux keep MyGo's per-user lock (`RequestSingleInstanceLock` plus
`OnSecondInstance`), because there is no elevated copy to bridge.

## Alternatives considered

- MyGo's socket lock on Windows. Rejected for the reasons above.
- Finding and activating the old window from the new process. Rejected:
  UIPI blocks messages to an elevated window.
- A Global\ mutex. Rejected: it would also block other users' sessions.

## Consequences

A repeat launch exits with code 0 in under 100ms, without UAC, and brings the
existing window back from the tray or from being minimized. The kernel
releases the mutex when a process dies, so a crash leaves no stale lock. The
updater waits for the old PID before starting the new binary, so restarts
are unaffected.

## Verification

- `internal/platform/single_instance_windows_test.go::TestSecondLaunchFindsAndWakesTheInstance`
- `internal/platform/single_instance_windows_test.go::TestInstanceObjectsAreReachableFromMediumIntegrity`

Proved: with the label removed and the probe disabled, both tests failed at
`a second launch cannot see the instance` and `acquire`; see
`docs/agent-notes-evidence/2026-10-25-single-instance-red.log`. With only the
label removed, the second failed at `mutex label "" is not Medium`; see
`docs/agent-notes-evidence/2026-10-25-single-instance-label-red.log`.
Restored, both pass; see
`docs/agent-notes-evidence/2026-10-25-single-instance-green.log`.

On Windows, I checked two cases by hand:

- A plain second launch against a hidden plain instance exited with code 0
  in 35ms and showed the window again.
- A plain launch against a minimized elevated instance exited with code 0 in
  92ms, showed no UAC prompt, and restored the window. One process remained.

The macOS and Linux path was compiled but not run.
