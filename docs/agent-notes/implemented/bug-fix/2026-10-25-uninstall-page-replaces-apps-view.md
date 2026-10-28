# Agent Note: 卸载进行中整页替换应用视图

Status: implemented

## Problem

Go 端把卸载进度画在应用页面的列里（插在统计卡片之前），页面其余部分照常渲染，于是
卡片、标签页和统计从进度页底下透出来，进度页自身也没有背景和边界。Rust 不是这样：
`src/ui/mod.rs` 在卸载进行中直接换掉整个应用视图。

```rust
View::Apps if self.residual.uninstall.is_some() => render_uninstall_progress(self),
View::Apps => render_apps_view(self, window, cx),
```

`render_uninstall_progress`（`src/ui/adapters/progress.rs`）是一整页 CARD：40px 内边距、
560px 居中列、64px 应用徽章、6px 进度条、36px 阶段行和 keep-open 脚注。Go 版本缺徽章、
背景和内边距，进度条也用了 8px。

## Decision

`AppsPage.RenderContent` 在 `Uninstalling` 时只渲染卸载页并 return，`uninstallProgress`
按上表对齐 Rust 结构。`.github` 之外的改动仅此两处；`cmd/ui-gallery` 增加
`--state=uninstalling` fixture，让这个页面能脱离真实卸载复现与比对。

## Alternatives considered

- 保留列表、在进度页下方加遮罩：Rust 没有这种形态，且列表按钮在卸载中本来就不该可点。
- 只补背景不改结构：溢出的标签页与统计仍会露出来，用户看到的仍是错的页面。
- 把卸载改成模态对话框：与 Rust 的整页过程页不符，还会遮住全局状态栏。

## Consequences

卸载期间看不到应用列表、标签页与统计卡片；卸载结束后原样恢复。测试中的 `verifying`
状态从"列表仍在、额外显示阶段"改为"只剩卸载页"，原来的点按钮断言对其它状态照旧。

## Verification

- `internal/ui/apps_page_test.go::TestAppsPageBusyAndNativeCapabilities`

Proved: 把卸载页改回修复前的形状（与列表同页渲染，只删早退 `return` 与整页包装）后，
同一 focused 测试失败于 `apps page still painted behind the uninstall page: "Force Clean"`；
日志见 `docs/agent-notes-evidence/2026-10-25-uninstall-page-replaces-list-manufactured-red.log`。
恢复后 focused 与 `go test ./internal/ui` 全部转绿，见
`docs/agent-notes-evidence/2026-10-25-uninstall-page-replaces-list-green.log`。

视觉复核：`go run ./cmd/ui-gallery --page=apps --state=uninstalling --en --output=<png>`
渲染出与 Rust 相同的整页结构（徽章 / 标题 / 阶段说明 / 6px 进度条 / 三阶段行 / 脚注）。
