# Agent Note: The live-database gate keeps a falsifiable crash-leftover channel

Status: implemented

## Problem

The SQLite live-database gate has to separate active databases from crash leftovers, and both mistakes are costly: deleting an active database destroys user data, while refusing every database with companion files (`-wal`, `-shm`, journals) means stale crash leftovers are never cleaned — companion files existing is not proof of a live connection.

## Decision

The gate protects a path only when occupancy evidence says a live connection exists (`holds_live_database` / active SQLite member) **and** `looks_like_crash_leftover` does not falsify it. The crash-leftover check requires both stale-time and shape evidence (`crash_leftover_requires_both_evidence`), so a fresh database is never waved through and an old one with real occupancy is still refused. The SQLite family groups companions with the main database file (`sqlite_family_key_groups_companions_with_main`) so a protected main file protects its companions and vice versa.

## Alternatives considered

Refusing everything with companion files — crash leftovers accumulate forever. Treating companion absence as proof of no connection — an active database can transiently lack visible companions, and read failures are unknown, not absence.

## Consequences

The channel is deliberately falsifiable rather than permissive: evidence must point at a live connection for the gate to close, and staleness plus shape must both hold to reopen it. Read failures keep the gate closed (Unknown is not absence).

## Verification

- `src/core/safety.rs::crash_leftover_requires_both_evidence`
- `src/core/safety.rs::live_database_directory_is_rejected`
- `src/core/safety.rs::sqlite_family_key_groups_companions_with_main`

Proved: organic red — the motivating failure was observed on the real machine (stale crash leftovers were permanently uncleanable while the gate treated any companion file as a live connection, recorded in the workspace pitfalls list); the bound tests reproduce the both-evidence requirement, the live-database refusal and the family grouping.
