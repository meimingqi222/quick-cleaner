# Agent Note: Cache-catalog config increment proven at the Rust level

Status: implemented

## Problem

The "发布闭环" / "应用通用性" rows want each migration entry to show a new layout/policy
is added by changing only rules and fixtures. The `~/.cache` catalog (schema 9
`catalog_children`) had a config-increment fixture (`rules/fixtures/cache-catalog-extra.toml`,
which adds a `cache-tool` package) and a saved CLI `explain` report, but no Rust test
proving the extra catalog entry surfaces as a target through the generic capability —
only a manual CLI check.

## Decision

Added `core::categories::cache::tests::cache_catalog_rule_only_fixture_surfaces_a_new_package`:
it replaces the shipped `cache` rule with the extra fixture (same id), validates the
bundle, scans a fixture `~/.cache` containing `cache-tool`, and asserts the target
surfaces with `PackageCache` category and preselected. This mirrors the
`version_rule_only_*` and `path_rule_only_*` Rust config-increment proofs.

## Alternatives considered

Relying on the CLI explain report is not a regression test. Mutating the production TOML
in the test would be flaky. Asserting a count would miss the new entry specifically. The
custom-snapshot scan is the same mechanism the production selector uses.

## Consequences

The cache catalog now has a Rust-level config-increment proof: adding a package is a
TOML line, and the test locks that it surfaces. Together with the path and version
increments, three entries have executable config-increment proofs (the others keep CLI
reports + goldens).

## Verification

- `src/core/categories/cache.rs::cache_catalog_rule_only_fixture_surfaces_a_new_package`
- `src/core/categories/cache.rs::home_cache_catalog_children_preserve_the_migration_baseline`

Test addition, not an organic bug fix; no red-run proof claimed. Actual unified gate
results and limitations are recorded in RULES_REFACTOR_STATUS.
