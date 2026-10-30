# Agent Note: 局部目录权限失败保留未知覆盖，不触发整盘重建

Status: implemented

## Problem

Go 智能清理的索引加载曾耗时 104.7 秒。无界面 `tools/indexbench` 复现：缓存读取约 1.8 秒、事件回放不足 1 秒，某个系统保护子目录返回 EPERM/EACCES 就让 `refreshMacDiskIndex` 整批失败，`MacDiskIndex.Load` 随即全盘重建。连续两轮约 98.6/93.2 秒。

## Decision

`internal/core/disk_index_macos.go` 只对增量子树的 EPERM/EACCES 保留旧子树数据并加入 blocked 覆盖，继续应用其他成功的子树。不可读内容是未知，不是不存在。不会清空该子树，不将旧值当作完整覆盖；之后成功重扫同一根才清除该范围的 blocked 状态。原快照不变，未知覆盖随缓存持久化。其他结构错误、事件历史丢失和取消行为保持原有保护。

## Alternatives considered

- 任何子目录错误都重建全盘：被系统保护的目录在全量遍历里仍不可读，只是扩大成本。
- 将失败子树当空树或直接清除 blocked：把未知解释为不存在，破坏索引覆盖契约。
- 忽略所有扫描错误：会掩盖结构损坏；只收敛已知权限错误。

## Consequences

局部 TCC/FDA 或 Unix 权限限制不会使每次交互扫描都隐式重建整卷。计数和大小可能保留旧子树的估计值；查询的覆盖检查会回退实时探测，不凭旧数据授权删除。恢复读取权限后，仍需收到/发起该根的重扫才能恢复完整覆盖。

## Verification

- `internal/core/disk_index_refresh_permission_test.go::TestMacDiskIndexPermissionFailureKeepsUnknownSubtreeWithoutRebuild`

Proved: 修复前 EPERM 和 EACCES 两个用例都在 `local permission error triggered a full rebuild` 失败，见 `docs/agent-notes-evidence/2026-10-29-index-permission-red.log`；修复后相同测试通过，见 `docs/agent-notes-evidence/2026-10-29-index-refresh-green.log`。同一测试检查健康子树更新、不可读旧数据保留、覆盖持久化、后续恢复，以及非权限错误仍请求重建。
