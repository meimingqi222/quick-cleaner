# Agent Note: 发现扫描共用冻结规则，映射查询不复制无关名称

Status: implemented

## Problem

扫描已有整盘索引后，第二次发现扫描仍慢。756 万节点、8352 个命中的定向测量中，每轮累计分配约 4.61 GiB、5200 万次：每个命中都经 `NewRuleRef` 用 JSON 深拷贝整份规则包；`SizeTree.Valid`、`IsDir` 和标签预扫描又解码映射节点的完整名字。

## Decision

`internal/core/devscan.go` 的 `DiscoverBuildArtifacts` 每次调用冻结一份规则，索引和遍历通道共用该只读快照；扫描用的 marker 也来自冻结快照。每个命中仍单独创建目标身份与 `CleanupPlan`，下次扫描重新冻结，源规则修改与返回的诊断副本不能改变已捕获的计划。

`internal/core/disk_tree.go` 在已经通过完整缓存校验的映射 base 上直接读标志位，覆盖与新增节点仍优先使用内存状态。`internal/core/disk_index_mapped_unix.go` 在映射名称池里比较 ASCII 标签名，不复制无关文件名；标签位图仍每轮重建；基底候选在完整加载时记录，所有覆盖和追加节点逐轮重新检查，避免失效管理遗漏。标签签名、实时项目证据、路径约束和执行前身份核验仍照常运行。

`internal/app/junk.go` 将阶段零计时移到 `owner.Load` 之前，并单独记录索引加载/更新耗时，避免把只包含查询规划的 0 秒误当缓存加载时间。

## Alternatives considered

- 直接引用调用方可变规则：失去冻结语义。保留一次完整拷贝；共用的只是同轮、同规则、同空 scope 的冻结结果。
- 按时间缓存全部发现结果，或跳过实时安全证据：会漏掉文件变化或放宽清理授权，本次不采用。
- 跨轮缓存标签位图：需要覆盖 override、删除、追加和子树替换的全部失效路径；不跨轮缓存完整位图；只复用不可变基底的候选 slot，再检查本轮全部 overlay 与追加节点。
- 删除索引加载时的格式、校验和、CSR 或覆盖验证：不属于性能快路径的合法替代，仍保留完整校验。早期优化未改新进程冷加载的堆分配；后续直接映射验证由 `2026-10-30-mapped-index-load.md` 维护。

## Consequences

优化索引与遍历两条发现通道，Windows heap 索引也共用冻结规则；原始节点读取只对 darwin/linux 的映射实现生效。规则包每轮冻结一次，不按目标数量复制。累计分配不是峰值驻留内存。

## Verification

- `internal/core/devscan_perf_test.go::TestBuildDiscoverySharesFrozenRuleAndKeepsTargetsIndependent`
- `internal/core/devscan_mapped_perf_test.go::TestMappedDiscoveryFlagsAndTagsAvoidNameAllocations`

Proved: 修复前自然红跑，两个通道都报告逐项复制规则；1027 个映射节点的标志查询产生 3078 次分配、标签扫描产生 1030 次分配，见 `docs/agent-notes-evidence/2026-10-29-discovery-allocation-red.log`。修复后同 focused 测试转绿，见 `docs/agent-notes-evidence/2026-10-29-discovery-allocation-green.log`；同时锁定规则冻结隔离、目标计划独立、覆盖/墓碑/新增节点与大小写标签语义。

同一索引副本的两轮定向对照：发现耗时 7.900/7.859 秒 → 1.205/1.082 秒，累计分配约 4721 MiB → 257 MiB；四轮均为 8352 项且路径、大小、文案、操作、规则观察的 SHA-256 指纹一致。原始测量见 `docs/agent-notes-evidence/2026-10-29-discovery-performance-before.log` 与 `docs/agent-notes-evidence/2026-10-29-discovery-performance-after.log`。这是预热文件系统下的单独发现阶段，不能直接换算为完整产品扫描时间。

2026-10-30 标签位图预扫描将同一节点的 Valid/IsDir 两次标志读取合为一次 nodeFlags 掩码，节点范围已由循环界定，名字、父节点和实时签名验证保持。固定 20 万节点基准由约 3.4 ms 降至 2.3–3.0 ms，仍每轮重建位图，见 `docs/agent-notes-evidence/2026-10-30-tag-benchmark-before.log` 和 `docs/agent-notes-evidence/2026-10-30-tag-benchmark-after.log`；本 note 既有标志、名字 override、父关系、墓碑与追加标签回归继续通过。


后续减少全卷标签遍历：`internal/core/disk_index_cache_format.go` 完整验证每个映射记录时记录 Used 文件中的 CACHEDIR.TAG 候选 slot；每轮只读未覆盖的基底候选，覆盖与追加记录全部重新判定，父目录状态也读当前快照。它缓存的是不可变 base 的名字候选，不缓存跨轮最终标签位图；实时 signature/identity 读取不变。普通文件新命名为 tag、原 tag 改名/移动/墓碑、目录同名及新增标签均保持原语义。

Proved: 仅撤掉基底候选遇到 overlay 时的跳过保护，同一标签回归在 mapped tag name override ignored 失败，见 `docs/agent-notes-evidence/2026-10-30-tag-candidates-red.log`；恢复后同 focused 绿，见 `docs/agent-notes-evidence/2026-10-30-tag-candidates-green.log`。固定 20 万节点由 2.3–3.0 ms 降至 2.7–4.2 us，1 次位图分配，见 `docs/agent-notes-evidence/2026-10-30-tag-candidates-after.log`。完整实机发现仍需实时文件系统证据，不能套用这个微基准倍数。
