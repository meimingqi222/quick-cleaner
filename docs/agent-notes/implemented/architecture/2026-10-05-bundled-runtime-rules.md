# Agent Note: Runtime rules ship with the application and reuse bounded capabilities

Status: implemented

## Problem

Remote rule distribution added signing, channel transport, cache state and scheduling without serving the user's main goal: reducing duplicated implementation through reusable runtime capabilities. The user explicitly removed remote delivery and requested rules bundled with application releases.

## Decision

Production snapshots come exclusively from the validated embedded package. Remove remote downloads, cache activation, signatures and key generation, update settings and the independent rule-release workflow. Keep check, pack and explain tools. Thread-scoped immutable snapshots remain for scans and isolated fixtures. Built-in policies and file layouts are consumed by shared runtime abilities; moving a static list alone does not prove the full refactor is complete. Parent/child scope handling, remaining native entry migration and plan review remain tracked by GOAL.

The superseded remote tests are retained only as historical source evidence outside the compiled tree. They are not runnable client capabilities or ongoing maintenance requirements. No user cache is deleted. Application binary updating remains separate and unchanged.

## Alternatives considered

Disabling remote delivery through an option would retain an unnecessary executable path. Keeping signing tools invites a second release process. Replacing all detection algorithms with expressions would make configuration an unsafe programming language. These approaches do not satisfy the requested architecture.

## Consequences

The shipped client has one rule source and no rule-network state. Existing scans retain immutable snapshots. Further capability and policy migration remains required; removing remote delivery does not itself complete the refactor. Historical channel tests are not counted as current regression coverage.

## Verification

- `src/core/rules/mod.rs::bundled_snapshot_is_stable_and_scoped_rules_do_not_replace_it`
- `src/core/rules/mod.rs::sensitive_name_policy_requires_complete_bounded_lists`
- `src/core/categories/helpers.rs::sensitive_name_policies_follow_runtime_configuration_and_preserve_baseline`

Shared exact/prefix/contains matching consumes snapshot lists for both Apple-cache and Group Container exclusions. The preserved baseline and fixture-only extension verify runtime configuration changes without adding a product-specific branch. Required policy lists cannot disappear or become empty; generic list budgets still apply.

- `src/core/rules/mod.rs::manifest_only_fixture_generates_targets_and_rechecks_membership`
- `src/core/rules/mod.rs::explicit_path_policies_match_pre_migration_baseline`

These existing isolated tests exercise runtime rule interpretation and pinned policies. Batch validation and remaining migration evidence are recorded in RULES_REFACTOR_STATUS; this note does not claim full Goal completion.

Proved: The first unified batch run failed sensitive_name_policy_requires_complete_bounded_lists because a blank name was accepted (502 passed, 1 failed). Saved output: `docs/agent-notes-evidence/2026-10-05-sensitive-name-validation-red.log`. The validation now rejects blank/control/path-containing names without changing the assertion; final batch results are recorded in RULES_REFACTOR_STATUS.
