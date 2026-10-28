# Agent Note: Loading indicators animate in paint-only frames

Status: implemented

## Problem

During a junk scan, the indeterminate bar under the top bar stuttered and
then stood still for about 1.3s. The recording showed 40 identical frames.
The bar computed its position from `c.Now()` while building the view, and
reading the build clock asks MyGo for no frame. It moved only when something
else repainted: pointer motion, the 2s status sampler, a worker publication.
Logging inside the instrumented app showed renders 0.7–2.2s apart through the
MFT parse, while every render took under 30ms. So the main thread was idle,
and nothing had asked for a frame. The centre indicator was MyGo's built-in
12-spoke `Spinner`, which steps 12 times per 0.9s by design.

## Decision

`internal/ui/indicators.go` draws both indicators in `Draw` callbacks. They
read `Painter.Now()` and call `Painter.AnimationFrame()`, so they repaint at
the display rate without rebuilding the page:

- `scanBar`: the head eases out and the tail eases in, so the bar stretches
  as it enters and shortens as it leaves.
- `loadingSpinner`: a primary-colored arc on a faint ring. The arc grows to
  three quarters and shrinks back while the whole turns, drawn with
  `Path.Circle` and `Trim`.

Both stay still when the desktop asks for reduced motion. The dashboard and
the junk, disk and apps pages use them in place of the old bar and
`ui.Spinner`.

## Alternatives considered

- Call `c.AnimationFrame()` from the build. Rejected: it rebuilds the whole
  page every frame to move a 3px bar.
- Use `Keyframes`/`Timeline`. Rejected: their values are read during the
  build, which has the same cost.
- Keep `ui.Spinner`. Rejected: its look and its 13 steps per second were
  part of the complaint.

## Consequences

The indicators keep moving through quiet stretches of a scan. They are
painted only while visible.

## Verification

- `internal/ui/indicators_test.go::TestScanBarKeepsAnimatingOnItsOwn`
- `internal/ui/indicators_test.go::TestLoadingSpinnerKeepsAnimatingOnItsOwn`
- `internal/ui/indicators_test.go::TestIndicatorGeometryStaysInRange`

Proved: with the build-time bar restored, `TestScanBarKeepsAnimatingOnItsOwn`
failed at `the scan bar asked for no next frame; it stands still until
something else repaints`; see
`docs/agent-notes-evidence/2026-10-25-loading-animation-red.log`. Restored,
both animation tests pass; see
`docs/agent-notes-evidence/2026-10-25-loading-animation-green.log`. An
elevated junk scan recorded at 30fps showed the bar moving continuously for
7s, including through the MFT parse. No frame repeated beyond the capture's
own duplicates.
