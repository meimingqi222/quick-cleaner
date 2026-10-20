# Agent Note: Orphan residuals match exact Bundle IDs only

Status: implemented

## Problem

Orphan residuals are support files left by already-uninstalled applications. Looser criteria — vendor-family prefixes (com.google matches both Chrome and its keystone agents), name similarity, or "the probe could not tell" — claim files that still belong to live applications and their helpers, deleting working software's support files. The workspace pitfalls list records this as a user-stepped regression with a non-obvious root cause.

## Decision

`is_orphan_id` matches only the exact Bundle ID of an application whose owners are gone. Vendor-family criteria and "undetectable" fallbacks never widen the rule, in either direction. The scan gate is all-or-nothing: `orphan_owners_all_gone` requires every owner to be absent, and an unknown or reappeared owner refuses the whole orphan result instead of deleting the confirmable subset.

2026-10-16 audit: Spotlight misses nested apps and developer browsers. Before scan classification
and again before cleanup, supplement the live owner set using NSWorkspace and a bounded
LaunchServices registration dump, then verify the actual registered path's Info.plist.
Stale missing paths are not installation evidence; unreadable or mismatched present identities
refuse the batch. Queries include owner ancestors, preserving the existing family protections.
Only the canonical current-user Trash root counts as a removed location; symlinks resolving
outside it protect a live owner. No arbitrary directory named .Trash grants removal evidence.

A real trashed application's exact CFBundleIdentifier can make its matching residual Certain,
subject to the live-owner exclusion first. It cannot upgrade sibling products or SDK namespaces.
Directory-name shape alone remains Possible: no catalog lookup or disappearance assumption
proves those owners. Generic/country-code namespaces and the existing two-part mixed-case
rule conservatively reject ordinary rotated logs and compiler caches. Unsupported namespaces
may be omitted; this is a deliberate false-negative tradeoff, never deletion authorization.

All orphan candidates require explicit selection independently of confidence. The bilingual
modal calls the list candidates and explains evidence instead of asserting every item belongs
to an uninstalled app. Exact containers, signed App Groups and embedded helper identities
are Certain in per-app scans; containers/scripts and retained Qingjian data require manual
selection. Name/family matches and package receipts without an exact package identity stay
Possible. Exact system-extension identities are Certain but still require manual action.

## Alternatives considered

Family-prefix matching over-claims shared vendor prefixes (`com.docker.docker` vs `com.docker.vmnetd`, `com.google.keystone.agent`). Per-item deletion when some owners are unknown — an unknown owner is not an absent one.

Promoting every syntactically valid ID after a Spotlight miss confuses absence of an index
entry with proof of an app identity. Using Possible to suppress selection misstates ownership.
Both are rejected. LaunchServices historical paths alone never upgrade confidence: the package
must still physically exist and its exact identity must be readable. Live owners take priority
even when a trashed copy exists.

## Consequences

Orphan scanning is conservative: one live or unknown owner suppresses the result. The bound tests pin real lookalike IDs (helpers, GPU agents, satellite bundles) as protected and exact orphan IDs as matched.

## Verification

- `src/platform/macos/residuals.rs::orphan_id_rule_keeps_live_owners_and_their_families`
- `src/platform/macos/residuals.rs::orphan_scan_finds_removed_apps_and_skips_live_ones`
- `src/platform/macos/residuals.rs::orphan_gate_refuses_when_any_owner_is_back_or_unknown`

Proved: organic red — the motivating over-claims were observed on the real machine per the workspace pitfalls list (recorded before this note existed); the bound tests reproduce the family-prefix lookalikes, the live-owner scan, and the unknown-owner gate refusal.

Additional regression bindings (2026-10-16):

- `src/platform/macos/residuals.rs::orphan_names_reject_rotated_logs_and_compiler_caches`
- `src/platform/macos/residuals.rs::orphan_evidence_upgrades_exact_removed_owner_without_preselection`
- `src/platform/macos/orphan_evidence.rs::registered_live_bundle_blocks_orphans_and_unknown_refuses`
- `src/platform/macos/orphan_evidence.rs::registration_records_keep_exact_paths_and_reject_unknown_output`
- `src/platform/macos/residuals.rs::scan_finds_embedded_helpers_scripts_recent_items_and_receipts`
- `src/core/apps.rs::residual_confirmation_is_independent_of_confidence_and_survives_dedup`

Proved: natural red for rotated logs: warp.log.old.0 parsed as Some instead of None in
`docs/agent-notes-evidence/2026-10-16-orphan-names-red.log`; restored focused scan green in
`docs/agent-notes-evidence/2026-10-16-orphan-names-green.log`.
Manufactured red cases each remove one guard and restore/re-run the same focused test:
live identity insertion (`docs/agent-notes-evidence/2026-10-16-registered-owner-red.log` and
`docs/agent-notes-evidence/2026-10-16-registered-owner-green.log`), exact removed-owner confidence
(`docs/agent-notes-evidence/2026-10-16-orphan-confidence-red.log` and
`docs/agent-notes-evidence/2026-10-16-orphan-confidence-green.log`), manual-selection exclusion
(`docs/agent-notes-evidence/2026-10-16-manual-selection-red.log` and
`docs/agent-notes-evidence/2026-10-16-manual-selection-green.log`).

Read-only native audit of all 48 original items: 13 false positives suppressed, Ghostty WebKit
upgraded to Certain using its actual trashed bundle identity, 34 remain Possible without an
independent app identity. No real residuals deleted. The host-specific row table is local in
docs/temp/2026-10-16-residual-confidence-audit.md; production behavior does not depend on it.

Final macOS validation (2026-10-16): cargo fmt --check, cargo build, strict all-target clippy,
rules check (schema 11, 13 rules), strict notes and UI localization/component guards pass.
Full library suite: 816 passed, 0 failed, 12 ignored. Final native read-only re-scan confirms
35 candidates (1 Certain, 34 Possible); all remain manually selected. No Windows-native
execution is claimed; its struct literals retain the original selection behavior.
P19 constraint merge, P28/P37 per-object identity, P31/P60 unknown-owner refusal and
P46 bounded process execution were specifically rechecked; Qingjian P49/P50 data
preservation and the earlier P65 Homebrew changes remain intact.
