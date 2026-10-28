# Agent Note: 智能清理独立探测重叠执行并共享遍历额度

Status: implemented

## Problem

智能清理在文件系统目标生成前串行运行 Docker、快照与 Homebrew 预览，随后等待磁盘索引；这些独立探测的等待时间叠加。无索引或覆盖不完整时，多个固定目标又各自创建目录遍历的并发额度，容易扩大磁盘竞争。

## Decision

`internal/app/junk_providers.go` 同时启动独立探测，与目标生成和索引加载重叠，汇合时按原 provider 顺序合并。异常在汇合处回传到原产品工作线程的 panic 恢复边界。`internal/core/fs_query.go` 和 `internal/core/junk_index.go` 让同一扫描的实时大小统计共享有界的目录遍历额度；索引和安全检查的选择不变。取消后不发布探测结果。

## Alternatives considered

在 UI 线程等待或向 UI 分批写共享切片：会阻塞界面或引入并发写。取消全部实时回退：未知覆盖不能当作零。为每个目录不限量创建任务：容易造成 I/O 风暴。

## Consequences

独立命令的等待可以重叠；加速程度取决于系统和可用索引。探测仍有各自已有的超时。固定目标、发现式预算、占用检测和清理安全验证保持。

## Verification

- `internal/app/junk_providers_test.go::TestJunkProvidersRunTogetherAndKeepOrder`
- `internal/app/junk_providers_test.go::TestJunkProviderPanicReachesWorker`
