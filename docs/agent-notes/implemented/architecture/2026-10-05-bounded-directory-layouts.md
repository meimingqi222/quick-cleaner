# Agent Note: Bundled directory layouts share bounded discovery and typed scopes

Status: implemented

## Problem

DNS and thumbnail discovery each enumerated the same per-user C/T directories. Log roots, name exclusions and Finder metadata directories lived in separate Rust providers. Moving defaults alone did not make new layouts configurable.

## Decision

The directory_selection capability interprets directories from the bundled immutable snapshot. Typed selectors choose direct children or exact named files with at most one child level. Root choices are home and the trusted user-temp parent. Configuration controls relative layouts, literal name filters, labels and recommendation policy. Runtime discovery shares inventories across selectors. Log extensions and exclusions are configurable; live-database probing and deletion protection remain core mechanisms.

Remove the four provider loops and their four unused defaults. Existing macOS log regression calls a test-only wrapper around the shared capability. The maintenance explain tool invokes the same selector against explicit fixture roots and returns diagnostic CleanupPlans and enumeration counts.

## Alternatives considered

Giving each provider a new list lookup would preserve duplicated enumeration. Adding recursion or scripts would enlarge authority and resource cost. Using Contents for metadata would claim the parent rather than the selected file. These are rejected.

## Consequences

A rule-only fixture adds a different exact-file layout while the previous snapshot retains its original behavior. The initial 13-target fixture, subsequently extended to 16 targets with container leaves, checks target, recommendation, operation and disposal; it does not prove complete migration. DNS and QuickLook require two shared directory reads, not four. Named .DS_Store directories are excluded from the file selector; depth remains one. Inventory failures and limits cannot grant a partial directory inventory. Parent/child normalization, other layout/age migration, UI review and macOS native checks remain tracked by GOAL. No real user resources are cleaned.

## Verification

- `src/core/rules/directories.rs::directory_layouts_preserve_baseline_and_share_reads`
- `src/core/rules/directories.rs::directory_rule_only_extension_keeps_old_snapshot_and_exact_files`
- `src/core/rules/directories.rs::directory_validation_rejects_escape_script_and_unbounded_modes`
- `src/core/rules/directories.rs::incomplete_directory_inventory_never_grants_partial_targets`
- `src/core/rules/directories.rs::directory_links_never_expand_layouts`

This architecture migration does not claim a bug-fix red proof. Actual unified gate results are recorded in RULES_REFACTOR_STATUS.

The already-built rules.exe also interpreted rules/fixtures/directory-layout-extra.toml without a Rust edit and reported one exact-file plan with no blocked reasons. Its diagnostic output is preserved under docs/agent-notes-evidence/2026-10-05-directory-layout-extra-explain.json. This is fixture discovery evidence, not a production external-rule loader or complete application-uninstall proof.

The subsequent schema 6 extension is documented by 2026-10-05-container-layouts-and-artifact-policies.md. Shared directory probes now distinguish confirmed absence from unknown and cap all root/leaf probes, including empty and missing layouts. Container layouts keep unknown contents unselected; a cached directory verdict cannot authorize a replacement file.
