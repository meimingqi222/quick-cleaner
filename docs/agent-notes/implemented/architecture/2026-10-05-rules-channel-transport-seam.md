# Agent Note: Rule channel update logic is transport-injectable for end-to-end proof

Status: implemented

## Problem

`check_and_update` hardwired the trusted-key registry, the user cache root, the GitHub channel URL and the HTTP transport, so the acceptance requirement "only change rules → publish a package → the compiled client adopts it on the next scan" had no executable proof. Every update matrix test exercised `verify`/`install_at` pieces, but nothing walked a signed channel manifest into a staged bundle and into scan targets.

## Decision

The fetch/verify/stage pipeline moved into `install_channel(channel, root, keys, transport)`, which authenticates the signed manifest before accepting its sequence-derived package URL, honors the replay watermark (`Ok(None)` when not newer), and returns the staged `RuleBundle` without activating it. The production `check_and_update` is now a thin shell holding the lock, provisioning gate, real channel and `activate`; the GitHub `rules-v{seq}` package URL shape is unchanged. Tests inject an in-memory transport and fixture keys, then run the same signed bundle through `load_selected` and a pinned snapshot to observe a new rule's scan target.

## Alternatives considered

Serving a local HTTP stub still depends on the production URL derivation and port binding, and cannot inject a foreign key id without production changes. Overriding `cache_root` via environment would let a test accidentally write to the real user cache. Testing `install_at` directly proves staging but not channel authentication order or watermark behavior. These approaches are rejected.

## Consequences

`install_channel` is the single place where an authenticated channel may choose a package URL; callers that skip its sequence check bypass replay protection, so the function keeps the watermark gate inside rather than trusting callers. The production activation path still applies only after a durable staged install. Settings/scheduling call-chain verification, the full update failure matrix documentation and production key provisioning remain open in the GOAL matrix; this change is not full refactor acceptance.

## Verification

- `src/core/rules/update.rs::rules_only_release_reaches_the_next_scan_without_recompiling`

The test edits a TOML-shaped rule inside the signed bundle, publishes manifest/signature/package through an in-memory transport, installs into an isolated cache root, confirms a same-sequence re-poll returns `Ok(None)` without lowering the watermark, and shows `append_path_targets` under the loaded snapshot surfacing the new rule's target with its declared operation and recommendation. A tampered accepted package falls back to embedded rules via the `load_cached` chain. The full Windows library suite passed with 507 tests and 9 ignored; strict clippy, `cargo build`, `cargo fmt --check` and `git diff --check` passed. No real channel, key or user cache was touched.
