//! 高质感矢量图标与图标徽标控件库
//! 采用纯矢量组合绘制，告别粗糙 Emoji，呈现现代沉浸式界面

use crate::ui::theme::*;
use gpui::{div, img, prelude::*, px, rgb, AnyElement, Div, Image, ImageFormat, ImageSource};
use std::sync::Arc;

pub fn icon_app_logo(size: f32) -> AnyElement {
    static PNG_BYTES: &[u8] = include_bytes!("../../../assets/icon.png");
    let image = Arc::new(Image::from_bytes(ImageFormat::Png, PNG_BYTES.to_vec()));
    let src = ImageSource::from(image);
    img(src).w(px(size)).h(px(size)).into_any_element()
}
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
                .child(
                    div()
                        .w(px(half))
                        .h(px(half))
                        .rounded(px(2.))
                        .bg(rgba(fg, 0.6)),
                ),
        )
        .child(
            div()
                .flex()
                .justify_between()
                .child(
                    div()
                        .w(px(half))
                        .h(px(half))
                        .rounded(px(2.))
                        .bg(rgba(fg, 0.6)),
                )
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
        .child(div().w(px(lid_w)).h(px(2.5)).rounded_full().bg(rgb(fg)))
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
                .child(
                    div()
                        .w(px(1.5))
                        .h(px(h_body * 0.55))
                        .rounded_full()
                        .bg(rgb(fg)),
                )
                .child(
                    div()
                        .w(px(1.5))
                        .h(px(h_body * 0.55))
                        .rounded_full()
                        .bg(rgb(fg)),
                ),
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
        .child(div().w(px(4.)).h(px(4.)).rounded_full().bg(rgb(fg)))
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
        .child(div().w(px(center)).h(px(center)).rounded_sm().bg(rgb(fg)))
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
        .child(div().w(px(3.)).h(px(2.)).rounded_full().bg(rgb(fg)))
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

/// 下拉箭头/下折角指示图标
pub fn icon_chevron_down(fg: u32, size: f32) -> AnyElement {
    div()
        .w(px(size))
        .h(px(size))
        .flex()
        .items_center()
        .justify_center()
        .text_xs()
        .text_color(rgb(fg))
        .child("▾")
        .into_any_element()
}

/// 右箭头指示图标
pub fn icon_chevron_right(fg: u32, size: f32) -> AnyElement {
    div()
        .w(px(size))
        .h(px(size))
        .flex()
        .items_center()
        .justify_center()
        .text_xs()
        .font_weight(gpui::FontWeight::BOLD)
        .text_color(rgb(fg))
        .child("›")
        .into_any_element()
}

/// 冗余整理 / 文件整理主图标（闪耀星尘与多图层）
pub fn icon_declutter(fg: u32, size: f32) -> AnyElement {
    div()
        .w(px(size))
        .h(px(size))
        .relative()
        .flex()
        .items_center()
        .justify_center()
        .child(
            div()
                .w(px(size * 0.75))
                .h(px(size * 0.75))
                .rounded_md()
                .border_2()
                .border_color(rgb(fg))
                .p(px(2.))
                .flex()
                .flex_col()
                .justify_between()
                .child(div().w_full().h(px(2.)).rounded_full().bg(rgb(fg)))
                .child(
                    div()
                        .w(px(size * 0.35))
                        .h(px(2.))
                        .rounded_full()
                        .bg(rgb(fg)),
                ),
        )
        .child(
            div()
                .absolute()
                .right(px(0.))
                .top(px(0.))
                .w(px(size * 0.35))
                .h(px(size * 0.35))
                .rounded_full()
                .bg(rgb(PRIMARY_BRIGHT)),
        )
        .into_any_element()
}

/// 大型文件文件夹图标
pub fn icon_folder_large(fg: u32, size: f32) -> AnyElement {
    let w = size * 0.85;
    let h = size * 0.65;
    div()
        .w(px(size))
        .h(px(size))
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .child(
            div()
                .w(px(w))
                .flex()
                .child(div().w(px(w * 0.45)).h(px(3.)).rounded_t_sm().bg(rgb(fg))),
        )
        .child(
            div()
                .w(px(w))
                .h(px(h))
                .rounded_b_md()
                .rounded_tr_md()
                .border_2()
                .border_color(rgb(fg))
                .flex()
                .items_center()
                .justify_center()
                .child(div().w(px(w * 0.35)).h(px(2.5)).rounded_full().bg(rgb(fg))),
        )
        .into_any_element()
}

/// 重复文件图标（双重叠重叠文档）
pub fn icon_files_duplicate(fg: u32, size: f32) -> AnyElement {
    let doc_w = size * 0.6;
    let doc_h = size * 0.75;
    div()
        .w(px(size))
        .h(px(size))
        .relative()
        .child(
            // 底层文档
            div()
                .absolute()
                .left(px(1.))
                .top(px(1.))
                .w(px(doc_w))
                .h(px(doc_h))
                .rounded_sm()
                .border_1()
                .border_color(rgba(fg, 0.5))
                .bg(rgba(fg, 0.1)),
        )
        .child(
            // 顶层文档
            div()
                .absolute()
                .right(px(1.))
                .bottom(px(1.))
                .w(px(doc_w))
                .h(px(doc_h))
                .rounded_sm()
                .border_2()
                .border_color(rgb(fg))
                .bg(rgb(CARD))
                .p(px(2.))
                .flex()
                .flex_col()
                .justify_around()
                .child(div().w_full().h(px(1.5)).rounded_full().bg(rgb(fg)))
                .child(
                    div()
                        .w(px(doc_w * 0.6))
                        .h(px(1.5))
                        .rounded_full()
                        .bg(rgb(fg)),
                ),
        )
        .into_any_element()
}

/// 相似图片图标（相框与双峰山脉）
pub fn icon_photos_similar(fg: u32, size: f32) -> AnyElement {
    let w = size * 0.85;
    let h = size * 0.7;
    div()
        .w(px(size))
        .h(px(size))
        .flex()
        .items_center()
        .justify_center()
        .child(
            div()
                .w(px(w))
                .h(px(h))
                .rounded_md()
                .border_2()
                .border_color(rgb(fg))
                .p(px(2.))
                .flex()
                .flex_col()
                .justify_between()
                .child(div().w(px(3.5)).h(px(3.5)).rounded_full().bg(rgb(fg)))
                .child(
                    div()
                        .w_full()
                        .h(px(h * 0.4))
                        .flex()
                        .items_end()
                        .gap(px(2.))
                        .child(
                            div()
                                .w(px(w * 0.35))
                                .h(px(h * 0.35))
                                .rounded_t_sm()
                                .bg(rgba(fg, 0.8)),
                        )
                        .child(
                            div()
                                .w(px(w * 0.45))
                                .h(px(h * 0.25))
                                .rounded_t_sm()
                                .bg(rgb(fg)),
                        ),
                ),
        )
        .into_any_element()
}

/// 下载项图标（向下箭头与托盘）
pub fn icon_downloads(fg: u32, size: f32) -> AnyElement {
    div()
        .w(px(size))
        .h(px(size))
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(2.))
        .child(
            div()
                .flex()
                .flex_col()
                .items_center()
                .child(div().w(px(2.5)).h(px(size * 0.35)).bg(rgb(fg)))
                .child(
                    div()
                        .w(px(size * 0.45))
                        .h(px(size * 0.25))
                        .border_b_2()
                        .border_l_2()
                        .border_color(rgb(fg)),
                ),
        )
        .child(
            div()
                .w(px(size * 0.75))
                .h(px(2.5))
                .rounded_full()
                .bg(rgb(fg)),
        )
        .into_any_element()
}

