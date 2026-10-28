# Agent Note: 索引文件名校验遵守本机路径分隔符

Status: implemented

## Problem

macOS 智能清理反复报 `invalid disk index: unsafe name component`，导致整盘索引不可用，固定缓存和构建产物发现退回遍历。现场索引包含以反斜杠为普通字符的文件名，例如 `G:\out\pdf\template_1.pdf`；原校验在所有平台都把反斜杠当路径分隔符，拒绝整个索引。除了全树校验，增量保存快路径还复制了一份相同判据。

## Decision

`internal/core/disk_index_cache_format.go` 的 `validDiskIndexName` 根据本机 `filepath.Separator` 判断反斜杠。Unix 接受它作为文件名字符，Windows 继续拒绝。全树、base/delta 解码及 `internal/core/disk_index_cache_overrides.go` 的增量快路径共用该判据。

空名、`.`、`..`、斜杠、NUL 和非法 UTF-8 仍不能进入有效索引；父节点关系、checksum、覆盖信息及水位校验不变。现场旧索引另有 `/dev/fd` 下两条空名记录，这类真实损坏仍应由正常交互加载触发全量重建，不能靠放宽校验「修复」。

## Alternatives considered

- 所有平台都允许反斜杠：Windows 会把单个节点解释为多个路径组件。
- 跳过坏记录或把空名当目录本身：会破坏父子关系、统计与覆盖证明。
- 只改全树校验：下一次增量持久化仍会拒绝同一文件名，或被迫退回昂贵的全树处理。
- 删除用户文件里的特殊字符：文件名合法，索引应适配文件系统。

## Consequences

合法 Unix 名称可完整读写、查找、更新和增量重载；索引格式保持 v7，无需主动丢弃有效旧索引。损坏旧索引首次重建仍有成本。P26/P27 的不完整覆盖保护、P28/P37 的对象身份和清理保护、P72 的仅验证增量快路径保持。

## Verification

- `internal/core/disk_index_filename_test.go::TestDiskIndexCacheNativeFilenameRoundTrip`
- `internal/core/disk_index_filename_test.go::TestDiskIndexCacheLoadsExistingNativeFilename`
- `internal/core/disk_index_filename_test.go::TestDiskIndexCacheRejectsUnsafeNativeComponents`

Proved: 修复前前两项测试分别在保存和加载时报 `unsafe name component`，自然红跑见 `docs/agent-notes-evidence/2026-10-28-index-native-filename-red.log`；修复后同 focused 全绿见 `docs/agent-notes-evidence/2026-10-28-index-native-filename-green.log`。单独恢复增量快路径旧判据，round-trip 测试在 `legal native filename rejected by incremental fast path` 失败，见 `docs/agent-notes-evidence/2026-10-28-index-native-override-red.log`；恢复共同判据后同 focused 全绿见 `docs/agent-notes-evidence/2026-10-28-index-native-override-green.log`。
