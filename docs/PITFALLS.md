# Pitfalls

反复踩过、根因不直观、以后很容易改回去的坑。

**提交前必须过一遍。** 见仓库根目录 [`AGENTS.md`](../AGENTS.md) 的「Commit 前必做」。新发现的同类回归补在这里，不要只写在 commit message 里。

每条都写：症状、根因、防护（代码 + 测试）、改相关代码时绝对不能做什么。

---

## P1 未加引号的 UninstallString 不能把目录当成 exe

Note: `2026-10-05-unquoted-uninstall-strings.md`

- **症状**：点卸载，官方卸载窗口弹不出来。日志类似：
  ```text
  开始卸载「PDF转换阅读器」，命令行: C:\Program Files (x86)\pdfcvt\uninstall.exe
  卸载失败：启动卸载程序失败（C:\Program Files）: 拒绝访问。 (os error 5)
  ```
  用户会以为是软件自带卸载器坏了。卸载器文件其实在，是我们没启动对。
- **根因**：Windows 注册表里的 `UninstallString` **经常不给路径加引号**。按空格从左往右拼接、用 `Path::exists()` 判断「命中」时，会先撞上真实存在的**目录**：
  - `C:\Program Files (x86)\pdfcvt\uninstall.exe`
  - 前缀 `C:\Program Files` 是文件夹，`exists() == true`
  - `CreateProcess("C:\Program Files")` → 拒绝访问，窗口永远不出现
- **防护**：
  - [`split_command`](../src/core/apps.rs) 的生产路径必须用 `Path::is_file()`，目录不算命中。
  - 注释里写的是「第一个拼出来确实存在的**文件**」，不要改回 `exists()`。
  - 测试：`directory_prefix_must_not_win_over_the_real_exe`、`unquoted_program_files_x86_is_not_a_directory`（`core::apps::command_tests`）。
- **改 `split_command` / 卸载启动 / 任何解析 UninstallString 的代码时**：
  - 禁止把 `is_file()` 改回 `exists()`。
  - 禁止按第一个空格切路径。
  - 禁止删掉或放宽上面两条测试。
  - 新增解析逻辑必须覆盖 `C:\Program Files (x86)\…\uninstall.exe` 这种中间有目录前缀的未加引号路径。

---

## P2 隐形占位块必须和真卡片有一样的内边距与边框

- **症状**：状态监控页 4×2 的卡片网格，**下排的卡片比上排明显宽**（真机
  实测上排四张各 235px、下排两张各 255px），两排边缘对不齐，像布局坏了。
- **根因**：末行不足四张时用「空 div + `flex_1` + `min_w`」补位。flex 的
  base size 会被**自身 padding + border 之和托底**（taffy 的
  `child.flex_basis = child.flex_basis.max(padding_border_sum)`，Chrome 和
  Firefox 行为一致）：
  - 真卡片 `p_5` + `border_1` → base size 被托到 42px
  - 空占位块 → base size 0
  - 剩余空间四等分，每人拿到同样的增量，于是真卡片比占位块**恒定宽 42px**
  - 满行（四张真卡）时人人都有这 42px，所以只有不满的那一行会出问题
- **防护**：
  - [`card_rows`](../src/ui/views/status.rs) 的占位块必须写成
    `div().flex_1().min_w(px(CARD_MIN_W)).p_5().border_1()`，和
    [`status_card`](../src/ui/views/status.rs) 的盒子度量逐项对齐。
  - 卡片外壳只能从 `status_card()` 来，不要在各张卡里重抄一份
    `flex_1 / min_w / p_5`——抄一份就多一个和占位块跑偏的机会。
- **改卡片网格时**：
  - 禁止把占位块「简化」回没有内边距的空 `div`。
  - 改 `status_card()` 的 `p_5` / `border_1` 时，占位块要跟着一起改。
  - 加减卡片（比如某平台读不到 GPU）后，必须实机看一眼两排宽度是否一致，
    别只看满行的那次。

---

## P3 卡片徽章不能靠「文字自己会缩」

Note: `2026-10-05-os-version-badge-width.md`

- **症状**：健康概览卡片右上角的系统版本徽章画到了卡片**外面**，压在旁边
  那张卡的位置上。Windows 上必现，macOS 上不明显。
- **根因**：两处叠加。
  1. 内容长度是平台相关的：macOS 的 `long_os_version()` 是 "macOS 15.6"，
     Windows 是 "Windows 11 Home China"（中文 SKU 更长），按 macOS 的长度
     配的版面到 Windows 就装不下。
  2. gpui 的 `text_ellipsis` 靠不住：nowrap 文字第一次测量（`MaxContent`）
     的结果会被缓存，之后拿到确定宽度时不会重新截断，于是既不省略也不换行，
     直接画出去。
- **防护**：
  - 源头先瘦身：[`short_os_name`](../src/core/status.rs) 只留「系统 + 版本号」，
    测试 `os_name_keeps_version_and_drops_the_sku`。
  - 版面兜底：徽章走 `header_chip`（可收缩 + `truncate`），卡片外壳
    `status_card()` 带 `overflow_hidden`，画出去的部分一定被裁掉。
- **改状态卡片文案时**：
  - 禁止假设「文字长了会自动省略」，gpui 这条路在这里不通。
  - 禁止拿 macOS 的字符串长度当版面依据，两边的系统名不一样长。
  - 往徽章里塞新内容前，先在 Windows 中文系统上看一眼。

---

## P4 WMI 方法入参的 `uint32` 在 VARIANT 里是 `VT_I4`

Note: `2026-10-05-wmi-uint32-variant-type.md`

- **症状**：调 WMI 方法（联想传感器的
  `LENOVO_OTHER_METHOD.GetFeatureValue(IDs)`）**每一次**都返回
  `0x80041005`（`WBEM_E_TYPE_MISMATCH`）。类可读、对象路径拿得到、权限也
  对，就是调不动。错误码指向「参数类型不匹配」，很容易往「是不是要管理员」
  「是不是这台固件不支持」上想——实测在这两条岔路上各绕了一轮。
- **根因**：MOF 里写的是 `uint32`，于是按 `VT_UI4` 填 VARIANT。但 **WMI 只
  用自动化（Automation）兼容的那一小撮 VARIANT 类型，无符号 32 位不在其
  中**：CIM 的 `uint32` 在 VARIANT 里一律是 `VT_I4`，按位塞进有符号的
  `lVal`。同理 `uint16` → `VT_I4`，`uint64` / `sint64` → `VT_BSTR`（字符串）。
