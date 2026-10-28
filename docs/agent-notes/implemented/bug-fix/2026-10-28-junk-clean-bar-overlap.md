# Agent Note: 智能清理底部胶囊条在矮窗口压住状态栏

Status: implemented

## Problem

用户反馈 Windows 上智能清理页底部错乱：`已选择清理 … GB (146 项)` 胶囊条压到全局状态栏上，按钮底边被窗口裁掉。窗口较高时不出现，因此容易被当成截图裁剪。

## Decision

`JunkPage.RenderContent` 的根列原先用 `Fill()`（宽高 100%）。它位于 44px 顶栏之后，却按整个内容列的高度取尺寸，比可用空间多出顶栏高度；超出部分没有裁剪，直接盖住底部的全局状态栏。窗口越矮、滚动内容越高，越容易触发。

改为与 `DeclutterPage`、`DiskPage`、`DevEnvPage` 一致的 `Grow(1).FillWidth().MinHeight(0)`：根列只占据顶栏之后的剩余高度，滚动区自行收缩，胶囊条始终贴在内容区底部、状态栏之上。

## Alternatives considered

- 给页面根列加 `Clip()`：只能遮住溢出，胶囊条本身仍会跑到内容区外，底部留白和滚动区高度依旧是错的。
- 按窗口高度做阈值判断：把布局常量绑到具体像素，改顶栏高度或 DPI 后再次失效。

## Consequences

任意窗口高度下，胶囊条与状态栏保持固定间距，不再互相覆盖。

## Verification

- `internal/ui/junk_test.go::TestJunkCleanBarStaysAboveTheStatusBar`

Proved: 撤销根列的 `Grow(1).FillWidth().MinHeight(0)`、恢复 `Fill()` 后，该测试在 800 高窗口失败：`800: clean bar bottom 787.0 overlaps the status bar at 778.0`，见 `docs/agent-notes-evidence/2026-10-28-junk-clean-bar-overlap-red.log`；恢复后同 focused 绿跑，见 `docs/agent-notes-evidence/2026-10-28-junk-clean-bar-overlap-green.log`。
