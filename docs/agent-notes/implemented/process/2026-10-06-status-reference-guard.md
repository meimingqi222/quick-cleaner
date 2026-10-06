# Agent Note: Status-record references are checked against real tests and code

Status: implemented

## Problem

The matrix mapping in `docs/RULES_REFACTOR_STATUS.md` cites test names, fixtures and
evidence paths as the proof for each acceptance row, but nothing verified those
references. A manual exhaustive audit found one live example: the 范围安全 row cited
`deep_preserved_paths_recurse_and_keep_siblings`, a test that did not exist (only the
one-level split was covered). The note anchors are guarded by
`verify-notes.py --strict-anchors`, but status tables were not: a renamed or deleted test
could silently rot a row's evidence.

## Decision

`scripts/check-status-references.py` extracts every backticked snake_case identifier
from the status record and requires each to resolve to one of:

- a current test name, compared against `cargo test --list` output (CI feeds it the
  union of the Windows and macOS lists, which the `verify-anchors` job already downloads);
- an identifier found in `src/`, `rules/`, `scripts/`, `tools/`, `vendor/`,
  `Cargo.toml` or `build.rs` (function names, config keys, dependency names);
- an entry in `scripts/status-reference-baseline.json` — historical recovery points that
  accurately describe deleted implementations (the removed remote-rule channel, replaced
  helpers) plus commit hashes, each with a written reason.

New unresolved references fail the run. The baseline is documented as "only additive for
honest history, never to paper over a typo". The audit that motivated the script also
**added the missing test** it found (`deep_preserved_paths_recurse_and_keep_siblings`,
now covering the `split_target` recursion branch) rather than just fixing the citation.

## Alternatives considered

Checking only recent sections — rejected: section boundaries move as batches are
appended, and the guard would silently stop covering older rows that are still cited by
the matrix. Allow-listing by name inside the script — rejected: reasons belong next to
the entries, in a data file, where a reviewer can judge them. Running it from the
agent-notes workflow (python-only) — rejected: it needs the platform test lists, which
only the Rust jobs produce.

## Consequences

A renamed or deleted test now breaks CI in the `verify-anchors` job instead of quietly
weakening a matrix row. The local equivalent is
`python -X utf8 scripts/check-status-references.py --tests <cargo test --list 输出>`
(~3 s locally, most of it the per-identifier source scan). `docs/RULES.md`'s submit
checklist lists both consistency guards.

## Verification

- Guard red/green: injecting `totally_fake_test_name_that_does_not_exist` into the
  status record fails the run with that exact name
  (`docs/agent-notes-evidence/2026-10-06-status-reference-guard-red.log`); restoring the
  file passes with `OK: 321 identifier(s) resolved (609 test names)`
  (`...-green.log`).
- CI: `ci.yml`'s `verify-anchors` job runs the guard against both platform test lists
  (YAML parses; the job already downloads those artifacts for the anchor check).

Script/CI addition, not an organic bug fix; the guard's own red/green is the evidence.
macOS native acceptance remains the standing external condition. Actual unified gate
results are recorded in RULES_REFACTOR_STATUS.
