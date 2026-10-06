# Agent Note: Scan operations are explicit and native resource parameters are typed

Status: implemented

## Problem

Categories and virtual display paths were still deciding cleanup at confirmation and execution time. A nominal typed operation had no native parameters; the cleaner parsed the display URI again. Path rules could not independently specify scope and disposal.

## Decision

Schema 2 requires operation and disposal on path entries. The schema source generates both the embedded package version and the client constant. Scan targets and results carry these values to confirmation without category inference. Docker references and snapshot names are fields of typed operations; native deduplication uses those parameters. Generic filesystem deletion refuses virtual URIs. Native parameter validation remains bounded and shared with command execution. File detection narrows a scanned path to an exact file and never grants its parent.

## Alternatives considered

Keeping category defaults at confirmation would preserve hidden execution policy. Parsing URI parameters after confirmation would keep presentation as authority. Accepting old packages with missing fields would make behavior depend on client defaults. All were rejected. The subsequent dynamic constructor migration is owned by `2026-10-05-runtime-provider-policies.md`; it does not prove the entire refactor complete.

## Consequences

Existing production path entries are migrated with a frozen 36-entry baseline for target, recommendation, operation, disposal, age and preservation. Rule versions increase for migrated production files. Schema 1 packages are rejected wholesale. No production key is generated and no real ecosystem cleanup is used for tests.

## Verification

- `src/core/rules/mod.rs::explicit_path_policies_match_pre_migration_baseline`
- `src/core/rules/mod.rs::path_policy_requires_explicit_fields_and_compatible_capabilities`
- `src/core/rules/mod.rs::manifest_only_fixture_generates_targets_and_rechecks_membership`
- `src/core/rules/plan.rs::native_parameters_and_keys_do_not_follow_display_uris`
- `src/core/cleaner.rs::native_resource_dedupe_uses_parameters_and_filesystem_helpers_never_dispatch`
- `src/ui/state.rs::selected_targets_preserve_scanned_operation_even_when_category_differs`

The full Windows library suite passed with 466 tests and 9 ignored. This architecture record does not claim a bug-fix red-run proof or complete cross-platform acceptance.
