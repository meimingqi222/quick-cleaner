# Agent Note: 压实后 owner 必须绑定新 base

Status: implemented

## Problem

增量超过阈值时缓存写出一个重新编号的新 base，owner 却继续保留旧映射。下一轮即使只有水位更新，也因 base checksum 不一致而退出增量快路径，再次全量压实。实机诊断中重复落盘约 13 秒。

## Decision

`internal/core/disk_index_macos.go` 将不可变快照交给 `internal/core/disk_index_persistence.go` 的单写者后台队列；只保留最新待写快照，写入与校验不持有 owner gate，Load 可以立即发布结果。成功压实后重新读取并完整校验已发布缓存，仅当 owner 仍持有对应快照和相同事件水位时采用新映射。旧快照保持独立、仍可供已有读者使用。重载失败时记录错误并保留本轮有效内存快照；扫描不会因诊断/落盘重载失败而凭空丢失数据。`persistOverrides` 的 base、覆盖、身份及水位前提保持不变。

## Alternatives considered

- 放宽 checksum 校验，允许旧 slot 索引在新 base 上写 delta：压实已改变节点编号，会把变更写给错误对象。
- 仅修改内存里的 checksum：没有更新节点与名称池映射，同样错误。
- 每轮同步等待缓存保存：UI 仍要等待约十几秒压实；异步队列保留相同的验证与发布约束。
- 每轮重读整个缓存：安静扫描也重新分配整树；只在已发布新 base 后重新绑定。

## Consequences

压实和重载在后台执行，后续小改动可继续使用增量快路径。新一轮已前进时不能采用较旧的重载；必要时待最新快照保存后再绑定。Close 不等待磁盘保存，CLI/测试通过 Flush 显式等待已排队的持久化并检查错误；应用在后台保存完成前退出可能重放更多历史事件，但不会降低已保存水位或损坏旧缓存。不会跳过任何索引验证或覆盖检查。

## Verification

- `internal/core/disk_index_rebind_test.go::TestMacDiskIndexCompactionRebindsMappedBase`
- `internal/core/disk_index_async_test.go::TestMacDiskIndexSlowPersistenceDoesNotBlockReaders`
- `internal/core/disk_index_async_test.go::TestMacDiskIndexFlushReportsFailureAndPreservesSnapshot`

Proved: 修复前 owner 仍映射旧 base，安静刷新重写整份 base 且没有 metadata-only delta，三个断言均失败，见 `docs/agent-notes-evidence/2026-10-29-index-rebind-red.log`；修复后同测试通过，见 `docs/agent-notes-evidence/2026-10-29-index-refresh-green.log`。

Proved: 异步化前，阻塞保存时 Load 一直不发布快照，见 `docs/agent-notes-evidence/2026-10-29-index-async-red.log`；异步化后测试验证 Load/Peek 不等待、Flush 可取消、更新队列保留最新数据与水位、Close 不阻塞保存，见 `docs/agent-notes-evidence/2026-10-29-index-async-green.log`。压实测试在 Flush 后检查新映射及下一轮 metadata-only delta；已返回的旧快照保持可读。
