//! 软件管理页面的局部组件

use crate::core::apps::InstalledApp;
use crate::core::model::{fmt_size, truncate};
use crate::ui::app_icons::try_get_icon;
use crate::ui::components::button::{Button, IconButton, IconButtonVariant};
use crate::ui::components::list_row::{ListRow, RowInset};
use crate::ui::components::scroll_area::{table_shell, virtual_list, ScrollArea};
use crate::ui::theme::*;
use crate::ui::Root;
use gpui::{div, img, prelude::*, px, rgb, AnyElement, Context, Div, ImageSource, SharedString};

pub(super) fn render_app_row(
    root: &Root,
    app: &InstalledApp,
    idx: usize,
    cx: &mut Context<Root>,
) -> AnyElement {
    let can_uninstall = app.can_uninstall();
    let app_for_uninst = app.clone();
    let app_for_resid = app.clone();
    let app_for_menu_rc = app.clone();
    let app_for_menu_btn = app.clone();

    let initial = app
        .name
        .chars()
        .next()
        .unwrap_or('?')
        .to_uppercase()
        .to_string();

    // 优先显示应用真实图标（NSWorkspace 提取），缓存未命中时回退到首字母
    let icon_element = app
        .icon_cache_key()
        .as_deref()
        .and_then(try_get_icon)
        .map(|image| {
            let src = ImageSource::from(image);
            img(src)
                .id(SharedString::from(format!("app-icon-{idx}")))
                .w(px(APP_ICON_SIZE))
                .h(px(APP_ICON_SIZE))
                .flex_none()
                .into_any_element()
        })
        .unwrap_or_else(|| {
            div()
                .w(px(APP_ICON_SIZE))
                .h(px(APP_ICON_SIZE))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .text_xs()
                .font_weight(gpui::FontWeight::BOLD)
                .text_color(rgb(PRIMARY))
                .child(initial.clone())
                .into_any_element()
        });

    let lang = root.language;
    let is_busy = root.residual.scanning || root.clean.running;
    let uninst_enabled = can_uninstall && !is_busy;

    let name_col = div()
        .flex()
        .flex_col()
        .child(
            div()
                .text_sm()
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(rgb(TEXT))
                .whitespace_nowrap()
                .overflow_hidden()
                .child(truncate(&app.name, 38)),
        )
        .when(app.discovery.is_some(), |d| {
            d.child(
                div()
                    .text_xs()
                    .text_color(rgb(OUTLINE))
                    .whitespace_nowrap()
                    .overflow_hidden()
                    .child(crate::ui::i18n::tr_app_discovery_label(lang, can_uninstall)),
            )
        })
        .when(app.discovery.is_none() && !app.version.is_empty(), |d| {
            d.child(
                div()
                    .text_xs()
                    .text_color(rgb(OUTLINE))
                    .child(format!("v{}", truncate(&app.version, 24))),
            )
        });

    let columns = div()
        .flex_none()
        .flex()
        .items_center()
        .gap_3()
        .child(
            div()
                .w(px(130.))
                .flex_none()
                .text_xs()
                .text_color(rgb(MUTED))
                .child(if app.publisher.is_empty() {
                    "—".to_string()
                } else {
                    truncate(&app.publisher, 16)
                }),
        )
        .child(
            div()
                .w(px(110.))
                .flex_none()
                .text_xs()
                .text_color(if app.last_used_date.is_some() {
                    rgb(TEXT)
                } else {
                    rgb(OUTLINE)
                })
                .child(app.last_used_date.clone().unwrap_or_else(|| "—".into())),
        )
        .child(
            div()
                .w(px(100.))
                .flex_none()
                .text_xs()
                .text_color(rgb(OUTLINE))
                .child(app.install_date.clone().unwrap_or_else(|| "—".into())),
        )
        .child(
            div()
                .w(px(95.))
                .flex_none()
                .text_right()
                .text_xs()
                .font_weight(gpui::FontWeight::BOLD)
                .text_color(if app.estimated_size >= 1024 * 1024 * 1024 {
                    rgb(PRIMARY)
                } else if app.estimated_size > 0 {
                    rgb(TEXT)
                } else {
                    rgb(OUTLINE)
                })
                .child(if app.estimated_size > 0 {
                    fmt_size(app.estimated_size)
                } else {
                    "—".into()
                }),
        )
        .child(
            div()
                .w(px(APP_ACTIONS_COL_W))
                .flex_none()
                .flex()
                .items_center()
                .justify_end()
                .gap_2()
                .child(
                    Button::small(
                        SharedString::from(format!("uninst-{idx}")),
                        crate::ui::i18n::tr_btn_uninstall(lang),
                    )
                    .colors(CARD, TEXT)
                    .disabled(!uninst_enabled)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.request_uninstall_app(app_for_uninst.clone(), cx);
                    })),
                )
                // macOS 上卸载已经会扫 Library 残留；强力清理卸不掉 .app，单独入口没有意义。
                .when(!cfg!(target_os = "macos"), |d| {
                    d.child(
                        Button::small(
                            SharedString::from(format!("clean-resid-{idx}")),
                            if app.discovery.is_some() {
                                crate::ui::i18n::tr_btn_remove_program(lang)
                            } else {
                                crate::ui::i18n::tr_btn_force_clean(lang)
                            },
                        )
                        .colors(PRIMARY_FIXED, PRIMARY)
                        .disabled(is_busy)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.start_residual_scan(app_for_resid.clone(), cx);
                        })),
                    )
                })
                .child(
                    IconButton::new(
                        SharedString::from(format!("more-btn-{idx}")),
                        IconButtonVariant::Tile,
                    )
                    .child("···")
                    .on_click(cx.listener(
                        move |this, event: &gpui::ClickEvent, _, cx| {
                            let pos = event.position();
                            let x: f32 = pos.x.into();
                            let y: f32 = pos.y.into();
                            this.open_app_context_menu(app_for_menu_btn.clone(), x, y);
                            cx.notify();
                        },
                    )),
                ),
        );

    ListRow::new(SharedString::from(format!("app-row-{idx}")))
        .height(APP_ROW_H)
        .inset(RowInset::X5)
        .divider(0.3)
        .leading(icon_element)
        .content(name_col)
        .trailing(columns)
        .on_right_click(
            cx.listener(move |this, event: &gpui::MouseDownEvent, _, cx| {
                let x: f32 = event.position.x.into();
                let y: f32 = event.position.y.into();
                this.open_app_context_menu(app_for_menu_rc.clone(), x, y);
                cx.notify();
            }),
        )
        .into_any_element()
}

