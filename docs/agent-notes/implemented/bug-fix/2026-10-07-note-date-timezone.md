# Agent Note: Date-only notes survive cross-timezone CI verification

Status: implemented

## Problem

The notes workflow run 37536644427 rejected six October 7 notes at
2026-10-06 21:50 UTC. Their author was already on October 7 in Asia/Taipei.
The verifier compared date-only filenames and archive headers with the runner's
local date, making valid records fail after moving between timezones.
The source skill and installed verifier share the same defect.

## Decision

Both future-date checks in `tools/regression-notes/verify-notes.py` use the current
civil date at UTC+14, the latest date that can exist in a real timezone. A record
with no timezone cannot be considered future while it is today somewhere.
Invalid calendar dates and archive-before-filename ordering remain errors.
This change repairs the repository copy; the external skill copies still need
the same update before they are used to vendor the script again.

## Alternatives considered

Renaming valid notes to yesterday falsifies their authoring date. Removing the
future-date guard accepts dates future everywhere. An unconditional one-day grace
also accepts tomorrow at UTC midnight, when no timezone has reached it. Setting
CI to Taipei fixes this one author but keeps the script dependent on deployment.

## Consequences

Verification is independent of runner timezone. The upper bound stays strict for
dates that are future everywhere; date-only records cannot establish which timezone
their author actually occupied. Archive seals and test-anchor rules are unchanged.

## Verification

`tools/regression-notes/tests/test_date_timezone.py` runs the complete verifier on
implemented and archived fixtures with a frozen UTC clock. It covers author-today,
future-everywhere, UTC-midnight, invalid-calendar and archive-order behavior.
The notes workflow runs this Python suite directly; its Rust test-list intersection
does not support Python anchors, so the test is cited by file path.

Proved: before the fix, the focused suite failed three assertions, including
rejecting an October 7 note at October 6 21:50 UTC (both implemented and archived),
in docs/agent-notes-evidence/2026-10-07-note-date-timezone-red.log. After replacing
both local-date checks, all seven verifier tests pass in
docs/agent-notes-evidence/2026-10-07-note-date-timezone-green.log.

Commit checklist: read P1-P46 in PITFALLS; this change only affects note validation.
It does not alter command parsing, uninstall launch, residual matching, path safety,
cleanup identity or process deadlines. The newly observed timezone failure is P47.
