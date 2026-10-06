# Agent Note: Residual scope collapse keeps one entry per physical target

Status: implemented

## Problem

The residual channel (Windows `platform::windows::residuals`, macOS
`platform::macos::residuals`) listed one item per discovery hit with only a shallow
de-dup:

- Windows `dedup_items` compared whole `ResidualKind` values. `ResidualKind::Directory(path, size)`
  and `ResidualKind::File(path, size)` embed the recorded size, so the same path
  re-scanned after the official uninstaller ran (same path, smaller size) was listed
  and weighed twice.
- Neither platform collapsed **parent/child** overlap. A directory item that
  contains another item double-counted the child's bytes in `total_file_size` and
  made cleanup try to delete a path that its own parent had already removed —
  a spurious failure. This is reachable: `scan_install_dir` records the install
  directory while `scan_uninstaller_leftover` records the uninstaller's parent
  directory, which can sit *under* the install directory. Registry keys and their
  values/subkeys have the same shape: deleting the key removes the value, so
  cleaning both fails on the second.

## Decision

A single core helper, `crate::core::apps::dedupe_residuals`, owns the collapse for
both platforms. Two order-independent levels:

1. **Identity de-dup** keyed on the residual identity *without the size*
   (`residual_key`): filesystem paths by normalized path, registry keys/values by
   root + normalized subpath (+ value name), tasks/extensions by id. The
   higher-confidence copy wins.
2. **Ancestor collapse**: a `Directory` item covers every filesystem item strictly
   under it; a `RegistryKey` covers its subkeys and values. The descendant is
   dropped because the ancestor's recorded size already counted it and deleting the
   ancestor removes it.

The ancestor keeps **its own** confidence. A merge never upgrades deletion
authority: a guessed (`Possible`) parent that absorbs a `Certain` child stays
`Possible`, so it cannot become auto-selectable by covering a more certain item.

Windows calls it from `scan_residuals_inner` (replacing `dedup_items`); macOS keeps
its `canonicalize`-based case-insensitive exact de-dup and then calls the same
helper for the parent/child pass.

## Alternatives considered

Comparing whole `ResidualKind` (the old behavior) misses same-path-different-size
duplicates and never merges overlap. Merging by upgrading the ancestor to the
child's confidence would let a name-guessed parent become Certain — a deletion
authority escalation, rejected. Subtracting overlap only from the statistics while
keeping both items would still delete twice. Dropping the ancestor in favor of a
more confident child would leave the rest of the tree uncleaned. All rejected.

## Consequences

A residual list now has one entry per physical target: no double-counted bytes, no
second delete attempt against an already-removed child, and a stable result
regardless of scanner order. Registry keys absorb their values and subkeys. The
behavior is covered by core fixtures; the Windows live scan test still passes.

## Verification

- `src/core/apps.rs::residual_dedup_ignores_recorded_size_and_keeps_confidence`
- `src/core/apps.rs::residual_dedup_collapses_parent_child_overlap_in_any_order`
- `src/core/apps.rs::residual_dedup_never_upgrades_a_guessed_parent`
- `src/core/apps.rs::residual_dedup_collapses_registry_key_over_subkeys_and_values`
- `src/platform/windows/residuals.rs::dedup_keeps_the_higher_confidence_record`

Model migration, not an organic bug fix; no red-run proof is claimed. Actual
unified gate results and limitations are recorded in RULES_REFACTOR_STATUS.
