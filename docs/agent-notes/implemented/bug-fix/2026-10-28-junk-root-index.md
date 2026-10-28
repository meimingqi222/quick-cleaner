# Agent Note: 智能清理保留 Unix 根卷共享索引

Status: implemented

## Problem

修复文件名校验并重建索引后，macOS 缓存扫描仍走 live 遍历。`internal/core/junk_index.go` 的 `normMount` 复用 `Norm`，后者把斜杠转成反斜杠并去掉尾部分隔符，使根挂载点 `/` 变成空字符串。规划器直接丢掉该树；卷归属及 query 选树也不能匹配根卷。能加载索引不代表扫描真正使用它。

## Decision

根挂载点在 `normMount` 中保留为 `/`；`junkMountContains` 明确让根卷包含绝对路径，拒绝相对路径。其余卷沿用原规范形式及路径边界判断，查询仍优先选择最长、最具体的挂载点。没有改变 `Norm` 的全局安全策略或 Windows 盘符匹配。

## Alternatives considered

- 全局改变 `Norm`：会影响白名单、路径保护和跨平台路径测试，修复范围过大。
- 只让规划器保留根树：query 的最长挂载点选择仍会拒绝它，扫描继续隐式回退。
- 看到根索引就宣称整盘覆盖：被跳过、dataless 或权限未知的子树仍必须回退，不能因性能绕过覆盖证明。

## Consequences

根卷共享树能用于固定缓存统计和发现式扫描，其他卷及更具体挂载点的优先级保持。可用树仍须经过实时目标身份和覆盖判断；首次重建、FSEvents 回放和原生命令探测的耗时不是这项修复消除的内容。

## Verification

- `internal/core/junk_index_root_test.go::TestJunkIndexRetainsUnixRootVolume`

Proved: 修复前测试在 `root-volume index was discarded` 失败，见 `docs/agent-notes-evidence/2026-10-28-junk-root-index-red.log`。修复后同 focused 通过，验证根树能统计不存在于实时磁盘的哨兵数据、根卷拒绝相对路径及更具体挂载点优先，见 `docs/agent-notes-evidence/2026-10-28-junk-root-index-green.log`。


同批本机文件系统目标实测：live 固定扫描 5.46 秒，根卷 query 固定扫描 3.09 秒，
复用 query 再扫 3.18 秒，711 项命中保持；索引冷读 2.05 秒另计。
这是单机固定目录阶段测量，未包含原生命令探测、FSEvents 回放、发现式阶段或 UI；
快照与实时大小存在小幅变化，不作为端到端或跨机器性能保证。
记录见 `docs/agent-notes-evidence/2026-10-28-cache-scan-performance.log`。
