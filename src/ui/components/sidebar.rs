//! 侧边栏导航控件 (CleanFlow / Modern macOS & Windows 11 设计风格)

use crate::core::i18n::Language;
use crate::ui::components::icons::*;
use crate::ui::i18n::*;
use crate::ui::theme::*;
use crate::ui::Root;
use gpui::{div, prelude::*, px, rgb, AnyElement, Context, IntoElement, SharedString};

/// 主区域当前显示哪个视图
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum View {
    Dashboard,
    Junk,
    Apps,
    Disk,
}

impl View {
    pub const ALL: [View; 4] = [View::Dashboard, View::Junk, View::Apps, View::Disk];

    /// 中文文案。**仅供日志与命令行**，界面上用 `title_lang(lang)`——
    /// 顶栏标题就曾经因为调了这个而在英文模式下一直显示中文。
    pub fn title(&self) -> &'static str {
        self.title_lang(Language::Zh)
    }

    pub fn title_lang(&self, lang: Language) -> &'static str {
        match (self, lang) {
            (View::Dashboard, Language::Zh) => "概览扫描",
            (View::Dashboard, Language::En) => "Overview",
            (View::Junk, Language::Zh) => "智能清理",
            (View::Junk, Language::En) => "Smart Clean",
            (View::Apps, Language::Zh) => "软件管理",
            (View::Apps, Language::En) => "Apps",
            (View::Disk, Language::Zh) => "磁盘透镜",
            (View::Disk, Language::En) => "Disk Lens",
        }
    }

    pub fn render_icon(&self, fg: u32) -> AnyElement {
        match self {
            View::Dashboard => icon_dashboard(fg, 18.),
            View::Junk => icon_trash(fg, 18.),
            View::Apps => icon_apps(fg, 18.),
            View::Disk => icon_disk(fg, 18.),
        }
    }
}

