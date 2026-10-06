# Agent Note: Declutter list refresh verifies the path is actually gone

Status: implemented

## Problem

GOAL E requires the cleanup list to refresh based on **actual verification**, not just
the report. The path-delete entry already re-checks `path.exists()` after cleaning
(`ui::actions::clean::start_clean_path`), but the declutter entry's post-clean prune
(`prune_cleaned_declutter_items`) removed an item from the list based only on the
report's `failed` set: `selected && !failed.contains(path)`. A selected item whose
disposal returned success but whose file is still on disk (still held open, partially
moved) was removed from the list — the user believes it was cleaned, but it is still
there.

## Decision

`prune_cleaned_declutter_items` now uses a pure helper
`declutter_item_is_gone(path, selected, failed)`: an item is dropped only when it was
selected, is not in the report's `failed` set, **and** `path.exists()` is false. The
helper is a free function so it is unit-testable without a gpui `Root`.

## Alternatives considered

Trusting the report alone is what caused the gap. Re-scanning the whole declutter set
would be expensive and is unnecessary — a per-selected-item `exists()` is enough and
matches the path-delete entry. Removing items that are merely unselected would be wrong.
All rejected.

## Consequences

A declutter item that still exists after cleaning stays in the list (and can be retried),
matching the path-delete entry's behavior. No behavior change for items that were
genuinely removed.

## Verification

- `src/ui/actions/declutter.rs::declutter_gone_requires_report_success_and_actual_absence`

Model/UI migration, not an organic bug fix; no red-run proof claimed. Actual unified
gate results and limitations are recorded in RULES_REFACTOR_STATUS.
