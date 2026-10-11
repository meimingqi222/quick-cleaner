# Agent Note: Process search keeps filtered suggestions open while typing

Status: implemented

## Problem

The process-search popover disappeared as soon as ordinary text was entered.
Its visibility predicate required either an empty query or a colon. Clicking an
unfocusable control outside the search also left the input focused and the help visible.

## Decision

Visibility in `src/ui/actions/status.rs` depends only on search focus and explicit
dismissal. `src/ui/views/status.rs` filters syntax examples while typing and shows
an action for the current query plus up to eight matching process-name groups with
counts from the already filtered process view. Suggestions retain the existing
localized syntax descriptions; new search/count copy lives in `src/ui/i18n/mod.rs`.
Only a leading colon or `port:` enters the port preview; `user:SYSTEM` is a user query.

Outside mouse-down on either deferred popover blurs the search unless the pointer
is inside the search bounds. Clicking suggestions stays inside the popover and
stops propagation. Syntax completion keeps the popover open so the user can edit
it; choosing a process/current-query action accepts the query and dismisses the
popover. Esc dismisses first, preserving the existing second-Esc clear behavior.
Editing or clicking the input reopens suggestions; backspace follows that rule too.

## Alternatives considered

Keeping the old nonempty-query dismissal contradicts the requested interaction.
Showing all syntax examples regardless of query gives unrelated suggestions.
Blurring on every search-container outside event would also blur clicks inside the
separately deferred popover, making suggestion selection unreliable.

## Consequences

Typing `mem` retains the popover and narrows syntax to memory; typing `ms` shows
matching process groups. No matches still provides the current-query action.
Process matching reuses the table's current filter rather than a second query parser.
The table continues filtering live. Keyboard navigation is recorded in
`2026-10-11-status-popover-keyboard-navigation.md`.

## Verification

- `src/ui/actions/status.rs::status_popover_visibility_rules`
- `src/ui/views/status.rs::typing_filters_syntax_suggestions`
- `src/ui/views/status.rs::process_suggestions_group_only_filtered_processes`

Proved: before changing the visibility predicate, the focused regression failed at
`status_popover_should_show(true, false, "chrome")`; evidence:
`docs/agent-notes-evidence/2026-10-11-status-search-popover-red.log`.
After the fix the same focused test passes in
`docs/agent-notes-evidence/2026-10-11-status-search-popover-green.log`.
Visibility assertions also cover empty/port/ordinary queries, loss of focus and Esc dismissal.

Pitfalls checked: read the complete P1-P50 list (P10-P13 retired). P2/P3 card geometry,
truncation and overflow guards in this view are preserved; P30 vendor/runtime assets
are untouched. All command parsing, official uninstalls, deletion/path/identity,
residual matching, process-tree deadlines, rules and note-date protections remain
unchanged. P51 points to this search lifecycle decision.

Validation: cargo fmt --check, cargo build, cargo clippy --all-targets -- -D warnings,
cargo test --lib (715 passed, 9 ignored outside sandbox), strict note anchors,
pitfall pointers and UI localization checks pass. Three registry-fixture tests
failed to create their fixtures inside the sandbox and passed outside it.
Native pointer/focus interaction was not exercised in this run.
