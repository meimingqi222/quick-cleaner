# Agent Note: 开发环境搜索框按退格删不掉文字

Status: implemented

## Problem

软件管理 → 开发环境的搜索框，光标在末尾时按 Backspace 没有任何反应；光标在中间时删掉的是
右边的字。`2f01f3e` 加开发环境页时，把退格回调接到了 `text_input_delete(false, cx)`：
这个函数实现的是 Delete 键（`delete_forward`），`false` 只是在选 Apps 输入框而非文件搜索框。
重构 `3072695` 原样保留了这个行为，只把它展开成了显式的 `delete_forward`。
根因在接口：`search_box` 让每个调用方自己传 `on_backspace`，四个搜索框四种写法
（其中文件搜索手写了一遍删除逻辑），而同一组件里的 Delete 键早已统一实现。

## Decision

`search_box` 去掉 `on_backspace` 参数。Backspace 与 Delete 在组件内同一分支处理：
`delete_backward` / `delete_forward` → 清掉组合态 → `binding.changed` → `binding.activity` →
`cx.notify()`。各页面的差异只放在 binding 的 `on_change` 里：进程搜索的 `on_change` 调
`search_popover.edited()`，它本来就等于旧退格回调里的 `reopen()` + 重置选中，行为不变；
文件搜索的 `on_change` 在索引未就绪时不重绘，所以组件自己补一次 `notify`。
删掉不再使用的 `search_backspace` / `file_search_backspace` / `status_search_backspace`。

## Alternatives considered

只把 dev_envs 的 `delete_forward` 改回 `delete_backward`：一行修复，但保留「每个调用方各写
一份退格」的接口，下一个新搜索框照样可能接错。`on_clear` / `on_escape` 仍由调用方传入，
因为它们确实有页面差异（例如 Esc 先收起浮层）。

## Consequences

四个搜索框（软件、开发环境、文件搜索、进程搜索）的退格和删除行为一致，退格后光标闪烁也会重置。
新增搜索框只需提供 binding，不需要也不能再实现编辑键。

## Verification

- `src/ui/components/search_input.rs::backspace_removes_before_caret_and_delete_after`

Proved: manufactured red —— 把组件里的退格分支换成 `delete_forward` 后 focused 失败，
`"wadaw"` 原样不变，与用户截图的现象一致，见
`docs/agent-notes-evidence/2026-10-12-search-backspace-red.log`；恢复后转绿，见
`docs/agent-notes-evidence/2026-10-12-search-backspace-green.log`。

Pitfalls checked: P1–P58 inspected; P51（进程搜索浮层可见性）相关的 `edited()` / `reopen()`
语义保持不变，未碰卸载、删除、路径保护与搜索语法。新增 P59 记录此坑。
