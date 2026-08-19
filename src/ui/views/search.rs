//! 文件快速检索视图 (Everything 风格全盘秒搜)

use crate::core::i18n::Language;
use crate::core::model::{fmt_size, truncate};
use crate::ui::components::cards::card;
use crate::ui::components::controls::{loading_state_view, page_heading};
use crate::ui::components::icons::*;
use crate::ui::components::scroll::{
    drag_capture, drag_to_offset, scroll_metrics, scrollbar, SCROLLBAR_W,
};
use crate::ui::i18n::*;
use crate::ui::theme::*;
use crate::ui::Root;
use gpui::{
    div, prelude::*, px, rgb, AnyElement, Context, Div, IntoElement, MouseButton, MouseDownEvent,
    SharedString, Stateful, Window,
};

/// 搜索结果行高
const ROW_H: f32 = 40.;

pub fn render_search_view(root: &Root, window: &mut Window, cx: &mut Context<Root>) -> AnyElement {
    let lang = root.language;
    let indexing = root.search.indexing;
    let query = root.search.query.clone();
    let results_len = root.search.results.len();
    let results_empty = root.search.results.is_empty();
    let ready = root.search_index_ready();
    let anim_phase = root.anim_phase;

    // 页面大标题
    let header = div().flex().justify_between().items_center().gap_4().child(
        div().flex_1().min_w(px(0.)).child(page_heading(
            tr_search_heading(lang),
            tr_search_subheading(lang),
        )),
    );

    let search_box = render_search_box(root, window, cx);

    let (status_text, status_color) = if indexing {
        (tr_search_building_index(lang).to_string(), PRIMARY)
    } else if !ready {
        (tr_search_no_index(lang).to_string(), CAUTION)
    } else if query.trim().is_empty() {
        (
            if lang == Language::Zh {
                "全盘索引就绪，输入关键字秒级检索".to_string()
            } else {
                "Index ready. Type to search instantly across all drives".to_string()
            },
            MUTED,
        )
    } else if root.search.is_searching {
        (
            if lang == Language::Zh {
                "正在后台高速检索…".to_string()
            } else {
                "Searching…".to_string()
            },
            PRIMARY,
        )
    } else if results_empty {
        (tr_search_no_results(lang).to_string(), OUTLINE)
    } else {
        (tr_search_results(lang, results_len), PRIMARY)
    };

    let status_tag = div()
        .px_3()
        .py(px(4.))
        .rounded_full()
        .bg(rgb(SURF_HIGH))
        .text_xs()
        .font_weight(gpui::FontWeight::MEDIUM)
        .text_color(rgb(status_color))
        .child(status_text);

    let is_group_kind = root.search.group_by_kind;
    let sort_col = root.search.sort_col;
    let sort_asc = root.search.sort_asc;

    let kind_sort_btn = div()
        .id("search-group-kind")
        .px_3()
        .py(px(4.))
        .rounded_full()
        .cursor_pointer()
        .border_1()
        .flex()
        .items_center()
        .gap_1p5()
        .text_xs()
        .when(is_group_kind, |d| {
            d.bg(rgb(PRIMARY_FIXED))
                .border_color(rgb(PRIMARY))
                .text_color(rgb(PRIMARY))
                .font_weight(gpui::FontWeight::SEMIBOLD)
        })
        .when(!is_group_kind, |d| {
            d.bg(rgb(SURF_LOW))
                .border_color(rgba(OUTLINE_VAR, 0.6))
                .text_color(rgb(MUTED))
                .font_weight(gpui::FontWeight::MEDIUM)
                .hover(|h| h.bg(rgb(SURF_HIGH)).text_color(rgb(TEXT)))
        })
        .child(icon_folder_file(
            if is_group_kind { PRIMARY } else { MUTED },
            12.,
        ))
        .child(tr_search_sort_kind(lang))
        .on_click(cx.listener(|this, _, _, cx| {
            this.search_toggle_group_by_kind(cx);
        }));

    // 控制栏与搜索框卡片（与 Apps / Declutter / Junk 页面保持统一设计语言）
    let controls_bar = card()
        .p_3()
        .flex()
        .items_center()
        .justify_between()
        .gap_3()
        .child(
            div()
                .flex()
                .items_center()
                .gap_3()
                .child(search_box)
                .child(status_tag),
        )
        .child(div().flex().items_center().child(kind_sort_btn));

    let name_arrow = if sort_col == crate::ui::SearchSortCol::Name {
        if sort_asc {
            " ▲"
        } else {
            " ▼"
        }
    } else {
        ""
    };
    let path_arrow = if sort_col == crate::ui::SearchSortCol::Path {
        if sort_asc {
            " ▲"
        } else {
            " ▼"
        }
    } else {
        ""
    };
    let size_arrow = if sort_col == crate::ui::SearchSortCol::Size {
        if sort_asc {
            " ▲"
        } else {
            " ▼"
        }
    } else {
        ""
    };

    // 表格头
    let table_header = div()
        .px_5()
        .py_2()
        .bg(rgb(SURF_LOW))
        .border_b_1()
        .border_color(rgba(OUTLINE_VAR, 0.4))
        .flex()
        .items_center()
        .gap_3()
        .child(
            div()
                .id("search-sort-name")
                .w(px(260.))
                .flex_none()
                .flex()
                .items_center()
                .gap_1()
                .cursor_pointer()
                .hover(|h| h.text_color(rgb(PRIMARY)))
                .when(sort_col == crate::ui::SearchSortCol::Name, |d| {
                    d.text_color(rgb(PRIMARY))
                        .font_weight(gpui::FontWeight::BOLD)
                })
                .when(sort_col != crate::ui::SearchSortCol::Name, |d| {
                    d.text_color(rgb(MUTED))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                })
                .text_xs()
                .child(tr_search_col_name(lang))
                .child(name_arrow)
                .on_click(cx.listener(|this, _, _, cx| {
                    this.search_toggle_sort(crate::ui::SearchSortCol::Name, cx);
                })),
        )
        .child(
            div()
                .id("search-sort-path")
                .flex_1()
                .min_w(px(0.))
                .flex()
                .items_center()
                .gap_1()
                .cursor_pointer()
                .hover(|h| h.text_color(rgb(PRIMARY)))
                .when(sort_col == crate::ui::SearchSortCol::Path, |d| {
                    d.text_color(rgb(PRIMARY))
                        .font_weight(gpui::FontWeight::BOLD)
                })
                .when(sort_col != crate::ui::SearchSortCol::Path, |d| {
                    d.text_color(rgb(MUTED))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                })
                .text_xs()
                .child(tr_search_col_path(lang))
                .child(path_arrow)
                .on_click(cx.listener(|this, _, _, cx| {
                    this.search_toggle_sort(crate::ui::SearchSortCol::Path, cx);
                })),
        )
        .child(
            div()
                .id("search-sort-size")
                .w(px(120.))
                .flex_none()
                .flex()
                .items_center()
                .justify_end()
                .gap_1()
                .cursor_pointer()
                .hover(|h| h.text_color(rgb(PRIMARY)))
                .when(sort_col == crate::ui::SearchSortCol::Size, |d| {
                    d.text_color(rgb(PRIMARY))
                        .font_weight(gpui::FontWeight::BOLD)
                })
                .when(sort_col != crate::ui::SearchSortCol::Size, |d| {
                    d.text_color(rgb(MUTED))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                })
                .text_xs()
                .child(tr_search_col_size(lang))
                .child(size_arrow)
                .on_click(cx.listener(|this, _, _, cx| {
                    this.search_toggle_sort(crate::ui::SearchSortCol::Size, cx);
                })),
        )
        .child(
            div()
                .w(px(140.))
                .flex_none()
                .text_center()
                .text_xs()
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(rgb(MUTED))
                .child(if lang == Language::Zh {
                    "操作"
                } else {
                    "Action"
                }),
        );

    // 列表底部信息条
    let footer_text = if query.trim().is_empty() {
        if lang == Language::Zh {
            "全盘 NTFS MFT 秒级索引支持".to_string()
        } else {
            "Powered by NTFS MFT Instant Index Engine".to_string()
        }
    } else {
        match lang {
            Language::Zh => format!("匹配到 {} 个文件 / 文件夹", results_len),
            Language::En => format!("Matched {} items", results_len),
        }
    };

    let list_footer = div()
        .px_5()
        .py_2()
        .bg(rgb(SURF_LOW))
        .border_t_1()
        .border_color(rgba(OUTLINE_VAR, 0.4))
        .flex()
        .items_center()
        .justify_between()
        .text_xs()
        .text_color(rgb(MUTED))
        .child(
            div()
                .font_weight(gpui::FontWeight::MEDIUM)
                .text_color(rgb(MUTED))
                .child(footer_text),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap_4()
                .child(div().text_color(rgb(MUTED)).child(if lang == Language::Zh {
                    "双击条目直接打开 · 点击表头自定义排序"
                } else {
                    "Double-click to open · Click header to sort"
                }))
                .child(
                    div()
                        .font_weight(gpui::FontWeight::MEDIUM)
                        .text_color(rgb(OUTLINE))
                        .child(if lang == Language::Zh {
                            "单次最多展示 500 项"
                        } else {
                            "Max 500 items"
                        }),
                ),
        );

    // 列表主体（占满卡片容器）
    let results_card = if indexing {
        card()
            .flex_1()
            .min_h(px(320.))
            .overflow_hidden()
            .flex()
            .flex_col()
            .child(loading_state_view(
                tr_search_indexing(lang),
                tr_search_building_index(lang),
                anim_phase,
            ))
    } else if results_empty {
        let empty_msg = if query.trim().is_empty() {
            tr_search_empty(lang)
        } else {
            tr_search_no_results(lang)
        };
        card()
            .flex_1()
            .min_h(px(320.))
            .overflow_hidden()
            .flex()
            .flex_col()
            .child(table_header)
            .child(
                div()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap_2()
                    .p_12()
                    .child(
                        div()
                            .text_base()
                            .font_weight(gpui::FontWeight::BOLD)
                            .text_color(rgb(MUTED))
                            .child(empty_msg),
                    ),
            )
            .child(list_footer)
    } else {
        render_results_card(root, table_header, list_footer, cx)
    };

    div()
        .id("file-search-view")
        .size_full()
        .min_w(px(0.))
        .p_8()
        .flex()
        .flex_col()
        .gap_4()
        .child(header)
        .child(controls_bar)
        .child(results_card)
        .into_any_element()
}

