# Agent Note: Verify portable program paths before removing the application row

Status: implemented

## Problem

After Magpie's file cleanup, the application row remained. The live history at
04:08 on 2026-10-07 records five successful targets and no failures; the recorded
executable, shortcut and data directory are now absent. The old UI completion
predicate only recognized UninstallEntry or InstallDir labels in residual items.
Those labels are not durable ownership evidence: scope deduplication can absorb
an executable into a parent with another source, and a missing executable is not
included when residuals are scanned again. The exact original residual selection
is not persisted, so the live incident does not prove which label was lost.

## Decision

Capture the listed application's discovery evidence before cleanup. After the
cleaner returns, verify its original program_paths in the background. A nonempty
list must produce NotFound for every path; an existing node, a link or an unknown
metadata error keeps the row. For discovered applications this physical result
replaces the source-label predicate. Registered applications retain their existing
completion rule. Selected failures still retain the row for retry.

## Alternatives considered

Always hiding a row on cleaner success would hide partial removals. Rescanning all
applications is unnecessary to verify a few owned paths. Changing deduplication to
upgrade the parent or guessing ownership of the executable's parent would violate
the established authority boundary.

## Consequences

Successful portable cleanup updates the list even when residual source labels
cannot prove completion. No deletion scope, identity gate or path protection is
changed. Missing discovery evidence fails closed instead of using a label fallback.

## Verification

- `src/core/apps.rs::discovered_cleanup_verifies_program_paths_without_source_labels`

Proved: a compatibility probe delegating to the old label-only predicate with no installation labels failed at "removed portable app must leave the list even without InstallDir labels"; output is in `docs/agent-notes-evidence/2026-10-07-discovered-cleanup-list-red.log`. Restoring filesystem verification passes the same focused test, recorded in `docs/agent-notes-evidence/2026-10-07-discovered-cleanup-list-green.log`. The final fixture also demonstrates deduplication losing the InstallDir label, partial artifact removal, empty ownership evidence and an existing directory.
