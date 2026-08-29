//! 冗余整理通用 UI 组件与导航头 (Common UI & Navigation)

use super::DeclutterTab;
use crate::core::i18n::Language;
use crate::core::model::fmt_size;
use crate::ui::i18n::*;
use crate::ui::theme::*;
use crate::ui::Root;
use gpui::prelude::*;
use gpui::{div, px, rgb, AnyElement, Context, Div, SharedString};

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
                        .child(tr_declutter_back_to_overview(lang)),
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

/// 底部「清理所选项」操作条 (Bottom Action Bar)
///
/// 下载项/重复文件/大文件/相似照片四个页签末尾原来各自摊了一份几乎一样的
/// h(70) 横条，彼此只是按钮 id、有没有 icon_trash、按钮文案带不带 "›"、
/// 主色是 PRIMARY 还是 ERROR 这类无意义的手滑差异，和各页签的业务逻辑毫无
/// 关系，收敛成一份，以 downloads.rs 那份的视觉为准（四份里的多数派）。
///
/// 顺带修一处措辞 bug：这里的「清理」是把文件移入废纸篓，并不释放磁盘
/// 空间，左侧文案原来写的「已选 N 个项目 • 释放 X」用词不对（参见
/// `core/cleaner.rs` 里 `recycle_path` 的注释：「已释放 X」必须是真的释放
/// 了才能这么写），统一改成中性的「共 X」/「X total」。
pub fn render_declutter_action_bar(
    lang: Language,
    tab: DeclutterTab,
    selected_count: usize,
    selected_size: u64,
    // show_clear_selection：是否显示「取消全选」。目前只有大型与旧文件页有这个
    // 入口，收敛操作条时它一度被整个删掉——那是功能回退，不是重复代码，所以
    // 按页签保留而不是一刀切。
    show_clear_selection: bool,
    cx: &mut Context<Root>,
) -> Div {
    // 沿用各页签原有的 button id 后缀，避免无谓的 diff；SimilarPhotos 之前
    // 的按钮叫 btn-delete-selected-photos，这里统一进 btn-remove-selected-*
    // 命名族，没有外部代码或测试引用旧 id。
    let id_suffix = match tab {
        DeclutterTab::Downloads => "downloads",
        DeclutterTab::Duplicates => "dups",
        DeclutterTab::LargeFiles => "large",
        DeclutterTab::SimilarPhotos => "photos",
        DeclutterTab::Overview => "overview",
    };

    div()
        .h(px(70.))
        .flex_none()
        .px_8()
        .bg(rgb(CARD))
        .border_t_1()
        .border_color(rgba(OUTLINE_VAR, 0.35))
        .shadow_md()
        .flex()
        .items_center()
        .justify_between()
        .child(
            div()
                .text_sm()
                .font_weight(gpui::FontWeight::MEDIUM)
                .text_color(rgb(TEXT))
                .child(tr_declutter_selected_summary(
                    lang,
                    selected_count,
                    &fmt_size(selected_size),
                )),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap_3()
                .when(show_clear_selection, |row| {
                    row.child(
                        div()
                            .id(SharedString::from(format!("btn-cancel-{id_suffix}")))
                            .px_5()
                            .py_2()
                            .rounded_lg()
                            .border_1()
                            .border_color(rgb(OUTLINE_VAR))
                            .hover(|h| h.bg(rgb(SURF_LOW)))
                            .cursor_pointer()
                            .text_sm()
                            .text_color(rgb(MUTED))
                            .child(tr_declutter_cancel_selection(lang))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.clear_declutter_selection(tab, cx);
                            })),
                    )
                })
                .child(
                    div()
                        .id(SharedString::from(format!(
                            "btn-remove-selected-{id_suffix}"
                        )))
                        .px_6()
                        .py_2()
                        .rounded_lg()
                        .bg(rgb(PRIMARY))
                        .when(selected_count > 0, |d| {
                            d.hover(|h| h.bg(rgb(PRIMARY_BRIGHT))).cursor_pointer()
                        })
                        .when(selected_count == 0, |d| d.opacity(0.4))
                        .text_sm()
                        .font_weight(gpui::FontWeight::BOLD)
                        .text_color(rgb(ON_PRIMARY))
                        .child(tr_declutter_remove_selected(lang))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.clean_declutter_selected(tab, cx);
                        })),
                ),
        )
}
