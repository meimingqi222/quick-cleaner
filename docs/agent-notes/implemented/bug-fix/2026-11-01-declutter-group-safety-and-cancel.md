# Agent Note: 整理页分组安全（展开崩溃、整组删除）与取消提示本地化

Status: implemented

## Problem

功能审阅发现三处问题：

1. 相似照片清理后点「展开」整页崩溃。`applyDeclutterPrune` 把 `ExpandedPhotoGroups`
   置为 `nil`，`TogglePhotoGroup` 随后往 nil map 写入，触发
   `panic: assignment to entry in nil map`。重新扫描也不重置展开状态，旧索引会套到新分组上。
2. 重复文件和相似照片都允许把一组全部勾选，清理时不检查。重复文件内容完全相同，整组勾选会把
   唯一的数据一起移入废纸篓，确认框也没有提示。
3. 取消只读任务（扫描、索引加载、搜索、删除前解析）时，状态栏直接显示 Go 的英文
   `context canceled`，没走本地化，也像一条报错。

## Decision

- 剪枝与重扫都把展开状态重置为空 map（不再置 nil）；重扫统一经 `setDeclutterPhotos`。
- 重复文件：任一组全部勾选时拒绝清理，提示具体文件名，要求至少保留一份。
- 相似照片：允许整组清理（相似不等于相同，整组都是废片是正常需求），但确认框顶部提示有几组会
  一张不剩。`requestArbitraryClean` 新增 `warning` 参数承载这段提示。
- `failure` 遇到 `context.Canceled` 时显示本地化的「已取消。」，覆盖 `start` 的通用分支和
  直接调用 `failure(ctx.Err())` 的各处。

## Alternatives considered

- **相似照片也整组拒绝。** 会挡住「这一组全是废片」的正常清理，用户只能逐张取消再勾。
- **重复文件只警告不拒绝。** 内容相同，整组删除没有正当用途，警告容易被一路点过。
- **在 UI 层禁止勾选最后一份。** 「全选」「保留最新」等批量操作与单击都要各自维护约束，
  清理入口一处检查更难被绕过。
- **只在 `start` 里处理取消。** 有调用方直接 `failure(ctx.Err())`，会漏掉。

## Consequences

- 相似照片清理后可以正常展开分组；重扫不会沿用旧的展开状态。
- 重复文件不会被整组移入废纸篓；相似照片整组清理需要看过提示再确认。
- 取消只读任务显示「已取消。」。`context.DeadlineExceeded` 等其他错误仍按原文显示。

## Verification

- `internal/app/declutter_safety_test.go::TestPhotoGroupExpandWorksAfterPruneAndRescan`
- `internal/app/declutter_safety_test.go::TestDuplicateCleanRefusesGroupWithEveryCopySelected`
- `internal/app/declutter_safety_test.go::TestPhotoCleanWarnsWhenWholeGroupSelected`
- `internal/app/declutter_safety_test.go::TestCancelledReadOnlyJobShowsLocalizedStatus`

Proved: TDD 先红。修复前四个测试分别报
`expand after prune panicked: assignment to entry in nil map`、
`every copy of a duplicate group reached the delete confirmation`、
`confirmation does not warn about the whole group: 将把 2 项移入废纸篓。`、
`status = "context canceled", want "已取消。"`，
见 `docs/agent-notes-evidence/2026-11-01-declutter-safety-red.log`。红跑前只抽出了不改行为的
`setDeclutterPhotos`（仅赋值），保证测试能编译。修复后同 focused 绿跑见
`docs/agent-notes-evidence/2026-11-01-declutter-safety-green.log`。

Pitfalls checked: P128 的剪枝条件（勾选 + 未失败 + 确实不在）未改；整理页仍只用扫描时冻结的身份。
