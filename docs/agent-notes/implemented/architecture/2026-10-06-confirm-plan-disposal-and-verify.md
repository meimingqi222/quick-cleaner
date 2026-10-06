# Agent Note: Disk-lens and path confirmations disclose the real disposal and result

Status: implemented

## Problem

Two gaps remained in the GOAL E full-entrance plan review:

1. `request_clean_path` and `request_clean_disk_selected` both claim unconditionally that
   files "do not go to the Recycle Bin and cannot be recovered", while both entrances
   honor the user's "delete to recycle bin" setting (`Root::disposal`). With the setting
   on, the confirmation stated the opposite of what the executor would do.
2. `start_clean_disk_selected` counted only failed deletions in its result status
   (`target.exists() && !was_skipped`). When every selected target was skipped by
   protection rules, the status still read "batch delete complete", contradicting the
   actual verified outcome.

## Decision

The confirmation details of both entrances now open with a disposal line
(`tr_confirm_disposal`) that reflects the current setting: recycle-bin mode says items
can be restored, permanent mode says deletion is unrecoverable and asks the user to
check for important data. The running-program caution is separated
(`tr_confirm_running_caution`) and applies to both modes because in-use or protected
items fail or get skipped under either. The path entrance keeps the concurrent
operation-granularity line (whole directory vs single file); the disk-lens entrance
keeps the resolved-scope list. The claim "not going to the recycle bin" now exists only
in the permanent branch.

The disk-lens finish counts unresolved targets by verified outcome — a target that
still exists after the clean is unresolved whether the cause was failure or protection
skip (`unresolved_disk_targets`), so a fully skipped batch can no longer be reported as
complete.

## Alternatives considered

Fixing only the text of the old static strings would still hard-code one disposal mode
into the other. Deriving the disposal per target from the executor's report would
disclose the method only after running. Keeping `was_skipped` in the finish count would
leave any future skip reason invisible again; existence after the clean is the verified
fact. All rejected.

## Consequences

Both entrances now state the deletion method that will actually run, in both languages,
and the disk-lens result status counts every surviving target. No execution behavior
changed — the summaries only disclose what was already true.

## Shared-probe multi-run performance evidence

Method: the compiled `rules.exe explain` CLI runs 5 times per fixture root on the same
machine (psutil RSS sampled at 2 ms; wall clock around the process). Fixtures:
`cache-catalog-extra` (4 `.cache` children incl. a Chromium leaf layout),
`directory-layout-extra` (Projects/isolated, 2 files), `manifest-layout-extra` (2
manifests, 3 rows). Results in `docs/agent-notes-evidence/2026-10-06-shared-probe-perf.json`
and `...-perf-py.log`:

- read counts are identical in every run: `directory_reads = 1` per fixture root,
  probes 2/2/4, manifest reads 0/0/2 — enumeration sharing and budgets hold across
  repeated scans; `budget_blocked = 0` throughout;
- wall time per run 28–45 ms (mean 43/30/31 ms, spread ≤ 6 ms); peak RSS ≈ 12–13 MB.

Limitations recorded honestly: the pre-migration binary is not rebuilt, so "before"
timings come from the per-batch read counts recorded in RULES_REFACTOR_STATUS rather
than a re-run; Chromium signature probing inside the cache walk performs its own
`read_dir` calls that the `Enumeration` counters do not include; debug-profile timings
are indicative, the load-bearing evidence is the stable, shared, bounded read counts.

## Verification

- `src/ui/i18n/mod.rs::disposal_summary_reflects_the_recycle_setting`
- `src/ui/actions/disk.rs::unresolved_counts_surviving_targets_regardless_of_reason`
- Performance evidence: `docs/agent-notes-evidence/2026-10-06-shared-probe-perf.json`

Model/UI migration, not an organic bug fix; no red-run proof claimed. Actual unified
gate results and limitations are recorded in RULES_REFACTOR_STATUS.
