# Agent Note: Two CI-only failures were environment-dependent assertions, not regressions

Status: implemented

## Problem

The first full run after the notes workflow gained the vendored tool's own suite
(run 37465106555, commit cf9654f) went red on **both** platforms, while the parent commit
had been green twelve minutes earlier:

- **macOS** — `production_target_table_is_deterministic_and_duplicate_free` compared the
  full path set of two consecutive `all_targets(None)` calls and failed with
  `固定目标表逐次一致`. The sets differed by exactly one path: a
  `Library/Group Containers/com.apple.siri.…` cache directory that macOS itself created
  between the two calls. The table is a function of (rules × the filesystem at call
  time); live enumeration of the OS's own cache directories is inherent to scanning, so
  strict cross-call set equality is not a property the product has — the assertion that
  introduced it over-reached.
- **Windows** — `windows_background_command_has_no_console_and_keeps_output_and_exit_code`
  panicked on `.expect("Windows console probe must run")`: `CreateProcess` of
  `powershell.exe` returned "the system cannot find the file specified" on a runner that
  was visibly starved (two unrelated tests reported "running for over 60 seconds" in the
  same job). The same commit's re-run is green in 6m41s, so the failure was environmental
  — but the panic message carried none of the information needed to see that.

## Decision

- Keep the table's environment-independent invariants, and say the scope in the test:
  (1) no duplicate physical path in one table, (2) **the same path always receives the
  same rule, operation, disposal, category, recommendation and label** across calls.
  Drop the cross-call set equality; the comment records the CI evidence so nobody
  "restores" it. Paths, not `size_hint`, form the identity — a Docker image size is a real
  reading that legitimately changes between calls.
- Keep the console probe a hard failure — a probe that did not run is not a pass — and
  make the panic print `where.exe`'s resolution of `powershell.exe` plus `PATH`, so the
  next red is diagnosable on the spot. No retry: a retry would also mask a genuine
  "powershell left the image" regression.

## Alternatives considered

Retrying the probe once on spawn failure — rejected, as above. Keeping the strict equality
and only asserting when the filesystem is quiet — unfalsifiable; the OS can always create
a group container between two calls. Comparing only paths present in both calls and
asserting the difference stays under a threshold — an arbitrary tolerance with the same
fragility. Asserting a golden path list — machine-dependent, which this repository already
rejected once for a fixed target count.

## Consequences

Genuine construction nondeterminism still fails the test: identical path mapped to a
different rule or disposal is asserted explicitly. Enumeration-dependent differences no
longer do, and the evidence for why lives in the test comment next to the assertion. The
console probe now prints its environment when it cannot run, which is the difference
between "flaky" and "we know what happened".

## Verification

- `src/core/categories/mod.rs::production_target_table_is_deterministic_and_duplicate_free`
- `src/core/proc.rs::windows_background_command_has_no_console_and_keeps_output_and_exit_code`

Proved: the old strict equality failed on a real macOS runner with a diff of exactly one
path that the OS created while the test ran (run 37465106555, CI log). The same defect is
reproducible locally: a temporary probe that creates one child directory of the user's
home cache (~/.cache) between the two calls failed with that one path as the entire diff,
while the narrowed invariants passed under the identical perturbation
(docs/agent-notes-evidence/2026-10-06-ci-flake-scope-probe.log). The Windows spawn failure
did not reproduce — the same commit's re-run is green — which is what identified it as
environmental; the diagnostic change is what makes a recurrence decidable.

Supersedes `2026-10-06-fixed-target-table-invariants.md`, which owned the assertion this
note narrows; the archived note keeps the history and the duplicate-freedom half of its
decision.