/// 重量/大文件图标 (Weight/Scale)
pub fn icon_weight(fg: u32, size: f32) -> AnyElement {
    let w = size * 0.7;
    let h = size * 0.65;
    div()
        .w(px(size))
        .h(px(size))
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .child(
            // 提手
            div()
                .w(px(w * 0.45))
                .h(px(size * 0.2))
                .rounded_t_full()
                .border_2()
                .border_color(rgb(fg)),
        )
        .child(
            // 砝码主体
            div()
                .w(px(w))
                .h(px(h))
                .rounded_md()
                .bg(rgb(fg))
                .flex()
                .items_center()
                .justify_center()
                .child(div().w(px(w * 0.4)).h(px(2.)).rounded_full().bg(rgb(CARD))),
        )
        .into_any_element()
}

/// 星星/最佳品质图标 (Star)
pub fn icon_star(fg: u32, size: f32) -> AnyElement {
    div()
        .w(px(size))
        .h(px(size))
        .flex()
        .items_center()
        .justify_center()
        .child(
            div()
                .w(px(size * 0.75))
                .h(px(size * 0.75))
                .rounded_full()
                .bg(rgb(fg))
                .flex()
                .items_center()
                .justify_center()
                .child(
                    div()
                        .w(px(size * 0.35))
                        .h(px(size * 0.35))
                        .bg(rgb(ON_PRIMARY))
                        .rounded_xs(),
                ),
        )
        .into_any_element()
}

