//! 弹窗对话框（二次确认与残留深度清理审查弹窗）

use crate::core::model::{fmt_size, Check};
use crate::ui::components::buttons::{danger_button, ghost_button};
use crate::ui::components::cards::card;
use crate::ui::components::controls::checkbox;
use crate::ui::theme::*;
use crate::ui::Root;
use gpui::{div, prelude::*, px, rgb, Context, IntoElement, SharedString};
use std::path::PathBuf;

#[derive(Clone, Debug)]
pub enum ConfirmKind {
    CleanSelected,
    CleanPath(PathBuf, u64),
    CleanDiskSelected,
}

#[derive(Clone, Debug)]
pub struct ConfirmRequest {
    pub kind: ConfirmKind,
    pub title: String,
    pub body: String,
    pub detail: String,
}

pub fn render_confirm_dialog(_root: &Root, req: &ConfirmRequest, cx: &mut Context<Root>) -> impl IntoElement {
    div()
        .absolute()
        .inset_0()
        .occlude()
        .bg(rgba(0x000000, 0.35))
        .flex()
        .items_center()
        .justify_center()
        .child(
            card()
                .w(px(460.))
                .shadow_2xl()
                .p_6()
                .flex()
                .flex_col()
                .gap_3()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_3()
                        .child(
                            div()
                                .w(px(36.))
                                .h(px(36.))
                                .flex_none()
                                .rounded_full()
                                .bg(rgb(ERROR_CONTAINER))
                                .flex()
                                .items_center()
                                .justify_center()
                                .child("⚠"),
                        )
                        .child(
                            div()
                                .text_lg()
                                .font_weight(gpui::FontWeight::BOLD)
                                .text_color(rgb(TEXT))
                                .child(req.title.clone()),
                        ),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(rgb(TEXT))
                        .child(req.body.clone()),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(rgb(ERROR))
                        .child(req.detail.clone()),
                )
                .child(
                    div()
                        .flex()
                        .justify_end()
                        .gap_2()
                        .pt_2()
                        .child(
                            div()
                                .id("confirm-cancel")
                                .child(ghost_button(String::from("取消"), true))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.confirm = None;
                                    cx.notify();
                                })),
                        )
                        .child(
                            div()
                                .id("confirm-accept")
                                .child(danger_button(String::from("确认永久删除"), true))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.confirm_accept(cx);
                                })),
                        ),
                ),
        )
}

