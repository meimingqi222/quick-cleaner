# Agent Note: Duplicate targets retain every deletion constraint

Status: implemented

## Problem

Deduplicating by keeping the first row hid conflicting deletion methods and discarded later rules' preserve paths. UI deduplication alone could not protect callers entering core directly.

## Decision

Normalize paths and merge rule constraints in both scan output and the common core cleanup entrance. Recommendation is the intersection. Conflicting operations, disposal, identity availability or snapshots block the target. Merge all preservation and ownership checks before any mutation. UI uses the same core target merge.

## Alternatives considered

Load-order precedence silently grants broader deletion. Retaining duplicate rows double-counts and retries the same path. A UI-only merge leaves other cleanup callers exposed.

## Consequences

Compatible targets count once. A parent operation overlapping any contributed preserve path is rejected. Rule conflicts remain visible in UI and cleanup logs. Unix identity checks continue using recheck semantics rather than adding mtime/length equality.

## Verification

- `src/ui/state.rs::duplicate_scan_policies_cannot_silently_select_a_deletion_method`
- `src/core/cleaner.rs::core_duplicate_merge_preserves_every_rule_in_both_orders`
- `src/core/cleaner.rs::typed_plan_rejects_conflict_and_wrong_operation_without_deleting`

Proved: temporarily removed the conflict block and ran the UI fixture. It failed at the assertion that revalidate rejects the merged target, cargo exit 101. Restored blocking passes the same real-file fixture. The core fixture checks both input orders, exactly one failed target, and a surviving preserved sentinel. The common merge retains all rules before deletion.
