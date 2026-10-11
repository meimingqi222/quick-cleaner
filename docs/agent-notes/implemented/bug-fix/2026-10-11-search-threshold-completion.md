# Agent Note: Threshold completion leaves values to the user

Status: implemented

## Problem

Typing mem offered a fixed mem > 1 GB example and accepting it inserted that threshold.
CPU and user examples similarly inserted 10% and SYSTEM. Literal example matching
missed compact operators and could replace a value already supplied by the user.
Memory shorthand such as mem>1m was not understood; the default MB unit was undisclosed.

## Decision

`src/ui/views/status.rs` completes field/operator templates, leaving values empty.
Reserved field templates take priority while applicable; the user's comparator is
preserved. Completed threshold values no longer offer a replacing template. The same
candidate payload is used by mouse and Enter. Input focus and open state are preserved.
`src/ui/i18n/mod.rs` describes MB and percent defaults rather than arbitrary examples.

`src/core/status.rs` preserves the already existing unitless MB behavior, adds common
k/m/g and KiB/MiB/GiB suffixes, accepts field case variants and rejects nonfinite,
negative and overflowing thresholds. Existing KB/MB/GB/B scaling stays binary.
No OR/AND or port-query semantics change. Broader UX findings and remaining work are
recorded in `docs/STATUS_SEARCH_UX.md`.

## Alternatives considered

Replacing 1 GB with another sample still chooses the user's threshold for them.
Duplicating numeric parsing in UI would drift from the table predicate. Rewriting
all query grammar here would silently change existing multi-clause semantics.
These alternatives are rejected.

## Consequences

mem, cpu and user completion now requires the user to supply a value, matching port
prefix completion. Explicitly typed values remain intact. The original report that
unitless mem>1 cannot filter was not reproduced by the current matcher: both unitless
and explicit MB assertions already passed before the shorthand assertion failed.
The running native app may be an older build; no native-input reproduction is claimed.
Incomplete/invalid input still falls back to text matching; diagnostic parsing remains
documented follow-up work rather than silently showing stale results.

## Verification

- `src/ui/views/status.rs::search_completion_leaves_threshold_values_to_the_user`
- `src/core/status.rs::process_search_memory_defaults_to_mb_and_accepts_shorthand`
- `src/core/status.rs::process_search_query_matches_ports_and_clauses`

Proved: organic red tests before implementation: completion expected "mem > " but
received "mem > 1 GB" in
`docs/agent-notes-evidence/2026-10-11-search-completion-red.log`; memory default MB
assertions passed, then mem>1m failed in
`docs/agent-notes-evidence/2026-10-11-search-memory-red.log`. The same focused tests
pass after implementation in
`docs/agent-notes-evidence/2026-10-11-search-completion-green.log` and
`docs/agent-notes-evidence/2026-10-11-search-memory-green.log`.

Pitfalls checked: P1-P53 inspected during this session; P2/P3 card clipping and
truncation, P30 vendor assets, P51 lifecycle, P52 candidate identity and P53 mouse
isolation remain intact. No uninstall, command execution, deletion, path protection,
residual matching or rule logic changes. P54 points here.

Validation: cargo fmt --check, cargo clippy --all-targets -- -D warnings and
cargo build --target x86_64-pc-windows-msvc pass. Full cargo test --lib outside
the sandbox passes 721 tests with 9 existing ignores. Strict notes, pitfall pointers,
UI localization and git diff --check pass. Native text entry is not manually verified.
