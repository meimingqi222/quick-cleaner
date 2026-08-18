//! 冗余整理通用 UI 组件与导航头 (Common UI & Navigation)

use super::DeclutterTab;
use crate::core::i18n::Language;
use crate::ui::theme::*;
use crate::ui::Root;
use gpui::prelude::*;
use gpui::{div, rgb, AnyElement, Context, Div, SharedString};

/// 顶部统一导航栏 (Stitch Unified Navigation)
pub fn render_unified_nav_header(
    current_tab: DeclutterTab,
    _current_title: &'static str,
    lang: Language,
    cx: &mut Context<Root>,
) -> Div {
    let tabs = [
        DeclutterTab::SimilarPhotos,
        DeclutterTab::Duplicates,
        DeclutterTab::LargeFiles,
        DeclutterTab::Downloads,
    ];

    let subtab_items = tabs.into_iter().enumerate().map(|(idx, tab)| {
        let active = tab == current_tab;
        div()
            .id(SharedString::from(format!("unified-subtab-{idx}")))
            .px_3()
            .py_1()
            .rounded_full()
            .text_xs()
            .font_weight(if active {
                gpui::FontWeight::BOLD
            } else {
                gpui::FontWeight::MEDIUM
            })
            .when(active, |d| d.bg(rgb(PRIMARY)).text_color(rgb(ON_PRIMARY)))
            .when(!active, |d| {
                d.text_color(rgb(MUTED))
                    .hover(|h| h.bg(rgb(SURF_HIGH)).text_color(rgb(TEXT)))
            })
            .cursor_pointer()
            .child(tab.title_lang(lang))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.declutter.tab = tab;
                cx.notify();
            }))
    });

    div()
        .flex_none()
        .flex()
        .items_center()
        .justify_between()
        .child(
            div()
                .id("btn-nav-back-to-overview")
                .flex()
                .items_center()
                .gap_2()
                .cursor_pointer()
                .hover(|h| h.opacity(0.8))
                .child(
                    div()
                        .text_sm()
                        .font_weight(gpui::FontWeight::BOLD)
                        .text_color(rgb(PRIMARY))
                        .child("‹"),
                )
                .child(
                    div()
                        .text_sm()
                        .font_weight(gpui::FontWeight::BOLD)
                        .text_color(rgb(TEXT))
                        .child(match lang {
                            Language::Zh => "返回概览",
                            Language::En => "Back to Overview",
                        }),
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.declutter.tab = DeclutterTab::Overview;
                    cx.notify();
                })),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .p_1()
                .rounded_full()
                .bg(rgb(SURF))
                .children(subtab_items),
        )
}

/// 通用空状态占位卡片 (Empty State Card)
pub fn render_empty_state_card(
    icon: &'static str,
    title: &'static str,
    desc: &'static str,
) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .p_12()
        .gap_3()
        .rounded_2xl()
        .bg(rgb(CARD))
        .border_1()
        .border_color(rgb(SURF_HIGH))
        .child(div().text_3xl().child(icon))
        .child(
            div()
                .text_sm()
                .font_weight(gpui::FontWeight::BOLD)
                .text_color(rgb(TEXT))
                .child(title),
        )
        .child(div().text_xs().text_color(rgb(MUTED)).child(desc))
        .into_any_element()
}
