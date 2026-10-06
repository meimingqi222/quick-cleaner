# Agent Note: Archived-note seals survive a checkout, and digest migrations are explicit

Status: implemented

## Problem

`archived/manifest.json` sealed each archived note with `sha256(path.read_bytes())`.
The blobs live in git with LF endings, but a Windows checkout (`core.autocrlf`) writes
CRLF into the working tree — so a manifest sealed on Windows failed on the Linux CI
runner with "archived note was modified after sealing", for content nobody had touched.
The first push of this repository's sealed notes hit exactly that: the notes workflow went
red on three notes while the identical command passed locally.

A second, latent defect: `--seal` only appended entries for notes missing from the
manifest, and refused to run at all while any note was reported modified. With digests
that are checkout-dependent, that made the EOL migration unrepairable through the
documented path.

## Decision

- `note_digest` hashes the **LF-normalised** bytes, so line endings are never mistaken
  for a modification; the seal means "content unchanged", not "bytes unchanged".
- `--seal` stays **append-only** (adds missing entries, prints each one, never rewrites a
  recorded hash). A note modified after sealing still stays red — fix forward with a new
  note, as the skill documents.
- New `--reseal` is the explicit escape hatch for **tooling migrations** (a
  digest-algorithm change, an EOL rollout): it rewrites the entries whose digest no longer
  matches and prints `resealed <path>` for each, so the rewrite is visible in review.

The fix lives in the vendored tool `tools/regression-notes/verify-notes.py` and in its
source of truth, the regression-notes skill (`regression-notes/scripts/verify-notes.py`,
version 0.4.2), kept byte-identical.

## Alternatives considered

Deleting the manifest and re-sealing from scratch — rejected: it discards the whole
seal record to repair three entries. Rewriting hashes inside plain `--seal` — rejected:
it would let "run --seal" launder a genuine post-seal edit; the explicit `--reseal`
flag keeps the append-only guarantee and makes the migration auditable. Making the
comparison ignore CRLF only on Windows — rejected: the manifest is shared across
checkouts, so the digest itself has to be checkout-independent.

## Consequences

A repository can seal notes on Windows and verify them on Linux/macOS (and vice versa).
Recovering from a future digest-format change is one documented command whose output
lists exactly which entries moved. The repo's own manifest was re-sealed once with
`--reseal` during this migration; the archived notes themselves were not edited (the
seal exists to prevent that).

## Verification

- Repo-side regression suite: `tools/regression-notes/tests/test_seal_digest.py` — the
  LF/CRLF digest stability, append-only `--seal` versus explicit `--reseal`, and the
  archived-note exemption from `--dump-anchors`; red against the pre-fix tool and green
  against the fixed one in the same run
  (docs/agent-notes-evidence/2026-10-06-seal-digest-inrepo-test.log). CI runs it in the
  notes workflow, next to the verifier it guards.
- Skill test suite (source of truth for the tool; run in
  `D:\code\my-agent-skills\regression-notes`): 85 tests OK, carrying the same three cases
  red-then-green (docs/agent-notes-evidence/2026-10-06-seal-digest-skill-suite.log); both
  installed copies under the agents skills directories were re-synced and run green.
- Repo side: `tools/regression-notes/verify-notes.py --notes-dir docs/agent-notes
  --strict-anchors` passes on the CRLF working tree and on an LF copy that simulates the
  CI checkout (evidence docs/agent-notes-evidence/2026-10-06-seal-eol-fix.log).
- Field evidence: the notes workflow run 37445291328 failed on three sealed notes before
  this fix and runs 37460107300/37461367172 pass after it
  (docs/agent-notes-evidence/2026-10-06-seal-eol-ci-red.log).

Proved: before the fix the CI notes workflow failed with three
"archived note was modified after sealing" errors while the identical
`--strict-anchors` command passed on the Windows working tree — the digest compared raw
bytes, so the checkout's line endings read as a modification
(docs/agent-notes-evidence/2026-10-06-seal-eol-ci-red.log, run 37445291328); after the
LF-normalised digest and the one-off re-seal the same workflow passes (run 37460107300)
and the repo's own strict run is green on both a CRLF tree and an LF copy
(docs/agent-notes-evidence/2026-10-06-seal-eol-fix.log). The manufactured red that pins
this now: `tools/regression-notes/tests/test_seal_digest.py` run against the pre-fix tool
fails all three cases with the same "modified after sealing" symptom, and passes against
the fixed tool (docs/agent-notes-evidence/2026-10-06-seal-digest-inrepo-test.log).

Bug-fix note. The regression test is cited as a path, not bound as a `path::anchor`: the
anchor intersection in CI unions the Rust test lists, so a Python test anchor could never
resolve there.
