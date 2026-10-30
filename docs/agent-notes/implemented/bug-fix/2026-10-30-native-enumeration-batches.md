# Agent Note: 原生枚举的结果存储按批次拥有

Status: implemented

## Problem

`diskWalkParseBatch` 明知一批记录数，仍逐条 append 扩容；返回后原生 worker 又复制首批记录。每目录还为只需要 fd 的 bulk 路径创建 os.File，增加对象和 fcntl。固定 100 个文件目录约 33 KiB/123 次分配，4096 个文件约 1.54 MB。

## Decision

`internal/platform/disk_walk_parser.go` 先验证 count 不超过字节缓冲容量上限，在第一条有效记录解析成功后一次预留该批容量。空批次和首记录失败仍返回 nil，后续失败仍保留已经完整解析的记录；名字继续复制成独立 Go string，不借用 native slab。

`internal/platform/disk_walk_darwin.go` 的首批结果直接成为 worker 独占的 batch.items，后续批次继续追加。bulk 路径直接持有 raw fd，仅 fallback 才包装 os.File；同一 defer 在 wrapper 和 raw fd 中选择一个关闭。超时尚未返回的原生调用仍持有 fd/slab，不能由外层提前释放。

O_NOFOLLOW、目录类型及 dataless 元数据检查、分配大小、hard-link 每路径计数、partial coverage、原 OS 线程 errno、fallback rewind 和 descriptor-relative no-follow 均不改变。

## Alternatives considered

- 持有 native slab 上的名称：下一次 syscall 或池复用会改写旧快照。
- 每条记录扩容、再复制首批：多数目录只有一批，重复开销随所有节点累积。
- 收到 count 立刻无条件分配：先保留非法 count/首记录失败的原有 nil 与错误契约。
- raw fd 和 os.File 两者都关闭：fd 号可能已被重用，第二次关闭可误关其他资源。

## Consequences

结果切片和名称在 worker 返回后仍归接收方独立拥有。count 的容量已受输入缓冲限制；不改变持久化 ABI 或删除授权。fallback 仍使用 os.File 的目录读取支持，native 只省掉不需要的包装。

## Verification

- `internal/platform/disk_walk_test.go::TestDiskWalkBatchAllocatesRecordsOnce`
- `internal/platform/disk_walk_buffers_darwin_test.go::TestDiskWalkNativeClosesDescriptorsOnEveryExit`
- `internal/platform/disk_walk_buffers_darwin_test.go::TestDiskWalkNativeReusesBufferWithoutAliasingResults`

Proved: 修改前 512 条记录解析分配 522 次，超过每个名称一次加结果存储的预算，见 `docs/agent-notes-evidence/2026-10-30-native-records-red.log`；批量预留后同 focused 与既有 malformed/partial、软预算、链接/硬链接、未知覆盖测试通过，见 `docs/agent-notes-evidence/2026-10-30-native-batch-after.log`。

固定 native benchmark 三次运行：空目录 20.2–20.6 us -> 17.7 us；100 文件目录 91.8–99.5 us / 约 33 KiB -> 81.2–84.0 us / 约 10.5 KiB；4096 文件目录 3.85–4.22 ms / 约 1.54 MB -> 3.45–3.73 ms / 约 0.75 MB。见 `docs/agent-notes-evidence/2026-10-30-native-batch-before.log` 与 `docs/agent-notes-evidence/2026-10-30-native-batch-after.log`。微基准不能替代整盘耗时，真实扫描受路径深度、权限和系统负载影响。

Proved: 仅暂时撤掉 raw-fd 分支的 Close，focused 退出路径测试观察到描述符由 5 增至 133，见 `docs/agent-notes-evidence/2026-10-30-native-fd-red.log`；立即恢复后完整原生 focused race 通过，见 `docs/agent-notes-evidence/2026-10-30-native-batch-green.log`。退出路径覆盖成功、取消和软预算；fallback 的既有行为测试继续保留。

真实首次扫描仍约 64.67 秒，spill 构建 4.04 秒；累计分配由此前约 4359 MiB 降到约 2866 MiB。第二轮 load 276.5 ms、discover 1164.8 ms、verify 22.0 ms；两轮均 8356 项、相同 fresh 指纹，见 `docs/agent-notes-evidence/2026-10-30-native-batch-refresh.jsonl`。事件与目录状态动态变化，首次墙钟时间没有证明进一步降低；profile 约 93% 采样仍在原生系统调用内，不将微基准收益直接换算为整盘收益。
