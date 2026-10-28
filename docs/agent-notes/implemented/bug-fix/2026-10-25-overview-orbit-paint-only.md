# Agent Note: The overview scan ring animates in paint-only frames

Status: implemented

## Problem

The dot circling the overview's scan ring read `c.Now()` and called
`c.After(1/60s)` while building. The whole overview therefore rebuilt 60
times a second during every scan and clean, just to move a 12px dot. P75
names the same mistake for the scan bar.

## Decision

`orbitDot` in `internal/ui/indicators.go` draws the dot in a `Draw` callback
from `Painter.Now()` and asks for the next frame with `AnimationFrame`. With
reduced motion it stays at twelve o'clock.

## Alternatives considered

- `AnimatePose` on the dot. Rejected: a pose eases toward a target and does
  not loop by itself.

## Consequences

A scanning overview builds no frames while the dot moves.

## Verification

- `internal/ui/dashboard_motion_test.go::TestScanningOverviewAnimatesWithoutRebuilding`

Proved: with the build-time `c.After` dot restored, the test failed at `the
scanning overview rebuilt 60 times in a second`; see
`docs/agent-notes-evidence/2026-10-25-overview-orbit-red.log`. Restored, it
passes; see `docs/agent-notes-evidence/2026-10-25-overview-orbit-green.log`.
