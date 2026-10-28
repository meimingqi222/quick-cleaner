# Agent Note: Shared UI behavior has an owner and actual migrated callers

Status: implemented

## Problem

Text editing selected pages through InputTarget inside Root, four virtualized lists
wired scrollbar dragging individually, and fixed deferred priorities could disagree
with the shell's Escape stack. Status search bypassed that stack entirely. Adding
component files without deleting callers would leave these independent paths active.

## Decision

Shared behavior now lives in sibling gpui-app-kit (input/text, scroll/scroll_area, overlay/overlay_stack).
QuickCleaner retains themed adapters, its OverlayId and Root business close routing.
Production uses InputHost bindings, because the Entity spike lacks both native
Windows and macOS IME acceptance. Components own text state/edit behavior and
rendering; adapters bind existing Root child state and business callbacks; shell
routes platform input to the focused registration. Preedit redraws without
filtering; commit notifies once. Programmatic assignment does not recurse.

OverlayStack stores stable identity, parent, policy, close callback and initial
focus target. Rendering and Escape share stack order. Children close before parents;
orphaned children cannot register themselves again in the same frame. Modal focus
prevents background edits; outside dismissal retains destination click delivery.
Existing popover occlusion and P51-P54 tests remain intact.

ScrollArea owns keyed drag state and window capture. All four virtualized lists
share its controller, preserve fixed row heights and visible-range rendering, and
retain independently owned page/category scroll handles. ScrollView supports
ordinary variable-height content and uses that controller in disk and package lists.
The old adapter input renderer, InputTarget, per-page drag state and old boundary
script/baseline are deleted. The new guard has zero exceptions and cannot grow.

## Alternatives considered

Shipping Entity input on simulated tests alone violates the native acceptance gate.
Moving business state into components changes scope. Replacing uniform_list with
whole-list rendering loses virtualization. Fixed modal/popup priorities cannot
represent a later child popup over a modal. Permanent exception growth hides a
failed migration. These alternatives are rejected.

## Consequences

New fields provide a binding rather than editing a page enum. Domain rows and query
semantics remain in their existing layers. The feature-gated gallery supplies safe
synthetic controls and a 10,000-row list. Windows gallery screenshots establish
limited manual evidence; the complete page matrix and native IME/platform checks
remain pending and are documented in docs/ui-baselines/README.md.

## Verification

- `src/ui/shell/overlay_stack.rs::nested_close_is_children_first_and_render_order_matches_escape`
- `src/ui/shell/overlay_stack.rs::disappearing_parent_removes_open_children_without_registering_them_again`
- `src/ui/shell/overlay_stack.rs::escape_closes_only_the_top_popup_and_modal_swallows`
- `src/ui/shell/overlay_stack.rs::closed_overlay_does_not_restore_an_input_from_the_previous_page`
- `src/ui/components/popover.rs::overlay_blocks_underlying_hover_clicks_and_scroll`
- `scripts/test_ui_components.py::test_baselines_can_shrink_but_cannot_grow_or_transfer`

Proved: neutralizing only the stack priority offset makes the nested/render-order
assertion fail in `docs/agent-notes-evidence/2026-10-11-ui-stack-priority-red.log`.
Restoring the function and immediately repeating the same focused test passes in
`docs/agent-notes-evidence/2026-10-11-ui-stack-priority-green.log`.

PITFALLS P1-P55 were read. P2/P3 card geometry and badge clipping are retained;
P51-P54 lifecycle, keyboard candidates, occlusion and threshold completion remain
covered. Core/platform deletion, uninstall, safety and query logic were not changed.
Final Windows full library regression passed 787 tests with 9 existing ignores
and zero failures (`cargo +1.99.0 test --lib -- --test-threads=1`). The strengthened
previous-page focus test also passed independently. Rust 1.99 format checks,
including the four included status files, clippy all-targets with ui-gallery,
and the separate Windows product build passed. Component guard has zero exceptions;
its five tests and the seven regression-note tool tests passed. Localization,
strict note anchors and PITFALLS pointers passed (eight existing missing-pointer
warnings). Native macOS and complete before/after visual acceptance are not claimed.
