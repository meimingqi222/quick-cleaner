# Agent Note: lsof 递归目录复检需要独立长超时

Status: implemented

## Problem

目录的 `lsof +D` 要递归遍历，若复用文件批的 3 秒超时，大目录容易被判为未知并拒删。目录与文件复检的参数和时限必须一起保持。

## Decision

`internal/platform/inuse_darwin.go` 通过 `inUseSpotCheckOptions` 为目录选择 `+D` 和 30 秒，为文件选择精确路径和 3 秒。Rust 参照实现也按递归目录单独分批并设置 30 秒。

## Alternatives considered

目录和文件一律用 3 秒会让正常慢速目录反复超时；一律用长超时会增加文件复检的最坏等待时间。

## Consequences

目录复检允许更久，失败与超时依旧不能视为空闲。测试检验参数和超时选择，并不模拟真实 lsof 的运行时间。

## Verification

- `internal/platform/inuse_darwin_test.go::TestInUseSpotCheckDirectoryTimeout`
- `src/platform/macos/inuse.rs::spot_check_uses_recursive_lsof_for_directories`
- `src/platform/macos/inuse.rs::recursive_directories_do_not_share_a_spot_check_batch`

Proved: 暂时将 Go 目录超时改回 3 秒，focused 测试报告 directory options 为 3s 并失败，见 `docs/agent-notes-evidence/2026-10-28-lsof-timeout-red.log`；恢复后同一测试通过，见 `docs/agent-notes-evidence/2026-10-28-lsof-timeout-green.log`。Rust 测试此轮未运行。
