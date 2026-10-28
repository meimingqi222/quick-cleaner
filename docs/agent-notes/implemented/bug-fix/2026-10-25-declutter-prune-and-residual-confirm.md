# Agent Note: 冗余整理清理后按失败清单剪枝；残留清理不再二次确认

Status: implemented

## Problem

两处与 Rust 参照不一致：

1. **冗余整理清理失败后，列表会整页重扫一遍**。`requestDeclutterClean` 的收尾回调是
   `func() { r.declutter.Scanned = false; r.scanDeclutter() }`，于是刚清理失败的条目被
   当成新扫描结果又列一次，看起来像「清理失败后清理项又出现一次」，用户也分不清哪些
   真的移走了。
2. **残留清理由二次确认框挡着**。`requestResidualClean` 先 `r.ask("清理选中的残留？")`
   才执行。Rust 的 `clean_selected_residuals` 没有这一步：勾选并按下清理就是显式同意，
   多一层对话框只是拖慢这个本来就要主动发起的动作。

## Decision

1. `pruneDeclutter` + `declutterItemIsGone` 取代重扫，语义照搬
   `prune_cleaned_declutter_items`（`src/ui/actions/declutter.rs`）：一条已勾选的行只有在
   **报告没失败、且路径确实不在了**时才从列表摘除；重复/相似分组在剩不到 2 项时整组消失，
   图片页的分组展开状态随之重置。`requestArbitraryClean` 的收尾回调改为接收
   `core.CleanReport`，磁盘页沿用原来的「清空选中并重扫」。
2. 去掉残留清理的 `r.ask`，直接进入清理。归属、占用、撤离证据的核验与「整批拒绝」保持
   原样——删掉的只是确认框，不是安全门。

## Alternatives considered

- 清理后仍然重扫，但把失败项标红：扫描无法区分「刚才失败」和「本来就在」，用户仍会看到
  同一批条目重新出现。
- 只按报告 `Failed` 摘除、不核验路径：报告说成功但文件还在时（延迟删除、权限边界）会把
  仍在盘上的条目从界面抹掉，用户以为清干净了。Rust 的注释点名了这个坑。
- 保留残留确认框：与 Rust 不一致，且用户已明确认为多余。

## Consequences

清理失败后条目留在原位，消息里给出失败数量，用户可以直接重试；分组类页签里少于 2 项的
分组不再显示。残留清理点一下就开始，失败时通过 `apps.Notice` 与历史记录如实报告。

## Verification

- `internal/app/declutter_prune_test.go::TestDeclutterRowLeavesListOnlyWhenReallyGone`
- `internal/app/declutter_prune_test.go::TestPruneDeclutterKeepsFailedRows`
- `internal/app/root_test.go::TestResidualCleanStartsWithoutConfirmation`
- `internal/app/integration_test.go::TestResidualCleanProbesOnlyAfterTheUserStartsIt`

Proved: 只把二次确认加回去时，`TestResidualCleanStartsWithoutConfirmation` 红于
`residual clean still opens a confirmation dialog`，见
`docs/agent-notes-evidence/2026-10-25-residual-clean-no-confirm-red.log`；只撤掉「路径确实
不在了」这条核验时，`TestDeclutterRowLeavesListOnlyWhenReallyGone` 红于
`a file still on disk left the list`，见
`docs/agent-notes-evidence/2026-10-25-declutter-prune-red.log`。恢复后两条 focused 与
`go test ./internal/app ./internal/ui` 全绿，见
`docs/agent-notes-evidence/2026-10-25-declutter-prune-green.log`。

`TestResidualCleanProbesOnlyAfterTheUserStartsIt` 由原来的「取消不探测」改写而来：没有确认
框后，唯一还成立且必须成立的约束是「用户按下之前不探测、不删除，未知归属整批拒绝」。
