# Agent Note: 磁盘透镜批量删除跳过已消失的行，未知身份仍整批拒绝

Status: implemented

## Problem

磁盘透镜进入 `%LOCALAPPDATA%\Temp`，表头全选后删除，状态栏报「无法冻结目标身份；未删除。」，
一项也没删。

根因：`requestDiskDelete` 对每个选中路径调用 `CaptureArbitraryTarget`，任何一项拿不到身份就
整批返回。磁盘索引落后于磁盘，Temp 这类高频变动目录里，扫描之后被别的程序删掉的文件仍在索引
里；对它们 `Lstat` 失败，身份必然拿不到。实测 Temp 当前存在的 7378 项身份全部可以冻结，失败
只来自已消失的行。全选越快越容易撞上，所以批量全选提速后问题才明显。

另外，这一步在确认框弹出前的主线程回调里逐项 `Lstat` + 读取对象 ID，7000 多项约 6 秒，窗口卡住。

## Decision

- 新增 `captureDiskTargets`（worker 中运行）：逐项冻结身份。拿不到身份时再 `Lstat`：
  - 确认 `ErrNotExist` 的路径放进 `gone`：没有东西可删，不授权删除，只从索引移除；
  - 存在却拿不到身份的路径放进 `failed`：整批拒绝，提示具体路径。未知身份绝不按「已消失」跳过。
- `requestSelectedDiskDelete` 在 worker 中完成「解析排除项 + 冻结身份」，主线程只做内存里的
  索引覆盖与保护检查。单行删除仍走同一函数（单项，主线程开销可忽略）。
- 全部选中项都已消失时，不弹删除确认，直接从列表移除并提示数量。
- 删除确认里的「约释放」只累加真正要删的目标在索引里的大小，不再用选中总量（含已消失项）。
- 删除完成后的 `pruneDiskPaths` 拆成两段：worker 里逐项 `os.Stat` 判定哪些真的没了，主线程
  `applyDiskPrune` 只在内存索引里摘节点。原先 7000 多项删除后要在主线程逐项 stat。

## Alternatives considered

- **跳过所有拿不到身份的行。** 会把「存在但身份未知」也当成可跳过，违反 P37「不要改成只在
  文件缺失时跳过」的反面：未知身份必须 fail closed。
- **删除前整页重扫刷新索引。** Temp 在重扫期间照样变化，且代价远大于逐项 `Lstat`。
- **身份缺失时退化为不带身份删除。** 等于去掉身份闸门（P25/P37 禁止）。

## Consequences

- Temp 全选删除只删仍在磁盘上的项，消失的项从列表移除，不再整批失败。
- 存在但身份无法冻结的项仍然阻止整批，并在提示中给出路径。
- 7000 多项的身份冻结、删除后的逐项存在性检查都不再卡主线程。
- 确认框的释放大小与实际删除目标一致。
- 剪枝在 worker 回报后才生效；调用方不能假设 `pruneDiskPaths` 返回时索引已更新。

## Verification

- `internal/app/root_test.go::TestSelectedDiskDeleteSkipsRowsAlreadyGoneFromDisk`
- `internal/app/root_test.go::TestCaptureDiskTargetsSeparatesGoneFromUnfreezable`
- `internal/app/root_test.go::TestSelectedDiskDeleteResolvesTargetsOnWorkerBeforeConfirming`
- `internal/app/disk_prune_test.go::TestDiskDeletePrunesTreeAndKeepsPosition`

Proved: TDD 先红。修复前 `TestSelectedDiskDeleteSkipsRowsAlreadyGoneFromDisk` 报
`a vanished row refused the batch: "无法冻结目标身份；未删除。"`，与用户截图一致，
见 `docs/agent-notes-evidence/2026-11-01-disk-delete-vanished-rows-red.log`；
修复后同 focused 绿跑见 `docs/agent-notes-evidence/2026-11-01-disk-delete-vanished-rows-green.log`。

Proved: 制造型红跑。把「存在但无法冻结 → 整批拒绝」临时改成跳过，
`TestCaptureDiskTargetsSeparatesGoneFromUnfreezable` 报
`existing unfreezable path was not refused`，
见 `docs/agent-notes-evidence/2026-11-01-disk-delete-unfreezable-red.log`；
恢复后同 focused 绿跑（同上 green log）。

Proved: 制造型红跑。确认文案临时改回 `r.disk.SelectedSize`，
`TestSelectedDiskDeleteSkipsRowsAlreadyGoneFromDisk` 报
`confirmation size counts vanished rows: 将永久删除 1 项，释放约 1.0 MB`（实际只删 4 KB），
见 `docs/agent-notes-evidence/2026-11-01-disk-delete-size-red.log`。
把剪枝临时改回主线程同步 stat，`TestDiskDeletePrunesTreeAndKeepsPosition` 报
`index pruned on the calling thread before the worker reported`，
见 `docs/agent-notes-evidence/2026-11-01-disk-prune-worker-red.log`。两处恢复后同 focused 绿跑（同上 green log）。

Pitfalls checked: P25/P37 身份闸门未放宽，删除目标仍逐项带冻结身份；P77/P128 文件系统访问不回主线程。
