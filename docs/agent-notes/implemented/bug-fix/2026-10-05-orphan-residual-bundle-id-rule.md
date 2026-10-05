# Agent Note: Orphan residuals match exact Bundle IDs only

Status: implemented

## Problem

Orphan residuals are support files left by already-uninstalled applications. Looser criteria — vendor-family prefixes (com.google matches both Chrome and its keystone agents), name similarity, or "the probe could not tell" — claim files that still belong to live applications and their helpers, deleting working software's support files. The workspace pitfalls list records this as a user-stepped regression with a non-obvious root cause.

## Decision

`is_orphan_id` matches only the exact Bundle ID of an application whose owners are gone. Vendor-family criteria and "undetectable" fallbacks never widen the rule, in either direction. The scan gate is all-or-nothing: `orphan_owners_all_gone` requires every owner to be absent, and an unknown or reappeared owner refuses the whole orphan result instead of deleting the confirmable subset.

## Alternatives considered

Family-prefix matching over-claims shared vendor prefixes (`com.docker.docker` vs `com.docker.vmnetd`, `com.google.keystone.agent`). Per-item deletion when some owners are unknown — an unknown owner is not an absent one.

## Consequences

Orphan scanning is conservative: one live or unknown owner suppresses the result. The bound tests pin real lookalike IDs (helpers, GPU agents, satellite bundles) as protected and exact orphan IDs as matched.

## Verification

- `src/platform/macos/residuals.rs::orphan_id_rule_keeps_live_owners_and_their_families`
- `src/platform/macos/residuals.rs::orphan_scan_finds_removed_apps_and_skips_live_ones`
- `src/platform/macos/residuals.rs::orphan_gate_refuses_when_any_owner_is_back_or_unknown`

Proved: organic red — the motivating over-claims were observed on the real machine per the workspace pitfalls list (recorded before this note existed); the bound tests reproduce the family-prefix lookalikes, the live-owner scan, and the unknown-owner gate refusal.
