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
  - 进程表同样要守列边界：详情展开会压缩名称列，名称文本可收缩并截断，
    端口与分组数量不收缩，三种行的名称列都要裁剪；回归用例及验收状态见 Note。
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
  - 两个 helper 的脚本与 spawn 现由 `../gpui-app-kit/crates/update-installer`
    单点维护，`src/platform/*/updater.rs` 只做预检和缓存路径；改 kit 时同样受本条约束。
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

## P30 GPUI 死锁补丁与 `runtime_shaders` 不能删

Note: `2026-10-15-shared-gpui-kit.md`

补丁现由兄弟仓库 `../gpui-app-kit/vendor/gpui` 单点维护，两个应用的根 Cargo manifest 必须 patch 到同一来源。
不能只在依赖库 manifest 中声明 patch，也不能退回没有激活死锁修复的 registry 版本。`runtime_shaders` 免 Metal Toolchain，仍须保留。

## P31 孤儿残留（已卸载软件）只认 Bundle ID 名

Note: `2026-10-05-orphan-residual-bundle-id-rule.md`

家族判据和「测不出」都不能放宽。Spotlight 不包含所有内嵌应用或开发浏览器，
扫描和清理前都要补查系统登记的真实包身份；旧登记路径不存在不是确定证据。
轮转日志不能因带多个点就被当成 Bundle ID。归属置信度与默认选择分开，
精确应用身份可以确定，但容器、应用组和用户数据仍需手动确认；共享 SDK 名称不能
被断言为已卸载应用。回归与逐条证据见同一 Note。

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

## P47 日期记录不能拿 CI 所在时区的今天判断未来

Note: `2026-10-07-note-date-timezone.md`

- **症状**：台北当天写的 note，本地验证通过，UTC runner 报 filename date cannot be in the future。
- **根因**：文件名和 Archived 只有日期，没有时区；runner 的 date.today() 比作者落后一天。
- **防护**：两个未来日期检查共用 UTC+14 的当前民用日期；日期无效、归档早于记录和全球仍未到达的日期继续拒绝。回归测试冻结 UTC 时间覆盖这些边界。
- **禁止回退**：不能改回本地 date.today()，不能无条件允许明天、删掉未来检查，或把合法 note 改成昨天来绕过 CI。

## P48 Windows Deny Delete 也可能阻断目录枚举

Note: `2026-10-07-deny-acl-directory-enumeration.md`

- **症状**：直接拆 ACL 能删目录，但 cleaner 清理仍失败，子文件完整保留。
- **根因**：目录 metadata 成功而 read_dir 返回 error 5，原枚举失败出口先于删除阶段的 ACL 恢复。
- **防护**：安全与链接检查后，枚举只在 AccessDenied 时修复当前节点并重试一次；其他错误不改权限，失败继续拒绝。非提权回归以自有夹具锁定枚举分支，提权 CI 锁端到端删除。
- **禁止回退**：不能把枚举失败当空目录，不能恢复整树 ACL 或祖先重扫，不能跳过子项保护，也不能靠忽略提权测试维持 CI 绿。

## P49 输入法安装目录与显示名不能代替官方卸载和数据身份

Note: `2026-10-09-qingjian-input-method-uninstall.md`

- **症状**：青简在 macOS 软件列表缺失；Windows 官方卸载后 Qingjian 数据与日志不出现在残留列表。
- **根因**：输入法住在两处 Input Methods；官方卸载是脚本。Windows 显示名「青简」与英文数据目录不同，且触发短名称限制。
- **防护**：发现与安装存活复核都包含输入法根；官方脚本仅限已审查摘要和明确身份/位置，校验两份安装、冻结脚本字节、保留用户 HOME、不传 purge，并复核本体消失。Windows 别名需完整卸载登记身份，数据不默认勾选。
- **禁止回退**：不能仅凭 uninstall.sh 文件名执行；不能失败后退回删本体；不能降低全局短名称门槛或把安装目录名当数据归属。提权不能使用 root HOME，退出 0 不能代替卸载完成。

## P50 内部 GUI 卸载不能当成删本体，官方卸载的数据删除先于残留选择

Note: `2026-10-09-reviewed-script-uninstall.md`

- **症状**：水杉设置应用或输入法本体被单独删除，系统输入源留下，或设置应用再次安装输入法；Windows 数据在残留确认前已被官方卸载器删除。
- **根因**：内部 GUI 动作不是外部命令；程序退出、本体删除和卸载全部关联产物并不等价。残留勾选只控制我们自己的清理。
- **防护**：内部 GUI 身份由明确配置阻止直接删除并给出官方引导；官方卸载的数据影响在确认前提示。共享脚本执行器只接受审查过的身份、摘要、参数与删除范围，逐份验证完成。
- **身份模型**：macOS 枚举的普通 bundle 使用 `Hkcu`，不能误当成 `Unregistered`。回归必须先断言真实解析身份命中引导；只断言卸载返回错误，会被沙箱拒删掩盖。
- **禁止回退**：不能自动启动可能重装输入法的设置应用，不能用厂商或 Bundle ID 前缀跨 edition 匹配，不能把没有数据警告当成官方卸载器保证保留数据。


