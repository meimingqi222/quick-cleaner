# Agent Note: macOS「强制退出」复用 SIGTERM 通道，对忽略 TERM 的进程无效

Status: implemented

## Problem

状态监控页进程详情里，「退出」和「强制退出」对占用端口的 python 服务
都点了没反应，两个按钮表现一致。

## Decision

契约新增 `force_terminate_process(pid, start_time, unique_id)`：

- macOS：`src/platform/macos/status.rs` 把 uniqueid 身份核对提取为
  `ensure_same_process`，`terminate_process` 发 `SIGTERM`，
  `force_terminate_process` 发 `SIGKILL`。SIGTERM 是协作信号
  （`trap '' TERM` 即免疫），强退语义只有 SIGKILL 能满足。
- Windows：`src/platform/windows/status.rs` 委托 `terminate_process`——
  `TerminateProcess` 本身就是不可拦截的强杀，无信号语义差异。
- `src/ui/actions/status.rs` `force_kill_process` 改走
  `force_terminate_process`；`kill_process`（确认弹窗后的「退出」）
  保持 `SIGTERM`。

`platform_contract!` 增加同签名断言，任何平台漏实现直接编译失败。

## Alternatives considered

给 `terminate_process` 加 `force: bool` 参数：调用方少一个函数名，
但 Windows 分支恒忽略该参数（本来即强杀），参数在契约里成了噪音；
独立入口让「强退另有通道」这个事实在契约层面显式化。

macOS 也让 `terminate_process` 直接发 SIGKILL：「退出」应该给进程
收尾机会（对齐活动监视器的「退出」vs「强制退出」），不能全用强杀。

## Consequences

「退出」= SIGTERM（可被处理）、「强制退出」= SIGKILL（无条件），
与系统活动监视器口径一致。Windows 行为不变。

## Verification

- `src/platform/macos/status.rs::force_terminate_kills_process_that_ignores_sigterm`

Proved: manufactured red —— `force_terminate_process` 先实现为委托
`terminate_process`（SIGTERM，即原 bug 形状），测试在
`child.wait()` 上阻塞 60s 直到 sleep 自然结束、断言失败，见
`docs/agent-notes-evidence/macos-force-kill-red.log`；改发 SIGKILL 后
`status.signal()==Some(9)`，focused 11 测全绿，见
`docs/agent-notes-evidence/macos-force-kill-green.log`。

Pitfalls checked: P1–P57 inspected; 结束进程链路只碰信号选择与身份
核对提取，uniqueid 双复核闸门未动。新增 P57 记录此坑。

Validation: cargo fmt --check 与 cargo clippy --all-targets -- -D warnings
通过（rustc 1.95.0）。`cargo test --lib` 745 通过，
`installed_bundle_ids_returns_known_apps` 仍为既有 Spotlight 环境失败。
Windows 侧为单行委托，由契约断言兜底；本机无 msvc target 未交叉编译。
