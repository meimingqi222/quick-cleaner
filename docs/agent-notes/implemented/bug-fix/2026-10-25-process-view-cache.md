# Agent Note: The process view is computed once per input change

Status: implemented

## Problem

`filteredProcesses` filtered and sorted the whole process table on every
render, and `processRows` grouped it again. Pointer moves and hovers rebuild
the page, so this ran many times per sample. Rust builds the view once per
sample (`rebuild_proc_view`).

## Decision

`processViewCache` in `internal/ui/status_page.go` keys the filtered view on
the snapshot pointer, query, filter, current user and sort. Each sample
publishes a new snapshot pointer. `cachedProcessRows` adds the set of
expanded groups to the key, because `ToggleGroup` mutates `ExpandedGroups` in
place.

## Alternatives considered

- Recompute in the controller after each sample. Rejected: query, filter and
  sort changes happen in the view between samples.

## Consequences

Hover-driven rebuilds reuse the cached slices.

## Verification

- `internal/ui/status_view_cache_test.go::TestProcessViewIsComputedOncePerInputs`

Proved: with the cache bypassed, the test failed at `unchanged inputs
filtered and sorted the processes again`; see
`docs/agent-notes-evidence/2026-10-25-process-view-cache-red.log`. Restored,
it passes; see `docs/agent-notes-evidence/2026-10-25-process-view-cache-green.log`.
