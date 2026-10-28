# Agent Note: UI 与卸载监视共享通知去重锁

Status: implemented

## Problem

`internal/app/apps.go` 从 UI 线程记录应用内卸载，`internal/app/trash_watch_darwin.go` 从后台 goroutine 查询、插入、遍历和删除同一个 `Root.trashSeen` map。普通 Go map 的并发访问会产生数据竞争，甚至触发不可恢复的 concurrent map 崩溃。

## Decision

`internal/app/root.go` 的 `trashSeenMu` 保护所有生产读写。`internal/app/tray.go` 的 `markTrashSeen` 在同一个临界区查询并认领通知；恢复安装的遍历和删除也共享该锁。发布 UI 更新及残留扫描在锁外执行。

## Alternatives considered

- 仅锁写入：遍历和读写仍会竞争。
- 先查再分别加锁写入：两个来源仍可能同时认领同一个通知。
- 把整次扫描放进锁内：会阻塞 UI 线程数秒。

## Consequences

通知去重在两个来源之间保持原子性；后台恢复检查不会与应用内卸载并发破坏 map。没有更改 UI 模型的主线程发布规则。

## Verification

- `internal/app/trash_seen_test.go::TestTrashSeenConcurrentClaims`

Proved: 将原有无锁 map 读写提取为共同方法后，32 个并发来源认领同一 ID，`go test -race` 实际报告 `race detected during execution of test`，见 `docs/agent-notes-evidence/2026-10-28-trash-seen-red.log`。加锁后同 focused 通过且仅认领一次，见 `docs/agent-notes-evidence/2026-10-28-trash-seen-green.log`。
