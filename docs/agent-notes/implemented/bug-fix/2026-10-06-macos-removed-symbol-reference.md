# Agent Note: macOS-gated test referenced a removed function (cfg-hidden break)

Status: implemented

## Problem

The refactor moved macOS Group Container cache selection into the declarative
`container_directories` capability and **deleted** `macos::push_group_container_caches`.
A macOS-gated test (`core::categories::mod.rs`, `#[cfg(target_os = "macos")]`) still
imported and called that function. Because the test is `target_os = "macos"`, the
**Windows build never compiled it**, so the broken reference went unnoticed by every
local gate — but on macOS the crate fails to compile (`unresolved import
super::macos::push_group_container_caches`), which is exactly the "not skipping native
branches" risk GOAL §4-G calls out.

## Decision

- Removed the stale `use super::macos::push_group_container_caches;` and the
  group-container half of the test; the Group Container behaviour is now covered by the
  rule-driven `container_directories` capability tests in `rules::directories`.
- Kept the `~/Library/Caches` half (`cache::push_user_cache_dirs`) and made the test
  **test-visible** (`#[test]`, renamed `unknown_user_cache_dirs_require_manual_selection`),
  so it compiles and runs on Windows — that is what catches a re-introduced stale
  reference here.
- Audited the whole tree for the same class of bug: for every function removed since the
  last pushed commit, none is now called (`rg` over `src`), so this was the only
  cfg-hidden dangling reference.

## Alternatives considered

Leaving the test macOS-gated keeps the build broken on macOS and invisible on Windows.
Re-adding a stub `push_group_container_caches` would resurrect a second source of truth
the refactor deliberately removed. Removing the whole test would drop the still-valid
`~/Library/Caches` assertions. All rejected.

## Consequences

The macOS build no longer breaks on this reference (it would have failed the macOS CI
job). The `~/Library/Caches` assertions now run on Windows too. Group Container coverage
stays with the rule capability tests.

## Verification

- `src/core/categories/mod.rs::unknown_user_cache_dirs_require_manual_selection`
- `src/core/rules/directories.rs` container-directory capability tests (Group Container
  coverage)

Proved: re-adding the stale `use super::macos::push_group_container_caches;` makes
`cargo test --lib --no-run` fail with `error[E0432]: unresolved import
super::macos::push_group_container_caches` (exit 101) — evidence
`docs/agent-notes-evidence/2026-10-06-macos-removed-symbol-red.log`. Restoring the fix
makes the focused run green — `docs/agent-notes-evidence/2026-10-06-macos-removed-symbol-green.log`.
The red run reproduces the macOS-gated failure path on Windows (the reference is now in
test-visible code); it is a manufactured proof, not the original macOS red.

Actual unified gate results and limitations are recorded in RULES_REFACTOR_STATUS.
