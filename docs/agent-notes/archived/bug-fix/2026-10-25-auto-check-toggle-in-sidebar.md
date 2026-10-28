# Agent Note: 自动检查更新挪进侧栏设置区，应用菜单只留 macOS

Status: implemented
Archived: 2026-10-27
Superseded-by: 2026-10-27-sidebar-matches-rust-reference.md

## Problem

`installMenu` 在 Windows/Linux 也装了应用菜单：MyGo 把菜单画进客户区，于是原生标题栏下面
多出一条写着同名文字的菜单栏。更实际的问题是，菜单是**唯一**能改「自动检查更新」的入口，
去掉菜单就等于去掉这个设置。Rust 参照里 `auto_check_updates` 没有任何 UI（只在
`src/core/settings.rs` 持久化，供 `src/ui/actions/updater.rs` 使用），所以这是移植时补出来
的入口，而不是原本就有的页面控件。

「删除到回收站」不是缺口：磁盘透镜页早就有 `disk-recycle` 复选框（`DiskPage.ToRecycleBin`
→ `SetRecycleBin` → `internal/app/disk.go` 写回 settings），菜单里那一条是重复入口。

## Decision

- 侧栏底部（语言切换、版本号所在的常驻区域）增加「自动检查更新」复选框：
  `Dashboard.AutoCheck` + `DashboardActions.SetAutoCheck`，文案走 `dashboardCopy.autoCheck`，
  动作写 `settings.AutoCheckUpdates`、同步更新控制器并保存。
- `installMenu` 只在 `darwin` 安装应用菜单，并去掉已移到页面的两个开关项，避免同一设置出现
  两个来源；macOS 菜单保留扫描、停止、检查更新、权限和退出。
- 撤掉上一轮为隐藏菜单栏加的 `opts.AutoHideMenuBar`：Windows/Linux 不再装菜单，也就没有
  需要在窗口内隐藏的菜单栏。

## Alternatives considered

- 只保留 `AutoHideMenuBar`（上一轮的做法）：菜单里的条目仍与页面重复，且要按 Alt 才看得见。
- 把开关放进更新对话框：对话框只在发现新版本时打开，平时用户没有入口切换。
- 新做一个独立设置页：侧栏底部已经是语言与版本的常驻区，为一个开关新开页面收益不足；
  真的长出第三、第四个设置项时再升级成页面。

## Consequences

Windows/Linux 没有应用菜单，也不会再出现重复标题栏；「自动检查更新」随时可在侧栏切换，
macOS 仍用系统菜单栏且保留 Cmd+Q。「删除到回收站」维持磁盘透镜页的唯一入口。

## Verification

- `internal/ui/dashboard_test.go`（当时的 `TestSidebarOffersAutoUpdateToggle`，已被
  `2026-10-27-sidebar-matches-rust-reference.md` 的 `TestSidebarSettingsAreaMatchesTheRustReference` 取代）

Proved: 修复前该断言把侧栏全部文案打印出来，里面没有任何自动检查更新选项；日志见
`docs/agent-notes-evidence/2026-10-25-auto-check-toggle-red.log`。修复后 focused 通过，
`go test ./internal/ui ./internal/app` 全绿，见
`docs/agent-notes-evidence/2026-10-25-auto-check-toggle-green.log`；点击开关会依次上报
true、false，说明复选框与 `AutoCheck` 状态双向一致。

UI 守卫：`check-ui-localization.py`、`check-ui-components.py` 均通过（文案走 i18n 表，
没有新增内联字符串）。
