# Agent Note: macOS-only logic tests now run on Windows

Status: implemented

## Problem

The macOS native acceptance row cannot be closed on this Windows host (no macOS C
toolchain/SDK — the cross-check fails at `ring`; the CI `test-macos` job can't be
triggered under the no-push constraint). But some macOS-only code is **platform-neutral
logic** that was needlessly compiled only in the macOS test binary, so it was never
exercised by the Windows suite.

## Decision

Made two macOS-only pure-logic items test-visible so their tests also run on Windows:

- `categories::macos::parse_snapshot_names` (`target_os = "macos"` → `any(target_os =
  "macos", test)`) and its test module (`all(test, target_os = "macos")` → `test`). It
  parses `tmutil listlocalsnapshots` output (drop header/blank lines, keep single-token
  snapshot names) — pure string logic.
- `rules::directories::append_fixture` (`all(test, target_os = "macos")` → `test`) and
  `categories::system::push_log_dir_targets` / its test (`all(test, target_os = "macos")`
  → `test`). The logs layout is driven by the declarative `directory_selection`, so the
  macOS `~/Library/Logs` split logic runs identically on Windows.

This mirrors the earlier browser-function treatment (`cfg(any(target_os = "macos",
test))`), which is why `declared_leaves` etc. already run on Windows.

## Alternatives considered

Cross-compiling for `x86_64-apple-darwin` is blocked by the missing macOS `cc`/SDK, so it
can't run the macOS binary. Leaving these tests macOS-only keeps real, portable logic out
of the Windows suite for no reason. Making whole macOS platform modules test-visible is
not possible (they call macOS-only APIs). All rejected.

## Consequences

Three more macOS-logic tests run in the Windows suite (snapshot-name parsing ×2, the
`~/Library/Logs` owner-split). This raises coverage of macOS logic without a macOS host,
but it is **not** macOS native acceptance — the Windows binary still can't prove the
macOS platform adapters behave on macOS.

## Verification

- `src/core/categories/macos.rs::snapshot_names_skip_header_and_blank_lines`
- `src/core/categories/macos.rs::snapshot_names_empty_output_yields_nothing`
- `src/core/categories/system.rs::logs_are_split_by_owner_and_hazards_stay_unpreselected`

Coverage improvement, not an organic bug fix; no red-run proof claimed. Actual unified
gate results and limitations are recorded in RULES_REFACTOR_STATUS.