## P51 进程搜索浮层的可见性不能由查询是否为空决定

Note: `2026-10-11-status-search-popover-lifecycle.md`

- **症状**：输入 `mem` / `ms` 后搜索建议突然消失；点击外部非焦点控件后帮助仍停留。
- **根因**：查询类型混入可见性判定；非焦点控件的鼠标点击不会自动让输入框失焦。
- **防护与测试**：焦点和显式关闭控制生命周期，输入仅筛选建议；决策、deferred 点击边界与回归测试见 Note。
- **禁止回退**：不能恢复“普通非空查询自动关闭”；不能把 deferred 浮层内的建议点击当作外部点击；不能把任何冒号都当端口查询（用户查询也有冒号）。


## P52 搜索候选的键盘选择必须跟渲染列表一致

Note: `2026-10-11-status-popover-keyboard-navigation.md`

- **症状**：popover 中的候选只能点击，方向键不能切换，Enter 不能选择。
- **防护与测试**：共享候选来源，以稳定行身份保存选中项，上下循环并滚动端口列表；回归绑定与红绿证据见 Note。
- **禁止回退**：不能另建一份键盘候选目录；不能在筛选后保留越界索引；不能抢走 IME 组合输入的方向键和 Enter，也不能把焦点移出搜索框来选择建议。

## P53 浮层必须隔离后层鼠标命中，不能只停止事件冒泡

Note: `2026-10-11-popover-mouse-isolation.md`

- **症状**：鼠标停在 popover 内时，后层表格仍触发 hover。
- **防护与测试**：通用 surface 使用 GPUI `occlude()`，统一鼠标与滚轮边界；实际命中测试和红绿证据见 Note。
- **禁止回退**：不能用 `stop_propagation()` 代替命中隔离；不能阻断浮层自身滚动或吞掉外部目的控件点击。

## P54 搜索条件补全不能替用户填写阈值

Note: `2026-10-11-search-threshold-completion.md`

- **症状**：输入 mem 时接受候选直接变成 mem > 1 GB；紧凑比较符缺少建议，内存默认单位不明确。
- **防护与测试**：补全字段和比较符，数值由用户填写；说明默认 MB，解析与回归见 Note。
- **禁止回退**：不能把固定示例当作补全值，不能覆盖已填写的数值，不能移除无单位 MB 或把阈值解析复制到 UI。

## P55 本机工具链落后于 CI 的 stable 时，clippy / fmt 会「本地绿、CI 红」

- **症状**：`cargo clippy --all-targets -- -D warnings` 与 `cargo fmt --check` 在本机全绿，推送后 CI 立刻挂在 Clippy / Check formatting。
- **根因**：CI 用 `dtolnay/rust-toolchain@stable`，随上游前进；本机 rustup 可能停在旧版本。新 clippy 会新增 lint（0.0.10 这轮是 1.99 的 `question_mark`），旧 rustfmt 的换行决策也可能不同。
- **防护**：提交前用与 CI 同版本的 toolchain 复核，例如 `rustup toolchain install 1.99.0 --component clippy,rustfmt` 后跑 `cargo +1.99.0 clippy --all-targets -- -D warnings`；CI 报错里的行号与 lint 名是唯一权威。
- **禁止回退**：不能因为「本地过了」就断定 CI 会过；不能用 `#[allow]` 消掉新 lint，要按 lint 建议改写（这轮把比较符前缀拆成 `split_cmp_op`，行为不变）。

## P56 macOS 端口/线程枚举必须走 libproc，空表 stub 让 `:port` 搜索静默失效

Note: `2026-10-12-macos-process-ports-map.md`

- **症状**：状态监控页进程搜索输入 `:3000` 在 macOS 上永远检索不到占用端口的进程，线程数列恒为 1；Windows 同一语法正常。没有任何报错，看起来像「没有进程占用」。
- **根因**：`process_ports_map` / `process_threads_map` 的平台契约是「返回 HashMap」，旧 macOS 实现是返回空表的占位 stub——类型满足契约，UI 层无从分辨「没权限/没数据」和「根本没实现」。
- **防护**：
  - [`process_ports_map`](../src/platform/macos/status.rs) 用 libproc 枚举：`proc_listallpids` → `proc_pidinfo(PROC_PIDLISTFDS)` → `proc_pidfdinfo(PROC_PIDFDSOCKETINFO)` 取 `insi_lport`；`process_threads_map` 用 `proc_pidinfo(PROC_PIDTASKINFO)`。
  - `socket_fdinfo` 的手工镜像结构由 `const` 断言钉死尺寸与关键偏移（SDK 实测 792B、`insi_lport`@268），ABI 变动会编不过。
  - 测试：`process_ports_map_lists_own_listener`、`process_threads_map_counts_own_threads`（本进程自证，不需要外部进程配合）。