/// 火箭/一键智能扫描图标 (Rocket)
pub fn icon_rocket(fg: u32, size: f32) -> AnyElement {
    div()
        .w(px(size))
        .h(px(size))
        .flex()
        .items_center()
        .justify_center()
        .child(
            div()
                .w(px(size * 0.6))
                .h(px(size * 0.8))
                .rounded_t_full()
                .border_2()
                .border_color(rgb(fg))
                .flex()
                .flex_col()
                .items_center()
                .justify_between()
                .py(px(2.))
                .child(div().w(px(3.)).h(px(3.)).rounded_full().bg(rgb(fg)))
                .child(div().w(px(size * 0.35)).h(px(2.)).bg(rgb(fg))),
        )
        .into_any_element()
}

/// 视频文件图标
pub fn icon_video(fg: u32, size: f32) -> AnyElement {
    div()
        .w(px(size))
        .h(px(size))
        .flex()
        .items_center()
        .justify_center()
        .child(
            div()
                .w(px(size * 0.85))
                .h(px(size * 0.65))
                .rounded_md()
                .border_2()
                .border_color(rgb(fg))
                .flex()
                .items_center()
                .justify_between()
                .px(px(2.))
                .child(div().w(px(2.)).h(px(3.)).bg(rgb(fg)))
                .child(
                    div()
                        .w(px(size * 0.3))
                        .h(px(size * 0.3))
                        .rounded_xs()
                        .bg(rgb(fg)),
                )
                .child(div().w(px(2.)).h(px(3.)).bg(rgb(fg))),
        )
        .into_any_element()
}

/// 压缩包图标
pub fn icon_zip(fg: u32, size: f32) -> AnyElement {
    div()
        .w(px(size))
        .h(px(size))
        .flex()
        .items_center()
        .justify_center()
        .child(
            div()
                .w(px(size * 0.7))
                .h(px(size * 0.85))
                .rounded_md()
                .border_2()
                .border_color(rgb(fg))
                .flex()
                .flex_col()
                .items_center()
                .py(px(2.))
                .gap(px(1.5))
                .child(div().w(px(3.)).h(px(1.5)).bg(rgb(fg)))
                .child(div().w(px(3.)).h(px(1.5)).bg(rgb(fg)))
                .child(div().w(px(3.)).h(px(1.5)).bg(rgb(fg)))
                .child(div().w(px(6.)).h(px(4.)).rounded_xs().bg(rgb(fg))),
        )
        .into_any_element()
}

