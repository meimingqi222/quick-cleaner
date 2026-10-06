# Agent Note: Version layouts share discovery and freeze retention facts

Status: implemented

## Problem

Codex and Devin repeated version inventory, current pointer and update-lock selection. Zed retained a dedicated version-cache loop. Configuring names alone left new layouts dependent on Rust branches.

## Decision

Schema 7 declares version_layouts and the version_retention capability. Relative roots, inventory paths, current pointer, lock age, exclusions, prefixes, exact cache leaves and download suffixes come from bundled rules. One bounded Enumeration is shared with directory selectors. Current links must resolve to an immediate ordinary child of the declared inventory; inventory links never claim external trees. Exact archive files use File and version caches use Contents.

Freeze the current verdict and kept name in RuleObservation. Before execution, recheck retention and lock state against the same snapshot. Changed current, active updates, unreadable evidence and invalid lock types block execution. Missing current keeps version targets visible but unselected; archives retain their prior idle-only recommendation. Core scope protection, identity, preservation and occupancy remain mandatory.

## Alternatives considered

Keeping per-application helpers preserves duplicate code. Following arbitrary current paths can authorize external resources. Treating a directory named as a lock as absence loses an unknown-state guard. These approaches are rejected.

## Consequences

Four declared layouts replace production Codex/Devin/Zed loops; old Unix fixtures use test-only wrappers around the same capability. A separate TOML fixture adds a different cache layout without application Rust. This proves generic layout discovery, not complete application uninstall or full migration. Remaining native facts, normalization, UI and macOS acceptance stay in GOAL.

## Verification

- `src/core/rules/versions.rs::version_pointer_retention_and_locks_revalidate_frozen_plans`
- `src/core/rules/versions.rs::invalid_version_pointer_or_lock_never_grants_cleanup`
- `src/core/rules/versions.rs::version_downloads_are_exact_files_and_leaf_layouts_keep_version_parents`
- `src/core/rules/versions.rs::version_rule_only_fixture_uses_shared_discovery_and_pinned_snapshot`
- `src/core/rules/versions.rs::version_layout_validation_rejects_escape_scripts_and_unbounded_parameters`

This architecture migration claims no bug-fix red proof. Actual unified gate results and limitations are recorded in RULES_REFACTOR_STATUS.
