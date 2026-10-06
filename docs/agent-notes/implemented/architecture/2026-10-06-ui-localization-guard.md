# Agent Note: All UI copy lives in i18n; a guard bans new inline strings

Status: implemented

## Problem

AGENTS.md requires UI copy to go through `ui/i18n.rs`'s `tr_*` functions, but 56 inline
`Language::Zh => … / Language::En => …` matches were still scattered across nine view and
component files (dialogs, the junk list, the disk volume picker, disk lens panes,
dashboard and the installed-apps list). The declutter entrance's status line was still
inline, and its confirmation dialog's doc comment claimed to show the scope while the
code only showed a count. The GOAL UI row asks for localization evidence, and inline
matches provide it only by accident — the next edit can forget the second language.

## Decision

All 56 sites moved into `tr_*` functions: view copy into the new `src/ui/i18n/views.rs`
(re-exported like the other i18n submodules), confirm-dialog action labels and residual
result copy into `src/ui/i18n/mod.rs`, and the two declutter status lines into
`src/ui/i18n/declutter.rs`. The declutter confirmation now appends the real scope list
through the same `confirm_scope_detail` the disk-lens entrance uses, so the entrance
shows what it said it shows.

`scripts/check-ui-localization.py` scans `src/ui/**` (excluding `i18n/`) for inline
`Language::Zh =>` arms, compares against `scripts/ui-localization-baseline.json`, and
fails when a file grows past the baseline or a new file introduces inline copy. The
baseline is now empty (`{}`), so the guard is a total ban; `--update` exists to record a
deliberate exception and is documented as "only to narrow". The script is wired into the
agent-notes workflow next to the pitfall-pointer check.

## Alternatives considered

Leaving views inline because both languages are present — rejected, that is exactly the
failure mode the rule exists to prevent. A Rust test walking source files — rejected, the
repo's existing consistency checks are Python scripts under `scripts/` wired into CI, and
`include_str!` per file would be brittle and does not scale to new files. A non-empty
baseline as permanent debt — rejected; the migration was mechanical and finishing it
removes the excuse for new inline copy.

## Consequences

`use crate::ui::i18n::*;` plus a `tr_*` call is now the only way to render copy, and any
regression is caught by CI before review. Copy changes are single-language edits in one
file. The disk_volume view gained the i18n import it never needed before.

## Verification

- `src/ui/i18n/views.rs::view_strings_are_localized_and_carry_their_values`
- `scripts/check-ui-localization.py` — reports `remaining baseline: 0` on the migrated
  tree (evidence in the batch gate log)

UI migration, not an organic bug fix; no red-run claimed. The guard failing on a
reintroduced inline string is the mechanical backstop. Windows-only evidence; macOS
native acceptance remains an external condition. Actual unified gate results are
recorded in RULES_REFACTOR_STATUS.
