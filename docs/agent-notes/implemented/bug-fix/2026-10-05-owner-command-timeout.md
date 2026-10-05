# Agent Note: An attempted ecosystem cleanup cannot grant bare-delete fallback

Status: implemented

## Problem

Go and pnpm cleanup used Option propagation after starting the mutating command. A timeout returned None, which cleaner interprets as an unavailable owner route and permission to fall back to filesystem deletion. That continued mutation on an unknown partially modified store.

## Decision

None is reserved for preflight unavailability or mismatched scope. Once the mutating command is attempted, timeout or failure returns Some(false). Go confirms the cache was removed, pnpm confirms store status, brew rechecks dry-run resources, and Docker confirms the exact reference is absent from a successful inventory. A failed resource query cannot mean absence.

## Alternatives considered

Treating timeout as tool unavailability authorizes further destruction. Trusting only exit zero misses no-op cleanup and lost verification. Requiring every ecosystem directory to disappear is wrong for prune operations.

## Consequences

Resource-specific verification adds bounded native queries after successful commands. No production ecosystem cleanup is needed for the regression fixtures; injected command interfaces exercise the attempted-timeout branch and real file sentinels.

## Verification

- `src/core/owner.rs::attempted_owner_timeout_never_grants_filesystem_fallback`
- `src/core/owner.rs::owner_completion_checks_resource_state_after_zero_exit`
- `src/core/docker.rs::remove_verifies_exact_reference_and_rejects_unknown_inventory`

Proved: temporarily restored Option propagation after the Go mutating command and ran the timeout fixture. It failed with left None versus required Some(false), cargo exit 101. Restored logic passes both Go and pnpm timeout cases and leaves the real store sentinel intact. Docker cases cover retained reference, other retained tags, unknown inventory and option injection.
