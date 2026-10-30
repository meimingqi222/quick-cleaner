# Agent Note: 事件覆盖检查与旧缺口清理按批次处理

Status: implemented

## Problem

真实回放约 2 万条文件变化时，refreshMacDiskIndex 在每个文件上遍历全部重扫根，并在每条删除上遍历所有 blocked 缺口。CPU profile 中重复 filepath.Rel 分别占约 3.1 和 4.6 秒；这不是磁盘子树扫描的成本。

## Decision

`internal/core/disk_index_macos.go` 用规范化路径集合记录已合并的重扫根，沿查询路径父链判断是否被覆盖，保持路径组件边界。成功删除和成功重扫先收集清除范围，最后一次过滤旧 blocked 列表。新子树报告的新缺口单独累积后追加，不能因为它位于成功重扫根下而被同批清除；权限失败的根不会加入清除集合，uncertain 仍最后加入。

此集合只加速 macOS 索引内部的词法范围判断，不参与清理授权，也不改变中央 safety 模块。

## Alternatives considered

- 使用不带分隔符边界的字符串前缀：会把 a-x 当作 a 的子目录。
- 对最终全部 blocked 统一过滤：会同时清掉本轮刚发现的未知云端目录。
- 忽略 coverage 或权限失败：会把未知当成不存在。

## Consequences

根覆盖判断从文件数乘重扫根数变为查询路径深度；旧缺口清理从删除数乘缺口数变为一次旧缺口遍历。旧快照独立，新的缺口和未恢复权限仍未知。

## Verification

- `internal/core/disk_index_path_set_test.go::TestMacIndexRemovedPathsClearCoverageInOneBatch`
- `internal/core/disk_index_path_set_test.go::TestMacIndexBatchClearPreservesNewSubtreeGaps`
- `internal/core/disk_index_path_set_test.go::TestMacIndexPathSetMatchesLexicalCoverage`

Proved: 修复前 513 个删除、128 个旧缺口累计分配 1991432 字节，超过 1 MiB 上限，见 `docs/agent-notes-evidence/2026-10-30-index-path-batch-red.log`；修复后绿跑，验证旧快照隔离、只清除对应旧缺口、保留新子树缺口，以及尾分隔符、前缀邻居和点路径与原词法判断一致，见 `docs/agent-notes-evidence/2026-10-30-index-final-green.log`。

单机真实回放（事件流动态变化，非固定输入 A/B）：修复前 load 21.20 秒、累计分配 94628.9 MiB；批量处理和增长预留后首轮 load 5.31 秒、2240.8 MiB。输出及分段见 `docs/agent-notes-evidence/2026-10-30-index-performance-before.log`、`docs/agent-notes-evidence/2026-10-30-index-performance-before-phases.log` 与 `docs/agent-notes-evidence/2026-10-30-index-growth-after.jsonl`。累计分配不是驻留内存。
