# Agent Note: Declutter cleanup confirms its scope before moving to Trash

Status: implemented

## Problem

GOAL E requires every cleanup entry to show its real plan/scope before it runs. The
declutter entry ("remove selected" on the downloads / large files / similar photos /
duplicates tabs) called `clean_declutter_selected` straight from the button and started
moving items to the Trash with no confirmation — the user could not see how many items
would be moved or that the operation is a Trash move (which frees no space).

## Decision

`clean_declutter_selected` now only builds a `ConfirmRequest` with the selected count
(`tr_confirm_declutter_msg`), a title and a detail that states the disposal truthfully
(moving to Trash frees no disk space; empty the Trash to reclaim it; items are
restorable). The actual work moved to `run_declutter_clean`, dispatched from
`confirm_accept` via the new `ConfirmKind::CleanDeclutter(tab)`.

The declutter selection is user-picked paths (like the disk lens), so the confirmation
shows the scope and the operation, not a rule version — consistent with the disk-lens
confirmation.

## Alternatives considered

Adding a rule version to the dialog would be meaningless (no rule produces these paths).
Silently confirming (no dialog) is the behavior the GOAL wants removed. Reusing
`ConfirmKind::CleanDiskSelected` would lose the tab (needed to re-derive the selection).
All rejected.

## Consequences

The declutter entry now shows a confirmation with its scope and the Trash disposal
before moving anything, matching the other cleanup entries. `run_declutter_clean`
re-derives the selection at confirm time, so it cleans exactly what the user selected.

## Verification

- `src/ui/i18n/declutter.rs::declutter_confirm_states_scope_in_both_languages`
- `src/ui/actions/declutter.rs::declutter_gone_requires_report_success_and_actual_absence`

Model/UI migration, not an organic bug fix; no red-run proof claimed. Actual unified
gate results and limitations are recorded in RULES_REFACTOR_STATUS.
