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
    let scanned = root.junk.scanned;

    let (big, sub) = if root.junk.scanning {
        (
            tr_scanning(lang).to_string(),
            match lang {
                Language::Zh => String::from("正在检查系统缓存与开发环境残留"),
                Language::En => String::from("Checking system caches and dev leftovers"),
            },
        )
    } else if root.clean.running {
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
        (
            tr_system_clean(lang).to_string(),
            tr_no_junk(lang).to_string(),
        )
    } else {
        (
            String::from("Smart Scan"),
            tr_start_smart_scan(lang).to_string(),
        )
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
    if root.junk.scanning || root.clean.running {
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
                // 圆里的可用宽度是**弦长**，不是直径：内圆直径 RING-56=224，
                // 主标题横跨圆心上下约 ±22px，那里的弦约 219px，留点余量取 200。
                // 不给上限的话，英文那些长句会直接顶出圆边。
                .max_w(px(RING - 56. - 24.))
                .text_center()
                .text_2xl()
                .font_weight(gpui::FontWeight::BOLD)
                .text_color(rgb(ON_PRIMARY))
                .child(big),
        )
        .child(
            div()
                // 副标题落在圆心下方 28~44px，那里的弦约 206px，取 180：
                // 既能让中文那句「正在检查系统缓存与开发环境残留」保持一行，
                // 又能让更长的英文换成两行——换行后第二行仍在圆内（弦还有 198px）。
                .max_w(px(RING - 56. - 44.))
                .text_center()
                .text_xs()
                .font_weight(gpui::FontWeight::MEDIUM)
                .text_color(rgba(ON_PRIMARY, 0.85))
                .child(sub),
        )
        .on_click(cx.listener(|this, _, _, cx| {
            if this.junk.scanning || this.clean.running {
                return;
            }
            if this.junk.scanned {
                this.view = View::Junk;
            } else {
                this.start_scan(cx);
            }
            cx.notify();
        }));

    let ring = ring.child(inner);

    let blurb = if root.junk.scanning {
        match lang {
            Language::Zh => String::from("正在扫描系统缓存、应用日志、开发依赖产物与临时文件…"),
            Language::En => String::from(
                "Scanning system caches, application logs, build artifacts, and temp files…",
            ),
        }
    } else if scanned && total > 0 {
        match lang {
            Language::Zh => format!(
                "已在 {} 个类别中发现 {} 可清理内容。",
                root.junk
                    .categories
                    .iter()
                    .filter(|c| c.total_size > 0)
                    .count(),
                fmt_size(total)
            ),
            Language::En => format!(
                "Found {} cleanable items across {} categories.",
                fmt_size(total),
                root.junk
                    .categories
                    .iter()
                    .filter(|c| c.total_size > 0)
                    .count()
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
            Language::En => {
                String::from("Quickly scan system caches, developer dependencies, and disk usage.")
            }
        }
    };

    // 底部 4 块快速入口卡片
    let quick_cards = div()
        .flex()
        .gap_3()
        .w_full()
        .max_w(px(860.))
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
                .child(icon_badge(
                    icon_trash(PRIMARY, 18.),
                    PRIMARY_FIXED,
                    PRIMARY,
                    38.,
                ))
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
                        .child(div().text_xs().text_color(rgb(OUTLINE)).child(
                            if root.junk.scanned {
                                fmt_size(total)
                            } else {
                                match lang {
                                    Language::Zh => "一键清理".into(),
                                    Language::En => "Clean Junk".into(),
                                }
                            },
                        )),
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
                .child(icon_badge(
                    icon_apps(PRIMARY, 18.),
                    PRIMARY_FIXED,
                    PRIMARY,
                    38.,
                ))
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
                        .child(div().text_xs().text_color(rgb(OUTLINE)).child(
                            if root.apps.scanned {
                                match lang {
                                    Language::Zh => format!("已发现 {} 款", root.apps.list.len()),
                                    Language::En => format!("{} Apps", root.apps.list.len()),
                                }
                            } else {
                                match lang {
                                    Language::Zh => "卸载分析".into(),
                                    Language::En => "Uninstall".into(),
                                }
                            },
                        )),
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.view = View::Apps;
                    if !this.apps.scanned && !this.apps.scanning {
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
                .child(icon_badge(
                    icon_disk(PRIMARY, 18.),
                    PRIMARY_FIXED,
                    PRIMARY,
                    38.,
                ))
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
                        .child(div().text_xs().text_color(rgb(OUTLINE)).child(
                            if let Some(s) = &root.disk.mft {
                                fmt_size(s.total_size)
                            } else {
                                match lang {
                                    Language::Zh => "空间透镜".into(),
                                    Language::En => "Storage".into(),
                                }
                            },
                        )),
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.view = View::Disk;
                    if this.disk.mft.is_none() && this.disk.error.is_none() && !this.disk.scanning {
                        this.start_mft_scan(cx);
                    }
                    cx.notify();
                })),
        )
        .child(
            card()
                .id("quick-declutter")
                .flex_1()
                .p_4()
                .cursor_pointer()
                .hover(|h| h.bg(rgb(SURF_LOW)))
                .flex()
                .items_center()
                .gap_3()
                .child(icon_badge(
                    icon_declutter(PRIMARY, 18.),
                    PRIMARY_FIXED,
                    PRIMARY,
                    38.,
                ))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .text_sm()
                                .font_weight(gpui::FontWeight::BOLD)
                                .text_color(rgb(TEXT))
                                .child(match lang {
                                    Language::Zh => "冗余整理",
                                    Language::En => "Declutter",
                                }),
                        )
                        .child(div().text_xs().text_color(rgb(OUTLINE)).child(match lang {
                            Language::Zh => "重复/大文件",
                            Language::En => "Duplicates/Photos",
                        })),
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.view = View::Declutter;
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

    if scanned && total > 0 && !root.clean.running {
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
