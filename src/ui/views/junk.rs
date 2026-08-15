//! 智能清理视图与操作底栏 (CleanFlow 质感分层清理)

use crate::core::categories::CategoryId;
use crate::core::model::{commas, fmt_size, truncate, Check};
use crate::ui::components::buttons::danger_button;
use crate::ui::components::cards::card;
use crate::ui::components::controls::{badge, checkbox, loading_state_view, page_heading};
use crate::ui::components::icons::*;
use crate::ui::theme::*;
use crate::ui::Root;
use gpui::{div, prelude::*, px, rgb, AnyElement, Context, Div, IntoElement, SharedString};

fn right_cell(width: f32, text: String, color: u32, bold: bool) -> Div {
    div()
        .w(px(width))
        .flex_none()
        .text_right()
        .text_color(rgb(color))
        .when(bold, |d| d.font_weight(gpui::FontWeight::SEMIBOLD))
        .child(text)
}

fn category_icon(cat: CategoryId, fg: u32, size: f32) -> AnyElement {
    match cat {
        CategoryId::SystemTemp => icon_trash(fg, size),
        CategoryId::UserTemp => icon_sparkle(fg, size),
        CategoryId::BrowserCache => icon_dashboard(fg, size),
        CategoryId::PackageCache => icon_apps(fg, size),
        CategoryId::Logs => icon_clock(fg, size),
        CategoryId::RecycleBin => icon_trash(fg, size),
        CategoryId::Thumbnails => icon_sparkle(fg, size),
        CategoryId::AiAgents => icon_sparkle(fg, size),
        CategoryId::DevBuild => icon_gear(fg, size),
        CategoryId::DevWorktrees => icon_shield(fg, size),
    }
}

