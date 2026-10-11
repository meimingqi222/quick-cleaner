# Manual UI acceptance matrix

Screenshots are manual evidence, not automatic visual assertions. Missing evidence
remains pending. The pre-migration screenshot set was not captured, so gallery
images cannot prove that every migrated page retained its original appearance.

Fixture: `cargo run --example ui-gallery --features ui-gallery`; add `--en`,
`--menu` or `--dialog`. No settings writes, scans, cleanup or uninstall actions.
Real-page fixtures read existing settings/platform metadata when constructing Root,
then replace displayed data with empty or synthetic state; product startup tasks
are not launched.
Default window: 1000 × 760 logical pixels. Capture platform, language, outer and
client dimensions, DPI, fixture arguments, date and reviewer observations.

| State | Windows | macOS |
| --- | --- | --- |
| Gallery zh/en: normal, busy, disabled, selected row, long text, meter, 10k list | captured; manual interaction pending | pending |
| Gallery menu, excluded anchor, outside dismissal | captured; manual interaction pending | pending |
| Gallery modal and background isolation | captured; manual interaction pending | pending |
| Dashboard: idle, scanning, empty, error | empty-state capture checked; remaining states pending | pending |
| Status: loading, all subtabs, short card row, process grouping/detail | empty-state capture checked; remaining states pending | pending |
| Status input: edited query, candidates, arrows/Enter/Esc, blur | pending | pending |
| Apps: loading, empty, filter, selected rows, menu, uninstall progress | empty-state capture checked; remaining states pending | pending |
| Junk: loading, empty, per-category scroll, partial/tri-state, confirmation | empty-state capture checked; remaining states pending | pending |
| Disk: loading, empty, error, volumes, selection, confirm | empty-state capture checked; remaining states pending | pending |
| Declutter: overview, downloads, large files, duplicates, photos, menu | empty-state capture checked; remaining states pending | pending |
| Search: indexing, empty, error, results, filter, selected rows | empty-state capture checked; remaining states pending | pending |
| Dialogs: confirm, residual, FDA, updater available/downloading/error | pending | pending |
| Minimum supported window, long zh/en copy, high DPI | pending | pending |
| Native Chinese IME: commit/cancel, candidate position after move/resize/DPI, switch/destroy | pending | pending |

Native verification steps: focus each input, enter Chinese and emoji, replace a
selection, paste, delete, move caret, accept/cancel composition, then navigate away
and back. During composition, arrows/Enter/Esc must belong to IME. Otherwise the
candidate list cycles and scrolls, Enter accepts the visible selected item, first
Esc closes candidates and a later Esc follows the existing input clear behavior.
Move/resize the window and change monitor DPI; native candidate windows must remain
at the caret. Open a modal above an input; background edits must be blocked. Close
through its explicit button; valid source focus should be restored unless another
control intentionally acquired focus.

For list acceptance use safe fixtures containing 10,000 synthetic rows. Compare
the viewport render count, drag outside the container, release outside, and confirm
another list/category retained its offset. Refresh/sort/filter and compare stable
identity behavior to the product baseline. No destructive live-data operations are
needed for visual acceptance.

## Windows component captures

2026-10-12, DPI 144 (150%), gallery 1000×760 logical client pixels; capture
1504×1187 physical pixels including native frame. Screen capture restored focus.
Files: windows/gallery-zh.png, windows/gallery-en-menu.png,
windows/gallery-zh-dialog.png. All three were opened and visually inspected: text,
selected controls, menu placement, modal card and visible rows render correctly.
Synthetic fixture labels are not product copy. These are post-migration images.

Real-page static fixtures use --page=0..6, --loading and --page=1 --candidates;
1280×880 logical client pixels. They block pointer access to business actions and
do not start product startup tasks. No pre-migration images are available.

Seven empty/synthetic page images (*-empty.png) were inspected as a contact sheet
on 2026-10-12, DPI 144, 1924×1367 physical capture pixels. Cards including incomplete
rows, table headers/footers, navigation and empty placeholders render correctly.
Candidate/loading screenshots were discarded: unrelated system IME windows/input
contaminated screen capture. Non-foreground PrintWindow returned no usable GPU image.
Those cases remain pending; the candidate and ordinary scrolling behavior is covered
by real GPUI tests, which do not substitute for native IME/visual acceptance.
