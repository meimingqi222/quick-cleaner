# Agent Note: 产品索引性能可脱离 GUI 重复测量

Status: implemented

## Problem

GUI 日志原先将索引加载排除在阶段零计时外，发现阶段的定向微基准也不能复现事件回放、局部重扫及持久化的整体成本。用户不应反复手动启动程序才能验证性能。

## Decision

`tools/indexbench` 提供 cache 和 refresh 两种模式；refresh 通过 `NewMacDiskIndexWithCache` 调用相同产品 owner，两轮共用同一实例。运行前复制选中卷的缓存及覆盖 sidecar 到私有临时目录，运行结束清理副本，正式缓存始终只读。输出每轮 load/discover/verify/persist_wait 的时间、累计分配、结果数和指纹，并支持 JSON Lines 与按阶段标记的 CPU profile。

`-product-discovery` 额外覆盖 GUI 相同的发现规划和未知目录实时回退。每轮先测前台扫描，再 Flush 等待后台保存；退出也先排空保存，之后移除临时副本。MiB 是整个进程在阶段窗口内的累计分配，包含并发后台保存。

产品 owner 日志细分缓存读取、事件回放、子树扫描、克隆、数组重建、持久化、回退原因与压实后重载。命令说明在 `tools/indexbench/README.md`。

## Alternatives considered

- 只调用 `DiscoverBuildArtifacts`：可定位纯发现成本，但会漏掉本轮的权限回退和重复压实。
- 直接使用正式目录做测量：会改变后续用户扫描的缓存状态；默认在副本中执行。

## Consequences

cache 模式每轮重新解析缓存；refresh 仅支持 macOS，第二轮通过同一 owner 复用内存状态。真实文件事件与权限随时间变化，不能将两次动态 replay 当成严格相同输入；结果指纹和回归测试分别用于发现结果对照与语义验证。

## Verification

- `tools/indexbench/main_test.go::TestCacheModeRunsRepeatedLoadsWithoutChangingSource`
- `tools/indexbench/main_test.go::TestSnapshotCopiesOnlySelectedVolumeAsPrivateFiles`
- `tools/indexbench/main_test.go::TestSnapshotRejectsRedirectedCacheFile`
- `tools/indexbench/main_test.go::TestProductDiscoveryIncludesUncoveredLiveBranch`

本轮完整 refresh 实测首轮 load/discover 3.80/4.47 秒，发现和复核后等待后台保存 15.20 秒；第二轮 0.94/4.13 秒，剩余等待小于 1 ms。两轮 8355 项，指纹与固定缓存发现对照一致。事件源随系统变化，不将不同 replay 的 load 数字当成严格 A/B；记录见 `docs/agent-notes-evidence/2026-10-29-product-refresh-after.jsonl` 与对应 `.log`。
