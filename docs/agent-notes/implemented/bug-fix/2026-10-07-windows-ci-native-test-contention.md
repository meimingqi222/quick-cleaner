# Agent Note: Windows CI native deadline tests run without competing live probes

Status: implemented

## Problem

CI run 37537438857 passed macOS and note validation but failed three Windows
tests: two process-tree fixtures never reached their descendant readiness marker,
and the elevated Deny-ACL cleanup test failed after a successful capability probe.
The timeout tests give PowerShell two seconds to start; live inventory and registry
tests ran concurrently for over a minute. The same process tests pass in isolation
on the development host. Native startup contention is a hypothesis, not proof that
the runner or ACL implementation is correct.

## Decision

The Windows test step in `.github/workflows/ci.yml` uses one Rust test-harness
thread. This removes competing test-level live probes from process and ACL deadline
acceptance. macOS and production code keep their existing behavior. Two-second
deadlines, readiness assertions, descendant delayed-write assertions and the
elevated cleanup assertion remain unchanged. A continued failure requires investigating
the implementation rather than increasing timeouts or skipping the tests.

## Alternatives considered

Increasing deadlines changes the behavior under test. Dropping readiness lets a
fixture that never ran claim containment success. Skipping elevated ACL cleanup
would hide a real regression after its capability probe passed. Automatic retry
can turn a real intermittent defect green without explanation.

## Consequences

Windows CI may take longer. Internal rayon concurrency is retained; test-harness
serialization does not claim to prove every production workload succeeds. A green
CI result establishes acceptance in an uncontended test configuration only.

## Verification

- `src/core/proc.rs::review_timeout_kills_shim_descendants_and_closes_pipes`
- `src/core/proc.rs::review_exited_parent_does_not_bypass_the_pipe_deadline`
- `src/core/cleaner.rs::acl_deny_delete_is_overridden_when_elevated`

Proved: real Windows CI run 37537438857 failed the two descendant-readiness
assertions and the elevated cleanup assertion, recorded in
`docs/agent-notes-evidence/2026-10-07-windows-ci-native-contention-red.log`.
The local focused process tests pass. Elevated acceptance must be verified on the
new CI run because this development session is not elevated.

Pitfalls P1-P47 reviewed: P6 elevated recovery, P20 owner failure handling,
P23 hidden consoles, P40 command quoting, P43 bounded node-only ACL repair and
P46 process-tree/pipe deadlines are preserved. No parsing, uninstall, residual
matching, safety rules or production concurrency is modified.
