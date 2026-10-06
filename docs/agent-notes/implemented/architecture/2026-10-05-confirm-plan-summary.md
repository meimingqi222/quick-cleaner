# Agent Note: Confirmation shows the frozen plan summary before cleanup

Status: implemented
Partly-superseded-by: 2026-10-06-rules-carry-no-user-facing-version.md

## Problem

GOAL E requires every cleanup entry to show the real plan summary before it runs:
scope, rule version, operations, preserved items and blocked reasons. The smart-clean
confirmation (`ui::actions::clean::request_clean_selected`) showed only the item count
and total size, plus a static caution paragraph. The rule version and the blocked
reasons that the scan had already frozen into each `CleanupPlan` were never surfaced,
so a user confirming a batch could not see which rules would act or that some targets
were already blocked.

## Decision

`JunkState::selected_plan_summary` walks the selected items' **already-frozen**
`ScanItem::plans` and returns `(rule labels, operations, blocked reasons)`, where each
rule label is `id@version` from `plan.rule.snapshot.definition(&plan.rule.id)` and the
operations are the distinct `Operation`s of the frozen targets. `Root` exposes a thin
delegate. `request_clean_selected` appends the non-empty parts to the confirm dialog's
detail via localized helpers (`tr_confirm_plan_rules`, `tr_confirm_plan_operations` +
`tr_operation_name`, `tr_confirm_plan_blocked`).

The summary reads the frozen plans verbatim — it never re-derives the source from the
display path or category, so the confirmation and the execution agree on what will
happen. Blocked reasons are deduplicated for display; the plan itself is unchanged.

The disk-lens confirmation (`ui::actions::disk::request_clean_disk_selected`) is the
other entry that lacked a scope view. Its targets are user-picked paths (not rule
plans), so it shows the resolved scope instead: `confirm_scope_detail` lists up to 12
paths and, when longer, states the total. It never expands or infers the selection —
the disk lens keeps exactly the user's range.

## Alternatives considered

Re-deriving the rule version or blocked set at confirm time from the current snapshot
would let the dialog and the executor disagree (the scan snapshot is the authority).
Reading `CleanupPlan::explanation()` (a diagnostic JSON) in the UI would couple the
view to a diagnostic shape. Adding the summary only for uninstall (which already shows
version and steps) would leave the smart-clean entry — the most common one — blind.
Showing every disk-lens path unbounded would make the dialog unusable on large
selections. All rejected.

## Consequences

The smart-clean confirmation now lists the rules that will run, the operation types
(`delete file`, `delete folder tree`, `empty folder contents`, …) and any blocked
targets with their reasons; the disk-lens confirmation lists the selected scope, in both
languages. No execution behavior changes.

## Verification

- `src/ui/state.rs::selected_plan_summary_lists_rules_and_blocked_reasons`
  (asserts rule labels, the distinct operations and blocked reasons)
- `src/ui/i18n/mod.rs::scope_detail_lists_paths_and_marks_truncation`
- `src/ui/i18n/mod.rs::path_operation_distinguishes_folder_and_file`

Model/UI migration, not an organic bug fix; no red-run proof claimed. Actual unified
gate results and limitations are recorded in RULES_REFACTOR_STATUS.

## Superseded

The `id@version` rule label is superseded by
`2026-10-06-rules-carry-no-user-facing-version.md`: the summary now names the contributing
rule ids only, because rules ship with the app and carry no user-facing version. Everything
else holds — the summary still reads the frozen plans verbatim, still lists the distinct
operations and the deduplicated blocked reasons, and the disk-lens scope detail is
unchanged. The test above is the same test under its current name.
