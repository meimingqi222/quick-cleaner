# Agent Note: Rust 参照树删除后，旧 note 的 Rust 引用作为历史保留

Status: implemented

## Problem

2026-11-01 所有者决定只维护 Go 版本，删除 Rust 参照树（`src/`、`examples/`、
Cargo、Rust CI、只扫描 `src/ui` 的 UI 检查脚本及其测试）。约 113 条有效 note 的
`## Verification` 引用 `src/*.rs` 里的 Rust 测试作为当年写下决策时的证据；直接删除后
`verify-notes.py`（默认 strict）报 60 个「verification target not found」，
`--dump-anchors` 也会列出 439 个指向已删文件的锚点。

## Decision

- `tools/regression-notes/verify-notes.py` 与 `scripts/check-anchors-vs-tests.py` 都
  新增可重复的 `--retired-prefix PATH/`：落在前缀下的验证目标和锚点视为历史，不要求
  存在；其他缺失照常报错。CI（`.github/workflows/agent-notes.yml`）统一传入
  `src/`、`examples/`、`scripts/test_ui_`。
- note 正文不改：决策理由、备选和当年的证据保持原样，以后改到相关 note 时再顺手把
  证据换成 Go 测试。
- `check-anchors-vs-tests.py` 保留并接入 CI：它对 Go/Python 锚点要求引用文件里有真实
  的 `func`/`def` 声明，比 `verify-notes` 的子串检查严格；cargo 测试清单参数改为可选。
- PITFALLS 顶部注明早期条目的 `.rs` 路径是历史记录，约束仍适用于对应 Go 代码。

## Alternatives considered

- **逐条清理 note**：只靠 Rust 测试验证的移入 `archived/`，其余删掉 Rust 引用。
  改动 100 多个文件，容易误删仍有用的上下文，所有者选择了保留历史的方案。
- **用 `--no-strict` 降级全部缺失目标**：会连 Go 测试被误删或改名也一起放过。
- **删除 `check-anchors-vs-tests.py`**：它的 cargo 清单部分随 Rust 失效，但 Go 声明
  检查仍在挡「改名后旧名留在注释里」，删掉是退步。

## Consequences

- 新 note 不能用 `--retired-prefix` 逃避绑定：前缀只覆盖已删除的路径，Go 测试缺失
  仍然失败。
- 不要为了「清理」去掉 CI 里的 `--retired-prefix`，否则 60 个历史引用会让门禁变红；
  也不要把新的现存目录加进前缀。

## Verification

- `tools/regression-notes/tests/test_retired_prefix.py::test_retired_citation_is_history_not_an_error`
- `tools/regression-notes/tests/test_retired_prefix.py::test_other_missing_targets_still_fail`
- `tools/regression-notes/tests/test_anchor_checks.py::test_retired_anchor_is_history_but_live_go_anchor_still_checked`

Proved: 删除 Rust 树后未加参数的自然红跑：`verify-notes.py --strict-anchors` 退出 1，
60 个 ERROR 全部是 `src/` 或 `examples/` 下的目标；`check-anchors-vs-tests.py` 报
444 个缺失锚点。加上 CI 中的 `--retired-prefix` 后两者均通过，且
`test_other_missing_targets_still_fail` 证明前缀外的缺失 Go 目标仍然失败。
