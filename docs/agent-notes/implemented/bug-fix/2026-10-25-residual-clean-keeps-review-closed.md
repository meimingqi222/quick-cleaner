# Agent Note: 清理残留后不再重开残留面板

Status: implemented

## Problem

清理残留成功后，收尾又调用了一次 `showResiduals(result, app)`，于是面板重新弹出：全部清掉时
第二次显示「0 项关联残留」，有失败项时重开的就是同一批删不掉的条目。用户看到的是「先显示
一次清理项，清理完成后又弹一次」。

Rust 明确不做这件事，`src/ui/actions/apps.rs` 里连同原因写在一起：

```rust
// 不自动重开残留对话框：删不掉的多半会一直删不掉，重开变成
// 点一次弹一次。状态栏报失败计数；残留扫描入口可再扫。
this.residual.result = None;
```

## Decision

`requestResidualClean` 的收尾改为关闭面板而不是重开：`r.residual.Items` 仍保留本次核验后
幸存的条目（后续上下文要用），`ResidualsOpen`、`Residuals`、`ResidualSize`、`SelectedSize`
与 `Occupancy` 一并清空；清理计数由 `finishCleanup` 的提示与历史记录报告，用户要看剩余项
就重新走残留扫描入口。卸载流程里「卸载后立刻看残留」的那次 `showResiduals` 不受影响。

## Alternatives considered

- 只在 `len(remaining) > 0` 时重开：仍有「点一次弹一次」的问题，正是 Rust 注释要挡的行为。
- 保留面板但把失败项置灰：Rust 没有这种形态，且面板开着仍会挡住下面的列表。
- 把剩余项合并进下次扫描结果：扫描本来就以磁盘现状为准，合并反而会把已消失的条目带回来。

## Consequences

清理后面板关闭，失败计数留在提示与历史里；残留入口重新扫描能看到还没清掉的条目。全清掉时
不会再出现一次「0 项关联残留」的空弹窗。

## Verification

- `internal/app/root_test.go::TestResidualCleanDoesNotReopenTheReviewPanel`

Proved: 修复前该断言红于 `cleaning reopened the residual review panel`，见
`docs/agent-notes-evidence/2026-10-25-residual-panel-reopen-red.log`；修复后 focused 与
`go test ./internal/app ./internal/ui` 全绿，见
`docs/agent-notes-evidence/2026-10-25-residual-panel-reopen-green.log`。
