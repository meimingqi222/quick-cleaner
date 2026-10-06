# Agent Note: Protected temporary roots support an explicit contents scope

Status: implemented

## Problem

Windows Temp was offered by the legacy provider but its protected root was rejected by both the plan and clean_dir_contents. Migrating it to path rules also filtered it as a protected residual. The root-preserving contents operation was being treated as tree removal, losing an existing target instead of implementing its intended scope.

## Decision

core safety remains the only protection authority. is_protected and is_contents_protected share the same checks; only the explicit Windows Temp contents extent permits retaining the protected root. Whole-root removal, Windows/System32, drive roots, home skeletons, whitelist and managed-session protection remain denied. Path discovery, plan validation and the contents cleaner consume this scope predicate. Child deletions still use all existing safety, identity, live-database, occupancy and link guards.

The rule root context is captured once. user_temp uses the trusted foreground-user resolver; unknown cannot fall back to the process temp directory. drive:<uppercase-letter> selects a bounded native root, with relative child paths. A dot target is allowed only for user_temp + contents without variables. Whole home/system/drive roots and tree removal of the dot target remain invalid.

## Alternatives considered

Skipping all safety checks for contents would expose protected subtrees. A synthetic child-path probe would obscure the authoritative scope. Keeping fixed system paths in the provider would preserve duplicate maintenance. Falling back from an unknown user root could select an administrator's resources. These are rejected.

## Consequences

Eight Windows fixed paths and two macOS fixed paths now use ordinary path rules. The obsolete constructors and two unused provider policies are removed. The path baseline expands to 46 entries; the remaining dynamic baseline has 43 rows. Dynamic SID, content and DNS discovery remain capabilities, with their further layout/age migration tracked by GOAL. Windows scope assertions read only system path names; the execution fixture deletes only a dedicated test sentinel and verifies its root survives. No actual Windows Temp, Hermes or ecosystem resource is cleaned.

## Verification

- `src/core/safety.rs::contents_scope_preserves_self_banned_roots_and_keeps_subtree_protection`
- `src/core/safety.rs::contents_scope_still_protects_whitelist_and_managed_worktrees`
- `src/core/rules/mod.rs::system_rules_keep_temp_scope_and_unknown_user_root_never_falls_back`
- `src/core/rules/mod.rs::root_scope_validation_rejects_skeleton_and_drive_escape`
- `src/core/rules/mod.rs::explicit_path_policies_match_pre_migration_baseline`
- `src/core/rules/mod.rs::provider_policy_defaults_match_migration_baseline`

Proved: before changing the scope predicate, the new API forwarded the existing is_protected check and the unchanged Temp assertion failed with cargo exit 101: "Temp contents must remain a supported scope". Saved evidence: `docs/agent-notes-evidence/2026-10-05-contents-scope-red.log`. The batch green and gates are recorded in RULES_REFACTOR_STATUS; this does not claim full Goal or macOS acceptance.
