//! 通用搜索输入框组件
//!
//! Apps 搜索和文件搜索共用此组件，差异通过 `SearchBoxSpec` 回调桥接。
//! IME（输入法）处理由 `EntityInputHandler`（`ui::text_input`）统一接管，
//! 采用 GPUI 底层平台字体引擎（DirectWrite / CoreText）进行真实排版与精确命中测试。

use crate::ui::components::icons::icon_search;
use crate::ui::text_input::{
    clamp_to_boundary, index_for_mouse_x, paint_search_text, SearchTextPaint,
};
use crate::ui::theme::*;
use gpui::{
    div, prelude::*, px, rgb, Bounds, Context, DispatchPhase, Div, FocusHandle, KeyDownEvent,
    MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, SharedString, Stateful,
};

/// 搜索框外观规格
pub struct SearchBoxSpec<'a> {
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
    /// true = 文件搜索框，false = Apps 搜索框
    pub is_file_search: bool,
}

/// 渲染一个搜索框。
///
/// `on_clear` / `on_backspace` / `on_escape` 是三个编辑动作的回调，
/// `on_bounds` 在 IME canvas paint 时被调用，用来存储搜索框位置给输入法候选窗口。
pub fn search_box(
    spec: SearchBoxSpec,
    on_clear: impl Fn(&mut crate::ui::Root, &mut Context<crate::ui::Root>) + 'static,
    on_backspace: impl Fn(&mut crate::ui::Root, &mut Context<crate::ui::Root>) + 'static,
    on_escape: impl Fn(&mut crate::ui::Root, &mut Context<crate::ui::Root>) + 'static,
    on_bounds: impl Fn(&mut crate::ui::Root, Bounds<Pixels>) + 'static,
    cx: &mut Context<crate::ui::Root>,
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
        is_file_search,
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
                        if is_file_search {
                            this.search.text_hit = Some(hit);
                        } else {
                            this.apps.text_hit = Some(hit);
                        }
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
                        this.poke_cursor_blink(cx);
                    })),
            )
        })
        // 鼠标按下：聚焦 + 基于真实字体引擎精准定位光标 + 双击全选 + 开始拖拽选区
        .on_mouse_down(MouseButton::Left, {
            let fh = focus_handle.clone();
            cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                fh.focus(window);
                if event.click_count >= 2 {
                    if is_file_search {
                        let len = this.search.query.len();
                        this.search.sel = 0..len;
                        this.search.text_drag = None;
                        this.search.marked = None;
                    } else {
                        let len = this.apps.search.len();
                        this.apps.search_sel = 0..len;
                        this.apps.text_drag = None;
                        this.apps.search_marked = None;
                    }
                    this.poke_cursor_blink(cx);
                    return;
                }
                let mouse_x: f32 = event.position.x.into();
                if is_file_search {
                    let idx = index_for_mouse_x(
                        &this.search.query,
                        mouse_x,
                        this.search.text_hit.as_ref(),
                        font_size,
                        window,
                    );
                    this.search.sel = idx..idx;
                    this.search.text_drag = Some(idx);
                    this.search.marked = None;
                } else {
                    let idx = index_for_mouse_x(
                        &this.apps.search,
                        mouse_x,
                        this.apps.text_hit.as_ref(),
                        font_size,
                        window,
                    );
                    this.apps.search_sel = idx..idx;
                    this.apps.text_drag = Some(idx);
                    this.apps.search_marked = None;
                }
                this.poke_cursor_blink(cx);
            })
        })
        .on_key_down(cx.listener(move |this, event: &KeyDownEvent, _window, cx| {
            let ctrl = event.keystroke.modifiers.control || event.keystroke.modifiers.platform;
            let shift = event.keystroke.modifiers.shift;
            match event.keystroke.key.as_str() {
                "backspace" => on_backspace(this, cx),
                "delete" => {
                    if is_file_search {
                        this.file_search_delete(cx);
                    } else {
                        this.apps_search_delete();
                        cx.notify();
                    }
                }
                "escape" => on_escape(this, cx),
                "left" => {
                    if is_file_search {
                        this.file_search_move_left(shift, cx);
                    } else {
                        this.apps_search_move_left(shift);
                        cx.notify();
                    }
                }
                "right" => {
                    if is_file_search {
                        this.file_search_move_right(shift, cx);
                    } else {
                        this.apps_search_move_right(shift);
                        cx.notify();
                    }
                }
                "home" => {
                    if is_file_search {
                        this.file_search_move_home(shift, cx);
                    } else {
                        this.apps_search_move_home(shift);
                        cx.notify();
                    }
                }
                "end" => {
                    if is_file_search {
                        this.file_search_move_end(shift, cx);
                    } else {
                        this.apps_search_move_end(shift);
                        cx.notify();
                    }
                }
                // Ctrl+A：全选
                "a" if ctrl => {
                    if is_file_search {
                        let len = this.search.query.len();
                        this.search.sel = 0..len;
                    } else {
                        let len = this.apps.search.len();
                        this.apps.search_sel = 0..len;
                    }
                    cx.notify();
                }
                // Ctrl+C：复制选中文本
                "c" if ctrl => {
                    let text = if is_file_search {
                        let sel = clamp_to_boundary(&this.search.query, this.search.sel.clone());
                        this.search.query[sel].to_string()
                    } else {
                        let sel =
                            clamp_to_boundary(&this.apps.search, this.apps.search_sel.clone());
                        this.apps.search[sel].to_string()
                    };
                    if !text.is_empty() {
                        cx.write_to_clipboard(gpui::ClipboardItem::new_string(text));
                    }
                }
                // Ctrl+X：剪切选中文本
                "x" if ctrl => {
                    if is_file_search {
                        let sel = clamp_to_boundary(&this.search.query, this.search.sel.clone());
                        let cut = this.search.query[sel.clone()].to_string();
                        if !cut.is_empty() {
                            cx.write_to_clipboard(gpui::ClipboardItem::new_string(cut));
                            this.search.query.replace_range(sel.clone(), "");
                            this.search.sel = sel.start..sel.start;
                            this.search.marked = None;
                            this.search_input_changed(cx);
                        }
                    } else {
                        let sel =
                            clamp_to_boundary(&this.apps.search, this.apps.search_sel.clone());
                        let cut = this.apps.search[sel.clone()].to_string();
                        if !cut.is_empty() {
                            cx.write_to_clipboard(gpui::ClipboardItem::new_string(cut));
                            this.apps.search.replace_range(sel.clone(), "");
                            this.apps.search_sel = sel.start..sel.start;
                            this.apps.search_marked = None;
                            cx.notify();
                        }
                    }
                }
                // Ctrl+V：粘贴
                "v" if ctrl => {
                    if let Some(item) = cx.read_from_clipboard() {
                        if let Some(pasted) = item.text() {
                            if is_file_search {
                                let sel =
                                    clamp_to_boundary(&this.search.query, this.search.sel.clone());
                                this.search.query.replace_range(sel.clone(), &pasted);
                                let caret = sel.start + pasted.len();
                                this.search.sel = caret..caret;
                                this.search.marked = None;
                                this.search_input_changed(cx);
                            } else {
                                let sel = clamp_to_boundary(
                                    &this.apps.search,
                                    this.apps.search_sel.clone(),
                                );
                                this.apps.search.replace_range(sel.clone(), &pasted);
                                let caret = sel.start + pasted.len();
                                this.apps.search_sel = caret..caret;
                                this.apps.search_marked = None;
                                cx.notify();
                            }
                        }
                    }
                }
                _ => {}
            }
            this.poke_cursor_blink(cx);
        }))
        // IME canvas：把 EntityInputHandler 挂到焦点上，同时注册
        // 窗口级鼠标拖拽/抬起事件来实现文本选区拖拽
        .child(
            gpui::canvas(move |bounds, _window, _cx| bounds, {
                let handle = focus_handle.clone();
                let entity = cx.entity();
                let fs = is_file_search;
                let fsize = font_size;
                move |_, bounds: Bounds<Pixels>, window, cx| {
                    entity.update(cx, |this, _| {
                        on_bounds(this, bounds);
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
                            if fs {
                                if let Some(anchor) = this.search.text_drag {
                                    let cur = index_for_mouse_x(
                                        &this.search.query,
                                        mouse_x,
                                        this.search.text_hit.as_ref(),
                                        fsize,
                                        window,
                                    );
                                    this.search.sel = cur.min(anchor)..cur.max(anchor);
                                    this.poke_cursor_blink(cx);
                                }
                            } else if let Some(anchor) = this.apps.text_drag {
                                let cur = index_for_mouse_x(
                                    &this.apps.search,
                                    mouse_x,
                                    this.apps.text_hit.as_ref(),
                                    fsize,
                                    window,
                                );
                                this.apps.search_sel = cur.min(anchor)..cur.max(anchor);
                                this.poke_cursor_blink(cx);
                            }
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
                            if fs {
                                if this.search.text_drag.is_some() {
                                    this.search.text_drag = None;
                                    cx.notify();
                                }
                            } else if this.apps.text_drag.is_some() {
                                this.apps.text_drag = None;
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
