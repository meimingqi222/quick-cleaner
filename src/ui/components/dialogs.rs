//! 弹窗对话框（二次确认与残留深度清理审查弹窗）

use crate::core::apps::{InstalledApp, ResidualOccupancy};
use crate::core::i18n::Language;
use crate::core::model::{fmt_size, truncate, Check};
use crate::ui::components::buttons::{danger_button, ghost_button, primary_button, small_button};
use crate::ui::components::cards::card;
use crate::ui::components::controls::checkbox;
use crate::ui::components::icons::*;
use crate::ui::i18n::*;
use crate::ui::theme::*;
use crate::ui::Root;
use gpui::{div, prelude::*, px, rgb, Context, IntoElement, SharedString};
use std::path::PathBuf;

#[derive(Clone, Debug)]
pub enum ConfirmKind {
    CleanSelected,
    CleanPath(PathBuf, u64),
    CleanDiskSelected,
    UninstallApp(Box<InstalledApp>),
}

#[derive(Clone, Debug)]
pub struct ConfirmRequest {
    pub kind: ConfirmKind,
    pub title: String,
    pub body: String,
    pub detail: String,
    /// 目标触及 `~/Library/Application Support` 下的应用数据。这里装的是
    /// 聊天记录、密码库这类不可重建的东西，确认弹窗要升级成醒目的
    /// Danger 警示（见 `render_confirm_dialog` 里的追加块）。
    pub app_data: bool,
}

pub fn render_confirm_dialog(
    root: &Root,
    req: &ConfirmRequest,
    cx: &mut Context<Root>,
) -> impl IntoElement {
    let lang = root.language;
    let cancel_label = tr_btn_cancel(lang);
    let is_uninstall = matches!(&req.kind, ConfirmKind::UninstallApp(_));
    let confirm_label = match &req.kind {
        ConfirmKind::UninstallApp(_) => match lang {
            Language::Zh => "确认卸载",
            Language::En => "Uninstall",
        },
        _ => match lang {
            Language::Zh => "确认永久删除",
            Language::En => "Delete Permanently",
        },
    };

    let badge = if is_uninstall {
        icon_badge(icon_apps(PRIMARY, 20.), PRIMARY_FIXED, PRIMARY, 40.)
    } else {
        icon_badge(icon_trash(ERROR, 20.), ERROR_CONTAINER, ERROR, 40.)
    };

    let detail_color = if is_uninstall { OUTLINE } else { ERROR };

    div()
        .absolute()
        .inset_0()
        .occlude()
        .bg(rgba(0x000000, 0.45))
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
                    div().flex().items_center().gap_3().child(badge).child(
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
                        .text_color(rgb(detail_color))
                        .child(req.detail.clone()),
                )
                .when(req.app_data, |d| {
                    d.child(
                        div()
                            .rounded_md()
                            .bg(rgb(ERROR_CONTAINER))
                            .border_1()
                            .border_color(rgb(ERROR))
                            .px_3()
                            .py_2()
                            .text_xs()
                            .text_color(rgb(ERROR))
                            .child(tr_confirm_app_data_warning(lang)),
                    )
                })
                .child(
                    div()
                        .flex()
                        .justify_end()
                        .gap_2()
                        .pt_2()
                        .child(
                            div()
                                .id("confirm-cancel")
                                .child(ghost_button(cancel_label.to_string(), true))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.confirm = None;
                                    cx.notify();
                                })),
                        )
                        .child(
                            div()
                                .id("confirm-accept")
                                .child(danger_button(confirm_label.to_string(), true))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.confirm_accept(cx);
                                })),
                        ),
                ),
        )
}

