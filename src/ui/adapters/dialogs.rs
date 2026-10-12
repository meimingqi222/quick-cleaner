//! 弹窗对话框（二次确认与残留深度清理审查弹窗）

use crate::core::apps::{InstalledApp, ResidualKind, ResidualOccupancy, ResidualScope};
use crate::core::i18n::Language;
use crate::core::model::{fmt_size, truncate, Check};
use crate::ui::components::badge::{Badge, BadgeTone};
use crate::ui::components::button::{Button, ButtonTone};
use crate::ui::components::controls::checkbox;
use crate::ui::components::dialog::DialogFrame;
use crate::ui::components::icons::*;
use crate::ui::components::list_row::{ListRow, RowInset, RowRadius};
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
    /// 冗余整理（相似照片/重复文件/大文件/下载）把选中的条目移入废纸篓。
    CleanDeclutter(crate::ui::views::declutter::DeclutterTab),
    UninstallApp(Box<InstalledApp>),
    /// 移除一个开发环境资产（conda 环境 / 全局包 / CLI 工具）。走生态自己的
    /// 卸载命令，不是文件删除。
    RemoveDevAsset(Box<crate::core::dev_env::DevAssetItem>),
    /// 批量移除一组开发环境资产。
    BatchRemoveDevAssets(Vec<crate::core::dev_env::DevAssetItem>),
    /// 结束一个进程（状态监控页）。`pid` + 进程名。
    KillProcess {
        pid: u32,
        start_time: u64,
        unique_id: Option<u64>,
        name: String,
    },
    /// 安装风扇特权守护进程，装完后施加这个档位（状态监控页）。
    /// 装系统组件是持久化改动，要在应用内讲清楚再走系统密码框。
    InstallFanHelper(crate::core::status::FanMode),
    /// 打断进行中的扫描/清理并安装应用更新。
    InstallUpdate,
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
    let is_kill = matches!(&req.kind, ConfirmKind::KillProcess { .. });
    let is_fan_install = matches!(&req.kind, ConfirmKind::InstallFanHelper(_));
    let is_update = matches!(&req.kind, ConfirmKind::InstallUpdate);
    let confirm_label = match &req.kind {
        ConfirmKind::UninstallApp(_) => tr_confirm_action_uninstall(lang),
        ConfirmKind::KillProcess { .. } => tr_confirm_kill_title(lang),
        ConfirmKind::InstallFanHelper(_) => tr_confirm_action_install(lang),
        ConfirmKind::InstallUpdate => tr_update_restart_install(lang),
        _ => tr_confirm_action_delete_permanently(lang),
    };

    let badge = if is_uninstall || is_fan_install || is_update {
        icon_badge(icon_apps(PRIMARY, 20.), PRIMARY_FIXED, PRIMARY, 40.)
    } else if is_kill {
        icon_badge(icon_shield(ERROR, 20.), ERROR_CONTAINER, ERROR, 40.)
    } else {
        icon_badge(icon_trash(ERROR, 20.), ERROR_CONTAINER, ERROR, 40.)
    };

    let detail_color = if is_uninstall || is_fan_install || is_update {
        OUTLINE
    } else {
        ERROR
    };

    let header = div().flex().items_center().gap_3().child(badge).child(
        div()
            .text_lg()
            .font_weight(gpui::FontWeight::BOLD)
            .text_color(rgb(TEXT))
            .child(req.title.clone()),
    );
    let footer = div()
        .flex()
        .justify_end()
        .gap_2()
        .pt_2()
        .child(
            Button::ghost("confirm-cancel", cancel_label).on_click(cx.listener(
                |this, _, _, cx| {
                    this.confirm = None;
                    cx.notify();
                },
            )),
        )
        .child(
            if is_fan_install || is_update {
                Button::primary("confirm-accept", confirm_label)
            } else {
                Button::danger("confirm-accept", confirm_label)
            }
            .on_click(cx.listener(|this, _, _, cx| {
                this.confirm_accept(cx);
            })),
        );

    DialogFrame::new("confirm-dialog")
        .priority(root.overlays.priority(crate::ui::shell::OverlayId::Confirm))
        .width(460.)
        .header(header)
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
        .footer(footer)
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
        evidence.push(tr_occupancy_more(lang, hidden));
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

    let has_sysext = res
        .items
        .iter()
        .any(|it| matches!(it.kind, ResidualKind::SystemExtension(..)));

    let done_label = tr_done_label(lang);
    // 空态文案也分两种口径：按应用扫出来的「没残留」说的是卸载干不干净，
    // 孤儿扫描说的则是「用户目录里没有找不到主人的东西」。
    let (empty_title, empty_desc) = match res.scope {
        ResidualScope::OrphanLeftovers => (tr_orphan_empty_title(lang), tr_orphan_empty_desc(lang)),
        ResidualScope::App => (
            tr_residual_app_empty_title(lang),
            tr_residual_app_empty_desc(lang),
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

                let row = ListRow::new(SharedString::from(format!("resid-item-{idx}")))
                    .inset(RowInset::X3Y2)
                    .radius(RowRadius::Lg)
                    .leading(
                        div()
                            .flex()
                            .items_center()
                            .gap_3()
                            .child(checkbox(check_state.into()))
                            // 置信度标签
                            .child(
                                Badge::new(item.confidence.label_lang(lang))
                                    .tone(if item.confidence.is_certain() {
                                        BadgeTone::Primary
                                    } else {
                                        BadgeTone::Caution
                                    })
                                    .padding(8., 1.)
                                    .no_shrink(),
                            )
                            // 来源标签
                            .child(
                                Badge::new(item.source.label_lang(lang))
                                    .padding(8., 1.)
                                    .weight(gpui::FontWeight::NORMAL)
                                    .no_shrink(),
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
                            ),
                    )
                    .content(
                        div()
                            .text_xs()
                            .text_color(rgb(TEXT))
                            // 来源已经在上面的徽章里显示过了，这里只要路径本身
                            .child(item.kind.display_label()),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if this.residual.selected.contains(&idx) {
                            this.residual.selected.remove(&idx);
                        } else {
                            this.residual.selected.insert(idx);
                        }
                        cx.notify();
                    }));
                let row = if item.size() > 0 {
                    row.trailing(
                        div()
                            .flex_none()
                            .text_xs()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(rgb(OUTLINE))
                            .child(fmt_size(item.size())),
                    )
                } else {
                    row
                };
                row.into_any_element()
            })
            .collect()
    };

    let clean_btn_text = tr_clean_selected_button(lang, selected_count, &fmt_size(selected_bytes));

    let footer = if is_empty {
        div()
            .flex()
            .items_center()
            .justify_end()
            .pt_2()
            .border_t_1()
            .border_color(rgba(OUTLINE_VAR, 0.4))
            .child(
                Button::primary("resid-done-btn", done_label).on_click(cx.listener(
                    |this, _, _, cx| {
                        this.residual.result = None;
                        this.residual.selected.clear();
                        cx.notify();
                    },
                )),
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
                        Button::small("resid-select-rec", tr_batch_rec(lang))
                            .tone(if is_recommended {
                                ButtonTone::Accent
                            } else {
                                ButtonTone::Neutral
                            })
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if let Some(r) = &this.residual.result {
                                    this.residual.selected = r.default_selection();
                                }
                                cx.notify();
                            })),
                    )
                    // 2. 全选所有
                    .child(
                        Button::small("resid-select-all", tr_batch_all(lang))
                            .tone(if all_selected {
                                ButtonTone::Accent
                            } else {
                                ButtonTone::Neutral
                            })
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if let Some(r) = &this.residual.result {
                                    this.residual.selected = (0..r.items.len()).collect();
                                }
                                cx.notify();
                            })),
                    )
                    // 3. 清空
                    .child(
                        Button::small("resid-select-none", tr_btn_clear_sel(lang))
                            .tone(ButtonTone::Muted)
                            .disabled(selected_count == 0)
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
                    .child(Button::ghost("resid-cancel", tr_btn_cancel(lang)).on_click(
                        cx.listener(|this, _, _, cx| {
                            this.residual.result = None;
                            this.residual.selected.clear();
                            cx.notify();
                        }),
                    ))
                    .child(
                        Button::danger("resid-clean", clean_btn_text)
                            .disabled(selected_count == 0)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.clean_selected_residuals(cx);
                            })),
                    ),
            )
    };

    // 标题两种口径：按应用扫出来的残留属于一个「主人」，孤儿扫描一次覆盖
    // 一堆已经不在的软件，用「发现「」的 N 项残留」会把空名字套进引号里。
    let (modal_title, modal_sub) = match res.scope {
        ResidualScope::App => (
            tr_residual_found_title(lang, &res.app_name, total_items),
            tr_residual_found_sub(lang, &fmt_size(res.total_file_size)),
        ),
        ResidualScope::OrphanLeftovers => (
            tr_orphan_modal_title(lang, total_items),
            tr_orphan_modal_sub(lang, &fmt_size(res.total_file_size)),
        ),
    };

    Some(
        DialogFrame::new("residual-modal-card")
            .priority(
                root.overlays
                    .priority(crate::ui::shell::OverlayId::Residual),
            )
            .width(760.)
            .max_height(580.)
            .gap(16.)
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
            // 系统扩展引导：程序注销不了，只能用户自己去设置里关。
            // 和占用条同一样式（提示不是错误），按钮直达登录项与扩展。
            .when(!is_empty && has_sysext, |d| {
                d.child(
                    div()
                        .flex_none()
                        .p_3()
                        .rounded_lg()
                        .bg(rgb(CAUTION_CONTAINER))
                        .flex()
                        .flex_col()
                        .gap_2()
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
                                        .child(tr_residual_sysext_title(lang)),
                                ),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(rgb(CAUTION))
                                .pl(px(22.))
                                .child(tr_residual_sysext_advice(lang)),
                        )
                        .child(
                            div().flex().justify_end().child(
                                Button::small(
                                    "resid-open-sysext-settings",
                                    tr_residual_sysext_open(lang),
                                )
                                .tone(ButtonTone::Accent)
                                .on_click(cx.listener(
                                    |this, _, _, cx| {
                                        this.open_system_extension_settings(cx);
                                    },
                                )),
                            ),
                        ),
                )
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
            .footer(footer),
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
        DialogFrame::new("fda-modal-card")
            .priority(root.overlays.priority(crate::ui::shell::OverlayId::Fda))
            .width(580.)
            .gap(16.)
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
                            .child(checkbox(check_state.into()))
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
                                Button::ghost("fda-later-btn", tr_fda_btn_later(lang)).on_click(
                                    cx.listener(|this, _, _, cx| {
                                        this.close_fda_guide(cx);
                                    }),
                                ),
                            )
                            .child(
                                Button::small("fda-check-btn", tr_fda_btn_check(lang)).on_click(
                                    cx.listener(|this, _, _, cx| {
                                        this.check_fda_permission(cx);
                                    }),
                                ),
                            )
                            .child(
                                Button::primary(
                                    "fda-open-settings-btn",
                                    tr_fda_btn_open_settings(lang),
                                )
                                .on_click(cx.listener(
                                    |this, _, _, cx| {
                                        this.open_fda_settings(cx);
                                    },
                                )),
                            ),
                    ),
            ),
    )
}