- **改这两处或 `ProcInfo.ports`/`threads` 的消费方时**：
  - 禁止退回「返回空 HashMap」的写法——任何「不支持」也要靠空字段语义区分，不能拿类型满足当实现完成。
  - `insi_lport` 是网络序 int，截 16 位后必须 `u16::from_be`，直接 `as u16` 会得到字节序反掉的端口。
  - 非本用户进程 `proc_pidfdinfo` 返回 EPERM 是常态（同 lsof 无特权口径），跳过即可，不要当成「枚举失败」整体返回空。

## P57 「强制退出」不能复用发 SIGTERM 的通道，SIGTERM 可被目标忽略

Note: `2026-10-12-macos-force-terminate-sigkill.md`

- **症状**：进程详情里「退出」和「强制退出」对同一个 python 服务都点了没反应，两个按钮表现完全一致。
- **根因**：`force_kill_process` 与 `kill_process` 共用 `terminate_process`，macOS 侧只发 `SIGTERM`。SIGTERM 是协作信号，目标注册 handler 或直接 `trap '' TERM` 就免疫；「强制退出」语义上必须是 SIGKILL，但契约里根本没有强杀入口，UI 层无从选择。
- **防护**：
  - 平台契约新增 [`force_terminate_process`](../src/platform/macos/status.rs)：`ensure_same_process` 共享 uniqueid 身份闸门后发 `SIGKILL`；Windows 侧委托 `TerminateProcess`（本来即强杀）。
  - `ui/actions/status.rs` 的 `force_kill_process` 走 `force_terminate_process`，「退出」仍走 `SIGTERM`。
  - 测试：`force_terminate_kills_process_that_ignores_sigterm`（`trap '' TERM` 的子进程，SIGTERM 投递后仍存活，SIGKILL 后 `status.signal()==9`）。
- **改结束进程链路时**：
  - 禁止把 `force_kill_process` 改回 `terminate_process` 或任何「复用同一条信号」的写法。
  - 新增平台实现时 Windows 侧委托 `terminate_process` 是允许的（无信号语义差异），macOS/Unix 侧必须发 `SIGKILL`。
  - 强杀前的 uniqueid 双复核不能省（macOS 无 pidfd，TOCTOU 只能靠它收窄）。

## P58 本地化守卫不能按某一种写法匹配，基线为 0 不代表没有内联文案

Note: `2026-10-12-ui-inline-cjk-guard.md`

- **症状**：守卫一直报 `remaining baseline: 0`，英文界面却在开发环境批量移除、相似照片、磁盘面包屑、残留弹窗等处显示中文。
- **根因**：旧守卫只匹配 `Language::Zh => "`。`if lang == Language::Zh`、元组 `match`、`(View, Language::Zh) =>`、带花括号的 `bilingual` 分支，以及完全没有英文分支的硬编码全部漏网，一共 89 处。
- **防护**：
  - [`check-ui-localization.py`](../scripts/check-ui-localization.py) 统计 i18n 之外 UI 代码里含中文或全角标点的字符串字面量；注释、测试模块、日志 / 断言宏和 `.expect` 的参数不算。
  - 测试：`scripts/test_ui_localization.py`（每种已知写法各一例）、`english_copy_has_no_cjk_and_keeps_every_count`。
- **改 UI 文案或这个守卫时**：
  - 不要把「剔除诊断调用」扩到状态栏、弹窗用的 `bilingual` / `Text::new`，它们是界面文案。
  - 不要为了让守卫变绿往基线里记例外；基线必须保持为空。
  - `core` / `platform` 的错误 payload 保持语言中立，中文外壳放在 `tr_scan_error` 一类的 `tr_*` 里。

## P59 搜索框的退格 / 删除只能由 `search_box` 组件实现，不能交回调用方

Note: `2026-10-12-search-box-backspace.md`

- **症状**：开发环境搜索框按 Backspace 删不掉文字（光标在末尾时无反应，在中间时删掉右边的字）。
- **根因**：`search_box` 曾让每个调用方传 `on_backspace`，四个搜索框四种写法；开发环境页把退格接到了实现 Delete 键的 `text_input_delete`（它的布尔参数选的是哪个输入框，不是删除方向）。
- **防护**：
  - [`search_box`](../src/ui/components/search_input.rs) 不再接受退格回调，Backspace / Delete 在组件内统一走 `delete_backward` / `delete_forward` → `binding.changed`。
  - 测试：`backspace_removes_before_caret_and_delete_after`（末尾、中间、多字节字符）。
- **改搜索框或新增输入框时**：
  - 不要重新加 `on_backspace` / `on_delete` 之类的编辑回调；页面差异放进 binding 的 `on_change`。
  - 接编辑回调时按函数实现判断删除方向，不要按函数名或布尔参数猜。

## P60 Spotlight 的单个缺失 ID 先精确补查，未知身份仍整轮拒绝

Note: `2026-10-14-spotlight-bundle-id-fallback.md`

- **症状**：索引正常但一个真实应用的 mdls ID 为 (null)，整轮孤儿残留扫描不可用。
- **根因**：将索引字段缺失直接当成应用身份无法读取，漏掉其可读 Info.plist。
- **防护与测试**：按路径配对缺项并有界读取对应 plist；恢复和未知拒绝测试见 Note。
- **禁止回退**：不能跳过未知应用返回部分 ID，不能放宽 P31；返回个数不匹配或补查失败仍拒绝。

