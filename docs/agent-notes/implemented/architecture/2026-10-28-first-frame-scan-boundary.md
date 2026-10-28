# Agent Note: 清理目标生成应留在后台扫描

Status: implemented

## Problem

首次绘制前同步生成清理目标会阻塞窗口出现。历史实现还遇到窗口级回调无法使用需要视图上下文的动画帧请求。

## Decision

Rust 的 `src/ui/actions/junk.rs` 在后台 executor 内调用 `all_targets`；启动时构造根视图，进入清理页或托盘操作再发起扫描。不要把目标生成移回首次绘制前的同步路径。动画帧 API 应在正确的视图上下文使用。

## Alternatives considered

在窗口建立时同步生成目标会让文件系统和规则探测延后首帧。

## Consequences

目标生成不阻塞窗口建立；这不表示根视图构造期间不存在其他 I/O。

## Verification

- `internal/app/root_test.go::TestNavigateToJunkTriggersFirstScan` 验证 Go 版本首次导航触发扫描。

Rust 后台生成的边界此次通过源码核对，未找到直接锁住首次绘制时序的自动测试，也未重现历史动画帧错误。
