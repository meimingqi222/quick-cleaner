# Agent Note: 8.3 短名不能靠字符串相等来断言

Status: implemented

## Problem

`TestAppShortcutNativeReadOnly` 用 `strings.EqualFold` 比较原生读取器返回的目标路径与
`t.TempDir()` 给出的路径。卷上启用 8.3 短名时，`t.TempDir()` 返回 `MEIMIN~1` 形式的短名
（本机为 `D:\code\QUICK-~1\...`），而 `IShellLink::GetPath` 返回长名，两条断言必然红：

```
apps_shortcut_windows_test.go:53: target="C:\Users\<user>\AppData\Local\Temp\...\literal target.exe" ok=true
```

`ok=true` 说明读取器本身正常，红的是断言方式。同一条链路上
`CanonicalAppPath(target)` 的断言同样以字符串形式比较，也会红。

## Decision

测试不再比字符串，改用 `sameAppPath`：两侧都过 `CanonicalAppPath`
（`GetFinalPathNameByHandle`，天然把 8.3 短名展开成长名）后比较；任一侧取不到
canonical 形式时退回字面比较。另新增 `8.3 short path` 子用例，用
`windows.GetShortPathName` 显式把短名写进 `.lnk`，让这条路径在启用 8.3 的卷上必被覆盖。
产品侧 `CanonicalAppPath` 与读取器本来就正确，实现代码未改。

## Alternatives considered

- 测试里调 `windows.GetLongPathName` 归一化：只在卷启用 8.3 时有意义，还要处理调用失败，
  等于把 `CanonicalAppPath` 重写一遍。
- 让 `readAppShortcuts` 返回短名：会把 8.3 形式带进应用清单匹配，等于把卷设置泄漏到产品。
- 直接跳过该测试：CI 的 windows-latest 关闭 8.3 时它不红，但开发机与企业卷会一直红，
  等于放掉这条防护。

## Consequences

启用与关闭 8.3 的卷上行为一致。关闭时 `shortPathName` 原样返回长名，`8.3 short path`
子用例退化成与 `long path` 等价的断言，不会引入环境分支。

## Verification

- `internal/platform/apps_shortcut_windows_test.go::TestAppShortcutNativeReadOnly`

覆盖两个子用例：`long path` 与 `8.3 short path`。

Proved: 撤掉 `sameAppPath` 里的 8.3 归一化（两侧退回 `strings.EqualFold`）后，两个子用例
都红，断言为 `apps_shortcut_windows_test.go:40: target=... ok=true`；日志见
`docs/agent-notes-evidence/2026-10-25-shortcut-83-name-manufactured-red.log`。恢复后同一
focused 运行转绿，且 `go test ./internal/platform` 整体通过；日志见
`docs/agent-notes-evidence/2026-10-25-shortcut-83-name-green.log`。修复前用原测试在本机
复现的原始红灯见 `docs/agent-notes-evidence/2026-10-25-shortcut-83-name-red.log`。

三份日志里的用户名已替换为 `<user>`，其余为原始输出。

三份日志里的用户名已替换为 `<user>`，其余为原始输出。
