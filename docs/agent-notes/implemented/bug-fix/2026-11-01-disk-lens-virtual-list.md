# Agent Note: 磁盘透镜大目录虚拟化与批量全选

Status: implemented

## Problem

磁盘透镜进入含数千个文件的缓存目录（实测 2643 项）时：

- 勾选表头全选要卡几秒才出现选中状态；
- 列表滚动明显掉帧。

根因有两处：`internal/ui/disk_page.go` 用 `ui.Scroll` 直接遍历 `s.Rows` 构建全部行，2643 个复选框、按钮和 tooltip 每帧都重建；`DiskSelectionState.Set` 每选一行都会重新扫描全部已选项清理父子覆盖关系，全选因此是 O(n²)。

## Decision

- 行列表改为 `ui.List` 虚拟化，只构建视口内的行；目录/标签/路径仍进入列表 key，切换目录自然重建滚动状态。行 key 使用 `Node.Idx`，保证复用稳定。
- 新增 `DiskSelectionState.SetMany(paths, sizes, on)`：一批兄弟行更新一次 map，统一清理被已选祖先覆盖的子项与无祖先的排除项；选择语义与逐项 `Set` 保持一致。收尾清理沿用 `Toggle` 的规则：带已选祖先的选择项先删后查，若中间隔着排除项（显式重新勾选）就保留。只按「有已选祖先」一刀切删除，会在任意目录全选时静默丢掉别处的重新勾选。
- `SetViewSelected` 收集当前视图可选行的路径和大小后调用 `SetMany`，不再逐行 `Set`。

## Alternatives considered

- **只优化全选，不虚拟化列表。** 滚动卡顿仍然存在，且全选后的一次重绘仍会构建全部行。
- **分页显示。** 会改变现有文件夹浏览交互，并丢失跨文件夹选择在原列表中的可见性。
- **直接重建选择状态。** 会丢掉其他文件夹的选择；`SetMany` 保留跨视图选择。

## Consequences

- 数千项文件夹的滚动只构建可见行，帧率恢复。
- 全选从逐行 O(n²) 变为线性批量更新，选择结果与逐项 `Set` 等价。
- 目录切换、行选择、删除按钮和右键进入目录的行为保持不变。

## Verification

- `internal/ui/disk_page_test.go::TestDiskPageVirtualizesLargeFolderRows`
- `internal/core/disk_selection_test.go::TestDiskSelectionSetManyMatchesIndividualSets`
- `internal/core/disk_selection_test.go::TestDiskSelectionSetManyKeepsReinclusionUnderExclusion`

Proved: 把行列表临时退回 `ui.Scroll` 全量构建时，
`TestDiskPageVirtualizesLargeFolderRows` 报
`offscreen row constructed instead of virtualized`；恢复 `ui.List` 后同 focused 绿跑。
批量选择用逐项 `Set` 作为对照，覆盖全选、取消和“父已选时取消子项”三种状态。
红绿输出见 `docs/agent-notes-evidence/2026-11-01-disk-lens-virtual-list-red.log`
与 `docs/agent-notes-evidence/2026-11-01-disk-lens-virtual-list-green.log`。

Proved: 审阅发现初版 `SetMany` 收尾删除所有带已选祖先的选择项。先写
`TestDiskSelectionSetManyKeepsReinclusionUnderExclusion`（TDD 先红）：在无关目录批量全选后，
排除项之下的重新勾选（测试中的 z，位于被排除的 y 之下）从 batch 结果中消失，与逐项 `Set` 不一致
（batch 2 项/31，individual 3 项/41）。改为与 `Toggle` 相同的先删后查后同 focused 绿跑。
证据见 `docs/agent-notes-evidence/2026-11-01-disk-selection-reinclusion-red.log`
与 `docs/agent-notes-evidence/2026-11-01-disk-selection-reinclusion-green.log`。

Pitfalls checked: P119/P120 的占用只做提示、按叶子清理语义未变；本次只改列表构建与选择批处理。
