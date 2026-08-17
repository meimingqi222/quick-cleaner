//! 进度条与扫描动画控件

use crate::core::model::{commas, fmt_size, truncate};
use crate::ui::components::buttons::ghost_button;
use crate::ui::i18n::*;
use crate::ui::theme::*;
use crate::ui::Root;
use gpui::{
    div, prelude::*, px, rgb, Animation, AnimationExt as _, Context, Div, IntoElement, SharedString,
};
use std::time::Duration;

/// 顶部扫描进度指示条（循环动画）
pub fn render_scan_line() -> Div {
    div()
        .flex_none()
        .w_full()
        .h(px(3.))
        .bg(rgb(SURF_HIGHEST))
        .child(div().h_full().bg(rgb(PRIMARY)).with_animation(
            SharedString::from("scan-progress"),
            Animation::new(Duration::from_millis(1400)).repeat(),
            |bar, delta| {
                let w = (delta * 2.0).fract() * 100.0;
                bar.w(px(w))
            },
        ))
}

/// 清理进行中的全局操作进度条
pub fn render_progress_bar(root: &Root, cx: &mut Context<Root>) -> impl IntoElement {
    const TRACK: f32 = 720.;

    let lang = root.language;
    let snap = root.clean_snapshot().unwrap_or_default();
    let ratio = snap.ratio();
    let cancelling = snap.cancelled;

    let counts = if snap.total_files > 0 {
        tr_file_progress(lang, &commas(snap.files), &commas(snap.total_files))
    } else {
        tr_file_count(lang, &commas(snap.files))
    };
    let bytes = if snap.total_bytes > 0 {
        format!("{} / {}", fmt_size(snap.bytes), fmt_size(snap.total_bytes))
    } else {
        fmt_size(snap.bytes)
    };

    div()
        .flex_none()
        .w_full()
        .px_8()
        .py_3()
        .bg(rgb(BG))
        .border_t_1()
        .border_color(rgba(OUTLINE_VAR, 0.6))
        .flex()
        .justify_center()
        .child(
            div()
                .w(px(TRACK))
                .flex()
                .flex_col()
                .gap_2()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_3()
                        .child(
                            div()
                                .text_sm()
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .text_color(rgb(if cancelling { OUTLINE } else { ERROR }))
                                .child(tr_clean_phase(lang, cancelling)),
                        )
                        .child(div().flex_1())
                        .child(div().text_xs().text_color(rgb(MUTED)).child(counts))
                        .when(snap.failed > 0, |d| {
                            d.child(
                                div()
                                    .text_xs()
                                    .text_color(rgb(ERROR))
                                    .child(tr_failed_count(lang, &commas(snap.failed))),
                            )
                        }),
                )
                // 进度槽
                .child(
                    div()
                        .w(px(TRACK))
                        .h(px(8.))
                        .rounded_full()
                        .bg(rgb(SURF_HIGHEST))
                        .child(
                            div()
                                .h_full()
                                .w(px((TRACK * ratio).max(2.)))
                                .rounded_full()
                                .bg(rgb(if cancelling { OUTLINE } else { PRIMARY })),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_3()
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(0.))
                                .overflow_hidden()
                                .text_xs()
                                .text_color(rgb(OUTLINE))
                                .child(truncate(&snap.current, 72)),
                        )
                        .child(
                            div()
                                .text_xs()
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .text_color(rgb(TEXT))
                                .child(bytes),
                        )
                        .child(
                            div()
                                .id("cancel-clean")
                                .child(ghost_button(String::from(tr_btn_stop(lang)), !cancelling))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.cancel_clean(cx);
                                })),
                        ),
                ),
        )
}