/// 行内应用图标边长。
const APP_ICON_SIZE: f32 = 32.0;

/// 软件表的行高。uniform_list 要求等高，因此行内名称强制单行。
pub(super) const APP_ROW_H: f32 = 54.0;

/// 操作列宽度。macOS 只有「卸载」+ 更多，Windows 还要放下「强力清理」。
pub(super) const APP_ACTIONS_COL_W: f32 = if cfg!(target_os = "macos") {
    120.0
} else {
    190.0
};

/// 列表主体：要么是占位提示，要么是虚拟化的行。
pub(super) enum ListBody {
    /// 加载中 / 空结果之类的整块占位
    Placeholder(AnyElement),
    /// 有多少行（内容按需渲染）
    Rows(usize),
}

pub(super) fn render_apps_list_card(
    root: &Root,
    table_header: Div,
    body: ListBody,
    list_footer: Div,
    cx: &mut Context<Root>,
) -> Div {
    // 右侧自绘滚动条。几何计算与「智能清理」的分类列表共用同一套度量，
    // 见 ui::components::scroll。
    let row_count = match &body {
        ListBody::Rows(n) => *n,
        ListBody::Placeholder(_) => 0,
    };
    let body_el: AnyElement = match body {
        ListBody::Placeholder(el) => el,
        // 148 款软件按旧写法是每帧构造上千个元素；uniform_list 只渲染视口内的行
        ListBody::Rows(n) => virtual_list(
            SharedString::from(format!("apps-list-rows-{}", root.apps.gen)),
            n,
            root.apps.scroll.clone(),
            cx.processor(|this, range: std::ops::Range<usize>, _window, cx| {
                let picked: Vec<crate::core::apps::InstalledApp> = range
                    .clone()
                    .filter_map(|i| this.apps.view.get(i).and_then(|&j| this.apps.list.get(j)))
                    .cloned()
                    .collect();
                picked
                    .into_iter()
                    .zip(range)
                    .map(|(app, idx)| render_app_row(this, &app, idx, cx))
                    .collect()
            }),
        )
        .size_full()
        .into_any_element(),
    };

    table_shell(
        table_header,
        ScrollArea::new(
            "apps_components-scroll",
            root.apps.scroll.0.borrow().base_handle.clone(),
            340.,
            row_count as f32 * APP_ROW_H,
            body_el,
        ),
        list_footer,
    )
    .min_h(px(340.))
}
