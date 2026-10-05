# Agent Note: Auto-update never crosses architectures and the helper leaves the parent process

Status: implemented

## Problem

Two failure modes in the update handoff. First, the candidate asset list put `aarch64` before `x86_64` while both Mac architectures subscribed to the same list, so an Intel Mac without a universal zip could install the aarch64 package and fail to boot after restart. Second, the replacement script ran as a child of the app process: after `cx.quit()` the session SIGHUP (macOS) or job teardown (Windows) killed the helper mid-run — dying between `mv` and `cp` leaves a disk with `QuickCleaner.app.old` but no bootable main binary.

## Decision

`candidate_asset_names` offers the universal zip first and then only zips matching the running architecture. The macOS helper detaches with `setsid` + `trap '' HUP` + stdio to `/dev/null`; the Windows helper spawns with `CREATE_NEW_PROCESS_GROUP | CREATE_BREAKAWAY_FROM_JOB`, retrying without the breakaway flag when the job object forbids it.

## Alternatives considered

Cross-architecture fallback "so the update still lands" — lands a binary the machine cannot run. Bare-spawning bash/PowerShell as a plain child — the helper dies with the parent exactly when the on-disk state is most fragile.

## Consequences

Both regressions are only visible on real update hardware (no universal zip on an Intel Mac; a helper killed mid-copy), which is why the candidate ordering is pinned by unit tests and the detachment flags are kept as explicit, commented code.

## Verification

- `src/core/updater.rs::mac_fallback_never_crosses_architecture`
- `src/core/updater.rs::intel_mac_does_not_pick_aarch64_when_universal_missing`

Proved: organic red — the motivating failures were observed on real update hardware (Intel Mac boot failure after an aarch64 install; an interrupted helper leaving only `QuickCleaner.app.old`); the bound tests pin the candidate order against both architectures with and without a universal asset.
