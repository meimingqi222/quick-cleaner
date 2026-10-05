# Agent Note: Protected session worktrees must not be offered as cleanable targets

Status: implemented

## Problem

The worktree discovery capability found valid linked checkouts, including session resources protected by core safety. The category builder still presented them as cleanable, contradicting the deletion boundary and the existing category invariant.

## Decision

The worktree category builder consults core safety before adding a discovered checkout. It does not copy owner marker rules or remove safety protection. The fixture proves a linked checkout is discoverable first, then remains intact and is excluded once workspace ownership evidence appears.

## Alternatives considered

Relaxing protection would expose session recovery source to deletion. Relaxing the category invariant would permit permanently uncleanable targets in the list. Both are rejected; discovery and deletion share the authoritative safety predicate.

## Consequences

Protected resources are excluded from cleanup discovery without deleting files or Git administration. Raw worktree inventory remains available to callers that inspect resources. No real checkout was deleted during validation.

## Verification

- `src/core/categories/mod.rs::every_target_has_cleanable_contents`
- `src/core/categories/dev.rs::owned_agent_session_worktrees_are_filtered_by_core_safety`

Proved: the unfiltered production builder caused every_target_has_cleanable_contents to fail on a protected session subtree; after adding the core safety filter the full library run passed. The added isolated fixture separately checks discovery before and after owner evidence and preservation of both checkout source and Git administration.
