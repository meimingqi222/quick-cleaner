# Agent Note: Fixed-target table is deterministic and duplicate-free

Status: implemented

## Problem

The matrix rows "范围安全" and "性能" require that the same physical target is only
listed/weighed once and that discovery does not accumulate duplicates. The existing
table-level tests covered no nested pairs (`production_targets_have_no_nested_pairs`)
and absolute/categorised paths, but nothing asserted the fixed-target table has **no
exact duplicate paths** or is **stable across runs** — the precondition for "no
duplicate full scan".

## Decision

`categories::tests::production_target_table_is_deterministic_and_duplicate_free`
builds the production fixed table twice via `all_targets(None)`, normalizes the paths,
and asserts: (1) the two runs produce the same sorted path set, and (2) the set has no
duplicates. `all_targets` resolves the real user home, so this exercises the full
production table, not a fixture.

## Alternatives considered

Asserting a fixed target count would be machine-dependent. Instrumenting `read_dir`
calls would need a production hook for a test-only concern. Comparing only counts would
miss a duplicate that replaces a dropped target. All rejected.

## Consequences

The fixed-target table's uniqueness and determinism are now locked at the table level,
complementing the per-target dedup in `dedupe_paths` and the discovered-channel dedup in
`merge_discovered`.

## Verification

- `src/core/categories/mod.rs::production_target_table_is_deterministic_and_duplicate_free`
- `src/core/categories/mod.rs::production_targets_have_no_nested_pairs`

Test-only addition, not an organic bug fix; no red-run proof claimed. Actual unified
gate results and limitations are recorded in RULES_REFACTOR_STATUS.
