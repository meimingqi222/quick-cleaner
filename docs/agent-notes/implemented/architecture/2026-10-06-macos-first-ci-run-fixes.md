# Agent Note: The macOS platform differences the first CI run surfaced

Status: implemented

## Problem

The first CI run on the pushed workspace executed the macOS job for the first time in this
repository's history. Build and clippy surfaced platform-only dead code, and the test run
surfaced 42 failures. Together they showed that "Windows green" had concealed three
classes of platform difference:

1. **Windows-only callers made shared helpers dead on macOS** (`flow.rs` capability
   executors, `plan.rs::observe_scan_paths`, an unused import and three orphaned doc
   comments in `platform/macos/residuals.rs`) — invisible because the Windows build never
   compiles the macOS modules and the macOS build never compiles the Windows callers.
2. **Authored-on-Windows expectations** in tests: the merge-order assertion hardcoded a
   backslash separator; a directory-link test removed a Unix symlink with `remove_dir`
   (ENOTDIR); byte accounting asserted the Windows logical size where Unix counts
   allocated blocks; the version-layout golden carried only the Windows layout rows.
3. **A discovery bug on any aliased root** (`facts::confined` walked the textual ancestor
   chain and rejected symlinked ancestors) — fixed separately in
   `2026-10-06-system-path-aliases-and-discovery.md`.

## Decision

- Gate platform-specific helpers by their real callers: `#[cfg(any(windows, test))]` for
  the items the cross-platform tests exercise, `#[cfg(windows)]` for those only Windows
  code calls (including each executor struct **and** its impl block).
- In `append_path_targets`, resolve the user-scoped roots per platform: Windows keeps its
  registry-based real-user resolution (independent of the `home` anchor), while macOS
  derives `~/Library/Caches` and `~/Library/Application Support` from the caller's home,
  so `home=None` means no user-scoped expansion instead of silently scanning the process
  account.
- Make the expectations platform-correct rather than platform-neutral: the separator comes
  from the joined path, the link is removed with `remove_file` on Unix, the byte
  expectation is split by platform, and the version baseline tags each row with its
  platform and is filtered to the current one (the macOS rows were added from the CI
  failure's own output).

## Alternatives considered

`#[allow(dead_code)]` on the macOS-dead helpers — rejected: it hides the signal instead of
expressing that the callers are Windows-only. Making the whole `platform/macos` module
compile on all hosts so its tests run everywhere — rejected: the modules call macOS APIs
(objc/libc) that do not build elsewhere; the pure predicates that could move already did
(`core/safety.rs` in the previous batch). Weakening the assertions or deleting the
platform-specific tests — rejected: each one now states a real platform fact.

## Consequences

The macOS job compiles clean and runs 631 tests (608 Windows + macOS-gated ones) green.
The `home=None` behaviour is now platform-correct instead of accidentally
platform-dependent. The Windows suite is unchanged at 608 passed.

## Verification

- CI run 37461367172 (commit 999076d): Test (Windows) 8m23s green (608 passed / 0 failed /
  9 ignored), Test (macOS) 6m34s green (631 passed / 0 failed / 11 ignored), Bound anchors
  job green; raw log lines in
  docs/agent-notes-evidence/2026-10-06-macos-native-acceptance-ci.log.
- Local Windows suite after every fix: `cargo test --lib` 608 passed / 0 failed,
  strict clippy, rules check, strict-anchors all green (batch logs under
  docs/agent-notes-evidence/2026-10-06-*).

This note records platform-behaviour decisions; the red-run proof belongs to the alias fix
note. Windows evidence plus the macOS CI run above; macOS real-machine spot checks beyond
CI remain out of scope for this host.
