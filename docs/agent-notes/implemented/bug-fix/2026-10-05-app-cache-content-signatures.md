# Agent Note: App caches are recognized by content signature; the app name only classifies

Status: implemented

## Problem

Application caches identified from the app's display name misclassify lookalike directories and over-claim: a Chromium profile root is not a cache, a freshly downloaded update package is not a stale cache, and one mixed directory can contain both cache and non-cache children. Deleting on name evidence destroys profiles and current-version data.

## Decision

Cache candidates are confirmed by content signature; the app name only decides which application a confirmed cache is attributed to. Hard rules the tests pin: a Chromium profile home yields its cache leaves, never the profile root; a fresh update package is not preselected; a mixed cache directory is split by content instead of deleted whole; library cache claims follow the rules that emitted them; Apple-owned cache directories are never probed.

## Alternatives considered

Name-based matching — the bug. Whole-root deletion "because most of it is cache" — breaks profile state that the hard rules exist to protect.

## Consequences

Content probing costs bounded reads per candidate. New cache sources join through the same signature path; adding a name-only shortcut for a specific application is the regression this note locks out.

## Verification

- `src/core/categories/cache.rs::mixed_cache_dir_is_split_by_content`
- `src/core/categories/cache.rs::fresh_update_package_is_not_preselected`
- `src/core/categories/cache.rs::home_cache_chromium_profile_yields_leaves_not_the_root`
- `src/core/categories/cache.rs::library_cache_claims_follow_the_rules_that_emit_them`

Proved: organic red — the motivating misclassifications were observed on the real machine per the workspace pitfalls list (recorded before this note existed); the bound tests reproduce the profile-root, update-package, mixed-directory and claim-tracking cases in isolated fixtures.
