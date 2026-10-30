# Agent Note: macOS 索引采集时间戳遵守 v7 范围

Status: implemented

## Problem

真实进程后台保存报 `entry exceeds v7 representation`，磁盘水位停留在前一天。原错误未记录节点或字段，本轮真实 CLI 回放未复现该坏节点，因此不能确认现场是哪一个字段。

定向复现发现一个确定缺口：MacWalkEntry 与文件事件的 mtime 为 64 位，v7 记录只有 32 位；普通扫描与增量 Lstat 原样传入超过 2106 年边界的时间，就会使整个检查点保存失败。现有 spill 通道已经饱和到 MaxUint32，但内存通道没有采用同一政策。

## Decision

`internal/core/disk_index_macos.go` 在内存扫描采集与增量文件元数据采集处将正时间戳饱和到 MaxUint32，与 spill ABI 一致。零值和正常时间保持，未来时间不会截断回很早的日期。此规范化只处理 macOS 采集数据；通用缓存写入仍严格拒绝不可表示字段，未放宽父子关系、名称、覆盖、checksum 或水位校验。

`internal/core/disk_index_cache_format.go` 的表示范围错误补充节点编号、parent、mtime、files、名称字节数与 UTF-8 是否有效；不记录完整路径或名字，以便下次真实错误区分字段。

## Alternatives considered

- 删除持久化范围校验：会静默截断数据，破坏缓存格式。
- 仅改为 uint32 强转：未来时间回绕成过去时间，可能误判年龄。
- 跳过整个未来文件或不推进事件水位：会丢数据或每次启动重复回放。
- 宣称已定位现场原坏节点：真实回放成功，只能证明该错误类的可复现边界修复。

## Consequences

高于 v7 上限的时间在索引里表示为最大未来值。清理仍独立读取实时身份和证据，不以缓存时间授权删除。原始现场错误若由其他字段导致，新增诊断仍会保留严格拒绝，并给出可定位信息。

## Verification

- `internal/core/disk_index_timestamp_test.go::TestMacWalkIndexBoundsTimestampsBeforePersistence`
- `internal/core/disk_index_timestamp_native_test.go::TestMacIndexFutureFileTimestampPersistsAcrossRestart`

Proved: 修复前普通采集的 4294967296/MaxUint64 仍原样进入树并阻止保存，见 `docs/agent-notes-evidence/2026-10-30-index-acquisition-growth-red.log`。仅撤销增量采集上限，真实临时文件的 mtime=4294970895 使 Flush 在范围校验失败，见 `docs/agent-notes-evidence/2026-10-30-native-future-time-red.log`；恢复后同 focused 绿跑，且磁盘水位与重启后的 owner 均为 2，未来时间保持 MaxUint32，见 `docs/agent-notes-evidence/2026-10-30-index-final-green.log`。
