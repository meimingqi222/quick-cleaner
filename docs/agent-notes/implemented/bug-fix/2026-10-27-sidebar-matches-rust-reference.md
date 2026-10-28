# Agent Note: 侧栏底栏按 Rust 参照保持三行，不补「自动检查更新」开关

Status: implemented

## Problem

侧栏底部设置区错乱：勾选框「自动检查更新」压在语言行和版本号之间，190px 宽的底栏被撑开，
「English」胶囊贴到边框上，版本号被挤到边缘。

追根是上一轮（`2026-10-25-auto-check-toggle-in-sidebar.md`）的判断错了。当时为了修 Windows
上重复标题栏，把应用菜单改成只在 macOS 装，并给「自动检查更新」在侧栏补了个页面入口，
理由是「菜单是它唯一的入口，去掉菜单就等于去掉这个设置」。

核对该前提不成立：Rust 参照树里 `auto_check_updates` 只有三处读取
（`src/core/settings.rs` 的默认值与反序列化、`src/ui/actions/updater.rs` 的调度判断），
**全仓库没有任何 UI 写入点**。侧栏 `src/ui/shell/sidebar.rs` 的底栏就是三行：
已释放空间、界面语言胶囊、版本行。也就是说这个开关在 Rust 里从来不存在，是移植时为了
「菜单不能白删」凭空补出来的控件。

## Decision

删掉这个复选框及其整条接线：`Dashboard.AutoCheck`、`DashboardActions.SetAutoCheck`、
`dashboardCopy.autoCheck`（中英两串）、`internal/app/root.go` 里的赋值与回调。

保留的部分与 Rust 一致，不动：

- `core.Settings.AutoCheckUpdates` 的持久化与默认值；
- `UpdateController.SetAutoCheck` 与 `Preferences.AutoCheck`——调度器仍按它决定是否自动检查，
  `internal/app/dialogs.go` 仍从设置初始化、并回写「跳过版本 / 上次检查时间」。

也就是说：自动检查更新**照常按设置生效**，只是不再有一个只出现在 Go 版里的开关去改它。

## Alternatives considered

- **留着开关，只修布局。** 治不了根本：Rust 没有这个控件，多出来的第四个控件本身就是
  与参照不符；再窄的底栏要同时塞进「标签 + 两个胶囊 + 版本」也只能靠压缩间距。
- **把开关挪进更新对话框。** 对话框只在发现新版本时出现，平时没有入口，等于把一个设置藏起来；
  Rust 也没有这个入口。
- **顺手把设置字段一起删掉。** 会让自动检查永远锁死在默认值，且与 Rust 的 settings schema
  分叉，旧配置文件里的字段也失去意义。

## Consequences

侧栏底栏恢复成 Rust 的三行，190px 宽度下不再挤压。`SetAutoCheck` 从 `DashboardActions`
消失后，`UpdateController.SetAutoCheck` 变成只有测试在调——它仍是更新调度的一部分，
不删；真要删它属于另一件事（改更新行为），不在这次 UI 对齐范围内。

复现这台机器上的错乱不需要真实窗口：`ui.NewTester` 的 `Texts()` 会把这个底栏的全部文案
列出来，多出来的那条一眼可见。

## Verification

- `internal/ui/dashboard_test.go::TestSidebarSettingsAreaMatchesTheRustReference`

Proved: 修复前该测试自然红跑，`Texts()` 里明确列出多出来的
`Check for updates automatically`，见
`docs/agent-notes-evidence/2026-10-27-sidebar-auto-check-red.log`；删掉开关后同 focused 转绿，
见 `docs/agent-notes-evidence/2026-10-27-sidebar-auto-check-green.log`。`go test ./internal/ui
./internal/app` 全绿。

Pitfalls checked: 逐条核对 P1–P89。P82 的「不能只删菜单而不给页面入口」原本就是基于误判的
前提，本次按参照纠正为「菜单本就没有这个开关，页面也不该凭空长出」；P82 的其余约束（应用菜单
只在 darwin、`删除到回收站` 的唯一入口是磁盘透视页）不变。P88 的「面板说明不能塞进共享
`Notice`」与本次改动无关。UI 守卫：`check-ui-localization.py`、`check-ui-components.py` 通过。
