# Agent Note: 智能清理未扫描空白与软件卸载点阵加载动画复刻

Status: implemented

## Problem

用户在体验时反馈两个明显的界面与交互问题：
1. **智能清理（Junk 页面）进去是一片空白**：初次进入智能清理页面时，中间无内容、无扫描动画，只有右上角不起眼的“重新扫描”和底部 0B 的清理栏，页面严重缺乏指引和即时反馈。
2. **软件卸载（Apps 页面）列表检索时完全没有动画**：Rust 原版在检索已安装软件时，展示了一个 64px 的粒子点阵 Spinner（中心呼吸光晕 + 8 瓣渐变轨道旋转圆点）及动态流动的省略号（`format!("{}{}", title, loading_dots)`）；而在 Go 版本中，该页面扫描占位被写成了简陋的静态灰色纯文本，没有任何动效。

## Decision

1. **智能清理导航自洽与未扫描空状态兜底**：
   - 在 `internal/app/root.go` 的 `Navigate(v)` 中补齐 `case view.Junk`：若未扫描过且未忙碌，自动启动首次 `scanJunk()`，与「软件卸载」「磁盘透镜」「文件检索」保持一致的开箱即扫行为；
   - 在 `internal/ui/junk.go` 中增加未扫描兜底状态 `unscannedPrompt`：若由于任何原因处于未扫描状态（`!s.Scanned && !s.Scanning`），在页面中央呈现醒目的清理图标、标题“尚未扫描系统冗余垃圾”以及“开始全面扫描”主按钮，同时在未扫描时隐藏底部胶囊栏，彻底告别死白页面。
2. **复刻 Rust 原版的点阵呼吸 Spinner 与 LoadingState**：
   - 在 `internal/ui/indicators.go` 中基于 `Draw` 回调与 `Painter.AnimationFrame()` 实现 `orbitSpinner`（64px 8 瓣旋转圆点 + 中心正弦脉冲呼吸光晕）及 `loadingDots`（4 拍流转省略号），不重构整页也能维持平滑动效；
   - 提取通用的 `loadingState(c, title, sub)`，并在「软件卸载」和「智能清理」扫描中统一使用，使视觉风格与动效完全对齐 Rust 原版。

## Alternatives considered

- **仅在智能清理页面加自动扫描，不加未扫描空状态**：若用户手动取消或状态未持久化，仍会再次复发页面空白问题。增加未扫描空状态卡片提供了双重防御。
- **沿用旧版简易弧线 `loadingSpinner(28)`**：虽然有动画，但视觉上与 Rust 原版的 64px 点阵圆点 Spinner 存在巨大差异，用户明确感知到了视觉降级。

## Consequences

- 首次点击侧栏「智能清理」直接无缝开启系统垃圾扫描并展示流动点阵动效；即使未扫描也具备居中大按钮引导，不再出现白屏。
- 软件卸载页面扫描占位完全恢复 Rust 参照的粒子 Spinner 与动态省略号动效。

## Verification

- `internal/app/root_test.go::TestNavigateToJunkTriggersFirstScan`
- `internal/ui/junk_test.go::TestJunkPageUnscannedStateRendersStartScanPrompt`
- `internal/ui/indicators_test.go::TestOrbitSpinnerKeepsAnimatingOnItsOwn`
- `internal/ui/apps_page_test.go::TestAppsPageKeepsRowsWhileMeasuring`

Proved:
1. 导航未触发首次扫描红跑：注释 `root.go` 中的 `case view.Junk`，`TestNavigateToJunkTriggersFirstScan` 失败 `navigating to junk did not trigger first scan`，见 `docs/agent-notes-evidence/2026-10-27-junk-auto-scan-red.log`；恢复后同 focused 绿跑，见 `docs/agent-notes-evidence/2026-10-27-junk-auto-scan-green.log`。
2. 智能清理未扫描空白红跑：撤销 `unscannedPrompt`，`TestJunkPageUnscannedStateRendersStartScanPrompt` 失败并抓到空白页面文本 `unscanned page did not show prompt: [智能清理 ... 发现可清理内容 —]`，见 `docs/agent-notes-evidence/2026-10-27-junk-unscanned-prompt-red.log`；恢复后同 focused 绿跑，见 `docs/agent-notes-evidence/2026-10-27-junk-unscanned-prompt-green.log`。