/// 残留弹窗顶部的占用警示条。扫描时发现该应用的进程/launchd 任务仍在，
/// 就在用户点「彻底清除」之前把因果讲清楚：数据库类残留删除必然失败，
/// 出路取决于证据强度（见下面的分支）。样式用「应用级占用」的橙色（与
/// junk 列表的 busy 徽标同一语义），不用红色——这是提示不是错误。
fn render_occupancy_banner(occ: &ResidualOccupancy, lang: Language) -> gpui::AnyElement {
    // 两种证据强度，标题和补救指引必须成对换：有活进程是「删除会失败」，
    // 出路是退出应用；只有未禁用的 launchd 登记是「清完会回来」，此刻
    // 并不拦截删除，而退出应用也解决不了——得从登录项里摘掉。
    let (title, advice) = if occ.processes.is_empty() {
        (
            tr_residual_registered_title(lang),
            tr_residual_registered_advice(lang),
        )
    } else {
        (
            tr_residual_occupied_title(lang),
            tr_residual_occupied_advice(lang),
        )
    };
    let mut evidence: Vec<String> = occ
        .processes
        .iter()
        .take(3)
        .map(|p| format!("· {}", truncate(p, 78)))
        .collect();
    if !occ.launchd_labels.is_empty() {
        let sep = if lang == Language::Zh { "、" } else { ", " };
        let labels = occ
            .launchd_labels
            .iter()
            .take(3)
            .cloned()
            .collect::<Vec<_>>()
            .join(sep);
        evidence.push(format!("· {}：{}", tr_residual_launchd_group(lang), labels));
    }
    let hidden = occ.processes.len().saturating_sub(3) + occ.launchd_labels.len().saturating_sub(3);
    if hidden > 0 {
        evidence.push(match lang {
            Language::Zh => format!("…以及另外 {hidden} 条"),
            Language::En => format!("…and {hidden} more"),
        });
    }
    // 补救指引和证据行同款样式、同一列缩进，就跟在证据后面一起渲染。
    evidence.push(advice.to_string());

    div()
        .flex_none()
        .p_3()
        .rounded_lg()
        .bg(rgb(CAUTION_CONTAINER))
        .flex()
        .flex_col()
        .gap_1()
        .child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(icon_sparkle(CAUTION, 14.))
                .child(
                    div()
                        .text_sm()
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(rgb(CAUTION))
                        .child(title),
                ),
        )
        .children(evidence.into_iter().map(|line| {
            div()
                .text_xs()
                .text_color(rgb(CAUTION))
                .pl(px(22.))
                .child(line)
        }))
        .into_any_element()
}

