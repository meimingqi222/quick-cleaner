# Agent Note: Windows 活跃缓存清理中目录 mtime 跳变、并发自删与映射映像不能误报失败

Status: implemented

## Problem

在 Windows 实机清理 `go-build`、`npm-cache` 与活跃应用日志（如 `Trae CN\logs`）时，顶部红色「未能清理」面板报出数百 MB 的 `未能删除` / `权限不足`，而这些目录并无 ACL 损坏：
1. **目录 `mtime` 自跳变触发 `FailChanged` / 拒绝深入**：NTFS 在删除目录内任一子文件或子目录时会立即更新父目录自身的 `LastWriteTime`。`TargetIdentity.Recheck` 在 Windows 上无差别要求 `mtime` 与扫描期一致，导致正在清理或后台刚增删过临时文件的缓存目录在复核时失败。
2. **并发自删文件与末尾非空目录竞态记为 `FailOther`**：活跃进程（编译器、包管理器、Electron 日志轮转）在清理枚举与删除间隙自行删掉临时文件或写入新日志，使 `cleanFile` / `enumerateCleanEntries` / `classifyUncheckedChild` 遇到 `os.ErrNotExist` 或长度增长，以及 `DeleteTree` 末尾 `RemoveDirectory` 返回 `ERROR_DIR_NOT_EMPTY`（errno 145），原逻辑将它们记为 `FailOther` 真实失败。
3. **内存映射 `.exe` / `.dll` 返回 `ERROR_ACCESS_DENIED`**：`go-build` 测试产物缓存或更新暂存目录中仍在运行的 `.exe` / `.dll` 被内核映像节（image section）映射时，`CreateFileW(DELETE, shareMode=0)` 可能成功，但随后的 `SetFileInformationByHandle(FileDispositionInfo)` 返回 `ERROR_ACCESS_DENIED`（errno 5）而非 `ERROR_SHARING_VIOLATION`（errno 32），原逻辑将其当成权限不足并白跑 `takeown`/`icacls`。

## Decision

- **Windows 目录稳定身份忽略 `mtime` 跳变。** `TargetIdentity` 记录 `isDir`；Windows 上当目标持有卷序列号 + FileID 强身份（`id.hasStable`）且当前仍为非链接目录（`now.isDir`）时，`Recheck` 只核验 `platform.ObjectID` 相等，不再因目录内子项变动更新目录 `mtime` 而拒删。普通文件与缺失强身份的目标保持 `mtime + length` 严格复核；Unix 保持 `dev + ino` 不变（P28）。
- **并发消失归为跳过，活跃追加与非空目录竞态归为 `FailInUse`。**
  - `cleanFile` 与 `DeleteTree` 遇到 `os.ErrNotExist` 返回 `CleanSkipped`（目标已不存在，不计失败也不虚增删除字节）。
  - `enumerateCleanEntries` 忽略枚举瞬间消失的子项，并对子目录使用带祖先缓存的 `captureIdentityUsing` 捕获完整 `ObjectID` 强身份，防止递归进入子目录后因孙项删除跳变 `mtime` 而失败。
  - `classifyUncheckedChild` 对消失子项返回跳过，对大小/时间正在变化的普通活跃文件归为 `FailInUse`；`DeleteTree` 末尾因并发新写入导致的 `ERROR_DIR_NOT_EMPTY` 归为 `FailInUse`。
- **内存映射映像锁归一化为 `ERROR_SHARING_VIOLATION`。** `platform.RemoveFileExclusive` 在 `deleteOpenedFile` 返回 `ERROR_ACCESS_DENIED` 且文件无只读位时，通过 `isMappedImageInUse` 探测（`FILE_WRITE_DATA` 打开报 `ERROR_SHARING_VIOLATION`，或非目录无只读位却拒绝写数据），命中时返回 `windows.ERROR_SHARING_VIOLATION`，使上层将其按 `FailInUse` 静默跳过且不触发无效的 ACL 恢复（P6/P43）。真实只读文件与 Deny Delete ACL 仍走原有的清只读与提权 ACL 拆解路径。

## Alternatives considered

- **把所有文件的 `mtime` 复核也一并去掉。** 普通文件若在扫描后被替换或写入用户新内容（如同名新文件且文件系统未提供强 ID 时），去掉 `mtime`/`size` 会削弱防误删边界；仅对已拿到 `ObjectID` 强身份的目录豁免 `mtime`，而活跃增长的普通文件在 `classifyUncheckedChild` 归为 `FailInUse` 保守保留不删。
- **把所有 `ERROR_ACCESS_DENIED` 都当成 `FailInUse`。** 会掩盖真正的权限不足，也会阻止提权模式下的 Deny Delete ACL 自动拆解（P6）；必须先排除只读位并通过写数据探针区分映像映射锁与真实 ACL 拒绝。

## Consequences

- `go-build`、`npm-cache` 及活跃应用日志目录在后台有并发读写或运行中测试二进制时，可删文件全部正常清理，正在使用的少量文件归入占用跳过，不再产生假红框告警，也不再对映射二进制空跑 `icacls`。
- 目录自身若在扫描后被删掉重建为另一个目录（`ObjectID` 变化）或被替换为符号链接/reparse 点，仍被 `Recheck` 严格拦截。

## Verification

- `internal/core/identity_windows_test.go::TestIdentityWindowsDirectoryChurnKeepsStableIdentity`
- `internal/core/cleaner_parallel_windows_test.go::TestCleanTargetsSurvivesDirectoryChurnAndVanishedFiles`
- `internal/platform/remove_windows_test.go::TestRemoveFileExclusiveClassifiesMappedImageAsSharingViolation`

Proved: 修复前先写上述三个测试自然红跑：`TestCleanTargetsSurvivesDirectoryChurnAndVanishedFiles` 报 `vanished file result=2 failed=1; want CleanSkipped with 0 failures`，`TestIdentityWindowsDirectoryChurnKeepsStableIdentity` 报 `directory churn invalidated stable Windows directory identity`，`TestRemoveFileExclusiveClassifiesMappedImageAsSharingViolation` 对运行中的子进程映像报 `Access is denied.; want sharing violation (errno 32)`，完整输出见 `docs/agent-notes-evidence/2026-11-01-active-cache-churn-red.log`；完成身份、并发竞态与映射映像判定修复后同组 focused 测试全部通过，见 `docs/agent-notes-evidence/2026-11-01-active-cache-churn-green.log`。