## P61 搜索选区不能排序后丢掉锚点与光标方向

Note: `2026-10-14-directed-search-selection.md`

- **症状**：Shift 反向方向键扩大选区，不能缩回；反向鼠标选区后键盘行为不正确。
- **根因**：仅保留排序范围，失去固定锚点与活动光标的区别。
- **防护与测试**：sel 保存 anchor..caret；绘制/切片单独规范化，原生 UTF-16 接口保留 reversed；真实键鼠事件及组合输入测试见 Note。
- **禁止回退**：不能对状态 sel 排序；不能用有向 sel 直接切片；不能只改键盘而漏掉鼠标和输入法方向。


## P62 增量索引不能每改一个文件就清空整张目录查找缓存

Note: `2026-10-15-index-batch-directory-cache.md`

- **症状**：启动或扫描长时间转圈；现场 2 万余文件变更的就地处理耗时 117 秒，后续千余目录重扫仅 1.19 秒。
- **根因**：每次更新、移除或追加节点都清空全局子名缓存，下一条路径查找反复重建宽目录索引；已有文件还被无谓地替换成新节点。
- **防护与测试**：文件元数据原槽更新；墓碑命中复核并按名字修复；追加只维护父目录。缓存复用、同名别名、递归替换回归与批量计时证据由 Note 单点维护。
- **禁止回退**：不能在逐文件热路径恢复全表失效，也不能用删除有效性检查换性能；不能只把全表失效缩成“每次清父目录”后就声称同目录批量变更已解决。计时门槛不能替代确定性的缓存复用和正确性断言。


## P63 托盘浮窗不能在自己的事件回调里再次更新自身窗口

Note: `2026-10-15-tray-popover-render-lifecycle.md`

- **症状**：关闭按钮无效；主窗口被旧浮窗遮挡；底部按钮被固定高度裁掉。
- **根因**：GPUI 分发中当前窗口暂时不在窗口表，再次 WindowHandle.update 返回 window not found；holder 已取走却没关窗。内容区没有高度和滚动约束。
- **防护与测试**：窗口切换回调整体 defer；固定顶栏、硬件区、底栏，磁盘与账户受限滚动。多卷中英文测试必须检查 CPU / 内存仍在首屏，而不能只检查底栏。真实鼠标分发与布局红绿证据见 Note；耗时采样交给后台 executor。
- **禁止回退**：不能在事件内部再次更新当前窗口，不能忽略关闭失败后丢掉 handle；不能拿延时替代后台执行，不能只增加高度掩盖内容溢出。启动 / Dock reopen 的裸 App 没有自动效果周期，程序化切窗要通过前台任务进入 update，不能只排 defer 等待下一次窗口事件。
- **主窗口生命周期**：Cmd-W / Alt-F4 只关闭窗口，Quit 才退出；macOS 主窗口关闭后 Accessory、重开前 Regular。Windows GPUI 的最后一个受管窗口关闭会退出消息循环，托盘成功注册后必须保留隐藏控制窗口，不能误认为自建托盘 HWND 就能阻止退出。

## P64 空间透镜必须轻量轮询，APFS 根快照不能重复显示容器容量

Note: `2026-10-15-tray-live-overview.md`

- **症状**：浮窗读数一直不变、首批数据迟到；Recovery 与 Macintosh HD 显示同一份容量；打开主界面与扫描入口行为重叠。
- **根因**：一次性全量状态采样串行等待目录统计；根快照设备只剥一层后缀；窗口构建和引导关闭隐式启动扫描。
- **防护与测试**：轻量持久采样器、独立账户读取、窗口存活检查、显式扫描入口、设备身份归组；定时与关闭、引导关闭、无进程表、APFS 快照的测试和红绿证据见 Note。
- **禁止回退**：不能每拍重建差值采样器；不能在托盘读数中恢复全系统进程/端口或主目录遍历；不能仅在贴回结果时检查实体；不能按容量相等去重；不能把打开窗口重新接成自动扫描。

## P65 孤儿扫描覆盖不等于卸载前覆盖，brew cleanup 不会注销应用安装记录

Note: `2026-10-16-webkit-homebrew-uninstall.md`

- **症状**：Ghostty 卸载后仍留下 `~/Library/WebKit/com.mitchellh.ghostty`；直接删除应用本体也留下 Homebrew cask 记录。
- **根因**：WebKit 只在孤儿扫描表，按应用扫描使用另一张表；通用 Homebrew cleanup 只清缓存和旧版本，不能代替 cask uninstall。
- **防护与测试**：按应用的 WebKit 精确 Bundle ID 匹配、Homebrew receipt 记录的 app target 归属、失败禁止裸删回退，以及本体和登记双重完成核验，见 Note。
- **禁止回退**：不能用应用名猜 cask、用家族前缀扩大 WebKit 所有权、直接删 Caskroom、加 `--zap` 提前清用户数据，或在命令失败/未知时退回废纸篓；一个 cask 包含多个独立 App 时不能从单选卸载连带移除。

