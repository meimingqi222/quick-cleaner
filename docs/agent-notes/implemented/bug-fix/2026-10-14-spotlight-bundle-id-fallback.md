# Agent Note: Spotlight 缺失 ID 时精确补查应用包

Status: implemented

## Problem

`src/platform/macos/apps.rs` 的 installed_bundle_ids 在 mdls 返回任意一个空 ID 或 (null)
时立即返回 None。本机索引正常、572 个真实应用包中有一个虚拟机应用元数据为 (null)，
但其 Info.plist 可以正常读出 CFBundleIdentifier，导致整轮孤儿残留扫描不可用。

## Decision

保留批量 Spotlight 查询。bundle_ids_from_metadata 先检查返回值数量，再按请求路径逐个配对。
仅对空或 (null) ID 使用已有有界 plist_string，从对应包的 Contents/Info.plist 精确读取
CFBundleIdentifier。补查仍失败、超时、缺失或为空/(null) 时返回 None，绝不返回部分集合。
查询失败与结果数量不符仍拒绝；正常 ID 不触发逐包进程。常见安装根补查保持原行为。

## Alternatives considered

跳过未知包并返回其余 ID 会将仍安装应用的数据误判成孤儿，违反 P31。
忽略 Spotlight 的全部结果改为逐包读取会增加大量进程，不符合仅补缺项的需要。
放宽扫描/清理层的 None 判据无法证明所有权，不能用来恢复可用性。

## Consequences

有效包的索引字段缺失不再阻断扫描；两条身份读取路径都失败时仍整轮拒绝。
每次补查最多两秒，未更改残留匹配、删除授权或路径保护。

## Verification

- `src/platform/macos/apps.rs::missing_spotlight_id_uses_exact_bundle_plist`
- `src/platform/macos/apps.rs::missing_spotlight_identity_stays_fail_closed`
- `src/platform/macos/apps.rs::installed_bundle_ids_returns_known_apps`

Proved: 修复前自然红跑，合法 plist + (null) metadata 的恢复断言失败，见
`docs/agent-notes-evidence/2026-10-14-spotlight-red.log`；精确补查后同 focused 转绿，见
`docs/agent-notes-evidence/2026-10-14-spotlight-green.log`。

Pitfalls checked: P1–P59 完整核对；P31 的未知所有者拒绝规则和 P49 输入法根补查保留，
P46 通过已有有界进程调用满足。未触及卸载执行、目录删除、身份复核或生态资产称重。新增 P60。
