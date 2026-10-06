# Agent Note: Residual registry anchors move into the rule

Status: implemented

## Problem

The residual channel reads its registry anchors from `residual-windows.toml` (run keys,
App Paths, MuiCache, Installer\Folders, …), so maintaining them is configuration. Two
groups were the last hold-outs: the shell-extension anchors (a hardcoded
`SHELLEX_PARENTS` const — eight `shellex` handler parents plus shell-icon-overlay and
Browser-Helper-Objects — and two hardcoded `Shell Extensions\Approved` strings inside
`scan_shell_extensions`), and the COM `Classes` roots (a hardcoded `(root, base, sam)`
array in `scan_com`). Adding an anchor to either required a Rust change, unlike every
other anchor.

## Decision

- `rules/residual-windows.toml` (version 1 → 3) gains `shell_extension_parents` (the ten
  parent keys), `shell_extension_approved_keys`, and `com_classes_roots` (the two COM
  class roots; the HKCU `Software` form is derived by the scanner).
- `scan_shell_extensions` now reads both shell lists via `residual_list` /
  `residual_anchor`; the HKCU `SOFTWARE → Software` case fix and the GUID-text algorithm
  stay in code. The `SHELLEX_PARENTS` const is removed.
- `scan_com` iterates `com_classes_roots` × views (HKLM 64-bit, HKCU) with the same case
  fix, instead of a hardcoded `(root, base, sam)` array. The effective scanned key set is
  unchanged (the extra HKCU `Classes\WOW6432Node` is a nonexistent key).
- `residual_shared_lists_follow_the_rule` asserts the new lists are non-empty in the
  shipped baseline (an empty anchor list means "scanner disabled", so presence is
  behaviour).

## Alternatives considered

Keeping the anchors in Rust made this one scanner config-inconsistent with the rest.
Deriving them at runtime is impossible — they are Windows conventions. Treating an
empty list as "scan everything" would be unsafe. All rejected.

## Consequences

Every residual scan anchor is now rule-maintained; adding a shell-extension location is
a TOML change. The scanner still implements the algorithm (enumerate subkeys/values,
compare GUIDs to the install registration). No behaviour change on the shipped baseline.

## Verification

- `src/platform/windows/residuals.rs::residual_shared_lists_follow_the_rule`

Configuration migration, not an organic bug fix; no red-run proof claimed. Actual
unified gate results and limitations are recorded in RULES_REFACTOR_STATUS.
