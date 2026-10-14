# Agent Note: 搜索选区必须保留锚点和活动光标

Status: implemented

## Problem

四个搜索框共用的 move_left/move_right 将选区始终排序，Shift+左只扩大左端，
Shift+右只扩大右端。在 a|bc 处按 Shift+右再 Shift+左得到 0..2，不能缩回原光标。
鼠标反向拖选同样丢失方向，原生输入接口始终报告 reversed=false。

## Decision

`src/ui/components/input.rs` 的 TextInputState.sel 保存有向字节端点 anchor..caret，允许
start > end。`src/ui/text_input.rs` 的移动函数独立钳住端点，Shift 只移动 caret，
Home/End 保留 anchor；非 Shift 按规范化选区收至对应侧。
`src/ui/components/search_input.rs` 的鼠标拖选保存 anchor..cur。
切片和绘制仍使用 selection()/clamp_to_boundary 得到规范化范围。
utf16_selection 同时提供规范化 UTF-16 范围和方向，由 `src/ui/shell/input.rs` 转发。
编辑、组合输入和清空沿用现有范围规范化及光标重置。

## Alternatives considered

单独增加锚点字段需要每次编辑、外部赋值、拖选和清空同步维护，容易留下陈旧锚点。
只改方向键会让反向鼠标选区及原生输入法仍丢方向。
把 sel 重新排序看似符合普通 Range 语义，却丢失下一次 Shift 操作所需的信息。

## Consequences

键盘可缩小选区、跨过锚点再向另一侧扩展，并能接续鼠标选区。
有向 sel 不能直接用于字符串切片；selection() 是规范化入口。所有现有消费点已核对。
非 Shift 收选区、中文/Emoji 字节边界和 UTF-16 输入法替换仍保持正确。

## Verification

- `src/ui/components/search_input.rs::shift_selection_shrinks_on_reverse`
- `src/ui/components/search_input.rs::shift_selection_preserves_anchor_across_unicode_and_home_end`
- `src/ui/components/search_input.rs::reverse_mouse_selection_keeps_anchor_for_shift_arrow`
- `src/ui/components/input.rs::directed_selection_reports_utf16_and_replaces_exact_text`

Proved: 修复前真实 GPUI 键盘事件自然红跑，Shift+右再 Shift+左的断言实际 0..2、期望 1..1，
见 `docs/agent-notes-evidence/2026-10-14-selection-red.log`；恢复方向后同 focused 转绿，见
`docs/agent-notes-evidence/2026-10-14-selection-green.log`。

Pitfalls checked: P1–P59 完整核对；P52 不抢组合输入键盘、P59 组件统一编辑行为保留。
未触及搜索语法、浮层生命周期或清理授权。新增 P61。
