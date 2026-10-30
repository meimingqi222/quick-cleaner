# Agent Note: FSEvents 编号差不能代替历史丢失证据

Status: implemented

## Problem

前台 refresh 在 current-since 超过 400 万时跳过 Changes，直接全盘重建；后台则永久推迟至交互重建。真实缓存副本在编号差 4178190 时触发该路径，首轮 load 74.076 秒，尽管尚未询问原生回放是否仍有可用历史。

## Decision

`internal/core/disk_index_macos.go` 的 refresh 仅在读取 current 失败或 current 小于已存水位时提前请求重建。正向编号差交给原生 Changes 进行有界回放：编号不是这个 root 下变化数，不能自行证明日志已丢失。

`internal/platform/fsevents_darwin.go` 既有 5 秒回放预算和 `internal/platform/fsevents.go` 的历史标志判定不变：RequiresFullScan、RootChanged、EventIdsWrapped、无法取得可用交付、回退水位及必须重扫根仍走原保护。未完成的历史仅采用实际 LastEventID，不提升到 current；应用变化成功才前进内存水位，保存失败不得推进磁盘检查点。后台仍绝不执行全盘重建。

## Alternatives considered

- 提高固定编号阈值：只是推迟同样的误判，编号差仍不等于损坏或真实工作量。
- 直接将水位设为 current 跳过事件：会永久漏变更。
- 去掉所有重建条件：损坏历史、编号回绕和根变化仍需要保守处理。
- 缩短原生预算或忽略 MustRescan：改变覆盖契约，本次不采用。

## Consequences

较旧检查点可先回放有效历史，避免仅由编号差触发的分钟级重建。真实事件很多时仍会耗费回放、增量更新和后台压实时间；历史不可用仍可要求重建。既有 background-never-rebuild 用例的 stale 场景改为真实编号回退，新增大正向编号差的前台/后台与完整/前缀场景。

## Verification

- `internal/core/disk_index_macos_test.go::TestMacDiskIndexLargeEventIDGapReplaysDeliveredHistory`

Proved: 修复前 4 个大编号差场景均未尝试有效回放，前台报 forbidden full rebuild、后台报 needs an interactive rebuild，见 `docs/agent-notes-evidence/2026-10-30-event-gap-red.log`；修复后同 focused 加既有丢失/回绕/根重扫/取消/交付水位测试 race 通过，见 `docs/agent-notes-evidence/2026-10-30-event-gap-green.log`。新增测试还锁定新文件合入、旧快照隔离，以及完整或未完成交付均只持久化 delivered 水位。

现场修复前的 CLI 三轮结果见 `docs/agent-notes-evidence/2026-10-30-final-refresh.jsonl`，第 1 轮因编号差重建，第 2/3 轮复用新索引。动态事件与权限覆盖会变化，不能把不同轮当成相同输入的 A/B。

修复后的真实旧缓存回放成功取得原生 root-rescan 扩展（Paths=0、MustRescan=20），随后按既有大范围变更保护进行重建，首轮仍为 75.258 秒；没有将它宣称为实际首轮提速。记录见 `docs/agent-notes-evidence/2026-10-30-event-gap-after.jsonl`。该记录第 2/3 轮与全量回归运行重叠，仅保留为结果一致性诊断，不作为隔离性能对照。
