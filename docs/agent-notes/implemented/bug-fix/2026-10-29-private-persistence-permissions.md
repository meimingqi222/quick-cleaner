# Agent Note: 敏感持久文件必须收紧已有权限

Status: implemented

## Problem

历史、设置和日志包含用户路径。Unix 创建模式受 umask 影响，旧文件也可能已经是 0644；日志轮转时 rename 保留旧权限。

## Decision

Go 在写入前为敏感文件设置 0600，启动日志时也收紧已有历史、设置和两代日志。Go 索引使用私有临时文件发布，并在回收时收紧旧索引权限。权限设置失败时不继续写入敏感内容。Windows 权限由系统 ACL 决定，Unix mode 不等同于 ACL。

## Alternatives considered

只改创建模式无法修复已有文件，也不能修复轮转后的旧日志；只 chmod 新日志会留下旧日志和历史。

## Consequences

保留现有文件格式与日志轮转行为；外部 ACL、父目录及 Windows ACL 不由此修改。

## Verification

- `internal/core/log_test.go::TestProductLogRestrictsExistingAndRotatedFiles`

Proved: 修复前轮转后的日志仍为 0644，断言失败，见 `docs/agent-notes-evidence/2026-10-29-private-files-red.log`；修复后相同测试通过。
