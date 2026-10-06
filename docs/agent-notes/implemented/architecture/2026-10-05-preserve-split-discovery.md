# Agent Note: Covering targets split around preserved paths at discovery

Status: implemented

## Problem

`dedupe_paths` merges a subtree target's constraints into the outermost covering
parent so the same physical path is never weighed or deleted twice. That merge can
pull a *preserved* path into a parent that deletes the whole subtree: the test
`covered_child_still_contributes_its_preserve_entries` merges a `hermes` child
(whose rule preserves `config.yaml`, `sessions`, `.env`) into an `engine` parent
that is a `Contents` target on the shared root. The parent plan then covers
`sessions`.

The plan layer only ever *refused* such an overlap
(`CleanupPlan::validate` -> "Target covers a preserved path"). GOAL B requires the
other branch too: a parent covering a preserved item must be split into independent
sub-targets, and only refuse when it cannot be split. Refusing wholesale also
punished the preserved item's siblings — a single preserved child hid the rest of
the directory from cleanup.

## Decision

Splitting happens at the **discovery** layer, not inside `CleanupPlan::new`. A plan
is built 1:1 from a scan item (`ScanItem::freeze_plan`), and `CleanupPlan::validate_binding`
requires the selected path to match a planned target, so replacing a plan's parent
target with children would make the parent item uncleanable. Producing independent
`ScanTarget`s instead keeps every item bound to its own plan.

`categories::split_covered_preserves` runs right after `dedupe_paths` in
`collect_targets`:

- Only `Tree` / `Contents` targets are considered — only they delete a whole
  subtree. Exact files, native resources and registrations never cover anything.
- A covering target is replaced by its real children, enumerated once with a
  depth bound (`SPLIT_MAX_DEPTH = 8`) and a total-target bound
  (`SPLIT_MAX_TARGETS = 4096`). A child equal to a preserved path is dropped; a
  child that *contains* a preserved path is recursed into so its siblings still
  get cleaned; every other child becomes its own target.
- A `Contents` parent's children become subtree removals (or exact files) rather
  than another `Contents` pass, because emptying a parent removes each child
  entirely — a nested `Contents` would leave empty shells behind.
- `RuleRef::declares_preserve` is a filesystem-free precheck so entries whose rules
  declare no preserve pay nothing.

When enumeration fails or a bound is hit, the original covering target is kept and
`CleanupPlan::validate` refuses it with the covered path — the overlap is never
silently deleted whole. Splitting only ever *narrows* the scope, so it cannot grant
new deletion authority.

## Alternatives considered

Splitting inside `CleanupPlan::new` is the smallest diff but breaks the
scan-item/plan 1:1 binding used by `validate_binding` and the UI selection. Keeping
the blanket refusal ignores GOAL B's split branch and hides siblings. Splitting into
nested `Contents` targets would leave empty directories. Letting configuration
declare the split would turn rules into a programming language. All rejected.

## Consequences

A covering target that contains a preserved path now yields one target per
non-preserved child (with the preserved subtree left untouched), instead of a
blocked parent. Labels gain the child name (`<parent> · <name>`); children inherit
the parent's category, disposal, rule reference and recommendation. The model
capability is exercised by fixtures; no production rule currently produces a
covering target that contains a preserve (`hermes` targets are program artifacts,
and the other entries declare no preserve), so the behavior is latent but no longer
only a refusal.

## Verification

- `src/core/categories/mod.rs::covering_targets_split_into_independent_children_around_preserves`
- `src/core/categories/mod.rs::unsplittable_covering_target_stays_whole_for_validate_to_refuse`
- `src/core/categories/mod.rs::covered_child_still_contributes_its_preserve_entries`
- `src/core/rules/plan.rs::preserved_overlap_reasons_name_the_direction_and_path`
- `src/core/rules/plan.rs::preservation_and_conflicts_cannot_grant_deletion`
- `src/core/categories/mod.rs::production_targets_have_no_nested_pairs`

This is a model migration, not an organic bug fix; it claims no red-run proof. The
new tests assert the split shape, order independence (both input orders), the
`Contents`->child-removal typing, that the preserved subtree survives on disk, and
that an unreadable covering target stays whole. Actual unified gate results and
limitations are recorded in RULES_REFACTOR_STATUS.
