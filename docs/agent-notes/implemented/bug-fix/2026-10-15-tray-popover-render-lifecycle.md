# Agent Note: 托盘浮窗的窗口重入与固定高度内容溢出

Status: implemented

## Problem

`src/ui/views/tray_popover.rs` 的关闭、打开主界面和快速扫描在当前窗口鼠标分发中
调用 `src/main.rs` 的关闭回调。该回调再次通过 WindowHandle 更新同一窗口，GPUI
此时已取出当前窗口，返回 window not found；holder 却已被 take，浮窗留在前景且失去跟踪。

430px 固定窗口在两卷、三个账户时，内容将底部操作栏推到 568px。延后 15ms 再调用
refresh_data 仍是在 UI 线程同步采样，不会因此成为后台任务。

## Decision

`src/ui/tray_host.rs` 独占窗口句柄、焦点、Esc、失活和关闭处理。
内容按钮只通过 TraySession 发业务事件，宿主将整个关窗与动作分发交给 cx.defer，
保证先退出当前窗口分发，再关闭浮窗，然后激活主窗口或启动扫描。
不要只将句柄清理延后而让主窗口先打开。每次打开独立会话，旧事件不能清理新会话。
启动及 Dock reopen 收到的是裸 App，不会自动 flush defer 队列。程序化 dismiss_then
通过前台 task 的 AsyncApp.update 执行整个切窗事务，不能只排 defer 并等待不存在的窗口事件。

顶栏、硬件区与底栏不收缩；磁盘和账户列表在中间使用 flex_1、min_h(0)、
overflow_y_scroll，将多余内容留在滚动区。硬件区固定在操作栏上方，不能放进磁盘列表
后面的滚动内容里：多卷时虽未溢出底栏，CPU / 内存卡片却已经在首屏之外。
`src/ui/tray_overview.rs` 持有采样与维护任务，通过 background_executor 执行，
将结果贴回模型；内容视图只观察模型。

macOS 状态项从所属 NSScreen 取得 display ID，将 AppKit 逻辑点转换为该屏幕左上角
坐标，交给同一个 GPUI display；不乘 Retina 比例，不写死 y=30。点击只监听左键松开，
移除会吞掉正常快速点击的 400ms 防抖。

### 主窗口与进程生命周期

`src/ui/desktop.rs` 将关闭窗口与 Quit 分开：macOS 的 Cmd-W、Windows 的 Alt-F4
走 CloseWindow，显式 Quit 才结束应用。原生标题栏关闭与快捷键关闭共用收尾逻辑。
主窗口关闭后 macOS 切换 Accessory，重开前恢复 Regular；仅打开托盘浮窗不会恢复 Dock。
`../gpui-app-kit/crates/app-host/src/desktop.rs` 的策略切换在主线程调用。Finder/Dock reopen 重新打开主窗口。
根视图由 main 独立持有，重开复用，因此扫描结果和后台调度器不会随窗口销毁而重建。

Windows 的 GPUI 在最后一个受管 HWND 销毁时发 PostQuitMessage；自建托盘 HWND
不在 GPUI 的集合内。因此仅在托盘注册成功后创建隐藏、非焦点 PopUp 控制窗口，
避免关闭主窗口或最后一个浮窗时退出消息循环。不能删掉控制窗口而假定托盘线程就能驻留。
托盘初始化失败则关闭主窗口时退出，不能留下没有返回入口的后台进程。
Windows 使用 Shell 的真实注册结果判断可驻留，图标由应用传入的 RGBA 资产加载；原生资源由 tray-icon 在事件线程管理，退出时删除通知图标。
右键使用应用注入的本地化原生菜单，Explorer 的 TaskbarCreated 会重新注册图标。

## Alternatives considered

忽略 WindowHandle.update 错误：会遗留孤立前景窗口，关闭失败却看起来状态已清空。
只增加窗口高度或继续压缩底栏：账户数量、字体与小屏变化后仍会溢出。
用 UI executor 的 timer 假装后台加载：只延后阻塞，不能使 UI 保持响应。

