# Agent Note: 清理结果应用、CLI 显式路径与诊断输出的三处回归修复

Status: implemented
Partly-superseded-by: 2026-11-01-junk-streamlined-clean-ux.md

## Problem

今天的提交在三个用户可见路径上留下回归：

1. Windows 缓存按文件清理后，目录目标只失败了一个叶子（如 `cache\sub\locked.bin`），`report.Failed` 记录的是叶子路径。`applyJunkCleanResult` 只按叶子路径判断目标是否失败，于是整条目标被当成完全成功删除，`RemainingLive` 的缩小更新也被 `continue` 跳过。
2. CLI 的 `selectJunk` 先把显式 `--path` 标记为“已命中”，再因 `CleanupAvailable()` 为 false 跳过选中；最终只校验路径是否存在于扫描结果，不校验是否真的被选中。`junk clean --path <blocked> --execute` 返回 0 且 `ok=0`，自动化会误判为已清理。
3. macOS 卷扫描回收死进程 spill 文件时用 `fmt.Printf` 写“回收死进程溢写文件”，会污染 `--json` 的 stdout；JSON 解析失败但退出码仍为 0。

## Decision

- `applyJunkCleanResult` 不再只看 `report.Failed` 叶子路径，而是结合 `report.FailInfo` 目标级聚合（`core.Norm`）决定条目去留并更新 `RemainingLive`。
- `selectJunk` 在显式路径命中规则阻止目标时返回错误（`path is blocked from cleanup: ...`）。`--all` 和默认推荐仍静默跳过 blocked 项，保持既有行为。
- spill 回收诊断改走 `stderr`，抽成 `logReclaimedMacWalkSpill` 以便跨平台测试；stdout 只留给机器可读报告。

## Superseded

由 `2026-11-01-junk-streamlined-clean-ux.md` 部分取代：
- **仍有效的核心机制**：`applyJunkCleanResult` 按 `report.FailInfo` 目标级聚合识别部分失败目标（而非只查 `report.Failed` 叶子路径）并刷新 `RemainingLive`、CLI 显式 blocked 路径返回非零错误、macOS spill 回收诊断走 `stderr` 均继续有效。
- **已调整的展示边界**：仅因活跃锁（`FailInUse` / `FailUnverified`）留下尾巴的目标在当次扫描视图中直接收起并由顶部 `CleanBanner` 提示；只有存在真实错误（如 `FailAccessDenied`）的部分失败目标保留在主列表、刷新 `RemainingLive` 并自动取消勾选。

## Alternatives considered

- **把叶子失败路径反向映射到父目标再决定去留。** 需要重新实现 core 已有的 `FailInfo` 聚合，容易和父子目标、虚拟路径、大小写规范化再次分叉；直接使用 `FailInfo` 更接近现有单一来源。
- **CLI 显式 blocked 路径继续返回 0，只在报告中标记 blocked。** 自动化无法区分“没有可清理内容”和“用户指定的目标被拒绝”，与 `--execute` 的显式授权语义冲突。
- **spill 诊断改成 `log.Printf`。** 默认写 stderr，但测试与调用方无法替换输出目标；显式 `os.Stderr` 更容易锁定契约。

## Consequences

- 存在真实错误的部分失败目录在清理后仍留在主列表，体积和文件数按剩余实测值缩小并取消勾选；完全成功或仅剩活跃锁尾巴的条目从当次视图收起。
- CLI 脚本能通过非零退出码发现显式路径被规则阻止，不会把空结果当成成功。
- macOS `--json` 的 stdout 保持纯 JSON；回收提示仍对用户可见，但走 stderr。

## Verification

- `internal/app/junk_test.go::TestJunkPartialLeafFailureCollapsesInUseTailAndKeepsRealError`
- `internal/cli/cli_test.go::TestJunkSelectionRequiresFreshExactEligibleTarget`
- `internal/platform/disk_walk_spill_output_test.go::TestMacWalkSpillReclaimLogUsesStderr`

Proved:
1. 部分清理目标红跑：把保留条件退回为只查 `report.Failed` 叶子路径时，子文件失败的目标被误判消失（红绿输出见 `docs/agent-notes-evidence/2026-11-01-review-fixes-red.log` 与 `docs/agent-notes-evidence/2026-11-01-review-fixes-green.log`）；后按 `2026-11-01-junk-streamlined-clean-ux.md` 将测试升级为 `TestJunkPartialLeafFailureCollapsesInUseTailAndKeepsRealError`，锁定真实错误子项仍按 `FailInfo` 保留并更新 `RemainingLive`。
2. CLI 显式 blocked 红跑：撤掉 `selectJunk` 的 explicit 错误分支，
   `TestJunkSelectionRequiresFreshExactEligibleTarget` 报
   `explicit rule-blocked path was reported as selected`；恢复后同 focused 绿跑。
3. spill stdout 红跑：`logReclaimedMacWalkSpill` 先用 `fmt.Printf`，
   `TestMacWalkSpillReclaimLogUsesStderr` 报
   `stdout polluted by spill diagnostics: "回收死进程溢写文件 2 个，释放 3.0 MB\n"`；
   改为 `fmt.Fprintf(os.Stderr, ...)` 后同 focused 绿跑。
   红绿输出见 `docs/agent-notes-evidence/2026-11-01-review-fixes-red.log` 与
   `docs/agent-notes-evidence/2026-11-01-review-fixes-green.log`。

Pitfalls checked: P117–P123 的 fail-closed、按叶子独占删除、占用只做提示、CLI 显式授权边界保持不变；新增 P124。
