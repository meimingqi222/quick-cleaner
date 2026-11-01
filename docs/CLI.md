# CLI

Go 产品程序提供真实的智能清理、磁盘分析和文件搜索。开发时可将下面的
`quick-cleaner` 换成 `go run ./cmd/quick-cleaner`。脚本需要读取精确退出码时，
先 `go build -o quick-cleaner.exe ./cmd/quick-cleaner`，再运行可执行文件；
`go run` 会把程序的非零退出码包装成自身的失败。

```powershell
quick-cleaner cli --help
quick-cleaner cli junk categories --json
quick-cleaner cli junk scan --json
quick-cleaner cli junk plan --root 'D:\code\demo' --json
quick-cleaner cli junk clean --root 'D:\code\demo' --path 'D:\code\demo\node_modules' --json
```

`scan` 返回扫描结果及选择状态；`plan` 额外返回规则观察、各计划的目标范围、
完成条件和执行步骤。`clean` 默认也是预览。三者每次都重新扫描。

实际清理必须使用 `clean --execute`，并显式指定一种选择方式：

```powershell
quick-cleaner cli junk clean --root 'D:\code\demo' --path 'D:\code\demo\node_modules' --execute --json
quick-cleaner cli junk clean --recommended --execute --json
```

`--path` 可重复，必须精确命中本次扫描发现的目标，不能指定任意文件或目标中的子路径；
显式路径命中被规则阻止的目标时返回错误，不会当作已选中。
`--recommended` 选推荐且未被规则阻止的目标；`--all` 选全部未被规则阻止的目标，
包括非推荐项。三种选择方式互斥。未指定时，预览使用推荐项。

占用只作为报告里的状态提示，不改变推荐和选择。只有规则阻止的项目会被跳过、
计入 `skipped_count`。执行时再判断占用：Windows 上缓存目录（`contents`）和单个
文件逐个以独占方式删除，被占用的文件跳过，其余照常清理；整棵目录树（如
`node_modules`）只要有文件被占用就整体跳过，避免留下半删的依赖。macOS 仍按目标
整体判断。这些跳过计入 `skipped_items`，不算失败，退出码仍为 0。

浏览器（Chrome、Edge、Brave）正在运行时，CLI 不会询问：默认把它们的缓存排除在
本次清理之外；加 `--close-apps` 才会先请求这些应用正常退出（最多等 8 秒，不强制
结束），没退出的应用仍持有的文件照常按文件跳过。报告的 `running_apps` 列出应用、
涉及路径、是否请求了退出以及是否仍在运行。系统或其他用户的进程永远不会被请求退出。

智能清理的其他参数：

| 参数 | 含义 |
| --- | --- |
| `--root DIR` | 只保留此目录内的固定文件目标，并在此目录下发现构建产物；不扫描 Docker、快照等原生资源 |
| `--category ID` | 限制选择的类别，可重复；ID 见 `junk categories`；报告仍列出扫描项及其选择状态 |
| `--exclude PATH` | 在持久排除项之外临时增加排除项，可重复；不改设置 |
| `--close-apps` | 执行时先请求正在运行的浏览器正常退出再清理其缓存；不加则跳过这些缓存 |
| `--discover=false` | 关闭构建产物发现 |
| `--discovery-budget 30s` | 构建发现的单独时间预算；到期报告 `partial`，执行模式拒绝开始清理 |

`--root` 不改变可信用户身份，也不授权删除整个根目录。构建发现仍遵守产品规则、
项目证据、忽略目录和深度策略（显式根的最大发现深度为 12）。未指定根时，使用
当前用户的固定目标、原生资源 provider 和代码目录发现规划。当前 CLI 使用实时
发现，不复用 GUI 的常驻索引缓存，因此耗时可能不同。

清理沿用 core 的中央路径保护、排除项、冻结计划、身份复核、占用检查和完成判据；
无法读取或解析持久设置时拒绝扫描，避免丢失排除项。JSON 计划只用于诊断，
不提供导入计划、任意路径删除或自动结束进程的命令。有选中目标的执行会写入产品
清理历史；全部提前跳过的空操作不产生历史。

