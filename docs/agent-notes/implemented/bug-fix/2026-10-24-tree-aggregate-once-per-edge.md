# Agent Note: Heap tree aggregates roll up once per edge

Status: implemented

## Problem

The Go MFT tree build took 390–450ms per volume. Rust took 150–165ms on the
same volumes. Read and parse times matched. `recomputeHeapAggregates` walked
every file's ancestor chain (O(files × depth)). `NewSizeTree` grew slots by
append after `buildMFTScanResult` had already built a second full slice.
`RebuildChildArrays` copied a slot per `slot()`/`IsDir()` call.

## Decision

In `internal/core/disk_tree.go`, files add to their directory. Then each
directory adds its total to its parent once all of its child directories
have. `heapEdge` is the single parent-link rule that child arrays and
aggregates share. Directories in a cycle never drain, so a cycle stays local
instead of reaching the root. Extra hard-link parents still walk ancestors.
`newSizeTreeFromSlots` takes ownership of slots, and
`internal/core/disk_mft.go` fills them directly.

## Alternatives considered

- Keep the walk and parallelize it. Rejected: the shared counters need
  atomics, and the work stays O(files × depth).
- A DFS from the root. Rejected: orphan directories would lose their direct
  totals.

## Consequences

Tree build: C: 452ms → 205–218ms, D: 387ms → 125–136ms. All 1328 discovered
paths and sizes are identical to Rust's and to the previous Go build. A tree
deeper than the old walk's 256 levels now aggregates fully.

## Verification

- `internal/core/disk_tree_test.go::TestHeapAggregatesRollUpOncePerEdge`

Proved: with the per-file ancestor walk restored, the test failed at
`aggregation walked per-file ancestor chains`; see
`docs/agent-notes-evidence/2026-10-24-tree-aggregate-red.log`. With the
per-edge roll-up, the same test passes; see
`docs/agent-notes-evidence/2026-10-24-tree-aggregate-green.log`.
