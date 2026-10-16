# Agent Note: Embedded uninstaller paths use path declarations, not name lists

Status: implemented

## Problem

CI run 38902146798 failed both native test jobs on Invalid residual matching policy.
The new Zoom path in `rules/residual-macos.toml` was stored in lists, whose values
are names and explicitly reject slashes. TOML syntax parsing and compilation pass,
but constructing the bundled runtime snapshot fails, affecting every consumer.
The separate Windows descendant-readiness failure is not caused by this policy error.

## Decision

Move the declaration to the existing locations structure, whose relative paths are
validated and bounded by the runtime bundle validator. `src/platform/macos/apps.rs`
reads the location both for discovery and the required-entry no-fallback gate.
Source Other is inert here: this key is not a residual candidate or removal target.
No validator, ownership exclusion, exact-ID match or confinement check is weakened.

The Zoom fixture also includes an unrelated Resources uninstaller: when the declared
entry disappears, discovery must still refuse generic fallback. This keeps both
consumers on the same declaration after its move.

## Alternatives considered

Allowing slashes in name lists would weaken every matching policy. Special-casing
one path-list key adds a second path language when locations already validates
exactly this data. Removing the Zoom declaration would lose the requested route.

## Consequences

Runtime snapshot initialization is valid again; Zoom's declared entry remains
mandatory and confined. Syntax-only TOML checks are insufficient: the source/bundle
runtime-policy regression must pass on both native CI jobs before release.

## Verification

- `src/core/rules/mod.rs::embedded_and_source_rules_have_identical_policy`
- `src/platform/macos/apps.rs::zoom_discovers_only_the_declared_confined_uninstaller`

Proved: both Windows and macOS source/bundle tests failed with Invalid residual
matching policy before the fix; the Zoom discovery test also failed at snapshot
initialization. Real CI excerpts are in
`docs/agent-notes-evidence/2026-10-16-uninstaller-path-policy-red.log`.
Run 38905831194 then passed source/bundle validation on both platforms and Zoom
discovery on macOS (811 native tests passed). Evidence:
`docs/agent-notes-evidence/2026-10-16-uninstaller-path-policy-green.log`.
Windows overall was still red on the independent descendant-readiness test; this
does not claim all CI passed. TOML parsing or build success alone is not acceptance.

Pitfalls checked: P19/P25/P28/P31/P37 ownership and identity are unchanged;
P36 configuration structure and runtime validation are preserved; P49/P50 official
entry failures still cannot grant direct deletion. The full pitfalls list was read.
