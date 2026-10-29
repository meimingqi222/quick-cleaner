# Agent Note: 磁盘透镜删除后在内存索引里摘除节点，不全量重扫

Status: implemented

## Problem

用户反馈磁盘透镜删任何一个文件都会全量重新加载，之前所在的路径也
丢了。Go 移植把删除收尾写成 `disk.Loaded=false + scanDisk()`：整卷
$MFT 重解析数秒，且 `scanDisk` 收尾把 `currentNode` 重置回根。

Rust 参照走 `prune_deleted_from_mft`：把确认删掉的路径从内存树里
摘除、祖先尺寸回填、路径栈退到最近的存活祖先。

## Decision

`pruneDiskPaths`：先取消在飞的文件搜索（遍历同一棵树的 goroutine），
记录 `currentNode` 的祖先链，再对每个已从盘上消失的目标
`FindNodeByPath` + `RemoveSubtreeInplace`；`searchTrees[mount]` 与
`r.tree` 是同一指针，搜索索引自动一致。位置取祖先链里最深的存活
节点，`disk.Space.Free` 回填摘除体积，清勾选并 `rebuildDisk`。

仍在盘上的目标（删除失败或被保护跳过）不摘除。取消清理时仍走旧的
`disk.Loaded=false` 重扫路径（部分删除边界不可靠）。

## Alternatives considered

- 保留全量重扫。Reproduces the reported UX bug on multi-second MFT volumes.
- 移除路径时只摘 UI 行不改树。Rejected: 搜索索引和祖先体积会继续
  撒谎，比慢还糟。

## Consequences

`SizeTree` 首次在加载后被主线程原地修改；改动前先 `searchCancel` +
代次递增，遍历 goroutine 只读 slots/parent 链、不碰 `childNames`，
窗口内的并发读只会看到过期 slot，不会崩。macOS 上 owner 持有的同
一棵树同步更新，FSEvents 之后也会补上同一笔删除。

## Verification

- `internal/app/disk_prune_test.go::TestDiskDeletePrunesTreeAndKeepsPosition`

Proved: 修复前收尾路径无条件 `Loaded=false + scanDisk()`，没有可测的
内存摘除行为（行为即重扫+回根）；实现后 focused 测试锁定「只删已消失
目标、位置保住/退到存活祖先、free 回填」。
