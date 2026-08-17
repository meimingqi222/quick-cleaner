//! 按钮控件库

use crate::ui::theme::*;
use gpui::{div, prelude::*, px, rgb, Div};

/// 主按钮（实心蓝色胶囊）
pub fn primary_button(label: String, enabled: bool) -> Div {
    div()
        .px_5()
        .py_2()
        .rounded_full()
        .bg(rgb(PRIMARY))
        .text_sm()
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .text_color(rgb(ON_PRIMARY))
        .flex()
        .items_center()
        .justify_center()
        .when(enabled, |d| {
            d.cursor_pointer().hover(|h| h.bg(rgb(PRIMARY_BRIGHT)))
        })
        .when(!enabled, |d| d.opacity(0.38))
        .child(label)
}

/// 危险按钮（用在永久删除等不可逆操作上）
pub fn danger_button(label: String, enabled: bool) -> Div {
    div()
        .px_5()
        .py_2()
        .rounded_full()
        .bg(rgb(ERROR))
        .text_sm()
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .text_color(rgb(ON_PRIMARY))
        .flex()
        .items_center()
        .justify_center()
        .when(enabled, |d| {
            d.cursor_pointer().hover(|h| h.bg(rgba(ERROR, 0.85)))
        })
        .when(!enabled, |d| d.opacity(0.38))
        .child(label)
}

/// 次按钮（描边胶囊）
pub fn ghost_button(label: String, enabled: bool) -> Div {
    div()
        .px_4()
        .py_2()
        .rounded_full()
        .border_1()
        .border_color(rgb(OUTLINE_VAR))
        .text_sm()
        .text_color(rgb(MUTED))
        .flex()
        .items_center()
        .justify_center()
        .when(enabled, |d| {
            d.cursor_pointer()
                .hover(|h| h.bg(rgb(SURF_LOW)).text_color(rgb(PRIMARY)))
        })
        .when(!enabled, |d| d.opacity(0.38))
        .child(label)
}

/// 小型操作按钮
pub fn small_button(label: String, bg_color: u32, text_c: u32, enabled: bool) -> Div {
    div()
        .px_3()
        .py(px(4.))
        .rounded_md()
        .bg(rgb(bg_color))
        .border_1()
        .border_color(rgba(OUTLINE_VAR, 0.6))
        .text_xs()
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .text_color(rgb(text_c))
        .flex()
        .items_center()
        .justify_center()
        .when(enabled, |d| {
            d.cursor_pointer()
                .hover(|h| h.border_color(rgba(PRIMARY, 0.8)).opacity(0.9))
        })
        .when(!enabled, |d| d.opacity(0.38))
        .child(label)
}