/// 文件夹文件图标
pub fn icon_folder_file(fg: u32, size: f32) -> AnyElement {
    let w = size * 0.88;
    let h = size * 0.65;
    div()
        .w(px(size))
        .h(px(size))
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .child(
            div()
                .w(px(w))
                .flex()
                .child(div().w(px(w * 0.44)).h(px(2.5)).rounded_t_sm().bg(rgb(fg))),
        )
        .child(
            div()
                .w(px(w))
                .h(px(h))
                .rounded_b_sm()
                .rounded_tr_sm()
                .bg(rgba(fg, 0.25))
                .border_1()
                .border_color(rgb(fg)),
        )
        .into_any_element()
}

/// 通用空白文档图标
pub fn icon_file_generic(fg: u32, size: f32) -> AnyElement {
    let w = size * 0.68;
    let h = size * 0.85;
    div()
        .w(px(size))
        .h(px(size))
        .flex()
        .items_center()
        .justify_center()
        .child(
            div()
                .w(px(w))
                .h(px(h))
                .rounded_sm()
                .border_1()
                .border_color(rgb(fg))
                .bg(rgba(fg, 0.15)),
        )
        .into_any_element()
}

/// 文本/文档图标（三条横线）
pub fn icon_file_doc(fg: u32, size: f32) -> AnyElement {
    let w = size * 0.68;
    let h = size * 0.85;
    div()
        .w(px(size))
        .h(px(size))
        .flex()
        .items_center()
        .justify_center()
        .child(
            div()
                .w(px(w))
                .h(px(h))
                .rounded_sm()
                .border_1()
                .border_color(rgb(fg))
                .bg(rgba(fg, 0.15))
                .flex()
                .flex_col()
                .justify_center()
                .px(px(2.))
                .gap(px(1.5))
                .child(div().w_full().h(px(1.2)).rounded_full().bg(rgb(fg)))
                .child(div().w(px(w * 0.75)).h(px(1.2)).rounded_full().bg(rgb(fg)))
                .child(div().w(px(w * 0.5)).h(px(1.2)).rounded_full().bg(rgb(fg))),
        )
        .into_any_element()
}

/// 代码源文件图标（<> 符号）
pub fn icon_file_code(fg: u32, size: f32) -> AnyElement {
    let w = size * 0.72;
    let h = size * 0.85;
    div()
        .w(px(size))
        .h(px(size))
        .flex()
        .items_center()
        .justify_center()
        .child(
            div()
                .w(px(w))
                .h(px(h))
                .rounded_sm()
                .border_1()
                .border_color(rgb(fg))
                .bg(rgba(fg, 0.15))
                .flex()
                .items_center()
                .justify_center()
                .text_xs()
                .font_weight(gpui::FontWeight::BOLD)
                .text_color(rgb(fg))
                .child("<>"),
        )
        .into_any_element()
}

/// 数据库文件图标（多层圆柱体）
pub fn icon_file_database(fg: u32, size: f32) -> AnyElement {
    let w = size * 0.75;
    let h = size * 0.85;
    div()
        .w(px(size))
        .h(px(size))
        .flex()
        .items_center()
        .justify_center()
        .child(
            div()
                .w(px(w))
                .h(px(h))
                .flex()
                .flex_col()
                .justify_between()
                .child(
                    div()
                        .w_full()
                        .h(px(h * 0.28))
                        .rounded_full()
                        .border_1()
                        .border_color(rgb(fg))
                        .bg(rgba(fg, 0.2)),
                )
                .child(
                    div()
                        .w_full()
                        .h(px(h * 0.28))
                        .rounded_b_full()
                        .border_b_1()
                        .border_x_1()
                        .border_color(rgb(fg))
                        .bg(rgba(fg, 0.2)),
                )
                .child(
                    div()
                        .w_full()
                        .h(px(h * 0.28))
                        .rounded_b_full()
                        .border_b_1()
                        .border_x_1()
                        .border_color(rgb(fg))
                        .bg(rgba(fg, 0.2)),
                ),
        )
        .into_any_element()
}

