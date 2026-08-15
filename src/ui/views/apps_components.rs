//! 软件管理页面的局部组件

use crate::core::apps::InstalledApp;
use crate::core::model::{fmt_size, truncate};
use crate::ui::components::buttons::small_button;
use crate::ui::components::cards::card;
use crate::ui::theme::*;
use crate::ui::components::scroll::{drag_to_offset, scroll_metrics, scrollbar, SCROLLBAR_W};
use crate::ui::Root;
use gpui::{div, prelude::*, px, rgb, AnyElement, Context, Div, SharedString};

pub(super) fn render_app_row(
    root: &Root,
    app: &InstalledApp,
    idx: usize,
    cx: &mut Context<Root>,
) -> AnyElement {
    let has_uninstaller = app.uninstall_string.is_some() || app.quiet_uninstall_string.is_some();
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

    div()
        .id(SharedString::from(format!("app-row-{idx}")))
        .w_full()
        .h(px(APP_ROW_H))
        .flex()
        .items_center()
        .gap_3()
        .px_5()
        .border_b_1()
        .border_color(rgba(OUTLINE_VAR, 0.3))
        .hover(|h| h.bg(rgb(SURF_LOW)))
        .on_mouse_down(
            gpui::MouseButton::Right,
            cx.listener(move |this, event: &gpui::MouseDownEvent, _, cx| {
                let x: f32 = event.position.x.into();
                let y: f32 = event.position.y.into();
                this.open_app_context_menu(app_for_menu_rc.clone(), x, y);
                cx.notify();
            }),
        )
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .flex()
                .items_center()
                .gap_3()
                .child(
                    div()
                        .w(px(36.))
                        .h(px(36.))
                        .flex_none()
                        .rounded_xl()
                        .bg(rgb(SURF_HIGH))
                        .border_1()
                        .border_color(rgba(OUTLINE_VAR, 0.5))
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_sm()
                        .font_weight(gpui::FontWeight::BOLD)
                        .text_color(rgb(PRIMARY))
                        .child(initial),
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
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .text_color(rgb(TEXT))
                                .whitespace_nowrap()
                                .overflow_hidden()
                                .child(truncate(&app.name, 38)),
                        )
                        .when(!app.version.is_empty(), |d| {
                            d.child(
                                div()
                                    .text_xs()
                                    .text_color(rgb(OUTLINE))
                                    .child(format!("v{}", truncate(&app.version, 24))),
                            )
                        }),
                ),
        )
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
                .text_sm()
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
                .w(px(190.))
                .flex_none()
                .flex()
                .items_center()
                .justify_end()
                .gap_2()
                .child(
                    div()
                        .id(SharedString::from(format!("uninst-{idx}")))
                        .child(small_button(
                            String::from("卸载"),
                            SURF_HIGH,
                            TEXT,
                            has_uninstaller,
                        ))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.request_uninstall_app(app_for_uninst.clone(), cx);
                        })),
                )
                .child(
                    div()
                        .id(SharedString::from(format!("clean-resid-{idx}")))
                        .child(small_button(
                            String::from("强力清理"),
                            PRIMARY_FIXED,
                            PRIMARY,
                            !root.residual_scanning,
                        ))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.start_residual_scan(app_for_resid.clone(), cx);
                        })),
                )
                .child(
                    div()
                        .id(SharedString::from(format!("more-btn-{idx}")))
                        .w(px(28.))
                        .h(px(28.))
                        .rounded_md()
                        .bg(rgb(SURF_HIGH))
                        .hover(|h| h.bg(rgb(SURF_HIGHEST)))
                        .cursor_pointer()
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_xs()
                        .font_weight(gpui::FontWeight::BOLD)
                        .text_color(rgb(MUTED))
                        .child("···")
                        .on_click(cx.listener(move |this, event: &gpui::ClickEvent, _, cx| {
                            let pos = event.position();
                            let x: f32 = pos.x.into();
                            let y: f32 = pos.y.into();
                            this.open_app_context_menu(app_for_menu_btn.clone(), x, y);
                            cx.notify();
                        })),
                ),
        )
        .into_any_element()
}

/// 软件表的行高。uniform_list 要求等高，因此行内名称强制单行。
pub(super) const APP_ROW_H: f32 = 73.0;

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
    let base = root.apps_list_scroll.0.borrow().base_handle.clone();
    let metrics = scroll_metrics(&base, 340.0, row_count as f32 * APP_ROW_H);

    let scrollbar_el = metrics.map(|m| {
        scrollbar("apps-scroll-thumb", m, |thumb| {
            thumb.on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(|this, event: &gpui::MouseDownEvent, _, cx| {
                    let mouse_y: f32 = event.position.y.into();
                    let start_top: f32 =
                        (-this.apps_list_scroll.0.borrow().base_handle.offset().y).into();
                    this.apps_scroll_drag = Some((mouse_y, start_top.max(0.0)));
                    cx.notify();
                }),
            )
        })
    });

    let body_el: AnyElement = match body {
        ListBody::Placeholder(el) => el,
        // 148 款软件按旧写法是每帧构造上千个元素；uniform_list 只渲染视口内的行
        ListBody::Rows(n) => gpui::uniform_list(
            "apps-list-rows",
            n,
            cx.processor(|this, range: std::ops::Range<usize>, _window, cx| {
                let picked: Vec<crate::core::apps::InstalledApp> = range
                    .clone()
                    .filter_map(|i| this.apps_view.get(i).and_then(|&j| this.apps.get(j)))
                    .cloned()
                    .collect();
                picked
                    .into_iter()
                    .zip(range)
                    .map(|(app, idx)| render_app_row(this, &app, idx, cx))
                    .collect()
            }),
        )
        .track_scroll(root.apps_list_scroll.clone())
        .size_full()
        .when(metrics.is_some(), |l| l.pr(px(SCROLLBAR_W)))
        .into_any_element(),
    };

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
                .child(body_el)
                .children(scrollbar_el),
        )
        .child(list_footer)
        .on_mouse_move(cx.listener(|this, event: &gpui::MouseMoveEvent, _, cx| {
            let Some(start) = this.apps_scroll_drag else {
                return;
            };
            let mouse_y: f32 = event.position.y.into();
            let base = this.apps_list_scroll.0.borrow().base_handle.clone();
            if let Some(new_top) = drag_to_offset(&base, start, mouse_y) {
                base.set_offset(gpui::point(px(0.0), px(-new_top)));
                cx.notify();
            }
        }))
        .on_mouse_up(
            gpui::MouseButton::Left,
            cx.listener(|this, _, _, cx| {
                if this.apps_scroll_drag.take().is_some() {
                    cx.notify();
                }
            }),
        )
}
