# Agent Note: 加载指示器不响应减少动画设置，始终按显示频率动

Status: implemented

## Problem

用户反馈 Windows 下扫描没有动画、像卡死一样。实测该机器的
`SPI_GETCLIENTAREAANIMATION` 为 False（「调整为最佳性能」、RDP、精简
工具或辅助功能设置都会关掉它），MyGo 据此把 `ReduceMotion` 置真，
`indicators.go` 全部指示器走 `still` 分支：画一张静态图并**不再调用**
`Painter.AnimationFrame()`。扫描的静默阶段（MFT 解析、目录遍历）连一帧
重绘都没有，界面看起来完全冻结。

先按「低频步进」修过一版（500ms 跳一格），用户在 RDP 下实测仍然觉得
不流畅，明确要求与 Rust 参照一致：加载指示器不要管这个系统开关。

## Decision

`indicatorNow(p)` 统一为 `AnimationFrame` + 当前绘制时钟，所有加载
指示器（`scanBar`、`loadingSpinner`、`orbitSpinner`、`orbitDot`）不读
`c.Preferences().ReduceMotion`，始终按显示频率动画——与 Rust 参照的
行为一致。这是有意覆盖桌面无障碍偏好：对「正在干活」的进度信号，
静止或卡顿的代价（误判为卡死）大于平滑动画的代价。

## Alternatives considered

- 静止（初版移植的选择）。Reproduces the reported bug: a 30s scan looks
  like a hang.
- 低频步进（中间版）。Rejected by the user: still choppy, especially
  over RDP where effective frame delivery already degrades.
- 按指示器逐个判断偏好。Rejected: more gates for the same outcome—the
  product wants progress signals that always move.

## Consequences

`Preferences().ReduceMotion` 在这份文件里不再被读取；MyGo 自带控件
（滚动惯性等）不受影响，那是框架层行为。`loadingState` 的省略号仍在
构建期取时钟，随页面重建刷新，不影响本决策。

## Verification

- `internal/ui/indicators_test.go::TestIndicatorsAnimateUnderReducedMotion`

Proved: the test was written before the fix and failed for all four
indicators（各报 `asked for no next frame under reduced motion`）；see
`docs/agent-notes-evidence/2026-10-29-reduced-motion-always-animate-red.log`。
修复后同 focused 测试全绿，见
`docs/agent-notes-evidence/2026-10-29-reduced-motion-always-animate-green.log`。
