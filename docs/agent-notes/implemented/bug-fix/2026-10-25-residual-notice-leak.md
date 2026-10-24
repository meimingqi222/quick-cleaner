# Agent Note: 残留面板的说明只在面板里，并改成能读懂的话

Status: implemented

## Problem

残留面板打开时，`showResiduals` 把说明写进了共享的 `r.apps.Notice`：

```go
"仅清理单独确认的残留；删除前重新验证应用不存在、目标身份及占用。"
```

`Notice` 在两处渲染——残留面板内部（`apps_page_residuals.go`）和应用页正文
（`apps_page.go`）。而关闭面板只做了 `ResidualsOpen = false`，全仓库唯一的清空点
（`apps.go` 应用列表重扫完成）要等到下次重扫才执行。于是关掉面板后，这句说明留在应用页
正文，下面跟着三张统计卡片，成了一条没有上下文的横幅。

文案本身也有问题：`目标身份`、`占用` 是内部术语，前两条（「单独确认」「应用不存在」）
对用户是废话——复选框本身就是确认。

## Decision

- 拆出 `AppsPage.ResidualNotice`，只在残留面板内渲染；`Notice` 继续承担真正的错误提示，
  页面正文与面板都渲染。两者混用正是这次泄漏的根因。
- `showResiduals` 写 `ResidualNotice`；新增 `closeResiduals()` 统一关闭（清面板状态与说明），
  `CloseResiduals` 动作和清理收尾都走它。
- 文案改成人话，保留唯一对用户有价值的那条信息——勾选不等于一定会删：
  「勾选的项目在删除前会再确认一次：应用确实已卸载、文件没有被替换、当前没被占用；任一项
  不通过就整批不删。」来源安装变体同样改写。

## Alternatives considered

- 关闭时清空 `Notice`：会把真正的失败提示（「残留清理授权无法验证」）一起抹掉，因为它们
  共用一个字段。
- 把说明写死在 UI 层：会丢掉双语与「来源安装」变体，也违反本仓库文案走控制层的约定。
- 直接删掉这句：面板里已有每条残留的「确定/可能」徽章与来源标签，但「勾选≠一定删掉」这个
  预期没有别处表达，删掉后用户遇到整批拒绝会以为是 bug。

## Consequences

面板关闭后正文不再残留横幅；清理失败等真实提示不受影响，仍会显示。用户能读到「勾选后仍可能
整批不删」的原因。

## Verification

- `internal/ui/apps_page_test.go::TestResidualNoticeStaysInThePanel`
- `internal/app/root_test.go::TestClosingResidualReviewDropsOnlyItsOwnExplanation`

Proved: 同时撤掉两处保护（关闭不清理说明 + 正文也渲染说明）后，两条 focused 分别红于
`panel explanation painted on the page body` 与
`explanation outlived the panel: "勾选的项目在删除前会再确认一次…"`，见
`docs/agent-notes-evidence/2026-10-25-residual-notice-leak-red.log`；恢复后 focused 与
`go test ./internal/ui ./internal/app ./internal/core` 全绿，见
`docs/agent-notes-evidence/2026-10-25-residual-notice-leak-green.log`。