/// 可执行程序/应用图标
pub fn icon_file_exe(fg: u32, size: f32) -> AnyElement {
    let w = size * 0.78;
    let h = size * 0.78;
    div()
        .w(px(size))
        .h(px(size))
        .flex()
        .items_center()
        .justify_center()
        .child(
            div()
                .w(px(w))
                .h(px(h))
                .rounded_md()
                .border_1()
                .border_color(rgb(fg))
                .bg(rgba(fg, 0.2))
                .flex()
                .items_center()
                .justify_center()
                .child(
                    div()
                        .w(px(w * 0.45))
                        .h(px(h * 0.45))
                        .rounded_xs()
                        .bg(rgb(fg)),
                ),
        )
        .into_any_element()
}

/// 定位/文件夹打开指示图标
pub fn icon_locate(fg: u32, size: f32) -> AnyElement {
    div()
        .w(px(size))
        .h(px(size))
        .flex()
        .items_center()
        .justify_center()
        .text_xs()
        .text_color(rgb(fg))
        .child("↗")
        .into_any_element()
}

/// 文件类型视觉分类
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileVisualKind {
    Directory,
    Executable,
    Database,
    Code,
    Document,
    Archive,
    Media,
    Generic,
}

impl FileVisualKind {
    pub fn from_name(name: &str, is_dir: bool) -> Self {
        if is_dir {
            return Self::Directory;
        }
        let ext = name.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
        match ext.as_str() {
            "exe" | "msi" | "bat" | "cmd" | "ps1" | "vbs" | "com" | "dll" | "sys" => {
                Self::Executable
            }
            "db" | "sqlite" | "sqlite3" | "dat" | "bin" | "sql" | "mdb" | "accdb" | "db3"
            | "rdb" => Self::Database,
            "rs" | "go" | "py" | "js" | "ts" | "jsx" | "tsx" | "c" | "cpp" | "h" | "hpp"
            | "java" | "kt" | "swift" | "rb" | "php" | "html" | "css" | "scss" | "json"
            | "yaml" | "yml" | "toml" | "xml" | "md" | "sh" | "lua" | "vue" => Self::Code,
            "pdf" | "doc" | "docx" | "xls" | "xlsx" | "ppt" | "pptx" | "txt" | "log" | "rtf"
            | "csv" | "ini" | "cfg" | "conf" => Self::Document,
            "zip" | "7z" | "rar" | "tar" | "gz" | "bz2" | "xz" | "iso" | "cab" | "tgz" => {
                Self::Archive
            }
            "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" | "ico" | "bmp" | "mp4" | "mkv"
            | "avi" | "mov" | "mp3" | "wav" | "flac" | "aac" | "webm" => Self::Media,
            _ => Self::Generic,
        }
    }

    /// 返回该类型对应的精致原生矢量图标（无厚重背景底板，极简克制）
    pub fn badge(&self, size: f32) -> AnyElement {
        let fg = match self {
            Self::Directory => 0xD97706,   // 琥珀暖金
            Self::Executable => 0x7C3AED,  // 科技深紫
            Self::Database => 0x0891B2,    // 沉稳青碧
            Self::Code => 0x059669,        // 翠绿源码
            Self::Document => 0x2563EB,    // 经典文档蓝
            Self::Archive => 0xC2410C,     // 暖橙归档
            Self::Media => 0xDB2777,       // 玫红媒体
            Self::Generic => 0x64748B,     // 优雅中性灰
        };
        let icon = match self {
            Self::Directory => icon_folder_file(fg, size),
            Self::Executable => icon_file_exe(fg, size),
            Self::Database => icon_file_database(fg, size),
            Self::Code => icon_file_code(fg, size),
            Self::Document => icon_file_doc(fg, size),
            Self::Archive => icon_zip(fg, size),
            Self::Media => icon_photos_similar(fg, size),
            Self::Generic => icon_file_generic(fg, size),
        };
        div()
            .w(px(size))
            .h(px(size))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .child(icon)
            .into_any_element()
    }
}