## P66 全局工具和应用内置依赖不能因目录名像构建产物就整目录清理

Note: `2026-10-20-build-installation-boundary.md`

- **症状**：nvm 的全局 node_modules（含 npm 与 CLI）进入构建垃圾列表；正在运行的 OpenCode 使其暂时被占用保护拦下。
- **根因**：只按 node_modules 名字识别，没有区分项目依赖与安装资产；停止占用者后原本不该删除的安装目录就可能被整删。
- **防护与测试**：安装归属由 core safety 判断，三条发现通道共用候选验证；项目清单、环境凭据及 CACHEDIR.TAG 不覆盖安装归属。回归与红绿证据见 Note。
- **禁止回退**：不能只增加 package.json 旁证后就放行全局包，不能递归把未验证依赖树里的子目录当项目，不能用结束进程绕过分类问题；全局工具仍走开发环境的生态卸载通道。

## P67 Windows 主窗口在第一帧呈现前不能显示

Note: `2026-10-24-startup-white-window.md`

- **症状**：启动后先闪一个带标题栏的白窗口，客户区全白，然后主界面才画出来。
- **根因**：`NewWindow` 在第一帧呈现前就 `ShowWindow`。`MyGoWindow` 客户区没有背景刷，表面子窗口盖住客户区且擦除时不填色，DWM 先合成白底。只设 `BackgroundColor` 填不到这块白。
- **防护**：Windows 上先隐藏窗口，用 `DWMWA_CLOAK` 罩住后再显示，让表面能画；`OnReadyToShow`（第一帧已呈现）里 `DwmFlush` 后取消罩。两秒没有第一帧仍显示，避免永远看不见。回归与红绿证据见 Note。
- **禁止回退**：不能去掉启动时的 `Hidden` 或 cloak-before-show；不能把 GUI 子系统的提权显示改成 `SW_HIDE`（那会把真窗口一起藏住）；不能只靠背景色认为白闪没了；隐藏着等 `OnReadyToShow` 而不先显示被罩住的窗口，内容窗口可能永远不画。

## P68 控制台子系统提权不能弹出标题为 exe 路径的终端窗口

Note: `2026-10-24-elevation-console-window.md`

- **症状**：`go run` 或以管理员重启时，先闪一个小窗口，标题是 `go-build` 临时 exe 的完整路径，然后主窗口才出现。
- **根因**：程序是控制台子系统。`ShellExecute` 提权的显示值是正常显示，Windows 终端会在进程代码运行前打开这个窗口。
- **防护**：只有控制台子系统（值为 3）提权时用隐藏显示。GUI 子系统保持正常显示。回归见 Note。
- **禁止回退**：不能把控制台提权改回显示值 1；不能让 GUI 子系统也用隐藏显示。

## P69 索引发现不能对每个目录打开 CACHEDIR.TAG

Note: `2026-10-24-indexed-cachedir-name.md`

- **症状**：提权后的垃圾扫描，MFT 解析只要几秒，发现式扫描却要两分钟。Rust 同样两卷大约 5 秒。
- **根因**：索引通道对每个目录都 `CreateFile` 一次 `CACHEDIR.TAG`。C: 约 53 万个目录、D: 约 22 万个，失败的打开加在一起就是这两分钟。
- **防护**：树上没有这个文件名就不打开。遍历通道没有树，仍然读盘验签名。回归见 Note。
- **禁止回退**：不能为了“树上可能过期”而在索引通道对每个目录打开 `CACHEDIR.TAG`；不能因为文件名在就跳过签名校验。

## P70 遍历发现不能对每个目录先做祖先链约束再打开 CACHEDIR.TAG

Note: `2026-10-24-walk-discovery-probes.md`

- **症状**：未提权的垃圾扫描，同一批代码根 Rust 11–14 秒，Go 51–66 秒，结果完全一样（641 条）。
- **根因**：没命中的目录都先对不存在的 `CACHEDIR.TAG` 做 `ConfinedRuleFile`（整条祖先链 Lstat + EvalSymlinks），失败后才打开；每进一层目录还不带缓存地重新约束一次自己；收集和称重全部串行。
- **防护**：先打开，打开成功才约束，并要求句柄与约束后的路径是同一个文件；单根内祖先证据缓存，命中在称重前不带缓存再约束一次；根之间并行，命中共享一份扇出预算称重。回归与红绿证据见 Note。
- **禁止回退**：不能为了“更安全”把顺序改回先约束后打开；不能去掉命中前的无缓存约束；不能让每个并行称重各自开 GOMAXPROCS 个目录协程。

## P71 MFT 记录数不能用固定上限判损坏

Note: `2026-10-24-mft-record-cap.md`

- **症状**：记录数超过 1600 万的大 NTFS 卷整卷扫描失败，报 `invalid NTFS MFT metadata`。Rust 正常。
- **根因**：为防损坏的有效长度触发超大预分配，Go 用固定 16M 条判损坏；Rust 读到有效长度和 run 实际字节数中较小的那个为止。
- **防护**：记录数取 `min(有效长度, run 字节数) / 记录大小`，只保留 uint32 上限；`Entries` 按块增长，损坏的 run 在第一块读失败时就停，不会先按声称大小分配。回归见 Note。
- **禁止回退**：不能重新加任何固定记录数上限；不能改回按记录数一次性预分配全部 `Entries`。

