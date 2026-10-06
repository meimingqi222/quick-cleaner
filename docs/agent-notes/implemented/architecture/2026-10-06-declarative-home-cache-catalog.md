# Agent Note: Declarative home-cache catalog dispatch and configured leaf recommendation

Status: implemented

## Problem

The `~/.cache` walk was the last cache entrance with hardcoded layout knowledge in Rust: the `.cache` root, the agents/packages/shown catalog dispatch with their categories and recommendation, the fallback shape, and the two unrecommended Chromium leaf names (`CachedProfilesData`, `blob_storage`). Adding a catalog or changing a recommendation required a Rust change even though the catalog rows themselves already lived in `cache.toml`. The `com.apple.` updater-probe exclusion in the `~/Library/Caches` walk was likewise a hardcoded application-family prefix.

## Decision

Schema 9 adds the `catalog_children` directory selection: a declared root (`home/.cache`) whose children are dispatched by declared catalogs, each with its own category and recommendation; unmatched children fall back to the directory rule's own shape (shown, not preselected); an optional leaf policy selects a declared leaf-signature capability (`chromium`) whose vocabulary and `shown_leaves` recommendation stay in `chromium.toml`. Leaf labels use `{name}`/`{trail}` tokens. Validation rejects unknown catalogs, invalid categories or signatures, `{trail}` in fallback labels, static fallback labels, duplicate paths across dispatched catalogs (map-order-dependent plans), wrong operations, and unknown select fields. The whole enumeration runs through the shared bounded `Enumeration` (4096/16384/512 budgets, link skipping, confinement, safety checks at emit), replacing the unbounded `read_dir` loop. The updater-probe exclusion prefixes moved to `cache.toml` (`updater_probe_exclude_prefixes`) and are validated like the other updater lists.

`engine.package_cache` lost its last production user and was removed from `engine.toml`, from the required-policy list and from the provider-policy baseline fixture (32 → 31 rows); recommendation for package caches now follows the cache rule's declared catalog policy, so engine provider-policy flips no longer affect migrated cache layouts (same pattern as the earlier manifest-layout migration). The Chromium leaf recommendation is data-driven (`shown_leaves`), with behavior identical to the previous hardcoded pair.

## Alternatives considered

Keeping the dispatch in Rust and only moving row data — rejected: adding a catalog would still need a Rust change. Referencing engine provider policies from the catalog policy — rejected: it would add a cross-rule policy reference for a static layout; directory rules carry their own recommendation like every other declarative layout, and the shipped bundle is static so user-visible behavior is unchanged. Making the `~/Library/Caches` walk fully declarative — deferred: its partial-claim, updater-signature and residual-children semantics are content-signature algorithms the GOAL keeps in code; only the application-family probe exclusion was configuration.

## Consequences

A new rebuildable `~/.cache` cache is now one TOML row (proven by `rules/fixtures/cache-catalog-extra.toml`: the compiled rules.exe emits a `PackageCache` plan for `cache-tool` with no Rust change). Leaf preselection tuning is a `chromium.toml` edit. The migration baseline `rules/fixtures/cache-catalog-baseline.json` pins the pre-migration target set (7 targets: catalog rows, fallback, Chromium leaves with trail labels, loose file and empty-leaf exclusions) against the production selector. `cache`/`chromium`/`engine` rules are version 3/3/6; schema 9.

## Verification

- `src/core/categories/cache.rs::home_cache_catalog_children_preserve_the_migration_baseline`
- `src/core/categories/cache.rs::catalog_children_policy_and_leaf_recommendation_follow_the_rule`
- `src/core/categories/cache.rs::rebuildable_home_cache_dirs_are_package_cache`
- `src/core/categories/cache.rs::home_cache_chromium_profile_yields_leaves_not_the_root`
- `src/core/categories/cache.rs::library_cache_claims_follow_the_rules_that_emit_them`
- `src/core/rules/directories.rs::catalog_children_reject_undeclared_unbounded_and_ambiguous_dispatch`
- `src/core/rules/mod.rs::provider_policy_defaults_match_migration_baseline`
- `src/core/categories/mod.rs::runtime_provider_policy_changes_discovery_and_keeps_frozen_extent`

This architecture migration does not claim a bug-fix red-run proof; the committed baseline fixture carries the pre/post equivalence. Windows-only evidence; macOS native acceptance is tracked separately by GOAL. No real Hermes, worktree or ecosystem resource was cleaned.
