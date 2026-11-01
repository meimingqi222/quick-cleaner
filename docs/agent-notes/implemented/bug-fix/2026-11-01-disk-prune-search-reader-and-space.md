# Agent Note: 剪枝等待搜索读者退出，可用空间从系统读取

Status: implemented

## Problem

功能审阅确认磁盘透镜删除后的剪枝有两处问题：

1. 文件搜索 goroutine 遍历同一棵 `SizeTree`。剪枝只调用 `searchCancel()` 发出信号，不等它退出，
   随后 `RemoveSubtreeInplace` 原地改树；`SearchCancellable` 每 4096 项才检查一次取消。搜索不受
   `busy()` 限制，清理期间可以继续输入。race 检测报告 `setSlot/applyDiskPrune` 与
   `SearchCancellable` 并发读写。`2026-10-29-disk-delete-prunes-index.md` 曾认为「窗口内只会读到
   过期 slot，不会崩」而接受这一点；按 Go 内存模型这是数据竞争，不能接受。
2. 剪枝把所有从原路径消失的节点大小加到 `disk.Space.Free`，移入回收站、外部移动、硬链接都会让
   可用空间虚增（实测 200 → 300）。

## Decision

- `pruneDiskPaths` 先 `holdSearch`：取消当前搜索、记下它的退出通道、暂停新搜索。worker 里等读者
  退出（不阻塞 UI），再 stat，主线程 `applyDiskPrune` 改树后 `releaseSearch` 重跑暂停期间的查询。
  `setSearchQuery` 在暂停时只记录查询；每个搜索 goroutine 退出时关闭 `searchDone`。
- 取消不会让旧搜索立刻退出：复审发现再次输入时 B 覆盖了 A 的 `searchDone`，剪枝只等 B，A 仍在读树。
  现在每轮搜索把上一轮的完成通道串进自己的退出流程：先等上一轮退出再关闭自己的通道，所以等最新一轮
  就等于等所有更早的读者。
- 剪枝作为 mutation 任务运行：被取消也执行收尾，已确认的部分照样摘除并释放搜索暂停；worker
  panic 时 `clearScanFlags` 也会解除暂停。
- 剪枝只改索引；worker 用 `platform.GetVolumeSpace`（经 `Root.volumeSpace` 注入）读取真实可用空间，
  同时更新卷列表行；读取失败就保持原值，绝不按摘除体积推算。

## Alternatives considered

- **剪枝改在克隆树上做再替换指针。** macOS 的 owner 也持有同一棵树，需要同步替换；大卷克隆的内存
  开销也高。
- **树加读写锁。** 读者在 core 里遍历，锁粒度要深入 `SizeTree`，而只读者只有搜索一处。
- **在主线程等待搜索退出。** 会让 UI 卡住，等待只放在 worker。

## Consequences

- 剪枝期间输入的搜索会稍后自动执行，结果基于剪枝后的树。
- 可用空间显示的是系统实测值；移入回收站不再增加可用空间。
- `applyDiskPrune` 本身不再负责停搜索，调用方必须走 `pruneDiskPaths`。

## Verification

- `internal/app/disk_prune_test.go::TestDiskPruneWaitsForSearchReaderAndHoldsNewSearches`
- `internal/app/disk_prune_test.go::TestDiskPruneTakesFreeSpaceFromSystemNotFromPrunedSizes`
- `internal/app/disk_prune_test.go::TestDiskDeletePrunesTreeAndKeepsPosition`
- `internal/app/disk_prune_test.go::TestDiskPruneWaitsForEveryEarlierSearchReader`

Proved: TDD 先红。修复前报 `in-flight search was not cancelled before pruning` 与
`system space ok=true: free = 300, want 230`。制造型红跑：去掉 worker 等待时报
`prune was applied while the search reader was still running`；去掉新搜索暂停时报
`a new search started while the prune was pending`。均见
`docs/agent-notes-evidence/2026-11-01-disk-prune-reader-space-red.log`；恢复后绿跑见
`docs/agent-notes-evidence/2026-11-01-disk-prune-reader-space-green.log`。
复审补充：TDD 先红，
`TestDiskPruneWaitsForEveryEarlierSearchReader` 修复前报
`prune was applied while the earlier search A was still running`，见
`docs/agent-notes-evidence/2026-11-01-disk-prune-reader-chain-red.log`；串联完成通道后绿跑见
`docs/agent-notes-evidence/2026-11-01-disk-prune-reader-chain-green.log`。
真实搜索对剪枝的 race 压测（未提交）去掉等待时同样不报竞争，说明真实窗口很窄，只能靠上述确定性测试锁定顺序。

Pitfalls checked: P128/P129 的剪枝 stat 仍在 worker；P129 的「只摘除确认消失的路径」不变。
