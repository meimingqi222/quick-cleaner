# Agent Note: Declared browser roots reuse one Chromium leaf vocabulary

Status: implemented

## Problem

Three entries carried their own copy of the same Chromium knowledge. The Windows browser entry hard-coded `Default/Cache`, `Default/Code Cache` and a profile-name test (`Profile ` prefix, `Guest Profile`, `System Profile`). The macOS Application Support entry hard-coded its own six-leaf list plus the `Crashpad/completed` split. The content-signature entry in `categories::chromium` read the leaf names from `chromium.toml` but kept the profile-name test in Rust, so the same fact existed three times and drifted: the browser copies only ever listed `Cache` and `Code Cache` leaves, while every other Chromium application already got `GPUCache`, the Dawn/shader caches, the CRX caches and the non-preselected `blob_storage`/`CachedProfilesData`.

Firefox's `Profiles/<profile>/cache2` layout was a fourth copy, with the relative path and the leaf name written directly in Rust.

## Decision

`categories::chromium` gains `declared_leaves(dir)` for roots whose product the rules already declare: no signature detection is needed, and the shared vocabulary, the profile policy and the `Crashpad/completed` rule apply unchanged. The profile-name policy moves into `chromium.toml` (`profile_exact`, `profile_prefixes`, `profile_suffixes`) and is read through the existing `RuleSnapshot::matches_name`, so the content-signature path and every browser entry match profiles by the same list. Whole-bundle validation requires at least one non-empty, name-safe profile mode.

Both browser entries now call `declared_leaves` and label candidates with the leaf trail (`Chrome · Default · GPUCache`, `Chrome · Crashpad · completed`). The Windows entry lost its `Default\\Cache` join, which also removed a latent cross-platform bug: the embedded backslash only worked on Windows.

Schema 8 `directory_selection` gains `named_directories` (the directory counterpart of `named_files`: a declared name inside the declared path or inside each of its children, `child_depth` at most one) and the `{parent}` label token. Firefox's profile caches are now a `macos.toml` entry (`Library/Application Support/Firefox/Profiles` × `cache2`, label `Firefox · {parent} · {name}`), so the relative layout and the leaf name live in configuration and `push_firefox_profile_caches` is gone. The entry keeps `Contents` on `cache2` only: profile data such as `cookies.sqlite` and `startupCache` stays untouched, and a same-named *file* never becomes a directory target.

Both macOS-only browser functions are now `#[cfg(any(target_os = "macos", test))]`, so the layouts are executed by tests on every host instead of being compiled only on macOS.

## Alternatives considered

Keeping per-entry leaf lists preserves three sources of truth and silently narrows Windows coverage. Letting the browser entries call `cache_leaves` (signature-gated) would drop leaves in compacted profiles where the signature threshold is not met, even though the product is declared. Teaching the Windows entry to enumerate every child directory would treat `Local State`-adjacent folders as profiles. Writing Firefox as `Children` on `Profiles` would claim each whole profile — including cookies and logins — as a `Contents` target. These are rejected.

## Consequences

On this Windows host the declared browser roots now yield 24 existing cache leaves where the old code listed 7: the same vocabulary macOS and every other Chromium application already used (Dawn/shader caches, CRX caches, `GPUCache`, `blob_storage`, …). `blob_storage` and `CachedProfilesData` stay listed but unselected; empty cache directories are no longer listed at all. Labels gain the profile segment (`Chrome · Default · Cache` instead of `Chrome 缓存`), which is the shape the development entry already used for Chromium leaves.

Remaining category layouts, native facts, scope normalization, UI review, release closure, performance comparison and macOS native acceptance stay tracked by GOAL.

## Verification

- `src/core/categories/chromium.rs::declared_profile_policy_comes_from_the_rule`
- `src/core/categories/chromium.rs::declared_roots_reuse_the_shared_leaf_vocabulary`
- `src/core/categories/browser.rs::declared_browser_roots_share_the_chromium_leaf_vocabulary`
- `src/core/categories/browser.rs::app_support_caches_follow_the_declared_catalog`
- `src/core/categories/browser.rs::missing_browser_user_data_emits_nothing`
- `src/core/rules/directories.rs::named_directories_stay_precise_and_datadriven`
- `src/core/rules/directories.rs::directory_layouts_preserve_baseline_and_share_reads`

This architecture migration claims no organic bug-fix red proof. The macOS directory gold fixture in `rules/fixtures/directory-layout-baseline.json` now carries the Firefox leaf (17 targets, seven shared inventory reads). Actual unified gate results and limitations are recorded in RULES_REFACTOR_STATUS.
