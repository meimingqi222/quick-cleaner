# Agent Note: 已有序索引直接输出压实 CSR

Status: implemented

## Problem

压实一个已按 parent-first 排序的映射索引，仍先分配整个 children 矩阵、DFS 获取顺序，再对所有节点排序恢复原 slot 顺序。763 万节点 profile 中邻接构建和排序占约一秒，名称还对相同 base pool offset 重复解码。

## Decision

`internal/core/disk_index_cache_format.go` 的有序通道首先验证所有 Used 节点的 parent index 小于自身，按旧 slot 顺序压实并建立 remap。名称只对每个原 base pool offset 解码一次，仍经共同 interner，保持字节输出一致；覆盖节点使用其真实新名称。数值写入不复制名字。

输出 CSR 本身先作为每个父节点的 child count 表，再做前缀和；名称 offset 数组在记录写完后复用为填充游标，按新 slot 顺序写 child。header/layout 由有序和旧通道共同的 allocateDiskIndex 生成。有 forward parent 的采集树继续使用原拓扑重排，不能对它强用 slot 顺序。

## Alternatives considered

- 保留 DFS 后排序：已有序的场景重复工作且大量分配。
- 所有树都直接按 slot 输出：forward-parent 的原生采集树会写出非法 base。
- 直接保留旧名字池：死节点名字可能跨多次压实永久累积。
- 改成其他持久化格式或 checksum：本次不改变 Rust-v7 ABI。

## Consequences

压实不需要重建完整邻接矩阵；名字池仍只包含当前引用名字，并维持旧的 interning 顺序。未知覆盖仍由共同发布路径按 checksum 绑定。通用写入和增量写入的验证前提没有取消。

## Verification

- `internal/core/disk_index_encode_perf_test.go::TestOrderedBaseEncodingDoesNotBuildAChildMatrix`

Proved: 修复前 200001 个节点编码分配 18695512 字节，超过每节点 64 字节的预算，见 `docs/agent-notes-evidence/2026-10-30-ordered-encode-red.log`；修复后 9558016 字节，完整解码通过，输出与旧 DFS/排序编码器逐字节一致，见 `docs/agent-notes-evidence/2026-10-30-mapped-encoding-final-green.log`。原 forward-parent、墓碑压实、coverage 与 trailing-name ABI 测试继续保留。

实机压实的后台保存分段由此前约 16 秒降至直接压实后的约 5 秒，再到有序编码后的 3.253 秒；重载 812 ms。最终两轮均为 8355 项、相同指纹。事件源动态变化，独立的分配预算与逐字节对照用于固定输入验证；记录见 `docs/agent-notes-evidence/2026-10-30-ordered-final.jsonl` 与对应 `.log`。
