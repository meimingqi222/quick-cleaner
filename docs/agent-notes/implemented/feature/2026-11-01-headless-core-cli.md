# Agent Note: Headless core CLI keeps fresh scan authority and process output

Status: implemented

## Problem

The product previously exposed only GUI startup and helper/version modes.
Testing real Smart Clean required driving the desktop, and disk/search tests
could not exercise the same binary headlessly. Routing commands through the
GUI's Windows startup would detach a child, lose redirected JSON and exit
codes, or activate an existing window instead of completing the command.

Initial CLI tests also exposed two reporting defects: cancelled directory
acquisition emitted no JSON, and failed help/category output returned success.

## Decision

`cmd/quick-cleaner/main.go` dispatches the explicit `cli` namespace before GUI
instance acquisition and elevation. `cmd/quick-cleaner/console_windows.go`
preserves the parent process and its working pipes for this namespace, including
GUI-subsystem builds. Ordinary GUI launches retain their existing lifecycle.

`internal/cli/cli.go` owns parsing, reports and exit codes without importing the
UI/app layer. Smart Clean in `internal/cli/junk.go` uses the product providers,
rules, build discovery, central whitelist and occupancy observations. Preview
is the default; mutation additionally needs `clean --execute` and an explicit
exact-path/recommended/all selection. Only scan-produced items become targets
via `ScanItem.CleanTarget`; serialized diagnostics cannot recreate execution
authority. `internal/cli/junk_execute.go` calls the existing verified cleaner
and records real outcomes. Persisted exclusions must parse successfully.

Rule-blocked items are unselectable. An explicit `--path` naming one returns
a usage error; bulk selection skips it and contributes to `skipped_count`. Occupancy is a badge and never
changes selection; see `2026-11-01-junk-inuse-occupancy-and-retry.md`.
Unknown paths still return usage errors. Execution-time
occupancy is classified per leaf by `CleanProgress.SplitFailures` in
`cleanupOutcome`: only in-use/unverified leaves move to `skipped_items` and
`skipped_targets`. Mixed targets retain real errors in `target_failures`;
raw core evidence is unchanged. Expected skips return exit 0, while true
failures/manual action or incomplete discovery return exit 3. An execution
with no selected targets, or where every target was skipped as in use, does
not write a history entry.

`internal/cli/tree.go` acquires a bounded live directory tree for explicit roots;
aggregation and name search remain core SizeTree operations. Native volume
mode calls the existing fresh platform scan, never silently falls back or
requests elevation. `internal/cli/native_supported.go` uses a disposable cache
for large macOS spill scans rather than changing the user's index. CLI live
acquisition reports blocked paths and cancellation as partial coverage. Output
failure always overrides a successful command exit.

## Alternatives considered

- Driving GUI actions from CLI: couples tests to desktop lifecycle and
  confirmations, and makes pipes and synchronous completion unreliable.
- Importing saved plan JSON or accepting arbitrary delete paths: loses frozen
  ownership, identity and operation evidence. Every invocation rescans instead.
- Reusing the GUI's persistent index owner in this first CLI: would introduce
  persistence and cache lifecycle into read-only tests. Live discovery and
  explicit native scans exercise real primitives, without claiming identical
  cached performance.
- Returning success after cancellation or emitting an unmarked empty tree:
  scripts could treat unknown coverage as an empty directory. Partial reports
  and distinct nonzero exits preserve that distinction.

## Consequences

The CLI covers Smart Clean scan/plan/explicit execution, disk analysis and
filename search. It does not manage running processes or expose arbitrary
deletion. Real cleanup is supported only on Windows/macOS. Concurrent filesystem
changes still go through core identity/occupancy checks; this headless entry is
independent of GUI instance ownership. Live directory sizes and native volume
accounting have the documented platform differences in `docs/CLI.md`.

## Verification

- `internal/cli/cli_test.go::TestJunkScanPlanAndExplicitExecution`
- `internal/cli/cli_test.go::TestJunkSelectionRequiresFreshExactEligibleTarget`
- `internal/cli/cli_test.go::TestInvalidCommandsRefuseBeforeScanning`
- `internal/cli/cli_test.go::TestTreeCommandsUseRealInventoryAndProductSearch`
- `internal/cli/cli_test.go::TestTreeCancellationStillWritesJSON`
- `internal/cli/cli_test.go::TestTreeDoesNotFollowLinks`
- `internal/cli/cli_test.go::TestOutputErrorsAreFailures`
- `cmd/quick-cleaner/main_test.go::TestCLIDispatchDoesNotInitializeGUI`
- `cmd/quick-cleaner/cli_windows_test.go::TestCLIConsoleAndGUISubsystemKeepPipesAndExitCodes`
- `internal/cli/junk_windows_test.go::TestJunkLockAfterScanIsSkippedWithoutFailure`
- `internal/cli/cli_test.go::TestUnsupportedCleanupExecutionIsUsageError`

Proved: unsupported `--execute` returned exit 1 on Linux while the CLI contract
requires usage exit 2; see
`docs/agent-notes-evidence/2026-11-01-cli-unsupported-exit-red.log`. After
classifying the platform gate as `ExitUsage`, the cross-platform test passes; see
`docs/agent-notes-evidence/2026-11-01-cli-unsupported-exit-green.log`.

Proved: the initial focused run failed `TestTreeCancellationStillWritesJSON`
with `unexpected end of JSON input`, and `TestOutputErrorsAreFailures` reported
exit 0 for broken help/category writers; see
`docs/agent-notes-evidence/2026-11-01-cli-report-red.log`. After retaining the
partial tree and propagating write errors, the focused packages pass; see
`docs/agent-notes-evidence/2026-11-01-cli-report-green.log`. The isolated lifecycle
test uses real rules and cleanup against a disposable trusted-home project,
verifies previews do not mutate or persist history, rejects malformed settings
and unscanned paths, honors both exclusion sources, and preserves source files
after explicit cleanup. Windows subprocess tests build both console and GUI
subsystems and assert pure JSON on stdout plus exit 2 for invalid commands.

Proved: before skip classification, a real handle acquired after the frozen
scan produced `Failed:[...]` and `Skipped:0`; the late-lock assertion failed
in `docs/agent-notes-evidence/2026-11-01-smart-skip-red.log`. The same focused
test passes in `docs/agent-notes-evidence/2026-11-01-smart-skip-green.log`.
The lifecycle test also holds a discovered cache file open before a fresh
invocation, verifies exit 0, zero selected bytes, one skipped target and no
history, then closes it and verifies actual deletion.

The same isolated Windows lifecycle also executes `--all` with two real
discovered projects: one holds a cache file open, the other has an unlocked
cache. It requires exit 0, both targets selected, the unlocked one actually
deleted, the held tree skipped at execution, no failures/manual actions, and
unchanged locked data and project sources. The persisted history names both
attempted targets with failed=0 and skipped>0. CLI and GUI selection tests
require All to pick optional and occupied items and to skip only rule-blocked
ones. Focused race verification: `go test -race ./internal/app ./internal/cli
-run 'TestJunkBusyItemsStayRecommendedAndSelectable|TestJunkSelectionRequiresFreshExactEligibleTarget|TestJunkScanPlanAndExplicitExecution' -count=1`.
