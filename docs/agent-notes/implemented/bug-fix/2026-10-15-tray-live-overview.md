# Agent Note: 空间透镜持续刷新与显式扫描入口

Status: implemented

## Problem

`src/ui/views/tray_popover.rs` 只在打开时取一次快照，并把全量 StatusSampler
（包括进程、端口、线程枚举）与用户目录浅层遍历串行执行。用户看到旧数字，
还要等无关统计才能显示硬件数据。浅层截断的账户体积也不是完整占用。
`src/main.rs` 在首次构造 Root 后自动扫垃圾和应用，使打开主界面与扫描入口重叠；
`src/ui/mod.rs` 关闭 FDA 引导还会补启动一次隐式扫描。

macOS 根设备名 `/dev/disk3s3s1` 带快照层，而 Recovery 是 `/dev/disk3s5`；
`src/platform/macos/disk.rs` 只去掉一次 sN 后缀，错误拆成两组，显示两份相同容量。

## Decision

`src/core/tray_status.rs` 只刷新 sysinfo 的 CPU 与内存，不建进程表。
采样器跨拍保留，先在后台建基线并等待 MINIMUM_CPU_UPDATE_INTERVAL，再显示首拍。
CPU / 内存约 2 秒一拍，磁盘 / 温度每五拍刷新；账户列表独立后台读取，不遍历主目录，
不再用局部估算伪装总占用。UI 显示内存用量 / 总量、交换空间、CPU / 内存标签和刷新周期。

轮询任务由 `src/ui/tray_overview.rs` 的业务模型持有，内容视图观察模型重绘。
每拍开始前通过 `src/ui/tray_host.rs` 提供的 PopoverLifetime 核对原窗口仍在 App 的窗口表中。
窗口销毁与实体释放不是同一个时点：GPUI 可能暂留上一帧实体，仅靠 Task Drop
仍可能多采样一轮。窗口检查必须先于后台采样，不能只在贴回结果时才检查实体。
已有 deferred 窗口切换与受限滚动约束仍由
`2026-10-15-tray-popover-render-lifecycle.md` 维护。

打开主界面只展示已有状态；垃圾扫描由主界面或托盘的扫描垃圾按钮触发，
托盘扫描切到垃圾页。应用库存由用户进入应用页时按需获取；更新调度器照常启动。
关闭权限引导不扫描；显式授予 FDA 后的权限重检仍可重扫以补齐新解锁的缓存。
内存释放进后台，开始时立即禁重入，按系统命令真实成功结果展示反馈。

APFS 设备分组去掉所有数字 sN 层，将根快照与同容器卷归到一起。
Recovery / Preboot / VM / Update 不能覆盖根卷的正常展示标签。

## Alternatives considered

每两秒重建完整 StatusSampler：CPU 差值基线失效，还会高频扫描全系统进程和端口。
把一次性采样放到后台但仍等待账户遍历：UI 不阻塞，却依然延迟显示所有数据。
根据总容量 / 剩余量相等来去重：两个独立磁盘可能碰巧容量相同，必须按设备身份归组。
只删启动扫描、不删引导关闭扫描：首次启动仍会隐式执行扫描。

## Consequences

关闭浮窗不再持续轮询；正在进行的单次系统调用无需强行打断。
没有新增持续后台全盘统计，账户区明确显示目录。重开浮窗重新建立 CPU 基线。
内存释放失败不再虚报成功；Windows 的 EmptyWorkingSet 仍仅针对本进程。
当前仅在 macOS 验证，Windows 原生交互待该平台验证。

## Verification

- `src/ui/tray_overview.rs::tray_refresh_updates_repeatedly_and_stops_on_close`
- `src/ui/mod.rs::dismissing_permission_guide_does_not_start_a_scan`
- `src/core/tray_status.rs::tray_sampler_reads_memory_without_building_a_process_table`
- `src/platform/macos/disk.rs::tray_snapshot_and_recovery_share_one_container`

Proved: 真实红跑中，轮询测试在关闭窗口后多采了一轮（4 != 3），见
`docs/agent-notes-evidence/2026-10-15-tray-refresh-red.log`；补窗口表检查后同 focused
转绿，见 `docs/agent-notes-evidence/2026-10-15-tray-refresh-green.log`。

Proved: 关闭引导测试在原行为下失败于 dismissing a window is not scan authorization，
见 `docs/agent-notes-evidence/2026-10-15-tray-entry-red.log`；修复后同测试转绿见
`docs/agent-notes-evidence/2026-10-15-tray-refresh-green.log`。

Proved: 仅将设备后缀处理恢复为单层后，容器相等断言失败，见
`docs/agent-notes-evidence/2026-10-15-tray-volume-red.log`；恢复后同 focused 转绿见
`docs/agent-notes-evidence/2026-10-15-tray-volume-green.log`。

Pitfalls checked: 本轮涉及 P2/P3 的盒度量和裁切、P34 首帧后台采样、P58 本地化、
P63 窗口 defer 与生命周期，均保留现有防护。未改卸载命令、残留归属与删除保护。
新增 P64。

Validation: cargo fmt --check、cargo build、cargo clippy --all-targets -- -D warnings、
本地化守卫与 strict-anchors 校验通过。完整 cargo test --lib 在宿主权限下
834 passed、12 ignored、0 failed；沙箱内日志写入、Spotlight、FSEvents、用户目录权限与
进程可见性相关的 8 项失败，在宿主重跑全部转绿。未做 Windows 实机验证。
