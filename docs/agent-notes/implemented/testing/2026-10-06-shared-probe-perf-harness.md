# Agent Note: Reproducible shared-probe performance harness

Status: implemented

## Problem

The "性能" matrix row asks for the same fixture measured multiple times with the
enumeration/read counts and peak resources, and GOAL §4-G requires recording "基线版本/工作区、
夹具规模、方法、多次结果". The only performance evidence was an ad-hoc run whose harness lived
outside the tree: it reported per-run counters but did not record the workspace commit, the
toolchain, the rules-package identity, the fixture scale or the method, and it could not be
re-run. That made the "no new duplicate full-directory scan" claim unauditable and left the
baseline unstated.

## Decision

Added `scripts/shared-probe-perf.ps1`, a committed, re-runnable harness. It records the baseline
(`git rev-parse HEAD`/branch/dirty, `rustc --version`, OS, date, `rules.exe check`) and the
method and fixture scale, builds the three isolated `rules/fixtures/*-extra.toml` roots exactly
as the isolation tests do, then runs the compiled `rules.exe explain 1 <extra.toml> <root>` five
times per fixture and reports the `directory_discovery` counters (plans, `directory_reads`,
`inventory_entries`, `directory_probes`, `manifest_reads`, `manifest_rows`, `candidate_checks`,
`budget_blocked`) plus the wall clock of each run. It writes
`docs/agent-notes-evidence/2026-10-06-shared-probe-perf-baseline.json` and a human-readable
`.log`. `explain` builds an isolated plan and never deletes anything.

The rebuilt fixtures reproduce the documented counters: `directory-layout-extra` 1 plan / 1
read / 1 candidate; `cache-catalog-extra` 4 plans / 1 read / 4 candidates; `manifest-layout-extra`
2 plans / 1 read / 5 manifest reads / 4 manifest rows. Across all five runs per fixture the read,
probe and candidate counters are byte-identical (only wall clock varies).

## Alternatives considered

Keeping the ad-hoc harness: it cannot be re-run or diffed, so drift in read counts would go
unnoticed. A Rust `#[test]` asserting wall-clock: timing assertions are flaky across machines
and `cargo test` would not record the durable baseline header. A before/after wall-clock
comparison: the pre-migration implementation no longer exists in the tree, so the "before" side
cannot be produced locally — asserting a fabricated number would be dishonest. Reusing the flat
`-extra.toml` fixtures verbatim: their documented expected outputs (plans/reads/rows) double as
the harness's self-check, so the fixtures are the right scale.

## Consequences

The performance row now cites a committed harness and records the baseline commit/workspace,
toolchain, fixture scale, method and five runs per fixture. The remaining limitation is
unchanged and explicit: this is the "after" side plus determinism, not a cross-version
wall-clock delta. The harness rebuilds fixtures under the OS temp dir; it never cleans real
resources and does not touch the repository's own files besides the two evidence files.

## Verification

Baseline-recorded evidence (committed commit, branch, dirty flag, rustc, OS, rules check, method,
scale, five runs per fixture):
`docs/agent-notes-evidence/2026-10-06-shared-probe-perf-baseline.json` and
`docs/agent-notes-evidence/2026-10-06-shared-probe-perf-baseline.log`.

- `scripts/shared-probe-perf.ps1`
- `src/core/rules/directories.rs::directory_layouts_preserve_baseline_and_share_reads`
- `src/core/rules/directories.rs::repeated_discovery_reads_shared_inventories_once_per_run`
- `src/core/categories/mod.rs::production_target_table_is_deterministic_and_duplicate_free`

This is a testing/tooling addition, not an organic bug fix; the counters are reproduced from
the production discovery path. Actual unified gate results are recorded in RULES_REFACTOR_STATUS.