fn render_search_box(root: &Root, window: &mut Window, cx: &mut Context<Root>) -> Stateful<Div> {
    let lang = root.language;
    let focused = root.search.focus_handle.is_focused(window);
    let query = root.search.query.clone();
    let search_focus_handle = root.search.focus_handle.clone();
    let selection = root.search.sel.clone();
    let marked = root.search.marked.clone();
    let font_size = 13.0;

    let sel = crate::ui::text_input::clamp_to_boundary(&query, selection);

    crate::ui::components::search_box::search_box(
        crate::ui::components::search_box::SearchBoxSpec {
            id: SharedString::from("file-search-box"),
            focus_handle: &search_focus_handle,
            text: &query,
            placeholder: SharedString::from(tr_file_search_placeholder(lang)),
            selection: sel,
            marked,
            width: 380.,
            height: 34.,
            font_size,
            cursor_h: 15.,
            focused,
            cursor_visible: root.cursor_blink_visible,
            is_file_search: true,
        },
        |this, cx| this.file_search_clear(cx),
        |this, cx| this.file_search_backspace(cx),
        |this, cx| this.file_search_clear(cx),
        |this, bounds| {
            this.search.bounds = Some(bounds);
        },
        cx,
    )
}

fn render_results_card(
    root: &Root,
    table_header: Div,
    list_footer: Div,
    cx: &mut Context<Root>,
) -> Div {
    let n = root.search.results.len();

    let base = root.search.scroll.0.borrow().base_handle.clone();
    let metrics = scroll_metrics(&base, 340.0, n as f32 * ROW_H);

    let scrollbar_el = metrics.map(|m| {
        scrollbar("search-scroll-thumb", m, |thumb| {
            thumb.on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(|this, event: &gpui::MouseDownEvent, _, cx| {
                    let mouse_y: f32 = event.position.y.into();
                    let start_top: f32 =
                        (-this.search.scroll.0.borrow().base_handle.offset().y).into();
                    this.search.scroll_drag = Some((mouse_y, start_top.max(0.0)));
                    cx.notify();
                }),
            )
        })
    });

    let list_el = gpui::uniform_list(
        SharedString::from("search-results-rows"),
        n,
        cx.processor(move |this, range: std::ops::Range<usize>, _window, cx| {
            let lang = this.language;
            let results = &this.search.results;
            range
                .filter_map(|i| {
                    let hit = results.get(i)?;
                    Some(render_result_row(i, hit, lang, cx))
                })
                .collect()
        }),
    )
    .track_scroll(root.search.scroll.clone())
    .size_full()
    .when(metrics.is_some(), |l| l.pr(px(SCROLLBAR_W)))
    .into_any_element();

    card()
        .flex_1()
        .min_h(px(340.))
        .overflow_hidden()
        .flex()
        .flex_col()
        .child(table_header)
        .child(
            div()
                .flex_1()
                .min_h(px(0.))
                .relative()
                .child(list_el)
                .children(scrollbar_el)
                .child(drag_capture(
                    cx.entity(),
                    |this, mouse_y, cx| {
                        let Some(start) = this.search.scroll_drag else {
                            return;
                        };
                        let base = this.search.scroll.0.borrow().base_handle.clone();
                        if let Some(new_top) = drag_to_offset(&base, start, mouse_y) {
                            base.set_offset(gpui::point(px(0.0), px(-new_top)));
                            cx.notify();
                        }
                    },
                    |this, cx| {
                        if this.search.scroll_drag.take().is_some() {
                            cx.notify();
                        }
                    },
                )),
        )
        .child(list_footer)
}

