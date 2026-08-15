//! 概览视图（Smart Scan 环形扫描中心）

use crate::core::model::fmt_size;
use crate::ui::components::buttons::primary_button;
use crate::ui::components::cards::card;
use crate::ui::components::icons::*;
use crate::ui::components::sidebar::View;
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

    let total = root.total_cleanable();
    let scanned = root.scanned;

    let (big, sub) = if root.scanning {
        (String::from("扫描中…"), String::from("正在检查系统缓存与开发环境残留"))
    } else if root.cleaning {
        (String::from("清理中…"), String::from("正在彻底移除选中垃圾"))
    } else if scanned && total > 0 {
        (fmt_size(total), String::from("发现可清理内容"))
    } else if scanned {
        (String::from("系统很干净"), String::from("暂无可清理垃圾"))
    } else {
        (String::from("Smart Scan"), String::from("点击开始一键智能扫描"))
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
        .child(icon_sparkle(ON_PRIMARY, 28.))
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
        String::from("正在并行扫描系统缓存、应用日志、临时文件与浏览器数据…")
    } else if scanned && total > 0 {
        format!(
            "已在 {} 个类别中发现 {} 可以清理。清理为安全永久删除，释放宝贵存储空间。",
            root.categories.iter().filter(|c| c.total_size > 0).count(),
            fmt_size(total)
        )
    } else if scanned {
        String::from("未发现可清理的残留垃圾，系统当前处于极佳状态。")
    } else {
        String::from("系统运行稳定。点击中心大圆盘，一键扫描垃圾、分析软件与优化空间。")
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
                                .child("智能清理"),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(rgb(OUTLINE))
                                .child(if root.scanned {
                                    fmt_size(total)
                                } else {
                                    "一键清理".into()
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
                .child(icon_badge(icon_apps(0x7547ab, 18.), 0xf3e8ff, 0x7547ab, 38.))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .text_sm()
                                .font_weight(gpui::FontWeight::BOLD)
                                .text_color(rgb(TEXT))
                                .child("软件管理"),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(rgb(OUTLINE))
                                .child(if root.apps_scanning {
                                    "检索软件中…".into()
                                } else if root.apps_scanned {
                                    format!("{} 款已装软件", root.apps.len())
                                } else {
                                    "软件与残留清理".into()
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
                .child(icon_badge(icon_disk(0x974700, 18.), 0xffedd5, 0x974700, 38.))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .text_sm()
                                .font_weight(gpui::FontWeight::BOLD)
                                .text_color(rgb(TEXT))
                                .child("磁盘透镜"),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(rgb(OUTLINE))
                                .child(if root.mft_scanning {
                                    "深度分析中…".into()
                                } else if let Some(ref scan) = root.mft {
                                    format!("{}: 盘已索引 ({} 文件)", root.disk_volume, scan.file_count)
                                } else if let Some((used, _)) = root.disk_space {
                                    format!("{}: 盘已用 {}", root.disk_volume, fmt_size(used))
                                } else {
                                    "空间分布与大文件".into()
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
        body = body.child(
            div()
                .id("goto-junk")
                .child(primary_button(String::from("查看详情并清理"), true))
                .on_click(cx.listener(|this, _, _, cx| {
                    this.view = View::Junk;
                    cx.notify();
                })),
        );
    }

    body = body.child(quick_cards);

    body.into_any_element()
}
