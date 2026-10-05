# Agent Note: Frozen installation artifacts require stable Windows object identity

Status: implemented

## Problem

The new installation-instance regression renamed an observed file and recreated the same path with the same byte count within one second. Existing Windows TargetIdentity uses second-level modification time and length, so the replacement passed verification. A scan-time artifact list alone also failed to prevent newly recognized bootstrap files from widening uninstall scope.

## Decision

AppDiscovery holds the immutable CleanupPlan from source discovery. InstallationInstance captures bounded artifact paths, confirmed absence and identity. Execution compares the live installation instance and artifact list to the scanned scope; it can shrink for shared references or confirmed disappearance, but cannot add paths, accept newly appearing artifacts or change instances. Windows artifacts additionally require volume serial and file index obtained from a read-attributes handle. Link/reparse ancestors and failed identity reads cannot confirm identity. Unix retains existing dev/ino checking.

## Alternatives considered

Changing the test payload length would make the weak Windows check pass without fixing same-size replacement. Sleeping for a timestamp change is timing-dependent. Rebuilding the full authorization at execution would silently widen the user's approved range. Applying timestamp/length equality to Unix would regress active-file protection semantics.

## Consequences

Exact installation artifacts add bounded metadata/handle reads, not a recursive scan. Scan finalization appends shortcut observations before exposing the final Arc; app clones retain that Arc. Supplement cleanup checks frozen startup/shortcut identities immediately before removal, including after registration callbacks. Confirmed disappearance permits recovery; unobserved, replaced or unreadable paths cannot authorize removal. The official uninstall command is no longer rebuilt at execution either — that freeze is owned by `2026-10-05-frozen-official-uninstall-command.md` and its note is the authority for the frozen-command route. The original generic TargetIdentity remains unchanged; its Windows weak-check limitation outside installation artifacts remains a migration concern. Source five-step reporting is connected; official runtime/registration facts and full UI acceptance remain outstanding. No actual Hermes installation was modified.

## Verification

- `src/core/rules/plan.rs::frozen_installation_instances_reject_expansion_appearance_and_replacement`
- `src/platform/windows/app_discovery.rs::discovered_installation_plan_rejects_new_artifacts_and_keeps_snapshot`
- `src/platform/windows/app_discovery.rs::discovered_shortcut_identity_is_frozen_and_missing_is_recoverable`
- `src/platform/windows/source_install.rs::supplement_rechecks_frozen_shortcut_after_registration_callback`
- `src/platform/windows/source_install.rs::supplement_retains_same_content_startup_replacement`

Proved: Before the stable Windows ID implementation, the unchanged same-length replacement assertion failed in the first test; the saved red output is `docs/agent-notes-evidence/rules-installation-identity-red.log`. After the fix, both isolated tests passed. They also pin new-path rejection, absence-to-presence rejection, half-uninstall recovery and frozen plan/snapshot references.

Proved: Temporarily replacing the final startup scanned_identity check with the old absence-only check caused supplement_retains_same_content_startup_replacement to return Ok after deleting a same-content, same-time replacement. Saved red output: `docs/agent-notes-evidence/rules-startup-identity-red.log`. The fixture asserts the old weak identity check accepts the replacement and derives the Gateway signature from the rule. The strong check was restored before final full-suite verification.
