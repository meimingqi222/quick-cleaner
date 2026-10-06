# Agent Note: Repeated discovery shares probes; path-delete shows its operation

Status: implemented

## Problem

Two smaller gaps from the "next batch" list:

- **Performance evidence (GOAL G).** The directory-layout fixtures asserted a fixed
  read count once (`directory_layouts_preserve_baseline_and_share_reads` → 7 reads for
  17 targets), but nothing showed the count is *stable across runs* — i.e. that shared
  probes do not accumulate or re-scan on a second pass.
- **Path-delete confirmation (GOAL E).** `request_clean_path` showed the path and size
  but not the operation granularity (whole folder vs single file), so the user could
  not see what "delete this path" would actually do.

## Decision

- `directories::tests::repeated_discovery_reads_shared_inventories_once_per_run` scans
  the same fixture three times and asserts the target set and the enumeration read
  count are byte-for-byte identical each run (and bounded by the 512 probe budget). A
  shared inventory is read once per run and never accumulates across runs.
- `tr_confirm_path_operation(lang, is_dir)` states the operation in both languages; the
  path-delete confirmation appends it to the detail.

## Alternatives considered

A wall-clock benchmark would be flaky in CI and the "before" side (the pre-migration
Rust enumeration) no longer exists, so determinism + boundedness is the honest evidence
available. Reusing the whole baseline fixture for the repeat test would duplicate ~40
lines; a small sharing fixture keeps it focused. Showing the operation only when the
path is a directory would hide the file case. All rejected.

## Consequences

The repeated-discovery test locks "no accumulation / no duplicate full scan" for the
declarative layouts. The path-delete dialog now says whether it deletes a folder and its
contents or a single file. (The concurrent native batch enriched the same dialog with a
truthful recycle-bin disposal line and a running-process caution; `tr_confirm_path_operation`
was folded into that detail.)

## Verification

- `src/core/rules/directories.rs::repeated_discovery_reads_shared_inventories_once_per_run`
- `src/ui/i18n/mod.rs::path_operation_distinguishes_folder_and_file`

Model/UI migration plus performance evidence, not an organic bug fix; no red-run proof
claimed. Actual unified gate results and limitations are recorded in RULES_REFACTOR_STATUS.
