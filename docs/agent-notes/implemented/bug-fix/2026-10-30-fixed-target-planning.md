# Agent Note: 固定目标规划共享规则与规范路径，保留各目标证据

Status: implemented

## Problem

发现扫描已共用规则，但 `FilesystemScanTargets` 的固定 provider 和声明目标仍逐项深拷贝整个规则包。真实 722 个目标的规划约 0.89 秒、累计分配 280 MiB。消除规则复制后仍分配约 119 MiB：`NormalizeScanTargets` 在目标对循环中反复执行 Norm，512 个独立目标的固定测试分配 15691456 字节。

## Decision

`internal/core/scan_providers.go` 为一次 FilesystemScanTargets 捕获一份冻结 bundle。`internal/core/rule_bundle.go` 的私有 capture 仅存在于枚举包装器中；`internal/core/plan_rule.go` 的 NewRuleRef 在该包装器下使用普通冻结 snapshot，并保留原始 source 的 origin 身份。返回引用的 snapshot 本身没有 capture 元数据，维持 reflect.DeepEqual 和 Merge 的既有去重语义。

固定目标可能对应不同 rule、scope、provider policy 和版本保护，不能直接共用一个 RuleRef。每次 NewRuleRef 仍独立生成 observation、变量和事实；每个 ScanFixed 目标仍捕获自己的身份与 CleanupPlan。普通公开 NewRuleRef 输入继续逐次冻结；下一轮 FilesystemScanTargets 重新捕获源规则。源规则和 Observation 诊断副本不可改变已捕获的计划。

`internal/core/categories.go` 在去重阶段保存每个 merged 目标的规范路径，覆盖对比直接读取这些局部字符串。实际目标路径、最外层父目标的选择、preserve、操作冲突、disposal 和推荐合并均保持原算法；preserve 拆分仍使用实际路径和原深度/预算。

## Alternatives considered

- 给公开 RuleSnapshot 标记全局 immutable：调用方仍能修改 Bundle，不构成真正冻结。
- 所有固定目标共用同一个 RuleRef：不同 scope 的变量/事实及 provider policy 会混淆。
- 返回引用的 origin 改为本轮冻结副本：与同源 provider 合并会误报 Mixed rule snapshots。
- 将捕获包装器直接存入返回 RuleRef：额外元数据改变 reflect.DeepEqual 和重复贡献去重语义。
- 为减少规范化而改变全局 Norm 或跳过重叠/preserve 检查：属于安全行为变化，本次只缓存相同计算结果。

## Consequences

冻结只发生一次，观察和计划按目标独立；同源独立调用仍能正常去重合并。规范路径分配随目标数量增长，不随目标对增长；覆盖比较本身仍为原有 O(N²)，避免改变嵌套和冲突优先级。

## Verification

- `internal/core/scan_targets_perf_test.go::TestFilesystemTargetsShareFrozenRulesAndKeepCaptureIdentity`
- `internal/core/scan_targets_perf_test.go::TestTargetNormalizationDoesNotAllocatePerTargetPair`
- `internal/core/scan_targets_perf_test.go::TestCapturedRuleScopesKeepIndependentEvidence`

Proved: 修复前 24 个固定缓存目标各有不同规则 snapshot，见 `docs/agent-notes-evidence/2026-10-30-fixed-rules-red.log`；捕获后源/诊断隔离、原始 origin、与独立 ProviderRuleRef 的 DeepEqual、独立身份/计划、下一轮重新捕获均通过 race，见 `docs/agent-notes-evidence/2026-10-30-fixed-rules-green.log`。

Proved: 规范路径缓存前 512 个独立 sibling 目标分配 15691456 字节，超过 2 MiB，见 `docs/agent-notes-evidence/2026-10-30-normalize-targets-red.log`；缓存后相同断言与前项计划/规则 focused race 绿，见 `docs/agent-notes-evidence/2026-10-30-fixed-targets-final-green.log`。既有计划和目标重叠/preserve 回归一同运行。


真实固定扫描规划：722 个目标、719 项、50187673600 字节在三阶段均一致；规则共享和规范键复用后，预热规划 889 ms / 280 MiB -> 317 ms / 28 MiB，见 `docs/agent-notes-evidence/2026-10-30-fixed-native-before.jsonl`、`docs/agent-notes-evidence/2026-10-30-fixed-native-capture.jsonl`、`docs/agent-notes-evidence/2026-10-30-fixed-native-after.jsonl`。分配窗口包含并发原生 provider，动态文件系统、Brew 预览和缓存预热使整段耗时不能作为严格 A/B。

不同 scope 的同一冻结规则额外检查 present/absent 两个根的实时 file fact 与 detected 状态、独立 Facts map，以及源规则修改只影响下一次捕获，focused race 见 `docs/agent-notes-evidence/2026-10-30-fixed-scopes-green.log`。
