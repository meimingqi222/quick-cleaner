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
保持不变，日志与历史记录照旧。`internal/app/uninstall_notice.go` 的 `uninstallFailureNotice`
据此产出双语提示：

- winget 家族退出码（`0x8A15xxxx`）：说明是官方卸载器失败、**未删除任何文件**，并指出可以在
  终端跑带 `--force` 的 `winget uninstall`；退出码保留在文案里便于报障。**不能指向「设置 →
  应用」**：那条入口执行的 `UninstallString` 就是同一条 `winget uninstall`
  （`--product-code GitHub.Copilot_…`），指过去只会再撞一次同样的拒绝。
- 其它非零退出码：同样的「未删除」与下一步指引，不冒充 winget。
- 安装未落定 / 仍登记：给出对应原因与操作。
- 未分类的错误：保留原文，不编造翻译。

## Alternatives considered

- 直接把这句英文写死成中文：会丢掉退出码，而且换一个退出码就没法区分。
- 在 `core` 里做本地化：`core` 不持有 UI 语言，也没法给「未删除任何文件」这种 UI 承诺负责。
- 失败时自动改用删除文件兜底：违反「失败不授权回退删除」，会把 winget 的校验绕过去。

## Consequences

页面显示「官方卸载器（winget）失败（0x8a150057）；未删除任何文件。可在终端运行带 --force
的 winget uninstall 强制卸载。」日志与历史记录里仍是原始英文错误，便于核对。

这条文案曾经指向「设置 → 应用」，那是错的：ARP 的 `UninstallString` 就是同一条
`winget uninstall --product-code GitHub.Copilot_Microsoft.Winget.Source_8wekyb3d8bbwe`，
用户照做只会再失败一次。现在测试同时断言文案里不出现「设置/Settings」。

## Verification

- `internal/app/uninstall_notice_test.go::TestUninstallFailureNoticeExplainsAndKeepsTheCode`

Proved: 只撤掉 winget 退出码识别后，focused 测试红于
`winget notice lost its code or hint: "官方卸载器异常退出（0x8a150057）；未删除任何文件。…"`，
见 `docs/agent-notes-evidence/2026-10-25-uninstall-failure-notice-red.log`；恢复后 focused 与
`go test ./internal/app ./internal/core` 全绿，见
`docs/agent-notes-evidence/2026-10-25-uninstall-failure-notice-green.log`。
