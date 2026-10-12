//! 通用搜索输入框组件
//!
//! Apps 搜索和文件搜索共用此组件，差异通过 `SearchBoxSpec` 回调桥接。
//! IME（输入法）处理由 `EntityInputHandler`（`ui::text_input`）统一接管，
//! 采用 GPUI 底层平台字体引擎（DirectWrite / CoreText）进行真实排版与精确命中测试。

use crate::ui::components::icons::icon_search;
use crate::ui::components::input::{InputBinding, InputHost};
use crate::ui::text_input::{
    clamp_to_boundary, index_for_mouse_x, paint_search_text, SearchTextPaint,
};
use crate::ui::theme::*;
use gpui::{
    div, prelude::*, px, rgb, Bounds, Context, DispatchPhase, Div, FocusHandle, KeyDownEvent,
    MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, SharedString, Stateful,
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
/// 光标移动等纯编辑由组件统一处理，改完文本走 `binding.changed`。
/// `on_bounds` 在 IME canvas paint 时被调用，用来存储搜索框位置给输入法候选窗口。
pub fn search_box<T: gpui::EntityInputHandler>(
    spec: SearchBoxSpec<'_, T>,
    on_clear: impl Fn(&mut T, &mut Context<T>) + 'static,
    on_escape: impl Fn(&mut T, &mut Context<T>) + 'static,
    on_bounds: impl Fn(&mut T, Bounds<Pixels>) + 'static,
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

    let sel = clamp_to_boundary(text, selection);

    // 文字、光标、选区全部由同一份 ShapedLine 画出来，点击命中也用这份排版。
    // 绝不能再用「整框 bounds + 33px」去猜文本起点——padding/图标/字号一变就偏一格。
    let paint_text = text.to_string();
    let paint_placeholder = placeholder.clone();
    let paint_sel = sel.clone();
    let paint_marked = marked.clone();
    let text_entity = cx.entity();
    let text_content = div()
        .relative()
        .flex_1()
        .min_w(px(0.))
        .h_full()
        .overflow_hidden()
        .child(
            gpui::canvas(
                |_, _, _| (),
                move |bounds, _, window, cx| {
                    let hit = paint_search_text(
                        window,
                        cx,
                        bounds,
                        SearchTextPaint {
                            text: &paint_text,
                            placeholder: paint_placeholder.as_ref(),
                            sel: &paint_sel,
                            marked: paint_marked.as_ref(),
                            font_size,
                            cursor_h,
                            focused,
                            cursor_visible,
                        },
                    );
                    text_entity.update(cx, |this, _| {
                        binding.input_mut(this).text_hit = Some(hit);
                    });
                },
            )
            .size_full(),
        );

    div()
        .id(id)
        .track_focus(focus_handle)
        // IME canvas 是绝对定位的，需要这个定位上下文
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
        .on_mouse_down(MouseButton::Left, {
            let fh = focus_handle.clone();
            cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                fh.focus(window);
                if event.click_count >= 2 {
                    let input = binding.input_mut(this);
                    input.sel = 0..input.text.len();
                    input.text_drag = None;
                    input.marked = None;
                    binding.activity(this, cx);
                    return;
                }
                let mouse_x: f32 = event.position.x.into();
                let input = binding.input_mut(this);
                let idx = index_for_mouse_x(
                    &input.text,
                    mouse_x,
                    input.text_hit.as_ref(),
                    font_size,
                    window,
                );
                let input = binding.input_mut(this);
                input.sel = idx..idx;
                input.text_drag = Some(idx);
                input.marked = None;
                binding.activity(this, cx);
            })
        })
        .on_key_down(cx.listener(move |this, event: &KeyDownEvent, window, cx| {
            if (binding.on_key)(this, event, window, cx) {
                cx.stop_propagation();
                return;
            }
            let ctrl = event.keystroke.modifiers.control || event.keystroke.modifiers.platform;
            let shift = event.keystroke.modifiers.shift;
            match event.keystroke.key.as_str() {
                "backspace" | "delete" => {
                    let input = binding.input_mut(this);
                    let sel = input.sel.clone();
                    input.sel = if event.keystroke.key == "backspace" {
                        crate::ui::text_input::delete_backward(&mut input.text, sel)
                    } else {
                        crate::ui::text_input::delete_forward(&mut input.text, sel)
                    };
                    input.marked = None;
                    binding.changed(this, cx);
                    binding.activity(this, cx);
                    // on_change 不保证重绘（文件搜索在索引未就绪时直接返回）。
                    cx.notify();
                }
                "escape" => on_escape(this, cx),
                // 光标移动不改文本，两个框都只要重绘
                "left" => {
                    let input = binding.input_mut(this);
                    input.sel =
                        crate::ui::text_input::move_left(&input.text, input.sel.clone(), shift);
                    input.marked = None;
                    cx.notify();
                }
                "right" => {
                    let input = binding.input_mut(this);
                    input.sel =
                        crate::ui::text_input::move_right(&input.text, input.sel.clone(), shift);
                    input.marked = None;
                    cx.notify();
                }
                "home" => {
                    let input = binding.input_mut(this);
                    input.sel =
                        crate::ui::text_input::move_home(&input.text, input.sel.clone(), shift);
                    input.marked = None;
                    cx.notify();
                }
                "end" => {
                    let input = binding.input_mut(this);
                    input.sel =
                        crate::ui::text_input::move_end(&input.text, input.sel.clone(), shift);
                    input.marked = None;
                    cx.notify();
                }
                // Ctrl+A：全选
                "a" if ctrl => {
                    let input = binding.input_mut(this);
                    input.sel = 0..input.text.len();
                    cx.notify();
                }
                // Ctrl+C：复制选中文本
                "c" if ctrl => {
                    let input = binding.input(this);
                    let text = input.text[input.selection()].to_string();
                    if !text.is_empty() {
                        cx.write_to_clipboard(gpui::ClipboardItem::new_string(text));
                    }
                }
                // Ctrl+X：剪切选中文本
                "x" if ctrl => {
                    let input = binding.input_mut(this);
                    let sel = input.selection();
                    let cut = input.text[sel.clone()].to_string();
                    if !cut.is_empty() {
                        cx.write_to_clipboard(gpui::ClipboardItem::new_string(cut));
                        let input = binding.input_mut(this);
                        input.text.replace_range(sel.clone(), "");
                        input.sel = sel.start..sel.start;
                        input.marked = None;
                        binding.changed(this, cx);
                    }
                }
                // Ctrl+V：粘贴
                "v" if ctrl => {
                    if let Some(item) = cx.read_from_clipboard() {
                        if let Some(pasted) = item.text() {
                            // 粘贴替换的是选区，不是 IME 组合中的那段
                            let input = binding.input_mut(this);
                            let sel = input.selection();
                            input.text.replace_range(sel.clone(), &pasted);
                            let caret = sel.start + pasted.len();
                            input.sel = caret..caret;
                            input.marked = None;
                            binding.changed(this, cx);
                        }
                    }
                }
                _ => {}
            }
            binding.activity(this, cx);
        }))
        // IME canvas：把 EntityInputHandler 挂到焦点上，同时注册
        // 窗口级鼠标拖拽/抬起事件来实现文本选区拖拽
        .child(
            gpui::canvas(move |bounds, _window, _cx| bounds, {
                let handle = focus_handle.clone();
                let entity = cx.entity();
                let fsize = font_size;
                move |_, bounds: Bounds<Pixels>, window, cx| {
                    entity.update(cx, |this, _| {
                        on_bounds(this, bounds);
                        (binding.register)(this, binding, window);
                    });
                    window.handle_input(
                        &handle,
                        gpui::ElementInputHandler::new(bounds, entity.clone()),
                        cx,
                    );

                    // 鼠标拖拽：更新选区
                    let ent = entity.clone();
                    window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
                        if phase != DispatchPhase::Bubble {
                            return;
                        }
                        if event.pressed_button != Some(MouseButton::Left) {
                            return;
                        }
                        let mouse_x: f32 = event.position.x.into();
                        ent.update(cx, |this, cx| {
                            let input = binding.input(this);
                            let Some(anchor) = input.text_drag else {
                                return;
                            };
                            let cur = index_for_mouse_x(
                                &input.text,
                                mouse_x,
                                input.text_hit.as_ref(),
                                fsize,
                                window,
                            );
                            binding.input_mut(this).sel = cur.min(anchor)..cur.max(anchor);
                            binding.activity(this, cx);
                        });
                    });

                    // 鼠标抬起：结束拖拽
                    let ent2 = entity.clone();
                    window.on_mouse_event(move |event: &MouseUpEvent, phase, _window, cx| {
                        if phase != DispatchPhase::Bubble {
                            return;
                        }
                        if event.button != MouseButton::Left {
                            return;
                        }
                        ent2.update(cx, |this, cx| {
                            if binding.input_mut(this).text_drag.take().is_some() {
                                cx.notify();
                            }
                        });
                    });
                }
            })
            .absolute()
            .size_full(),
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::components::input::TextInputState;
    use gpui::{Render, TestAppContext, UTF16Selection, Window};
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
        register: |_, _, _| {},
        font_size: 12.,
    };

    impl gpui::EntityInputHandler for Field {
        fn text_for_range(
            &mut self,
            _: Range<usize>,
            _: &mut Option<Range<usize>>,
            _: &mut Window,
            _: &mut Context<Self>,
        ) -> Option<String> {
            None
        }
        fn selected_text_range(
            &mut self,
            _: bool,
            _: &mut Window,
            _: &mut Context<Self>,
        ) -> Option<UTF16Selection> {
            None
        }
        fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
            None
        }
        fn unmark_text(&mut self, _: &mut Window, _: &mut Context<Self>) {}
        fn replace_text_in_range(
            &mut self,
            _: Option<Range<usize>>,
            _: &str,
            _: &mut Window,
            _: &mut Context<Self>,
        ) {
        }
        fn replace_and_mark_text_in_range(
            &mut self,
            _: Option<Range<usize>>,
            _: &str,
            _: Option<Range<usize>>,
            _: &mut Window,
            _: &mut Context<Self>,
        ) {
        }
        fn bounds_for_range(
            &mut self,
            _: Range<usize>,
            _: Bounds<Pixels>,
            _: &mut Window,
            _: &mut Context<Self>,
        ) -> Option<Bounds<Pixels>> {
            None
        }
        fn character_index_for_point(
            &mut self,
            _: gpui::Point<Pixels>,
            _: &mut Window,
            _: &mut Context<Self>,
        ) -> Option<usize> {
            None
        }
    }

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
