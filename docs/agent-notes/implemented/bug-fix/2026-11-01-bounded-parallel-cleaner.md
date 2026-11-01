# Agent Note: Windows 小文件清理共用有界并发，硬链接排除自身占用

Status: implemented

## Problem

用户清理 17 万个文件时进度缓慢。永久删除逐目录、逐文件串行；每个孩子重复核验同一个
父目录（Windows 含 ObjectID 打开），枚举后还逐文件重复 Lstat。直接并行独占删除又会
让硬链接或同时进行的占用探测互相制造 sharing violation。

## Decision

`internal/core/cleaner_files.go` 为一轮 `CleanProgress` 共享 7 个额外任务槽，含调用线程
最多 8 路；递归无槽就内联，返回前等所有孩子结束才删父目录。SQLite 家族仍是不可拆分
的任务，先伴随文件后主文件，活库与伴随失败继续保护主文件。macOS 保持串行。

枚举普通文件复用 DirEntry.Info；目录仍 Lstat，因为 Windows 枚举的目录长度与 Lstat
可能不同。删除前统一核验父身份一次，再分别核验所有孩子，所有检查先于本目录删除。
中央 safety、只读/AccessDenied 局部修复、取消、独占删除和文件字节计数均保留。

`internal/platform/remove_windows.go` 的独占删除与原始占用探测共用 `inUseProbeMu`：
普通路径读锁并发，共享冲突释放读锁后持写锁再试，排除本进程全部探测/删除句柄。
外部以 FILE_SHARE_DELETE 打开的文件仍拒绝；不能放宽共享模式或回退 os.Remove。

## Alternatives considered

- 整树 RemoveAll 或放宽共享模式：丢失逐叶占用、路径保护及活库边界。
- 每层独立开 8 个 worker：深树会无界膨胀；递归等待槽又会死锁。
- SQLite 主文件与 WAL 分别派发：伴随失败时可能已经删了主文件。
- 共享冲突直接记被占用：自身硬链接操作会产生假锁；只在本目录内重试也不够。

## Consequences

优化范围是 Windows 永久递归删除，不改变 OpTree 的整体占用闸门或回收站语义。
基准 `internal/core/cleaner_perf_test.go` 分别在 1 和 32 个子目录中生成 2048 个小文件，
只计删除时间，返回后核对文件数和目标确实消失。三轮中位数分别为 4.362 秒 →
0.775 秒、3.786 秒 → 0.280 秒。修改前后原始记录见
`docs/agent-notes-evidence/2026-11-01-cleaner-performance-before.log` 与
`docs/agent-notes-evidence/2026-11-01-cleaner-performance-after.log`；这是隔离临时目录
基准，不能直接换算用户 8.8 GB 整轮清理或实际 SSD 释放速率。

Pitfalls checked: P5/P6 只读目录和局部 ACL 重试保留，P15/P33 活库家族保留，
P19/P25/P37 目标身份保持，P43 不递归重写 ACL，P117 排除自身硬链接句柄，
P118/P120 叶子跳过和父失败不重复计数。未触及卸载命令解析、官方卸载或残留名字匹配。

## Verification

- `internal/core/cleaner_parallel_windows_test.go::TestCleanerParallelBudgetCoversNestedWorkAndDrains`
- `internal/core/cleaner_parallel_windows_test.go::TestParallelCleanerDeletesHardlinkNamesWithoutFalseOccupancy`
- `internal/platform/remove_windows_test.go::TestRemoveFileExclusiveWaitsForOwnHardlinkProbe`
- `internal/platform/remove_windows_test.go::TestRemoveFileExclusiveRefusesHeldFilesEvenWithShareDelete`
- `internal/core/cleaner_test.go::TestCleanerRecursiveWhitelistAndCancellation`
- `internal/core/cleaner_test.go::TestCleanerRetainsNestedLiveSQLiteFamily`
- `internal/core/cleaner_leaf_windows_test.go::TestContentsCleanupSkipsOnlyLockedLeaves`

Proved: 只将并行入口恢复串行，嵌套预算测试在 peak=1 断言失败，见
`docs/agent-notes-evidence/2026-11-01-cleaner-parallel-red.log`；恢复后同 focused
通过，见 `docs/agent-notes-evidence/2026-11-01-cleaner-parallel-green.log`。只撤
共享冲突的全局写锁复核，自身硬链接探测被报外部占用，见
`docs/agent-notes-evidence/2026-11-01-cleaner-hardlink-red.log`；恢复后同 focused
通过，见 `docs/agent-notes-evidence/2026-11-01-cleaner-hardlink-green.log`。

Final checks: Windows `go test -race ./...`、`go vet ./...`、`go build ./...`、
Windows 资源检查、格式及 note/测试锚点和 PITFALLS 指针检查通过。全量 race 输出见
`docs/agent-notes-evidence/2026-11-01-cleaner-race.log`。另以 race 连跑三次 128 个
跨目录硬链接的真实删除夹具，均删除全部缓存路径并保留树外原始链接。
macOS arm64 的 core/platform 交叉构建通过，未声称 macOS 原生运行验收。
