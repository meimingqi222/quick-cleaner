//! 高质感矢量图标与图标徽标控件库
//! 采用纯矢量组合绘制，告别粗糙 Emoji，呈现现代沉浸式界面

use crate::ui::theme::*;
use gpui::{div, prelude::*, px, rgb, AnyElement, Div};

/// 颜色徽标容器：包裹图标的高级渐变/柔和背景底板
pub fn icon_badge(icon: AnyElement, bg_color: u32, border_color: u32, size: f32) -> Div {
    div()
        .w(px(size))
        .h(px(size))
        .flex_none()
        .rounded_xl()
        .bg(rgb(bg_color))
        .border_1()
        .border_color(rgba(border_color, 0.4))
        .flex()
        .items_center()
        .justify_center()
        .shadow_sm()
        .child(icon)
}

// ---------------------------------------------------------------------------
// 核心矢量图标
// ---------------------------------------------------------------------------

/// 仪表盘/概览图标（2x2 极简 Bento 网格）
pub fn icon_dashboard(fg: u32, size: f32) -> AnyElement {
    let half = (size - 3.) / 2.;
    div()
        .w(px(size))
        .h(px(size))
        .flex()
        .flex_col()
        .justify_between()
        .child(
            div()
                .flex()
                .justify_between()
                .child(div().w(px(half)).h(px(half)).rounded(px(2.)).bg(rgb(fg)))
                .child(div().w(px(half)).h(px(half)).rounded(px(2.)).bg(rgba(fg, 0.6))),
        )
        .child(
            div()
                .flex()
                .justify_between()
                .child(div().w(px(half)).h(px(half)).rounded(px(2.)).bg(rgba(fg, 0.6)))
                .child(div().w(px(half)).h(px(half)).rounded(px(2.)).bg(rgb(fg))),
        )
        .into_any_element()
}

/// 垃圾桶/清理图标
pub fn icon_trash(fg: u32, size: f32) -> AnyElement {
    let w_body = size * 0.75;
    let h_body = size * 0.7;
    let lid_w = size * 0.9;
    div()
        .w(px(size))
        .h(px(size))
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(1.5))
        // 顶盖
        .child(
            div()
                .w(px(lid_w))
                .h(px(2.5))
                .rounded_full()
                .bg(rgb(fg)),
        )
        // 桶身
        .child(
            div()
                .w(px(w_body))
                .h(px(h_body))
                .rounded_b(px(3.))
                .border_2()
                .border_color(rgb(fg))
                .flex()
                .items_center()
                .justify_center()
                .gap(px(2.))
                .child(div().w(px(1.5)).h(px(h_body * 0.55)).rounded_full().bg(rgb(fg)))
                .child(div().w(px(1.5)).h(px(h_body * 0.55)).rounded_full().bg(rgb(fg))),
        )
        .into_any_element()
}

/// 软件/应用九宫格图标
pub fn icon_apps(fg: u32, size: f32) -> AnyElement {
    let dot = (size - 4.) / 3.;
    let row = || {
        div()
            .flex()
            .justify_between()
            .child(div().w(px(dot)).h(px(dot)).rounded(px(1.5)).bg(rgb(fg)))
            .child(div().w(px(dot)).h(px(dot)).rounded(px(1.5)).bg(rgb(fg)))
            .child(div().w(px(dot)).h(px(dot)).rounded(px(1.5)).bg(rgb(fg)))
    };
    div()
        .w(px(size))
        .h(px(size))
        .flex()
        .flex_col()
        .justify_between()
        .child(row())
        .child(row())
        .child(row())
        .into_any_element()
}

/// 磁盘/存储驱动器图标
pub fn icon_disk(fg: u32, size: f32) -> AnyElement {
    div()
        .w(px(size))
        .h(px(size * 0.75))
        .rounded_md()
        .border_2()
        .border_color(rgb(fg))
        .p(px(2.))
        .flex()
        .items_center()
        .justify_between()
        .child(
            div()
                .w(px(size * 0.35))
                .h(px(size * 0.35))
                .rounded_full()
                .border_2()
                .border_color(rgb(fg)),
        )
        .child(
            div()
                .w(px(4.))
                .h(px(4.))
                .rounded_full()
                .bg(rgb(fg)),
        )
        .into_any_element()
}

