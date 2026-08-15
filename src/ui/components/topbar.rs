//! 顶部应用操作栏 (CleanFlow 质感顶栏)

use crate::core::model::fmt_size;
use crate::ui::components::buttons::ghost_button;
use crate::ui::components::icons::*;
use crate::ui::components::sidebar::View;
use crate::ui::theme::*;
use crate::ui::Root;
use gpui::{div, prelude::*, px, rgb, Context, IntoElement};

pub fn render_top_bar(root: &Root, cx: &mut Context<Root>) -> impl IntoElement {
    let is_apps_busy = root.apps_scanning || root.residual_scanning;
    let is_mft_busy = root.mft_scanning;
    let is_junk_busy = root.scanning || root.cleaning;

    let (busy, label) = match root.view {
        View::Dashboard | View::Junk => (
            is_junk_busy,
            if is_junk_busy {
                "扫描中…"
            } else {
                "重新扫描"
            },
        ),
        View::Apps => (
            is_apps_busy,
            if is_apps_busy {
                "读取中…"
            } else {
                "刷新软件列表"
            },
        ),
        View::Disk => (
            is_mft_busy,
            if is_mft_busy {
                "扫描中…"
            } else {
                "重新分析磁盘"
            },
        ),
    };

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
        .child(
            div()
                .flex()
                .items_center()
                .gap_3()
                .child(
                    div()
                        .text_lg()
                        .font_weight(gpui::FontWeight::BOLD)
                        .text_color(rgb(TEXT))
                        .child(root.view.title()),
                )
                .child(
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
                        .child(icon_shield(if root.elevated { PRIMARY } else { OUTLINE }, 12.))
                        .child(if root.elevated {
                            "管理员模式"
                        } else {
                            "普通模式"
                        }),
                ),
        )
        // 右侧操作
        .child(
            div()
                .flex()
                .items_center()
                .gap_3()
                .when(root.freed_total > 0, |d| {
                    d.child(
                        div()
                            .px_3()
                            .py(px(4.))
                            .rounded_full()
                            .bg(rgb(PRIMARY_FIXED))
                            .text_xs()
                            .font_weight(gpui::FontWeight::BOLD)
                            .text_color(rgb(PRIMARY))
                            .child(format!("本次已释放 {}", fmt_size(root.freed_total))),
                    )
                })
                .child(
                    div()
                        .id("rescan")
                        .child(ghost_button(label.to_string(), !busy))
                        .on_click(cx.listener(|this, _, _, cx| match this.view {
                            View::Dashboard | View::Junk => {
                                if !this.scanning && !this.cleaning {
                                    this.start_scan(cx);
                                }
                            }
                            View::Apps => {
                                if !this.apps_scanning && !this.residual_scanning {
                                    this.start_apps_scan(cx);
                                }
                            }
                            View::Disk => {
                                if !this.mft_scanning {
                                    this.start_mft_scan(cx);
                                }
                            }
                        })),
                ),
        )
}
