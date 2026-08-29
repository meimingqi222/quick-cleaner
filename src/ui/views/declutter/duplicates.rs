//! 重复文件比对视图 (Duplicates Comparison View)

use super::common::{
    render_declutter_action_bar, render_empty_state_card, render_unified_nav_header,
};
use super::DeclutterTab;
use crate::core::model::fmt_size;
use crate::ui::components::controls::checkbox;
use crate::ui::components::icons::{icon_badge, icon_files_duplicate};
use crate::ui::i18n::*;
use crate::ui::theme::*;
use crate::ui::Root;
use gpui::prelude::*;
use gpui::{div, px, rgb, AnyElement, Context, SharedString};

pub fn render_duplicates_tab(root: &Root, cx: &mut Context<Root>) -> AnyElement {
    let lang = root.language;
    let state = &root.declutter;

    let total_cleanable: u64 = state
        .duplicate_groups
        .iter()
        .map(|g| g.cleanable_size())
        .sum();
    let total_selected_count: usize = state
        .duplicate_groups
        .iter()
        .flat_map(|g| &g.files)
        .filter(|f| f.selected)
        .count();

    let tab_nav = render_unified_nav_header(
        DeclutterTab::Duplicates,
        tr_declutter_duplicates_title(lang),
        lang,
        cx,
    );

    // 取前 50 组展示以保证 GPUI 极速流畅渲染
    let display_groups: Vec<_> = state.duplicate_groups.iter().take(50).enumerate().collect();

    let groups_view: Vec<AnyElement> = if display_groups.is_empty() {
        vec![render_empty_state_card(
            "📑",
            tr_declutter_duplicates_empty_title(lang),
            tr_declutter_duplicates_empty_desc(lang),
        )]
    } else {
        display_groups
            .into_iter()
            .map(|(g_idx, group)| {
                let file_rows = group.files.iter().enumerate().map(|(f_idx, file)| {
                    let is_sel = file.selected;
                    let is_orig = file.is_original;
                    let f_path = file.path.clone();
                    let f_name = group.filename.clone();

                    div()
                        .id(SharedString::from(format!("dup-row-{g_idx}-{f_idx}")))
                        .flex_none()
                        .flex()
                        .items_center()
                        .justify_between()
                        .px_6()
                        .py_3()
                        .border_b_1()
                        .border_color(rgba(OUTLINE_VAR, 0.2))
                        .when(is_sel, |d| d.bg(rgba(ERROR, 0.06)))
                        .when(!is_sel, |d| d.hover(|h| h.bg(rgb(SURF_LOW))))
                        .on_mouse_down(
                            gpui::MouseButton::Right,
                            cx.listener(move |this, event: &gpui::MouseDownEvent, _, cx| {
                                let x: f32 = event.position.x.into();
                                let y: f32 = event.position.y.into();
                                this.open_declutter_context_menu(
                                    f_path.clone(),
                                    f_name.clone(),
                                    x,
                                    y,
                                );
                                cx.notify();
                            }),
                        )
                        .child(
                            div()
                                .id(SharedString::from(format!("dup-sel-{g_idx}-{f_idx}")))
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
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w(px(0.))
                                        .flex()
                                        .flex_col()
                                        .gap(px(2.))
                                        .child(
                                            div()
                                                .text_xs()
                                                .font_weight(gpui::FontWeight::MEDIUM)
                                                .text_color(rgb(TEXT))
                                                .overflow_hidden()
                                                .child(file.path_display.clone()),
                                        )
                                        .child(
                                            div()
                                                .text_xs()
                                                .text_color(rgb(OUTLINE))
                                                .overflow_hidden()
                                                .child(tr_declutter_modified_at(
                                                    lang,
                                                    &file.modified_at_str,
                                                )),
                                        ),
                                )
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if let Some(g) = this.declutter.duplicate_groups.get_mut(g_idx)
                                    {
                                        if let Some(f) = g.files.get_mut(f_idx) {
                                            f.selected = !f.selected;
                                            cx.notify();
                                        }
                                    }
                                })),
                        )
                        .child(
                            div()
                                .flex_none()
                                .flex()
                                .items_center()
                                .gap_2()
                                .child(
                                    div()
                                        .id(SharedString::from(format!(
                                            "btn-reveal-dup-{g_idx}-{f_idx}"
                                        )))
                                        .px_2()
                                        .py(px(2.))
                                        .rounded_md()
                                        .bg(rgb(SURF_HIGH))
                                        .hover(|h| {
                                            h.bg(rgb(PRIMARY_FIXED)).text_color(rgb(PRIMARY))
                                        })
                                        .text_xs()
                                        .font_weight(gpui::FontWeight::MEDIUM)
                                        .text_color(rgb(MUTED))
                                        .cursor_pointer()
                                        .child(tr_declutter_reveal(lang))
                                        .on_click({
                                            let p = file.path.clone();
                                            cx.listener(
                                                move |_, _event: &gpui::ClickEvent, _, cx| {
                                                    cx.stop_propagation();
                                                    crate::platform::reveal_in_explorer(&p);
                                                },
                                            )
                                        }),
                                )
                                .child(
                                    div()
                                        .id(SharedString::from(format!(
                                            "btn-open-dup-{g_idx}-{f_idx}"
                                        )))
                                        .px_2()
                                        .py(px(2.))
                                        .rounded_md()
                                        .bg(rgb(SURF_HIGH))
                                        .hover(|h| {
                                            h.bg(rgb(PRIMARY_FIXED)).text_color(rgb(PRIMARY))
                                        })
                                        .text_xs()
                                        .font_weight(gpui::FontWeight::MEDIUM)
                                        .text_color(rgb(MUTED))
                                        .cursor_pointer()
                                        .child(tr_declutter_open(lang))
                                        .on_click({
                                            let p = file.path.clone();
                                            cx.listener(
                                                move |_, _event: &gpui::ClickEvent, _, cx| {
                                                    cx.stop_propagation();
                                                    crate::platform::open_in_default_app(&p);
                                                },
                                            )
                                        }),
                                )
                                .when(is_orig, |d| {
                                    d.child(
                                        div()
                                            .px_2()
                                            .py(px(2.))
                                            .rounded_md()
                                            .bg(rgb(SURF_HIGH))
                                            .text_xs()
                                            .font_weight(gpui::FontWeight::BOLD)
                                            .text_color(rgb(OUTLINE))
                                            .child(tr_declutter_duplicates_original(lang)),
                                    )
                                })
                                .when(!is_orig && is_sel, |d| {
                                    d.child(
                                        div()
                                            .px_2()
                                            .py(px(2.))
                                            .rounded_md()
                                            .bg(rgb(ERROR_CONTAINER))
                                            .text_xs()
                                            .font_weight(gpui::FontWeight::BOLD)
                                            .text_color(rgb(ERROR))
                                            .child(tr_declutter_duplicates_copy(lang)),
                                    )
                                }),
                        )
                });

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
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(0.))
                                    .flex()
                                    .items_center()
                                    .gap_3()
                                    .child(icon_badge(
                                        icon_files_duplicate(0x7547ab, 18.),
                                        0xefdbff,
                                        0x7547ab,
                                        32.,
                                    ))
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w(px(0.))
                                            .flex()
                                            .flex_col()
                                            .child(
                                                div()
                                                    .text_sm()
                                                    .font_weight(gpui::FontWeight::BOLD)
                                                    .text_color(rgb(TEXT))
                                                    .overflow_hidden()
                                                    .child(group.filename.clone()),
                                            )
                                            .child(
                                                div()
                                                    .text_xs()
                                                    .text_color(rgb(OUTLINE))
                                                    .overflow_hidden()
                                                    .child(tr_declutter_duplicates_group_sub(
                                                        lang,
                                                        &fmt_size(group.size_per_copy),
                                                        group.files.len(),
                                                    )),
                                            ),
                                    ),
                            ),
                    )
                    .child(div().flex().flex_col().children(file_rows))
                    .into_any_element()
            })
            .collect()
    };

    div()
        .id("declutter-dups-view")
        .size_full()
        .flex()
        .flex_col()
        .child(
            div()
                .id("declutter-dups-scroll-inner")
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
                        .flex()
                        .items_end()
                        .justify_between()
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap_1()
                                .child(
                                    div()
                                        .text_2xl()
                                        .font_weight(gpui::FontWeight::BOLD)
                                        .text_color(rgb(TEXT))
                                        .child(tr_declutter_duplicates_heading(lang)),
                                )
                                .child(div().text_xs().text_color(rgb(MUTED)).child(
                                    tr_declutter_duplicates_summary(
                                        lang,
                                        state.duplicate_groups.len(),
                                    ),
                                )),
                        )
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_2()
                                .child(
                                    div()
                                        .id("btn-auto-mark-newest")
                                        .px_4()
                                        .py_2()
                                        .rounded_lg()
                                        .bg(rgb(SURF_HIGH))
                                        .border_1()
                                        .border_color(rgba(OUTLINE_VAR, 0.4))
                                        .text_xs()
                                        .font_weight(gpui::FontWeight::MEDIUM)
                                        .text_color(rgb(TEXT))
                                        .hover(|h| h.bg(rgb(SURF_LOW)))
                                        .cursor_pointer()
                                        .child(tr_declutter_duplicates_keep_newest(lang))
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.declutter.pick_duplicates_keep_newest();
                                            cx.notify();
                                        })),
                                )
                                .child(
                                    div()
                                        .id("btn-auto-mark-oldest")
                                        .px_4()
                                        .py_2()
                                        .rounded_lg()
                                        .bg(rgb(SURF_HIGH))
                                        .border_1()
                                        .border_color(rgba(OUTLINE_VAR, 0.4))
                                        .text_xs()
                                        .font_weight(gpui::FontWeight::MEDIUM)
                                        .text_color(rgb(TEXT))
                                        .hover(|h| h.bg(rgb(SURF_LOW)))
                                        .cursor_pointer()
                                        .child(tr_declutter_duplicates_keep_oldest(lang))
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.declutter.pick_duplicates_keep_oldest();
                                            cx.notify();
                                        })),
                                ),
                        ),
                )
                .children(groups_view),
        )
        // 底部操作条：与其余三个页签共用 common.rs 里的实现，见其上注释。
        .child(render_declutter_action_bar(
            lang,
            DeclutterTab::Duplicates,
            total_selected_count,
            total_cleanable,
            false,
            cx,
        ))
        .into_any_element()
}
