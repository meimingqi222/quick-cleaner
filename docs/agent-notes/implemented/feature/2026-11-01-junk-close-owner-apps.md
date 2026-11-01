# Agent Note: 清理前一次性询问关闭正在运行的缓存所属应用

Status: implemented
Partly-superseded-by: 2026-11-01-junk-streamlined-clean-ux.md

## Problem

按文件独占删除（P120）后，浏览器运行时它持有的缓存文件会被跳过，但用户不知道
是哪个应用占着，也无法在清理时顺手处理。原来列表每行有「退出应用后清理」按钮，
要逐项点击，且只覆盖部分缓存项；用户要求的产品行为是：用户自己的应用占用时，
清理前提示关闭；系统进程占用时自动跳过。

## Decision

后续审阅修复：枚举失败时 `FindRunningApps` 保留有所属应用的候选组，不能把
不可读解释成无人运行；CLI 默认保守跳过其缓存。关闭流程的初次或后续枚举失败
立即返回未确认退出的应用，不发送无依据的关闭请求、不再空等。

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
  结果）；正在运行的应用以默认勾选的复选框列出。
  - 勾选：GUI 清理前 `CloseRunningAppsForCleanup` 先请求正常退出（`WM_CLOSE`），
    超时后逐个复核创建时间、映像、用户和关键进程身份并强制结束，之后最多等 2 秒
    复查进程列表。没有确认退出的应用整项缓存跳过，继续清理其他目标时立即提示。
    取消后不再发送退出请求。
  - 未勾选：该应用的缓存项本次不进入清理目标，状态栏说明已跳过几项。
- **去掉每行的「退出应用后清理」按钮**。
- **CLI**：无法交互，默认把正在运行的所属应用的缓存排除在外；`--close-apps`
  才通过 `CloseRunningApps` 请求正常退出，不强制。报告 `running_apps` 列出应用、
  路径、是否请求退出、是否仍在运行。
- **Windows 已退出祖先。** `cacheProcessSnapshot` 曾逐 PID 查询祖先；Windows
  保留已退出创建者的 PID，查询该 PID 会返回 `process does not exist`，导致退出请求
  在发送前被拒绝。`cache_ancestry_windows.go` 使用完整 Toolhelp 快照：确认缺失的
  祖先表示已退出，快照错误、缺失目标、循环/过深、本程序及其后代仍拒绝。精确目标
  句柄校验保持在退出请求处，不能用祖先缺失代替目标身份校验。其他平台保持原判断。

## Superseded

由 `2026-11-01-junk-streamlined-clean-ux.md` 部分取代：
- **仍有效的核心机制**：`browsers.toml` 声明式进程校验、扫描期只读 `RunningAppOwners`、`cache_ancestry_windows.go` 完整 Toolhelp 祖先快照、GUI 超时强制退出身份双复核以及 CLI 默认跳过/`--close-apps` 仅正常退出均继续有效。
- **已演进的分支**：① 无运行占用应用时 GUI 直接启动清理，不再弹常规删除确认框；② `FindRunningApps` 在声明式浏览器之外叠加百毫秒级首锁抽样 `platform.FindLockedCacheOwners`，覆盖 Notion、Termius 等任意动态锁住缓存的桌面应用，关闭时若有 `Executables` 则按完整可执行路径精确匹配，正常退出宽限期由 8 秒调整为 4 秒；③ 占用询问使用三按钮 `ConfirmCloseApps`（含「跳过占用直接清理」与全选/取消全选），清理后的跳过账单面板已下线。

## Alternatives considered

- **用 Restart Manager 在清理前逐文件全量探测每个目标。** 4.2k 文件约 10.7 秒，
  一次清理几十个缓存目录不可接受；后续改为毫秒级排他首锁抽样 + 合并单次会话查询。
- **独立的第二个对话框。** 先确认清理再弹「关闭应用」，用户要连点两次；合并进
  单一占用询问弹窗，无占用则零弹窗。
- **只发 WM_CLOSE 后继续清理。** 最初采用，用户实际遇到勾选关闭仍未退出，现已按
  明确选择改为 GUI 超时强制结束，并在弹窗告知风险。CLI 未改变这一授权范围。
- **未勾选的应用也按文件清理它没打开的缓存。** 应用运行中删除其缓存索引之外的
  文件可能让它状态不一致；用户选择不关闭，就整体跳过并取消勾选。
- **给 `Layout` 新增 `processes` 字段。** 当时会让 Rust 参照树解析失败；`lists`
  在两边都是通用结构。

## Consequences

