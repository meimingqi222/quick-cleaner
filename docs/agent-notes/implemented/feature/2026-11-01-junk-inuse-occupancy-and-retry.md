# Agent Note: 占用不影响勾选，Windows 缓存按文件独占删除，残留按占用性质分流

Status: implemented

## Problem

用户在 Windows 上完成一次智能清理后，看到「上次清理有 49 个目标仍有残留」：
- 「被占用」行只有大小、文件数和路径，不知道是哪个应用持有。Windows 扫描期
  的 `DetectInUse` 是空实现，占用只在删除失败后才被记录；真正能报出进程名的
  Restart Manager 只接在「退出应用后清理」这一条缓存释放链路上。
- 「占用状态未知」行大多是目录超过旧探测预算（4096 项 / 3 秒）后被 fail-closed
  拒绝的目标（如 `go-build` 77,829 文件），并不是锁。
- 系统服务/关键进程占用本来就不会被结束（缓存释放探测会拒绝），但界面把它和
  真正的权限失败混在同一块红色「残留」里，看起来像报错。
- 缓存项在主列表里有「退出应用后清理」，清理失败后进入残留面板却没有任何重试
  入口，成了死胡同。

## Decision

产品原则（2026-11-01 与用户确认）：默认勾选 = 规则推荐，与占用无关；全选能选中
全部未被规则阻止的项目，且不出现明显报错；系统进程占用自动跳过；用户进程占用
由清理前的「关闭应用」对话框处理（阶段 3，另行提交）。本 note 的早期版本曾在
扫描阶段按占用撤销推荐和勾选，已按此原则推翻，见下方 Alternatives。

- **占用只是 badge。** `ApplyBusy` 只挂 `Busy`，不改 `Recommended`，删除
  `RecommendedBeforeBusy`；`ScanItem.CleanupAvailable` 只看规则阻止。推荐、全选、
  反选、类别勾选、`CleanableSize` 和 CLI `--all` / `--path` 都不看占用。
- **扫描不逐文件探测。** Windows `DetectInUse` 恢复为空实现；`internal/app/junk.go`
  恢复异步挂 badge（`armJunkBusy` / `applyReadyJunkBusy`），不等占用结果再发布，
  发现项也不再探测。
- **执行期按文件判断（Windows）。** `platform.RemoveFileExclusive` 以共享模式 0、
  DELETE 权限打开文件，再设置删除标记。打开本身就是占用检查：没有「先探测再删」
  的时间窗口，也不会删掉别人以 `FILE_SHARE_DELETE` 打开的文件（`os.Remove` 会）。
  `cleanFile` 统一走它。`CleanTargets` 用 `leafOccupancyGate` 对 Windows 上永久
  处置的 `OpContents` / `OpFile` 跳过整体探测，一个文件被锁只跳过这个文件；
  探测预算也不再限制这些目标。
- **`OpTree` 保持整体判断。** `node_modules`、`target/` 这类整树目标在运行的项目
  下半删会坏掉，所以仍然有一个文件被占用就整体跳过。macOS 没有共享模式，本轮
  所有目标仍按目标整体判断。
- **父目录按子项分类。** `DeleteTree` 在有子项未删时不再尝试删除目录，也不记
  `FailOther`，改用 `CleanProgress.failParent`：子项全是占用/未确认时父目录记为
  跳过，任一真实错误则记为失败；父目录不再累加 Failed/Ignored。SQLite 主文件因
  伴随文件未删而保留时，同样继承伴随文件的分类。
- `rules/windows.toml` 复用 files selection，只选择 Explorer 下的
  `thumbcache_*.db` / `iconcache_*.db` 文件，不再清空 Explorer 目录。
- 执行后才发现占用的目标由 `applyJunkCleanResult` 挂上 Busy badge，不改推荐和
  勾选；权限等真实错误不改成占用。
- CLI 执行时，全部目标都因占用跳过、未删除任何文件也没有失败的运行不写历史。
- 新增只读的 `platform.OccupancyOwners`（Windows 用 Restart Manager）。它只在
  `FailInUse` 目标上运行，返回进程名/PID/是否系统进程，**不参与删除或结束授权**。
  归属证据失败（超过预算、不可读、Restart Manager 报错）只表示「未识别」，绝不
  把占用改成空闲。
