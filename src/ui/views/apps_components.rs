//! 软件管理页面的局部组件

use crate::core::apps::InstalledApp;
use crate::core::model::{fmt_size, truncate};
use crate::ui::components::buttons::small_button;
use crate::ui::components::cards::card;
use crate::ui::theme::*;
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
        .flex()
        .items_center()
        .gap_3()
        .px_5()
        .py_3()
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

pub(super) fn render_apps_list_card(
    root: &Root,
    table_header: Div,
    rows: Vec<AnyElement>,
    list_footer: Div,
    display_count: usize,
    cx: &mut Context<Root>,
) -> Div {
    // 右侧自绘滚动条：GPUI 的 overflow_scroll 只处理滚动，不绘制滚动条。
    // 通过 ScrollHandle 读取当前偏移、视口高度和最大偏移，实时显示列表滚动位置。
    let scroll_handle = root.apps_list_scroll.clone();
    let viewport_h: f32 = scroll_handle.bounds().size.height.into();
    let max_scroll_y: f32 = scroll_handle.max_offset().height.into();
    let scroll_top: f32 = (-scroll_handle.offset().y).into();

    // 首帧布局前 ScrollHandle 还没有 bounds/max_offset，用行高做一次保守估算，
    // 保证内容明显溢出时滚动条立即可见；开始滚动后由真实布局值接管。
    let estimated_viewport_h = 340.0;
    let estimated_content_h = display_count as f32 * 73.0;
    let estimated_max_y = (estimated_content_h - estimated_viewport_h).max(0.0);
    let viewport_eff = if viewport_h > 0.0 {
        viewport_h
    } else {
        estimated_viewport_h
    };
    let max_eff = if max_scroll_y > 0.0 {
        max_scroll_y
    } else {
        estimated_max_y
    };
    let has_scroll = max_eff > 0.0;

    let scrollbar = if has_scroll {
        let track_h = viewport_eff;
        let content_h = viewport_eff + max_eff;
        let thumb_h = ((viewport_eff / content_h) * track_h).clamp(28.0, track_h);
        let thumb_top = (scroll_top.max(0.0) / max_eff) * (track_h - thumb_h);

        Some(
            div()
                .absolute()
                .top(px(0.))
                .right(px(0.))
                .bottom(px(0.))
                .w(px(12.))
                .bg(rgba(OUTLINE_VAR, 0.14))
                .child(
                    div()
                        .id("apps-scroll-thumb")
                        .absolute()
                        .right(px(2.))
                        .top(px(thumb_top.clamp(0.0, track_h - thumb_h)))
                        .w(px(8.))
                        .h(px(thumb_h))
                        .rounded_full()
                        .bg(rgb(OUTLINE))
                        .opacity(0.9)
                        .cursor_pointer()
                        .on_mouse_down(
                            gpui::MouseButton::Left,
                            cx.listener(|this, event: &gpui::MouseDownEvent, _, cx| {
                                let mouse_y: f32 = event.position.y.into();
                                let start_top: f32 = (-this.apps_list_scroll.offset().y).into();
                                this.apps_scroll_drag = Some((mouse_y, start_top.max(0.0)));
                                cx.notify();
                            }),
                        ),
                ),
        )
    } else {
        None
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
                .child(
                    div()
                        .id("apps-list-rows")
                        .size_full()
                        .overflow_scroll()
                        .scrollbar_width(px(12.))
                        .track_scroll(&root.apps_list_scroll)
                        .flex()
                        .flex_col()
                        .children(rows),
                )
                .children(scrollbar),
        )
        .child(list_footer)
        .on_mouse_move(cx.listener(|this, event: &gpui::MouseMoveEvent, _, cx| {
            let Some((start_mouse_y, start_scroll_top)) = this.apps_scroll_drag else {
                return;
            };
            let viewport_h: f32 = this.apps_list_scroll.bounds().size.height.into();
            let max_scroll_y: f32 = this.apps_list_scroll.max_offset().height.into();
            if viewport_h <= 0.0 || max_scroll_y <= 0.0 {
                return;
            }

            let content_h = viewport_h + max_scroll_y;
            let thumb_h = ((viewport_h / content_h) * viewport_h).clamp(28.0, viewport_h);
            let travel = (viewport_h - thumb_h).max(1.0);
            let mouse_y: f32 = event.position.y.into();
            let new_top = (start_scroll_top + (mouse_y - start_mouse_y) / travel * max_scroll_y)
                .clamp(0.0, max_scroll_y);
            this.apps_list_scroll
                .set_offset(gpui::point(px(0.0), px(-new_top)));
            cx.notify();
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
