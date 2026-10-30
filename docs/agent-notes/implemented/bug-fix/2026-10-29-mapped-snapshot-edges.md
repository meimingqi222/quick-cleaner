# Agent Note: 映射快照共享未变更的子节点表

Status: implemented

## Problem

增量只改少量节点，却在 Clone 和刷新结束时各重建一次全卷子节点数组。763 万节点的安静刷新仍分配约 2.5 GiB，克隆和数组重建分别约 0.9 和 1.5 秒；目录计数也重复解码所有名称。

## Decision

`internal/core/disk_tree.go` 的映射快照共享不可变 base 子节点表，追加节点只为修改的父节点建立独立列表。Clone 复制可变覆盖、新节点和覆盖列表，保留旧读者隔离；映射树 RebuildChildArrays 只重建追加节点的边，墓碑由读者过滤。所有子节点读者统一经过 childList，包含发现与 delta 解码校验。目录与文件计数只读标志位。祖先去重 scratch 以增长容量预留空间；不能让每追加一个节点都重新分配一次全卷大小的数组。

## Alternatives considered

- 共享可写列表或原地更新旧快照：会让并发读者看到新扫描的修改。
- 每次复制所有 slice header：仍随整卷节点数分配和遍历。
- 跳过 delta 结构校验：共享布局不改变磁盘格式、覆盖与身份校验。

## Consequences

映射节点身份保持不变；追加边和 slot 覆盖属于新快照。heap/MFT 树继续完整重建原有数组；映射树现直接读取不可变的磁盘 CSR，无需基底数组副本；冷加载验证和兼容语义由 `2026-10-30-mapped-index-load.md` 维护。

## Verification

- `internal/core/disk_tree_snapshot_test.go::TestMappedSnapshotEditsStayLocal`
- `internal/core/disk_tree_snapshot_test.go::TestMappedAppendGrowthDoesNotReallocateWholeTreePerFile`

Proved: 修复前，5 万节点的安静克隆、数组重建和计数分配 7188144 字节，超过 256 KiB 上限，见 `docs/agent-notes-evidence/2026-10-29-mapped-snapshot-red.log`；修复后通过，并验证跨代追加、墓碑、兄弟快照隔离及 delta 重载，见 `docs/agent-notes-evidence/2026-10-29-mapped-snapshot-green.log`。

Proved: 容量修复前，2.5 万节点快照追加 128 个文件分配 13725896 字节，见 `docs/agent-notes-evidence/2026-10-30-index-acquisition-growth-red.log`；增长预留后低于 1 MiB，计数和旧快照隔离不变，见 `docs/agent-notes-evidence/2026-10-30-index-final-green.log`。

后续计数优化：CountUsedFiles/Dirs 从已完整验证的 base header 计数出发，逐个扣除被覆盖的原节点、加入当前覆盖与追加节点。统计只与变化量有关，不再每轮扫描所有基底 flags；heap 树仍检查全部 slots。不能用 root.files/root.Size 代替活节点计数，它们是不同契约。

- `internal/core/disk_tree_snapshot_test.go::TestMappedCountsMatchLiveSlotsAcrossSnapshotChanges`

Proved: 仅撤掉原 base 节点的扣减，focused 测试在跨代删除/追加后与逐 slot 统计不一致，见 `docs/agent-notes-evidence/2026-10-30-count-overlay-red.log`；恢复后同 focused 绿，见 `docs/agent-notes-evidence/2026-10-30-count-overlay-green.log`。测试包含子树墓碑、追加目录/文件、原快照隔离、delta 重载和不同于 root 聚合的计数。

固定 20 万节点、少量 overlay 的两种计数合计从 1.69–1.85 ms 降为 72–78 ns，均不分配，见 `docs/agent-notes-evidence/2026-10-30-count-benchmark-before.log` 与 `docs/agent-notes-evidence/2026-10-30-count-benchmark-after.log`。大量真实变更仍需逐项调整，不将该微基准倍数套用于完整扫描。
