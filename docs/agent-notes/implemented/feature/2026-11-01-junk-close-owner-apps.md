# Agent Note: 清理前一次性询问关闭正在运行的缓存所属应用

Status: implemented

## Problem

按文件独占删除（P120）后，浏览器运行时它持有的缓存文件会被跳过，但用户不知道
是哪个应用占着，也无法在清理时顺手处理。原来列表每行有「退出应用后清理」按钮，
要逐项点击，且只覆盖部分缓存项；用户要求的产品行为是：用户自己的应用占用时，
清理前提示关闭；系统进程占用时自动跳过。

## Decision

- **规则声明所属进程。** `rules/browsers.toml` 用 `[lists]` 声明
  `owner_processes_<目录行 en 名小写>`（chrome.exe / msedge.exe / brave.exe）。
  用 `lists` 而不是给 `Layout` 加字段，是因为当时 Rust 参照树仍以
  `deny_unknown_fields` 解析同一份 TOML。`validateOwnerProcesses` 要求键对应
  `windows_user_data` 中已有的行、只在 `browsers` 规则中出现、值为 1–8 个不含路径的
  `.exe` 映像名。
- **`ScanTarget` / `ScanItem` 带 `Owner *AppOwner`。** 只用于提示和征询，
  不授予删除或结束权限。
- **扫描时只读进程列表。** `platform.RunningAppOwners` 用 Toolhelp 快照按映像名
  匹配，再用缓存释放同一套 `cacheVerifyHandle` 校验：同一用户、非关键进程、非
  Explorer、可执行文件路径一致。其他用户、服务和关键进程直接省略，不报错。
  `RunningOwnerBusy` 生成「应用打开中 · Chrome」标签，随占用 badge 异步到达，
  不影响勾选。
- **一次确认，按应用合并。** 点「立即清理」后，在 worker 上执行
  `FindRunningApps`（期间 `JunkPage.Preparing` 禁止重复点击，生成号变化则丢弃
  结果），然后只弹一个确认框；正在运行的应用以默认勾选的复选框列在其中。
  - 勾选：清理前 `CloseRunningApps` 对该应用的同名进程逐个请求正常退出
    （`WM_CLOSE`，每次仍复核身份），最多等 8 秒。**不强制结束**：没退出的应用，
    它仍持有的文件由按文件独占删除跳过，状态栏说明「未在 8 秒内退出」。
  - 未勾选：该应用的缓存项本次不进入清理目标，状态栏说明已跳过几项。
- **去掉每行的「退出应用后清理」按钮**，入口只剩确认框。「已跳过」面板中原有的
  「退出应用后重试」（走 `requestCacheRelease`，含强制结束二次确认）保留。
- **CLI**：无法交互，默认把正在运行的所属应用的缓存排除在外；`--close-apps`
  才请求退出。报告 `running_apps` 列出应用、路径、是否请求退出、是否仍在运行。

## Alternatives considered

- **用 Restart Manager 在清理前探测每个目标的占用者。** 4.2k 文件约 10.7 秒，
  一次清理几十个缓存目录不可接受；声明式进程名只需毫秒级的进程列表。
- **独立的第二个对话框。** 先确认清理再弹「关闭应用」，用户要连点两次；合并进
  同一个确认框。
- **8 秒后强制结束。** 会丢未保存的标签页或输入；按文件删除已经能安全地绕开
  仍被持有的文件，不值得冒这个风险。需要强制时，「已跳过」面板的重试入口仍有
  二次确认。
- **未勾选的应用也按文件清理它没打开的缓存。** 应用运行中删除其缓存索引之外的
  文件可能让它状态不一致；用户选择不关闭，就整体跳过。
- **给 `Layout` 新增 `processes` 字段。** 当时会让 Rust 参照树解析失败；`lists`
  在两边都是通用结构。

## Consequences

- 只有 Windows 上声明了进程的浏览器（Chrome、Edge、Brave）会出现在确认框里；
  Electron 应用缓存等动态发现的目标没有进程映射，被占用的文件只按文件跳过。
- macOS 的 `RunningAppOwners` 目前返回空。
- 进程名可以被同名程序冒用；确认框只按名称征询，退出请求仍复核同一用户、
  非关键进程与可执行文件路径，且只发正常退出请求。

## Verification

- `internal/core/app_owner_test.go::TestBundledBrowserOwnersAreDeclared`
- `internal/core/app_owner_test.go::TestOwnerProcessListsMustMatchBrowserRows`
- `internal/core/app_owner_test.go::TestFindRunningAppsGroupsByOwnerAndSkipsIdleApps`
- `internal/core/app_owner_test.go::TestRunningOwnerBusyBadgesOnlyRunningOwners`
- `internal/core/app_owner_test.go::TestCloseRunningAppsAsksNormallyAndReportsStragglers`
- `internal/core/app_owner_test.go::TestWindowsBrowserTargetsCarryTheirOwner`
- `internal/platform/app_owners_windows_test.go::TestRunningAppOwnersListsOwnProcessByImageName`
- `internal/platform/app_owners_windows_test.go::TestRunningAppOwnersOmitsSystemProcesses`
- `internal/app/junk_owner_test.go::TestJunkCleanSkipsUncheckedRunningOwnerApps`
- `internal/app/junk_owner_test.go::TestJunkCleanQuitsCheckedOwnerAppsFirst`
- `internal/app/junk_owner_test.go::TestJunkCleanWithoutRunningOwnersShowsNoAppList`
- `internal/ui/product_dialog_test.go::TestConfirmationListsRunningAppsAsToggles`
- `internal/ui/junk_test.go::TestJunkPageSelectionAndIndependentActions`
- `internal/cli/cli_test.go::TestExecuteJunkLeavesRunningOwnerCachesOutUnlessAskedToClose`

Proved: 制造型红跑，证据见
`docs/agent-notes-evidence/2026-11-01-owner-apps-red.log`：让 `runJunkClean` 把所有
列出的应用都当作已勾选时，`TestJunkCleanSkipsUncheckedRunningOwnerApps` 报
「test closed real applications」并列出被尝试的 chrome-cache；去掉
`validateOwnerProcesses` 的 `!rows[key]` 时，`TestOwnerProcessListsMustMatchBrowserRows`
报「unknown row accepted」。恢复后同组及其余第 3 阶段测试共 13 个全部通过，见
`docs/agent-notes-evidence/2026-11-01-owner-apps-green.log`。

Pitfalls checked: P66（只对已分类为浏览器缓存的目标询问，结束仍走缓存释放的身份
复核，不绕过分类）、P117（Restart Manager 不进入清理前路径）、P119（运行中标签不改
推荐与勾选）、P120（未退出的应用由按文件独占删除兜底）。新增 P121。
