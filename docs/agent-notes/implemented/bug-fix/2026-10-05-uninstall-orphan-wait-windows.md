# Agent Note: UAC gaps and vanished uninstall processes use distinct wait windows

Status: implemented

## Problem

A 15-second fail-fast killed slow UAC flows: the user is still looking at the elevation prompt (or walked away) while the UI already reported uninstall failure. Inno/NSIS uninstallers are two-stage — the parent copies itself to a temp directory and relaunches (same `unins000` stem, so `saw_procs` becomes true), the second stage exits with `RestartElevated`, and during the UAC window the process list is empty. That state is signal-identical to "user cancelled and the wizard exited cleanly"; only a longer timeout can tell them apart. Application processes inside the install directory being closed during UAC also lands in the same `ProcsVanished` state.

## Decision

`uninstall_orphan` splits the two empty waits: `FailedCommand` (the command failed and no process was ever seen) stays at 15 seconds; `ProcsVanished` waits 120 seconds. `child_ok && !saw_procs` still returns `None` and falls through to the 30-minute overall timeout. `saw_procs` counts only processes observed inside the `wait_for_uninstall_settled` loop — it never includes the parent consumed by `child.wait()`.

## Alternatives considered

Merging both waits into one 15-second window — kills ordinary UAC. Compressing `ProcsVanished` to make cancel feel faster — reintroduces the misfire on every slow elevation. Treating `saw_procs` as including the waited parent — misclassifies normal UAC gaps as orphaned.

## Consequences

The bound tests assert `ProcsVanished`'s timeout stays at or above 60 seconds and strictly longer than the failed-command window, so the split cannot silently collapse again.

## Verification

- `src/platform/windows/apps.rs::uac_gap_is_not_orphaned`
- `src/platform/windows/apps.rs::wizard_ran_then_exited_is_orphaned`
- `src/platform/windows/apps.rs::failed_command_without_process_is_orphaned`

Proved: organic red — the motivating failure was observed on the real machine (uninstall reported failure while the UAC prompt was still open; recorded in the pitfalls list before this note existed); the bound tests reproduce the UAC gap, the two-stage wizard exit, and the fast-failing command with distinct expected outcomes.
