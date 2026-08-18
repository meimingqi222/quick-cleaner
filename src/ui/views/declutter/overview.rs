//! 冗余整理总览视图 (Overview Bento Grid)

use super::DeclutterTab;
use crate::core::i18n::Language;
use crate::core::model::fmt_size;
use crate::ui::components::icons::{
    icon_badge, icon_downloads, icon_files_duplicate, icon_photos_similar, icon_rocket, icon_weight,
};
use crate::ui::theme::*;
use crate::ui::Root;
use gpui::prelude::*;
use gpui::{div, px, rgb, AnyElement, Context};

pub fn render_overview_tab(root: &Root, cx: &mut Context<Root>) -> AnyElement {
    let lang = root.language;
    let state = &root.declutter;

    let total_savings = state.total_potential_savings();
    let downloads_size = state.total_downloads_size();
    let large_files_size = state.total_large_files_size();
    let large_files_count = state.large_files.len();
    let dup_count = state.duplicate_groups.len();
    let photo_group_count = state.photo_groups.len();

    div()
        .id("declutter-overview-scroll")
        .size_full()
        .min_h(px(0.))
        .overflow_scroll()
        .p_8()
        .flex()
        .flex_col()
        .gap_6()
        // --- 1. Hero 区域 ---
        .child(
            div()
                .flex_none()
                .flex()
                .items_end()
                .justify_between()
                .border_b_1()
                .border_color(rgba(OUTLINE_VAR, 0.25))
                .pb_6()
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .max_w(px(580.))
                        .child(
                            // 扫描中 / 扫描完成显示耗时，未扫描时不显示徽章
                            if state.scanning {
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .px_3()
                                    .py(px(3.))
                                    .rounded_full()
                                    .bg(rgb(0xefdbff))
                                    .child(
                                        div()
                                            .w(px(6.))
                                            .h(px(6.))
                                            .rounded_full()
                                            .bg(rgb(0x7547ab)),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .font_weight(gpui::FontWeight::BOLD)
                                            .text_color(rgb(0x5c2d91))
                                            .child(match lang {
                                                Language::Zh => "扫描中...",
                                                Language::En => "Scanning...",
                                            }),
                                    )
                            } else if let Some(secs) = state.scan_elapsed_secs {
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .px_3()
                                    .py(px(3.))
                                    .rounded_full()
                                    .bg(rgb(0xefdbff))
                                    .child(
                                        div()
                                            .w(px(6.))
                                            .h(px(6.))
                                            .rounded_full()
                                            .bg(rgb(0x7547ab)),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .font_weight(gpui::FontWeight::BOLD)
                                            .text_color(rgb(0x5c2d91))
                                            .child(match lang {
                                                Language::Zh => {
                                                    format!("扫描耗时 {:.1}s", secs)
                                                }
                                                Language::En => {
                                                    format!("Scan took {:.1}s", secs)
                                                }
                                            }),
                                    )
                            } else {
                                div()
                            },
                        )
                        .child(
                            div()
                                .text_3xl()
                                .font_weight(gpui::FontWeight::BOLD)
                                .text_color(rgb(TEXT))
                                .child(match lang {
                                    Language::Zh => "磁盘冗余整理与瘦身",
                                    Language::En => "Declutter Your Drive",
                                }),
                        )
                        .child(
                            div()
                                .text_sm()
                                .text_color(rgb(MUTED))
                                .child(match lang {
                                    Language::Zh => "我们识别到了占用存储空间的非必要文件。查看下方维度，只需轻点几下即可重获充裕的磁盘性能。",
                                    Language::En => "We've identified unnecessary files hoarding your storage space. Review the categories below and reclaim your disk performance with a single click.",
                                }),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .items_center()
                        .gap_2()
                        .child(
                            div()
                                .id("btn-start-smart-scan")
                                .px_6()
                                .py_3()
                                .rounded_xl()
                                .bg(rgb(PRIMARY))
                                .hover(|h| h.bg(rgb(PRIMARY_BRIGHT)))
                                .cursor_pointer()
                                .shadow_md()
                                .flex()
                                .items_center()
                                .gap_2()
                                .text_color(rgb(ON_PRIMARY))
                                .font_weight(gpui::FontWeight::BOLD)
                                .text_sm()
                                .child(icon_rocket(0xffffff, 18.))
                                .child(if state.scanning {
                                    match lang {
                                        Language::Zh => "正在智能分析扫描...",
                                        Language::En => "Scanning Drive...",
                                    }
                                } else {
                                    match lang {
                                        Language::Zh => "开启全盘冗余扫描",
                                        Language::En => "Start Smart Scan",
                                    }
                                })
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.start_declutter_scan(cx);
                                })),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(rgb(OUTLINE))
                                .child(if state.scanned {
                                    format!(
                                        "{} {}",
                                        if lang == Language::Zh { "可优化空间:" } else { "Potential Savings:" },
                                        fmt_size(total_savings)
                                    )
                                } else {
                                    match lang {
                                        Language::Zh => "预计耗时: ~1-2 秒 (索引加速)".to_string(),
                                        Language::En => "Estimated time: ~1-2 secs (Indexed)".to_string(),
                                    }
                                }),
                        ),
                ),
        )
        // --- 2. Bento Grid 布局 ---
        .child(
            div()
                .flex_none()
                .flex()
                .flex_col()
                .gap_4()
                // 上半区：Downloads (8列) + Large Files (4列)
                .child(
                    div()
                        .flex()
                        .gap_4()
                        .w_full()
                        // 卡片 1: 下载项整理 (Downloads Folder)
                        .child(
                            div()
                                .id("card-bento-downloads")
                                .flex_1()
                                .p_6()
                                .rounded_xl()
                                .bg(rgb(CARD))
                                .border_1()
                                .border_color(rgba(OUTLINE_VAR, 0.45))
                                .shadow_sm()
                                .hover(|h| h.bg(rgb(SURF_LOW)).border_color(rgba(PRIMARY, 0.5)))
                                .cursor_pointer()
                                .flex()
                                .flex_col()
                                .justify_between()
                                .min_h(px(190.))
                                .child(
                                    div()
                                        .flex()
                                        .items_start()
                                        .justify_between()
                                        .child(
                                            div()
                                                .flex()
                                                .items_center()
                                                .gap_4()
                                                .child(icon_badge(
                                                    icon_downloads(0x0078d4, 24.),
                                                    0xe0f2fe,
                                                    0x0078d4,
                                                    48.,
                                                ))
                                                .child(
                                                    div()
                                                        .flex()
                                                        .flex_col()
                                                        .gap_1()
                                                        .child(
                                                            div()
                                                                .text_base()
                                                                .font_weight(gpui::FontWeight::BOLD)
                                                                .text_color(rgb(TEXT))
                                                                .child(match lang {
                                                                    Language::Zh => "下载项文件夹",
                                                                    Language::En => "Downloads Folder",
                                                                }),
                                                        )
                                                        .child(
                                                            div()
                                                                .text_xs()
                                                                .text_color(rgb(MUTED))
                                                                .child(match lang {
                                                                    Language::Zh => "历史安装包、归档与缓存",
                                                                    Language::En => "Old installers and archives",
                                                                }),
                                                        ),
                                                ),
                                        )
                                        .child(
                                            div()
                                                .px_3()
                                                .py(px(3.))
                                                .rounded_full()
                                                .bg(rgb(SURF_HIGH))
                                                .text_xs()
                                                .font_weight(gpui::FontWeight::MEDIUM)
                                                .text_color(rgb(TEXT))
                                                .child(match lang {
                                                    Language::Zh => "查看 ›",
                                                    Language::En => "Review ›",
                                                }),
                                        ),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .items_end()
                                        .justify_between()
                                        .child(
                                            div()
                                                .flex()
                                                .flex_col()
                                                .child(
                                                    div()
                                                        .text_xs()
                                                        .font_weight(gpui::FontWeight::BOLD)
                                                        .text_color(rgb(OUTLINE))
                                                        .child(match lang {
                                                            Language::Zh => "可释放空间",
                                                            Language::En => "POTENTIAL SAVINGS",
                                                        }),
                                                )
                                                .child(
                                                    div()
                                                        .text_2xl()
                                                        .font_weight(gpui::FontWeight::BOLD)
                                                        .text_color(rgb(0x0078d4))
                                                        .child(if state.scanned {
                                                            fmt_size(downloads_size)
                                                        } else if lang == Language::Zh {
                                                            "待扫描".to_string()
                                                        } else {
                                                            "Pending".to_string()
                                                        }),
                                                ),
                                        )
                                        .child(
                                            // 文件类型比例柱
                                            div()
                                                .flex()
                                                .items_end()
                                                .gap_3()
                                                .child(
                                                    div()
                                                        .flex()
                                                        .flex_col()
                                                        .items_center()
                                                        .gap_1()
                                                        .child(div().w(px(8.)).h(px(36.)).rounded_t_sm().bg(rgb(0x0078d4)))
                                                        .child(div().text_xs().text_color(rgb(OUTLINE)).child(".DMG")),
                                                )
                                                .child(
                                                    div()
                                                        .flex()
                                                        .flex_col()
                                                        .items_center()
                                                        .gap_1()
                                                        .child(div().w(px(8.)).h(px(22.)).rounded_t_sm().bg(rgb(0x7547ab)))
                                                        .child(div().text_xs().text_color(rgb(OUTLINE)).child(".ZIP")),
                                                )
                                                .child(
                                                    div()
                                                        .flex()
                                                        .flex_col()
                                                        .items_center()
                                                        .gap_1()
                                                        .child(div().w(px(8.)).h(px(14.)).rounded_t_sm().bg(rgb(0x974700)))
                                                        .child(div().text_xs().text_color(rgb(OUTLINE)).child(".PKG")),
                                                ),
                                        ),
                                )
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.declutter.tab = DeclutterTab::Downloads;
                                    cx.notify();
                                })),
                        )
                        // 卡片 2: 大型与旧文件 (Large & Old Files)
                        .child(
                            div()
                                .id("card-bento-large-files")
                                .w(px(310.))
                                .flex_none()
                                .p_6()
                                .rounded_xl()
                                .bg(rgb(CARD))
                                .border_1()
                                .border_color(rgba(OUTLINE_VAR, 0.45))
                                .shadow_sm()
                                .hover(|h| h.bg(rgb(SURF_LOW)).border_color(rgba(ERROR, 0.5)))
                                .cursor_pointer()
                                .flex()
                                .flex_col()
                                .justify_between()
                                .min_h(px(190.))
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap_3()
                                        .child(icon_badge(
                                            icon_weight(0xba1a1a, 20.),
                                            0xffdad6,
                                            0xba1a1a,
                                            38.,
                                        ))
                                        .child(
                                            div()
                                                .text_base()
                                                .font_weight(gpui::FontWeight::BOLD)
                                                .text_color(rgb(TEXT))
                                                .child(match lang {
                                                    Language::Zh => "大型与旧文件",
                                                    Language::En => "Large & Old Files",
                                                }),
                                        ),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .justify_around()
                                        .py_2()
                                        .child(
                                            div()
                                                .w(px(64.))
                                                .h(px(64.))
                                                .rounded_full()
                                                .border_4()
                                                .border_color(rgb(ERROR))
                                                .bg(rgb(SURF_HIGH))
                                                .flex()
                                                .flex_col()
                                                .items_center()
                                                .justify_center()
                                                .child(
                                                    div()
                                                        .text_sm()
                                                        .font_weight(gpui::FontWeight::BOLD)
                                                        .text_color(rgb(TEXT))
                                                        .child(format!("{large_files_count}")),
                                                )
                                                .child(
                                                    div()
                                                        .text_xs()
                                                        .text_color(rgb(OUTLINE))
                                                        .child(match lang {
                                                            Language::Zh => "个文件",
                                                            Language::En => "Files",
                                                        }),
                                                ),
                                        )
                                        .child(
                                            div()
                                                .flex()
                                                .flex_col()
                                                .max_w(px(140.))
                                                .child(
                                                    div()
                                                        .text_xs()
                                                        .text_color(rgb(MUTED))
                                                        .child(match lang {
                                                            Language::Zh => "超过 100MB 且半年未访问",
                                                            Language::En => "Over 100MB untouched in 6 mos",
                                                        }),
                                                ),
                                        ),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .justify_between()
                                        .border_t_1()
                                        .border_color(rgba(OUTLINE_VAR, 0.3))
                                        .pt_2()
                                        .child(
                                            div()
                                                .text_xs()
                                                .font_weight(gpui::FontWeight::BOLD)
                                                .text_color(rgb(ERROR))
                                                .child(if state.scanned {
                                                    if lang == Language::Zh {
                                                        format!("总计 ~{}", fmt_size(large_files_size))
                                                    } else {
                                                        format!("~{} Total", fmt_size(large_files_size))
                                                    }
                                                } else if lang == Language::Zh {
                                                    "待扫描".to_string()
                                                } else {
                                                    "Pending".to_string()
                                                }),
                                        )
                                        .child(
                                            div()
                                                .text_xs()
                                                .font_weight(gpui::FontWeight::BOLD)
                                                .text_color(rgb(PRIMARY))
                                                .child(match lang {
                                                    Language::Zh => "查看 ›",
                                                    Language::En => "Review ›",
                                                }),
                                        ),
                                )
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.declutter.tab = DeclutterTab::LargeFiles;
                                    cx.notify();
                                })),
                        )
                )
                // 下半区：Duplicates (6列) + Similar Photos (6列)
                .child(
                    div()
                        .flex()
                        .gap_4()
                        .w_full()
                        // 卡片 3: 重复文件 (Duplicates)
                        .child(
                            div()
                                .id("card-bento-duplicates")
                                .flex_1()
                                .p_6()
                                .rounded_xl()
                                .bg(rgb(CARD))
                                .border_1()
                                .border_color(rgba(OUTLINE_VAR, 0.45))
                                .shadow_sm()
                                .hover(|h| h.bg(rgb(SURF_LOW)).border_color(rgba(0x7547ab, 0.5)))
                                .cursor_pointer()
                                .flex()
                                .flex_col()
                                .justify_between()
                                .min_h(px(160.))
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .justify_between()
                                        .child(
                                            div()
                                                .flex()
                                                .items_center()
                                                .gap_3()
                                                .child(icon_badge(
                                                    icon_files_duplicate(0x7547ab, 20.),
                                                    0xefdbff,
                                                    0x7547ab,
                                                    38.,
                                                ))
                                                .child(
                                                    div()
                                                        .flex()
                                                        .flex_col()
                                                        .child(
                                                            div()
                                                                .text_base()
                                                                .font_weight(gpui::FontWeight::BOLD)
                                                                .text_color(rgb(TEXT))
                                                                .child(match lang {
                                                                    Language::Zh => "重复文件",
                                                                    Language::En => "Duplicates",
                                                                }),
                                                        )
                                                        .child(
                                                            div()
                                                                .text_xs()
                                                                .text_color(rgb(OUTLINE))
                                                                .child(match lang {
                                                                    Language::Zh => "冗余副本数据",
                                                                    Language::En => "REDUNDANT DATA",
                                                                }),
                                                        ),
                                                ),
                                        ),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .items_end()
                                        .justify_between()
                                        .child(
                                            div()
                                                .flex()
                                                .items_baseline()
                                                .gap_2()
                                                .child(
                                                    div()
                                                        .text_2xl()
                                                        .font_weight(gpui::FontWeight::BOLD)
                                                        .text_color(rgb(TEXT))
                                                        .child(format!("{dup_count}")),
                                                )
                                                .child(
                                                    div()
                                                        .text_xs()
                                                        .text_color(rgb(MUTED))
                                                        .child(match lang {
                                                            Language::Zh => "组完全相同副本",
                                                            Language::En => "Sets found across folders",
                                                        }),
                                                ),
                                        )
                                        .child(
                                            div()
                                                .px_3()
                                                .py(px(3.))
                                                .rounded_full()
                                                .bg(rgb(0xefdbff))
                                                .text_xs()
                                                .font_weight(gpui::FontWeight::BOLD)
                                                .text_color(rgb(0x5c2d91))
                                                .child(match lang {
                                                    Language::Zh => "智能挑选 ›",
                                                    Language::En => "Select ›",
                                                }),
                                        ),
                                )
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.declutter.tab = DeclutterTab::Duplicates;
                                    cx.notify();
                                })),
                        )
                        // 卡片 4: 相似图片 (Similar Photos)
                        .child(
                            div()
                                .id("card-bento-similar-photos")
                                .flex_1()
                                .p_6()
                                .rounded_xl()
                                .bg(rgb(CARD))
                                .border_1()
                                .border_color(rgba(OUTLINE_VAR, 0.45))
                                .shadow_sm()
                                .hover(|h| h.bg(rgb(SURF_LOW)).border_color(rgba(0x974700, 0.5)))
                                .cursor_pointer()
                                .flex()
                                .flex_col()
                                .justify_between()
                                .min_h(px(160.))
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .justify_between()
                                        .child(
                                            div()
                                                .flex()
                                                .items_center()
                                                .gap_3()
                                                .child(icon_badge(
                                                    icon_photos_similar(0x974700, 20.),
                                                    0xffdbc8,
                                                    0x974700,
                                                    38.,
                                                ))
                                                .child(
                                                    div()
                                                        .flex()
                                                        .flex_col()
                                                        .child(
                                                            div()
                                                                .text_base()
                                                                .font_weight(gpui::FontWeight::BOLD)
                                                                .text_color(rgb(TEXT))
                                                                .child(match lang {
                                                                    Language::Zh => "相似图片",
                                                                    Language::En => "Similar Photos",
                                                                }),
                                                        )
                                                        .child(
                                                            div()
                                                                .text_xs()
                                                                .text_color(rgb(OUTLINE))
                                                                .child(match lang {
                                                                    Language::Zh => "视觉冗余整理",
                                                                    Language::En => "VISUAL CLUTTER",
                                                                }),
                                                        ),
                                                ),
                                        )
                                        .child(
                                            div()
                                                .px_2()
                                                .py(px(2.))
                                                .rounded_full()
                                                .bg(rgb(SURF_HIGH))
                                                .text_xs()
                                                .text_color(rgb(MUTED))
                                                .child(match (state.scanned, lang) {
                                                    (true, Language::Zh) => "● 已分析",
                                                    (true, Language::En) => "● Analyzed",
                                                    (false, Language::Zh) => "● 待扫描",
                                                    (false, Language::En) => "● Pending",
                                                }),
                                        ),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .gap_2()
                                        .child(
                                            div()
                                                .flex()
                                                .items_center()
                                                .justify_between()
                                                .child(
                                                    div()
                                                        .text_xs()
                                                        .text_color(rgb(MUTED))
                                                        .child(match lang {
                                                            Language::Zh => "相似/连拍照片组",
                                                            Language::En => "Estimated groups",
                                                        }),
                                                )
                                                .child(
                                                    div()
                                                        .text_base()
                                                        .font_weight(gpui::FontWeight::BOLD)
                                                        .text_color(rgb(TEXT))
                                                        .child(format!("{photo_group_count}")),
                                                ),
                                        )
                                        .child(
                                            div()
                                                .w_full()
                                                .h(px(4.))
                                                .rounded_full()
                                                .bg(rgb(SURF_HIGH))
                                                .child(
                                                    div()
                                                        .w(px(80.))
                                                        .h_full()
                                                        .rounded_full()
                                                        .bg(rgb(0x974700)),
                                                ),
                                        ),
                                )
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.declutter.tab = DeclutterTab::SimilarPhotos;
                                    cx.notify();
                                })),
                        ),
                ),
        )
        .into_any_element()
}
