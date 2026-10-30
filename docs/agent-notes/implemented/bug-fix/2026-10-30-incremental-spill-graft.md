# Agent Note: 增量刷新接入子树 spill，失败扫描不遗留 spill

Status: implemented

## Problem

macOS 增量刷新对 MustRescan 根逐个调用原生扫描。子树超过内存阈值时，扫描结果只带 spill 文件、不带内存树；旧的应用变更代码只接受内存树，遇到 spill 就返回 `ErrDiskIndexRebuildNeeded`，于是一次根目录级重扫退化为整卷重建（实测约 72 秒，见 2026-10-30 10:57 GUI 日志）。

同时，原生遍历已产出 spill 后若遍历报错或转换被取消，`finishMacWalkScan` 直接返回零值结果；refresh 的清理只遍历结果里的 spill，看不到这份文件，临时文件遗留在缓存目录。

## Decision

- `internal/core/disk_tree_spill.go` 的 `appendSpillSubtree` 把 spill 记录按父先子后的顺序流式追加进私有刷新候选，只加偏移，不先物化第二棵内存树。数量、名称、父链和文件长度都在追加前或追加中校验，不可信计数不能先行预留内存。
- `internal/core/disk_index_macos.go` 的 refresh 对 spill 结果调用该接入，内存树结果仍走 `AppendSubtree`。
- `finishMacWalkScan` 持有 spill 直到成功转交；遍历错误或转换失败时由它删除 spill。成功结果中的 spill 仍由 refresh 的延迟清理在接入后删除。

## Alternatives considered

- 子树出现 spill 时继续整卷重建：正确但慢，正是本问题。
- 先把 spill 读成内存子树再 `AppendSubtree`：峰值内存翻倍，10 万节点基准约 39 MB 对 10.6 MB。
- 由调用方负责失败扫描的 spill：失败时调用方拿到的是零值结果，没有句柄可删。

## Consequences

根目录重扫中的大子树不再触发整卷重建。spill 接入失败会让本次刷新失败并丢弃候选，不会发布半棵树。

## Verification

- `internal/core/disk_index_subtree_spill_test.go::TestMacDiskIndexSpilledSubtreeDoesNotRebuildVolume`
- `internal/core/disk_index_subtree_spill_test.go::TestSpillGraftMatchesMemorySubtreeAcrossLayouts`
- `internal/core/disk_index_subtree_spill_test.go::TestSpillGraftRejectsMalformedRecordsAndCount`
- `internal/core/disk_index_subtree_spill_test.go::TestNativeScanOwnsSpillUntilSuccessfulTransfer`

Proved: 修复前 spill 子树触发 `apply events: disk index needs an interactive rebuild` 后整卷重建，见 `docs/agent-notes-evidence/2026-10-30-incremental-spill-red.log`；不可信计数在校验长度前预留约 64 MB，见 `docs/agent-notes-evidence/2026-10-30-incremental-spill-count-red.log`；失败扫描遗留 spill（walk-error、conversion-cancel 两个子测试失败），见 `docs/agent-notes-evidence/2026-10-30-native-spill-owner-red.log`。修复后同 focused 全绿，见 `docs/agent-notes-evidence/2026-10-30-native-spill-owner-green.log`。
