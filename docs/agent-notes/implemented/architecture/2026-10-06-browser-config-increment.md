# Agent Note: Browser entry config-only increment fixture

Status: implemented

## Problem

The "全量迁移" and "应用通用性" matrix rows want each migrated entry to prove that a
new layout/app arrives through rules + fixtures alone, with no application-specific Rust
branch. Every other entry already carries a `rules/fixtures/*-extra.toml` file consumed by
a `*_rule_only_fixture_*` test (path, directory, development, manifest, version,
cache-catalog). The browser entry did not: its only increment-shaped proof was
`browser_roots_follow_the_embedded_catalog`, whose expectations are derived row-by-row
from the *embedded* `browsers.toml`. That shows the surface tracks the shipped catalog, but
it never demonstrates that swapping in a *different* browsers ruleset — a fixture that adds
a root — surfaces the new root through the shared Chromium leaf vocabulary without touching
Rust. The 2026-10-05 resume checkpoint called this out explicitly as the browser entry's
missing fixture.

## Decision

Added `rules/fixtures/browser-extra.toml`: a fixture copy of the `browsers` rule with one
extra `windows_user_data` row (`NovaSoft/Nova/User Data`, labels `Nova`). The new test
`core::categories::browser::tests::browser_rule_only_fixture_surfaces_a_new_root_with_shared_leaves`
parses that TOML, replaces the `browsers` rule in a clone of the embedded bundle, validates
the bundle, and pins it with `rules::with_snapshot`. It then builds a fixture User Data tree
for the added root (`Default/Cache`, `Default/Code Cache`, plus a `Default/Cookies`
negative) and drives the production `push_chromium_browser_targets` over every
`windows_user_data` row read from the *fixture* snapshot. The added root must surface with
the shared leaves (recommended), carry the fixture's own label, and never list `Cookies`.

No production code changed: `push_chromium_browser_targets` already discovers roots from
`browsers` and leaves from `chromium`; the fixture only proves the wiring.

## Alternatives considered

- Reusing `browser_roots_follow_the_embedded_catalog` as the increment proof: it derives
  expectations from the same embedded catalog, so it cannot fail when a *different* ruleset
  is supplied — it is a self-consistency check, not an increment proof.
- Asserting a hard-coded root name/count in Rust instead of a fixture: that would put a
  browser-specific fact back into code, the exact thing the refactor removes.
- Copying only the `windows_user_data` catalog into the fixture: a partial `browsers` rule
  is not what a maintainer ships; the fixture mirrors the full rule plus the one added line,
  matching `cache-catalog-extra.toml`.

## Consequences

The browser entry now has the same config-only increment proof as every other migrated
entry, closing the last "缺" the resume checkpoint listed. Adding a Chromium-family browser
stays a one-line `browsers.toml` edit (plus recompile); the test fails with a clear message
if the fixture no longer adds the root, so it is bound to the configuration rather than to a
shape assertion. The fixture is a full copy of `browsers.toml`, so it must be refreshed if
the production rule's other catalogs change — the same maintenance coupling as the other
`*-extra.toml` fixtures.

## Verification

Manufactured red/green proof in `docs/agent-notes-evidence/2026-10-06-browser-config-increment-red.log`:
removing the one added `windows_user_data` row from the fixture makes the test panic with
`夹具必须真的新增了这个浏览器根`; restoring it (after forcing a rebuild) is green.

- `src/core/categories/browser.rs::browser_rule_only_fixture_surfaces_a_new_root_with_shared_leaves`
- `src/core/categories/browser.rs::browser_roots_follow_the_embedded_catalog`
- `src/core/categories/browser.rs::browser_layout_matches_the_baseline`

Test/fixture addition, not an organic bug fix. Actual unified gate results and limitations
are recorded in RULES_REFACTOR_STATUS.
