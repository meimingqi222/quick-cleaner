//! 卡片与磁贴容器控件

use crate::ui::theme::*;
use gpui::{div, prelude::*, px, rgb, Div};

/// 白色卡片：设计稿里所有内容块的统一容器
pub fn card() -> Div {
    div()
        .rounded_xl()
        .bg(rgb(CARD))
        .border_1()
        .border_color(rgba(OUTLINE_VAR, 0.55))
        .shadow_sm()
}

/// 指标磁贴：小号说明标签在上，大号数值在下，保证不换行与不溢出
pub fn stat_tile(label: &str, value: String, accent: u32) -> Div {
    card()
        .px_4()
        .py_3()
        .flex()
        .flex_col()
        .justify_center()
        .gap(px(2.))
        .child(
            div()
                .text_xs()
                .text_color(rgb(OUTLINE))
                .whitespace_nowrap()
                .child(label.to_string()),
        )
        .child(
            div()
                .text_xl()
                .font_weight(gpui::FontWeight::BOLD)
                .text_color(rgb(accent))
                .whitespace_nowrap()
                .child(value),
        )
}
