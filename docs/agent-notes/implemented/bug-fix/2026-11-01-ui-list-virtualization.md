# Agent Note: 大列表虚拟化与主线程逐项检查外移

Status: implemented

## Problem

磁盘透镜修好之后，同类问题在其它页面仍然存在：

- Declutter 的下载 / 大文件 / 重复文件 / 相似照片四个 tab 用 `ui.Scroll` 每帧构建全部卡片；
  结果集上千时滚动掉帧，且「显示更多」之外的隐藏行也一直在重建。
- Dev Env 的语言包列表一次构建全部包行，全局包里常见上千项。
- 应用残留面板（尤其孤儿扫描）和卸载后的 Leftover 面板把每条残留都构建出来。
- 清理结束后 `pruneDeclutter` 在主线程对每个勾选项 `os.Stat`，同时用 `map[string]struct{}`
  再遍历失败列表；取消删除后回到额面板会明显卡住。
- 磁盘透镜「删除所选」在主线程调 `ResolveTargets`，遇到选了整个目录又排除子项时递归
  `ReadDir`，确认框迟迟不出现。
- `MergeDiscovered` 用「每个发现项 × 每个固定项」的 `atOrUnder` 双层循环判断重叠，
  大扫描结束时主线程被 O(D×F) 字符串比较拖住。

## Decision

- Declutter 的 tab 内容改为固定头部 + `ui.List` 视口；重复组和照片组以「组」为一行，
  组内文件仍在行内构建，但只构建可见组。
- Dev Env 包列表改用 `ui.List`，行 key 用包 ID；`ItemKey` 必须非空且唯一，否则 MyGo
  会因为重复 key 直接 panic。下载/大文件行没有稳定唯一字段时不要设 `ItemKey`，
  用默认的行索引。
- 应用残留和 Leftover 面板的首层列表改为 `ui.List`；残留面板用固定高度视口
  （`min(280, ...)`），Leftover 面板沿用剩余高度。
- `pruneDeclutter` 在 worker 里 stat 勾选项，主线程只接收 `gone` 集合；
  失败路径集合改为 map，`declutterItemIsGone` 拆成 `declutterSelectedPaths` +
  `applyDeclutterPrune`。「勾选 + 报告没失败 + 路径确实不在」三条仍然同时成立才摘除。
- `RequestDeleteSelected` 在 worker 里解析排除项，finish 回到主线程再弹确认。
- `MergeDiscovered` 改为 `fixedSet` + 每个固定路径的边界前缀集合；条目侧沿父链查
  `fixedSet`。语义等价于原来的双向 `atOrUnder`，复杂度降为 O(路径深度)。

## Alternatives considered

- **只把每帧统计缓存起来。** 缓存无法解决首次构建上千行和滚动时重建全部行。
- **给每个列表加分页。** 会改变现有「显示更多」交互，并让已选但不在当前页的条目不可见。
- **让 `MergeDiscovered` 整体挪到 worker。** 白名单和固定目标可能在扫描期间变化，
  合并必须留在主线程；这里只能把比较从 O(D×F) 降下来（与 P77 一致）。
- **异步解析排除项后直接删除，不再弹确认。** 会绕过共享确认门禁。

## Consequences

- 大结果集的滚动只构建视口行，Declutter / Dev Env / 残留 / Leftover 都已覆盖。
- 清理后的列表修剪不再阻塞主线程；失败的条目仍然留在原处并可重试。
- 磁盘透镜的确认框不再等待目录递归。
- 扫描结束的合并只做哈希查找，与原来的重叠判定结果一致。

## Verification

- `internal/ui/declutter_page_test.go::TestDeclutterListsVirtualizeLargeResultSets`
- `internal/ui/dev_env_page_test.go::TestDevEnvPackageListVirtualizesLargeInventory`
- `internal/app/declutter_prune_test.go::TestPruneDeclutterKeepsFailedRows`
- `internal/app/root_test.go::TestSelectedDiskDeleteResolvesTargetsOnWorkerBeforeConfirming`
- `internal/core/scanner_test.go::TestMergeDiscoveredOverlapCoversBothDirectionsAtPathBoundaries`

Proved: 制造型红跑（只撤一处防护、跑 focused、恢复后同 focused 绿）：

- 把 `listBox` 退回逐行构建时，`TestDeclutterListsVirtualizeLargeResultSets` 报
  `offscreen download row constructed instead of virtualized`。
- 把 Dev Env 包列表退回逐行构建时，`TestDevEnvPackageListVirtualizesLargeInventory` 报
  `offscreen package row constructed instead of virtualized`。
- 把 `requestSelectedDiskDelete` 改回同步解析时，
  `TestSelectedDiskDeleteResolvesTargetsOnWorkerBeforeConfirming` 报
  `confirmation appeared before targets were resolved`。
- 把 `pruneDeclutter` 改回同步时，`TestPruneDeclutterKeepsFailedRows` 报
  `worker did not publish`（等待 worker 发布超时）。

红绿输出见 `docs/agent-notes-evidence/2026-11-01-ui-list-virtualization-red.log`
与 `docs/agent-notes-evidence/2026-11-01-ui-list-virtualization-green.log`。

Pitfalls checked: P77（主线程合并仍不碰文件系统，只优化比较）、P127（磁盘透镜列表仍
虚拟化、选择语义不变）、P119/P120（占用只做提示、删除授权仍走中央 safety，未改）。
