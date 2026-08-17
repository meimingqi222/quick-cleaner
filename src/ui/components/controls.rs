//! 复选框与标题通用控制控件

use crate::core::model::Check;
use crate::ui::theme::*;
use gpui::{div, prelude::*, px, rgb, Div};

/// 三态复选框
pub fn checkbox(state: Check) -> Div {
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
        Check::Off => base
            .border_2()
            .border_color(rgb(OUTLINE))
            .text_color(rgba(TEXT, 0.)),
        Check::On => base
            .bg(rgb(PRIMARY))
            .border_2()
            .border_color(rgb(PRIMARY))
            .text_color(rgb(ON_PRIMARY))
            .child("✓"),
        Check::Partial => base
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
        .gap_1()
        .child(
            div()
                .text_2xl()
                .font_weight(gpui::FontWeight::BOLD)
                .text_color(rgb(TEXT))
                .child(title.to_string()),
        )
        .child(
            div()
                .text_sm()
                .text_color(rgb(MUTED))
                .child(subtitle.to_string()),
        )
}

/// 状态或类型微型徽章
pub fn badge(text: String, bg_color: u32, text_c: u32) -> Div {
    div()
        .px_2()
        .py(px(2.))
        .rounded_md()
        .bg(rgb(bg_color))
        .text_xs()
        .font_weight(gpui::FontWeight::MEDIUM)
        .text_color(rgb(text_c))
        .child(text)
}

/// 高质感动效加载 Spinner：基于 8 瓣柔光轨道的平滑旋转指示器
pub fn loading_spinner(anim_phase: usize, size: f32, color: u32) -> gpui::AnyElement {
    let count = 8;
    let radius = (size - 12.) / 2.0;
    let center = size / 2.0;

    let mut container = div().w(px(size)).h(px(size)).flex_none().relative();

    // 8 个径向粒子，按当前相位动态计算透明度与大小
    for i in 0..count {
        let angle = (i as f32) * (std::f32::consts::PI * 2.0 / count as f32);
        let x = center + radius * angle.cos() - 4.0;
        let y = center + radius * angle.sin() - 4.0;

        // 旋转步进
        let shift = (anim_phase + (count - i)) % count;
        let opacity = match shift {
            0 => 1.0,
            1 => 0.85,
            2 => 0.65,
            3 => 0.45,
            4 => 0.30,
            5 => 0.20,
            6 => 0.12,
            _ => 0.06,
        };
        let dot_size = if shift <= 1 {
            8.0
        } else if shift <= 3 {
            7.0
        } else {
            6.0
        };

        container = container.child(
            div()
                .absolute()
                .left(px(x))
                .top(px(y))
                .w(px(dot_size))
                .h(px(dot_size))
                .rounded_full()
                .bg(rgba(color, opacity)),
        );
    }

    // 中心柔和微光呼吸球
    let pulse_opacity =
        0.12 + 0.10 * (((anim_phase % 12) as f32 / 12.0) * std::f32::consts::PI).sin();
    container = container.child(
        div()
            .absolute()
            .left(px(center - 14.0))
            .top(px(center - 14.0))
            .w(px(28.0))
            .h(px(28.0))
            .rounded_full()
            .bg(rgba(color, pulse_opacity)),
    );

    container.into_any_element()
}

/// 统一的高端加载状态视图组件
pub fn loading_state_view(title: &str, subtitle: &str, anim_phase: usize) -> gpui::AnyElement {
    let dot_count = (anim_phase / 4) % 4;
    let dots = match dot_count {
        1 => ".  ",
        2 => ".. ",
        3 => "...",
        _ => "   ",
    };

    div()
        .flex_1()
        .size_full()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap_4()
        .p_12()
        .child(loading_spinner(anim_phase, 64.0, PRIMARY))
        .child(
            div()
                .text_lg()
                .font_weight(gpui::FontWeight::BOLD)
                .text_color(rgb(PRIMARY))
                .child(format!("{title}{dots}")),
        )
        .child(
            div()
                .text_xs()
                .font_weight(gpui::FontWeight::NORMAL)
                .text_color(rgb(MUTED))
                .child(subtitle.to_string()),
        )
        .into_any_element()
}
