//! 通用搜索输入框组件
//!
//! Apps 搜索和文件搜索共用此组件，差异通过 `SearchBoxSpec` 与 binding 回调桥接。
//! 排版、命中、拖选、编辑键和 IME 转发由 `gpui_app_widgets::input` 统一实现；
//! 这里只决定外观、清空与 Esc 的产品动作。

use crate::ui::components::icons::icon_search;
use crate::ui::components::input::{InputBinding, InputHost, InputPaintState};
use crate::ui::text_input::{clamp_to_boundary, search_paint_style};
use crate::ui::theme::*;
use gpui::{
    div, prelude::*, px, rgb, Context, Div, FocusHandle, KeyDownEvent, MouseButton, MouseDownEvent,
    SharedString, Stateful,
};

/// 搜索框外观规格
pub struct SearchBoxSpec<'a, T: 'static> {
    /// 唯一 DOM id（`div().id(...)`）
    pub id: SharedString,
    /// 焦点句柄
    pub focus_handle: &'a FocusHandle,
    /// 当前文本
    pub text: &'a str,
    /// placeholder 文案
    pub placeholder: SharedString,
    /// 选区范围（字节）
    pub selection: std::ops::Range<usize>,
    /// 组合中文本范围（字节）
    pub marked: Option<std::ops::Range<usize>>,
    /// 宽度
    pub width: f32,
    /// 高度
    pub height: f32,
    /// 文字字号（px）
    pub font_size: f32,
    /// 光标高度（px）
    pub cursor_h: f32,
    /// 是否获得焦点
    pub focused: bool,
    /// 光标是否在闪烁可见相位
    pub cursor_visible: bool,
    /// 目标输入框（Apps / File / Status）
    pub binding: InputBinding<T>,
}

