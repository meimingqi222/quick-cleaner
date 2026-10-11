//! 顶部应用操作栏 (CleanFlow 质感顶栏)

use crate::core::model::{commas, fmt_size};
use crate::ui::components::buttons::ghost_button;
use crate::ui::components::icons::*;
use crate::ui::components::sidebar::View;
use crate::ui::i18n::*;
use crate::ui::state::StatusTab;
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
        View::Status => (false, tr_btn_refresh_now(lang)),
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
            .text_sm()
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

    let status_subtabs = if root.view == View::Status {
        let cur_tab = root.monitor.tab;
        let proc_count = root
            .monitor
            .snapshot
            .as_ref()
            .map(|s| s.process_count)
            .unwrap_or(0);

        Some(
            div()
                .id("status-tab-switcher")
                .flex()
                .items_center()
                .gap_1()
                .p(px(2.))
                .rounded_lg()
                .bg(rgb(SURF_LOW))
                .border_1()
                .border_color(rgba(OUTLINE_VAR, 0.4))
                .child(
                    div()
                        .id("status-tab-overview")
                        .px_2p5()
                        .py(px(3.))
                        .rounded_md()
                        .cursor_pointer()
                        .text_xs()
                        .font_weight(if cur_tab == StatusTab::Overview {
                            gpui::FontWeight::BOLD
                        } else {
                            gpui::FontWeight::MEDIUM
                        })
                        .when(cur_tab == StatusTab::Overview, |d| {
                            d.bg(rgb(CARD)).text_color(rgb(PRIMARY)).shadow_sm()
                        })
                        .when(cur_tab != StatusTab::Overview, |d| {
                            d.text_color(rgb(MUTED))
                                .hover(|h| h.bg(rgb(SURF_HIGH)).text_color(rgb(TEXT)))
                        })
                        .child(tr_status_tab_overview(lang))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.set_status_tab(StatusTab::Overview, cx);
                        })),
                )
                .child(
                    div()
                        .id("status-tab-processes")
                        .px_2p5()
                        .py(px(3.))
                        .rounded_md()
                        .cursor_pointer()
                        .text_xs()
                        .font_weight(if cur_tab == StatusTab::Processes {
                            gpui::FontWeight::BOLD
                        } else {
                            gpui::FontWeight::MEDIUM
                        })
                        .flex()
                        .items_center()
                        .gap_1p5()
                        .when(cur_tab == StatusTab::Processes, |d| {
                            d.bg(rgb(CARD)).text_color(rgb(PRIMARY)).shadow_sm()
                        })
                        .when(cur_tab != StatusTab::Processes, |d| {
                            d.text_color(rgb(MUTED))
                                .hover(|h| h.bg(rgb(SURF_HIGH)).text_color(rgb(TEXT)))
                        })
                        .child(tr_status_tab_processes_title(lang))
                        .when(proc_count > 0, |d| {
                            d.child(
                                div()
                                    .px_1p5()
                                    .py(px(0.5))
                                    .rounded_full()
                                    .text_xs()
                                    .font_weight(gpui::FontWeight::BOLD)
                                    .when(cur_tab == StatusTab::Processes, |p| {
                                        p.bg(rgb(PRIMARY_FIXED)).text_color(rgb(PRIMARY))
                                    })
                                    .when(cur_tab != StatusTab::Processes, |p| {
                                        p.bg(rgb(SURF_HIGH)).text_color(rgb(MUTED))
                                    })
                                    .child(commas(proc_count as u64)),
                            )
                        })
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.set_status_tab(StatusTab::Processes, cx);
                        })),
                ),
        )
    } else {
        None
    };

    let right_actions = if root.view == View::Status {
        let is_paused = root.monitor.paused;
        div().flex().items_center().gap_2p5().child(
            div()
                .id("status-pause-toggle")
                .px_2p5()
                .py(px(3.))
                .rounded_full()
                .cursor_pointer()
                .border_1()
                .flex()
                .items_center()
                .gap_1p5()
                .text_xs()
                .font_weight(gpui::FontWeight::MEDIUM)
                .when(!is_paused, |d| {
                    d.bg(rgba(PRIMARY, 0.08))
                        .border_color(rgba(PRIMARY, 0.25))
                        .text_color(rgb(PRIMARY))
                        .child(icon_pulse(PRIMARY, 11.))
                        .child(tr_status_live_monitoring(lang))
                })
                .when(is_paused, |d| {
                    d.bg(rgba(CAUTION, 0.12))
                        .border_color(rgba(CAUTION, 0.35))
                        .text_color(rgb(CAUTION))
                        .child("⏸")
                        .child(tr_status_paused(lang))
                })
                .on_click(cx.listener(|this, _, _, cx| {
                    this.toggle_status_pause(cx);
                })),
        )
    } else {
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
                        View::Status => {
                            this.refresh_status_now(cx);
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
                                #[cfg(windows)]
                                {
                                    this.search.indices.clear();
                                }
                                this.start_search_index(cx);
                            }
                        }
                    })),
            )
    };

    div()
        .h(px(44.))
        .flex_none()
        .w_full()
        .min_w(px(0.))
        .flex()
        .items_center()
        .justify_between()
        .px_5()
        .bg(rgb(CARD))
        .border_b_1()
        .border_color(rgba(OUTLINE_VAR, 0.4))
        // 左侧标题与状态子Tab
        .child(
            div()
                .flex()
                .items_center()
                .gap_4()
                .child(title_area)
                .children(status_subtabs),
        )
        // 右侧操作
        .child(right_actions)
}
