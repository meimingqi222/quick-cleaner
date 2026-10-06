# Agent Note: Rules carry no user-facing version in the UI

Status: implemented

## Problem

The UI presented the cleanup rules as if they had their own release train: the sidebar
footer showed a second line under the app version reading "清理规则 v1 / Cleanup rules v1"
(clickable, expanding into a "rules ship with app updates" explainer), the smart-clean
confirmation labelled the rules that would run as `engine@3`, and the uninstall
confirmation read "规则 hermes r2（规则包 v1）". None of those numbers map to anything a
user can act on: the rules are embedded in the binary by `build.rs`, so the bundle
sequence is a build-time constant, and the per-rule revision is an internal id. Presented
as a product version, it invites exactly the question the remote-channel removal already
answered — "which rules am I on, and can I update them separately?" — and it puts a second
version line under the app version for no information.

## Decision

No rule version is rendered anywhere in the UI:

- the sidebar shows the app version (and the update entry) only; the rules row, its
  expand panel and the two strings behind it are deleted, along with the panel state on
  `Root`;
- the smart-clean confirmation lists the contributing rule **ids**;
- the uninstall confirmation names the rule **id** and its target count.

The internal fields stay, because they are not version display: `RuleBundle::sequence`
is the snapshot identity that `plan.rs` re-checks so a plan and its observation can only
come from the same frozen snapshot (GOAL §3: results, plan, confirmation and execution
bind to one version), and `RuleDefinition::version` still carries per-bundle revision
metadata for rules and fixtures. Neither is a user concept.

## Alternatives considered

Keeping the row but hiding the number behind hover — still implies an independent version
and still costs a line. Dropping `bundle.sequence` from the bundle as well — rejected: it
is the mechanism that rejects a stale observation, and GOAL requires that binding;
removing it would delete a safety check to tidy a display. Showing the app version next to
the rule ids in the confirmation — the sidebar already shows the app version, and the
confirmation's job is to say *which rules* will act, not which build they came from.

## Consequences

The sidebar footer is one line again, and nothing in the UI suggests rules can be updated
apart from the app. The confirmations still name the rules that will act — the audit value
is the rule id — while a user cannot compare or pin a rule version that ships with the
binary. `rules check` and the plan/snapshot machinery keep printing and enforcing the
sequence internally.

## Verification

- `src/ui/state.rs::selected_plan_summary_lists_rules_and_blocked_reasons` (asserts the
  rule ids, the distinct operations and the blocked reasons)
- `scripts/check-ui-localization.py` stays at an empty baseline: the removed strings were
  `tr_` helpers, not inline copy, and no new copy was introduced
- `cargo test --lib` 608 passed / 0 failed, `cargo clippy --all-targets -- -D warnings`
  clean (verified on `--target x86_64-pc-windows-msvc` because the running app held the
  debug binary)

Partly supersedes `2026-10-05-confirm-plan-summary.md`: that note's decision to label
rules as `id@version` is dropped here; its frozen-plan-summary decision still stands.