- `finishCleanup` 把 `FailInUse` / `FailUnverified` 归入「已跳过」列表，使用中性
  样式；`FailAccessDenied` / `FailChanged` / `FailOther` 仍进入红色「未能清理」。
  系统、其他用户或受保护进程占用的行不提供结束入口。
- 仍在 `CacheReleaseEligible` 范围内的应用占用行提供「退出应用后重试」，直接复用
  `requestCacheRelease`：重新探测占用者、重新验证扫描期身份与计划、先请求正常退出，
  强制结束仍需二次确认。没有为普通残留新增结束进程通道（P66）。
- 缓存释放探测返回系统进程/其他用户/关键进程占用时，不再把
  `service-or-critical-owner` 这类内部 token 原样写进状态栏，而是显示
  「系统、其他用户或受保护进程占用，无法结束；已跳过」。
- Windows 目录占用探测改为 1024 文件一批、8 路并行排他打开，预算从 4096/3 秒
  提到 16384/5 秒。实测 4.1k 文件约 0.8–0.9 秒；Restart Manager 单独探测 4.2k
  文件约 10.7 秒，不能替代。超过预算、不可读文件、链接和特殊文件仍按已知=false
  处理；找到锁时以锁为准。
- 并行排他打开会让同一个文件的硬链接互相产生共享冲突。所有原始文件探测共用
  读写锁：首次探测持读锁并行，遇到共享冲突后释放读锁、持写锁复核，排除本进程
  其他批次/目录的探测句柄；复核仍冲突才作为占用证据。未知错误仍是未知，真实
  应用的共享句柄仍会阻止清理。
- 「已跳过」必须贯穿到用户可见的计数，而不只是残留面板：
  `CleanProgress` 新增 `Ignored`，`FailInUse` / `FailUnverified` 仍保留 failed
  明细归因，但进度条显示 `失败 = Failed - Ignored`，并在 `Ignored > 0` 时单独
  显示「已跳过 N」。状态栏混合场景拆成「X 个目标未能清理 / Y 个目标正在使用或
  无法确认，已跳过」；历史记录 failed 扣除跳过叶子、skipped 加上跳过叶子。
- `TargetFailInfo.SkippedLeaves` 单独统计全部失败叶子里的 InUse/Unverified 子集，
  历史只扣这个计数。目标内有任何真实错误时，`Reason` 选择真实错误中最多的一类；
  仅当所有失败叶子都是跳过原因时才使用中性样式，不能按总体多数原因隐藏少数错误。
- 残留明细上限从 20 提到 40，超过时显示「另有 N 项未显示」，避免标题计数与
  可读行数脱节。归属归属探测的 4 秒总预算用 `select` 截止，预算耗尽后不再启动
  新探测；Restart Manager 仍在后台 worker 上运行。

## Alternatives considered

- **直接把超预算目录当成空闲并继续删除。** 会删掉仍被占用的目录中的一部分，
  违反既有 fail-closed 规则（P15/P66）；没有采用。
- **把 Restart Manager 作为目录占用的唯一判据。** 实测明显更慢，且仍有预算上限；
  只作为只读归属证据。
- **让残留面板直接强杀占用进程。** 会绕过缓存释放已有的身份复核、系统进程拒绝和
  二次确认；只复用现有释放通道。
- **把所有占用都标成红色失败。** 正是用户反馈的体感问题；改为「已跳过」并保留
  真正失败项的告警。
- **只等待当前批次结束后串行复核共享冲突。** 其他并发目录的探测也可能打开同一
  文件，仍会产生自占用误报；复核必须排除本进程全部原始探测，不按目录单独加锁。
- **按目标多数原因把全部失败叶子换算为跳过。** 两个占用文件和一个权限错误会被
  记录为失败 0、跳过 3；保留叶子子集计数与真实错误优先的目标摘要。
- **扫描阶段按占用撤销推荐、禁用勾选、剔除可清理量。** 本 note 早期版本这样做，
  后被推翻：全选不再是全部；结果要等逐文件探测完才出现；超过预算的大目录几乎
  全变成「无法确认安全」而不可勾选；用户进程占用也无从提示关闭。根本原因在执行
  期按目标整体放弃，扫描阶段只是提前展示了这个缺陷。
- **执行期对所有目标（含 `OpTree`）都按文件跳过。** 运行中的项目会留下半删的
  依赖树，比完整保留更糟；只对缓存内容与单文件这样做。
