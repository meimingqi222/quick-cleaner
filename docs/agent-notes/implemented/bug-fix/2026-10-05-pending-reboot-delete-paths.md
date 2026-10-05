# Agent Note: PendingFileRenameOperations paths are und deletable until reboot

Status: implemented

## Problem

Uninstallers (or Windows file replacement) lock shell extensions and register them via `MoveFileEx(MOVEFILE_DELAY_UNTIL_REBOOT)` under `HKLM\SYSTEM\CurrentControlSet\Control\Session Manager\PendingFileRenameOperations`, entries shaped `*1\??\<path>` (`*1` = delete on reboot). From registration until the next reboot the system refuses every delete/rename of those paths regardless of ACL. Observed on a real machine (Baidu Netdisk `YunShellExtV164.dll.<timestamp>`): `SHFileOperationW` returned `0x0000007C`, `Remove-Item` failed with access denied, the user had FullControl with no Deny entries, and takeown failed — nothing user-space can fix it.

## Decision

`is_pending_reboot_delete` reads the REG_MULTI_SZ and matches `*1\??\<normalized path>`. `clean_residuals` reports such paths as `CleanResult::ManualAction`, never `Failed` — retrying is meaningless, the only way out is a reboot. The status text (`tr_status_residual_cleaned_manual`) says the system will clear the path after reboot, and the residual scan must not re-report these paths after a reboot.

## Alternatives considered

Routing pending-delete into the ACL remediation path wastes takeown/icacls runs that cannot succeed. Recording `Failed` and offering the retry dialog sends users into an infinite loop.

## Consequences

The parser handles the `*1` source prefix, the NT namespace `\??\`, and entries whose destination part is empty (source-only rename/delete entries). Failure classification changes here must keep ManualAction distinct from both Ok and Failed.

## Verification

- `src/platform/windows/residuals.rs::pending_src_strips_star_prefix_and_nt_namespace`
- `src/platform/windows/residuals.rs::pending_set_membership_matches_normalized_path`
- `src/platform/windows/residuals.rs::pending_parse_handles_star_prefix_and_empty_dest`

Proved: organic red — the motivating failure was observed on the real machine (SHFileOperationW `0x0000007C`, access denied with a clean ACL and failed takeown; recorded in the pitfalls list before this note existed); the bound tests reproduce the REG_MULTI_SZ shapes seen in the incident and pin the normalized matching.
