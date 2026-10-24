# Agent Note: 卸载失败要说清原因和下一步，而不是抛原始退出码

Status: implemented

## Problem

卸载 `Copilot CLI` 失败时，页面只显示一行英文：

```
Capability step failed: uninstaller exited abnormally: exit code: 0x8a150057
```

这不只是措辞问题——它既没说「有没有删东西」，也没给下一步，还把一个十六进制码丢给用户。
中文界面里出现整句英文，是因为 `apps.go` 走的是 `core.SameText(err.Error())`，错误原文直接
当展示文案。

真实原因在 winget 自己的日志里：

```
[CLI] File hash does not match ARP Entry. Expected: e8e6d7f4…  Actual: f9191acb…
[CLI] Terminating context: 0x8a150057 at …PortableFlow.cpp
```

winget 认为便携包的实际文件哈希与 ARP 登记的对不上（`C:\Users\…\WinGet\Packages\GitHub.Copilot_…`
里的文件被更新过），因此**拒绝卸载并返回 0x8A150057**。QuickCleaner 的行为是对的：如实
转述、不跑回退删除；问题只在怎么把这件事讲给用户。

## Decision

`internal/core/apps_uninstall_registered.go` 把三种已知失败改成类型化错误
（`UninstallerExitError`、`ErrUninstallNotSettled`、`StillRegisteredError`），`Error()` 文案
保持不变，日志与历史记录照旧。

**哈希不匹配由产品自己重试，不推给用户**。`0x8A150057` 是 winget 的可恢复自我校验拒绝，
`--force` 是它文档里的非安全阻断覆盖开关，所以 `runRegisteredAppUninstaller` 拿到这个码时
自动用 `wingetForceCommand`（追加 `--force`，已带则不加）再跑一次，形态与既有的
`WingetAdminContextProhibited` 桌面用户重试一致。`UninstallerExitError.Forced` 记住这次
重试已经发生过。

`internal/app/uninstall_notice.go` 的 `uninstallFailureNotice` 据此产出双语提示：

- winget 家族退出码（`0x8A15xxxx`）：说明是官方卸载器失败、**未删除任何文件**，退出码保留
  便于报障。**不能指向「设置 → 应用」**：那条入口执行的 `UninstallString` 就是同一条
  `winget uninstall`（`--product-code GitHub.Copilot_…`），指过去只会再撞一次同样的拒绝。
- `Forced` 为真（已自动重试仍失败）：文案要如实说「已自动加 --force 重试」并只留排查入口，
  **不能再让用户手动加 --force**——那是把程序该做的事推给人。
- 其它非零退出码：同样的「未删除」与下一步指引，不冒充 winget。
- 安装未落定 / 仍登记：给出对应原因与操作。
- 未分类的错误：保留原文，不编造翻译。

## Alternatives considered

- 直接把这句英文写死成中文：会丢掉退出码，而且换一个退出码就没法区分。
- 在 `core` 里做本地化：`core` 不持有 UI 语言，也没法给「未删除任何文件」这种 UI 承诺负责。
- 只提示用户自己去终端跑 `--force`：把自己能做的恢复动作外包给用户，用户照做还会漏掉
  `--scope user` 等前序参数。
- 失败时自动改用删除文件兜底：违反「失败不授权回退删除」，会把 winget 的校验绕过去。

## Consequences

哈希漂移这类可恢复的拒绝现在由产品自己处理：winget 返回 `0x8A150057` 时自动追加 `--force`
重跑一次，用户不需要打开终端。重试后仍失败才提示，且文案明确「已自动加 --force 重试」，
只留排查入口。日志与历史记录里仍是原始英文错误，便于核对。

这条路线曾经走过两个错：先是指向「设置 → 应用」（ARP 的 `UninstallString` 就是同一条
`winget uninstall --product-code GitHub.Copilot_Microsoft.Winget.Source_8wekyb3d8bbwe`，用户
照做只会再失败一次），再是让用户自己去终端加 `--force`（把自己能做的恢复动作外包给人）。
测试现在同时断言：文案不出现「设置/Settings」，未重试时不出现 `--force`，已重试时必须出现
「已自动加 --force 重试」。

## Verification

- `internal/app/uninstall_notice_test.go::TestUninstallFailureNoticeExplainsAndKeepsTheCode`
- `internal/core/apps_uninstall_registered_test.go::TestRegisteredUninstallRetriesWingetHashMismatchWithForce`
- `internal/core/apps_uninstall_registered_test.go::TestRegisteredUninstallForceRetryMarksTheFailure`
- `internal/core/apps_uninstall_registered_test.go::TestWingetForceCommandDoesNotDoubleTheFlag`

Proved: 只撤掉自动 `--force` 重试分支后，`TestRegisteredUninstallRetriesWingetHashMismatchWithForce`
红于 `force retry did not recover: uninstaller exited abnormally: exit code: 0x8a150057`，
`TestRegisteredUninstallForceRetryMarksTheFailure` 红于 `want a forced UninstallerExitError`，
见 `docs/agent-notes-evidence/2026-10-25-uninstall-auto-force-red.log`；恢复后 focused 与
`go test ./internal/core ./internal/app` 全绿，见
`docs/agent-notes-evidence/2026-10-25-uninstall-auto-force-green.log`。

文案侧：只撤掉 winget 退出码识别后，`TestUninstallFailureNoticeExplainsAndKeepsTheCode` 红于
`winget notice lost its code or hint`，见
`docs/agent-notes-evidence/2026-10-25-uninstall-failure-notice-red.log`；恢复后绿，见
`docs/agent-notes-evidence/2026-10-25-uninstall-failure-notice-green.log`。
