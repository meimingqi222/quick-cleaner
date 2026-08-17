//! 进度条与扫描动画控件

use crate::core::model::{commas, fmt_size, truncate};
use crate::ui::components::buttons::ghost_button;
use crate::ui::components::controls::loading_spinner;
use crate::ui::components::icons::{icon_apps, icon_badge, icon_check};
use crate::ui::i18n::*;
use crate::ui::theme::*;
use crate::ui::{Root, UninstallPhase};
use gpui::{
    div, prelude::*, px, rgb, Animation, AnimationExt as _, AnyElement, Context, Div, IntoElement,
    SharedString,
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

/// 软件卸载期间的专用过程页。阶段来自后台任务的原子状态，已有的 50ms tick
/// 负责推动 spinner 并让阶段切换及时反映到界面。
pub fn render_uninstall_progress(root: &Root) -> AnyElement {
    const TRACK: f32 = 420.;

    let Some(progress) = root.residual.uninstall.as_ref() else {
        return div().into_any_element();
    };
    let lang = root.language;
    let phase = progress.phase();
    let phase_index = phase as usize;
    let ratio = match phase {
        UninstallPhase::Discovering => 0.24,
        UninstallPhase::Removing => 0.64,
        UninstallPhase::Verifying => 0.92,
    };
    let phase_description = match phase {
        UninstallPhase::Discovering => tr_uninstall_phase_discovering(lang),
        UninstallPhase::Removing => tr_uninstall_phase_removing(lang),
        UninstallPhase::Verifying => tr_uninstall_phase_verifying(lang),
    };
    let stages = [
        tr_uninstall_stage_discover(lang),
        tr_uninstall_stage_remove(lang),
        tr_uninstall_stage_verify(lang),
    ];

    let stage_rows = stages.into_iter().enumerate().map(|(index, label)| {
        let indicator = if index < phase_index {
            div()
                .w(px(24.))
                .h(px(24.))
                .rounded_full()
                .bg(rgb(PRIMARY))
                .flex()
                .items_center()
                .justify_center()
                .child(icon_check(ON_PRIMARY, 12.))
                .into_any_element()
        } else if index == phase_index {
            loading_spinner(root.anim_phase, 24., PRIMARY)
        } else {
            div()
                .w(px(24.))
                .h(px(24.))
                .rounded_full()
                .border_1()
                .border_color(rgb(OUTLINE_VAR))
                .into_any_element()
        };

        div()
            .h(px(36.))
            .flex()
            .items_center()
            .gap_3()
            .child(indicator)
            .child(
                div()
                    .text_sm()
                    .font_weight(if index == phase_index {
                        gpui::FontWeight::SEMIBOLD
                    } else {
                        gpui::FontWeight::NORMAL
                    })
                    .text_color(rgb(if index <= phase_index { TEXT } else { OUTLINE }))
                    .child(label),
            )
    });

    div()
        .size_full()
        .bg(rgb(CARD))
        .flex()
        .items_center()
        .justify_center()
        .p_10()
        .child(
            div()
                .w(px(560.))
                .flex()
                .flex_col()
                .items_center()
                .gap_5()
                .child(icon_badge(
                    icon_apps(PRIMARY, 28.),
                    PRIMARY_FIXED,
                    PRIMARY,
                    64.,
                ))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .items_center()
                        .gap_2()
                        .child(
                            div()
                                .text_2xl()
                                .font_weight(gpui::FontWeight::BOLD)
                                .text_color(rgb(TEXT))
                                .child(tr_uninstall_progress_title(lang, &progress.app_name)),
                        )
                        .child(
                            div()
                                .text_sm()
                                .text_color(rgb(MUTED))
                                .child(phase_description),
                        ),
                )
                .child(
                    div()
                        .w(px(TRACK))
                        .h(px(6.))
                        .rounded_full()
                        .bg(rgb(SURF_HIGHEST))
                        .child(
                            div()
                                .h_full()
                                .w(px(TRACK * ratio))
                                .rounded_full()
                                .bg(rgb(PRIMARY)),
                        ),
                )
                .child(
                    div()
                        .w(px(TRACK))
                        .py_2()
                        .flex()
                        .flex_col()
                        .children(stage_rows),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(rgb(OUTLINE))
                        .child(tr_uninstall_keep_open(lang)),
                ),
        )
        .into_any_element()
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