- **防护**：
  - [`Wmi::call_number_with_args`](../src/platform/windows/wmi.rs) 的
    `Arg::Number` 固定填 `VT_I4`，注释里写明了为什么。
  - `Put` 的第四个参数传 0：往实例上写时类型取自类定义，自己再指定一遍
    只会多一处对不上的机会。
  - 自检：[`examples/thermalprobe.rs`](../examples/thermalprobe.rs) 里那段
    `root\default:StdRegProv.EnumKey(hDefKey, sSubKeyName)`——普通用户就能
    调，`ReturnValue = 0` 说明「取类定义 → GetMethod → SpawnInstance →
    Put → ExecMethod」整条链是通的。厂商类要管理员，没有这段自检就分不清
    「参数塞错了」和「没权限」。
- **改 WMI 调用时**：
  - 禁止按 MOF 的字面类型去挑 VARIANT 标签，先查 CIM→VARIANT 的映射。
  - 入参**要么全给要么别给**：漏掉一个可选参数（比如 `EnumKey` 的
    `sSubKeyName`）会得到 `0x80041008`（`WBEM_E_INVALID_PARAMETER`），和
    类型写错的报错很像，但根因完全不同。
  - 半同步查询（`WBEM_FLAG_RETURN_IMMEDIATELY`）的失败在 `Next()` 上报，
    不在 `ExecQuery` 上。把 `Next()` 的负 HRESULT 当成「枚举完了」，
    「没权限」就会伪装成「没有实例」——这个坑本身也踩过一次。

---

## P5 Windows 目录也有只读位，`RemoveDirectory` 对只读目录直接 access denied

Note: `2026-10-05-readonly-directory-deletion.md`

- **症状**：`go/pkg/mod` 清理失败清单里一长串
  `拒绝访问 (os error 5)`，路径全是**目录**（`…@v1.2.3`、`.github`、
  `.circleci`）。同批日志显示文件已经删掉了不少，但空壳目录留下，父目录
  报 `目录不是空的 (os error 145)`。用户看起来像「整个 go 缓存清不掉」。
- **根因**：Go 工具链故意把 module 目录设成 `FILE_ATTRIBUTE_READONLY`
  防意外修改。文件侧的 `remove_file_forcing` 早就 `clear_readonly` 再重试；
  **目录侧以前直接 `std::fs::remove_dir`，漏了清只读**。Windows 的
  `RemoveDirectory` 对带只读位的目录返回 `ERROR_ACCESS_DENIED`，不是
  Unix 那种「目录只读照样能删里面的文件、也能 rmdir 空目录」。
- **防护**：
  - [`remove_dir_forcing`](../src/core/cleaner.rs)：先试删 → 清目录只读位 →
    重试。`delete_tree` 的目录出口必须走它，不要写回裸 `remove_dir`。
  - 测试：`deletes_readonly_directory_shell`（`core::cleaner::tests`）——
    只读目录 + 里面的文件，整棵树必须删干净。
- **改 `delete_tree` / 目录删除时**：
  - 禁止把目录删除「简化」成单次 `std::fs::remove_dir`。
  - 禁止假设「文件的 clear_readonly 已经够了」——目录属性是另一回事。
  - 新增任何「先删内容再删自己」的路径，出口都要过 `remove_dir_forcing`。

---

## P6 应用写在日志目录上的 Deny Delete ACL，提权后要先拆再删

Note: `2026-10-05-deny-delete-acl-override.md`

- **症状**：WorkBuddy 进程早已退出（`Get-Process` 查不到），但
  `~/.workbuddy/logs/2026-09-06` 仍删不掉。`Get-Acl` 能看到：
  ```text
  LAPTOP-…\USER  Deny  DeleteSubdirectoriesAndFiles, Delete
  ```
  手动 `Remove-Item` 报 Access denied。清理日志里是
  `拒绝访问 (os error 5)`，不是 `os error 32`（句柄占用）。
- **根因**：应用用 DACL Deny 做防删，进程退了 ACL 还在。以前的删除路径
  只试一次 `remove_file` / `remove_dir`，拿到 PermissionDenied 就记失败
  放弃。**提权进程本来可以 takeown + icacls 拆掉 Deny 再删**——没有进程
  占用时这不是「系统不让删」，是我们没走完该走的步骤。
- **防护**：
  - [`force_delete_access`](../src/platform/windows/security.rs)：takeown
    `/a` + **`icacls /remove:d`（当前用户 SID + Everyone）** +
    `icacls /grant *S-1-5-32-544:F /c /q /l`，只修当前节点，每条命令最多两秒。
    禁用递归与继承授权（P43）。用 SID 不用组名
    （中文系统上「Administrators」本地化后按名字授权会静默失败）。
  - **不能只 `/grant`**：Windows AccessCheck 把命中的 Deny 当权威，
    后面的 Allow 盖不回去。WorkBuddy 日志实机就是「Deny Delete + Allow
    FullControl 并存」——只加 Administrators Full 仍然删不掉。
  - 接线位置：[`remove_file_forcing`](../src/core/cleaner.rs) /
    [`remove_dir_forcing`](../src/core/cleaner.rs)，**只在
    `ErrorKind::PermissionDenied` 时触发**。error 32（句柄占用）改 ACL
    也解不开，不要误走这条慢路径。
  - Deny `(D,DC)` 常写在**父目录**上。删子文件失败时，叶子上 icacls 往往
    仍返回成功（叶子本来没有 Deny），不能据此跳过父目录。必须叶子拆完再
    删、仍失败才拆未受 `is_protected` 保护的父目录再删。
  - 未提权时 `force_delete_access` 直接返回 false，不空跑子进程。
  - macOS 恒 false（没有这种 Deny Delete 防删形态），门面契约两平台同签名。
- **改删除失败重试时**：
  - 禁止对所有失败一视同仁地跑 takeown/icacls——必须区分
    PermissionDenied 与 sharing violation。
  - 禁止把 `/remove:d` 改回只 `/grant`——那是 AccessCheck 语义，不是实现细节。
  - 禁止在未提权时也去起 `takeown`（会弹 UAC 或直接失败，还拖慢整批）。
  - 禁止把 `force_delete_access` 从门面契约里拿掉——拿掉后 macOS 编不过，
    或 Windows 上静默不再有 ACL 补救。
  - 禁止把「只对失败的那条叶子拆 ACL」当成已经够了——目录级 DC Deny 不会
    出现在子文件的 DACL 上。

---

## P7 PendingFileRenameOperations 里的路径重启前谁都删不掉

Note: `2026-10-05-pending-reboot-delete-paths.md`

- **症状**：卸载后残留目录（百度网盘
  `AppData\Roaming\baidu\BaiduNetdisk\YunShellExtV164.dll.<时间戳>`）反复
  清理失败。日志：`SHFileOperationW 返回 0x0000007C`；`Remove-Item` 报
  Access denied。ACL 上用户是 FullControl、没有 Deny，takeown 也失败。