/// 渲染一个搜索框。
///
/// `on_clear` / `on_escape` 由调用方决定（清空可能连带收起浮层等）；退格、删除、
/// 光标移动等纯编辑由共享 binding 统一处理，改完文本走 `binding.changed`。
pub fn search_box<T: 'static>(
    spec: SearchBoxSpec<'_, T>,
    on_clear: impl Fn(&mut T, &mut Context<T>) + 'static,
    on_escape: impl Fn(&mut T, &mut Context<T>) + 'static,
    cx: &mut Context<T>,
) -> Stateful<Div> {
    let SearchBoxSpec {
        id,
        focus_handle,
        text,
        placeholder,
        selection,
        marked,
        width,
        height,
        font_size,
        cursor_h,
        focused,
        cursor_visible,
        binding,
    } = spec;

    // 文字、光标、选区全部由同一份排版画出来，点击命中也用这份排版。
    // 绝不能再用「整框 bounds + 33px」去猜文本起点——padding/图标/字号一变就偏一格。
    let paint = InputPaintState {
        text: text.to_string().into(),
        placeholder,
        sel: clamp_to_boundary(text, selection),
        marked,
        cursor_height: cursor_h,
        focused,
        cursor_visible,
    };
    let text_content = div()
        .relative()
        .flex_1()
        .min_w(px(0.))
        .h_full()
        .overflow_hidden()
        .child(binding.text_element(cx.entity(), search_paint_style(), paint));

    div()
        .id(id)
        .track_focus(focus_handle)
        // 平台输入画布是绝对定位的，需要这个定位上下文
        .relative()
        .w(px(width))
        .h(px(height))
        .px_3()
        .rounded_full()
        .bg(rgb(SURF_LOW))
        .border_1()
        .when(focused, |d| d.border_color(rgb(PRIMARY)).bg(rgb(CARD)))
        .when(!focused, |d| {
            d.border_color(rgba(OUTLINE_VAR, 0.6))
                .hover(|h| h.bg(rgb(SURF_HIGH)))
        })
        .flex()
        .items_center()
        .gap_2()
        .cursor_text()
        .child(icon_search(
            if focused { PRIMARY } else { OUTLINE },
            font_size - 1.,
        ))
        .child(text_content)
        .when(!text.is_empty(), |d| {
            d.child(
                div()
                    .id("clear-btn")
                    .px_1()
                    .rounded_full()
                    .text_xs()
                    .text_color(rgb(OUTLINE))
                    .cursor_pointer()
                    .hover(|h| h.text_color(rgb(ERROR)))
                    .child("✕")
                    .on_click(cx.listener(move |this, _, _, cx| {
                        on_clear(this, cx);
                        binding.activity(this, cx);
                    })),
            )
        })
        // 鼠标按下：聚焦 + 基于真实字体引擎精准定位光标 + 双击全选 + 开始拖拽选区
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                binding.mouse_down(this, event, window, cx);
            }),
        )
        .on_key_down(cx.listener(move |this, event: &KeyDownEvent, window, cx| {
            if (binding.on_key)(this, event, window, cx) {
                cx.stop_propagation();
                return;
            }
            if event.keystroke.key == "escape" {
                on_escape(this, cx);
            } else {
                binding.edit_key(this, &event.keystroke, cx);
            }
            binding.activity(this, cx);
        }))
        // 平台输入画布：把 IME 处理器挂到焦点上，并在窗口级跟随拖拽选区
        .child(binding.platform_element(cx.entity()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::components::input::TextInputState;
    use gpui::{Render, TestAppContext, Window};
    use std::ops::Range;

    struct Field {
        input: TextInputState,
        changes: usize,
    }

    const BINDING: InputBinding<Field> = InputBinding {
        read: |this| &this.input,
        write: |this| &mut this.input,
        on_change: |this, _| this.changes += 1,
        on_activity: |_, _| {},
        on_key: |_, _, _, _| false,
        font_size: 12.,
        font_weight: crate::ui::text_input::SEARCH_FONT_WEIGHT,
    };

    impl Render for Field {
        fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let focus_handle = self.input.focus_handle.clone();
            search_box(
                SearchBoxSpec {
                    id: "field".into(),
                    focus_handle: &focus_handle,
                    text: &self.input.text,
                    placeholder: SharedString::default(),
                    selection: self.input.sel.clone(),
                    marked: None,
                    width: 200.,
                    height: 32.,
                    font_size: 12.,
                    cursor_h: 13.,
                    focused: focus_handle.is_focused(window),
                    cursor_visible: true,
                    binding: BINDING,
                },
                |_, _| {},
                |_, _| {},
                cx,
            )
        }
    }

    fn edit(cx: &mut TestAppContext, text: &str, caret: usize, key: &str) -> (String, usize) {
        let (view, cx) = cx.add_window_view(|_, cx| Field {
            input: TextInputState::new(cx.focus_handle()),
            changes: 0,
        });
        view.update_in(cx, |this, window, cx| {
            this.input.set_text(text);
            this.input.sel = caret..caret;
            this.input.focus_handle.focus(window);
            cx.notify();
        });
        cx.run_until_parked();
        cx.simulate_keystrokes(key);
        view.read_with(cx, |this, _| (this.input.text.clone(), this.changes))
    }

    #[gpui::test]
    fn shift_selection_shrinks_on_reverse(cx: &mut TestAppContext) {
        let (view, cx) = cx.add_window_view(|_, cx| Field {
            input: TextInputState::new(cx.focus_handle()),
            changes: 0,
        });
        view.update_in(cx, |this, window, cx| {
            this.input.set_text("abc");
            this.input.sel = 1..1;
            this.input.focus_handle.focus(window);
            cx.notify();
        });
        cx.run_until_parked();
        cx.simulate_keystrokes("shift-right shift-left");
        view.read_with(cx, |this, _| {
            assert_eq!(
                this.input.sel,
                1..1,
                "reversing Shift-arrow must return to original caret"
            )
        });
    }

    #[gpui::test]
    fn shift_selection_preserves_anchor_across_unicode_and_home_end(cx: &mut TestAppContext) {
        let (view, cx) = cx.add_window_view(|_, cx| Field {
            input: TextInputState::new(cx.focus_handle()),
            changes: 0,
        });
        for (keys, expected) in [
            ("shift-left shift-right", 1..1),
            (
                "shift-right shift-left shift-left",
                Range { start: 1, end: 0 },
            ),
            ("shift-left shift-right shift-right", 1..4),
            ("shift-home shift-end", 1..9),
            ("shift-end shift-home", Range { start: 1, end: 0 }),
            ("shift-left right", 1..1),
            ("shift-right left", 1..1),
        ] {
            view.update_in(cx, |this, window, cx| {
                this.input.set_text("a中😀b");
                this.input.sel = 1..1;
                this.input.focus_handle.focus(window);
                cx.notify();
            });
            cx.run_until_parked();
            cx.simulate_keystrokes(keys);
            view.read_with(cx, |this, _| assert_eq!(this.input.sel, expected, "{keys}"));
        }
        cx.simulate_keystrokes("shift-right backspace");
        view.read_with(cx, |this, _| {
            assert_eq!(this.input.text, "a😀b");
            assert_eq!(this.input.sel, 1..1);
        });
    }

    #[gpui::test]
    fn reverse_mouse_selection_keeps_anchor_for_shift_arrow(cx: &mut TestAppContext) {
        let (view, cx) = cx.add_window_view(|_, cx| Field {
            input: TextInputState::new(cx.focus_handle()),
            changes: 0,
        });
        view.update_in(cx, |this, _, cx| {
            this.input.set_text("abc");
            cx.notify();
        });
        cx.run_until_parked();
        let (anchor, caret) = view.read_with(cx, |this, _| {
            let hit = this
                .input
                .text_hit
                .as_ref()
                .expect("rendered text geometry");
            let y = hit.bounds.center().y;
            (
                gpui::point(hit.bounds.left() + hit.line.x_for_index(2), y),
                gpui::point(hit.bounds.left() + hit.line.x_for_index(1), y),
            )
        });
        cx.simulate_mouse_down(anchor, MouseButton::Left, gpui::Modifiers::default());
        cx.simulate_mouse_move(caret, MouseButton::Left, gpui::Modifiers::default());
        cx.simulate_mouse_up(caret, MouseButton::Left, gpui::Modifiers::default());
        view.read_with(cx, |this, _| {
            assert_eq!(this.input.sel, Range { start: 2, end: 1 })
        });
        cx.simulate_keystrokes("shift-right");
        view.read_with(cx, |this, _| assert_eq!(this.input.sel, 2..2));
    }

    /// 开发环境搜索框曾把退格接到向前删除上：光标在末尾时按退格没反应。
    /// 退格与删除现在由组件统一实现，调用方不再各写一份。
    #[gpui::test]
    fn backspace_removes_before_caret_and_delete_after(cx: &mut TestAppContext) {
        assert_eq!(edit(cx, "wadaw", 5, "backspace"), ("wada".into(), 1));
        assert_eq!(edit(cx, "abc", 1, "backspace"), ("bc".into(), 1));
        assert_eq!(edit(cx, "abc", 1, "delete"), ("ac".into(), 1));
        assert_eq!(edit(cx, "中文", "中".len(), "backspace"), ("文".into(), 1));
    }
}
