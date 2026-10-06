# Agent Note: Windows failed subtrees are not recursively retried or repermissioned

Status: implemented

## Problem

Windows permanent deletion remained near completion with failures already counted. The user's local log contains sharing violations (32) and nonempty directories (145). After every unsuccessful directory removal, `delete_tree` unconditionally repaired the entire subtree's ACL and recursively retried surviving children. Each ancestor repeated the same work: a four-directory fixture with one locked leaf produced 31 failures instead of 5. `takeown` and `icacls` also used unbounded process waits. Cancellation started a fresh recursive measurement of unfinished targets.

## Decision

`src/core/cleaner.rs` removes the unconditional subtree retry. Files and empty directory shells still use the existing read-only retry and AccessDenied-only ACL recovery. Leaf recovery still retries after repairing its eligible parent, preserving the parent DeleteChild case. Sharing violations and nonempty directory errors do not authorize ACL repair.

`src/platform/windows/security.rs` repairs only the requested node: no recursive flags, no inheritable grants, and icacls uses link-local mode. The Deny-removal-before-Allow sequence, SID trustees, elevation gate and platform facade remain intact. Each permission tool has a two-second native process wait; a stalled child is killed and reaped before returning failure. Null standard streams avoid pipe joins and blocked output.

Cancellation skips remaining-target measurement and keeps existing scan estimates. Noncancelled partial failure still remeasures the surviving data.

## Alternatives considered

Increasing the retry limit or timeout would preserve multiplicative work. Removing all ACL recovery would regress WorkBuddy Deny Delete/parent DeleteChild cleanup. Detaching a timed-out thread would allow it to keep changing ACLs after the report. Running permission tools recursively from a parent would also modify unrelated children without their core safety checks.

## Consequences

Failed subtrees are visited once, and a locked leaf remains while removable siblings disappear. This fixes the demonstrated retry path; the screenshot alone does not prove which native call was waiting. Native filesystem calls themselves can still block in a faulty driver or disconnected volume; this change does not claim to interrupt those calls. Cancellation during a permission repair can wait for the bounded commands already in flight. Actual elevated Deny recovery retains its existing privilege-dependent regression test.

Pitfall review: P5 read-only directory exit retained; P6 Deny removal, parent recovery and error-class gating retained with bounded node scope; P7 residual reboot guard untouched; P19/P25/P37 identities and merged plans untouched; P20 owner timeout fallback unchanged; P26/P27 skipped-subtree semantics unchanged; P33 live SQLite grouping retained; P35 protected Temp root preserved. Remaining pitfalls reviewed for indirect effects; no uninstall, command parsing, discovery, UI copy or platform layering changes.

## Verification

- `src/core/cleaner.rs::locked_leaf_is_not_retried_at_every_ancestor`
- `src/core/cleaner.rs::cancelled_cleanup_does_not_remeasure_remaining_targets`
- `src/platform/windows/security.rs::stalled_acl_command_is_terminated_at_deadline`
- `src/core/cleaner.rs::acl_deny_delete_is_overridden_when_elevated`
- `src/core/cleaner.rs::deletes_readonly_directory_shell`
- `src/core/cleaner.rs::clean_targets_remeasures_partially_failed_dir`

Proved: restored only the original unconditional directory retry and its enumeration helper, then ran the focused locked-leaf test. Assertion failed with 31 failures versus the expected 5 (`docs/agent-notes-evidence/2026-10-07-windows-delete-retry-red.log`). Restored the fix immediately and the same focused test passed (`docs/agent-notes-evidence/2026-10-07-windows-delete-retry-green.log`). The timeout test starts a real sleeping Windows process, verifies a 150ms deadline returns failure within three seconds and confirms the child has exited. Windows cleaner tests passed; macOS native acceptance was not run.

Final checks: `cargo fmt --check`, `cargo build`, `cargo test --lib` (692 passed, 9 ignored), `cargo clippy --all-targets -- -D warnings`, strict note anchors, pitfall pointers and `git diff --check` passed. The session was not elevated, so the existing elevated Deny-ACL test returned at its privilege gate; elevated ACL acceptance remains unverified in this session. The existing vendored GPUI unreachable-code warning remains outside the package's clippy errors.