pub fn render_residual_modal(root: &Root, cx: &mut Context<Root>) -> Option<impl IntoElement> {
    let lang = root.language;
    let res = root.residual.result.as_ref()?;
    let total_items = res.items.len();
    let is_empty = total_items == 0;

    let selected_count = root.residual.selected.len();
    let all_selected = selected_count == total_items && total_items > 0;
    let rec_selection = res.default_selection();
    let is_recommended = root.residual.selected == rec_selection;

    let selected_bytes: u64 = root
        .residual
        .selected
        .iter()
        .filter_map(|&idx| res.items.get(idx))
        .map(|it| it.size())
        .sum();

    let (empty_title, empty_desc, done_label) = match lang {
        Language::Zh => (
            "该软件未发现关联的文件或注册表残留",
            "官方卸载已彻底清理所有文件与配置注册表。",
            "完成",
        ),
        Language::En => (
            "No residual files or registry traces found",
            "Uninstallation has cleanly removed all associated files and registry configurations.",
            "Done",
        ),
    };

    let item_rows: Vec<gpui::AnyElement> = if is_empty {
        vec![div()
            .w_full()
            .p_8()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_2()
            .child(icon_badge(
                icon_shield(PRIMARY, 24.),
                PRIMARY_FIXED,
                PRIMARY,
                52.,
            ))
            .child(
                div()
                    .text_base()
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(rgb(TEXT))
                    .child(empty_title),
            )
            .child(div().text_xs().text_color(rgb(OUTLINE)).child(empty_desc))
            .into_any_element()]
    } else {
        res.items
            .iter()
            .enumerate()
            .map(|(idx, item)| {
                let is_checked = root.residual.selected.contains(&idx);
                let check_state = if is_checked { Check::On } else { Check::Off };

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
                    // 置信度标签
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
                            .child(item.confidence.label_lang(lang)),
                    )
                    // 来源标签
                    .child(
                        div()
                            .flex_none()
                            .px_2()
                            .py(px(1.))
                            .rounded_md()
                            .text_xs()
                            .font_weight(gpui::FontWeight::NORMAL)
                            .bg(rgb(SURF_HIGH))
                            .text_color(rgb(MUTED))
                            .child(item.source.label_lang(lang)),
                    )
                    // 类别标签 (文件/目录/注册表项/注册表值)
                    .child(
                        div()
                            .flex_none()
                            .px_1()
                            .text_xs()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(rgb(OUTLINE))
                            .child(item.kind.kind_label_lang(lang)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .text_xs()
                            .text_color(rgb(TEXT))
                            // 来源已经在上面的徽章里显示过了，这里只要路径本身
                            .child(item.kind.display_label()),
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
                        if this.residual.selected.contains(&idx) {
                            this.residual.selected.remove(&idx);
                        } else {
                            this.residual.selected.insert(idx);
                        }
                        cx.notify();
                    }))
                    .into_any_element()
            })
            .collect()
    };

    let clean_btn_text = match lang {
        Language::Zh => format!(
            "彻底清除所选 ({}) · 释放 {}",
            selected_count,
            fmt_size(selected_bytes)
        ),
        Language::En => format!(
            "Clean Selected ({}) · Free {}",
            selected_count,
            fmt_size(selected_bytes)
        ),
    };

    let footer = if is_empty {
        div()
            .flex()
            .items_center()
            .justify_end()
            .pt_2()
            .border_t_1()
            .border_color(rgba(OUTLINE_VAR, 0.4))
            .child(
                div()
                    .id("resid-done-btn")
                    .child(primary_button(done_label.to_string(), true))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.residual.result = None;
                        this.residual.selected.clear();
                        cx.notify();
                    })),
            )
    } else {
        div()
            .flex()
            .items_center()
            .justify_between()
            .pt_2()
            .border_t_1()
            .border_color(rgba(OUTLINE_VAR, 0.4))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    // 1. 推荐选中
                    .child(
                        div()
                            .id("resid-select-rec")
                            .child(small_button(
                                tr_batch_rec(lang).to_string(),
                                if is_recommended {
                                    PRIMARY_FIXED
                                } else {
                                    SURF_HIGH
                                },
                                if is_recommended { PRIMARY } else { TEXT },
                                true,
                            ))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if let Some(r) = &this.residual.result {
                                    this.residual.selected = r.default_selection();
                                }
                                cx.notify();
                            })),
                    )
                    // 2. 全选所有
                    .child(
                        div()
                            .id("resid-select-all")
                            .child(small_button(
                                tr_batch_all(lang).to_string(),
                                if all_selected {
                                    PRIMARY_FIXED
                                } else {
                                    SURF_HIGH
                                },
                                if all_selected { PRIMARY } else { TEXT },
                                true,
                            ))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if let Some(r) = &this.residual.result {
                                    this.residual.selected = (0..r.items.len()).collect();
                                }
                                cx.notify();
                            })),
                    )
                    // 3. 清空
                    .child(
                        div()
                            .id("resid-select-none")
                            .child(small_button(
                                tr_btn_clear_sel(lang).to_string(),
                                SURF_HIGH,
                                MUTED,
                                selected_count > 0,
                            ))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.residual.selected.clear();
                                cx.notify();
                            })),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(
                        div()
                            .id("resid-cancel")
                            .child(ghost_button(tr_btn_cancel(lang).to_string(), true))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.residual.result = None;
                                this.residual.selected.clear();
                                cx.notify();
                            })),
                    )
                    .child(
                        div()
                            .id("resid-clean")
                            .child(danger_button(clean_btn_text, selected_count > 0))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.clean_selected_residuals(cx);
                            })),
                    ),
            )
    };

    let modal_title = match lang {
        Language::Zh => format!("发现「{}」的 {} 项关联残留", res.app_name, total_items),
        Language::En => format!(
            "Found {} residual items for \"{}\"",
            total_items, res.app_name
        ),
    };
    let modal_sub = match lang {
        Language::Zh => format!(
            "包括应用缓存、用户配置数据及注册表孤儿项，预计释放 {}",
            fmt_size(res.total_file_size)
        ),
        Language::En => format!(
            "Includes caches, app configuration and registry traces. Potential space: {}",
            fmt_size(res.total_file_size)
        ),
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
                card()
                    .id("residual-modal-card")
                    .w(px(760.))
                    .max_h(px(580.))
                    .shadow_2xl()
                    .p_6()
                    .flex()
                    .flex_col()
                    .gap_4()
                    .child(
                        div()
                            .flex()
                            .items_start()
                            .gap_4()
                            .child(icon_badge(
                                icon_search(PRIMARY, 20.),
                                PRIMARY_FIXED,
                                PRIMARY,
                                44.,
                            ))
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
                                            .child(modal_title),
                                    )
                                    .child(div().text_xs().text_color(rgb(MUTED)).child(modal_sub)),
                            ),
                    )
                    // 占用警示：必须在「彻底清除」按钮之前出现，否则用户
                    // 只会在失败日志里见到活数据库闸门的拒绝原因。
                    // 没扫出残留时不显示——「无残留」和「会删除失败」
                    // 同屏是自相矛盾的。
                    .when(!is_empty && res.occupancy.is_occupied(), |d| {
                        d.child(render_occupancy_banner(&res.occupancy, lang))
                    })
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
                    .child(footer),
            )
            .into_any_element(),
    )
}