## P72 增量索引落盘不能再整树校验、整 base 解码

Note: `2026-10-24-override-only-persist.md`

- **症状**：macOS 索引每次 FSEvents 刷新落盘，200 万节点要 2.85 秒、分配 1.29 GB；Rust 只看 overrides。
- **根因**：映射树的改动只在 `overrides` 和追加节点里，但增量落盘仍然重新 load、再整份读入解码 base、两遍全树 `validateDiskTree`、全树计数；映射树每次 `slot()` 都要解码一个名字。
- **防护**：`persistOverrides` 在全树校验之前运行，只校验和编码改动部分，计数从校验过的 base 头部增量调整；前提（同一 base、覆盖信息有效、delta 不更新、身份未变、父节点向前）任一不确定就走完整路径。两条路径共用 `writeDiskIndexDelta`，测试锁定字节一致。回归见 Note。
- **禁止回退**：不能把 `validateDiskTree` 挪回快路径之前；不能为了“更稳”在快路径里重新读入或解码整个 base；不能让快路径在前提不确定时自行判定失败而不交给完整路径。

## P73 发布资产名、包内结构与 Windows 资源不能随构建方式改动

- **症状**：一旦改名或改包内结构，已安装用户的更新器找不到资产或载荷，静默停在旧版本；exe 少了资源，资源管理器、任务栏和窗口都退回默认图标，版本信息也是空的。
- **根因**：Rust 与 Go 两套更新器都按固定资产名（`quick-cleaner-windows-x86_64.zip`、`quick-cleaner-macos-{universal,aarch64,x86_64}.zip` 及 `.sha256`）和 zip 内的 `quick-cleaner.exe` / `QuickCleaner.app` 认领更新。Go 没有 build.rs，图标、版本信息和清单全靠 `cmd/quick-cleaner/rsrc_windows_amd64.syso`；MyGo 的窗口类加载 ID 为 1 的图标资源。
- **防护**：`scripts/package-windows.ps1` / `scripts/package-macos.sh` 保持 Rust 时期的名字、结构、bundle id 与图标；`tools/winres` 按 `internal/app` 的 `Version` 生成与 Rust `winres` 相同的资源，CI 与 release 都跑 `go run ./tools/winres -check`，release 还校验 tag 与 `Version` 一致。测试见 `tools/winres/main_test.go`。
- **禁止回退**：不能改资产名、zip 内结构、bundle id 或可执行文件名；不能删除或不再生成 `.syso`；改版本号后不能跳过 `go run ./tools/winres`；不能换成 mygo 自带清单（缺 `asInvoker` 与 `longPathAware`）。

## P74 托盘存在时，退出不能被“关闭即隐藏到托盘”吞掉

Note: `2026-10-24-tray-quit.md`

- **症状**：托盘菜单的“退出”、托盘面板里的退出按钮和 macOS 的 Cmd+Q 全都没反应，进程一直在。
- **根因**：MyGo 的 `App.Quit` 先发 `OnBeforeQuit`，再逐个关闭窗口，任一 `OnClose` 拦截就放弃整个退出；主窗口的 `OnClose` 在托盘存在时总是拦截并隐藏。
- **防护**：`onBeforeQuit` 放行后设 `quitting`，`onMainClose` 只在未退出时隐藏到托盘；有确认框、修改操作或正在安装更新时两处都拒绝并取消（共用 `quitBlocked`）。回归见 Note。
- **禁止回退**：不能去掉 `!r.quitting` 判断；不能改用 `App.Exit` 绕过退出事件；不能只在托盘回调里设标记（Cmd+Q 与 `RoleQuit` 直接调 `App.Quit`）。

## P75 加载动画不能在构建视图时读时钟

Note: `2026-10-25-loading-animation-frames.md`

- **症状**：扫描时顶部进度条一顿一顿，MFT 解析期间整段停住约 1.3 秒；鼠标一动它才动。
- **根因**：在构建视图时用 `c.Now()` 算位置不会向 MyGo 请求下一帧，只有鼠标、状态采样或后台结果触发重绘时才挪一下；主线程其实是空闲的。
- **防护**：`internal/ui/indicators.go` 的 `scanBar` / `loadingSpinner` 在 `Draw` 里读 `Painter.Now()` 并调用 `Painter.AnimationFrame()`，只重绘、不重建页面；测试用 `ui.Tester` 断言持续请求帧且画面在变。回归见 Note。
- **禁止回退**：不能在构建函数里用 `c.Now()` 驱动持续动画；不能为了动起来改成每帧 `c.AnimationFrame()` 重建整页；概览环形扫描的圆点也走 `orbitDot`（见 `2026-10-25-overview-orbit-paint-only.md`），同样不能改回 `c.After`；新增加载指示器复用这些函数，不要再用内置 12 辐条 `ui.Spinner`。

## P76 状态采样只在状态页可见时运行

