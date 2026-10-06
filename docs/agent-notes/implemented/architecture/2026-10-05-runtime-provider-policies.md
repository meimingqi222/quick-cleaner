# Agent Note: Dynamic providers retain configured policy and typed discovery extent

Status: implemented

## Problem

Dynamic providers still inferred cleanup operations and disposal from presentation categories. Their compatibility constructors parsed virtual display paths to recover native parameters and did not explain which policy produced the recommendation. Static path rules were explicit, but this separate route retained duplicated defaults.

## Decision

Provider constructors require a typed operation and a rule/policy reference. Discovery fixes the resource extent and native parameters; the bundled snapshot supplies recommendation permission and disposal. Recommendation is the intersection of configured permission and discovery evidence. Existing content, age, current-version, live-database and worktree protection checks remain. A policy cannot expand a discovery extent or enable arbitrary commands.

Schema 3 validates bounded provider policies and requires the profiles used by built-in providers. Missing policy references and disposal/operation conflicts block a target. Frozen rule observations include the policy name and values; a later snapshot cannot change an existing plan. Docker, snapshots and Homebrew construct their typed resource directly. Worktree discovery retains the exact inspected registration and skips an unavailable inspection. Legacy category operation/disposal functions and URI classification are compiled only for old-baseline tests. Typed validation directly checks file extent, ecosystem routes and exact registration; empty/remove helpers never infer a native command from a display URI.

## Alternatives considered

Replacing category inference with a category-keyed configuration lookup would retain category as authorization. Parsing a display URI in a generic constructor would retain presentation as command input. Putting all recommendation logic into a boolean rule would bypass discovery evidence. These were rejected.

## Consequences

Dynamic recommendation and compatible disposal changes now require configuration changes rather than provider code. The migration fixture initially covered 53 constructor rows. Ten fixed targets subsequently moved to the 46-entry path baseline, leaving 43 dynamic rows. Five directory-selector constructor rows then moved to the directory-layout fixture, leaving 38 dynamic rows. Three container paths then moved to shared directory selection, leaving 35 dynamic rows; the defaults fixture still does not prove all cross-platform discovery behavior. Runtime fixture scans exercise package cache and obsolete extensions; native construction uses isolated typed parameters. Remaining layout, age and probe migration, parent/child normalization, application entrances and UI acceptance remain tracked by GOAL. No real Hermes, worktree or ecosystem resource was cleaned.

## Verification

- `src/core/categories/mod.rs::runtime_provider_policy_changes_discovery_and_keeps_frozen_extent`
- `src/core/categories/mod.rs::provider_extent_and_parameters_ignore_category_and_display_uri`
- `src/core/rules/mod.rs::provider_policy_defaults_match_migration_baseline`
- `src/core/rules/mod.rs::provider_policy_validation_rejects_missing_unbounded_and_script_fields`
- `src/core/rules/plan.rs::typed_filesystem_guards_reject_owner_bypass_and_unknown_worktree`
- `src/core/categories/dev.rs::owned_agent_session_worktrees_are_filtered_by_core_safety`
- `src/core/categories/cache.rs::home_cache_chromium_profile_yields_leaves_not_the_root`

This architecture migration does not claim a bug-fix red-run proof. Batch results are recorded in RULES_REFACTOR_STATUS; Windows tests do not prove macOS native acceptance.