pub fn render_sidebar(root: &Root, cx: &mut Context<Root>) -> impl IntoElement {
    let current = root.view;
    let lang = root.language;

    let nav_item = |v: View, cx: &mut Context<Root>| {
        let active = current == v;
        let fg_color = if active { PRIMARY } else { MUTED };

        div()
            .id(SharedString::from(format!(
                "nav-{}",
                v.title_lang(Language::En)
            )))
            .h(px(44.))
            .flex()
            .items_center()
            .gap_3()
            .px_3()
            .rounded_xl()
            .cursor_pointer()
            .when(active, |d| d.bg(rgb(PRIMARY_FIXED)))
            .when(!active, |d| d.hover(|h| h.bg(rgb(SURF_LOW))))
            .child(
                div()
                    .w(px(24.))
                    .h(px(24.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(v.render_icon(fg_color)),
            )
            .child(
                div()
                    .flex_1()
                    .text_sm()
                    .font_weight(if active {
                        gpui::FontWeight::BOLD
                    } else {
                        gpui::FontWeight::MEDIUM
                    })
                    .text_color(rgb(fg_color))
                    .child(v.title_lang(lang)),
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                this.view = v;
                if v == View::Disk && this.disk.mft.is_none() && this.disk.error.is_none() {
                    this.start_mft_scan(cx);
                }
                if v == View::Apps && !this.apps.scanned && !this.apps.scanning {
                    this.start_apps_scan(cx);
                }
                cx.notify();
            }))
    };

    let items: Vec<gpui::AnyElement> = View::ALL
        .into_iter()
        .map(|v| nav_item(v, cx).into_any_element())
        .collect();

    div()
        .w(px(240.))
        .flex_none()
        .h_full()
        .bg(rgb(BG))
        .border_r_1()
        .border_color(rgba(OUTLINE_VAR, 0.5))
        .flex()
        .flex_col()
        .justify_between()
        // 顶部品牌与导航
        .child(
            div()
                .flex()
                .flex_col()
                .gap_4()
                .p_4()
                // 品牌区
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_3()
                        .px_2()
                        .py_2()
                        .child(
                            div()
                                .w(px(38.))
                                .h(px(38.))
                                .flex_none()
                                .child(icon_app_logo(38.)),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .child(
                                    div()
                                        .text_base()
                                        .font_weight(gpui::FontWeight::BOLD)
                                        .text_color(rgb(TEXT))
                                        .child(tr_app_title(lang)),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(rgb(OUTLINE))
                                        .child(tr_app_subtitle(lang)),
                                ),
                        ),
                )
                // 导航项列表
                .child(div().flex().flex_col().gap_1().children(items)),
        )
        // 底栏统计与语言切换
        .child(
            div()
                .p_4()
                .flex()
                .flex_col()
                .gap_3()
                .border_t_1()
                .border_color(rgba(OUTLINE_VAR, 0.4))
                .child(
                    div()
                        .px_3()
                        .py_3()
                        .rounded_xl()
                        .bg(rgb(CARD))
                        .border_1()
                        .border_color(rgba(OUTLINE_VAR, 0.4))
                        .flex()
                        .items_center()
                        .gap_3()
                        .child(
                            div()
                                .w(px(32.))
                                .h(px(32.))
                                .rounded_lg()
                                .bg(rgb(PRIMARY_FIXED))
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(icon_trash(PRIMARY, 16.)),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(rgb(MUTED))
                                        .child(tr_freed_total(lang)),
                                )
                                .child(
                                    div()
                                        .text_sm()
                                        .font_weight(gpui::FontWeight::BOLD)
                                        .text_color(rgb(PRIMARY))
                                        .child(crate::core::model::fmt_size(
                                            root.clean.freed_total,
                                        )),
                                ),
                        ),
                )
                // 语言切换 Pill
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .px_1()
                        .child(div().text_xs().text_color(rgb(MUTED)).child(
                            if lang == Language::Zh {
                                "界面语言"
                            } else {
                                "Language"
                            },
                        ))
                        .child(
                            div()
                                .id("lang-switch-container")
                                .flex()
                                .items_center()
                                .bg(rgb(CARD))
                                .border_1()
                                .border_color(rgba(OUTLINE_VAR, 0.5))
                                .rounded_lg()
                                .p(px(2.))
                                .gap(px(2.))
                                .child(
                                    div()
                                        .id("lang-zh-btn")
                                        .px_2()
                                        .py(px(2.))
                                        .rounded_md()
                                        .text_xs()
                                        .font_weight(if lang == Language::Zh {
                                            gpui::FontWeight::BOLD
                                        } else {
                                            gpui::FontWeight::NORMAL
                                        })
                                        .cursor_pointer()
                                        .when(lang == Language::Zh, |d| {
                                            d.bg(rgb(PRIMARY_FIXED)).text_color(rgb(PRIMARY))
                                        })
                                        .when(lang != Language::Zh, |d| {
                                            d.text_color(rgb(MUTED)).hover(|h| h.bg(rgb(SURF_HIGH)))
                                        })
                                        .child("中文")
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.set_language(Language::Zh, cx);
                                        })),
                                )
                                .child(
                                    div()
                                        .id("lang-en-btn")
                                        .px_2()
                                        .py(px(2.))
                                        .rounded_md()
                                        .text_xs()
                                        .font_weight(if lang == Language::En {
                                            gpui::FontWeight::BOLD
                                        } else {
                                            gpui::FontWeight::NORMAL
                                        })
                                        .cursor_pointer()
                                        .when(lang == Language::En, |d| {
                                            d.bg(rgb(PRIMARY_FIXED)).text_color(rgb(PRIMARY))
                                        })
                                        .when(lang != Language::En, |d| {
                                            d.text_color(rgb(MUTED)).hover(|h| h.bg(rgb(SURF_HIGH)))
                                        })
                                        .child("English")
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.set_language(Language::En, cx);
                                        })),
                                ),
                        ),
                ),
        )
}
