# Agent Note: 映射索引直接校验与读取，避免冷加载复制整树

Status: implemented

## Problem

cache.Load 虽然最终使用 mmap，却先构造全量 cacheRecord、treeSlot、名称字符串和子节点数组，然后丢弃大部分堆副本。763 万节点的冷读约 3.5 秒、分配约 1.4 GiB；压实后的重载又支付同样成本。已有小 delta 还会对全部基底节点再做一次全树验证。

## Decision

`internal/core/disk_index_cache_format.go` 共用原来的 checksum、头部、卷标识与布局边界验证；映射通道直接验证记录的 UTF-8/名称、Used、根、严格前向父子关系、目录属性、CSR 范围、重复/缺失子节点和计数。父节点必须排在当前记录之前，因此无需堆 slot 或遍历状态来证明无环。CSR 用位图验证唯一性，仍读取和检查全部基底。

`internal/core/disk_tree.go` 的 treeChildView 对 heap slice 或已验证的 little-endian CSR 提供同一只读遍历，公开 ChildIndices 继续返回独立副本；没有 unsafe 类型转换。新增节点及覆盖列表仍在独立 heap overlay 中。合法但乱序的 CSR 行只为该行建立排序副本，保留旧加载器按 slot 顺序重建的查询顺序。

增量解析仍先验证完整 checksum、基底绑定、事件水位、所有记录、覆盖、child overlay 和统计；映射基底已完整校验后，通过 validateOverrides 验证全部变化及其对目录子节点的影响，计数从已校验基底调整。失败候选不修改基底。

映射读取的 slot/name 方法仅用于已经通过上述验证的 base；数值序列化可用 slotValues 避免复制无关名称，但不得将缺少名称的数值副本写回树。未映射的读入路径保留原 heap 解码器，作为兼容 fallback 和差分验证参照。映射解析失败会关闭本次映射。

## Alternatives considered

- 只检查 checksum 或信任 header：不能证明名称、关系、CSR 和统计合法。
- 直接把 bytes 转成机器端序的 uint32 slice：需要 unsafe，且大端 Linux 会错误解码。
- 忽略 delta 的全量基底之外检查：墓碑、目录类型变化和追加父关系必须仍逐项校验。
- 直接按任意合法 CSR 行顺序查询：旧实现选择 slot 顺序，大小写别名及 capped 查询会改变。
- 保留完整基底 heap 副本：映射的内存和加载优势被前置复制抵消。

## Consequences

磁盘 ABI、校验范围和保守覆盖行为不变。普通有序 CSR 不分配节点矩阵或名称副本；乱序行按需复制。metadata-only 或小 delta 的加载成本主要为一次基底读取与完整校验、以及真实变化。Windows 无 mmap fallback 仍走 heap 路径。

## Verification

- `internal/core/disk_index_mapped_load_test.go::TestMappedLoadDoesNotMaterializeValidatedBase`
- `internal/core/disk_index_mapped_load_test.go::TestMappedLoaderMatchesHeapValidationAndChildOrder`

Proved: 修复前 200001 节点、6434408 字节的 base 加载分配 36232520 字节，见 `docs/agent-notes-evidence/2026-10-30-mapped-load-red.log`。仅基底改映射后，小 delta 仍因全树验证分配 3461640 字节，见 `docs/agent-notes-evidence/2026-10-30-mapped-delta-load-red.log`。修复后同用例约 41 KB/56 KB；改变的节点、delta 重载和旧快照隔离仍正确，见 `docs/agent-notes-evidence/2026-10-30-mapped-encoding-final-green.log`。

Proved: 初次直接读取乱序 CSR 时，差分测试在 mutation 0 node 1 的子节点顺序失败，见 `docs/agent-notes-evidence/2026-10-30-mapped-csr-order-red.log`；归一化该行后，与 heap 解码器在 255 个 checksum 重算的随机单 bit 变化及 1 个合法乱序 CSR 上比较接受/拒绝、每个 slot、统计和子节点顺序均一致，见 `docs/agent-notes-evidence/2026-10-30-mapped-encoding-final-green.log`。既有损坏 base/delta、截断、覆盖及 Rust ABI 回归也保留。

实机 305 MB、约 764 万节点的缓存副本：cache-read 从 3.471 秒降至 892 ms，后续复测 838 ms；完整发现仍为 8355 项、相同指纹。记录见 `docs/agent-notes-evidence/2026-10-30-direct-compaction.log`、`docs/agent-notes-evidence/2026-10-30-mapped-cold.log` 与 `docs/agent-notes-evidence/2026-10-30-ordered-final.log`。事件回放量、文件系统缓存和机器负载随时间变化，不将整体 load 当作固定输入 A/B。


- `internal/core/disk_index_mapped_load_test.go::TestMappedTagCandidatesDoNotCopyUnrelatedLongNames`

Proved: 移除候选名字长度的外层检查后，10000 个长名字加载分配 1295544 字节，见 `docs/agent-notes-evidence/2026-10-30-tag-long-names-red.log`；仅恢复该检查仍因原名称合法性检查的 string 转换分配约 655 KB。现在名字合法性对 string/byte slice 共用泛型字节判据，映射直接传 bytes；标签长度在 string 转换前排除。新 focused 测试低于 64 KiB，名字安全/ABI 差分也通过，见 `docs/agent-notes-evidence/2026-10-30-tag-long-names-green.log`。这不放宽斜杠、NUL、点路径、Windows 反斜杠或 UTF-8 验证。
