# Agent Note: Status sampling runs only on the visible Status page

Status: implemented

## Problem

The Go status monitor enumerated every process and read the hardware every
2s for the life of the app: hidden in the tray, on another page, and even
while "pause" was on, which only stopped applying samples. Every sample also
invalidated the window. Rust's monitor exits when the user leaves the
Status page and starts again on return.

## Decision

In `internal/app/status.go`, `syncStatusSampling` runs on the main thread. It
stores `statusWanted`: Status page active, window shown, not paused.
`Render`, the pause toggle and the window's hide, minimize, show and restore
events call it. `runStatusLoop` skips ticks while sampling is not wanted. It
samples at once when sampling becomes wanted (`statusWake`). A manual refresh
always samples, even while paused, as in Rust. The sampler instance is kept,
so CPU deltas stay meaningful.

## Alternatives considered

- Stop and recreate the goroutine as Rust does. Rejected: a fresh sampler's
  first CPU reading is distorted, and the wake channel is simpler.
- Keep sampling and drop results. Rejected: that keeps all of the cost.

## Consequences

No process enumeration or window invalidation happens unless the Status
page is on screen.

## Verification

- `internal/app/status_sampling_test.go::TestStatusSamplingOnlyOnTheVisibleStatusPage`
- `internal/app/status_sampling_test.go::TestStatusLoopSamplesOnlyWhenWantedOrAsked`

Proved: with the skip removed, `TestStatusLoopSamplesOnlyWhenWantedOrAsked`
failed at `a tick off the Status page sampled`; see
`docs/agent-notes-evidence/2026-10-25-status-sampling-red.log`. Restored,
both pass; see `docs/agent-notes-evidence/2026-10-25-status-sampling-green.log`.