pub fn render_junk_view(root: &Root, cx: &mut Context<Root>) -> AnyElement {
    let total = root.total_cleanable();

    let found = card()
        .flex_none()
        .px_5()
        .py_3()
        .flex()
        .items_center()
        .gap_4()
        .child(
            div()
                .flex()
                .flex_col()
                .child(
                    div()
                        .text_xs()
                        .font_weight(gpui::FontWeight::MEDIUM)
                        .text_color(rgb(OUTLINE))
                        .child("共发现可清理"),
                )
                .child(
                    div()
                        .text_xl()
                        .font_weight(gpui::FontWeight::BOLD)
                        .text_color(rgb(if total > 0 { ERROR } else { PRIMARY }))
                        .child(if root.scanned {
                            fmt_size(total)
                        } else {
                            String::from("—")
                        }),
                ),
        )
        .child(
            div()
                .w(px(36.))
                .h(px(36.))
                .flex_none()
                .rounded_full()
                .bg(rgb(if total > 0 { ERROR_CONTAINER } else { PRIMARY_FIXED }))
                .flex()
                .items_center()
                .justify_center()
                .child(icon_trash(if total > 0 { ERROR } else { PRIMARY }, 18.)),
        );

    let header = div()
        .flex()
        .items_center()
        .justify_between()
        .gap_4()
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .child(page_heading(
                    "智能清理",
                    "系统缓存、浏览器与包管理缓存默认已勾选；AI 助手缓存与项目构建产物需手动勾选",
                )),
        )
        .child(found);

    let mut cards: Vec<AnyElement> = Vec::new();
    for summary in &root.categories {
        let id = summary.category;
        let size = summary.total_size;
        let safety = id.safety();
        let expanded = root.expanded.contains(&id);
        let state = root.cat_check(summary);
        let dim = size == 0;

        let head = div()
            .flex()
            .items_center()
            .gap_4()
            .px_5()
            .py_4()
            .child(
                div()
                    .id(SharedString::from(format!("cb-{}", id.name())))
                    .flex_none()
                    .cursor_pointer()
                    .child(checkbox(state))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.toggle_category(id);
                        cx.notify();
                    })),
            )
            .child(
                div()
                    .id(SharedString::from(format!("row-{}", id.name())))
                    .flex_1()
                    .min_w(px(0.))
                    .flex()
                    .items_center()
                    .gap_4()
                    .cursor_pointer()
                    .child(icon_badge(
                        category_icon(id, safety_color(safety), 18.),
                        safety_container(safety),
                        safety_color(safety),
                        38.,
                    ))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .child(
                                        div()
                                            .text_base()
                                            .font_weight(gpui::FontWeight::BOLD)
                                            .text_color(rgb(TEXT))
                                            .child(id.name()),
                                    )
                                    // 开发者类目默认不勾选，用徽标说明「要自己勾」，
                                    // 免得用户以为扫出来了却没被清掉是 bug
                                    .when(id.is_developer(), |d| {
                                        d.child(badge(
                                            "需手动勾选".into(),
                                            safety_container(safety),
                                            safety_color(safety),
                                        ))
                                    }),
                            )
                            .child(div().text_xs().text_color(rgb(MUTED)).child(id.desc())),
                    )
                    .child(
                        div()
                            .text_sm()
                            .font_weight(gpui::FontWeight::BOLD)
                            .text_color(rgb(if dim { OUTLINE } else { safety_color(safety) }))
                            .child(if size > 0 {
                                fmt_size(size)
                            } else {
                                String::from("0 B")
                            }),
                    )
                    .child(
                        div()
                            .w(px(16.))
                            .flex_none()
                            .text_xs()
                            .font_weight(gpui::FontWeight::BOLD)
                            .text_color(rgb(OUTLINE))
                            .child(if expanded { "▾" } else { "▸" }),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.toggle_expand(id);
                        cx.notify();
                    })),
            );

        let mut sub_rows: Vec<AnyElement> = Vec::new();
        if expanded {
            for (idx, item) in summary.items.iter().enumerate() {
                let path_buf = item.path.clone();
                let checked = root.selected.contains(&path_buf);
                let item_dim = item.size == 0;
                let is_even = idx % 2 == 0;

                sub_rows.push(
                    div()
                        .id(SharedString::from(format!("item-{}", item.path.display())))
                        .flex()
                        .items_center()
                        .gap_3()
                        .px_5()
                        .py_2()
                        .bg(if is_even { rgb(CARD) } else { rgb(SURF_LOW) })
                        .border_t_1()
                        .border_color(rgba(OUTLINE_VAR, 0.3))
                        .hover(|h| h.bg(rgb(SURF)))
                        .child(
                            div()
                                .id(SharedString::from(format!("cb-item-{}", item.path.display())))
                                .flex_none()
                                .cursor_pointer()
                                .child(checkbox(if checked {
                                    Check::On
                                } else {
                                    Check::Off
                                }))
                                .on_click(cx.listener({
                                    let pb = path_buf.clone();
                                    move |this, _, _, cx| {
                                        this.toggle_item(&pb);
                                        cx.notify();
                                    }
                                })),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(0.))
                                .flex()
                                .flex_col()
                                .child(
                                    div()
                                        .text_sm()
                                        .font_weight(gpui::FontWeight::MEDIUM)
                                        .text_color(rgb(TEXT))
                                        .child(item.label.clone()),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(rgb(OUTLINE))
                                        .child(truncate(&item.path.to_string_lossy(), 60)),
                                ),
                        )
                        .child(right_cell(
                            70.,
                            if item.file_count > 0 {
                                format!("{} 个文件", commas(item.file_count))
                            } else {
                                String::from("0")
                            },
                            OUTLINE,
                            false,
                        ))
                        .child(right_cell(
                            85.,
                            if item.size > 0 {
                                fmt_size(item.size)
                            } else {
                                String::from("0 B")
                            },
                            if item_dim { OUTLINE } else { TEXT },
                            !item_dim,
                        ))
                        .into_any_element(),
                );
            }
        }

        cards.push(
            card()
                .overflow_hidden()
                .child(head)
                .when(expanded, |d| d.children(sub_rows))
                .into_any_element(),
        );
    }

    let mut skipped_banner: Option<AnyElement> = None;
    if !root.last_failed.is_empty() {
        skipped_banner = Some(
            card()
                .p_4()
                .bg(rgb(ERROR_CONTAINER))
                .border_color(rgba(ERROR, 0.4))
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
                                .text_sm()
                                .font_weight(gpui::FontWeight::BOLD)
                                .text_color(rgb(ERROR))
                                .child(format!(
                                    "⚠️ 上次清理有 {} 处项目被占用或受系统保护而安全跳过",
                                    root.last_failed.len()
                                )),
                        )
                        .child(
                            div()
                                .id("failed-details-toggle")
                                .text_xs()
                                .font_weight(gpui::FontWeight::BOLD)
                                .text_color(rgb(ERROR))
                                .cursor_pointer()
                                .child(if root.show_failed_details {
                                    "收起详情 ▴"
                                } else {
                                    "查看详情 ▾"
                                })
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.show_failed_details = !this.show_failed_details;
                                    cx.notify();
                                })),
                        ),
                )
                .when(root.show_failed_details, |d| {
                    d.child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .p_2()
                            .rounded_md()
                            .bg(rgba(CARD, 0.6))
                            .children(root.last_failed.iter().take(20).map(|p| {
                                div()
                                    .text_xs()
                                    .text_color(rgb(ERROR))
                                    .child(truncate(&p.to_string_lossy(), 80))
                                    .into_any_element()
                            })),
                    )
                })
                .into_any_element(),
        );
    }

    let body: AnyElement = if root.scanning {
        loading_state_view(
            "正在全面扫描系统冗余垃圾",
            "安全检索系统临时缓存、应用日志与回收站，准备释放空间",
            root.anim_phase,
        )
    } else {
        div()
            .flex()
            .flex_col()
            .gap_3()
            .children(cards)
            .into_any_element()
    };

    div()
        .id("junk-scroll")
        .size_full()
        .min_w(px(0.))
        .overflow_scroll()
        .p_8()
        .flex()
        .flex_col()
        .gap_5()
        .child(header)
        .children(skipped_banner)
        .child(body)
        .into_any_element()
}

