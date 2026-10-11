# UI migration status

This is the current migration record. Component APIs and real callers are listed in
[UI_COMPONENTS.md](UI_COMPONENTS.md); manual evidence is tracked in
[ui-baselines/README.md](ui-baselines/README.md). The detailed original inventory
is preserved locally in docs/temp/UI_MIGRATION_INVENTORY_2026-10-11.md.

## Batch gates

| Batch | Code outcome | Acceptance status |
| --- | --- | --- |
| 0 | Page/control inventory and all-list investigation done; original guard violations recorded; Entity spike rejected for missing native evidence | Complete native IME and pre-migration visual evidence unavailable |
| 1 | components/adapters/shell boundaries; business dialogs/progress moved; shared Button/IconButton/Badge/ListRow/Segmented/Meter callers migrated | Automated component tests pass; page visual matrix pending |
| 2 | Shell OverlayStack registers IDs, parent, policy, focus target, close callback; all menus/dialogs and status candidates use stack render priorities | Stack/actual GPUI occlusion tests pass; native matrix pending |
| 3 | InputHost chosen; independent adapter bindings for Apps/dev, file search and status; focused platform routing in shell | Common editing/IME regression passes; native IME still pending |
| 4 | Four uniform_list callers share virtualization/ScrollArea; disk/package ordinary lists use ScrollView; Apps/Search use table_shell | Actual GPUI visible-range and drag/independent-state tests pass |
| 5 | Old input renderer/enum, per-page drag fields, old guard/baseline removed; status split; docs/gallery and zero-exception CI/local guard | Code migration complete; final visual/platform acceptance remains open |

The plan's final acceptance gate has NOT been reached. No native macOS evidence,
no complete before/after page screenshots, and no dual-platform native IME proof
are claimed. Entity input is not a second active production path.

## Behavior ownership and deletion checklist

- Generic controls have no Root/core/platform imports. Domain software/process/file
  row rendering stays with its view; reusable rows own layout and interaction slots.
- Apps, file and status input use the same component edit implementation. InputTarget
  and adapters/search_box.rs are deleted; Root's platform handler delegates to the
  focused binding, with no fallback to an unrelated field.
- OverlayStack is in shell. Rendering and Esc agree on the top layer; registration
  updates in place, children close first, lost parents clean their descendants and
  composition retains IME keys. Outside clicks reach destination controls; modal
  backdrops and popup surfaces retain occlusion. Menus and all dialogs use shared shells.
- Apps/Junk/Search/Status retain uniform_list, row heights 54/36/34/32px, and only
  build visible ranges. Category/list handles remain independent. Old drag fields and
  view-level scrollbar math/capture are deleted. Ordinary scrolling stays distinct.
- Menu copy uses tr_*; inline language matching was removed from menu views.
- status.rs now contains entry/original test anchors; cards, process rows, suggestions
  and table live under views/status/. Includes retain existing GPUI test paths.
- check-ui-components.py replaces the earlier boundary script. Baseline has zero
  exceptions and cannot grow. CI and local hook execute it and its regression tests.
- No core/platform query, deletion, scan, cleanup, uninstall or safety behavior changed.

## Retained list constraints

| Caller | Virtualization | Height | State and loading |
| --- | --- | --- | --- |
| Apps | shared virtual_list → GPUI uniform_list | 54px | apps.scroll; existing icon-cache strategy retained |
| Junk | shared virtual_list per category | 36px | category handles and keyed drag IDs remain independent |
| Search | shared virtual_list | 34px | search.scroll; existing file visual kind |
| Status processes | shared virtual_list | 32px | monitor.proc_scroll; visible-range process icon loading retained |
| Disk browser / package list | ScrollView, no virtualization | variable | component-keyed handles; bounded/existing data construction |
| Other ordinary overflow containers | GPUI normal scrolling | variable | no copied low-level scrollbar dragging |

Validation records and red/green priority evidence are attached to
docs/agent-notes/implemented/architecture/2026-10-11-ui-component-ownership.md.
