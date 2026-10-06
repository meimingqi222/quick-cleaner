# Agent Note: User-level LaunchAgents are booted out before their plist is trashed

Status: implemented

## Problem

macOS residual cleanup documents (`core::apps::ResidualSource::LaunchAgent`) that a
LaunchAgent "随用户登录启动，删前要先 `launchctl bootout`". The elevated path honours
that: `/Library/LaunchDaemons` and `/Library/LaunchAgents` items go through
`elevate::elevated_remove`, which boots out before removing and reports only paths that
are verified gone. The user-level path (`~/Library/LaunchAgents`) did not: the plist was
trashed while the loaded job stayed resident in launchd until logout, so an uninstalled
app's leftover agent kept running after the user cleaned its login item.

## Decision

Before trashing a residual whose source is `ResidualSource::LaunchAgent`, the cleaner
runs `launchctl bootout gui/<uid> <plist>` through the new `bootout_user_agent`. The
command is best-effort (stdout/stderr discarded, result ignored): it is harmless when the
agent is not loaded, and the completion criterion stays the file being gone — the same
criterion the elevated batch uses (verified absence), reached through the trash +
post-call verification path.

## Alternatives considered

Relying on the trash alone — rejected: it leaves the loaded job running, which is the
user-visible symptom this fixes. Making bootout a success criterion — rejected: bootout
fails for a not-loaded agent, and the file is the registration; requiring both would
report false failures for the common already-unloaded case. Booting out by label instead
of path — rejected: the label lives inside the plist and the path form is what the
elevated path already uses.

## Consequences

Cleaning a user LaunchAgent now stops the loaded instance as well as removing the plist,
matching the elevated path's order (unregister, then remove). No change for
non-LaunchAgent residuals or for files with no loaded job.

## Verification

macOS-only change: `src/platform/macos/residuals.rs::bootout_user_agent` and its call in
`clean_residuals_inner`. This host has no macOS toolchain (`pub mod macos` is
`cfg(target_os = "macos")`, and cross-checking the target fails inside third-party C
dependencies), so the change is **not compiled or executed here** — an honest limitation.
What was verified locally: the inserted code is pattern-identical to the already-shipped
`elevate.rs` agent arm (`libc::getuid()` + `["bootout", &format!("gui/{uid}")]` +
null stdio + ignored status), and `cargo fmt --check` parses the file. Compilation and
behavior land with the macOS CI job (`test-macos`) and are recorded as part of the
standing macOS native-acceptance gap in RULES_REFACTOR_STATUS.
