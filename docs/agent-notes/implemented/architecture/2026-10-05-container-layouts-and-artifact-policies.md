# Agent Note: Container layouts and artifact policies use bounded runtime discovery

Status: implemented

## Problem

Group Containers retained three dedicated paths and constructors. Updater signatures and age, IDE roots and login-item age were still fixed in Rust. Directory entry limits did not bound empty or missing directory probes, and a missing inventory was indistinguishable from unknown in diagnostics.

## Decision

Schema 6 adds container_directories to shared selection, sourcing roots, leaves and name exclusions from configuration. The capability selects only declared leaf directories; a container location cannot prove regenerable data and never enables automatic recommendation. Recheck type before emission so cached directory metadata cannot authorize a replacement file.

Directory sessions share Confirmed, Absent and Unknown probes and bounded inventories. A hard 512-directory probe cap includes anchors, empty and missing roots, and container leaves. Entry and candidate-check budgets cap repeated configuration as well as large inventories. Unknown remains explained and never becomes an empty deletion grant.

Updater matching keeps file/directory distinctions, fixed algorithmic routes and unchanged default seven-day age. Names, literal suffixes and presentation suffixes are configured in cache.toml. Required lists, capability declarations and bounded nonzero ages are validated wholesale. IDE and login roots and the login age are configured in macos.toml; product-version and plist algorithms remain code.

## Alternatives considered

Adding three list lookups inside the old container loop preserves repeated constructors. Treating empty/missing scopes as free probes permits unbounded metadata work. A boolean layout recommendation for group data would replace evidence with configuration. These are rejected.

## Consequences

The directory gold fixture expands to 16 targets, and remaining dynamic defaults shrink from 38 to 35 rows. Actual group targets all remain unselected, including temporary paths whose old engine policy was false. Password-family exclusions stay case-insensitive. New container leaves and updater signatures need configuration rather than application-specific branches. No real user resource is cleaned. Development version layouts, full scope normalization, native facts, UI and macOS acceptance remain tracked by GOAL.

## Verification

- `src/core/rules/directories.rs::directory_layouts_preserve_baseline_and_share_reads`
- `src/core/rules/directories.rs::directory_probe_budget_covers_empty_missing_and_duplicate_roots`
- `src/core/rules/directories.rs::unknown_inventory_and_container_scope_remain_explainable_and_bounded`
- `src/core/rules/directories.rs::container_paths_follow_configuration_without_claiming_parent_or_recommending_unknown_contents`
- `src/core/rules/directories.rs::cached_directory_probe_does_not_authorize_a_replacement_file`
- `src/core/categories/updater.rs::updater_rules_change_typed_signatures_age_and_labels_without_application_branches`
- `src/core/rules/mod.rs::layout_and_artifact_policies_reject_missing_unsafe_or_unbounded_values`

This architecture migration does not claim an organic bug-fix red run. Final batch gates and limitations are recorded in RULES_REFACTOR_STATUS.
