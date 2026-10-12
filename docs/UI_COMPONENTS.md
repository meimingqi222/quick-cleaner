# UI components

`components/` owns reusable rendering and local interaction. It must not import
`Root`, `core` or `platform`. `adapters/` translates domain data and localized
labels into component props and translates events into existing actions. `shell/`
owns navigation, focused input routing and overlay ordering. Business state stays
in Root's existing child states.

## Choose an existing component first

| Capability | Component | Actual callers |
| --- | --- | --- |
| Actions, disabled/busy state | Button / IconButton | dialogs, Apps, Search, page toolbars |
| Status labels, narrow clipping | Badge | status cards, Apps rows, residuals |
| Row layout and interaction slots | ListRow | Apps, Junk, Search, process rows |
| Selection controls | SegmentedControl / ToggleChip | shell tabs, Apps filters, status filters |
| Determinate progress | Meter / ProportionBar | status, disk, cleanup progress |
| Menus and edge placement | Menu / MenuItem | Apps, Declutter, disk volumes |
| Modal shell | DialogFrame | confirmation, residuals, FDA, update |
| Single-line edit and IME state | InputHost / InputBinding / search_box | Apps/dev environments, file search, status |
| Anchored candidate interaction | PopoverState / popover_surface / popover_list / popover_row | status candidates; shared primitives tested independently |
| Fixed-height virtualization | virtual_list / ScrollArea | Apps 54px, Junk 36px, Search 34px, processes 32px |
| Table slots | table_shell | Apps, Search |
| Ordinary variable-height scrolling | ScrollView | disk browser, development environment package list |

Colors in views, adapters and shell come from `theme.rs` tokens; the guard rejects
hex literals there. `Button::small` picks a closed `ButtonTone` and `Badge` a
`BadgeTone`; add a tone instead of passing raw colors. `BadgeTone::custom` remains
only for data-driven colors (safety level, status value).

Preserve domain rows and query parsers in their current layers. Do not add a new
business enum to a component to distinguish pages. Extract behavior only when
real callers need it; do not manufacture a second domain consumer for a one-off.
Popover's occlusion and regression contract remains in [UI_POPOVER.md](UI_POPOVER.md).

## Input ownership

The Entity spike was not accepted: native Windows and macOS IME evidence is missing.
Production therefore uses the planned InputHost fallback. Adapters bind independent
text state, edit callbacks and geometry. The shell only forwards platform input to
the registered binding whose focus handle is focused. No focused binding means no
edit. Composition updates redraw without filtering; committing calls the adapter
once. Programmatic assignment does not dispatch an edit event.

InputTarget and the former adapter renderer were removed. There is no alternate
production Entity implementation. The low-level Unicode layout/edit helpers remain
in `text_input.rs`; platform routing is in `shell/input.rs`.

## Overlays and scrolling

OverlayStack determines both Esc target and deferred render priority. IDs update
in place, reopen on top and preserve initial focus snapshots. Parent removal returns
descendants first. A non-dismissible or Persistent top blocks Escape from reaching
lower layers. IME retains Escape during composition. Outside dismissal checks the
top layer and lets destination clicks continue; modal surfaces occlude the background.
Page navigation clears transient menus, input registration and stale composition.

ScrollArea owns keyed drag state and window-level move/up capture. Virtual lists
still render only the supplied visible range and keep page/category scroll handles.
Ordinary GPUI overflow containers remain distinct from uniform_list. The common
scrollbar controller must be used when adding a custom scrollbar; pages may issue
scroll-to-item/top commands but may not wire low-level dragging.

## Development and gates

Run `cargo run --example ui-gallery --features ui-gallery -- --en --menu`.
`--dialog` shows a synthetic modal. The example uses a 10,000-row synthetic list,
has no scans or destructive actions, and is excluded from default/release bundles.
`--page=0` through `--page=6` shows real pages populated with empty/synthetic state;
pointer actions are blocked. Add `--loading` for loading states, or
`--page=1 --candidates` for threshold suggestions. These fixtures do not start the
normal product startup tasks. Root construction reads settings/platform metadata,
then visible volume capacities and exclusions are cleared in memory.

Run `python -X utf8 scripts/check-ui-components.py` and
`python -X utf8 -m unittest discover -s scripts -p test_ui_components.py`.
CI and the local pre-commit hook run both. The legacy baseline is empty and cannot
grow. Comments/literals are masked; aliases, nested imports and interactive masks
are checked. Keep the manual platform matrix in `ui-baselines/README.md` honest:
screenshots establish manual evidence, not an automated visual test.
