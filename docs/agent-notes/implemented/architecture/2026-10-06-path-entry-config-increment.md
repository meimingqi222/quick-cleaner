# Agent Note: Path-entry capability proves config-only increments at the Rust level

Status: implemented

## Problem

GOAL's "发布闭环" / "全量迁移" rows want each migration entry to show that a new
layout/cleanup policy is added by changing only rules and fixtures — no app-specific
Rust branch. The path-entry capability (`append_path_targets_with_roots`) was covered by
a before/after baseline (`explicit_path_policies_match_pre_migration_baseline`) and by
the `rules explain` CLI, but had no Rust test proving a **newly declared** path surfaces
as a target through the generic capability.

## Decision

Added `rules/fixtures/path-extra.toml` (one rule, one `[[entries]]` declaring
`home/QuickCleanerFixtureCache`) and
`core::rules::tests::path_rule_only_fixture_surfaces_a_new_target_without_rust_changes`:
it appends the fixture rule to the embedded bundle, builds a snapshot, runs
`append_path_targets_with_roots` with a single synthetic `home` root, and asserts the
new path surfaces with the declared category (`UserCache`), operation (`Contents`),
recommendation and the snapshot-pinned rule reference.

## Alternatives considered

Extending the existing baseline test would mix "the production set is unchanged" with
"a new entry surfaces", which are different claims. A CLI-only explain report is not a
Rust regression test. Reusing a production rule id would collide. All rejected.

## Consequences

The path-entry capability now has a Rust-level config-increment proof: adding a declared
path (no code) produces a target. This complements the existing baseline and the CLI
explain output, and matches the version-layout-extra Rust test for the version layouts.

## Verification

- `src/core/rules/mod.rs::path_rule_only_fixture_surfaces_a_new_target_without_rust_changes`
- `src/core/rules/mod.rs::explicit_path_policies_match_pre_migration_baseline`

Test-only addition, not an organic bug fix; no red-run proof claimed. Actual unified
gate results and limitations are recorded in RULES_REFACTOR_STATUS.
