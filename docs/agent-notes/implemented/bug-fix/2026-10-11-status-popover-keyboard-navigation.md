# Agent Note: Process-search candidates support keyboard selection

Status: implemented
Partly-superseded-by: 2026-10-11-popover-mouse-isolation.md

## Problem

The process-search popover had clickable candidates, but `src/ui/components/search_box.rs`
ignored Up, Down and Enter. Its first query action looked selected even when keyboard
selection could not move. Port candidates beyond the viewport were inaccessible by keyboard.

## Decision

The status search handles Up/Down to cycle candidates and Enter to choose the highlighted
candidate. Other search boxes retain their existing key handling. During IME composition
these keys are left to the input method. Left/Right remain text-caret controls.

`src/ui/views/status.rs` shares the syntax catalog and port filtering between the rendered
rows and the keyboard candidate list, including the same eight process-group and eighty
port-row caps. Each candidate carries its row identity, query and completion behavior.
`src/ui/actions/status.rs` resolves selection by identity instead of a raw row index so
sampling/filter changes cannot leave an out-of-range index. A missing selection falls
back to the first row. Input edits reset selection in `src/ui/text_input.rs` and status
clear/backspace actions; `src/ui/state.rs` owns selection and the port scroll handle.

Syntax completion keeps the popover open for further editing; process, port and current
query selections accept and dismiss it, just like mouse clicks. Enter on an explicitly
dismissed popover is inert. Arrow keys reopen it. Esc retains the previous lifecycle.
Navigation scrolls the port list to keep the selected candidate visible.

## Superseded

The interaction contract and regression anchors above remain valid. The subsequent
popover refactor moves selection, scrolling and keyboard state into
`src/ui/components/popover.rs`; status owns the candidate catalog and business actions.
The shared search-box component no longer contains status-specific navigation logic.
Both syntax and port rows now use one rendering entry and one candidate list.

## Alternatives considered

Handling arrows on the detached popover would require moving focus off the text input.
Remembering only an index can select a different row or exceed the list after filtering.
Building separate keyboard-only syntax or port catalogs would let Enter act on a different
candidate than the rendered row. These alternatives are rejected.

## Consequences

All rendered candidate kinds can be reached while the search input retains focus.
Selection wraps at either end and empty candidate lists are inert. The initial highlight
is the first candidate; continued typing resets it. No cleanup or process termination
behavior changes.

## Verification

- `src/ui/views/status.rs::popover_keyboard_navigation_wraps_and_survives_filter_changes`
- `src/ui/actions/status.rs::status_popover_visibility_rules`
- `src/ui/views/status.rs::typing_filters_syntax_suggestions`

Proved: manufactured red by changing only the Down step from current + 1 to current;
the focused navigation regression fails expecting index 1 and receiving index 0 in
`docs/agent-notes-evidence/2026-10-11-status-popover-keyboard-red.log`. Restoring the step
and immediately rerunning the same test passes in
`docs/agent-notes-evidence/2026-10-11-status-popover-keyboard-green.log`.
The regression covers both wrap directions, stable identity after filtering, a vanished
row, empty/single-row lists and the chosen syntax query/completion behavior.
Native keyboard/IME and scrolling interaction have not been manually exercised.

Pitfalls checked: P1-P51 previously read in this session. P2/P3 status card geometry and
truncation stay intact; P30 vendored assets are unchanged; P51 focus/Esc/typing lifecycle
is preserved. No official uninstall, command parsing, residual name matching, deletion,
path/identity protection, rule, deadline or note-date logic is modified. P52 points here.

Validation: cargo fmt --check and cargo clippy --all-targets -- -D warnings pass.
Final cargo test --lib outside the sandbox passes 716 tests (9 existing ignores).
The ordinary cargo build cannot replace the running target/debug executable;
cargo build --target x86_64-pc-windows-msvc succeeds in the independent target directory.
Strict note anchors, pitfall pointers, UI localization and git diff --check pass.
