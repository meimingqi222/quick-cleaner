# Agent Note: Deny-Delete DACLs are stripped with takeown plus /remove:d, never /grant alone

Status: implemented
Partly-superseded-by: 2026-10-07-windows-delete-retry-stall.md

## Problem

Applications write Deny-Delete DACLs on their log directories as tamper protection (WorkBuddy form observed on a real machine: `LAPTOP-…\USER  Deny  DeleteSubdirectoriesAndFiles, Delete`). The process is long gone, so a handle is not the cause; `Remove-Item` fails with access denied and cleanup logs show `os error 5`, not `os error 32`. An elevated process could strip the Deny and delete — failing to do so reports "the system prevents this" when the required steps simply were not taken.

## Decision

`force_delete_access` (platform/windows/security.rs) runs `takeown /a /r` + `icacls /remove:d` (current-user SID + Everyone) + `icacls /grant *S-1-5-32-544:(OI)(CI)F /t /c /q`. The SID is used instead of the group name because localized "Administrators" silently fails on Chinese systems. It is wired into `remove_file_forcing` / `remove_dir_forcing` and fires only on `ErrorKind::PermissionDenied` — error 32 (sharing violation) cannot be fixed by rewriting the ACL, and routing it here wastes a slow subprocess on every busy file.

## Alternatives considered

`/grant` alone does not work: Windows AccessCheck treats a matching Deny as authoritative and later Allow entries never override it — the real machine had Deny Delete and Allow FullControl coexisting and stayed undeletable. Running takeown for every failure class slows whole batches and triggers UAC when not elevated; `force_delete_access` returns false immediately when not elevated (macOS is constant false, same signature).

## Superseded

The successor replaces recursive ACL repair with bounded repair of the failing node and its eligible parent. `/r`, `/t` and inheritable grants are retired; the directory-wide retry after any failure is removed. Removing Deny before granting Allow, localized SID use, elevation gating and the parent DeleteChild check remain required.

## Consequences

Deny `(D,DC)` is often written on the parent directory: the leaf's own DACL frequently looks clean (leaf icacls succeeds), so "stripped the failing leaf" is never sufficient — strip the leaf, retry, and only then strip non-`is_protected` parents. CI images that are elevated but cannot strip Deny from Temp skip the bound test instead of going red; the positive case still runs on real machines.

## Verification

- `src/core/cleaner.rs::acl_deny_delete_is_overridden_when_elevated`

Proved: organic red — the motivating failure was observed on the real machine (Get-Acl showed the Deny entries, manual Remove-Item failed, cleanup log reported os error 5); the bound test writes a real `Deny (D,DC)` via icacls, asserts bare deletion fails while the Deny stands, and verifies the `force_delete_access` override path when elevated.
