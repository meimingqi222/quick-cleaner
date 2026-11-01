# Agent Note: 开发环境移除后同步容量与包计数，并在后台重新测量

Status: implemented

## Problem

开发环境移除成功后刻意不重扫，`dropRemovedDevAssets` 只删行，`TotalLogical`、`TotalReclaimable`、
父环境 `Size` 和 `PackageCount` 都保持旧值并直接显示（审阅实测：行数 0，逻辑容量仍 100、可释放仍 80）。

## Decision

- 立即记账：移除资产时总量扣减其独占字节；移除包时父环境扣减包的逻辑/独占/文件数，`PackageCount`
  减去移除数（换新指针，不改共享的旧值）；可释放总量按扫描同一公式重算（所有可移除资产的独占字节）。
- 立即记账只是估计：扫描总量按「测量目录内的唯一文件」计，删掉一个目录时，与其他测量目录无关的硬链接
  文件也一并离开总量，与剩余资产共享的文件则变成对方的独占字节，精确值必须重新读取。所以移除后用
  `remeasureDev` 在后台对剩余顶层资产调用 `MeasureBatchStorage`（经 `Root.measureDevStorage` 注入），
  结果替换各资产大小与两个总量。不是 busy 任务，页面可继续使用；新的扫描、移除或资产列表变化都会丢弃结果。

## Alternatives considered

- **移除后完整重扫。** 原设计刻意避免（重新发现耗时、列表跳动）；重新测量只读剩余目录的存储。
- **只按逻辑大小扣减。** 共享硬链接被重复扣掉；只按独占字节扣减又会漏掉离开测量范围的硬链接。两者都只是界。
- **把总量标为「待刷新」不更新。** 后台测量结束前用户看到的仍是明显过期的数字。

## Consequences

- 移除后容量立即变化，随后几秒内更新为实测值。
- 包的大小在重新选择环境时才重读，不在本次后台测量范围内。

## Verification

- `internal/app/declutter_safety_test.go::TestDevRemovalKeepsTotalsAndParentInStep`

Proved: TDD 先红。修复前报 `total logical = 350, want 262`，见
`docs/agent-notes-evidence/2026-11-01-dev-removal-totals-red.log`。制造型红跑：去掉后台重新测量时报
`worker did not publish`；忽略代次时报 `stale measurement overwrote newer state`（同一 red log 追加）。
恢复后绿跑见 `docs/agent-notes-evidence/2026-11-01-dev-removal-totals-green.log`。

Pitfalls checked: 移除执行路径与复核不变，只改移除后的展示数据。
