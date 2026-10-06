# Agent Note: Version-layout baseline golden

Status: implemented

## Problem

The "全量迁移" matrix row wants a before/after baseline per entry. The version-layout
entry (Codex CLI / Devin CLI, migrated from Rust loops into `development.toml`'s
`version_layouts`) had a config-increment test (`version_rule_only_fixture_…`) but no
baseline golden pinning the targets its four layouts produce — so a change to a layout's
`versions`/`current`/`exclude`/`downloads` handling would only be caught by the shape
tests, not by a per-entry fixture.

## Decision

Added `rules/fixtures/version-layout-baseline.json` (three targets: the Codex CLI old
version, the Devin old version, the Devin `.tar.gz` download) and
`core::rules::versions::tests::version_layouts_match_the_baseline`. The test builds a
minimal fixture per platform-relevant layout (an old version dir + a current version dir
+ a `current` junction + the lock file + a download package), scans each layout via the
existing `scan` helper, and asserts each target's `layout`/`path`/`category`/`operation`/
`disposal`/`recommended` matches the golden. The golden was generated from the
production code path (temporary generator test) then read back.

## Alternatives considered

Extending the config-increment test would mix "a new layout surfaces" with "the shipped
layouts are unchanged". Asserting a fixed count would miss per-layout drift. The golden
is the minimal artifact that locks the whole entry.

## Consequences

The version-layout entry now has a baseline golden in the same style as the
directory/development/path/cache/residual entries. The baseline covers the
platform-relevant layouts (on Windows: `codex_cli`, `devin_windows`; `devin_unix` is
skipped by `platform.matches()`).

## Verification

- `src/core/rules/versions.rs::version_layouts_match_the_baseline`
- `src/core/rules/versions.rs::version_rule_only_fixture_uses_shared_discovery_and_pinned_snapshot`

Test/fixture addition, not an organic bug fix; no red-run proof claimed. Actual unified
gate results and limitations are recorded in RULES_REFACTOR_STATUS.