## Consequences

打开主窗口前能够真正销毁浮窗，点击内容区不会自行关闭；失焦及 Esc 会收起。
布局测试填入两卷、三个账户，用 GPUI 真实布局结果约束底栏边界。
另用中英文各六卷场景检查 CPU、内存、硬件区完整位于操作栏之上，磁盘滚动区域
有可用高度且不与硬件区重叠；真实滚轮事件验证后续磁盘可达、硬件区位置不变。
不能仅以底栏没有溢出推断全部核心信息可见。
按钮回归实际派发 MouseDown，并验证平台窗口已经移除；不只检查布尔状态。
另有打开主窗口、快速扫描、Esc 和失活的窗口移除测试。

## Verification

- `src/ui/views/tray_popover.rs::tray_close_button_removes_its_window`
- `src/ui/views/tray_popover.rs::tray_footer_stays_inside_small_window`
- `src/ui/views/tray_popover.rs::tray_hardware_stays_visible_with_many_volumes`

- `../gpui-app-kit/crates/app-host/src/desktop.rs::close_shortcut_removes_window_without_quitting`
- `../gpui-app-kit/crates/app-host/src/desktop.rs::native_close_delivers_cleanup_once`
- `../gpui-app-kit/crates/app-host/src/desktop.rs::keepalive_survives_the_last_user_window`

Proved: 将 Cmd-W 单独恢复绑定 Quit 后，快捷键测试失败于关闭回调次数 0 != 1，见
`docs/agent-notes-evidence/2026-10-15-desktop-close-shortcut-red.log`；恢复后 focused 全绿见
`docs/agent-notes-evidence/2026-10-15-desktop-close-shortcut-green.log`。

Proved: 修复前关闭按钮测试真实失败于关闭自身窗口的断言，错误 window not found，见
`docs/agent-notes-evidence/2026-10-15-tray-close-red.log`。布局为制造型：仅撤掉内容区
flex_1/min_h/overflow_y_scroll 约束，同 focused 测试失败于 footer bottom <= 430，
实测 bottom=568，见 `docs/agent-notes-evidence/2026-10-15-tray-layout-red.log`；
恢复后同 focused 测试与按钮测试结果见 `docs/agent-notes-evidence/2026-10-15-tray-green.log`。

Pitfalls checked: 已读 P1–P62；P2/P3 约束盒度量与溢出，P30 保留 vendored GPUI 与
runtime_shaders，P34 首帧不做同步采样，P53 不用冒泡停止替代命中隔离，P58 文案仍走 tr_*。
未修改命令行解析、官方卸载、残留归属或删除安全路径。新增 P63。


Proved: 多卷硬件可见性测试为修复前真实红跑，硬件区 top=463 / bottom=586，
操作栏 top=304.5，失败于硬件区必须完整位于操作栏上方的断言，见
`docs/agent-notes-evidence/2026-10-15-tray-hardware-visible-red.log`；固定硬件区后，
同 focused 与原托盘测试转绿见
`docs/agent-notes-evidence/2026-10-15-tray-hardware-visible-green.log`。

Native verification: 用 340×430 逻辑点、中文、六卷的独立 GPUI 预览窗口检查画面，
硬件区完整显示 CPU / 温度 / 内存 / 交换空间，磁盘区域独立裁切；不启动真实扫描。

- `../gpui-app-kit/crates/app-host/src/tray.rs::tray_host_programmatic_open_needs_no_window_event`

Proved: 裸 App 启动测试在仅使用 defer 的实现下失败于 startup must not wait for a window event，
见 `docs/agent-notes-evidence/2026-10-15-tray-startup-red.log`；改由前台任务进入 update 后，
同 focused 转绿，见 `docs/agent-notes-evidence/2026-10-15-tray-startup-green.log`。
