# Agent Note: 部分覆盖不能触发已覆盖代码目录的重复发现

Status: implemented

## Problem

GUI 的 JunkIndex 发现规划在 home 下任何子树未覆盖时，先遍历索引，再对 CodeRoots(home) 整体实时遍历。单独索引发现约 1.2 秒，却不能代表 GUI 的约 17 秒；只读 CLI 完整规划复现 15.96/17.87 秒。

## Decision

`internal/core/junk_index.go` 在实时回退中传递已扫描索引的覆盖判断；`internal/core/devscan.go` 的 walkCollector 跳过完整覆盖的分支。缺项、blocked 子树及其祖先继续沿原 CodeRoots 范围、深度和安全证据遍历。公开的 DiscoverBuildArtifacts 独立遍历保持原行为，240 秒预算、取消、结果合并与身份冻结保持。

每轮发现为 held 快照预先规范化未知路径并建立未知节点与祖先集合；目录查询只走自身父链，再检查节点存在。该索引不跨轮缓存，不在每个目录上重做所有 gap 的 ASCII 归一和 filepath.Rel。与 SizeTree.CoversSubtree 的祖先/后代未知、大小写及缺项语义一致。

`tools/indexbench` 的 `-product-discovery` 调用相同 BeginJunkIndex/DiscoverWithin 规划；默认的单树测量只代表索引发现，不能直接描述 GUI 完整发现时间。

## Alternatives considered

- home 有索引就完全关闭 live 回退：会漏掉未覆盖目录。
- 保留整棵代码目录重扫以防快照不新：之前完整覆盖的 home 已经跳过实时回退；局部未知不应扩大成所有分支未知。
- 只优化直接发现函数并报告为 GUI 时间：会遗漏规划层的重复遍历。

## Consequences

只复用已证明完整覆盖且已做索引发现的分支，未知目录仍可被发现。实机 cache 模式对照，发现 15.96/17.87 秒变为 5.12/3.32 秒，四轮均为 8355 项、相同指纹；机器负载和文件系统预热会影响时间。此测量不含 GUI 渲染与原生 provider。

## Verification

- `internal/core/junk_index_test.go::TestPartialIndexDiscoveryWalksOnlyUncoveredBranches`
- `internal/core/junk_index_test.go::TestJunkCoveragePreparesGapsOncePerDiscovery`

Proved: 修复前未知项目仍可发现，但完整索引的 healthy 分支重复探测 2 次，见 `docs/agent-notes-evidence/2026-10-29-partial-discovery-red.log`；修复后 healthy 零次实时探测，未知项目的 node_modules 仍被发现，见 `docs/agent-notes-evidence/2026-10-29-partial-discovery-green.log`。实机对照见 `docs/agent-notes-evidence/2026-10-29-product-discovery-before.jsonl` 与 `docs/agent-notes-evidence/2026-10-29-product-discovery-after.jsonl`。

Proved: 覆盖预处理前单个健康目录查询对 129 个 gap 产生 778 次分配，见 `docs/agent-notes-evidence/2026-10-30-discovery-coverage-red.log`；预处理后低于 30 次，并与原 CoversSubtree 核对已知、缺失、未知祖先和大小写目录，见 `docs/agent-notes-evidence/2026-10-30-discovery-coverage-green.log`。

2026-10-30 继续复测完整规划：第二轮实时回退 5.295 秒降至 18 ms，发现阶段 6.733 秒降至 1.607 秒；两轮结果均为 8355 项，相同指纹。最后一次完整 refresh 的第二轮 load 204 ms、discover 1607 ms、verify 22 ms，保存为 27 ms 的 delta。首轮 load 5.940 秒、discover 4.078 秒，之后仍需等待约 13.936 秒后台压实/重载；没有将后台时间隐藏为零。真实事件流和缓存预热随时间变化，不作为固定输入性能保证。记录见 `docs/agent-notes-evidence/2026-10-30-index-growth-after.jsonl`、`docs/agent-notes-evidence/2026-10-30-index-performance-after.jsonl` 与对应 `.log`。
