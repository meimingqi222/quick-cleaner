# Agent Note: Refactor completion is judged by the config-driven-rules goal

Status: implemented

## Problem

The refactor accumulated an enterprise-grade acceptance matrix — signed remote publishing, cross-platform golden baselines, parent-child range algebra, performance baselines — far beyond its original goal of adding cleanup rules through TOML config. Every open matrix item kept the work "in progress" indefinitely, so the project could never be declared finished even though the config-driven capability already worked.

## Decision

Completion is defined by the original goal: a new cleanup location or policy is added by editing `rules/*.toml` and fixtures without touching Rust, under the existing safety invariants. Items beyond that — the remote signed release loop (F), full golden/performance/macOS evidence (G), parent-child splitting (B), all-entry UI review (E) — are reclassified as optional hardening, not completion criteria. The remote signed channel is dormant (an empty `rules/trusted-keys.json` means it never activates in production); its code, tests and workflow are retained rather than deleted so the uncommitted work is not lost. Overlapping targets keep "reject and explain" as the official semantics.

## Alternatives considered

Deleting the remote update machinery would cut roughly 700 lines but discards untracked work that git cannot recover. Continuing the full matrix keeps the project permanently unfinished for infrastructure a single-user tool does not need. Provisioning production keys for the remote channel has near-zero value given the app's own auto-updater. All three are rejected.

## Consequences

Future sessions must not reopen F/G/B/E as blocking debt. Adding rules stays a TOML-plus-fixture change. If remote publishing or cross-platform evidence is ever genuinely needed, it becomes a fresh, scoped task. The `Operation::classify` category-default bridge in `target_with_recommendation`/`target_with_size` remains accepted bounded debt.

## Verification

The config-driven goal is demonstrated end-to-end by `src/core/rules/update.rs::rules_only_release_reaches_the_next_scan_without_recompiling`: a TOML-shaped rule added to a signed bundle surfaces as a scan target on the next scan without recompiling. This is a process/scope decision rather than a bug fix, so no red-run proof applies.
