//! 复选框与标题通用控制控件

use crate::ui::theme::*;
use gpui::{div, prelude::*, px, rgb, Div};

/// 复选框的显示状态，与业务模型无关；业务侧在 adapters 里转换。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CheckState {
    Unchecked,
    Checked,
    Partial,
}

/// 三态复选框
pub fn checkbox(state: CheckState) -> Div {
    let base = div()
        .w(px(20.))
        .h(px(20.))
        .flex_none()
        .rounded_md()
        .flex()
        .items_center()
        .justify_center()
        .text_xs()
        .font_weight(gpui::FontWeight::BOLD);

    match state {
        CheckState::Unchecked => base
            .border_2()
            .border_color(rgb(OUTLINE))
            .text_color(rgba(TEXT, 0.)),
        CheckState::Checked => base
            .bg(rgb(PRIMARY))
            .border_2()
            .border_color(rgb(PRIMARY))
            .text_color(rgb(ON_PRIMARY))
            .child("✓"),
        CheckState::Partial => base
            .bg(rgb(PRIMARY))
            .border_2()
            .border_color(rgb(PRIMARY))
            .text_color(rgb(ON_PRIMARY))
            .child("−"),
    }
}

/// 页面标题区：大标题 + 副标题
pub fn page_heading(title: &str, subtitle: &str) -> Div {
    div()
        .flex()
        .flex_col()
        .gap_0p5()
        .child(
            div()
                .text_lg()
                .font_weight(gpui::FontWeight::BOLD)
                .text_color(rgb(TEXT))
                .child(title.to_string()),
        )
        .child(
            div()
                .text_xs()
                .text_color(rgb(MUTED))
                .child(subtitle.to_string()),
        )
}
