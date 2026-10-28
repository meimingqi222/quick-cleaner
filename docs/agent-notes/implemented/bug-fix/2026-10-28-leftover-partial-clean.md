# Agent Note: 浮窗部分清理保留未选择的残留

Status: implemented

## Problem

`internal/app/tray.go` 的浮窗清理只复核所选项目，却用这份复核结果替换整个应用的残留行。所选项目全部消失后，未勾选的数据和只能手动处理的系统扩展也从列表消失。

## Decision

冻结选择时单独保留未选择和手动项目。清理后只从所选范围移除核验为已消失的项目，再与未选范围合并，沿用原行的应用、图标与扫描证据。剩余项目清空勾选并重新统计大小；只有整个候选列表为空才撤下该行。若该行已因恢复安装而撤回，完成回调不重新创建它。

## Alternatives considered

- 清理后重新扫描：会扩大发现范围，丢失原先冻结的证据。
- 只保留所选项目的幸存者：把「没处理」误表示为「已清掉」。

## Consequences

用户可稍后继续复核未选数据及系统扩展。删除授权和清理后不重开主窗口复核面板的约束保持。

## Verification

- `internal/app/tray_leftover_clean_test.go::TestLeftoverCleanKeepsUnselectedAndManualItems`

Proved: 修复前所选项已消失、未选文件与系统扩展仍在，测试却收到空列表，断言 `unselected/manual residuals disappeared` 失败；见 `docs/agent-notes-evidence/2026-10-28-leftover-partial-red.log`。修复后同 focused 绿跑见 `docs/agent-notes-evidence/2026-10-28-leftover-partial-green.log`。