Note: `2026-10-25-status-sampling-visible-page.md`

- **症状**：程序常驻托盘或停在别的页面时，仍每 2 秒枚举全部进程、读取硬件并让窗口重绘；点“暂停”也只是不显示。
- **根因**：Go 移植把 Rust“离开状态页即退出”的采样任务改成了常驻循环。
- **防护**：`syncStatusSampling` 按“状态页 + 窗口可见 + 未暂停”维护 `statusWanted`，`runStatusLoop` 不需要时跳过计时，变为需要时立即采样，手动刷新始终采样。回归见 Note。
- **禁止回退**：不能让采样循环无条件按计时执行；不能用“暂停”只丢弃结果代替停止采样；新增使用采样结果的页面要同时更新 `syncStatusSampling` 的条件。

## P77 主线程合并扫描结果时不能访问文件系统

Note: `2026-10-25-discovered-merge-off-main.md`

- **症状**：扫描结束那一刻界面卡顿约 260ms。
- **根因**：`MergeDiscovered` 在主线程上对 1300 多个发现路径逐个 `os.Stat`，并在内层循环反复规范化路径。
- **防护**：worker 先调 `DropVanishedDiscovered`；`MergeDiscovered` 只做内存里的白名单与重叠判断。回归见 Note。
- **禁止回退**：不能把 `os.Stat` 等文件系统调用放回主线程的合并或任何 `publish` 回调里。

## P78 Windows 单实例必须在提权前检测，且跨完整性级别可见

Note: `2026-10-25-single-instance.md`