- **先探测、再用 `os.Remove` 删除。** 有时间窗口，且会删掉以 `FILE_SHARE_DELETE`
  打开的文件；用共享模式 0 的删除句柄一次完成。
- **排除整个 Explorer 缓存目录。** 有些具体缓存文件可清理；现有规则 DSL 能精确
  列出数据库文件，不需要新增 provider 或扩大到目录中的其他内容。

## Consequences

- 小缓存被占用时显示「Edge 正在使用」之类的归属，并可直接退出应用后重试。
- 系统服务、其他用户或受保护进程占用显示「系统、其他用户或受保护进程占用，已跳过」，
  不再像错误。
- Windows 上缓存目录里被占用的文件单独跳过，其余照常删除；探测预算不再限制
  缓存内容与单文件目标。超过预算的 `OpTree` 目标（大型 `node_modules`）仍整体
  跳过，是已知限制。
- macOS 本轮仍按目标整体判断占用。
- 扫描不再等待占用探测；占用 badge 异步出现，不影响推荐、勾选和可清理量。
- 显示的可清理量是预计值，实际释放量以清理结果为准。
- 清理完成的状态栏在只有跳过项时显示「正在使用或无法确认，已跳过」，不再说
  「部分清理完成」；混合场景分开报告失败目标与跳过目标，进度条同步显示「已跳过」
  而不是把占用计成失败。

## Verification

- `internal/platform/occupancy_windows_test.go::TestWindowsDirectoryProbeCoversLargeUnlockedTrees`
- `internal/platform/occupancy_windows_test.go::TestOccupancyOwnersReportsTheLockingProcess`
- `internal/app/junk_test.go::TestJunkDetailsSeparateSkippedInUseFromRealFailures`
- `internal/app/junk_test.go::TestJunkOnlyInUseTargetsReportSkippedNotFailed`
- `internal/app/cache_release_test.go::TestCacheReleaseFailureTreatsSystemOwnersAsSkipped`
- `internal/ui/junk_test.go::TestJunkPageExclusionsFailuresAndLocalization`
- `internal/core/cleaner_test.go::TestCleanProgressReportsInUseAndUnverifiedAsSkipped`
- `internal/ui/dashboard_test.go::TestDashboardCleanProgressSeparatesSkippedFromFailed`
- `internal/ui/junk_test.go::TestJunkSkippedDetailsCapRowsAndReportRemainder`
- `internal/platform/occupancy_windows_test.go::TestWindowsProbeDoesNotMistakeHardlinksForExternalLocks`
- `internal/core/cleaner_targets_test.go::TestTargetFailInfoPreservesMixedFailures`
- `internal/app/junk_test.go::TestJunkMixedLeafFailuresKeepErrorAndExactHistory`
- `internal/platform/occupancy_windows_test.go::TestWindowsScanDoesNotProbeTargetFiles`
- `internal/core/inuse_test.go::TestApplyBusyDoesNotDisableNativeResources`
- `internal/core/inuse_test.go::TestApplyBusyKeepsRecommendationAndSelection`
- `internal/core/rule_directories_test.go::TestWindowsThumbnailRulesSelectOnlyCacheDatabaseFiles`
- `internal/app/junk_test.go::TestJunkBusyItemsStayRecommendedAndSelectable`
- `internal/app/junk_test.go::TestJunkLateOccupancyKeepsBadgeOnlyAndRealErrors`
- `internal/platform/remove_windows_test.go::TestRemoveFileExclusiveRefusesHeldFilesEvenWithShareDelete`
- `internal/core/cleaner_leaf_windows_test.go::TestContentsCleanupSkipsOnlyLockedLeaves`
- `internal/core/cleaner_leaf_windows_test.go::TestFailParentPrefersRealErrorsOverSkips`
- `internal/core/cleaner_leaf_windows_test.go::TestTreeTargetsKeepWholeTargetOccupancyGate`
- `internal/cli/cli_test.go::TestJunkSelectionRequiresFreshExactEligibleTarget`
- `internal/core/inuse_test.go::TestSplitFailuresKeepsRealErrorsInMixedTargets`
- `internal/ui/junk_test.go::TestJunkPageSelectionAndIndependentActions`

