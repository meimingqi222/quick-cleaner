# Agent Note: Browser entry baseline golden

Status: implemented

## Problem

The "全量迁移" matrix row wants a stable isolated fixture/golden for every migrated
entry. The browser entry (`categories::browser`, driven by `browsers.toml` +
`chromium.toml`'s leaf vocabulary) had behavioural tests
(`declared_browser_roots_share_the_chromium_leaf_vocabulary`,
`app_support_caches_follow_the_declared_catalog`,
`browser_roots_follow_the_embedded_catalog`) but no baseline golden pinning the exact
targets its Windows User Data layout produces — so a drift in the leaf vocabulary or
recommendation would only be caught by the shape assertions, not by a per-entry fixture.

## Decision

Added `rules/fixtures/browser-layout-baseline.json` (7 targets: `Default/Cache`,
`Default/Code Cache`, `Default/GPUCache`, `Default/DawnGraphiteCache`,
`Profile 1/Cache` — all recommended; `Default/blob_storage`,
`Default/CachedProfilesData` — listed but not preselected) and
`core::categories::browser::tests::browser_layout_matches_the_baseline`, which builds a
fixture User Data tree (leaves + `blob_storage` + `CachedProfilesData` + a second
profile + `Cookies`/`Local State` negatives), runs
`push_chromium_browser_targets`, and asserts each target's `path`/`label_zh`/`category`/
`operation`/`disposal`/`recommended` matches the golden. The golden was generated from
the production path (temporary generator test) then read back.

## Alternatives considered

Extending the shape tests would mix "leaf vocab is shared" with "the shipped Windows
target set is unchanged". Asserting counts would miss per-leaf drift. The golden is the
minimal artifact that locks the entry, matching the path/directory/version/cache/residual
baselines.

## Consequences

The browser entry now has a baseline golden in the same style as every other migrated
entry; the "每一迁移入口有稳定隔离夹具/金样" requirement is met for it. The fixture
also pins the negative cases (empty cache dirs, `Cookies`, `Local State` don't surface).

## Verification

- `src/core/categories/browser.rs::browser_layout_matches_the_baseline`
- `src/core/categories/browser.rs::declared_browser_roots_share_the_chromium_leaf_vocabulary`

Test/fixture addition, not an organic bug fix; no red-run proof claimed. Actual unified
gate results and limitations are recorded in RULES_REFACTOR_STATUS.
