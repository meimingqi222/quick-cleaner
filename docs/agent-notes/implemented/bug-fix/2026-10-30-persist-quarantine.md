# Agent Note: 索引检查点隔离不可编码节点后重试

Status: implemented

## Problem

2026-10-29 14:23 至 10-30 08:48 的 GUI 日志中，后台保存共 8 次报 `entry exceeds v7 representation`。检查点失败后事件水位不前进，此后每次启动都从同一旧水位回放，事件越积越多；10:57 回放 14.5 万事件并收到根目录重扫，整卷重建约 72 秒。原错误未记录字段，现场坏节点无法确认；`2026-10-30-index-native-time-bounds.md` 修了时间戳这一可复现来源，但任何其他不可编码字段仍会重复同样的连锁。

## Decision

`internal/core/disk_index_persistence.go` 中，保存返回 `ErrDiskIndexCorrupt` 时调用 `quarantineUnrepresentable`，在克隆树上：

- mtime、files 超过 uint32 时饱和，与采集政策一致；
- 新增节点名称非 UTF-8、超长或不安全时剪掉子树、墓碑改名为 `_`，并把父目录加入 blocked，让发现阶段对该目录实时回退；
- base 节点在加载时已验证，不改其身份。

修复数大于 0 才重试一次；已发布给读者的快照不改。压实或全量保存成功后，rebind 会把 owner 换成已隔离的磁盘版本；仅写 delta 时内存快照保留原节点，下次保存会再次隔离，下次启动读到已隔离版本。

## Alternatives considered

- 放宽写入校验或截断名称：破坏缓存格式，且截断名可能与真实同级名冲突。
- 保存失败时整盘重建：正是要避免的 72 秒路径，且坏节点会再次出现。
- 直接修改已发布快照：与并发读者产生数据竞争。

## Consequences

一个坏节点只让其父目录失去覆盖，而不是让整卷检查点停滞。父引用越界等无法修复的错误仍保持失败并记录日志。

## Verification

- `internal/core/disk_index_quarantine_test.go::TestPersistenceQuarantinesUnrepresentableNodes`

Proved: 修复前非 UTF-8 新增名与未来时间使 Flush 失败，错误与现场一致（`entry 5 exceeds v7 representation ... utf8=false`），见 `docs/agent-notes-evidence/2026-10-30-persist-quarantine-red.log`；修复后同 focused 通过，保存隔离 2 个节点、水位为 2，见 `docs/agent-notes-evidence/2026-10-30-persist-quarantine-green.log`。
