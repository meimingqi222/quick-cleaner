# Agent Note: 原生目录枚举缓冲区只在调用结束后复用

Status: implemented

## Problem

`internal/platform/disk_walk_darwin.go` 每枚举一个目录分配 256 KiB 缓冲区，百万目录的累计分配约 249 GiB，使原生扫描和 GC 占用大量 CPU。采集超时后原生调用可能仍未返回，简单按超时归还池会造成数据竞争。

## Decision

用 sync.Pool 持有固定数组指针，每次 diskWalkNative 独占一个 slab，只在该函数实际返回时归还。外层 budget 或取消不归还正在调用的缓冲区。解析出的名称复制为 Go string，返回快照不持有 slab 字节。errno 仍在原 OS 线程读取，no-follow、dataless 和未知覆盖语义不变。

## Alternatives considered

- 降低固定 buffer 大小：仍逐目录分配，还可能增加 bulk 系统调用次数。
- 超时时归还 buffer：后台系统调用仍可写入，破坏其他采集和旧快照。
- 零拷贝返回名称：后续池复用会改变已返回的文件名。

## Consequences

每个并发原生 worker 保留一个 buffer，GC 可回收池内空闲对象。挂起 worker 继续持有自己的 slab，不会将其提前共享。race runtime 会主动丢弃部分池对象，回归的累计分配预算为 48 MiB，明显低于旧实现固定的 64 MiB。

## Verification

- `internal/platform/disk_walk_buffers_darwin_test.go::TestDiskWalkNativeReusesBufferWithoutAliasingResults`

Proved: 修复前连续枚举 256 个空目录分配 67256944 字节，超过预算，见 `docs/agent-notes-evidence/2026-10-30-native-buffer-red.log`；复用后相同 focused 测试通过且前后目录名称独立，见 `docs/agent-notes-evidence/2026-10-30-native-buffer-green.log`。