pub fn render_clean_bar(root: &Root, cx: &mut Context<Root>) -> impl IntoElement {
    let size = root.selected_size();
    let count = root.selected_count();
    let enabled = root.scanned && !root.cleaning && !root.scanning && count > 0;

    div()
        .flex_none()
        .w_full()
        .px_8()
        .py_3()
        .bg(rgb(BG))
        .border_t_1()
        .border_color(rgba(OUTLINE_VAR, 0.5))
        .flex()
        .justify_center()
        .child(
            div()
                .flex()
                .items_center()
                .gap_6()
                .pl_6()
                .pr(px(6.))
                .py(px(6.))
                .rounded_full()
                .bg(rgb(CARD))
                .border_1()
                .border_color(rgba(OUTLINE_VAR, 0.6))
                .shadow_lg()
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .text_xs()
                                .font_weight(gpui::FontWeight::MEDIUM)
                                .text_color(rgb(OUTLINE))
                                .child("已选择清理"),
                        )
                        .child(
                            div()
                                .flex()
                                .items_baseline()
                                .gap_2()
                                .child(
                                    div()
                                        .text_lg()
                                        .font_weight(gpui::FontWeight::BOLD)
                                        .text_color(rgb(TEXT))
                                        .child(fmt_size(size)),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .font_weight(gpui::FontWeight::MEDIUM)
                                        .text_color(rgb(OUTLINE))
                                        .child(format!("({count} 项)")),
                                ),
                        ),
                )
                .child(
                    div()
                        .id("clean-now")
                        .child(danger_button(
                            if root.cleaning {
                                String::from("清理中…")
                            } else {
                                String::from("✨ 立即清理")
                            },
                            enabled,
                        ))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.request_clean_selected(cx);
                        })),
                ),
        )
}