- 声明了进程的浏览器与动态持有缓存锁句柄的桌面应用都会合并出现在 `ConfirmCloseApps` 对话框中。
- macOS 的 `RunningAppOwners` / `FindLockedCacheOwners` 目前返回空。
- 退出请求复核同一用户、非关键进程、精确创建时间与可执行文件路径；GUI 的超时强制阶段也执行同样校验。

## Verification

- `internal/core/app_owner_test.go::TestOwnerEnumerationFailureKeepsCandidatesAndStopsWaiting`

Proved: 修复前候选组为空，断言 unobservable owner treated as absent 失败，见
`docs/agent-notes-evidence/2026-11-01-owner-enumeration-red.log`；恢复保守候选和
错误即返回后同 focused 通过，见
`docs/agent-notes-evidence/2026-11-01-owner-enumeration-green.log`。

- `internal/core/app_owner_test.go::TestBundledBrowserOwnersAreDeclared`
- `internal/core/app_owner_test.go::TestOwnerProcessListsMustMatchBrowserRows`
- `internal/core/app_owner_test.go::TestFindRunningAppsGroupsByOwnerAndSkipsIdleApps`
- `internal/core/app_owner_test.go::TestFindRunningAppsMergesDeclaredBrowsersAndLockedDesktopApps`
- `internal/core/app_owner_test.go::TestRunningOwnerBusyBadgesOnlyRunningOwners`
- `internal/core/app_owner_test.go::TestCloseRunningAppsAsksNormallyAndReportsStragglers`
- `internal/core/app_owner_test.go::TestCleanupClosesBackgroundOwnersAfterGracePeriod`
- `internal/core/app_owner_test.go::TestCancelledCleanupDoesNotCloseOwners`
- `internal/core/app_owner_test.go::TestCleanupDoesNotForceExitedOrUnobservableOwners`
- `internal/core/app_owner_windows_test.go::TestCleanupClosesWindowlessNativeProcess`
- `internal/platform/cache_release_windows_test.go::TestCacheOwnerAncestryAcceptsExitedParentsButRefusesCleanerTree`
- `internal/platform/occupancy_windows_test.go::TestFindLockedCacheOwnersAttributesSiblingAppAndExcludesCleanerTree`
- `internal/core/app_owner_test.go::TestWindowsBrowserTargetsCarryTheirOwner`
- `internal/platform/app_owners_windows_test.go::TestRunningAppOwnersListsOwnProcessByImageName`
- `internal/platform/app_owners_windows_test.go::TestRunningAppOwnersOmitsSystemProcesses`
- `internal/app/junk_owner_test.go::TestJunkCleanSkipsUncheckedRunningOwnerApps`
- `internal/app/junk_owner_test.go::TestJunkCleanSkipAndCleanButtonLeavesBusyCachesUnselected`
- `internal/app/junk_owner_test.go::TestJunkCleanQuitsCheckedOwnerAppsFirst`
- `internal/app/junk_owner_test.go::TestJunkCleanKeepsCachesWhenOwnerCannotBeStopped`
- `internal/app/junk_owner_test.go::TestJunkCleanWithoutRunningOwnersCleansWithoutDialog`
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

Pitfalls checked: P66（只对已选中的缓存目标抽样询问，结束仍走缓存释放的身份
复核，不绕过分类）、P119（运行中标签不改推荐与勾选）、P120（未识别所属应用的占用
仍由按文件独占删除兜底；已选择关闭却未确认退出的所属应用整项跳过）。P121 按用户
授权修订，新增 P122。

Proved: 2026-11-01 后续自然红跑，真实无窗口隔离进程在旧祖先查询下报
`process does not exist`，仍存活，见
`docs/agent-notes-evidence/2026-11-01-owner-close-native-red.log`；改为完整快照后同
测试确认进程退出码为 1，见 `docs/agent-notes-evidence/2026-11-01-owner-close-native-green.log`。
只撤 GUI 的强制阶段时，后台进程回归仍返回 Chrome，见
`docs/agent-notes-evidence/2026-11-01-owner-force-red.log`；恢复后同 focused 绿，见
`docs/agent-notes-evidence/2026-11-01-owner-force-green.log`。只撤未退出应用的缓存
排除时，真实临时缓存被删除，见 `docs/agent-notes-evidence/2026-11-01-owner-skip-red.log`；
恢复后同 focused 绿，见 `docs/agent-notes-evidence/2026-11-01-owner-skip-green.log`。

后续全量验证：Windows `go test -race ./...`、`go vet ./...`、`go build ./...` 与
资源检查通过；全量 race 输出见 `docs/agent-notes-evidence/2026-11-01-cleaner-race.log`。
