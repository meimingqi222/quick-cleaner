# Agent Note: Scan results retain plans and ownership observations through confirmation

Status: implemented

## Problem

Constructing a plan inside the cleaner captured ownership only after confirmation. Deduplicating selected paths could lose a second observation, and recomputing preservation could forget a restriction present at scan time.

## Decision

Fixed and discovered scanner outputs freeze an Arc-backed plan before returning to the UI. Plans capture pinned rule observations, variable resolutions, evidence, target operations, disposal and identity. Confirmation copies the same references. Core deduplication retains all contributing plan references and checks each selection binding. Native binding uses typed resource keys so display aliases do not change resource identity. Frozen and live preservation both apply, and unknown scan evidence blocks execution.

## Alternatives considered

Recreating a plan at confirmation or execution loses the earlier evidence. Keeping just the first duplicate observation hides unknown evidence. Applying only current preservation allows an update in the underlying manifest to broaden the confirmed selection. These approaches are rejected.

## Consequences

The explanation is a diagnostic export, not a deserializable grant of authority. Non-scanner callers still have a legacy plan bridge; application and residual entrances, step dependencies, capability completion, overlap normalization and UI explanations remain tracked in GOAL and the status file. This change is not full refactor acceptance.

## Verification

- `src/core/rules/mod.rs::manifest_only_fixture_generates_targets_and_rechecks_membership`
- `src/core/rules/plan.rs::frozen_and_live_preservation_both_block_manifest_targets`
- `src/core/cleaner.rs::duplicate_scanned_plans_keep_unknown_observations_and_selection_binding`
- `src/ui/state.rs::selected_targets_preserve_scanned_operation_even_when_category_differs`

The full Windows library suite passed with 469 tests and 9 ignored. Fixtures verify changing manifest facts, both directions of preserve changes, duplicate order and unchanged Arc references. No actual local Hermes or ecosystem cleanup was executed. This architecture record does not claim a bug-fix red-run proof or macOS acceptance.