- **症状**：重复打开会多开；或者每次重复打开都先弹 UAC；或者普通权限的启动看不到管理员实例，又开出一个。
- **根因**：管理员进程创建的内核对象默认带 High 完整性标签，普通进程无法打开或写入；MyGo 的单实例套接字放在临时目录，提权到另一个管理员账户时目录也不同；检测放在提权之后就会先弹 UAC。
- **防护**：会话级 `Local\` 互斥量和唤醒事件，SDDL 授予已认证用户同步与发信号权限，并把完整性标签降为 Medium；句柄不可继承；`main` 在 `RelaunchAsAdminIfNeeded` 之前调用 `InstanceRunning`，提权后再 `AcquireInstance`，抢锁失败也只唤醒。回归见 Note。
- **禁止回退**：不能在 Windows 上改用 MyGo 的套接字锁；不能去掉 SDDL 里的 `S:(ML;;NW;;;ME)`；不能把检测挪到提权之后；不能让互斥量句柄可继承；不能改成 `Global\\`。

## P79 路径断言不能比较 8.3 短名与长名

Note: `2026-10-25-shortcut-83-name.md`

- **症状**：`TestAppShortcutNativeReadOnly` 在启用 8.3 短名的卷上必红，失败信息里 `ok=true` 但路径被判不等；同一份代码在 windows-latest 上却是绿的，于是本地红灯容易被当成环境噪声忽略。
- **根因**：`t.TempDir()` 返回 `MEIMIN~1` 形式的短名（本机 `D:\code\QUICK-~1\...`），而 `IShellLink::GetPath` 与 `CanonicalAppPath`（`GetFinalPathNameByHandle`）返回长名，`strings.EqualFold` 比较两者永远不等。
- **防护**：测试改用 `sameAppPath`，两侧都过 `CanonicalAppPath` 后再比，取不到 canonical 形式时退回字面比较；`8.3 short path` 子用例用 `windows.GetShortPathName` 显式构造短名场景。回归见 Note。
- **禁止回退**：不能把 `sameAppPath` 改回直接 `strings.EqualFold` 比较原路径；不能因为 CI 不红就删掉 `8.3 short path` 子用例；不能改 `CanonicalAppPath` 去返回短名形式（会把卷设置泄漏进应用清单匹配）。

## P80 卸载进行中必须整页替换应用视图

Note: `2026-10-25-uninstall-page-replaces-apps-view.md`

- **症状**：卸载应用时进度页像是浮在列表上，底下卡片、标签页与统计从它后面透出来，和 Rust 的卸载过程页完全不是一个东西。
- **根因**：Go 端把 `uninstallProgress` 插在应用页面列里、其余内容照常渲染；Rust 是整页替换（`View::Apps if self.residual.uninstall.is_some() => render_uninstall_progress(self)`），且卸载页自带 CARD 背景与 40px 内边距。
- **防护**：`AppsPage.RenderContent` 在 `Uninstalling` 时只渲染卸载页并早退；布局对齐 Rust（560px 居中列、64px 应用徽章、6px 进度条、36px 阶段行、keep-open 脚注）。回归见 Note。
- **禁止回退**：不能把卸载页改回与列表同页渲染（会重新出现透底和卸载中仍可点的列表按钮）；不能去掉整页 CARD 背景或 40px 内边距；不能把进度条改回 8px。

## P81 应用名带版本号时，残留模糊匹配仍要认出短目录名

Note: `2026-10-25-residual-versioned-display-name.md`

- **症状**：卸载后残留页显示「0 项关联残留」，但 `%APPDATA%\<产品名>` 明明还在；换成不带版本号的同类应用就能扫到。
- **根因**：`Name` 取 Windows 的 `DisplayName`，常把版本写进名字（`MiniMax Code 3.0.74` → `minimaxcode3074`，15 字符）；候选目录 `MiniMax`（7 字符）走「应用名包含候选」的反向包含规则，要求 `7*2 >= 15`，不成立即被拒。Rust 的 `norm`/`name_matches` 同源，所以两边都有这个盲区。
- **防护**：`windowsResidualShortName` 剥掉尾部纯数字版本段（含 `v` 前缀）后再匹配，精确目录也再试一次短名 token；剥完不足 3 字符或本身就是通用名（`App 2` → `App`）则保留原名。回归见 Note。
- **禁止回退**：不能为了命中短名就把反向包含的比例（`size*2 >= 名字长度`）整体放宽；不能改成按发布者目录匹配（`Google`/`Tencent` 会跨产品误报）；不能只改 Rust 或只改 Go 让两个版本分叉。

## P82 只有菜单能改的设置必须有页面入口，应用菜单只留 macOS

Note: `2026-10-25-auto-check-toggle-in-sidebar.md`

- **症状**：Windows 原生标题栏下面又出现一条写着「QuickCleaner」的菜单栏；一旦去掉菜单，「自动检查更新」就再也没有入口。
- **根因**：`installMenu` 在所有平台都装了 macOS 风格的应用菜单（`Label: "QuickCleaner"`），MyGo 在 Windows/Linux 把它画进客户区；而这个菜单同时是该设置唯一的修改途径。
- **防护**：自动检查更新放进侧栏底部的常驻设置区（语言、版本旁），动作写 settings 并同步控制器；`installMenu` 只在 `darwin` 执行，菜单里不再保留已移到页面的开关；`AutoHideMenuBar` 随之撤掉。回归见 Note。
- **禁止回退**：不能把应用菜单重新装回 Windows/Linux（会再出现重复标题栏）；不能让同一设置同时存在于菜单和页面（两个来源会漂移）；不能只删菜单而不给页面入口；`删除到回收站` 的唯一入口是磁盘透视页的 `disk-recycle` 复选框。

## P83 清理之后的列表要按失败清单剪枝，不能整页重扫

Note: `2026-10-25-declutter-prune-and-residual-confirm.md`

- **症状**：冗余整理清理失败后，刚提示失败的那些条目又作为「新结果」整批出现在列表里，看起来像同一次清理把清理项显示了两遍。
- **根因**：收尾回调是 `Scanned = false; scanDeclutter()`，清理结束就整页重扫；重扫只认磁盘现状，分不出「刚才失败」和「本来就在」。
- **防护**：`pruneDeclutter` 只摘掉真正移走的行，判定要求三条同时成立——已勾选、报告没失败、路径确实不在了；分组页签在剩不到 2 项时整组移除。回归见 Note。
- **禁止回退**：不能在清理后改回整页重扫；不能只按 `CleanReport.Failed` 摘除而不核验路径（报告说成功但文件还在时会把仍在盘上的条目从界面抹掉）；不能把未勾选的行一起删掉。

## P84 残留清理不需要二次确认，但安全核验不能跟着删

Note: `2026-10-25-declutter-prune-and-residual-confirm.md`

- **症状**：勾选残留项、点清理，还要再点一次确认框里的「确认永久删除」；Rust 版本没有这一步。
- **根因**：Go 端在 `requestResidualClean` 里加了 `r.ask("清理选中的残留？")`，而 Rust 的 `clean_selected_residuals` 直接执行——勾选并按下按钮就是显式同意。
- **防护**：去掉确认框，直接进入清理；归属、占用、撤离证据的核验与「未知归属整批拒绝」保持原样。回归见 Note。
- **禁止回退**：不能把确认框加回来（与参照不一致且被用户明确否掉）；不能因为去掉确认框就跳过 `residualAuthority`/`orphanAbsent` 的核验或允许部分删除；未知归属时仍必须整批拒绝并如实报告未清理。

## P85 清理残留后不能自动重开残留面板

Note: `2026-10-25-residual-clean-keeps-review-closed.md`

- **症状**：清理残留后，面板又弹一次——全清掉时显示「0 项关联残留」，有失败项时重开的还是那批删不掉的条目，看起来像「点一次弹一次」。
- **根因**：`requestResidualClean` 的收尾无条件调用 `showResiduals(result, app)` 重开面板；Rust 在同样位置写的是 `this.residual.result = None`，注释说明「删不掉的多半会一直删不掉」。
- **防护**：收尾改为关闭面板（`ResidualsOpen=false`、`Residuals=nil`、尺寸与 `Occupancy` 清空），只把幸存条目留在 `r.residual.Items`；计数交给 `finishCleanup` 的提示与历史记录。回归见 Note。
- **禁止回退**：不能在清理后重新打开残留面板；不能只在有剩余项时才重开（同样是点一次弹一次）；不能把剩余项合并进下次扫描结果；卸载流程里「卸载后立刻看残留」的那次展示不属于本约束。