/// 闪烁星芒/智能扫描图标
pub fn icon_sparkle(fg: u32, size: f32) -> AnyElement {
    let center = size * 0.45;
    div()
        .w(px(size))
        .h(px(size))
        .relative()
        .flex()
        .items_center()
        .justify_center()
        .child(
            div()
                .w(px(center))
                .h(px(center))
                .rounded_sm()
                .bg(rgb(fg)),
        )
        .child(
            div()
                .absolute()
                .w(px(size))
                .h(px(2.))
                .rounded_full()
                .bg(rgb(fg)),
        )
        .child(
            div()
                .absolute()
                .w(px(2.))
                .h(px(size))
                .rounded_full()
                .bg(rgb(fg)),
        )
        .into_any_element()
}

/// 搜索放大镜图标
pub fn icon_search(fg: u32, size: f32) -> AnyElement {
    let r = size * 0.65;
    div()
        .w(px(size))
        .h(px(size))
        .relative()
        .child(
            div()
                .w(px(r))
                .h(px(r))
                .rounded_full()
                .border_2()
                .border_color(rgb(fg)),
        )
        .child(
            div()
                .absolute()
                .right(px(1.))
                .bottom(px(1.))
                .w(px(size * 0.45))
                .h(px(2.))
                .rounded_full()
                .bg(rgb(fg)),
        )
        .into_any_element()
}

/// 对勾/完成图标
pub fn icon_check(fg: u32, size: f32) -> AnyElement {
    div()
        .w(px(size))
        .h(px(size))
        .flex()
        .items_center()
        .justify_center()
        .child(
            div()
                .w(px(size * 0.7))
                .h(px(size * 0.4))
                .border_b_2()
                .border_l_2()
                .border_color(rgb(fg)),
        )
        .into_any_element()
}

/// 盾牌/管理员提权图标
pub fn icon_shield(fg: u32, size: f32) -> AnyElement {
    div()
        .w(px(size))
        .h(px(size))
        .rounded_t_md()
        .rounded_b_xl()
        .border_2()
        .border_color(rgb(fg))
        .flex()
        .items_center()
        .justify_center()
        .child(
            div()
                .w(px(size * 0.35))
                .h(px(size * 0.35))
                .rounded_sm()
                .bg(rgb(fg)),
        )
        .into_any_element()
}

/// 时钟/最后使用/历史图标
pub fn icon_clock(fg: u32, size: f32) -> AnyElement {
    div()
        .w(px(size))
        .h(px(size))
        .rounded_full()
        .border_2()
        .border_color(rgb(fg))
        .relative()
        .flex()
        .items_center()
        .justify_center()
        .child(
            div()
                .w(px(1.5))
                .h(px(size * 0.35))
                .bg(rgb(fg))
                .rounded_full(),
        )
        .into_any_element()
}

/// 设置/齿轮图标
pub fn icon_gear(fg: u32, size: f32) -> AnyElement {
    div()
        .w(px(size))
        .h(px(size))
        .rounded_full()
        .border_2()
        .border_color(rgb(fg))
        .flex()
        .items_center()
        .justify_center()
        .child(
            div()
                .w(px(size * 0.4))
                .h(px(size * 0.4))
                .rounded_full()
                .bg(rgb(fg)),
        )
        .into_any_element()
}

/// 帮助/问号图标
pub fn icon_help(fg: u32, size: f32) -> AnyElement {
    div()
        .w(px(size))
        .h(px(size))
        .rounded_full()
        .border_2()
        .border_color(rgb(fg))
        .flex()
        .items_center()
        .justify_center()
        .text_xs()
        .font_weight(gpui::FontWeight::BOLD)
        .text_color(rgb(fg))
        .child("?")
        .into_any_element()
}

/// 铃铛/通知图标
pub fn icon_bell(fg: u32, size: f32) -> AnyElement {
    div()
        .w(px(size))
        .h(px(size))
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(1.))
        .child(
            div()
                .w(px(size * 0.7))
                .h(px(size * 0.6))
                .rounded_t_full()
                .border_2()
                .border_color(rgb(fg)),
        )
        .child(
            div()
                .w(px(3.))
                .h(px(2.))
                .rounded_full()
                .bg(rgb(fg)),
        )
        .into_any_element()
}

/// 刷新/重试图标
pub fn icon_refresh(fg: u32, size: f32) -> AnyElement {
    div()
        .w(px(size))
        .h(px(size))
        .rounded_full()
        .border_2()
        .border_t_0()
        .border_color(rgb(fg))
        .relative()
        .into_any_element()
}
