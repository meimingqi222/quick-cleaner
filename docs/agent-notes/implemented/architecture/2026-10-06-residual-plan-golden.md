# Agent Note: Residual plan golden fixture

Status: implemented

## Problem

The "统一计划" matrix row wants a plan fixture for every entry. The residual channel got
a typed `cleanup_plan` in an earlier batch but had only a shape test
(`residual_scan_result_builds_a_typed_plan`) — nothing pinned the **exact** scope /
operation / completion for each residual kind against a golden, so an accidental change
to the native mapping or the completion condition would not be caught.

## Decision

Added `rules/fixtures/residual-plan-baseline.json` (six entries: file, directory,
registry key, registry value, scheduled task, system extension) and
`core::apps::tests::residual_plan_matches_the_baseline`, which builds a synthetic
residual set, produces the plan via `ResidualScanResult::cleanup_plan()`, and asserts
each target's `path`, `operation`, `scope` and `completion` matches the golden.

The golden was generated from the production code path (a temporary generator test),
then read back by the asserting test — the standard way to create a baseline.

## Alternatives considered

Keeping only the shape test would not catch a changed identifier format or completion
kind. Asserting a fixed target count would miss per-kind drift. The golden is the
minimal artifact that locks the whole typed plan.

## Consequences

The residual channel's plan (scope/operation/completion for every kind) is now pinned to
a fixture, matching the baseline style used by the directory/version/path/cache entries.

## Verification

- `src/core/apps.rs::residual_plan_matches_the_baseline`
- `src/core/apps.rs::residual_scan_result_builds_a_typed_plan`

Test/fixture addition, not an organic bug fix; no red-run proof claimed. Actual unified
gate results and limitations are recorded in RULES_REFACTOR_STATUS.
