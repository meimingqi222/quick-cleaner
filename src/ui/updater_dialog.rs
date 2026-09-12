//! 应用更新对话框

use crate::core::updater::UpdateStatus;
use crate::ui::components::buttons::{ghost_button, primary_button, small_button};
use crate::ui::components::cards::card;
use crate::ui::components::icons::icon_app_logo;
use crate::ui::i18n::*;
use crate::ui::theme::*;
use crate::ui::Root;
use gpui::{div, prelude::*, px, rgb, Context, IntoElement};

pub fn render_update_dialog(root: &Root, cx: &mut Context<Root>) -> impl IntoElement {
    let lang = root.language;
    let status = root.update.status.clone();

    let (title, body, primary_label, primary_enabled) = match &status {
        UpdateStatus::Available {
            latest_version,
            notes,
            release_url,
            ..
        } => {
            let note = if notes.trim().is_empty() {
                release_url.clone()
            } else {
                crate::core::model::truncate(notes.trim(), 240)
            };
            (
                format!("{} {latest_version}", tr_update_new_version(lang)),
                note,
                tr_update_download(lang).to_string(),
                true,
            )
        }
        UpdateStatus::Downloading { progress, .. } => {
            let live = root
                .update
                .live_progress
                .as_ref()
                .and_then(|p| p.lock().ok().map(|p| p.percent));
            let percent = live.unwrap_or(progress.percent);
            (
                tr_update_downloading(lang).to_string(),
                format!("{percent:.0}%"),
                tr_update_downloading(lang).to_string(),
                false,
            )
        }
        UpdateStatus::Verifying { .. } => (
            tr_update_verifying(lang).to_string(),
            String::new(),
            tr_update_verifying(lang).to_string(),
            false,
        ),
        UpdateStatus::Downloaded {
            latest_version, ..
        } => (
            format!("{} {latest_version}", tr_update_new_version(lang)),
            tr_update_ready_hint(lang).to_string(),
            tr_update_restart_install(lang).to_string(),
            true,
        ),
        UpdateStatus::Installing { .. } => (
            tr_update_installing(lang).to_string(),
            String::new(),
            tr_update_installing(lang).to_string(),
            false,
        ),
        UpdateStatus::Error { message, .. } => (
            tr_update_failed(lang).to_string(),
            message.clone(),
            tr_update_retry(lang).to_string(),
            true,
        ),
        _ => (
            tr_update_dialog_title(lang).to_string(),
            String::new(),
            tr_update_check_now(lang).to_string(),
            !root.update.checking,
        ),
    };

    let show_skip = matches!(status, UpdateStatus::Available { .. });
    let release_url = match &status {
        UpdateStatus::Available { release_url, .. }
        | UpdateStatus::Downloading { release_url, .. } => Some(release_url.clone()),
        UpdateStatus::Error { .. } => None,
        _ => None,
    };

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
                .w(px(420.))
                .p_5()
                .flex()
                .flex_col()
                .gap_4()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_3()
                        .child(icon_app_logo(36.))
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .child(
                                    div()
                                        .text_base()
                                        .font_weight(gpui::FontWeight::BOLD)
                                        .text_color(rgb(TEXT))
                                        .child(tr_update_dialog_title(lang)),
                                )
                                .child(
                                    div()
                                        .text_sm()
                                        .text_color(rgb(MUTED))
                                        .child(title),
                                ),
                        ),
                )
                .when(!body.is_empty(), |d| {
                    d.child(
                        div()
                            .text_sm()
                            .text_color(rgb(OUTLINE))
                            .max_h(px(120.))
                            .overflow_hidden()
                            .child(body),
                    )
                })
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap_2()
                        .child(
                            div().flex().items_center().gap_2().when(show_skip, |d| {
                                d.child(
                                    div()
                                        .id("update-skip")
                                        .cursor_pointer()
                                        .child(small_button(
                                            tr_update_skip_version(lang).to_string(),
                                            SURF_HIGH,
                                            TEXT,
                                            true,
                                        ))
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.skip_this_update_version(cx);
                                        })),
                                )
                            })
                            .when(release_url.is_some(), |d| {
                                d.child(
                                    div()
                                        .id("update-open-release")
                                        .cursor_pointer()
                                        .child(small_button(
                                            tr_update_open_release(lang).to_string(),
                                            SURF_HIGH,
                                            TEXT,
                                            true,
                                        ))
                                        .on_click(cx.listener(|this, _, _, _cx| {
                                            if let Some(url) = match &this.update.status {
                                                UpdateStatus::Available { release_url, .. }
                                                | UpdateStatus::Downloading {
                                                    release_url, ..
                                                } => Some(release_url.clone()),
                                                _ => None,
                                            } {
                                                this.open_update_release_page(url);
                                            }
                                        })),
                                )
                            }),
                        )
                        .child(
                            div().flex().items_center().gap_2().child(
                                div()
                                    .id("update-later")
                                    .cursor_pointer()
                                    .child(ghost_button(tr_update_later(lang).to_string(), true))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.dismiss_update_dialog(cx);
                                    })),
                            ),
                        ),
                )
                .child(
                    div()
                        .id("update-primary")
                        .cursor_pointer()
                        .child(primary_button(primary_label, primary_enabled))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            match &this.update.status {
                                UpdateStatus::Available { .. } => this.start_update_download(cx),
                                UpdateStatus::Downloaded { .. } => this.install_update_now(cx),
                                UpdateStatus::Error { .. } => this.retry_update(cx),
                                UpdateStatus::Idle { .. }
                                | UpdateStatus::Checking { .. }
                                | UpdateStatus::NotAvailable { .. } => {
                                    this.check_for_updates_manual(cx)
                                }
                                _ => {}
                            }
                        })),
                ),
        )
}
