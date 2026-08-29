//! 顶部应用操作栏 (CleanFlow 质感顶栏)

use crate::core::model::fmt_size;
use crate::ui::components::buttons::ghost_button;
use crate::ui::components::icons::*;
use crate::ui::components::sidebar::View;
use crate::ui::i18n::*;
use crate::ui::theme::*;
use crate::ui::Root;
use gpui::{div, prelude::*, px, rgb, Context, IntoElement};

pub fn render_top_bar(root: &Root, cx: &mut Context<Root>) -> impl IntoElement {
    let lang = root.language;
    let is_apps_busy = root.apps.scanning || root.residual.scanning;
    let is_mft_busy = root.disk.scanning;
    let is_junk_busy = root.junk.scanning || root.clean.running;
    let is_declutter_busy = root.declutter.scanning;

    let (busy, label) = match root.view {
        View::Dashboard | View::Junk => (is_junk_busy, tr_btn_rescan(lang, is_junk_busy)),
        // 状态页的采样常驻后台，按钮永远可点（只是对齐一次节拍）
        View::Status => (false, tr_btn_rescan(lang, false)),
        View::Apps => (is_apps_busy, tr_btn_refresh_apps(lang, is_apps_busy)),
        View::Disk => (is_mft_busy, tr_btn_reanalyze_disk(lang, is_mft_busy)),
        View::Declutter => (is_declutter_busy, tr_btn_rescan(lang, is_declutter_busy)),
        View::Search => (
            root.search.indexing,
            tr_btn_rescan(lang, root.search.indexing),
        ),
    };

    let title_area = div().flex().items_center().gap_3().child(
        div()
            .text_lg()
            .font_weight(gpui::FontWeight::BOLD)
            .text_color(rgb(TEXT))
            .child(root.view.title_lang(lang)),
    );

    #[cfg(target_os = "macos")]
    let title_area = {
        let is_granted = root.fda_status;
        title_area.child(
            div()
                .id("fda-status-badge")
                .px_2()
                .py(px(2.))
                .rounded_full()
                .border_1()
                .when(is_granted, |d| {
                    d.bg(rgb(PRIMARY_FIXED))
                        .border_color(rgb(PRIMARY))
                        .text_color(rgb(PRIMARY))
                })
                .when(!is_granted, |d| {
                    d.bg(rgb(CAUTION_CONTAINER))
                        .border_color(rgba(CAUTION, 0.5))
                        .text_color(rgb(CAUTION))
                        .cursor_pointer()
                        .hover(|h| h.opacity(0.85))
                })
                .text_xs()
                .font_weight(gpui::FontWeight::MEDIUM)
                .flex()
                .items_center()
                .gap_1()
                .child(icon_shield(if is_granted { PRIMARY } else { CAUTION }, 12.))
                .child(if is_granted {
                    tr_fda_status_granted(lang)
                } else {
                    tr_fda_status_limited(lang)
                })
                .when(!is_granted, |d| {
                    d.on_click(cx.listener(|this, _, _, cx| {
                        this.open_fda_guide(cx);
                    }))
                }),
        )
    };

    #[cfg(not(target_os = "macos"))]
    let title_area = title_area.child(
        div()
            .px_2()
            .py(px(2.))
            .rounded_full()
            .border_1()
            .when(root.elevated, |d| {
                d.bg(rgb(PRIMARY_FIXED))
                    .border_color(rgb(PRIMARY))
                    .text_color(rgb(PRIMARY))
            })
            .when(!root.elevated, |d| {
                d.bg(rgb(SURF_LOW))
                    .border_color(rgb(OUTLINE_VAR))
                    .text_color(rgb(OUTLINE))
            })
            .text_xs()
            .font_weight(gpui::FontWeight::MEDIUM)
            .flex()
            .items_center()
            .gap_1()
            .child(icon_shield(
                if root.elevated { PRIMARY } else { OUTLINE },
                12.,
            ))
            .child(tr_elevation_mode(lang, root.elevated)),
    );

    div()
        .h(px(60.))
        .flex_none()
        .w_full()
        .min_w(px(0.))
        .flex()
        .items_center()
        .justify_between()
        .px_8()
        .bg(rgb(CARD))
        .border_b_1()
        .border_color(rgba(OUTLINE_VAR, 0.4))
        // 左侧标题
        .child(title_area)
        // 右侧操作
        .child(
            div()
                .flex()
                .items_center()
                .gap_3()
                .when(root.clean.freed_total > 0, |d| {
                    d.child(
                        div()
                            .px_3()
                            .py(px(4.))
                            .rounded_full()
                            .bg(rgb(PRIMARY_FIXED))
                            .text_xs()
                            .font_weight(gpui::FontWeight::BOLD)
                            .text_color(rgb(PRIMARY))
                            .child(tr_freed_pill(lang, &fmt_size(root.clean.freed_total))),
                    )
                })
                .child(
                    div()
                        .id("rescan")
                        .child(ghost_button(label.to_string(), !busy))
                        .on_click(cx.listener(|this, _, _, cx| match this.view {
                            View::Dashboard | View::Junk => {
                                if !this.junk.scanning && !this.clean.running {
                                    this.start_scan(cx);
                                }
                            }
                            // 状态页的轮询常驻，重扫按钮无意义，点了只对齐采样节拍
                            View::Status => {
                                this.start_status_monitor(cx);
                            }
                            View::Apps => {
                                if !this.apps.scanning && !this.residual.scanning {
                                    this.start_apps_scan(cx);
                                }
                            }
                            View::Disk => {
                                if !this.disk.scanning {
                                    this.restart_mft_scan(cx);
                                }
                            }
                            View::Declutter => {
                                if !this.declutter.scanning {
                                    this.start_declutter_scan(cx);
                                }
                            }
                            View::Search => {
                                if !this.search.indexing {
                                    // 清空旧索引，强制重建
                                    #[cfg(windows)]
                                    {
                                        this.search.indices.clear();
                                    }
                                    this.start_search_index(cx);
                                }
                            }
                        })),
                ),
        )
}
