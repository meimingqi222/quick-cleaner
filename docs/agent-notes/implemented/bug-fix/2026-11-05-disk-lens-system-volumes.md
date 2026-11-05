# Agent Note: 磁盘透镜全盘大文件排除系统卷，walk 不跨进 APFS 辅助卷

Status: implemented

## Problem

磁盘透镜「全盘大文件」被删不掉的系统对象占满：`/System/Volumes/Preboot` 里数 GB 的引导文件、`/System/Volumes/VM/swapfile*`、`/private/var/vm/sleepimage`，以及挂在 `/Volumes/Recovery` 下、带「删除」按钮的 Recovery 文件。用户要找的是 `.m2/repository.zip` 这类可清理项。

根因两层。macOS 根卷 walk 只跳过 firmlink 镜像 `/System/Volumes/Data` 和 `/dev` 一类伪根，于是 Preboot/VM/Update 以及 `/Volumes` 下的其它挂载被收进 Macintosh HD 这棵树；Windows 已有 `hiddenNTFSMetadata` 把 pagefile/`$MFT` 从大文件榜拿掉，macOS 没有对等过滤，保护判断只关掉删除按钮，榜单仍被占满。`/Volumes/Recovery` 又不在 `macPrefixes` 里，行上会出现删除。

## Decision

- **Walk**：`diskWalkSkipDir` 把 `/System/Volumes` 整棵（含 Data 镜像和 Preboot/VM/Update/Recovery 等 APFS 角色）以及 `/Volumes` 挂载目录从根卷遍历里拿掉。增量刷新经 `DiskWalkIgnoredPath(path, mount)` 同样忽略；`mount == "/"` 时忽略全部 `/Volumes/*`，外接盘以 `/Volumes/Backup` 为根时仍索引自己的内容。Recovery/Preboot 等角色名在任何 mount 下都忽略。
- **全盘大文件**：`LargestFiles` 在 top-N 堆里跳过不可清理项。必须在堆内过滤：先取 200 再删保护项，系统文件占满前 200 时列表会被饿空。热路径只做文件名 NTFS meta 判断，以及 `largestFileHiddenDirs` 打上的前缀保护目录/白名单根的 parent 指针上溯——不能 `PathOf` 或 `IsProtected`（后者会 `Lstat` worktree）。exact 保护的目录（如 Desktop）不打标，避免把桌面上的大文件藏掉。目录树仍显示保护行，环图诚实性不变。
- **删除安全**：darwin 政策把 `/Volumes/{recovery,preboot,update,vm}` 视为保护路径，Recovery 不再出现删除按钮。`/private/var/vm` 本来就在 `macPrefixes` 里，继续索引、只从大文件榜隐藏。

## Alternatives considered

**从索引里拿掉整个 `/System` 和 `/private/var/vm`。** 密封系统卷和 sleepimage 是 Data/容器上的真占用；目录树和环图需要它们解释「空间去哪了」。不建的话会多出一块对不上 Statfs 的「其他已用」。

**对 `LargestFiles(200)` 的结果再滤保护项。** 截图里前 200 几乎全是系统文件，滤完用户项所剩无几。

**按 `/Volumes/` 前缀一律忽略。** 外接盘就挂在那里，根卷 walk 跳过 `/Volumes` 目录本身即可；刷新必须看当前 `mount`，否则 Backup 盘自己的增量会全部被丢掉。这和 P90 不用 `/Volumes/` 前缀判断「是否已安装」是同一类陷阱，场景相反：这里排除的是「别卷的内容混进本卷索引」。

**只打「系统保护项目」标签、仍留在大文件榜。** 原来就是这样，清理入口被不可选项占满。

**在堆里对每个候选调用 `IsProtected`。** 一上来就 `IsManagedAgentWorktree`（祖先 `Lstat`）。堆还没被大文件填满时，几乎每个节点都会走这条路径，点一次「全盘大文件」会卡死。改为前缀目录打标 + parent 上溯，文件名 NTFS meta 直接跳过。

## Consequences

- 新扫描的根卷树不再含 Preboot/VM/Recovery 挂载内容；旧缓存里已有的这些文件会留到下次全量重建，但全盘大文件榜立刻不再展示。
- `/System` 和 `/private/var/vm` 仍在目录树里，标保护、不可选。
- 外接盘、用户目录名恰好叫 `Volumes` 的路径（`/Users/…/Volumes`）不被跳过。
- 大文件榜不再跑 worktree `Lstat`；agent worktree 里的大文件可能出现在榜上，删除时仍受 `IsProtected` 拦截。

## Verification

- `internal/platform/disk_walk_test.go::TestDiskWalkMirrorPruningAndDatalessCoverage`
- `internal/platform/disk_walk_test.go::TestDiskWalkHelperVolumesAndMountsSkipped`
- `internal/core/disk_tree_test.go::TestLargestFilesSkipsProtectedWithoutStarvingTheList`
- `internal/core/disk_tree_test.go::TestLargestFilesHidesWhitelistSubtreeNotLocalSystemName`
- `internal/core/disk_tree_test.go::TestLargestFilesHidesMacPrefixSubtrees`
- `internal/core/safety_test.go::TestMacSafetyPolicy`
- `internal/app/root_test.go::TestRebuildDiskLargestFilesOmitsProtected`

Proved: 测试先于实现（TDD）。`TestDiskWalkMirrorPruningAndDatalessCoverage` 在 Data 已跳过、VM/Preboot/Update 仍入队时失败；`TestLargestFilesSkipsProtectedWithoutStarvingTheList` 得到 pagefile/hiberfil 而不是用户文件；`TestMacSafetyPolicy` 未保护 `/Volumes/Recovery`；`TestRebuildDiskLargestFilesOmitsProtected` 的全盘大文件行仍含 pagefile。输出见 `docs/agent-notes-evidence/2026-11-05-disk-lens-system-volumes-red.log`。实现后同 focused 绿，见 `docs/agent-notes-evidence/2026-11-05-disk-lens-system-volumes-green.log`。
