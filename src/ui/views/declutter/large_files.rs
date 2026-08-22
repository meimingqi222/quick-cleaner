//! 大型与旧文件视图 (Large & Old Files View)

use super::common::{
    render_declutter_action_bar, render_empty_state_card, render_unified_nav_header,
};
use super::DeclutterTab;
use crate::core::i18n::Language;
use crate::core::model::fmt_size;
use crate::ui::components::controls::checkbox;
use crate::ui::components::icons::{
    icon_badge, icon_folder_large, icon_photos_similar, icon_video, icon_zip,
};
use crate::ui::theme::*;
use crate::ui::Root;
use gpui::prelude::*;
use gpui::{div, px, rgb, AnyElement, Context, SharedString};

pub fn render_large_files_tab(root: &Root, cx: &mut Context<Root>) -> AnyElement {
    let lang = root.language;
    let state = &root.declutter;

    let filtered_indices: Vec<usize> = state
        .large_files
        .iter()
        .enumerate()
        .filter(|(_, item)| {
            if item.size < state.min_size_filter {
                return false;
            }
            if let Some(kind) = state.kind_filter {
                if item.icon_type != kind {
                    return false;
                }
            }
            true
        })
        .map(|(idx, _)| idx)
        .collect();

    let total_found: u64 = filtered_indices
        .iter()
        .map(|&idx| state.large_files[idx].size)
        .sum();

    let total_sel: u64 = filtered_indices
        .iter()
        .filter(|&&idx| state.large_files[idx].selected)
        .map(|&idx| state.large_files[idx].size)
        .sum();

    let total_sel_count = filtered_indices
        .iter()
        .filter(|&&idx| state.large_files[idx].selected)
        .count();

    let tab_nav = render_unified_nav_header(
        DeclutterTab::LargeFiles,
        match lang {
            Language::Zh => "大型与旧文件",
            Language::En => "Large & Old Files",
        },
        lang,
        cx,
    );

    let rows: Vec<AnyElement> = if filtered_indices.is_empty() {
        vec![render_empty_state_card(
            "📦",
            match lang {
                Language::Zh => "未发现符合条件的大型文件",
                Language::En => "No large files found",
            },
            match lang {
                Language::Zh => "未找到大于指定筛选体积的文件，您可以尝试清除筛选条件。",
                Language::En => "No files exceed the size filter. Try clearing the filter.",
            },
        )]
    } else {
        filtered_indices
            .iter()
            .take(100)
            .map(|&idx| {
                let item = &state.large_files[idx];
                let is_sel = item.selected;
                let l_path = item.path.clone();
                let l_name = item.filename.clone();

                let icon_el = match item.icon_type {
                    0 => icon_badge(icon_video(0x0078d4, 18.), 0xe0f2fe, 0x0078d4, 36.),
                    1 => icon_badge(icon_zip(0x7547ab, 18.), 0xefdbff, 0x7547ab, 36.),
                    2 => icon_badge(icon_folder_large(0x059669, 18.), 0xd1fae5, 0x059669, 36.),
                    _ => icon_badge(icon_photos_similar(0x974700, 18.), 0xffdbc8, 0x974700, 36.),
                };

                div()
                    .id(SharedString::from(format!("large-row-{idx}")))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_between()
                    .px_6()
                    .py_4()
                    .border_b_1()
                    .border_color(rgba(OUTLINE_VAR, 0.2))
                    .when(is_sel, |d| d.bg(rgba(PRIMARY, 0.05)))
                    .when(!is_sel, |d| d.hover(|h| h.bg(rgb(SURF_LOW))))
                    .on_mouse_down(
                        gpui::MouseButton::Right,
                        cx.listener(move |this, event: &gpui::MouseDownEvent, _, cx| {
                            let x: f32 = event.position.x.into();
                            let y: f32 = event.position.y.into();
                            this.open_declutter_context_menu(l_path.clone(), l_name.clone(), x, y);
                            cx.notify();
                        }),
                    )
                    .child(
                        div()
                            .id(SharedString::from(format!("large-sel-{idx}")))
                            .flex_1()
                            .min_w(px(0.))
                            .flex()
                            .items_center()
                            .gap_4()
                            .cursor_pointer()
                            .child(checkbox(if is_sel {
                                crate::core::model::Check::On
                            } else {
                                crate::core::model::Check::Off
                            }))
                            .child(icon_el)
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(0.))
                                    .flex()
                                    .flex_col()
                                    .gap(px(2.))
                                    .child(
                                        div()
                                            .text_sm()
                                            .font_weight(gpui::FontWeight::MEDIUM)
                                            .text_color(rgb(TEXT))
                                            .overflow_hidden()
                                            .child(item.filename.clone()),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(rgb(OUTLINE))
                                            .overflow_hidden()
                                            .child(item.path_display.clone()),
                                    ),
                            )
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if let Some(f) = this.declutter.large_files.get_mut(idx) {
                                    f.selected = !f.selected;
                                    cx.notify();
                                }
                            })),
                    )
                    .child(
                        div()
                            .flex_none()
                            .flex()
                            .items_center()
                            .gap_3()
                            .child(
                                div()
                                    .id(SharedString::from(format!("btn-reveal-large-{idx}")))
                                    .px_2()
                                    .py(px(2.))
                                    .rounded_md()
                                    .bg(rgb(SURF_HIGH))
                                    .hover(|h| h.bg(rgb(PRIMARY_FIXED)).text_color(rgb(PRIMARY)))
                                    .text_xs()
                                    .font_weight(gpui::FontWeight::MEDIUM)
                                    .text_color(rgb(MUTED))
                                    .cursor_pointer()
                                    .child(match lang {
                                        Language::Zh => "定位",
                                        Language::En => "Reveal",
                                    })
                                    .on_click({
                                        let p = item.path.clone();
                                        cx.listener(move |_, _event: &gpui::ClickEvent, _, cx| {
                                            cx.stop_propagation();
                                            crate::platform::reveal_in_explorer(&p);
                                        })
                                    }),
                            )
                            .child(
                                div()
                                    .id(SharedString::from(format!("btn-open-large-{idx}")))
                                    .px_2()
                                    .py(px(2.))
                                    .rounded_md()
                                    .bg(rgb(SURF_HIGH))
                                    .hover(|h| h.bg(rgb(PRIMARY_FIXED)).text_color(rgb(PRIMARY)))
                                    .text_xs()
                                    .font_weight(gpui::FontWeight::MEDIUM)
                                    .text_color(rgb(MUTED))
                                    .cursor_pointer()
                                    .child(match lang {
                                        Language::Zh => "打开",
                                        Language::En => "Open",
                                    })
                                    .on_click({
                                        let p = item.path.clone();
                                        cx.listener(move |_, _event: &gpui::ClickEvent, _, cx| {
                                            cx.stop_propagation();
                                            crate::platform::open_in_default_app(&p);
                                        })
                                    }),
                            )
                            .child(
                                div()
                                    .w(px(90.))
                                    .text_right()
                                    .text_sm()
                                    .font_weight(gpui::FontWeight::BOLD)
                                    .text_color(rgb(ERROR))
                                    .child(fmt_size(item.size)),
                            )
                            .child(
                                div()
                                    .w(px(110.))
                                    .text_right()
                                    .text_xs()
                                    .text_color(rgb(MUTED))
                                    .child(item.last_accessed_str.get(lang).to_string()),
                            ),
                    )
                    .into_any_element()
            })
            .collect()
    };

    div()
        .id("declutter-large-view")
        .size_full()
        .flex()
        .flex_col()
        .child(
            div()
                .id("declutter-large-scroll-inner")
                .flex_1()
                .min_h(px(0.))
                .overflow_scroll()
                .p_8()
                .pb_8()
                .flex()
                .flex_col()
                .gap_6()
                .child(tab_nav)
                .child(
                    div()
                        .flex_none()
                        .p_6()
                        .rounded_xl()
                        .bg(rgb(CARD))
                        .border_1()
                        .border_color(rgba(OUTLINE_VAR, 0.4))
                        .shadow_sm()
                        .flex()
                        .flex_col()
                        .gap_4()
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
                                                .text_xl()
                                                .font_weight(gpui::FontWeight::BOLD)
                                                .text_color(rgb(TEXT))
                                                .child(match lang {
                                                Language::Zh => "审查大型文件",
                                                Language::En => "Review Items",
                                                }),
                                        )
                                        .child(
                                            div()
                                                .text_xs()
                                                .text_color(rgb(MUTED))
                                                .child(match lang {
                                                    Language::Zh => "选择不再需要的文件，安全清理以释放宝贵的磁盘空间。",
                                                    Language::En => "Select files you no longer need. Safely remove them to free up disk space.",
                                                }),
                                        ),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .items_end()
                                        .child(div().text_xs().text_color(rgb(OUTLINE)).child(match lang {
                                            Language::Zh => "筛选总计",
                                            Language::En => "TOTAL FOUND",
                                        }))
                                        .child(
                                            div()
                                                .text_xl()
                                                .font_weight(gpui::FontWeight::BOLD)
                                                .text_color(rgb(ERROR))
                                                .child(fmt_size(total_found)),
                                        ),
                                )
                        )
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_3()
                                .pt_3()
                                .border_t_1()
                                .border_color(rgba(OUTLINE_VAR, 0.2))
                                .child(
                                    div()
                                        .id("chip-filter-size")
                                        .px_3()
                                        .py(px(4.))
                                        .rounded_full()
                                        .when(state.min_size_filter > 0, |d| {
                                            d.bg(rgb(PRIMARY_FIXED))
                                                .border_1()
                                                .border_color(rgb(PRIMARY))
                                                .text_color(rgb(PRIMARY))
                                        })
                                        .when(state.min_size_filter == 0, |d| {
                                            d.bg(rgb(SURF_HIGH)).text_color(rgb(MUTED))
                                        })
                                        .text_xs()
                                        .font_weight(gpui::FontWeight::MEDIUM)
                                        .cursor_pointer()
                                        .child(match (state.min_size_filter, lang) {
                                            (0, Language::Zh) => "大小: 全部 ▾",
                                            (0, Language::En) => "Size: All ▾",
                                            (50_000_000, Language::Zh) => "大小: > 50MB ✕",
                                            (50_000_000, Language::En) => "Size: > 50MB ✕",
                                            (100_000_000, Language::Zh) => "大小: > 100MB ✕",
                                            (100_000_000, Language::En) => "Size: > 100MB ✕",
                                            (500_000_000, Language::Zh) => "大小: > 500MB ✕",
                                            (500_000_000, Language::En) => "Size: > 500MB ✕",
                                            (_, Language::Zh) => "大小: > 1GB ✕",
                                            (_, Language::En) => "Size: > 1GB ✕",
                                        })
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.declutter.min_size_filter = match this.declutter.min_size_filter {
                                                100_000_000 => 500_000_000,
                                                500_000_000 => 1_000_000_000,
                                                1_000_000_000 => 0,
                                                _ => 100_000_000,
                                            };
                                            cx.notify();
                                        })),
                                )
                                .child(
                                    div()
                                        .id("chip-filter-kind")
                                        .px_3()
                                        .py(px(4.))
                                        .rounded_full()
                                        .when(state.kind_filter.is_some(), |d| {
                                            d.bg(rgb(PRIMARY_FIXED))
                                                .border_1()
                                                .border_color(rgb(PRIMARY))
                                                .text_color(rgb(PRIMARY))
                                        })
                                        .when(state.kind_filter.is_none(), |d| {
                                            d.bg(rgb(SURF_HIGH)).text_color(rgb(MUTED))
                                        })
                                        .text_xs()
                                        .cursor_pointer()
                                        .child(match (state.kind_filter, lang) {
                                            (Some(0), Language::Zh) => "类型: 视频 ✕",
                                            (Some(0), Language::En) => "Kind: Video ✕",
                                            (Some(1), Language::Zh) => "类型: 压缩包 ✕",
                                            (Some(1), Language::En) => "Kind: Archive ✕",
                                            (Some(2), Language::Zh) => "类型: 文件夹 ✕",
                                            (Some(2), Language::En) => "Kind: Folder ✕",
                                            (Some(3), Language::Zh) => "类型: 图片 ✕",
                                            (Some(3), Language::En) => "Kind: Image ✕",
                                            (_, Language::Zh) => "类型: 全部 ▾",
                                            (_, Language::En) => "Kind: All Types ▾",
                                        })
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.declutter.kind_filter = match this.declutter.kind_filter {
                                                None => Some(0),
                                                Some(0) => Some(1),
                                                Some(1) => Some(2),
                                                Some(2) => Some(3),
                                                _ => None,
                                            };
                                            cx.notify();
                                        })),
                                )
                                .child(
                                    div()
                                        .id("chip-filter-clear")
                                        .ml_auto()
                                        .text_xs()
                                        .font_weight(gpui::FontWeight::MEDIUM)
                                        .text_color(rgb(PRIMARY))
                                        .hover(|h| h.underline())
                                        .cursor_pointer()
                                        .child(match lang {
                                            Language::Zh => "清除筛选",
                                            Language::En => "Clear Filters",
                                        })
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.declutter.min_size_filter = 0;
                                            this.declutter.kind_filter = None;
                                            cx.notify();
                                        })),
                                )
                        ),
                )
                .child(
                    div()
                        .flex_none()
                        .rounded_xl()
                        .bg(rgb(CARD))
                        .border_1()
                        .border_color(rgba(OUTLINE_VAR, 0.4))
                        .shadow_sm()
                        .overflow_hidden()
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .px_6()
                                .py_3()
                                .bg(rgb(SURF_LOW))
                                .border_b_1()
                                .border_color(rgba(OUTLINE_VAR, 0.25))
                                .flex()
                                .items_center()
                                .justify_between()
                                .text_xs()
                                .font_weight(gpui::FontWeight::BOLD)
                                .text_color(rgb(OUTLINE))
                                .child(div().flex_1().min_w(px(0.)).child(match lang {
                                    Language::Zh => "文件名",
                                    Language::En => "FILE NAME",
                                }))
                                .child(
                                    div()
                                        .flex_none()
                                        .flex()
                                        .items_center()
                                        .gap_8()
                                        .child(div().w(px(100.)).text_right().child(match lang {
                                            Language::Zh => "大小",
                                            Language::En => "SIZE",
                                        }))
                                        .child(div().w(px(120.)).text_right().child(match lang {
                                            Language::Zh => "最后访问",
                                            Language::En => "LAST ACCESSED",
                                        })),
                                ),
                        )
                        .child(div().flex().flex_col().children(rows)),
                ),
        )
        // 底部操作条：与其余三个页签共用 common.rs 里的实现，见其上注释。
        .child(render_declutter_action_bar(
            lang,
            DeclutterTab::LargeFiles,
            total_sel_count,
            total_sel,
            true,
            cx,
        ))
        .into_any_element()
}