- **根因**：卸载器（或 Windows 文件替换）用
  `MoveFileEx(MOVEFILE_DELAY_UNTIL_REBOOT)` 把锁住的 shell 扩展挂到
  `HKLM\SYSTEM\CurrentControlSet\Control\Session Manager\
  PendingFileRenameOperations`，条目形如 `*1\??\<path>`（`*1` = 重启删除）。
  **登记之后到重启之前，系统拒绝一切删除/改名**，与 ACL 无关。
- **防护**：
  - [`is_pending_reboot_delete`](../src/platform/windows/residuals.rs)：
    读 REG_MULTI_SZ，匹配 `*1\??\<归一化路径>`。
  - [`clean_residuals`](../src/platform/windows/residuals.rs)：命中时记
    [`CleanResult::ManualAction`]，不记 Failed——重试无意义，出路是重启。
  - 状态文案：`tr_status_residual_cleaned_manual` 写明「需重启后由系统清除」。
- **改残留清理 / 删除失败分类时**：
  - 禁止把 pending-delete 当成普通 Access denied 去走 ACL 补救（白跑 takeown）。
  - 禁止把它记进 `failed` 并自动重开重试对话框。
  - 重启后 Windows 会自己删；残留扫描在重启后不应再报这些路径。

---

## P8 见过卸载进程再没了，15s fail-fast 会误杀慢 UAC

Note: `2026-10-05-uninstall-orphan-wait-windows.md`

- **症状**：Inno/NSIS 卸载，UAC 还在等用户点是，界面已经报卸载失败。用户读提示
  或离开座位超过十几秒必现。`--no-elevate` 或用户范围再提权时更容易碰到。
- **根因**：`saw_procs` **只统计 `wait_for_uninstall_settled` 环里见过的进程**，
  不含已经 `child.wait()` 掉的父进程。所以「父进程 exit 0 → 直接等 UAC」
  （`child_ok && !saw_procs`）本来就不会 fail-fast。会误杀的是**两阶段**：
  父进程拷到临时目录并拉起第二阶段（stem 仍是 `unins000`，环里 `saw_procs=true`），
  第二阶段再 `RestartElevated` 退掉，UAC 弹出来时进程列表是空的。
  这和「用户点了取消、向导退干净」信号完全一样，只能靠超时区分。
  安装目录里的应用进程在 UAC 期间被关掉，也会走进同一条 `ProcsVanished`。
- **防护**：
  - [`uninstall_orphan`](../src/platform/windows/apps.rs) 把两种空等拆开：
    `FailedCommand`（命令失败且从未见进程）15s；`ProcsVanished` 120s。
  - `child_ok && !saw_procs` 仍是 `None`，交给 30 分钟总超时。
  - 测试：`uac_gap_is_not_orphaned`、`wizard_ran_then_exited_is_orphaned`
    （断言 `ProcsVanished` 的超时 ≥ 60s 且长于失败命令）、
    `failed_command_without_process_is_orphaned`。
- **改卸载收尾等待时**：
  - 禁止把 `ProcsVanished` 和 `FailedCommand` 合成同一个 15s。
  - 禁止把 `saw_procs` 理解成「含 child.wait() 那个父进程」——那样会把
    普通 UAC 空窗也判成空等。
  - 禁止为了「取消更快返回」把 `ProcsVanished` 压回十几秒。

---

## P9 自动更新：macOS 回退不能跨架构，helper 必须离开父进程

Note: `2026-10-05-updater-arch-fallback-and-detach.md`

- **症状**：Intel Mac 在没有 universal zip 时可能装上 aarch64 包，重启打不开。
  或用户点安装后应用退出，新版本没换上，只剩 `QuickCleaner.app.old`。
- **根因**：
  1. 候选列表把 `aarch64` 写在 `x86_64` 前面，且两种 Mac 都订阅同一份列表。
  2. 替换脚本是父进程的子进程；`cx.quit()` 后会话 SIGHUP（macOS）或 job
     结束（Windows）会把 helper 带走。死在 `mv` 之后、`cp` 之前，磁盘上
     没有可启动的主程序。
- **防护**：
  - [`candidate_asset_names`](../src/core/updater.rs)：universal 之后只跟本架构
    zip。测试 `mac_fallback_never_crosses_architecture`、
    `intel_mac_does_not_pick_aarch64_when_universal_missing`。
  - macOS helper：`setsid` + `trap '' HUP` + stdio 接到 `/dev/null`。
  - Windows helper：`CREATE_NEW_PROCESS_GROUP | CREATE_BREAKAWAY_FROM_JOB`，
    job 不允许 breakaway 时去掉该标志再 spawn。
- **改更新安装交接时**：
  - 禁止把「另一个 CPU 的 zip」放进候选列表当 fallback。
  - 禁止再改回「裸 spawn bash/powershell、不脱离父进程」。

---

## P14 未登记应用：快捷方式证明能启动，不证明拥有父目录或能卸载

Note: `2026-10-04-unregistered-windows-apps.md`

- **症状**：Hermes 源码安装没有 ARP 卸载登记，应用列表找不到它。直接把快捷方式目标的父目录作为安装目录，会误删下载或共享工具目录。
- **防护**：未登记应用保存精确 exe 与快捷方式证据；普通 portable 默认只移除这些文件。同名数据和注册表不默认勾选。官方卸载必须匹配内容与布局适配器，执行前复核，用程序树外运行时调用官方保留数据模式。
- **禁止回退**：不能因名称匹配授权卸载，不能猜 `uninstall.*` 执行，不能从未知 exe 推断父目录所有权；无 ARP 和退出码 0 均不能代替安装产物消失。短路径和 verbatim 路径必须先规范化。
- **测试**：`src/platform/windows/app_discovery.rs` 中的 Hermes 无 ARP、portable 父目录、数据置信度、官方进程保留数据及空操作后验证清理测试。

---

## P15 Windows 陈旧 SQLite 的原始占用探测不能回调活库安全判定

Note: `2026-10-04-windows-sqlite-probe-recursion.md`、`2026-10-05-live-database-crash-leftover-channel.md`

- **症状**：陈旧 SQLite 删除测试栈溢出；增大线程栈也无效。
- **根因**：`is_live_database → looks_like_crash_leftover → is_open → spot_check_without_handle_probe → is_live_database` 递归。
- **防护**：Windows 原始占用探测独立使用排他共享打开；真实占用返回 true，未知错误与链接返回 None，不回调 safety。保留活库闸门及陈旧且确定无人占用时的例外。
- **禁止回退**：不能将原始探测接回 spot-check 策略；不能把未知占用改成无人占用；不能靠增加栈大小处理递归。
- **测试**：`delete_tree_cleans_stale_nested_sqlite_family` 与 `stale_sqlite_raw_probe_is_nonrecursive_and_detects_shared_handles`（真实共享句柄）。

---

## P16 Hermes 外置依赖与半卸载：最后才能删除安装记录

Note: `2026-10-04-hermes-split-install-uninstall.md`

