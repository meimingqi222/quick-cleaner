# Agent Note: 智能清理优先使用刚刷新过的索引

Status: implemented

## Problem

智能清理调用共享 owner 刷新磁盘索引后，把新树追加在已经持有的旧树之后；`BeginJunkIndex` 对同一卷保留首棵树，故该轮扫描仍使用旧快照。完成回调又只在搜索树不存在时赋值，导致搜索继续使用旧快照。

## Decision

`internal/app/junk_index_snapshot.go` 将刚刷新的树排在其他树之前，并在发布时替换同卷搜索树。`internal/app/junk.go` 对扫描和搜索都使用它。其他卷、实时目标身份及未知覆盖回退保持原有逻辑。

## Alternatives considered

依赖旧树的存在避免重复分配：旧树是不可变快照，不会自动吸收 owner 的新事件。只修搜索或只修本轮扫描：另一处仍继续使用旧数据。

## Consequences

事件刷新后的新文件、大小和覆盖信息会被当前智能清理与后续搜索使用；不是凭新索引授权删除。

## Verification

- `internal/app/junk_index_snapshot_test.go::TestJunkRefreshPublishesAndUsesNewSnapshot`

Proved: 按原有追加与仅空位赋值行为，扫描实际读到旧大小 1，搜索仍持有旧树，两条断言均失败，见 `docs/agent-notes-evidence/2026-10-28-junk-fresh-index-red.log`；恢复优先与替换后同 focused 通过，见 `docs/agent-notes-evidence/2026-10-28-junk-fresh-index-green.log`。
