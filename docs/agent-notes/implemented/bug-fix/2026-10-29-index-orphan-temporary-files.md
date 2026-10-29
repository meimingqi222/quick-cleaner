# Agent Note: 过期索引临时文件需要独立回收

Status: implemented

## Problem

索引清理只识别 .bin，进程中断留下的 .bin.tmp 不匹配，因而长期残留，可能包含文件系统路径。

## Decision

在现有索引回收时删除超过 30 天的可识别索引临时普通文件，保留近期文件、未知名称及活动索引；Go 检查文件身份，避免处理重定向条目。

## Alternatives considered

按 .tmp 后缀无条件删除会影响不属于索引的文件或在写文件；删除现役索引会破坏扫描复用。

## Consequences

清理仅在既有索引回收触发时运行，没有总容量上限；旧临时文件最多保留到下一次触发回收。测试使用当前平台的绝对路径生成索引名，不能硬编码 Unix 的 `/`。

## Verification

- `internal/core/disk_index_cache_prune_test.go::TestDiskIndexPruneOldTemporaryFiles`

Proved: 修复前过期索引临时文件未删除，断言失败，见 `docs/agent-notes-evidence/2026-10-29-index-temp-red.log`；修复后相同测试通过。

Windows CI 在旧夹具中将 `/` 当作挂载点，因不是 Windows 绝对路径导致删除断言失败：见 `docs/agent-notes-evidence/2026-10-29-index-temp-windows-ci-red.log`。