pub fn render_residual_modal(root: &Root, cx: &mut Context<Root>) -> Option<impl IntoElement> {
    let res = root.residual_result.as_ref()?;
    let total_items = res.items.len();
    let selected_count = root.residual_selected.len();
    let all_selected = selected_count == total_items && total_items > 0;

    let selected_bytes: u64 = root
        .residual_selected
        .iter()
        .filter_map(|&idx| res.items.get(idx))
        .map(|it| it.size())
        .sum();

    let item_rows: Vec<gpui::AnyElement> = if res.items.is_empty() {
        vec![div()
            .w_full()
            .p_8()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_2()
            .child(
                div()
                    .text_base()
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(rgb(PRIMARY))
                    .child("✨ 该软件非常干净，未发现关联的文件或注册表残留！"),
            )
            .into_any_element()]
    } else {
        res.items
            .iter()
            .enumerate()
            .map(|(idx, item)| {
                let is_checked = root.residual_selected.contains(&idx);
                let check_state = if is_checked {
                    Check::On
                } else {
                    Check::Off
                };

                div()
                    .id(SharedString::from(format!("resid-item-{idx}")))
                    .flex()
                    .items_center()
                    .gap_3()
                    .px_3()
                    .py_2()
                    .rounded_lg()
                    .cursor_pointer()
                    .hover(|h| h.bg(rgb(SURF_LOW)))
                    .child(checkbox(check_state))
                    // 把握程度直接标在行首：模糊匹配出来的默认不勾选，
                    // 用户需要一眼看出哪些是「确定」哪些只是「可能」
                    .child(
                        div()
                            .flex_none()
                            .px_2()
                            .py(px(1.))
                            .rounded_md()
                            .text_xs()
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .bg(rgb(if item.confidence.is_certain() {
                                PRIMARY_FIXED
                            } else {
                                CAUTION_CONTAINER
                            }))
                            .text_color(rgb(if item.confidence.is_certain() {
                                PRIMARY
                            } else {
                                CAUTION
                            }))
                            .child(item.confidence.label()),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .text_xs()
                            .text_color(rgb(TEXT))
                            .child(item.display_label()),
                    )
                    .when(item.size() > 0, |d| {
                        d.child(
                            div()
                                .flex_none()
                                .text_xs()
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .text_color(rgb(OUTLINE))
                                .child(fmt_size(item.size())),
                        )
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if this.residual_selected.contains(&idx) {
                            this.residual_selected.remove(&idx);
                        } else {
                            this.residual_selected.insert(idx);
                        }
                        cx.notify();
                    }))
                    .into_any_element()
            })
            .collect()
    };

    Some(
        div()
            .id("residual-modal-backdrop")
            .absolute()
            .inset_0()
            .occlude()
            .bg(rgba(0x000000, 0.45))
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .id("residual-modal-card")
                    .w(px(720.))
                    .max_h(px(580.))
                    .rounded_2xl()
                    .bg(rgb(CARD))
                    .border_1()
                    .border_color(rgba(OUTLINE_VAR, 0.6))
                    .shadow_xl()
                    .p_6()
                    .flex()
                    .flex_col()
                    .gap_4()
                    .child(
                        div()
                            .flex()
                            .items_start()
                            .gap_4()
                            .child(
                                div()
                                    .w(px(40.))
                                    .h(px(40.))
                                    .flex_none()
                                    .rounded_full()
                                    .bg(rgb(PRIMARY_FIXED))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .text_lg()
                                    .child("🔍"),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .flex()
                                    .flex_col()
                                    .child(
                                        div()
                                            .text_lg()
                                            .font_weight(gpui::FontWeight::BOLD)
                                            .text_color(rgb(TEXT))
                                            .child(format!(
                                                "发现「{}」的 {} 项关联残留",
                                                res.app_name, total_items
                                            )),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(rgb(MUTED))
                                            .child(format!(
                                                "包括应用缓存、用户配置数据及注册表孤儿项，预计释放 {}",
                                                fmt_size(res.total_file_size)
                                            )),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .id("resid-list-scroll")
                            .flex_1()
                            .max_h(px(320.))
                            .overflow_scroll()
                            .border_1()
                            .border_color(rgba(OUTLINE_VAR, 0.5))
                            .rounded_xl()
                            .p_2()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .children(item_rows),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .pt_2()
                            .border_t_1()
                            .border_color(rgba(OUTLINE_VAR, 0.4))
                            .child(
                                div()
                                    .id("resid-toggle-all")
                                    .child(ghost_button(
                                        if all_selected {
                                            String::from("取消全选")
                                        } else {
                                            String::from("全选所有")
                                        },
                                        total_items > 0,
                                    ))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        if all_selected {
                                            this.residual_selected.clear();
                                        } else {
                                            if let Some(r) = &this.residual_result {
                                                this.residual_selected =
                                                    (0..r.items.len()).collect();
                                            }
                                        }
                                        cx.notify();
                                    })),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_3()
                                    .child(
                                        div()
                                            .id("resid-cancel")
                                            .child(ghost_button(
                                                String::from("取消 / 关闭"),
                                                true,
                                            ))
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.residual_result = None;
                                                this.residual_selected.clear();
                                                cx.notify();
                                            })),
                                    )
                                    .child(
                                        div()
                                            .id("resid-clean")
                                            .child(danger_button(
                                                format!(
                                                    "彻底清除所选 ({}) · 释放 {}",
                                                    selected_count,
                                                    fmt_size(selected_bytes)
                                                ),
                                                selected_count > 0,
                                            ))
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.clean_selected_residuals(cx);
                                            })),
                                    ),
                            ),
                    ),
            )
            .into_any_element(),
    )
}
