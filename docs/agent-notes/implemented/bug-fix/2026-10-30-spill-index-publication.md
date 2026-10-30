# Agent Note: 首次 spill 构建只发布一次已验证映射

Status: implemented

## Problem

首次全盘扫描的 spill 构建逐条读取 header 和名称，为每个文件遍历全部祖先；构建结束又物化整树、重新编码发布，owner 再排队保存相同全量树。完整尾记录头缺少名称时还被误认作正常 EOF，可能替换已有有效检查点。

## Decision

`internal/core/disk_index_cache_format.go` 两遍使用缓冲 reader：计数跳过名称，填充复用名称字节并立即复制到输出映射。父节点先于子节点的既有 spill 契约允许倒序每节点聚合一次，保持原有整数加法语义。

构建文件先通过完整 v7 校验，准备 checksum 绑定的 coverage；解除 writer-owned 映射后 truncate、sync、close。持有共享写锁时检查内存和磁盘水位，先发布 immutable coverage，再 rename 实际构建文件，随后同步目录并删除旧 delta。最后重新加载正式只读映射。`internal/core/disk_index_macos.go` 的 full 分支识别已发布的 spill，避免第二次 Save。

`internal/core/disk_index_mapped_unix.go` 的 decodeSpillIndex 仅借用构建映射，生命周期由 builder 管理，不得为此借用 wrapper 自动 unmap。截断 header 或名称、非法名字/父关系均失败，不能将这些错误解释为空尾记录。

## Alternatives considered

- 只优化聚合：仍保留多次全量复制、编码和发布。
- 直接返回可写映射或跳过验证：破坏不可变快照及缓存信任边界。
- 先替换 base 再发布 coverage：崩溃可留下缺失覆盖身份的检查点。
- 在名称缺失时接受 EOF：可能把截断采集数据发布为完整覆盖。

## Consequences

首次构建只发布一次；返回树仍按正式缓存的完整验证与覆盖加载路径创建。只读映射保留不变，调用者仍负责删除 spill。动态全盘扫描结果不能用于严格的同输入 A/B，独立固定基准负责验证优化。

## Verification

- `internal/core/disk_index_spill_test.go::TestSpillOwnerPublishesOnceAndReturnsMappedBase`
- `internal/core/disk_index_spill_test.go::TestSpillBuildDoesNotMaterializeBaseAndRejectsTrailingName`

Proved: 修复前观察到多余 Save=1、返回 heap、100017 节点累计分配 52967864 字节，以及缺失尾名称被接受并替换旧检查点，见 `docs/agent-notes-evidence/2026-10-30-spill-pipeline-red.log`；修复后同 focused 测试通过，见 `docs/agent-notes-evidence/2026-10-30-spill-pipeline-green.log`。

固定 10 万文件/64 层基准由 304–331 ms、约 53 MB、80 万次分配降至 65–67 ms、约 6.1 MB、约 700 次分配，见 `docs/agent-notes-evidence/2026-10-30-spill-benchmark-before.log` 与 `docs/agent-notes-evidence/2026-10-30-spill-benchmark-after.log`。同文件附加测试还检查内存/磁盘旧水位、截断 header/name、forward-parent、file-parent、unsafe-name 不替换既有检查点及 coverage。
