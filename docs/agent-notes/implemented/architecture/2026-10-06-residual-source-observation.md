# Agent Note: Registered-app residuals carry a complete source observation

Status: implemented

## Problem

GOAL item 2 says the residual/native entries must not treat the default engine label as
a complete source observation. `scan_residuals` assigned every residual item the app's
discovery rule, or — for a registered app with no rule — a bare `RuleRef::engine()`,
whose `observation` is `None`. So the "source" of a registered app's residuals was an
opaque label with no rule version/schema/sequence attached, unlike every other target
that goes through `CleanupPlan::new` (which calls `.observed()`).

## Decision

`scan_residuals` now uses `RuleRef::engine().observed()` for the no-rule fallback: the
engine reference still identifies the policy source, but it now carries a captured
observation (rule id/version, schema, sequence, scope) instead of a bare label. A
discovered app already carries its own observed rule reference (which may include a
provider policy), so only the fallback is changed.

## Alternatives considered

Calling `.observed()` unconditionally would wipe a discovered app's provider-policy
observation (it re-captures the observation), so it is limited to the fallback. Leaving
the bare label keeps the source opaque. Inventing a per-app synthetic rule would turn
configuration into code and is not needed. All rejected.

## Consequences

Every residual item now carries a rule reference whose observation is present, so the
residual's source is explainable (which rule/version produced it) rather than an opaque
"engine" tag. No behavior change to the residual scan or clean.

## Verification

- `src/platform/windows/residuals.rs::registered_app_residuals_carry_a_complete_source_observation`

Model migration, not an organic bug fix; no red-run proof claimed. Actual unified gate
results and limitations are recorded in RULES_REFACTOR_STATUS.
