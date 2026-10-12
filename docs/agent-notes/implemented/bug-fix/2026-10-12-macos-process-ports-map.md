# Agent Note: macOS 进程端口枚举是空表，`:port` 搜索永远匹配不到

Status: implemented

## Problem

状态监控页进程搜索输入 `:3000` 在 macOS 上检索不到占用端口的进程；
进程列表的线程数列也一直是占位值。Windows 上同一语法正常。

## Decision

`src/platform/macos/status.rs` 的 `process_ports_map` / `process_threads_map`
原来只是返回空 `HashMap` 的 stub。现在走与 lsof 相同的 libproc 通道：
`proc_listallpids` 枚举 PID，`proc_pidinfo(PROC_PIDLISTFDS)` 拿每个进程的
fd 表，`proc_pidfdinfo(PROC_PIDFDSOCKETINFO)` 读 `socket_fdinfo`，
`SOCKINFO_IN`/`SOCKINFO_TCP` 且 `AF_INET`/`AF_INET6` 的 fd 取
`insi_lport`（网络序 int，截 16 位后 `u16::from_be`）。线程数走
`proc_pidinfo(PROC_PIDTASKINFO)` 的 `pti_threadnum`。

`socket_fdinfo` 不在 libc 0.2 导出里，按 SDK `<sys/proc_info.h>` 手工镜像到
`insi_lport` 为止，尾部用 `_rest` 补齐内核要写满的 792B。尺寸与关键偏移
由 `const` 断言钉死（SDK 实测：`socket_fdinfo`=792、`soi_family`@184、
`soi_kind`@256、`insi_lport`@268），ABI 变动在编译期炸而不是读错位。

非本用户进程的 `proc_pidfdinfo` 返回 EPERM，与无特权 lsof 口径一致，跳过。

## Alternatives considered

枚举 `net.inet.tcp.pcblist` 等 sysctl 只能拿 PCB 表，拿不到 owning pid，
需要再回扫 fd 才能归因，并不比 fd 枚举省事。

加 `netstat2`/`libproc` crate 引入新依赖只为复用其结构体定义；项目已有
「单点 FFI 手写小绑定」的先例（同文件的 SMC、`proc_pidinfo`），手写镜像
结构体 + 编译期尺寸断言更轻。

端口按「进程打开的本地端口」归因（监听 + 已连接），和 Windows 的
`GetExtendedTcpTable` 本地端口列一致；只报 LISTEN 会漏掉用户想找的
主动建连进程。

## Consequences

`:port` 搜索、纯端口号文本匹配、线程数列在 macOS 上都有数据了。
权限边界与 lsof 相同：只看得到同用户进程（未提权运行时）的端口。

## Verification

- `src/platform/macos/status.rs::process_ports_map_lists_own_listener`
- `src/platform/macos/status.rs::process_threads_map_counts_own_threads`

Proved: manufactured red —— 把 `process_ports_map` 退回空表后
`process_ports_map_lists_own_listener` 失败，见
`docs/agent-notes-evidence/macos-process-ports-red.log`；恢复实现后
focused 三测全绿，见
`docs/agent-notes-evidence/macos-process-ports-green.log`。

Pitfalls checked: P1–P55 inspected; 本改动只碰 macOS libproc 枚举，
未动卸载/删除/路径保护/搜索语法（P51 的「冒号即端口」边界在
`src/core/status.rs` 没动）。新增 P56 记录此坑。

Validation: cargo fmt --check 与 cargo clippy --all-targets -- -D warnings
在本机 rustc 1.95.0 通过（P55：CI 为更新 stable，结构镜像只用稳定语法，
`offset_of!` 自 1.77 可用）。`cargo test --lib` 744 通过，
`installed_bundle_ids_returns_known_apps` 在未改动的工作区同样失败
（Spotlight 环境不可用），与本次无关。
