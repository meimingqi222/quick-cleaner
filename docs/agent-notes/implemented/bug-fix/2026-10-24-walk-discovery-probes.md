# Agent Note: Walk discovery confines a tag only after it opens

Status: implemented

## Problem

Unelevated Go discovery took 51–66s on the same code roots where Rust took
11–14s. Both found the same 641 items. Every directory without a marker
confined a missing `CACHEDIR.TAG` along its whole ancestor chain before the
open failed. Each directory also re-confined itself without a memo. Roots
and hit measurement ran one after another.

## Decision

In `internal/core/devscan.go`, `hasCacheDirectoryTag` opens the tag first.
Only an opened file is confined, and the opened handle must match the
confined path (`os.SameFile`). Otherwise a swap after the open is refused.
`walkCollector` memoizes ancestor evidence for one root. Before measurement,
`discoverWalked` confines each hit again without the memo. Roots run
concurrently. Hits are measured on one shared fan-out budget
(`measurePathShared`). Output keeps root order.
`ruleLstat` in `internal/core/rule_facts.go` is the default probe, so the
test can count probes.

## Alternatives considered

- Drop confinement in the walk entirely. Rejected: a junction could still
  redirect evidence reads.
- Keep the confine-first order and only parallelize. Rejected: the cost is
  O(depth) per directory either way, and the walk still scaled with depth².

## Consequences

The walk channel matches Rust: 11.3s, with the identical 641 paths and
157.47 GB on this machine. Indexed discovery is unchanged.

## Verification

- `internal/core/devscan_test.go::TestWalkDiscoveryStatsEachDirectoryOnce`

Proved: with the confine-before-open order restored, the test failed at
`walk discovery made 1643 ancestor probes for 200 directories`; see
`docs/agent-notes-evidence/2026-10-24-walk-discovery-probes-red.log`. With
the order restored to open-first, the same test passes; see
`docs/agent-notes-evidence/2026-10-24-walk-discovery-probes-green.log`.
