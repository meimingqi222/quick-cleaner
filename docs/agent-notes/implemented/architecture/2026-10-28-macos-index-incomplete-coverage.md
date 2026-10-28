# Agent Note: macOS 索引缺失与未覆盖子树的区别

Status: implemented

## Problem

File Provider 的 dataless 目录可正常读取元数据，却可能在枚举时长期挂起；跳过的子树不能被解释为不存在，否则使用索引的扫描会漏掉目标。

## Decision

`internal/platform/disk_walk_darwin.go` 读取系统目录标志识别 dataless；遍历策略记录跳过或超时子树。`internal/core/disk_tree.go` 与 `internal/core/fs_query.go` 在覆盖不完整时回退实时查询，覆盖信息随索引缓存保存。路径缺失与未覆盖是不同状态。

## Alternatives considered

按目录名推测 File Provider 或把索引缺项视为不存在，都会产生误判和漏扫。

## Consequences

遇到不可枚举子树时索引可能不完整；实时回退的耗时依赖文件系统，不能承诺恒定速度。

## Verification

- `internal/platform/disk_walk_test.go::TestDiskWalkMirrorPruningAndDatalessCoverage`
- `internal/platform/disk_walk_test.go::TestDiskWalkTimeoutContinuesAndPersistsExclusion`
- `internal/core/disk_index_macos_test.go::TestMacDiskIndexRawCoverageAndMetadataErrors`
- `internal/core/fs_query_test.go::TestFSIndexMissingAndBlockedRootsFallBack`
- `internal/core/disk_index_cache_coverage_test.go::TestDiskIndexCacheCoverageSurvivesReloadDeltaAndCompaction`

这些测试覆盖模拟标志、超时、回退及缓存覆盖；不证明每个真实 File Provider 的挂起行为。
