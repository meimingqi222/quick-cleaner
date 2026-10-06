# Agent Note: System path aliases above the root do not invalidate rule discovery

Status: implemented

## Problem

The first time the CI macOS job actually ran the test suite (after the push that made
macOS compile and lint clean), 42 discovery tests failed with `Unknown` where they
expected `Confirmed`/`Absent`, or found zero targets. Root cause: `facts::confined()`
walked the **textual** ancestor chain of the root and rejected any ancestor that is a
symbolic link ("Redirected evidence ancestor"). macOS's `env::temp_dir()` is
`/var/folders/...`, and `/var` is a system symlink to `/private/var` — so every fixture
root had a link ancestor and every confinement check failed closed.

This is not test-only: on macOS the *production* `user_temp` anchor (and therefore the
DNS and QuickLook cache entrances) sits under the same alias, so those entrances would
have discovered nothing on a real Mac.

## Decision

`confined()` now: rejects the root itself being a link (unchanged), canonicalises the
root (resolving system/user aliases such as `/var` → `/private/var` or a Windows
junction redirecting `%TEMP%`), then walks the root and each relative component exactly
as before — every hop **below** the root still has to be a real directory, and any link
found there is still refused with "Redirected evidence". The ancestor-level rejection is
gone because after canonicalisation the real ancestor chain contains no links by
construction; the protection that matters (root or any descendant swapped for a link)
is unchanged.

## Alternatives considered

Special-casing `/var` on macOS — rejected: Windows users redirect `%TEMP%` with
junctions for the same reason, so the defect is cross-platform and deserves one fix.
Canonicalising both root and target and comparing — rejected: the target may legitimately
not exist yet (that must stay `Ok(false)`, not an error), and the per-component walk
already provides the link protection. Teaching `testing::fixture` to canonicalise — rejected:
production anchors (`user_temp`) hit the same alias, so the fix belongs in the check.

## Consequences

Discovery works on any host whose temp or home path is reached through a stable alias;
macOS DNS/QuickLook entrances are no longer silently empty. Genuine redirection at or
below the root is still refused, so the identity/link protection PITFALLS relies on is
intact.

## Verification

- `src/core/rules/facts.rs::confined_path_tolerates_an_aliased_ancestor_but_rejects_redirected_links`
  (alias ancestor tolerated; root-as-link refused; link below the root refused)
- CI macOS job: 42 fixture-discovery failures on run 37452308801 are the field evidence
  that motivated this fix.

Proved: with the canonicalisation removed (textual ancestor walk restored), the new test
fails on this Windows host too — a junction redirecting the fixture root's parent is
reported as "Redirected evidence ancestor" instead of a successful confinement check;
evidence `docs/agent-notes-evidence/2026-10-06-confined-alias-red.log`, restored run green
in `...-confined-alias-green.log`. Windows suite stays at 608 passed / 0 failed.
