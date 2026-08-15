//! 概览视图（Smart Scan 环形扫描中心）

use crate::core::i18n::Language;
use crate::core::model::fmt_size;
use crate::ui::components::buttons::primary_button;
use crate::ui::components::cards::card;
use crate::ui::components::icons::*;
use crate::ui::components::sidebar::View;
use crate::ui::i18n::*;
use crate::ui::theme::*;
use crate::ui::Root;
use gpui::{
    div, linear_gradient, prelude::*, px, rgb, Animation, AnimationExt as _, AnyElement, Context,
    SharedString,
};
use std::time::Duration;

pub fn render_dashboard_view(root: &Root, cx: &mut Context<Root>) -> AnyElement {
    const RING: f32 = 280.;
    const DOT: f32 = 14.;
    const TRACK: f32 = 8.;

    let lang = root.language;
    let total = root.total_cleanable();
    let scanned = root.scanned;

    let (big, sub) = if root.scanning {
        (
            tr_scanning(lang).to_string(),
            match lang {
                Language::Zh => String::from("正在检查系统缓存与开发环境残留"),
                Language::En => String::from("Checking system caches and dev leftovers"),
            },
        )
    } else if root.cleaning {
        (
            tr_cleaning(lang).to_string(),
            match lang {
                Language::Zh => String::from("正在彻底移除选中垃圾"),
                Language::En => String::from("Permanently removing selected items"),
            },
        )
    } else if scanned && total > 0 {
        (fmt_size(total), tr_found_cleanable(lang).to_string())
    } else if scanned {
        (tr_system_clean(lang).to_string(), tr_no_junk(lang).to_string())
    } else {
        (String::from("Smart Scan"), tr_start_smart_scan(lang).to_string())
    };

    let mut ring = div()
        .relative()
        .w(px(RING))
        .h(px(RING))
        .flex()
        .items_center()
        .justify_center()
        .child(
            div()
                .absolute()
                .inset_0()
                .rounded_full()
                .border(px(TRACK))
                .border_color(rgb(SURF_HIGHEST)),
        );

    // 扫描时沿着轨道绕圈的小点
    if root.scanning || root.cleaning {
        ring = ring.child(
            div()
                .absolute()
                .w(px(DOT))
                .h(px(DOT))
                .rounded_full()
                .bg(rgb(PRIMARY_BRIGHT))
                .with_animation(
                    SharedString::from("scan-orbit"),
                    Animation::new(Duration::from_millis(1500)).repeat(),
                    |dot, delta| {
                        let a = delta * std::f32::consts::TAU;
                        let r = RING / 2. - TRACK / 2.;
                        dot.left(px(RING / 2. + r * a.sin() - DOT / 2.))
                            .top(px(RING / 2. - r * a.cos() - DOT / 2.))
                    },
                ),
        );
    }

    let inner = div()
        .id("smart-scan")
        .w(px(RING - 56.))
        .h(px(RING - 56.))
        .rounded_full()
        .bg(linear_gradient(
            150.,
            gpui::linear_color_stop(rgb(PRIMARY_BRIGHT), 0.),
            gpui::linear_color_stop(rgb(PRIMARY), 1.),
        ))
        .shadow_xl()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(6.))
        .cursor_pointer()
        .child(icon_dashboard(ON_PRIMARY, 28.))
        .child(
            div()
                .text_2xl()
                .font_weight(gpui::FontWeight::BOLD)
                .text_color(rgb(ON_PRIMARY))
                .child(big),
        )
        .child(
            div()
                .text_xs()
                .font_weight(gpui::FontWeight::MEDIUM)
                .text_color(rgba(ON_PRIMARY, 0.85))
                .child(sub),
        )
        .on_click(cx.listener(|this, _, _, cx| {
            if this.scanning || this.cleaning {
                return;
            }
            if this.scanned {
                this.view = View::Junk;
            } else {
                this.start_scan(cx);
            }
            cx.notify();
        }));

    let ring = ring.child(inner);

    let blurb = if root.scanning {
        match lang {
            Language::Zh => String::from("正在扫描系统缓存、应用日志、开发依赖产物与临时文件…"),
            Language::En => String::from("Scanning system caches, application logs, build artifacts, and temp files…"),
        }
    } else if scanned && total > 0 {
        match lang {
            Language::Zh => format!(
                "已在 {} 个类别中发现 {} 可清理内容。",
                root.categories.iter().filter(|c| c.total_size > 0).count(),
                fmt_size(total)
            ),
            Language::En => format!(
                "Found {} cleanable items across {} categories.",
                fmt_size(total),
                root.categories.iter().filter(|c| c.total_size > 0).count()
            ),
        }
    } else if scanned {
        match lang {
            Language::Zh => String::from("未发现可清理的冗余缓存，系统状态良好。"),
            Language::En => String::from("No redundant caches found. System is running cleanly."),
        }
    } else {
        match lang {
            Language::Zh => String::from("快速扫描系统缓存、开发依赖产物与磁盘占用。"),
            Language::En => String::from("Quickly scan system caches, developer dependencies, and disk usage."),
        }
    };

    // 底部 3 块快速入口卡片
    let quick_cards = div()
        .flex()
        .gap_4()
        .w_full()
        .max_w(px(720.))
        .child(
            card()
                .id("quick-junk")
                .flex_1()
                .p_4()
                .cursor_pointer()
                .hover(|h| h.bg(rgb(SURF_LOW)))
                .flex()
                .items_center()
                .gap_3()
                .child(icon_badge(icon_trash(PRIMARY, 18.), PRIMARY_FIXED, PRIMARY, 38.))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .text_sm()
                                .font_weight(gpui::FontWeight::BOLD)
                                .text_color(rgb(TEXT))
                                .child(tr_view_junk(lang)),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(rgb(OUTLINE))
                                .child(if root.scanned {
                                    fmt_size(total)
                                } else {
                                    match lang {
                                        Language::Zh => "一键清理".into(),
                                        Language::En => "Clean Junk".into(),
                                    }
                                }),
                        ),
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.view = View::Junk;
                    cx.notify();
                })),
        )
        .child(
            card()
                .id("quick-apps")
                .flex_1()
                .p_4()
                .cursor_pointer()
                .hover(|h| h.bg(rgb(SURF_LOW)))
                .flex()
                .items_center()
                .gap_3()
                .child(icon_badge(icon_apps(PRIMARY, 18.), PRIMARY_FIXED, PRIMARY, 38.))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .text_sm()
                                .font_weight(gpui::FontWeight::BOLD)
                                .text_color(rgb(TEXT))
                                .child(tr_view_apps(lang)),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(rgb(OUTLINE))
                                .child(if root.apps_scanned {
                                    match lang {
                                        Language::Zh => format!("已发现 {} 款", root.apps.len()),
                                        Language::En => format!("{} Apps", root.apps.len()),
                                    }
                                } else {
                                    match lang {
                                        Language::Zh => "卸载分析".into(),
                                        Language::En => "Uninstall".into(),
                                    }
                                }),
                        ),
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.view = View::Apps;
                    if !this.apps_scanned && !this.apps_scanning {
                        this.start_apps_scan(cx);
                    }
                    cx.notify();
                })),
        )
        .child(
            card()
                .id("quick-disk")
                .flex_1()
                .p_4()
                .cursor_pointer()
                .hover(|h| h.bg(rgb(SURF_LOW)))
                .flex()
                .items_center()
                .gap_3()
                .child(icon_badge(icon_disk(PRIMARY, 18.), PRIMARY_FIXED, PRIMARY, 38.))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .text_sm()
                                .font_weight(gpui::FontWeight::BOLD)
                                .text_color(rgb(TEXT))
                                .child(tr_view_disk(lang)),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(rgb(OUTLINE))
                                .child(if let Some(s) = &root.mft {
                                    fmt_size(s.total_size)
                                } else {
                                    match lang {
                                        Language::Zh => "空间透镜".into(),
                                        Language::En => "Storage".into(),
                                    }
                                }),
                        ),
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.view = View::Disk;
                    if this.mft.is_none() && this.mft_error.is_none() && !this.mft_scanning {
                        this.start_mft_scan(cx);
                    }
                    cx.notify();
                })),
        );

    let mut body = div()
        .id("dashboard-scroll")
        .size_full()
        .min_w(px(0.))
        .overflow_scroll()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap_6()
        .p_8()
        .child(ring)
        .child(
            div()
                .max_w(px(520.))
                .text_sm()
                .text_center()
                .text_color(rgb(MUTED))
                .child(blurb),
        );

    if scanned && total > 0 && !root.cleaning {
        let btn_text = match lang {
            Language::Zh => String::from("查看详情并清理"),
            Language::En => String::from("Review & Clean Junk"),
        };
        body = body.child(
            div()
                .id("goto-junk")
                .child(primary_button(btn_text, true))
                .on_click(cx.listener(|this, _, _, cx| {
                    this.view = View::Junk;
                    cx.notify();
                })),
        );
    }

    body = body.child(quick_cards);

    body.into_any_element()
}
