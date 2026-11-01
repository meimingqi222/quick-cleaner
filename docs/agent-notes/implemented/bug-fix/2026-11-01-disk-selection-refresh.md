# Agent Note: 磁盘透镜勾选只刷新选择状态，不重建行

Status: implemented

## Problem

磁盘透镜进入 7400 项的目录（Temp）后，每点一次复选框约 55 ms，全选约 60 ms，连续点击发黏。

剖析（7400 行、30 次重建）：每次勾选都走 `rebuildDisk`，对每一行重新执行
`IsProtected`（27%，含 `IsManagedAgentWorktree` 14%）、`IsSelected` 的祖先遍历（19%）、
`CoversSubtree`（16%）、两项白名单检查（17%）和路径拼接。勾选根本不会改变这些结果。

## Decision

- 新增 `refreshDiskSelection`：复用现有行，只更新 `Selected`、计数、总大小和表头状态。
  单行勾选、全选/取消全选、清空选择三个动作改用它；导航、切换标签、剪枝仍走完整 `rebuildDisk`，
  `rebuildDisk` 构建行后也调用它计算选择状态。
- 新增 `DiskSelectionState.ChildSelected(path, parentSelected)`：同一目录的行共享父目录，
  父目录状态只算一次，每行只查自身的选中/排除项。「最大文件」标签的行父目录各不相同，退回
  `IsSelected`。

## Alternatives considered

- **按目录缓存保护/覆盖结果。** 要正确处理索引剪枝、白名单变更等失效条件，容易留下陈旧的
  「可删除」状态；而勾选路径本来就不需要这些结果。
- **加快 `IsProtected` 等安全检查。** 属于中央安全模块，不为界面性能改动判定逻辑。

## Consequences

- 7400 行：单击勾选 55 ms → 1.8 ms，全选 60 ms → 12 ms（主要是 `SetMany`），进入目录
  55 ms → 41 ms。
- 选择动作不能再依赖 `rebuildDisk` 刷新其他字段；以后若勾选需要改变行的其他属性，必须改回
  完整重建或显式更新那些字段。

## Verification

- `internal/app/root_test.go::TestDiskSelectionActionsRefreshRowsWithoutRebuilding`
- `internal/core/disk_selection_test.go::TestDiskSelectionChildSelectedMatchesAncestorWalk`

Proved: TDD 先红。先加只调用 `rebuildDisk` 的 `refreshDiskSelection` 占位，
`TestDiskSelectionActionsRefreshRowsWithoutRebuilding` 报
`step 0 rebuilt every row for a selection change`（行内哨兵被完整重建重置），
见 `docs/agent-notes-evidence/2026-11-01-disk-selection-refresh-red.log`。
测试每一步还把结果与完整重建逐行比对，保证选择状态、计数与表头一致。

Proved: 制造型红跑。`ChildSelected` 临时去掉排除项检查，
`TestDiskSelectionChildSelectedMatchesAncestorWalk` 报 `...\a\x: ChildSelected=true IsSelected=false`
（同一份 red log 追加）。恢复后两个测试同 focused 绿跑，见
`docs/agent-notes-evidence/2026-11-01-disk-selection-refresh-green.log`。

Pitfalls checked: P127 的「最近显式选择优先」语义未变（`ChildSelected` 与祖先遍历逐节点一致）；
`internal/core/safety.go` 未改动。
