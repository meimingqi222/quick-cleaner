# Agent Note: Hide background consoles and explain preserved worktree contents

Status: implemented

## Problem

Worktree cleanup repeatedly opened Windows console windows because the shared timeout runner spawned Git without CREATE_NO_WINDOW. Five real Maka checkouts were preserved because they contained uncommitted source changes, but every preflight refusal became the unrelated in-use-check-inconclusive reason. The long path preceded that reason and pushed it beyond the visible failure row. The user explicitly chose to preserve these changes.

## Decision

Set CREATE_NO_WINDOW only on the shared Windows background runner before spawning. Preserve stdout/stderr collection, timeout, termination and exit-code behavior. Worktree preflight returns dirty, locked or unverifiable reasons rather than a bool; cleaner records those reasons and i18n provides both languages. Failure details put the actionable reason and size before the full path. All existing deletion and Git-registration protections remain enforced.

## Alternatives considered

Forcing removal would discard real source changes and violate the user's preserve choice. Removing console suppression only from Git would leave other background owner commands with the same problem. Throwing away subprocess output would hide errors and weaken completion checks. Merely relabeling every unknown result as dirty would misrepresent locks, missing tools and broken registrations.

## Consequences

Background Git and owner commands use no console on Windows. The five retained worktrees stay on disk and show why they cannot be cleaned automatically. Long paths may still be clipped, but the reason appears first. Interactive official uninstallers retain their separate launch path. Scan-time metadata discovery remains unchanged; no new scan-time subprocesses were added.

## Verification

- `src/core/proc.rs::windows_background_command_has_no_console_and_keeps_output_and_exit_code`
- `src/core/worktrees.rs::dirty_locked_and_changed_registration_never_fall_back_to_deletion`
- `src/ui/i18n/mod.rs::worktree_reason_precedes_long_path_in_both_languages`

Proved: restoring the old path-first failure text causes the bilingual long-path test to fail; restoring reason-first text passes. The real Windows PowerShell child reports GetConsoleWindow equal to zero, emits captured stderr and preserves exit code 7. Removing the no-window flag alone still reports zero under this tool host, so that negative console experiment does not prove a failure; the positive native check and explicit spawn flag verify the implementation. Isolated Git repositories check dirty/untracked/ignored, locked and unknown reasons and preserve their files. Read-only native Git status confirms uncommitted source edits in each of the five real retained checkouts; they were not deleted.

Pitfalls checked: P1/P8/P14/P16/P17/P21 official uninstall and long desktop dispatch remain separate; P2/P3 rendering protections are unchanged and failure text now puts its meaning before long paths; P4 WMI is untouched; P5/P6/P7 filesystem/ACL/reboot protection is retained; P9 update-helper process flags remain separate; P15 raw database probes are unchanged; P18/P19 rule and duplicate-plan guards remain intact; P20 timeout never authorizes owner fallback; P22 exact registration and dirty-worktree protections remain in force. P23 records the observed console and hidden-reason failure.
