# Agent Note: macOS uninstall verification keeps unknown paths and dangling links

Status: implemented

## Problem

`src/platform/macos/residuals.rs` 使用 exists 过滤卸载后候选，读失败和悬空链接
均会被误判为不存在。`src/platform/macos/apps.rs` 的卸载完成判据有同样问题；
`src/platform/macos/elevate.rs` 更将任意 metadata 错误记作提权删除成功。
这会漏报未清掉的残留，而不是发现规则的缺项。

## Decision

三处仅接受 symlink_metadata 的 NotFound 作为消失证据。仍在或未知均不报完成；
残留候选继续展示，身份未知仍由原有删除前闸门拒绝。卸载收敛等待遇未知立即
失败，不空等也不回退删除；已有路径正常等待窗口不变。

## Alternatives considered

只修提权回执：不能修复残留展示及本体完成判定。canonicalize 后检查：会跟随
不属于本应用的链接，且仍不能把未知变成消失。增加 sleep：不解决证据语义。

## Consequences

未知状态可能要求用户修复权限后重试，不再显示虚假的卸载成功或空残留列表。
不改变安全策略、扫描授权及对象身份要求。与 P16/P20/P25/P37/P44 的完成核验
原则一致；这不是已经发生的用户数据事故，不新增 PITFALLS 编号。

## Verification

- `src/platform/macos/residuals.rs::uninstall_verification_keeps_unreadable_paths_and_dangling_links`
- `src/platform/macos/apps.rs::zoom_discovers_only_the_declared_confined_uninstaller`

Proved: 在 Linux 隔离 Cargo harness 中提取生产 verify_residuals 函数和同名测试，
仅将其文件系统谓词撤回 exists，循环链接祖先与悬空链接两个应保留路径都被滤掉，
断言 left=[] 失败（exit 101）。恢复生产谓词后同 focused 为 1 passed。
证据为 `docs/agent-notes-evidence/2026-10-16-macos-uninstall-absence-red.log` 和
`docs/agent-notes-evidence/2026-10-16-macos-uninstall-absence-green.log`。
真实临时文件与身份捕获参与执行；未用权限错误的伪造返回值。

Limitation: harness 的原生扩展/应用元数据边界为未调用的 stub；未运行提权删除、
真实 Homebrew 或原生卸载器，不是端到端卸载验收。完整产品测试仍需 macOS。