- **症状**：官方 lite 卸载退出 0，启动器或源码已删，但外置 venv、工具与 Gateway 登记还在；应用又从列表消失，无法重试。
- **根因**：依赖位于 `installs/<源码规范路径 SHA256 前16位>` 和 `tools`，不是源码子目录。官方模块可能吞掉错误，删除模块自身不证明卸载完成。
- **防护**：核验源码或精确安装记录；允许缺失 exe/源码/运行时的恢复发现；先清已确认产物并复核登记，最后删 facts 和安装状态。安装器、构建进程、命名 profile 或新共享引用会阻止危险清理。文件仍统一由 core cleaner 执行。
- **禁止回退**：不能递归删除数据 home；不能从 `hermes-agent` 名字授权恢复；不能提前删除 retry 证据；不能按 Gateway 名称删除任务；不能清其他安装引用的 tools；不能把读失败当作已不存在。
- **测试**：`half_uninstalled_hermes_and_dead_shortcuts_remain_discoverable`、`late_failure_keeps_install_record_for_recovery`、`stale_residual_scan_cannot_remove_newly_shared_tools`、`gateway_task_cleanup_requires_exact_installation_action`。

---

## P17 Windows 降权启动：登记长命令超过原生接口限制

Note: `2026-10-05-windows-desktop-user-long-command.md`

- **症状**：Hermes 卸载动画结束，启动器已删，程序依赖和恢复记录仍在；降权启动报参数错误 `-2147024809`。
- **根因**：`CreateProcessWithTokenW` 命令行最多 1024 个 UTF-16 单元，普通 `Command` 成功不能证明降权调用成功。后续登记清理脚本比官方 Python 调用长。
- **防护**：长命令以 JSON 参数数据交给固定桌面用户 dispatcher；保留 SID 核验、原始引用和工作目录，禁止管理员回退。调用失败立即保存 OS 错误，随后才释放环境块。失败显示原因并保留重试记录。
- **禁止回退**：不能恢复直接传入长脚本，不能把参数拼成脚本源码，不能用动画结束或退出码替代登记核验；不能只测非提权启动。
- **测试**：`registration_cleanup_handles_quoted_paths_and_preserves_unrelated_values` 必须经过真实启动器，并在管理员测试进程中验证；`long_desktop_command_preserves_arguments_cwd_exit_and_cleans_staging` 验证真实子进程回执。

---

## P18 规则远程缓存水位（已退休）

Note: `2026-10-05-bundled-runtime-rules.md`

用户取消远程规则下发；客户端不再加载规则下载缓存，旧水位算法及回归仅留历史归档。不能恢复下载入口或将静态清单迁移误称为通用规则重构全部完成。

## P19 相同路径去重必须合并全部约束

Note: `2026-10-05-duplicate-rule-constraints.md`

- **根因**：只取第一条会丢保留项或隐藏处置冲突；UI 合并不能保护 core 的其他调用者。
- **防护**：先在 core 合并，再执行；推荐取交集，保留项和归属证据取并集。处置、身份或快照冲突阻止目标，不能按加载顺序决定。Unix 仍用 dev/ino 复核，不加 mtime/len 相等约束。
- **测试**：`duplicate_scan_policies_cannot_silently_select_a_deletion_method`、`core_duplicate_merge_preserves_every_rule_in_both_orders`。
- **动态来源**：提供者必须显式提交类型化范围/资源参数和具名配置策略，不得恢复类别或 URI 操作推断。策略观察冻结，缺失和处置冲突阻止。Note: `2026-10-05-runtime-provider-policies.md`。

## P20 已尝试生态命令后，超时不能回退裸删

Note: `2026-10-05-owner-command-timeout.md`

- **根因**：Go/pnpm 变更命令的 `?` 将超时传成 None，cleaner 把它当成 owner 不适用，继续删未知状态的 store。
- **防护**：None 只用于执行前缺工具或范围失配；已尝试清理必须返回 Some(false) 阻止回退。完成按原生资源状态核验，读失败不能当不存在。
- **测试**：`attempted_owner_timeout_never_grants_filesystem_fallback`、`owner_completion_checks_resource_state_after_zero_exit`、`remove_verifies_exact_reference_and_rejects_unknown_inventory`。

## P21 外置运行环境与恢复记录不能作为同一步树删除

Note: `2026-10-05-install-record-last.md`、`2026-10-05-capability-plan-lifecycles.md`

稳定期不能把刻意留到最后的恢复记录要求为已消失。精确豁免与最终完整核验的决策及真实扫描库存红跑绑定见 Note: `2026-10-05-settle-window-inside-lifecycle.md`。

- **根因**：installs 状态里同时含 environments 和 facts；把整个状态目录留到最后，会在运行环境还存在时完成核验，最终失败又可能先删 facts。
- **防护**：补充清理先移除拥有的依赖子项，保留 facts 所在子树；核验后才清记录。最后一步前复查进程与共享引用。规则不能重排五步骤依赖。
- **测试**：`verification_runs_after_runtime_cleanup_and_before_record_removal`、`late_failure_keeps_install_record_for_recovery`、`lifecycle_keeps_retry_records_after_any_dependency_failure`。
- **报告约束**：源码五步骤经共用能力 runner 执行，失败或取消必须保留已执行结果和原始原因并阻止依赖步骤；`source_runner_reports_fixed_dependencies_and_preserves_failure_reasons` 覆盖所有失败位置、取消和缺失扫描事实。不能为返回兼容 Result 丢弃失败报告，也不能重建扫描授权。

---

## P22 Worktree 不能整删容器，登记只能精确清理

Note: `2026-10-05-exact-worktree-registration.md`、`2026-10-05-protected-worktree-discovery.md`

- **发现与删除一致**：会话资源被 core safety 保护时，开发分类生成也必须调用同一安全判定排除；不能为了让目标“能清”放宽保护或表级不变量。隔离测试 `owned_agent_session_worktrees_are_filtered_by_core_safety` 验证先可发现、出现归属证据后排除，并保留 checkout 和 Git admin；原有 `every_target_has_cleanable_contents` 继续约束整表。