磁盘分析和文件搜索是只读命令：

```powershell
quick-cleaner cli disk analyze --root 'D:\code' --top 20 --json
quick-cleaner cli search --root 'D:\code' --query '*.go' --limit 100 --json
quick-cleaner cli disk analyze --volume 'D:\' --json
quick-cleaner cli search --volume 'D:\' --query report --json
```

`--root` 和 `--volume` 必须二选一。`--root` 实时遍历目录，跳过链接与特殊文件，
使用产品的 SizeTree 聚合和名称匹配；`--volume` 必须是挂载卷根，调用 Windows
MFT / macOS 原生整卷扫描。Windows 原生整卷扫描可能需要管理员权限；CLI 不会
自动弹 UAC。权限不足时报错，可以改用 `--root`。

搜索匹配文件和目录的**名称**，普通文本为子串，`*` / `?` 为通配符，ASCII 字母
不区分大小写；不支持正则、`|` 多模式或文件内容搜索。沿用产品先取有界匹配项、
再按大小排序的语义，结果不代表全树最大的 N 个匹配项。磁盘分析列出根的直属子项，
按大小降序排列。`--limit` 默认 100，`--top` 默认 20。

实时目录扫描的 `--max-entries` 默认 2,000,000，包含根。预算用尽或目录不可读时，
返回已采集结果、`blocked_paths` 和 `partial=true`。目录统计的字节数沿用平台
文件尺寸政策（Windows 为逻辑大小，POSIX 为分配大小），硬链接按路径计算；
这些数字不表示一定可回收的空间。`--volume` 沿用产品原生索引的统计政策。

所有耗时命令支持 `--timeout`（默认 `5m`）和 Ctrl+C。取消是协作式的，已开始的
原生系统调用或 provider 可能延后返回；清理取消也不会回滚已删除的文件。
CLI 不启动窗口、不激活已有 GUI，也不自动提权。Windows/macOS 支持执行清理；
Linux 支持目录分析、搜索和清理预览。扫描、预览、分析和搜索不写产品设置、历史
或持久索引；大型 macOS 扫描可使用可清理的临时 spill/index 文件。

`--json` 的 stdout 是单个 `schema_version: 1` 报告，诊断在 stderr。常用字段：

| 报告 | 字段 |
| --- | --- |
| 智能清理 | `dry_run`、`partial`、`cancelled`、`selected_count`、`selected_bytes`、`skipped_count`、`items`；执行后有 `cleanup`（成功、跳过、失败、残留和释放字节） |
| 分析与搜索 | `source`、`root`、`bytes`、`files`、`directories`、`partial`、`cancelled`、`truncated`、`blocked_paths`、`rows` |

`truncated=true` 表示显示数量达到 `--limit` / `--top`，与采集不完整的 `partial`
不同。取消或超时后，已进入采集阶段的命令尽量输出带标记的报告；参数、配置和
采集错误可能只有 stderr。脚本必须同时检查退出码。输出管道失败返回非零。

`skipped_count` 是所选类别/路径范围内扫描时不可清理的目标数。执行期额外跳过见
`cleanup.skipped` / `skipped_items` / `skipped_targets`；真正的失败见 `failed` /
`target_failures`，需手动处理的见 `manual`。`remaining` 保留未清理的实测内容，
其中可以包括正常跳过项，不能仅凭它非空判为失败。

| 退出码 | 含义 |
| --- | --- |
| `0` | 在所选范围和产品发现策略内完成；正常跳过占用/未知项或数量截断仍可为 0 |
| `1` | 配置、权限、采集、平台或输出错误 |
| `2` | 命令或参数错误，包括指定路径未命中新扫描目标 |
| `3` | 发现/目录覆盖不完整，或清理有真实失败、需手动处理项 |
| `124` | 超时 |
| `130` | 取消 |
