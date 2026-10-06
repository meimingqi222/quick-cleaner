# Agent Note: Windows residuals reference the residual-windows rule, not a default engine tag

Status: implemented

## Problem

The Windows residual channel bound every scanned item to the app's discovery rule when
one existed, and fell back to a bare `RuleRef::engine()` otherwise — an opaque default
label with no rule behind the scan. The macOS side already referenced a real
`residual-macos` rule that carries the channel's shared name facts. GOAL requires source
observations to be explainable; a default engine tag is not a complete source.

## Decision

A new `rules/residual-windows.toml` (id `residual-windows`, platform `windows`,
version 1, required `file` + `registration`) now carries the residual channel's shared
facts as lists: the `Run`/`RunOnce` scan anchors, the AppCompatFlags key anchors, and
the generic-folder exclusion names. The scanner reads them through the rule snapshot
(`residual_list`) instead of Rust constants, so adding an anchor or an excluded folder
name is a TOML edit. The vendor-key reverse lookup, value-text matching and the
identity/completion gates remain code capabilities.

`scan_residuals` binds non-discovered apps to `RuleRef::new("residual-windows", None).observed()`
— a real rule reference with the frozen version/schema/sequence observation, mirroring
the macOS `residual-macos` pattern; discovered apps keep their own rule reference.

## Alternatives considered

Keeping the concurrent `engine().observed()` fallback — rejected: the observation
captured an unrelated rule's version, and the channel's shared lists would stay hidden
in Rust. Migrating every residual scanner (vendor-key lookup, MUI cache, COM, tasks) to
declarative selectors — deferred: those are registry-walking algorithms the GOAL keeps
as code; only shared name/anchor facts belong in the rule. Weakening the
`generic_folder_names` semantics — rejected, the exclusion fence is pinned by tests.

## Consequences

Windows residual items now explain themselves as `residual-windows@1` with a captured
observation; the channel's anchors and exclusions are configuration. The rule ships
embedded with the app (rules check now reports 13 rules) and is covered by the same
immutable-snapshot guarantees as every other rule.

## Verification

- `src/platform/windows/residuals.rs::residual_scan_binds_the_residual_windows_rule`
- `src/platform/windows/residuals.rs::residual_shared_lists_follow_the_rule`
- `src/platform/windows/residuals.rs::registered_app_residuals_carry_a_complete_source_observation`

Model/migration, not an organic bug fix; no red-run proof claimed. Windows-only
evidence; macOS native acceptance remains an external condition. Actual unified gate
results are recorded in RULES_REFACTOR_STATUS.
