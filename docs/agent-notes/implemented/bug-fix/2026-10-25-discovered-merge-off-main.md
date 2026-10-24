# Agent Note: Discovered paths are statted on the worker, not at merge

Status: implemented

## Problem

At the end of an elevated junk scan, the main thread stalled for about
260ms (measured with frame logging). `MergeDiscovered` called `os.Stat` on
each of about 1,300 discovered paths. It also normalized each discovered
path again for every fixed target it compared against.

## Decision

`DropVanishedDiscovered` in `internal/core/scanner.go` stats the items. The
discovery worker in `internal/app/junk.go` calls it right after discovery.
`MergeDiscovered` touches no filesystem and normalizes each path once. It
keeps the whitelist and overlap checks, because exclusions and fixed targets
can change during discovery.

## Alternatives considered

- Move the whole merge to the worker. Rejected: categories are owned by the
  main thread and can change while discovery runs.

## Consequences

Phase two's publication no longer blocks rendering on filesystem calls. A
path deleted in the instant between worker and merge stays listed. The
existing identity checks still guard any cleanup of it.

## Verification

- `internal/core/scanner_test.go::TestDiscoveredExistenceIsCheckedBeforeTheMainThreadMerge`

Proved: with `os.Stat` restored inside `MergeDiscovered`, the test failed at
`the main-thread merge consulted the filesystem`; see
`docs/agent-notes-evidence/2026-10-25-discovered-merge-red.log`. Restored, it
passes; see `docs/agent-notes-evidence/2026-10-25-discovered-merge-green.log`.