/// macOS 完全磁盘访问权限（Full Disk Access）引导弹窗
pub fn render_fda_onboarding_modal(
    root: &Root,
    cx: &mut Context<Root>,
) -> Option<impl IntoElement> {
    if !root.show_fda_onboarding {
        return None;
    }

    let lang = root.language;
    let is_dismissed = root.settings.macos_fda_dismissed;
    let check_state = if is_dismissed { Check::On } else { Check::Off };

    let step_item = |num: &str, title: &str, desc: &str| {
        div()
            .flex()
            .items_start()
            .gap_3()
            .p_2()
            .rounded_lg()
            .bg(rgb(SURF_LOW))
            .child(
                div()
                    .w(px(22.))
                    .h(px(22.))
                    .flex_none()
                    .rounded_full()
                    .bg(rgb(PRIMARY_FIXED))
                    .text_xs()
                    .font_weight(gpui::FontWeight::BOLD)
                    .text_color(rgb(PRIMARY))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(num.to_string()),
            )
            .child(
                div()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .gap(px(1.))
                    .child(
                        div()
                            .text_xs()
                            .font_weight(gpui::FontWeight::BOLD)
                            .text_color(rgb(TEXT))
                            .child(title.to_string()),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(MUTED))
                            .child(desc.to_string()),
                    ),
            )
    };

    Some(
        div()
            .id("fda-modal-backdrop")
            .absolute()
            .inset_0()
            .occlude()
            .bg(rgba(0x000000, 0.45))
            .flex()
            .items_center()
            .justify_center()
            .child(
                card()
                    .id("fda-modal-card")
                    .w(px(580.))
                    .shadow_2xl()
                    .p_6()
                    .flex()
                    .flex_col()
                    .gap_4()
                    // 头部
                    .child(
                        div()
                            .flex()
                            .items_start()
                            .gap_4()
                            .child(icon_badge(
                                icon_shield(PRIMARY, 22.),
                                PRIMARY_FIXED,
                                PRIMARY,
                                48.,
                            ))
                            .child(
                                div()
                                    .flex_1()
                                    .flex()
                                    .flex_col()
                                    .gap(px(2.))
                                    .child(
                                        div()
                                            .text_lg()
                                            .font_weight(gpui::FontWeight::BOLD)
                                            .text_color(rgb(TEXT))
                                            .child(tr_fda_title(lang)),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(rgb(MUTED))
                                            .child(tr_fda_desc(lang)),
                                    ),
                            ),
                    )
                    // 步骤列表
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .child(step_item(
                                "1",
                                tr_fda_step1_title(lang),
                                tr_fda_step1_desc(lang),
                            ))
                            .child(step_item(
                                "2",
                                tr_fda_step2_title(lang),
                                tr_fda_step2_desc(lang),
                            ))
                            .child(step_item(
                                "3",
                                tr_fda_step3_title(lang),
                                tr_fda_step3_desc(lang),
                            )),
                    )
                    // 贴心提示
                    .child(
                        div()
                            .px_3()
                            .py_2()
                            .rounded_lg()
                            .bg(rgb(SURF_HIGH))
                            .text_xs()
                            .text_color(rgb(OUTLINE))
                            .child(tr_fda_notice(lang)),
                    )
                    // 底部操作区
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .pt_2()
                            .border_t_1()
                            .border_color(rgba(OUTLINE_VAR, 0.4))
                            // 不再自动提示复选框
                            .child(
                                div()
                                    .id("fda-dont-ask-toggle")
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .cursor_pointer()
                                    .child(checkbox(check_state))
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(rgb(MUTED))
                                            .child(tr_fda_dont_ask(lang)),
                                    )
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.toggle_fda_dismissed(cx);
                                    })),
                            )
                            // 按钮组
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .child(
                                        div()
                                            .id("fda-later-btn")
                                            .child(ghost_button(
                                                tr_fda_btn_later(lang).to_string(),
                                                true,
                                            ))
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.close_fda_guide(cx);
                                            })),
                                    )
                                    .child(
                                        div()
                                            .id("fda-check-btn")
                                            .child(small_button(
                                                tr_fda_btn_check(lang).to_string(),
                                                SURF_HIGH,
                                                TEXT,
                                                true,
                                            ))
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.check_fda_permission(cx);
                                            })),
                                    )
                                    .child(
                                        div()
                                            .id("fda-open-settings-btn")
                                            .child(primary_button(
                                                tr_fda_btn_open_settings(lang).to_string(),
                                                true,
                                            ))
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.open_fda_settings(cx);
                                            })),
                                    ),
                            ),
                    ),
            )
            .into_any_element(),
    )
}
