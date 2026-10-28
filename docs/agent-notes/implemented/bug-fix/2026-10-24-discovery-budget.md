# Agent Note: Phase-two discovery has Rust's 240s budget and partial flag

Status: implemented

## Problem

Rust bounds phase-two discovery at 240s (`DISCOVERY_BUDGET`). When the budget
runs out, it marks the discovered categories partial. The view then shows the
"Partial" badge and a `≥` size. Go always passed `partial=false`, so an
unindexed walk had no time limit. The view already rendered the flag, but
nothing ever set it.

## Decision

`JunkIndex.DiscoverWithin` in `internal/core/junk_index.go` runs `Discover`
under `context.WithTimeout(ctx, DiscoveryBudget)`. It reports expiry only
when the budget, not the caller, stopped it. `internal/app/junk.go` passes
that flag to `MergeDiscovered`. Items found before expiry are kept. A hit
whose measurement the deadline interrupted is dropped. An understated size
is never shown as exact.

## Alternatives considered

- A watchdog flag like Rust's derived `AtomicBool`. Rejected: Go discovery
  already honours `ctx`, so a deadline context is the same mechanism.
- Treat a user cancel as partial. Rejected: a cancelled scan is not shown.

## Consequences

Slow walks end after 240s and say that the totals are lower bounds.

## Verification

- `internal/core/junk_index_test.go::TestDiscoverWithinMarksOnlyBudgetExpiryPartial`

Proved: with the budget ignored, the test failed at
`an exhausted budget must report partial results`; see
`docs/agent-notes-evidence/2026-10-24-discovery-budget-red.log`. With the
budget restored, the same test passes; see
`docs/agent-notes-evidence/2026-10-24-discovery-budget-green.log`.
