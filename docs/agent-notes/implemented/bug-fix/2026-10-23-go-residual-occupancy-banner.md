# Agent Note: Go residual review shows scan-time occupancy evidence

Status: implemented

## Problem

The Go residual review received process and launchd evidence from the scan, then
dropped it. Rust shows that evidence above the residual list before the user
confirms cleanup: a running process means database residue will fail, while
launchd-only evidence means residue may return at login. Those titles are not
interchangeable, and the banner is not a deletion gate.

## Decision

`showResiduals` copies `ResidualScanResult.Occupancy` onto the page snapshot.
`AppsPage.occupancyBanner` renders the Rust copy: up to three process names
(truncated at 78 runes), one launchd line with up to three labels, a hidden
count, and the matching advice. Empty occupancy renders nothing. Live cleaner
gates stay unchanged.

## Alternatives considered

Folding occupancy into the existing notice would lose the process-versus-launchd
title split and would be overwritten by later verification notices. Treating the
banner as a disable-clean control would change Rust, where occupancy explains
risk and the cleaner still rechecks at disposal.

## Consequences

Windows occupancy remains empty, matching the original Windows API, so the
banner stays hidden there. The banner can be stale relative to the moment of
deletion; it must not be read as current vacancy. No real uninstall or residual
cleanup was run.

## Verification

- `internal/ui/apps_page_test.go::TestAppsPageOccupancyBannerStates`

Proved: the empty, process-plus-overflow, and launchd-only cases disagree on
title, truncation, hidden count, and list separator. A page that ignores
`Occupancy` fails the occupied cases; a page that always shows the running-process
title fails the launchd-only case.
