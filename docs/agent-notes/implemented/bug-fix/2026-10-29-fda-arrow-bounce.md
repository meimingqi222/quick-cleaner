# Agent Note: Go 版完全磁盘访问引导保留 Rust 箭头弹跳

Status: implemented

## Problem

用户看到完全磁盘访问拖拽提示里的箭头静止。Go 移植保留了面板、箭头和
`fdaArrow` 引用，却遗漏 Rust 的 `ensure_bounce_animation`，因此没有挂动画。

## Decision

`internal/platform/permission_drop_darwin.go` 给箭头启用 layer，并通过
`ensureFDABounceAnimation` 添加 `CABasicAnimation`：`position.y` 从原位置向上
8pt，单程 0.6 秒，自动往返、无限重复、`easeInEaseOut`。

创建内容与每次显示都检查动画是否存在；已有 `bounce` 时直接返回，丢失时补挂。
显示会被控制器每 150ms 轮询，不能无条件重新添加同名动画，否则不断重启，
用户看到的又会是近乎静止或抖动。

## Alternatives considered

**每次显示重新添加动画。** 会不断重置进度。

**只在创建时添加。** 面板再次显示时，系统若已摘掉动画便无法恢复。

**Go 定时器逐帧改位置。** 原生 Core Animation 已能完成插值，无需再增加刷新循环。

## Consequences

动画与 Rust 参照保持一致，仍由现有主线程发布器调用，不改权限检查、拖拽载荷
或面板定位。测试直接构造原生内容、读取 CALayer 上的动画，验证幅度、时长、
方向、无限往返，以及重复检查不重启、动画丢失后补挂；不打开系统设置或更改权限。
原生检查在独立子进程的初始化主线程执行：仅锁住 Go 测试 goroutine 所在的线程
不保证它是 AppKit 主线程，完整测试序列曾在 NSImageView 图像准备处触发主线程断言。

## Verification

- `internal/platform/permission_drop_darwin_test.go::TestFDADropArrowHasRustBounce`

Proved: 修复前自然红跑，断言报 `FDA arrow is static: missing Rust bounce animation`，
证据见 `docs/agent-notes-evidence/2026-10-29-fda-bounce-red.log`。迁移后同 focused
测试通过，见 `docs/agent-notes-evidence/2026-10-29-fda-bounce-green.log`。

`go build ./...`、`go vet ./...`、notes 与 PITFALLS 指针校验通过。全量 Go race
测试在沙箱外通过；沙箱内 HOME 下照片 fixture 建目录被拒绝、`vm.swapusage`
读取失败及 FSEvents ID 为 0。Rust 全量库测试在沙箱外 840 通过、12 忽略。
