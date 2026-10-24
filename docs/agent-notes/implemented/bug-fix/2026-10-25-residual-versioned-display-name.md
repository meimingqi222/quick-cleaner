# Agent Note: 应用名带版本号时残留扫描仍要认出短目录名

Status: implemented

## Problem

卸载 `MiniMax Code 3.0.74` 后残留页显示 0 项，但 `%APPDATA%\MiniMax`（1.4 MB）确实还在。
原因是匹配规则对名称长度敏感：

- `Name` 就是 Windows 的 `DisplayName`，即 `MiniMax Code 3.0.74`；
- `windowsResidualName` 去掉非字母数字后是 `minimaxcode3074`（15 个字符）；
- 候选目录 `MiniMax` 归一化后是 `minimax`（7 个字符），走「应用名包含候选」这条反向包含
  规则，要求 `7*2 >= 15`，不成立，于是被拒；`MiniMax Agent` 也因同理被拒。

Rust 的 `norm` 与 `name_matches` 是同一套逻辑（数字同样保留），所以这不是移植回归——
两个版本都有的盲区，只是 Windows 的 DisplayName 经常把版本号写进名字，触发面很宽。

## Decision

新增 `windowsResidualShortName`：剥掉名称尾部纯数字的版本段（含 `v` 前缀），得到用于匹配的
短名；`MiniMax Code 3.0.74` → `MiniMax Code`（11），此时 `7*2 >= 11` 成立。精确目录匹配也
再试一次短名 token（`<root>\<Short>`）。剥完若不足 3 个字符，或剥出来的名字本身是通用名
（`IsSafeAppToken` 拒绝，如 `App 2` → `App`），就保留原名，不放宽匹配。

## Alternatives considered

- 直接放宽反向包含的长度比例：会让 `Remote Desktop Manager` 之类的长名匹配到公共目录，
  正是注释里点名要挡的情况。
- 按发布者目录匹配（`%APPDATA%\<Publisher>`）：`Google`/`Tencent` 这类发布者目录是共享的，
  会跨产品误报。
- 只改 Rust 参照或只改 Go：两边逻辑同源，只改一边会让两个版本行为分叉。

## Consequences

带版本号的显示名（`7-Zip 24.08`、`Foo v1.2`）现在能匹配到去掉版本后的目录；名字本身就是
版本之外的整体时不变。不同产品仍不匹配：`MiniMax Agent` 依然被拒，没有跨产品扩张。

## Verification

- `internal/core/apps_residual_windows_test.go::TestVersionedDisplayNameStillMatchesShortAppDataFolder`
- `internal/core/apps_residual_windows_test.go::TestResidualShortNameDropsTrailingVersion`

Proved: 修复前同一条断言红于 `short app data folder rejected: name="minimaxcode3074"`，见
`docs/agent-notes-evidence/2026-10-25-residual-versioned-name-red.log`；修复后 focused 与
`go test ./internal/core` 全部通过，见
`docs/agent-notes-evidence/2026-10-25-residual-versioned-name-green.log`。

真实数据只读复核：对 `InstalledApp{Name: "MiniMax Code 3.0.74", Publisher: "MiniMax"}` 调用
`ScanAppResiduals`，修复前 `items=0`，修复后 `items=1`，即
`%APPDATA%\Roaming\MiniMax`（1,485,098 B，`LikelyAppDataDir`）；`MiniMax Agent` 仍不匹配。