- **症状**：Maka 创建的 `copilot-api` worktree 漏检；旧清理只删 agent 的 worktrees 目录，Git 登记仍指向已删除的 checkout。
- **根因**：把固定容器当垃圾目录，未核对 `.git → admin/commondir → admin/gitdir` 双向关联。Windows canonicalize 的 `\\?\` 前缀也会使 Git 参数和路径保护失配。
- **防护**：在已知 agent 容器浅层枚举逐个 worktree，遇到仓库根停止；扫描不启动 Git、不查状态、不遍历源码。执行前核对身份、关联、锁、未提交/未跟踪/忽略文件与子模块；普通驱动器路径交给 Git 和 safety。文件仍经 core cleaner 删除，确认目录消失后用无 force 的 `git worktree remove -- <精确路径>` 清登记，核验 admin 条目消失。
- **禁止回退**：不能用仓库级 `prune --expire now` 影响其他登记；不能凭退出码 0 认为清理完成；不能在核验失败或命令超时后回退裸删；不能将链接 worktree 退化成普通 Tree。失败删除保留登记；回收站处置拒绝此永久清理通道，避免还原后 `.git` 失效。
- **测试**：`cleanup_removes_exact_registration_and_preserves_other_stale_entries`、`dirty_locked_and_changed_registration_never_fall_back_to_deletion`、`discovery_only_lists_linked_checkouts_and_stops_at_repository_roots`。
- **统一流程约束**：worktree 的步骤报告必须执行 checkout 消失核验，再调用精确注销，最后独立核验 checkout/登记均消失。`worktree_runner_retains_registration_after_failed_or_unverified_checkout_cleanup` 覆盖删除失败、空操作、登记变化、取消和未知占用；不能为了报告成功跳过这些依赖或把 Git 退出码当成最终完成。

---

## P23 后台 Git 弹控制台，拒删原因不能藏在长路径后

Note: `2026-10-05-hidden-background-worktree-reasons.md`

- **症状**：Windows 清理每个 worktree 时弹黑窗口；含未提交源码的 5 个 worktree 显示“仍有残留”，长路径把真正原因挤出可见范围。
- **根因**：通用超时命令入口没有 `CREATE_NO_WINDOW`；worktree 检查只返回 bool，脏文件、锁定和未知登记全部变成“占用状态未知”；原因排在完整路径后。
- **防护**：后台命令在 Windows 创建前设置 `CREATE_NO_WINDOW`，保留管道、超时和退出码；worktree 返回明确的类型化拒删原因；中英文详情优先显示原因和体积，再显示路径。含源码改动的真实 worktree 继续保留。
- **禁止回退**：不能为了无窗口丢弃 stderr 或成功核验；不能把无法核验当作可删除；不能为“清干净”放宽未提交改动保护；不能把原因移回长路径后。
- **测试**：`windows_background_command_has_no_console_and_keeps_output_and_exit_code`、`dirty_locked_and_changed_registration_never_fall_back_to_deletion`、`worktree_reason_precedes_long_path_in_both_languages`。

---

## P24 已合并的 worktree 仍可能被应用会话引用

Note: `2026-10-05-managed-worktree-reference-retirement.md`

- **症状**：14 个干净 Maka worktree 清理后，Host 启动报 `Live subagent worktree is unavailable`，模型连接刷新失败。
- **根因**：Git 干净和分支已合并不代表应用工作区绑定已退休。Maka 启动从会话元数据恢复不可变 `subagentWorkspace`；只删目录与 Git 登记会留下活引用。
- **防护**：`core/safety.rs` 根据 workspace 中的 Maka 标记或 `runtime.sqlite` 保护 `subagent-worktrees` 和全部后代；普通目录、单文件及 worktree 清理均不能绕过。明确显示应用会话引用拒删原因。原生 `session.remove` 是目前支持的协调入口，先提交会话墓碑，再清目录、分支和 Git 登记；删除子任务聊天必须获得明确授权，不能由普通目录清理确认隐含授权。归档不会解绑。QuickCleaner 自动原生删除入口尚未集成。
- **禁止回退**：不能把“已合并”“执行结束”或“已归档”当成解绑；不能直接改运行中的 SQLite 字段；不能只检查命令退出码；原生失败或超时不得回退裸删；不能写死 `copilot-api` 或用户目录作为归属判据。
- **测试**：`managed_clean_checkout_cannot_be_deleted_without_reference_retirement`（干净 Git、迁移 workspace、单文件和 `.git` 消失仍受保护）；原有精确登记和脏目录测试继续有效。

## P25 安装产物同秒同长度替换能绕过 Windows 弱身份

Note: `2026-10-05-installation-stable-object-identity.md`、`2026-10-05-frozen-official-uninstall-command.md`、`2026-10-05-frozen-scan-plan-references.md`

- **根因**：通用 TargetIdentity 在 Windows 只比较秒级 mtime 与长度；旧文件改名后，同长度的新文件可在同一秒通过复核。执行时重建产物列表还可能扩大扫描授权。
- **防护**：安装实例冻结产物范围及确认缺失状态；现状只能收缩范围，不能增加新目标。Windows 安装产物额外核验卷序列号与文件 ID，读取失败或链接/reparse 祖先不能确认身份。Unix 保持 dev/ino。通用 TargetIdentity 其他 Windows 调用点仍须后续迁移，不能称全部身份防护已完成。
- **禁止回退**：不能以修改测试长度、增加 sleep 或重建授权替代稳定身份；不能给 Unix 加回 mtime/len。
- **测试**：`frozen_installation_instances_reject_expansion_appearance_and_replacement`、`discovered_installation_plan_rejects_new_artifacts_and_keeps_snapshot`。
- **快捷方式/启动项约束**：扫描合并完成前捕获快捷方式身份，克隆沿用最终计划；补充清理每个 startup/shortcut 删除前复核冻结身份，尤其不能因登记回调前已经检查过就跳过。同内容同时间的 Gateway 启动项替换仍须保留；`supplement_retains_same_content_startup_replacement` 与 `supplement_rechecks_frozen_shortcut_after_registration_callback` 绑定此约束。确认消失可恢复，未知和未观察路径不可授权。
- **官方命令约束**：卸载命令连同样本证据（解释器、声明的 module 文件）冻结进 `CleanupPlan::official`，执行只走 `OfficialOperation::command()` 的复核结果，不能在执行时重新发现解释器或切换路线。模块文件扫描时缺失或重定向必须冻结 powershell 恢复路线——启动脚本对扫描后才出现的模块文件照样 runpy，冻结解释器路线会把「扫描后投放的代码」变成执行通道。红跑证明旧重建路径会真的拉起被替换的假 python.exe（os error 216）。
- **测试**：`frozen_official_command_rejects_replaced_interpreter`、`frozen_official_command_rejects_replaced_module`、`frozen_recovery_route_never_switches_to_late_runtime`、`missing_module_freezes_recovery_even_with_runtime`（`platform::windows::app_discovery::tests`）。

---

## 旧编号迁移区（原 AGENTS.md 摘要表，内容为一行版，待按需要补全症状/防护/测试）

## P26 File Provider 的 `SF_DATALESS` 目录 `stat` 正常但 `readdir` 永久卡死

要靠 `getattrlistbulk` 的 `ATTR_CMN_FLAGS` 识别跳过，不能按路径名猜。

## P27 索引不含被跳过的子树

dataless / hang 集的缺项不能当成文件不存在——别把 SizeTree 的缺项当成文件不存在。

## P28 Unix 身份复核只认 dev+ino

别把 mtime/len 加回去（会永久拒删活跃文件）。与 P19 的「Unix 仍用 dev/ino 复核」同一条约束。

## P29 `lsof +D` 复检目录批用独立的长超时

不能和文件批共用 3 秒超时。

## P30 `vendor/gpui` 与 `runtime_shaders` 不能删

前者是启动死锁补丁，后者免 Metal Toolchain。

## P31 孤儿残留（已卸载软件）只认 Bundle ID 名

Note: `2026-10-05-orphan-residual-bundle-id-rule.md`

家族判据和「测不出」都不能放宽。

## P32 应用缓存靠内容签名认

Note: `2026-10-05-app-cache-content-signatures.md`

应用名只决定归类；Profile 本体、旧版本当前版、空目录都有硬规矩。

## P33 活库闸门要留「崩溃残留」可证伪通道

Note: `2026-10-05-live-database-crash-leftover-channel.md`

伴随文件存在 ≠ 有活连接。

## P34 首次窗口绘制前不能同步生成清理目标

窗口级回调不能调用需要当前视图的 `request_animation_frame()`。


## P35 Windows Temp 的 contents 不能按树删除范围复核

Note: `2026-10-05-protected-temp-contents-scope.md`

- **根因**：目录本身受保护被误作内容范围禁止；旧提供者能列出 Temp，但计划/cleaner 拒绝，规则迁移还会丢失目标。
- **防护**：扫描、计划与内容删除复用 core safety 的 is_contents_protected。Windows Temp 根保留、子项继续逐个防护；树删除、系统子树、驱动器根、白名单和会话保护仍拒绝。不得用虚拟子路径探测或跳过 safety 替代范围判定。
- **根归属**：user_temp 仅来自可信前台用户；未知就跳过，不能回退进程账户。点目标仅支持 user_temp contents 无变量，其他骨架根与盘符逃逸拒绝。
- **测试**：`contents_scope_preserves_self_banned_roots_and_keeps_subtree_protection`、`contents_scope_still_protects_whitelist_and_managed_worktrees`、`system_rules_keep_temp_scope_and_unknown_user_root_never_falls_back`。

## P36 规则 TOML 里被注释吞掉的表头会报成无关表的重复键

Note: `2026-10-05-declarative-path-templates.md`

- **症状**：改完 `rules/*.toml` 后 build script 直接 panic，报的是完全没动过的表，例如
  `parse rule TOML: duplicate key 'path' in table 'catalogs.vscode_family'`；照这个位置去找，代码里根本没有重复键。
- **根因**：删/改注释时把空行一起删掉，`# 说明…` 和后面的 `[[entries]]` 挤在同一行，表头整行变成注释，
  后面的键全部落进上一个表，于是报错点在无关位置。
- **防护**：注释块与表头之间保留空行；任何规则改动后跑 `cargo run --example rules -- check`，
  整包校验会把这类错误变成显式失败。模板与选择器边界另有回归覆盖：`path_templates_stay_declarative_and_bounded`。
- **禁止回退**：不要为了让 check 通过去改无关表；不要放宽 `directories` / `version_layouts` 校验。

## P37 残留清理删每个条目前必须复核扫描期身份

Note: `2026-10-05-windows-residual-identity-gate.md`

- **症状**：残留清理里，扫描后被换掉的目标（同名不同物）被当成原残留删掉。用户看到的是「清理完成」，实际把窗口期内活应用重建的配置/登录态送进了回收站。
- **根因**：macOS `clean_residuals` 在 `dispose` 前有 `item.identity.recheck(path)` 闸门，Windows `clean_residuals` 从 pending-reboot 检查直接走到 `dispose`。两边都「看起来完整」，编译和测试都过，不对称因此长期没暴露。批次级 `validate_discovered_residual_clean` 只在清理前整批跑一次、且是 Windows 发现式专属，替代不了逐项、删除前的复核。
- **防护**：Windows `clean_residuals` 的 `File`/`Directory` 分支在 `dispose` 前要求 `item.identity.is_some_and(|identity| identity.recheck(path))`；失败记 `CleanResult::Failed` 并跳过，身份缺失同样拒绝（fail closed），**绝不回退裸删**。与 macOS 共用同一道闸门。
- **测试**：`residual_cleanup_rejects_path_replaced_after_scan`（Windows 与 macOS 各一份）。红跑证据 `docs/agent-notes-evidence/2026-10-05-windows-residual-identity-red.log`（撤闸门 → 失败），恢复后 `...-green.log`。
- **禁止回退**：不要为了「能删掉」去掉身份复核或改成只在文件缺失时跳过；不要把批次级快照检查当作逐项复核的替代。

## P38 开发资产的清单只能来自生态自己，按目录列出来的条目只能展示

Note: `2026-10-06-dev-asset-inventory-authority.md`

- **症状**：开发环境页把 `node_modules` 的每个直接子目录都列成一个「用户装的全局包」。
  本机实测 bun 的清单声明 3 个全局包，目录里躺着 132 个目录——另外 129 个是那 3 个的
  传递依赖，全部以同等样貌出现在列表里，且看起来一样可删。删掉 `@babel/parser`
  会打断依赖它的工具，而用户不知道它是谁装的。同类错误还包括：`~/.conda` 被当成
  conda 安装根，凭空多出一个名叫 base 的假环境；Windows 上取
  `user_cache_dir().parent()` 得到 `AppData`，探的是 `AppData\miniconda3` 而不是
  `%LOCALAPPDATA%\miniconda3`。
- **根因**：目录结构不携带「谁装的」这一信息。传递依赖与用户显式安装的包在
  `node_modules` 里没有任何可靠区别；生态早就把答案写在自己的清单里
  （`node_modules` 旁边的 `package.json` 的 `dependencies`、`conda info --json` 的
  `envs` 与 `root_prefix`、`pipx list --json`），我们却去数目录。
- **防护**：清单只有两级来源，条目自己带着这个事实（`core::dev_env::AssetSource`）：
  `Tool` = 生态自己声明的，`Layout` = 命令与声明都拿不到时的降级扫描。只有 `Tool`
  来源的条目进移除通道（`remove::channel_for` 是「哪个生态能删」的唯一名单，界面用的
  `can_remove` 与执行用的 `PreparedRemoval` 都从它取）。解析只有一份，在
  `core::dev_env::inventory`，发现层与移除层共用——两份解析会给出「界面上有这个包、
  点删除却说不存在」。base 环境由 conda 报的 `root_prefix` 认定，不猜安装根。
  清单回退到 mamba 时必须冻结并执行 mamba，不得又调用 conda；
  `review_mamba_inventory_freezes_mamba_executor` 锁定此行为。
  生态写在自己安装目录里的**登记文件**也算生态声明，不算「按目录列」：pip 的
  `*.dist-info`（PEP 376）与 venv 的 `pyvenv.cfg`（PEP 405）是这两个生态自己写的
  账本，枚举它们与跑命令读清单是同一权威（见
  `core::dev_env::python::pip_packages` 的文档）。
- **测试**：`a_transitive_dependency_is_not_mistaken_for_an_installed_package`、
  `bun_items_are_never_removable_even_when_its_manifest_declares_them`、
  `without_a_manifest_the_listing_falls_back_to_the_directory`、
  `dot_conda_directory_is_never_listed_as_an_environment`、
  `a_layout_sourced_item_has_no_removal_channel`。红跑证据
  `docs/agent-notes-evidence/2026-10-06-dev-asset-inventory-authority-red.log`。
- **禁止回退**：不要把「按目录列」当成等价实现加回来（包括「按目录列但灰掉」——
  灰掉同样需要自己反推依赖图）；不要让 `Layout` 来源的条目获得移除入口（venv 例外：
  它的凭据是 `pyvenv.cfg` 本身，预检重验，见 `remove::Channel::VenvDirectory`）；不要在
  `discovery` 与 `remove` 里各写一份清单解析；`dependencies` 键不存在时不得当作
  「一个都没装」，退回按目录列并降为 `Layout`；不要把「读 `*.dist-info` /
  `pyvenv.cfg` 登记」与「按目录猜安装」混为一谈——前者是读生态的账本，后者才是
  这条禁止的。

## P39 目录 mtime 不是「最后使用时间」，atime 未经证实

Note: `2026-10-06-dev-asset-inventory-authority.md`

- **症状**：一个天天在用的 conda 环境，界面显示「3 个月前」；用户据此认为它闲置并
  删掉。反过来，一个装完就没碰过的环境因为刚装过依赖而显示「今天」。既有的
  `core/declutter/large_files.rs` 已经把 mtime 当「最后访问时间」呈现给用户
  （`tr_declutter_col_last_accessed`），是同一处措辞与事实不符。
- **根因**：目录的 mtime 只在**直接子项被增删**时变化，往里写文件不会动它；因此它
  反映「最后一次装/卸」，不反映「最后一次用」。atime 在本仓库完全没有验证过，而且
  Windows 默认不更新最后访问时间（NTFS 的 LastAccessUpdate 策略）——拿它当使用时间
  等于在没有依据的地方给一个数字。
- **防护**：`core::dev_env::TimedEvidence` 把时间点与来源绑在一起
  （`TimestampSource`：conda 事务记录 / 包的 `package.json` / 工具元数据 / 目录项），
  界面按来源措辞并显示依据，列名是「最后变更」而不是「最近使用」。时间**不参与
  预选**、不参与任何删除判定。没有依据时显示「无记录」，不拿别的字段凑。
- **测试**：`timestamps_only_come_from_tool_authored_sources`、
  `inspect_conda_meta_reads_version_count_and_transaction_time`、
  `a_read_error_is_not_absence`（读失败同样不是证据）。
- **禁止回退**：不要用 `atime` 判定闲置；不要把目录 mtime 写成「最后使用」；不要给
  `minimum_age` 之类的阈值配上自动勾选；不要删掉 `TimestampSource` 只留一个裸时间戳。

## P40 Windows 工具链命令要先做 PATHEXT 解析，`Command::new("npm")` 找不到 `.cmd` 垫片

Note: `2026-10-06-windows-tool-shim-launch.md`

- **症状**：`npm` 明明在 PATH 上，开发环境页的 npm 通道却是空的；`uv`、`node` 这类有真
  `.exe` 的命令一切正常，所以看起来像「有些生态认得出来、有些认不出」。同一个缺陷让
  `npm uninstall --global` 也跑不起来——界面给了移除入口也只会永远拒绝。
- **根因**：`CreateProcess` **不查 `PATHEXT`**，只找名字与 `name.exe`。npm 在 Windows 上
  装出来的是 `npm`、`npm.cmd`、`npm.ps1`，没有 `npm.exe`，所以启动失败、返回 `None`，
  而失败没有报错，只表现成「这个生态什么都查不到」。pnpm（`pnpm.CMD`）、pipx 同理。
- **防护**：`platform::tool_command` 是工具链命令的唯一入口（`core::proc::run_tool_with_timeout`），
  内部先经 `resolve_tool_program` 按 `PATH` × `PATHEXT` 解析，顺序交给 `PATHEXT`。
  `.cmd` / `.bat` 显式经 `cmd.exe /d /s /c` 启动：实测 `CreateProcess` 的隐式批处理处理
  也能跑，但只有显式路径给得了 `/d`（关掉注册表 `AutoRun`，那是别人写的代码）。两条路
  cmd 都会重新解析参数，所以含 `" & | < > ^ % !` 的参数一律拒绝而不是转义。
  完整命令用 `raw_arg` 和 `/s /c` 最外层引号，不得再用 `.arg(line)` 触发 CRT
  转义；`review_shim_preserves_spaces_and_trailing_backslash` 真实执行带空格脚本，
  锁定空格、尾反斜杠与空参数的传递。
- **测试**：`a_command_that_only_exists_as_a_shim_is_still_found`、
  `pathext_order_decides_which_candidate_wins`、
  `a_name_that_already_has_an_extension_is_not_extended_again`、
  `a_script_shim_is_launched_through_cmd`、`an_argument_with_cmd_metacharacters_is_refused`。
  红跑证据 `docs/agent-notes-evidence/2026-10-06-windows-tool-shim-red.log`（退回
  `CreateProcess` 的解析规则后 npm 通道 0 条）。
- **禁止回退**：不要把工具链命令改回 `Command::new(name)` 或 `run_with_timeout`；
  不要为 `.cmd` 开特例而让两条解析规则并存；不要在 `cmd_line` 里改成「转义元字符」
  放行；新增工具链生态时必须走 `run_tool_with_timeout`（传 runner 时直接用函数本身，
  不要包一层闭包——本轮就出现过探针包了旧 runner 从而看到假象）。

## P41 Python 有两个包目录，界面必须分开报，数量才对得上 `pip list`

Note: `2026-10-07-python-two-package-directories.md`

- **症状**：开发环境页的 Python 那组显示「1 个包」，同一台机器上 `pip list` 报 145 个；
  用户看到的是「明明装了东西，界面说没有」。
- **根因**：一个解释器有**两个**包目录——自带的 `Lib/site-packages` 与用户级
  `site.getusersitepackages()`。微软商店版 / 系统 Python 的安装目录只读，用户
  `pip install` 的东西只可能落在用户级目录，于是两个目录的份额极端不均（本机
  1 : 144）。模型只认一个目录，就量到了那个几乎空的安装目录。
  同一轮还有一个坑：`site.getsitepackages()` 返回**列表**，商店版上它的第 0 个
  元素是 prefix（整个安装目录），照 `[0]` 取会把几百 MB 的安装目录算成包的体积。
- **防护**：`core/dev_env/python` 的 `InterpreterFacts` 同时持有两个目录，
  `interpreter_rows` 对每个存在的目录出一行并标出 `SiteScope`；包数量本地数
  `*.dist-info` 而不是跑子进程枚举发行版；包目录一律从
  `sysconfig.get_paths()["purelib"]` 与 `site.getusersitepackages()` 取，列表形态
  一律不认（留空，不取 `[0]`）。
- **测试**：`one_row_per_package_directory_so_the_count_matches_pip_list`、
  `probe_reads_version_prefix_and_both_package_dirs`、
  `an_array_shaped_package_dir_is_rejected_rather_than_indexed`。红跑证据
  `docs/agent-notes-evidence/2026-10-07-python-two-site-rows-red.log`。
- **禁止回退**：不要把两个目录合并成一行或只报其中一个；不要把 `site.getsitepackages()`
  的第 0 个元素当包目录。移除入口的划分在 2026-10-07 之后收窄为：**安装级行整行
  只读**（`site-packages` 是解释器的一部分，且商店版在文件系统层就不可写，
  `core/safety.rs` 也不保护 `C:\Program Files` 之下的子目录）；用户级行与 venv 行
  本身仍不是删除单位，但可展开，行内的 pip 包条目逐个走 `python -m pip uninstall`
  卸载，venv 整体以 `pyvenv.cfg` 为凭删除（见
  `2026-10-07-pip-package-and-venv-removal.md`）。不要让安装级行或「整目录删除
  site-packages」回来。

## P42 开发环境的体积测算每个文件要两次元数据调用，别把它改回单线程

Note: `2026-10-07-dev-asset-measurement-parallelism.md`

- **症状**：开发环境页一次扫描要十几秒到半分钟，界面只有一个转圈，用户以为卡死。
- **根因**：唯一走遍每个文件的一步是体积测算，而它每个文件至少两次元数据级系统
  调用：遍历一次（`read_dir` + `symlink_metadata`）、取硬链接数再一次
  （Windows 上必须 `CreateFileW` + `GetFileInformationByHandle`——`WIN32_FIND_DATA`
  里没有链接数字段）。本机实测每次约 300µs，4 万个文件顺序执行就是二十多秒。
  慢的不是某个调用写错了，是调用次数。
- **防护**：遍历与句柄两段都并行（`core/dev_env/storage` 里 rayon 递归 + `par_iter`），
  跨目录去重仍顺序做；`per_dir` 与入参顺序对齐（调用方按位置贴回体积）；
  Python 解释器只量包目录，不量整个安装目录。
- **测试**：`per_dir_sizes_stay_aligned_with_the_input_order`、
  `test_measure_real_dir_and_hardlink`（硬链接语义不变）。计时证据
  `docs/agent-notes-evidence/2026-10-07-devenv-scan-performance-green.log`。
- **禁止回退**：不要把这段改回单线程循环，不要为每个文件再加元数据调用，不要
  把「独占可释放」换成不查链接数的近似值（conda 的 pkgs 缓存与环境、pnpm / uv
  的 store 与安装之间都是硬链接，近似值会系统性高估可释放量）。改动前后都跑一次
  `live_probe_lists_the_assets_on_this_machine` 的计时输出对比。

## P43 Windows 非空目录不能触发整树 ACL 恢复与重复删除

Note: `2026-10-07-windows-delete-retry-stall.md`

- **症状**：永久删除接近完成后长时间停顿，已有占用失败；停止后还会重扫剩余目标。
- **根因**：锁住叶子使祖先目录非空，每层无条件修复整树 ACL 并重试子树，失败与遍历次数随深度倍增；权限工具无超时。
- **防护**：叶子与目录出口仅在 AccessDenied 时修复当前节点或未受保护的父节点，工具等待有界；取消后不再启动剩余目标称重。决策与测试绑定见 Note。
- **禁止回退**：不能以 error 145/32 触发权限恢复，不能恢复递归 `/r`、`/t`、继承授权或祖先重试整棵失败子树，不能以脱离后台线程冒充中止 ACL 修改。

## P44 未登记应用清理后的列表完成判定不能只认残留来源标签

Note: `2026-10-07-discovered-cleanup-list-completion.md`

- **症状**：Magpie 文件清理记录没有失败，程序文件与快捷方式确实消失，软件列表仍显示旧条目。
- **根因**：旧列表完成判定只认 `InstallDir` / `UninstallEntry` 标签；范围去重或重新扫描缺失文件后，标签不能代表原先发现的程序是否仍在。现场未持久化原始勾选，不能据此断言具体是哪条标签丢失。
- **防护**：未登记应用在清理后后台复核原发现证据的非空 `program_paths`，逐个只认 `NotFound`；任何仍在或未知都保留条目。所选失败项仍保留重试入口。
- **测试**：`discovered_cleanup_verifies_program_paths_without_source_labels`；红绿证据与决策见 Note。
- **禁止回退**：不能把清理返回成功直接当成应用消失，不能凭残留来源标签替代程序路径核验，不能扩大到 exe 的父目录树删除，不能放宽身份或路径保护。

## P45 venv 整树删除必须逐节点保护并绑定扫描期目录身份

Note: `2026-10-07-pip-package-and-venv-removal.md`

- **症状**：父目录不受保护，但白名单子目录被删；根 Junction 的目标文件被删；同名 cfg 的重建环境被旧扫描结果删掉。
- **根因**：局部递归只检查根保护，read_dir 穿透根重定向，cfg 存在不能证明对象没被替换。
- **防护**：复用 cleaner::delete_tree 的逐节点 safety，发现与删除前复核稳定身份并拒绝根/祖先 symlink 或 reparse。身份未知拒绝，原地内容变动不算替换。
- **测试**：review_venv_preserves_protected_children、review_venv_replaced_after_scan_is_refused、review_venv_junction_root_and_ancestor_are_refused，红绿证据见 Note。
- **禁止回退**：不能写第二条不查 safety 的递归删除，不能用 cfg/mtime 代替稳定对象身份，不能先 canonicalize 待删链接再授权目标树。

## P46 命令超时必须终止进程树，输出管道也受同一截止时间约束

Note: `2026-10-07-command-process-tree-deadline.md`

- **症状**：cmd 被杀后真实工具仍继续卸载；读取线程 join 等后代管道，超时实际无限等待。
- **根因**：仅 kill 直接子进程，父退出后无期限 read_to_end。
- **防护**：Windows 挂起启动、入 Job 后恢复，结束关闭 Job；Unix 新进程组。输出非阻塞轮询，父退出与两个 EOF 均受截止时间限制。
- **测试**：review_timeout_kills_shim_descendants_and_closes_pipes、review_exited_parent_does_not_bypass_the_pipe_deadline，含实际后代启动与延迟写入回执。
- **禁止回退**：不能退回只杀 cmd、普通启动后再挂 Job、无限 join 或脱离修改线程冒充取消。
