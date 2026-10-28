# Agent Note: 残留清理弹窗遮挡进度条与全量 lsregister 慢速复核修复

Status: implemented

## Problem

用户在应用清理残留时反馈两个明显问题：
1. **清理进度条在后台被遮挡**：点击「彻底清除所选」后，发现残留审查弹窗没有关闭，弹窗中的按钮全部被禁用置灰；而底部的清理进度条被遮挡在 Modal 弹窗背后，且显示为 `0 个文件 / 0 B`，停滞不前。
2. **清理速度比 Rust 版本慢很多**：清理仅数项几 MB 的残留，却卡顿近 10 秒才开始清理。

排查发现：
1. **弹窗生命周期与进度初始化错误**：`internal/app/residuals.go` 中的 `closeResiduals()` 被推迟到了后台清理完成后的回调中；而 Rust 原版在点击开始清理时即刻将面板关闭（`residual.result = None`），进度条在主窗口无遮挡展示。同时 `core.NewCleanProgress(0, 0)` 传入了 0 个文件和 0 字节，丢失了待清理项的统计数据。
2. **`BundleIsStillInstalled` 存在灾难性的全系统转储调用**：在 macOS 上确认应用是否已被卸载时，`BundleIsStillInstalled`（`internal/platform/apps_inventory_macos.go`）在通过 Spotlight 哨兵检测后，误调用了全系统的 `macRegistrationIDs`。该函数执行 `/System/Library/.../lsregister -dump`，生成超过 40 万行文本并遍历解析整个 LaunchServices 注册表，单次执行耗时超过 6~9 秒。针对单一 Bundle ID 的登记查询，系统原生提供了直接面向特定 ID 的 `macWorkspacePaths`（`URLsForApplicationsWithBundleIdentifier:`，耗时 < 1ms），全系统的 dump 是给整体盘点用的，塞进单项卸载复核纯属多余的移植错误。

## Decision

1. **弹窗及时关闭与状态恢复**：
   - 在 `requestResidualClean` 中，用户确认清理后立即调用 `closeResiduals()` 关闭弹窗，让主界面的全局进度条正常展示；
   - 备份当前面板状态（条目、选项、说明、尺寸），若安全核验未通过（`residualAuthority` 拒绝），原样还原弹窗并保留说明和选项，与 Rust 参照行为严格对齐；
   - 统计选中项的总数与总大小，正确传入 `core.NewCleanProgress(uint64(len(items)), totalBytes)`，使进度条能即时显示总量与实时进度。
2. **剥离 `BundleIsStillInstalled` 中多余的全量 `lsregister -dump`**：
   - 移除 `BundleIsStillInstalled` 中对 `macRegistrationIDs`（全系统 40 万行 dump）的调用；
   - 保留针对单一 ID 的 `macWorkspacePaths(ctx, id)` 及随后的 `macRegisteredLive` 卷过滤与 Plist 验证，既满足 P90 对 LaunchServices 登记证据和卷过滤的严密防护，又将核验耗时从 9 秒压至 0.2 秒以内。

## Alternatives considered

- **在 Modal 弹窗内直接画进度条**：弹窗几何有限，且 Rust 的全局进度条统一下沉在底栏，在弹窗里画进度条会破坏与 Rust 界面行为的一致性，也不符合已有的 `progressBar` 结构。
- **完全删除 `BundleIsStillInstalled` 里的 LaunchServices 检查**：Rust 的 `bundle_is_still_installed` 确实只用了 `mdfind` 和哨兵；但保留 `macWorkspacePaths(ctx, id)` 能进一步复查未被 Spotlight 索引的独立卷副本，且基于系统原生 API 仅需毫秒级耗时，既安全又快速。

## Consequences

- 残留清理开始后弹窗立即收起，进度条不再被遮挡，能实时显示待删除文件数及总容量。
- 授权复核失败时能够完整还原现场，用户能清晰看见拒绝原因。
- macOS 残留清理前置等待时间从近 10 秒骤降至 200 毫秒以内。

## Verification

- `internal/app/root_test.go::TestResidualCleanDoesNotReopenTheReviewPanel`
- `internal/app/root_test.go::TestResidualCleanRestoresReviewOnRefusal`
- `internal/platform/apps_inventory_darwin_test.go::TestBundleIsStillInstalledFastPathAbsence`

Proved:
1. `BundleIsStillInstalled` 性能修复红跑：撤销防护重新接入 `macRegistrationIDs`，`TestBundleIsStillInstalledFastPathAbsence` 超时红跑为 `BundleIsStillInstalled took 9.010542667s; expected fast path under 2s without full lsregister dump`，证据见 `docs/agent-notes-evidence/2026-10-27-bundle-absence-fast-path-red.log`；恢复后同 focused 测试 0.16s 绿跑，见 `docs/agent-notes-evidence/2026-10-27-bundle-absence-fast-path-green.log`。
2. 弹窗遮挡与进度修复红跑：撤销清理开始时的 `closeResiduals()`，`TestResidualCleanDoesNotReopenTheReviewPanel` 判定红跑 `review panel remained open while cleaning was in progress`，证据见 `docs/agent-notes-evidence/2026-10-27-residual-panel-close-red.log`；恢复后同 focused 测试绿跑，见 `docs/agent-notes-evidence/2026-10-27-residual-panel-close-green.log`。