Proved:
1. 旧 4096 上限红跑：把 `inUseDirMaxEntries` 临时改回 `1 << 12`，
   `TestWindowsDirectoryProbeCoversLargeUnlockedTrees` 报
   `unlocked 4100-entry tree = busy false, known false; want false, true`，证据见
   `docs/agent-notes-evidence/2026-11-01-junk-inuse-red.log`；恢复 `1 << 14` 后
   focused 测试绿跑（clear 0.72s、busy 0.83s），见
   `docs/agent-notes-evidence/2026-11-01-junk-inuse-green.log`。
2. `TestOccupancyOwnersReportsTheLockingProcess` 用真实子进程持有排他句柄，
   断言 Restart Manager 归属返回该 PID 且未误标为系统进程。
3. 分类与重试路径：`TestJunkDetailsSeparateSkippedInUseFromRealFailures` 断言
   应用占用/系统占用/未确认进入已跳过、权限失败仍进入失败；UI 测试点击
   `Quit app and retry` 并断言回调收到原路径。
4. 进度条分流红跑：注释掉 `p.Ignored.Add(1)` 后
   `TestCleanProgressReportsInUseAndUnverifiedAsSkipped` 报
   `failed=3 ignored=0; want failed=3 ignored=2`；恢复后同 focused 绿跑。再把
   dashboard 的 `if snap.Ignored > 0` 临时禁用，
   `TestDashboardCleanProgressSeparatesSkippedFromFailed` 报
   `in-use failures still reported as failed`；恢复后同 focused 绿跑。证据见
   `docs/agent-notes-evidence/2026-11-01-junk-skip-red.log` 与
   `docs/agent-notes-evidence/2026-11-01-junk-skip-green.log`。
5. 审阅发现的两个问题自然红跑：未改探测行为时，256 个硬链接的 4 批并发探测
   报出 65–181 个假锁；目标摘要把两个 InUse 与一个 AccessDenied 汇成 InUse；
   混合目标历史断言得到 failed=3 skipped=0，而期望 1/2。三个正式回归的输出见
   `docs/agent-notes-evidence/2026-11-01-review-regressions-red.log`。修复后同一组
   focused 全绿，见 `docs/agent-notes-evidence/2026-11-01-review-regressions-green.log`；
   硬链接测试同时确认外部共享句柄仍报占用、关闭后恢复空闲，摘要测试覆盖失败
   次序颠倒与 Changed/Other，不靠多数原因隐藏真实错误。
6. （已被第 8 条推翻，仅作历史）扫描前置智能跳过的红绿证据见
   `docs/agent-notes-evidence/2026-11-01-smart-skip-red.log` /
   `docs/agent-notes-evidence/2026-11-01-smart-skip-green.log`，以及
   `docs/agent-notes-evidence/2026-11-01-smart-skip-late-red.log` /
   `docs/agent-notes-evidence/2026-11-01-smart-skip-late-green.log`。这些断言
   锁的是已撤销的「占用即不可选」行为，对应测试已改写。
8. 占用与选择脱钩、按文件独占删除的制造型红跑，证据见
   `docs/agent-notes-evidence/2026-11-01-leaf-occupancy-red.log`：
   - A 轮同时撤三处防护：`leafOccupancyGate` 强制 false 时，
     `TestContentsCleanupSkipsOnlyLockedLeaves` 报三个未锁文件被保留、
     `files=0`；`RemoveFileExclusive` 改用共享读/写/删除打开时，
     `TestRemoveFileExclusiveRefusesHeldFilesEvenWithShareDelete` 报
     `err=<nil>; want sharing violation`；`ApplyBusy` 重新清除 `Recommended` 时，
     `TestApplyBusyKeepsRecommendationAndSelection` 报 `Recommended:false`。
   - B 轮：`failParent` 一律记 `FailOther` 时，目标摘要变成
     `Reason:other SkippedLeaves:0`，`TestFailParentPrefersRealErrorsOverSkips`
     把纯占用父目录归为失败。
   恢复后同一组 focused 全绿，见
   `docs/agent-notes-evidence/2026-11-01-leaf-occupancy-green.log`。

Pitfalls checked: P15（活库闸门保留在 `cleanFileFamilies` / `CleanPath`）、P66
（未新增结束进程入口；`OpTree` 仍整体判断，不靠结束进程绕过分类）、P84/P85/P88
不变；P117 注明探测已退出扫描期；P118 叶子计数保持；P119 改写为「占用只是提示」；
新增 P120 按文件独占删除。
