# Agent Note: Dev-asset size measurement is parallel and must stay hardlink-aware

Status: implemented

## Problem

开发环境页一次扫描要十几秒到半分钟，而扫描里唯一走遍每个文件的一步是体积测算。
真机计时（同一台机器、同一份条目）：

```text
phases: conda 138ms, python 1.46s, node 2.02s, tools 184ms
measure: 24.1s for 13 dirs, 43438 files, logical 2.45 GB
```

90% 的时间在 `measure_batch_storage`。拆开看更清楚：

- 遍历本身（`read_dir` + `symlink_metadata`）就要 **4.43 s / 13736 文件**；
- 取硬链接数（每个文件一次 `CreateFileW` + `GetFileInformationByHandle`）
  再要 **3.52 s**，合起来约 **300µs/文件**。

也就是说瓶颈不是某个调用写错了，而是**每个文件至少两次元数据级系统调用**，
这台机器上每次约 300µs（有杀毒/过滤驱动介入的机器上很常见）。顺序执行时
4 万个文件就是二十多秒。

顺带证伪了一个看起来很美、但行不通的方案：想用 `FindFirstFileExW` 一次枚举
整个目录跳过逐文件句柄——但 `WIN32_FIND_DATA` **没有链接数字段**（`nNumberOfLinks`
在 `FILE_STANDARD_INFORMATION` 里，要句柄才拿得到）。所以硬链接感知的测量在
Windows 上必然要为每个文件开一次句柄，省不掉，只能并行。

## Decision

测量全程并行，两段都并行：

1. **遍历**按目录递归并行（`collect_files` 里每个目录的直接子目录交给 rayon）。
   只并行后半段等于只优化了一半——遍历和句柄是同一个数量级。
2. **取链接数/身份**按文件并行（`scan_dir` 里 `par_iter`）。
3. **跨目录去重仍顺序做**：硬链接的唯一身份只能算一次的判断没法并行，所以
   扫描结果先按目录收集成 `(长度, 链接数, 身份)`，并行段结束后再按顺序去重。
   身份条目的是 `links > 1` 的文件——单链接的文件不可能与另一个目录共享，
   但这个短路只是注释里的推论，代码仍把身份带上，语义与并行前完全一致。

`per_dir` 的顺序必须与入参一致（调用方按位置把体积贴回条目上），rayon 的
`collect::<Vec<_>>` 保序，另有 `per_dir_sizes_stay_aligned_with_the_input_order`
锁住这一点。

Python 解释器不再量整个安装目录，只量它的包目录：安装目录（`Lib` / `DLLs` /
`tcl` / `include`）既不可能被这个页面释放，又是最大的单棵树之一。

## Alternatives considered

用 `FindFirstFileExW` 一次枚举一个目录、彻底去掉逐文件句柄——不可行：
`WIN32_FIND_DATA` 不带链接数（见上），要做硬链接感知就绕不开句柄。

只报逻辑体积、不算独占可释放——拒绝：那会系统性高估「能释放多少」，
而 conda 的 `pkgs` 缓存与环境之间、pnpm / uv 的 store 与安装之间都是硬链接，
高估的正是用户最关心那个数字。

把「独占」改成依赖目录 mtime 之类的启发式——拒绝：mtime 只反映直接子项的
增删，判断不了共享；猜错的方向是**高估**，也就是鼓励用户白删。

给测量结果做磁盘缓存——暂缓：目录 mtime 在深层内容变化时不会变，用它当失效
键会端出陈旧数字；要做就得有更强的键，那是另一件事。

## Consequences

同一台机器、同一份条目（后来又加了用户级包目录，文件数从 43438 涨到 57449）：

```text
measure: 4.94s for 14 dirs, 57449 files, logical 3.07 GB
```

按文件算从 553µs 降到 86µs（约 6.4×），按同一份条目算从 24.1s 降到 3.2s。
扫描总时长（四段命令 + 测量）从约 28s 降到约 7s。

同时把 `measure_dir_storage` 删掉了：它和 `measure_batch_storage` 是两份遍历器，
而单目录调用可以用 `measure_batch_storage(std::slice::from_ref(&dir))` 表达
——「同一份判断不许存两份」在这套测量逻辑里同样适用。

代价：并发读元数据会让磁盘更忙（这些目录都在本地盘上，实测没有负收益）。
以后若要把这里改成单线程、或给每个文件再加一次元数据调用，先看这份计时。

## Verification

- `src/core/dev_env/storage.rs::per_dir_sizes_stay_aligned_with_the_input_order`
- `src/core/dev_env/storage.rs::test_measure_real_dir_and_hardlink`
- `src/core/dev_env/storage.rs::test_measure_empty_or_nonexistent`
- `src/core/dev_env/discovery.rs::live_probe_lists_the_assets_on_this_machine`

架构记录，不主张 bug-fix 红跑：本次改动不改变任何一项的体积语义
（`test_measure_real_dir_and_hardlink` 里的硬链接用例逐项与总计都不变），
证据是同一台机器前后两次计时，见
`docs/agent-notes-evidence/2026-10-07-devenv-scan-performance-green.log`。
Windows 上实际执行；macOS 侧未在本机跑，两个平台都走同一套 `par_iter`。
