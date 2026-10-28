# Agent Note: Empty states, paging and layout say what they mean

Status: implemented

## Problem

A UI audit of the Go port found screens that misstated their state or hid
information:

- Search showed "No matching files found" before anything was typed, and
  its footer repeated the status chip.
- An empty junk result listed zero-byte categories, with selection buttons
  that looked usable.
- Declutter lists stopped at 100/100/50/30 entries with no way to see the
  rest. This was inherited from Rust.
- Disk Lens wrapped the ring summary so "GB" sat alone on a line, and
  labelled unlisted used space "Other used 0 items" (inherited).
- The tray's disk list grew into all spare height, leaving a gap above the
  hardware section, and cut a long volume's sizes off.
- The apps table titles were bordered, centred buttons that did not line up
  with their cells.

## Decision

- `SearchPage.emptyCopy` distinguishes three cases: index not ready, nothing
  typed, and no match. The footer keeps only the hints.
- `JunkPage` shows a clean state once scanning and discovery have finished
  with no items, and disables the selection buttons when there is nothing to
  select.
- `DeclutterPage.Shown` with `ShowMore` lists one more page per click. The
  controller resets it on a new scan.
- The disk ring writes each summary part on its own line. Unlisted physical
  usage is "Other used space", and files show their type icon.
- The tray disk list takes only its own height, and the slack goes above the
  quick actions. Card sizes and the percentage get their own lines; only the
  label truncates.
- `tableHeaderButton` gives the apps and search tables flat titles, aligned
  start or end like their cells.

## Alternatives considered

- Fully virtualized declutter lists. Rejected for now: they sit in a page
  scroll with other content, and paging keeps the first screen as before.
- Wrapping instead of splitting the disk summary. Rejected: the unit still
  breaks off.

## Consequences

Each screen states its real state, and every declutter entry can be reached.

## Verification

- `internal/ui/search_page_test.go::TestSearchEmptyStatesExplainThemselves`
- `internal/ui/junk_test.go::TestJunkEmptyResultIsACleanState`
- `internal/ui/declutter_page_test.go::TestDeclutterListsShowMoreInsteadOfHidingTheRest`
- `internal/ui/disk_page_test.go::TestDiskPageEventsAreControllerRequests`
- `internal/ui/disk_page_test.go::TestDiskPageBreakdownPhysicalApparentAndUnique`
- `internal/ui/tray_popover_test.go::TestTrayPopoverKeepsSectionsTogetherAndSizesWhole`
- `internal/ui/apps_page_test.go::TestAppsTableHeadersAlignWithTheirCells`

Proved: each test failed with its fix reverted. Search failed at "an empty
query is not a failed search", junk at "empty result", declutter at "after
show more", the disk ring at "ring summary line "Used 75%" missing", the
tray at "114px between the last disk and the hardware section", and the apps
headers at "Publisher title starts 41px from its cells". Restored, all pass.
Evidence:
`docs/agent-notes-evidence/2026-10-25-search-empty-state-red.log`,
`docs/agent-notes-evidence/2026-10-25-search-empty-state-green.log`,
`docs/agent-notes-evidence/2026-10-25-junk-clean-state-red.log`,
`docs/agent-notes-evidence/2026-10-25-junk-clean-state-green.log`,
`docs/agent-notes-evidence/2026-10-25-declutter-show-more-red.log`,
`docs/agent-notes-evidence/2026-10-25-declutter-show-more-green.log`,
`docs/agent-notes-evidence/2026-10-25-disk-ring-summary-red.log`,
`docs/agent-notes-evidence/2026-10-25-disk-ring-summary-green.log`,
`docs/agent-notes-evidence/2026-10-25-tray-layout-red.log`,
`docs/agent-notes-evidence/2026-10-25-tray-layout-green.log`,
`docs/agent-notes-evidence/2026-10-25-apps-header-red.log`,
`docs/agent-notes-evidence/2026-10-25-apps-header-green.log`.
