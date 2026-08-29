//! 下载项整理视图 (Downloads View)

use super::common::{
    render_declutter_action_bar, render_empty_state_card, render_unified_nav_header,
};
use super::DeclutterTab;
use crate::core::i18n::Language;
use crate::core::model::fmt_size;
use crate::ui::components::controls::checkbox;
use crate::ui::components::icons::{icon_badge, icon_downloads};
use crate::ui::i18n::*;
use crate::ui::theme::*;
use crate::ui::Root;
use gpui::prelude::*;
use gpui::{div, px, rgb, AnyElement, Context, SharedString};

pub fn render_downloads_tab(root: &Root, cx: &mut Context<Root>) -> AnyElement {
    let lang = root.language;
    let state = &root.declutter;

    let tab_nav = render_unified_nav_header(
        DeclutterTab::Downloads,
        tr_declutter_downloads_title(lang),
        lang,
        cx,
    );
    let total_sel: u64 = state.total_downloads_cleanable();
    let total_sel_count = state.download_items.iter().filter(|f| f.selected).count();

    let rows: Vec<AnyElement> = if state.download_items.is_empty() {
        vec![render_empty_state_card(
            "📥",
            tr_declutter_downloads_empty_title(lang),
            tr_declutter_downloads_empty_desc(lang),
        )]
    } else {
        state
            .download_items
            .iter()
            .take(100)
            .enumerate()
            .map(|(idx, item)| {
                let is_sel = item.selected;
                let d_path = item.path.clone();
                let d_name = item.filename.clone();

                div()
                    .id(SharedString::from(format!("down-row-{idx}")))
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
                            this.open_declutter_context_menu(d_path.clone(), d_name.clone(), x, y);
                            cx.notify();
                        }),
                    )
                    .child(
                        div()
                            .id(SharedString::from(format!("down-sel-{idx}")))
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
                            .child(icon_badge(
                                icon_downloads(0x0078d4, 18.),
                                0xe0f2fe,
                                0x0078d4,
                                36.,
                            ))
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
                                            .child(item.path.to_string_lossy().to_string()),
                                    ),
                            )
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if let Some(f) = this.declutter.download_items.get_mut(idx) {
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
                                    .id(SharedString::from(format!("btn-reveal-down-{idx}")))
                                    .px_2()
                                    .py(px(2.))
                                    .rounded_md()
                                    .bg(rgb(SURF_HIGH))
                                    .hover(|h| h.bg(rgb(PRIMARY_FIXED)).text_color(rgb(PRIMARY)))
                                    .text_xs()
                                    .font_weight(gpui::FontWeight::MEDIUM)
                                    .text_color(rgb(MUTED))
                                    .cursor_pointer()
                                    .child(tr_declutter_reveal(lang))
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
                                    .id(SharedString::from(format!("btn-open-down-{idx}")))
                                    .px_2()
                                    .py(px(2.))
                                    .rounded_md()
                                    .bg(rgb(SURF_HIGH))
                                    .hover(|h| h.bg(rgb(PRIMARY_FIXED)).text_color(rgb(PRIMARY)))
                                    .text_xs()
                                    .font_weight(gpui::FontWeight::MEDIUM)
                                    .text_color(rgb(MUTED))
                                    .cursor_pointer()
                                    .child(tr_declutter_open(lang))
                                    .on_click({
                                        let p = item.path.clone();
                                        cx.listener(move |_, _event: &gpui::ClickEvent, _, cx| {
                                            cx.stop_propagation();
                                            crate::platform::open_in_default_app(&p);
                                        })
                                    }),
                            )
                            .child(
                                div().w(px(60.)).flex().justify_center().child(
                                    div()
                                        .px_2()
                                        .py(px(2.))
                                        .rounded_md()
                                        .bg(rgb(SURF_HIGH))
                                        .text_xs()
                                        .text_color(rgb(MUTED))
                                        .child(if lang == Language::Zh {
                                            item.kind_zh
                                        } else {
                                            item.kind_en
                                        }),
                                ),
                            )
                            .child(
                                div()
                                    .w(px(90.))
                                    .text_right()
                                    .text_sm()
                                    .font_weight(gpui::FontWeight::BOLD)
                                    .text_color(rgb(PRIMARY))
                                    .child(fmt_size(item.size)),
                            )
                            .child(
                                div()
                                    .w(px(90.))
                                    .text_right()
                                    .text_xs()
                                    .text_color(rgb(MUTED))
                                    .child(item.downloaded_at_str.get(lang).to_string()),
                            ),
                    )
                    .into_any_element()
            })
            .collect()
    };

    div()
        .id("declutter-downloads-view")
        .size_full()
        .flex()
        .flex_col()
        .child(
            div()
                .id("declutter-downloads-scroll-inner")
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
                                        .child(tr_declutter_downloads_heading(lang)),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(rgb(MUTED))
                                        .child(tr_declutter_downloads_subheading(lang)),
                                ),
                        )
                        .child(
                            div()
                                .text_xl()
                                .font_weight(gpui::FontWeight::BOLD)
                                .text_color(rgb(PRIMARY))
                                .child(tr_declutter_downloads_count(
                                    lang,
                                    state.download_items.len(),
                                )),
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
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w(px(0.))
                                        .child(tr_declutter_col_name(lang)),
                                )
                                .child(
                                    div()
                                        .flex_none()
                                        .flex()
                                        .items_center()
                                        .gap_8()
                                        .child(
                                            div()
                                                .w(px(70.))
                                                .text_center()
                                                .child(tr_declutter_col_kind(lang)),
                                        )
                                        .child(
                                            div()
                                                .w(px(100.))
                                                .text_right()
                                                .child(tr_declutter_col_size(lang)),
                                        )
                                        .child(
                                            div()
                                                .w(px(100.))
                                                .text_right()
                                                .child(tr_declutter_col_downloaded(lang)),
                                        ),
                                ),
                        )
                        .child(div().flex().flex_col().children(rows)),
                ),
        )
        .child(render_declutter_action_bar(
            lang,
            DeclutterTab::Downloads,
            total_sel_count,
            total_sel,
            false,
            cx,
        ))
        .into_any_element()
}
