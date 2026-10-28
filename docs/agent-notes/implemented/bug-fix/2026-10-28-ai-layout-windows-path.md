# Agent Note: AI 布局基线比较规范化双方路径

Status: implemented

## Problem

Windows CI 的 AI agent 布局基线测试将 worktree 登记路径判为夹具外。Windows 临时目录可能使用短名，而发现的 worktree 登记路径是普通长路径；夹具根的 canonicalize 结果又带有 verbatim 前缀。三种形式直接比较无法保证相同对象匹配。

## Decision

测试专用的 `ai_agent_rows` 对夹具根和目标分别调用 canonicalize 后，再严格按路径组件求相对路径。路径不存在或不在规范化后的夹具内仍然失败；目标、操作、标签与金样不变。生产 worktree 发现与安全规则未变。

## Alternatives considered

移除夹具边界断言或修改迁移金样会掩盖真正的范围回归；仅剥掉 verbatim 前缀无法解决短名和长名差异。

## Consequences

比较依赖目标和 Git 登记确实存在，符合该基线测试只记录现存目录的约定。Windows 原生运行仍由 CI 验证。

## Verification

- `src/core/categories/dev.rs::ai_agent_layouts_preserve_the_migration_baseline`
- `src/core/categories/dev.rs::ai_agent_rows_accepts_normal_and_verbatim_fixture_paths`

Proved: Windows CI 修复前实际在基线测试报告 target outside the fixture，见 `docs/agent-notes-evidence/2026-10-28-ai-layout-windows-red.log`（本机路径已匿名化）；修复后需通过相同 Windows CI 测试确认。
