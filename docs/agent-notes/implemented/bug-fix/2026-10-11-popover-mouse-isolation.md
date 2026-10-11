# Agent Note: Reusable search popovers own mouse hit isolation

Status: implemented

## Problem

Hovering the search popover also highlighted the process table behind it. Stopping
mouse-down propagation does not block GPUI hover hit testing. Separate syntax and
port surfaces duplicated lifecycle, selection, scrolling and mouse behavior.

## Decision

`src/ui/components/popover.rs` owns the reusable state and rendering primitives.
The surface uses `occlude()` to block underlying hitboxes, stops all mouse-down/up
propagation and stops ancestor scroll propagation after the child list scrolls.
Outside clicks dismiss without swallowing the destination click; anchor bounds are
excluded from dismissal. Deferred rendering keeps the surface above the table.

`PopoverState` owns dismissal, stable candidate identity, keyboard selection and the
scroll handle. IME composition leaves navigation, confirmation and Escape unhandled.
Rows synchronize hover selection; lists scroll keyboard selection into view.
Callers supply focus, candidate identities, content and business completion callbacks.
Status search uses one candidate list for both rendering and Enter, with process-group
identity based on normalized name rather than a changing representative PID.

## Alternatives considered

Adding more stop-propagation handlers does not fix hover hit testing. A whole-window
modal barrier would unnecessarily swallow outside destination clicks. A generic
component that owns process data or query syntax would couple other views to status.
These alternatives are rejected. Explicit anchor geometry remains caller-owned;
automatic viewport placement is outside this anchored component's current contract.

## Consequences

Underlying table hover, clicks and scrolling are blocked only inside the surface.
Its rows remain clickable and its list scrolls. Input focus and business-specific
completion stay in the caller; query edits filter and reopen rather than dismiss.
`docs/UI_POPOVER.md` documents reuse. GPUI test-support is a dev dependency only.

## Verification

- `src/ui/components/popover.rs::overlay_blocks_underlying_hover_clicks_and_scroll`
- `src/ui/components/popover.rs::keyboard_scrolls_to_selected_row_and_escape_dismisses`
- `src/ui/components/popover.rs::keyboard_lifecycle_preserves_ime_and_stable_selection`
- `src/ui/actions/status.rs::status_popover_visibility_rules`
- `src/ui/views/status.rs::popover_keyboard_navigation_wraps_and_survives_filter_changes`

Proved: removing only the surface's `occlude()` makes the actual GPUI hit-test
regression fail at "popover must occlude table hover" in
`docs/agent-notes-evidence/2026-10-11-popover-occlusion-red.log`. Restoring it and
immediately running the same focused test passes in
`docs/agent-notes-evidence/2026-10-11-popover-occlusion-green.log`.
The fixture also verifies left/right click isolation, child scrolling, ancestor
scroll isolation, anchor exemption, outside destination clicks and resumed table hover.
A focused keyboard fixture verifies scrolling, Enter and Escape. Native Windows
pointer and IME interaction have not been manually exercised.

Pitfalls checked: P1-P52 read in this session. P2/P3 status geometry and truncation,
P30 vendored assets, P51 lifecycle and P52 shared keyboard candidates remain protected.
No deletion, uninstall, path protection, residual identity or command execution changes.
P53 points here.

Validation: cargo fmt --check, cargo clippy --all-targets -- -D warnings and
cargo build --target x86_64-pc-windows-msvc pass. Full cargo test --lib outside the
sandbox passes 719 tests with 9 existing ignores. Strict note anchors, pitfall
pointers, UI localization and git diff --check pass. The running default debug
executable is left intact; the independent target contains the rebuilt executable.
