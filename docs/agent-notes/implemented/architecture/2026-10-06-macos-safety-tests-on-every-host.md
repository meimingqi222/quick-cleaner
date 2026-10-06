# Agent Note: macOS safety predicates are exercised on every host's test build

Status: implemented

## Problem

The macOS safety skeleton (`core/safety.rs`) — protected system prefixes, self-banned
home skeletons, the elevated-residual allowlist, `under_home_app_support` strictness —
was compiled only on macOS, and its seven regression tests carried
`#[cfg(target_os = "macos")]`. On this Windows host (and in the Windows CI job) none of
that logic was ever executed; it could only be checked on macOS, which is the standing
external gap. The logic itself is pure path/suffix matching plus `guards()` state — it
does not call any macOS API.

## Decision

The macOS branches and their fixtures/constants change from `#[cfg(target_os = "macos")]`
to `#[cfg(any(target_os = "macos", test))]`, and the non-macOS stubs from
`not(target_os = "macos")` to `not(any(target_os = "macos", test))`. The seven tests lose
their per-test macOS gate. Production behavior is unchanged on every platform (`test` is
false in production builds; the `any(...)` set equals the old set there); the only
difference is that test builds on any host compile and run the macOS predicates.

## Alternatives considered

Extracting the predicates into a cross-platform module — rejected: they belong to the
safety module's guard state and the extraction would be a larger refactor for the same
result. Renaming the three platform variants of `status.rs`'s icon/label helpers
(`icon_source_path`, `bundle_label`, `process_display_name`) into a test-visible form —
evaluated and declined: it needs a dispatcher plus renames across three cfg variants for
cosmetic icon/label logic, while the safety conversion covers the decision-critical
surface. Leaving the tests macOS-only and waiting for CI — rejected: this host can run
them now, which is exactly what GOAL §6 asks for when macOS is unavailable.

## Consequences

Seven macOS-path assertions now run in every `cargo test --lib` on this host and in the
Windows CI job: the suite grew 600 → 607 with zero regressions. The macOS CI job still
matters (compilation of the full macOS tree, macOS APIs, the trash round-trip), so the
external gap is narrowed, not closed.

## Verification

- `cargo test --lib` on this Windows host: 607 passed / 0 failed / 9 ignored, including
  `protects_macos_system_trees`,
  `macos_home_skeletons_are_self_banned_but_contents_are_free`,
  `library_subtree_stays_protected_for_generic_cleaning`,
  `elevated_residual_allowlist_only_opens_one_level`,
  `elevated_residual_allowlist_never_touches_apple_items`,
  `elevated_residual_allowlist_never_touches_bare_system_components`,
  `under_home_app_support_is_strict`.
- Evidence: docs/agent-notes-evidence/2026-10-06-macos-safety-tests-on-windows-batch.log
  (fmt/build/clippy/rules check/guards all exit 0).

Coverage shift, not a bug fix; no red-run claimed. macOS native acceptance (full-tree
compile and run on a macOS host) remains the external condition recorded in
RULES_REFACTOR_STATUS.