fn render_result_row(
    i: usize,
    hit: &crate::core::disk::SearchHit,
    lang: Language,
    cx: &mut Context<Root>,
) -> AnyElement {
    let visual = FileVisualKind::from_name(&hit.name, hit.is_dir);
    let icon_badge = visual.badge(18.);

    let name_display = truncate(&hit.name, 35);
    let path_display = truncate(&hit.path, 110);
    let size_str = fmt_size(hit.size);

    let path_for_click = hit.path.clone();

    div()
        .id(SharedString::from(format!("search-row-{i}")))
        .w_full()
        .h(px(ROW_H))
        .flex()
        .items_center()
        .gap_3()
        .px_5()
        .border_b_1()
        .border_color(rgba(OUTLINE_VAR, 0.2))
        .hover(|h| h.bg(rgb(SURF_LOW)))
        .cursor_pointer()
        .child(
            // 名称与文件类型原生矢量图标
            div()
                .w(px(260.))
                .flex_none()
                .flex()
                .items_center()
                .gap_2()
                .child(icon_badge)
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .text_sm()
                        .font_weight(gpui::FontWeight::MEDIUM)
                        .text_color(rgb(TEXT))
                        .whitespace_nowrap()
                        .overflow_hidden()
                        .child(name_display),
                ),
        )
        .child(
            // 完整路径展示（截断）
            div()
                .flex_1()
                .min_w(px(0.))
                .text_xs()
                .text_color(rgb(MUTED))
                .whitespace_nowrap()
                .overflow_hidden()
                .child(path_display),
        )
        .child(
            // 大小展示（单行清晰对齐）
            div()
                .w(px(120.))
                .flex_none()
                .flex()
                .items_center()
                .justify_end()
                .child(
                    div()
                        .text_sm()
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(if hit.size >= 1024 * 1024 * 1024 {
                            rgb(PRIMARY)
                        } else {
                            rgb(TEXT)
                        })
                        .whitespace_nowrap()
                        .child(size_str),
                ),
        )
        .child(
            // 操作按钮组：[打开] + [定位]
            div()
                .w(px(140.))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .gap_2()
                .child(
                    // 打开按钮：主操作，清爽浅蓝底色 + 主色深蓝文字，悬停深蓝高亮
                    div()
                        .id(SharedString::from(format!("open-btn-{i}")))
                        .px_2p5()
                        .py(px(3.5))
                        .rounded_md()
                        .bg(rgba(PRIMARY, 0.08))
                        .border_1()
                        .border_color(rgba(PRIMARY, 0.28))
                        .hover(|h| {
                            h.bg(rgb(PRIMARY))
                                .border_color(rgb(PRIMARY))
                                .text_color(rgb(ON_PRIMARY))
                        })
                        .text_xs()
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(rgb(PRIMARY))
                        .cursor_pointer()
                        .child(if lang == Language::Zh {
                            "打开"
                        } else {
                            "Open"
                        })
                        .on_click(cx.listener({
                            let path = path_for_click.clone();
                            move |_this, _, _, cx| {
                                let p = std::path::PathBuf::from(&path);
                                crate::platform::open_in_default_app(&p);
                                cx.notify();
                            }
                        })),
                )
                .child(
                    // 定位按钮：次操作，纯白微卡片 + 精致边框 + 蓝色定位小图标
                    div()
                        .id(SharedString::from(format!("reveal-btn-{i}")))
                        .px_2p5()
                        .py(px(3.5))
                        .rounded_md()
                        .bg(rgb(CARD))
                        .border_1()
                        .border_color(rgba(OUTLINE_VAR, 0.7))
                        .hover(|h| {
                            h.bg(rgb(PRIMARY_FIXED))
                                .border_color(rgb(PRIMARY))
                                .text_color(rgb(PRIMARY))
                        })
                        .text_xs()
                        .font_weight(gpui::FontWeight::MEDIUM)
                        .text_color(rgb(TEXT))
                        .cursor_pointer()
                        .flex()
                        .items_center()
                        .gap_1()
                        .child(icon_locate(PRIMARY, 9.))
                        .child(if lang == Language::Zh {
                            "定位"
                        } else {
                            "Reveal"
                        })
                        .on_click(cx.listener({
                            let path = path_for_click.clone();
                            move |_this, _, _, cx| {
                                let p = std::path::PathBuf::from(&path);
                                crate::platform::reveal_in_explorer(&p);
                                cx.notify();
                            }
                        })),
                ),
        )
        // 双击整行也可以直接使用默认应用打开
        .on_mouse_down(
            MouseButton::Left,
            cx.listener({
                let path = path_for_click.clone();
                move |_this, event: &MouseDownEvent, _window, cx| {
                    if event.click_count >= 2 {
                        let p = std::path::PathBuf::from(&path);
                        crate::platform::open_in_default_app(&p);
                        cx.notify();
                    }
                }
            }),
        )
        .into_any_element()
}
