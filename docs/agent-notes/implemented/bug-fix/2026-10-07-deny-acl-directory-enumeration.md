# Agent Note: Recover a denied directory enumeration before visiting its children

Status: implemented

## Problem

Windows CI runs 37537438857 and 37587437605 failed elevated Deny-ACL cleanup even
though direct permission recovery passed its capability probe. Serializing tests
fixed the two process-fixture failures but did not fix this assertion.
A local, self-owned directory with a Deny (D,DC) entry reproduces the missing case:
directory metadata succeeds, but read_dir returns error 5. delete_tree returned
Failed at enumeration, before reaching the permission recovery at deletion.
Run 37589062534 confirms the same state on CI: protection and live-database gates
are false, no child was deleted, directory enumeration still returns error 5.

## Decision

`src/core/cleaner.rs` retries directory enumeration once after node-only permission
recovery, only when the first read returns AccessDenied. The caller has already
checked core safety, cancellation and link handling. The existing platform recovery
retains elevation gating and bounded takeown/icacls commands. The retry cannot walk
or modify an ancestor or unvisited child; each child still passes its own protection.
No other I/O error authorizes recovery, and a failed retry remains Failed.

## Alternatives considered

Restoring recursive ACL repair or an ancestor subtree retry would reintroduce P43.
Increasing the CI timeout does not help an enumeration error occurring before the
tool is invoked. Skipping the elevated test would hide an actual regression. Assuming
Deny Delete only affects deletion contradicts the observed read_dir error.

## Consequences

The existing elevated acceptance can reach and delete children after removing a
node's Deny. Production read and deletion protections remain distinct. The new
regression uses an owned fixture and removes its own Deny through a callback, so
it can exercise the enumeration branch without an elevated development session.
This does not substitute for end-to-end elevated acceptance on the CI runner.

## Verification

- `src/core/cleaner.rs::denied_directory_enumeration_recovers_only_the_current_node`
- `src/core/cleaner.rs::acl_deny_delete_is_overridden_when_elevated`
- `src/core/cleaner.rs::locked_leaf_is_not_retried_at_every_ancestor`

Proved: removing only the recovery call makes the focused enumeration test fail
at its successful-read assertion with error 5, recorded in
`docs/agent-notes-evidence/2026-10-07-acl-enumeration-red.log`. Restoring it makes
the same test pass in `docs/agent-notes-evidence/2026-10-07-acl-enumeration-green.log`.
The fixture also asserts the initial denial, exact recovery scope, preserved child
and no recovery on NotFound. The pre-fix elevated CI failure is recorded in
`docs/agent-notes-evidence/2026-10-07-windows-ci-native-contention-red.log`.
The CI stage diagnosis is preserved in
`docs/agent-notes-evidence/2026-10-07-acl-enumeration-ci-red.log`.

Pitfalls P1-P47 reviewed; P5/P6/P43/P45 remain enforced. Only AccessDenied at the
current authorized directory adds one bounded recovery attempt. No second blacklist,
recursive permissions, nonempty-directory repair or retry of a failed child tree
is introduced. P48 records the enumeration trap.
