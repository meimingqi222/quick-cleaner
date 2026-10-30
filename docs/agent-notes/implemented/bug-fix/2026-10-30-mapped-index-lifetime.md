# Agent Note: 映射回收必须等待最后一个读者

Status: implemented

## Problem

成功加载的 mmap 没有资源回收路径。后台压实重载和多轮扫描持续创建映射，旧树不可达后仍保留映射。直接给 owner 加自动回收又会让只保留 CSR 字节的子节点视图在 GC 后访问已解除的映射。

## Decision

`internal/core/disk_index_mapped_unix.go` 仅为 mapDiskIndex 创建的正式映射登记 runtime.AddCleanup，参数只持有文件与映射字节，不反向引用 owner；显式 close 先取消 cleanup。builder 的借用 spill wrapper 不登记 cleanup，仍由 builder 解除映射。

`internal/core/disk_tree.go` 的 treeChildView 携带映射 owner；读取后 KeepAlive。数值、名称、标签比较和头部访问也保持 owner 到最后一次使用之后。header 返回 128 字节副本，名称池仅在持有 owner 的方法内部使用，编码器只查询 poolLen。`internal/core/disk_index_cache.go` 和 `internal/core/disk_index_cache_format.go` 在原始映射字节的完整解析/覆盖恢复期间保持 owner 可达。

## Alternatives considered

- 只添加 finalizer/cleanup：原始 CSR slice 不构成对 owner 的引用，可导致释放后访问。
- 切换 owner 时立即 close 旧映射：旧 UI、后台持久化和克隆快照仍可能读取。
- cleanup 参数持有 owner：自我保活，永远不会回收。
- 对借用 spill wrapper 注册 cleanup：与 builder 的 unmap 重复，可能提前解除可写映射。

## Consequences

正式映射在最后读者不可达且 GC 触发后回收，不承诺立即回收或退出前执行。仍存活的快照及子节点视图共享不可变映射。手动 close 只用于尚未发布或已确定无读者的路径，不作为并发关闭 API。

## Verification

- `internal/core/disk_index_mapped_lifetime_test.go::TestMappedOwnerReleasesResourcesAfterLastReader`
- `internal/core/disk_index_mapped_lifetime_test.go::TestMappedChildReaderKeepsOwnerUntilReleased`

Proved: 修复前放弃映射 owner、单独保留 os.File 强引用后，反复 GC 仍未关闭文件，见 `docs/agent-notes-evidence/2026-10-30-mapped-lifetime-red.log`；修复后 owner 回收和保留子节点视图两项通过，见 `docs/agent-notes-evidence/2026-10-30-mapped-lifetime-green.log`。

Proved: 仅撤掉 children 返回视图的 owner 引用，focused 子节点测试在 GC 后报 child reader lost its mapping owner、file already closed，见 `docs/agent-notes-evidence/2026-10-30-mapped-child-lifetime-red.log`；立即恢复后同 focused 全绿，见 `docs/agent-notes-evidence/2026-10-30-mapped-lifetime-green.log`。单独保留文件引用排除了 os.File 自身 GC 造成的假阳性。
