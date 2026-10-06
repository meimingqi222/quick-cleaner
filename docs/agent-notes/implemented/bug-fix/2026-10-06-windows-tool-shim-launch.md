# Agent Note: Tool-chain commands are resolved through PATHEXT before launch

Status: implemented

## Problem

`core::proc::run_with_timeout` 用 `Command::new("npm")` 起进程。Windows 上
`CreateProcess` **不查 `PATHEXT`**：它只找 `npm` 与 `npm.exe`，而 npm 在 Windows
上装出来的是 `npm`（无扩展名的 shell 脚本）、`npm.cmd`、`npm.ps1` 三件套，没有
一个叫 `npm.exe`。启动因此失败，`run_with_timeout` 返回 `None`。

失败不带任何报错，表现是「这个生态什么都查不到」：开发环境页的 npm 通道静默变成
0 条，而且**卸载命令同样跑不起来**——界面就算给了移除入口也只会永远拒绝。pnpm
（`pnpm.CMD`）、pipx 同理。`uv.exe`、`node.exe` 这类有真 `.exe` 的不受影响，所以
缺陷看起来像「有些生态认得出来、有些认不出」，而不是「启动机制坏了」。

本机实测（`npm` 在 PATH 上、`C:\nvm4w\nodejs\npm.cmd` 存在）：按 `CreateProcess`
的规则解析时 npm 通道 0 条，补上 `PATHEXT` 解析后 7 条。

这个通道同时承载 Node 全局包与 `core::owner` 的 pnpm store 清理，所以受影响的
不只是新页面。

## Decision

新增 `platform::tool_command(name, args)`，`core::proc::run_tool_with_timeout` 是
它唯一的调用方；工具链命令一律先经 `resolve_tool_program`：绝对路径直接判定，
否则按 `PATH` × `PATHEXT` 查找，优先顺序完全交给 `PATHEXT`（`.EXE` 排在 `.CMD`
前面，能直接启动的优先），不自己排。

`resolve_in` 把查找核心抽成收参数的纯函数：改 `PATH` 是进程级的，测试并行跑会
互相干扰，而「只找 `.exe`、漏掉 `.cmd`」正是要锁住的那条行为。

解析出的 `.cmd` / `.bat` 仍然显式经 `cmd.exe /d /s /c <一整行>` 启动。实测
`CreateProcess` 对显式 `.cmd` 路径的隐式批处理处理**是能跑的**（这是本次被证伪的
一个初始假设），但显式路径才给得了 `/d`——`AutoRun` 是注册表里别人写的命令，会
在我们的命令之前先执行。两条路 cmd 都会重新解析参数，所以 `cmd_line` 直接拒绝含
`" & | < > ^ % !` 的参数，而不是尝试叠两层转义：参数本来只有包名、绝对路径和固定
开关，撞上元字符说明有东西不对。

交给 `/s /c` 的完整命令用 `raw_arg`，带一对最外层引号；路径/参数中的
空格、空参数和括号采用 cmd 引号，反斜杠原样保留。不能把完整命令交给
`.arg(line)`：CRT 会增加反斜杠转义，cmd 把它们当成程序名的一部分。

## Alternatives considered

只给 `.cmd` 垫片加特例、其余命令照旧 `Command::new`——拒绝：`CreateProcess`
补 `.exe` 这件事同样是隐式的，两条解析规则并存会让「这个命令为什么找不到」变成
两套答案。

用 `where.exe npm` 找程序——拒绝：多起一个进程，而且 `where` 的输出要自己解析
（多行、可含未找到提示），比直接在 PATH 里找更脆。

把参数转义后交给 cmd——拒绝：cmd 的转义与 CreateProcess 的引号规则要叠在一起，
多一层就多一处能写错的地方；拒绝不合法字符的代价是零，因为这些参数来自我们
自己构造的命令表。

## Consequences

npm / pnpm / pipx 三条通道在 Windows 上从「静默失效」变成可用。新增一个可注入的
runner（`run_tool_with_timeout`），并且**传入时直接用函数本身**、不再包一层闭包：
本轮就出现过探针里包了旧的 `run_with_timeout` 从而看到假象的情况，可注入的接缝
意味着每个调用点都可能传错 runner。

`core::owner` 的 `PreparedOwner::prepare` 一并改走该通道，pnpm store 的
`pnpm store path` 预检在 Windows 上因此才真正跑得起来；此前它失败后按「预检不可
用」回退到清空目录内容，绕开了 pnpm 自己的 store 一致性管理。

macOS 侧同样提供 `tool_command`（只做 PATH 查找，不做扩展名展开），加入
`platform_contract!`，任何平台漏实现都会在编译期失败。

## Verification

- `src/platform/windows/command.rs::a_command_that_only_exists_as_a_shim_is_still_found`
- `src/platform/windows/command.rs::pathext_order_decides_which_candidate_wins`
- `src/platform/windows/command.rs::a_name_that_already_has_an_extension_is_not_extended_again`
- `src/platform/windows/command.rs::a_script_shim_is_launched_through_cmd`
- `src/platform/windows/command.rs::an_argument_with_cmd_metacharacters_is_refused`
- `src/core/dev_env/discovery.rs::live_probe_lists_the_assets_on_this_machine`

Proved: 把 `resolve_tool_program` 退回 `CreateProcess` 的规则（只找名字与 `.EXE`）
后，同一台机器、同一条 `live_probe_lists_the_assets_on_this_machine` 从 11 条
node 条目掉到 4 条，npm 的 7 个全局包全部消失，而 `npm.cmd` 确实在 PATH 上、
`uv.exe` 一侧不受影响——正是「只有 `.cmd` 垫片的命令失效」这一形态；证据
`docs/agent-notes-evidence/2026-10-06-windows-tool-shim-red.log`。恢复之后同一
命令回到 11 条，证据 `...-green.log`。Pitfalls checked: P20（已尝试生态命令后不
回退裸删）、P25（授权不在执行期扩张）——本次改动只扩大「能问到清单」的范围，不改
任何删除语义。Windows-only 验证；macOS 侧只做了编译期契约检查。

- `src/platform/windows/command.rs::review_shim_preserves_spaces_and_trailing_backslash`

Proved: 修复前带空格脚本路径实际启动返回 exit 1，stderr 报带反斜杠引号的
程序名不存在；`docs/agent-notes-evidence/2026-10-07-review-removal-red.log`。
改用 cmd 原始命令行后，同一脚本断言空格参数、尾反斜杠和空参数均保留；
`docs/agent-notes-evidence/2026-10-07-review-removal-green.log`。P40 已核对。
