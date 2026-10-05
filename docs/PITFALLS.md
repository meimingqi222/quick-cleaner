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
  LAPTOP-…\meimingqi222  Deny  DeleteSubdirectoriesAndFiles, Delete
  ```
  手动 `Remove-Item` 报 Access denied。清理日志里是
  `拒绝访问 (os error 5)`，不是 `os error 32`（句柄占用）。
- **根因**：应用用 DACL Deny 做防删，进程退了 ACL 还在。以前的删除路径
  只试一次 `remove_file` / `remove_dir`，拿到 PermissionDenied 就记失败
  放弃。**提权进程本来可以 takeown + icacls 拆掉 Deny 再删**——没有进程
  占用时这不是「系统不让删」，是我们没走完该走的步骤。
- **防护**：
  - [`force_delete_access`](../src/platform/windows/security.rs)：takeown
    `/a /r` + **`icacls /remove:d`（当前用户 SID + Everyone）** +
    `icacls /grant *S-1-5-32-544:(OI)(CI)F /t /c /q`。用 SID 不用组名
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

## P18 规则缓存读失败不能重置防回放水位

Note: `2026-10-05-rule-state-replay-watermark.md`

- **根因**：损坏状态被当成首次安装，允许旧签名包重新启用。
- **防护**：只在没有状态文件时初始化；损坏或读失败拒绝更新，保留最高已接受序号。回退不降低水位，切换后保留失败不能撤销已接受状态。
- **测试**：`corrupt_state_never_resets_replay_protection`、`interrupted_publication_retries_and_corrupt_updates_leave_active_state`。

## P19 相同路径去重必须合并全部约束

Note: `2026-10-05-duplicate-rule-constraints.md`

- **根因**：只取第一条会丢保留项或隐藏处置冲突；UI 合并不能保护 core 的其他调用者。
- **防护**：先在 core 合并，再执行；推荐取交集，保留项和归属证据取并集。处置、身份或快照冲突阻止目标，不能按加载顺序决定。Unix 仍用 dev/ino 复核，不加 mtime/len 相等约束。
- **测试**：`duplicate_scan_policies_cannot_silently_select_a_deletion_method`、`core_duplicate_merge_preserves_every_rule_in_both_orders`。

## P20 已尝试生态命令后，超时不能回退裸删

Note: `2026-10-05-owner-command-timeout.md`

- **根因**：Go/pnpm 变更命令的 `?` 将超时传成 None，cleaner 把它当成 owner 不适用，继续删未知状态的 store。
- **防护**：None 只用于执行前缺工具或范围失配；已尝试清理必须返回 Some(false) 阻止回退。完成按原生资源状态核验，读失败不能当不存在。
- **测试**：`attempted_owner_timeout_never_grants_filesystem_fallback`、`owner_completion_checks_resource_state_after_zero_exit`、`remove_verifies_exact_reference_and_rejects_unknown_inventory`。

## P21 外置运行环境与恢复记录不能作为同一步树删除

Note: `2026-10-05-install-record-last.md`、`2026-10-05-capability-plan-lifecycles.md`

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
