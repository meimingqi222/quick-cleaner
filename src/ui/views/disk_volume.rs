//! 磁盘透镜卷选择器

use crate::core::disk::VolumeId;
use crate::core::i18n::Language;
use crate::core::model::fmt_size;
use crate::ui::components::icons::*;
use crate::ui::components::list_row::{ListRow, RowHover, RowInset, RowVariant};
use crate::ui::components::menu::{Menu, MenuBorder};
use crate::ui::components::overlay::OverlayPolicy;
use crate::ui::i18n::*;
use crate::ui::theme::*;
use crate::ui::Root;
use gpui::{
    div, point, prelude::*, px, rgb, AnyElement, Bounds, Context, IntoElement, Pixels, SharedString,
};
use std::cell::Cell;

pub(super) fn format_volume_label(vol: &VolumeId, lang: Language) -> String {
    let raw = vol.display();
    #[cfg(windows)]
    {
        tr_volume_drive_label(lang, raw)
    }
    #[cfg(not(windows))]
    {
        if raw == "/" {
            tr_volume_system_root(lang).to_string()
        } else if let Some(stripped) = raw.strip_prefix("/Volumes/") {
            stripped.to_string()
        } else {
            raw.to_string()
        }
    }
}

/// 左侧卡片顶部的单行紧凑磁盘下拉切换按钮
pub(super) fn render_volume_selector_button(
    root: &Root,
    cx: &mut Context<Root>,
) -> impl IntoElement {
    let lang = root.language;
    let drive_label = format_volume_label(&root.disk.volume, lang);
    let short = if drive_label.chars().count() > 14 {
        let s: String = drive_label.chars().take(14).collect();
        format!("{s}…")
    } else {
        drive_label
    };
    let is_open = root.disk.volume_menu_open;
    let multi_vols = root.disk.volumes.len() > 1;

    div()
        .id("disk-volume-selector-btn")
        .flex()
        .items_center()
        .gap_1p5()
        .px_2p5()
        .py(px(3.))
        .rounded_lg()
        .border_1()
        .cursor_pointer()
        .when(is_open, |d| {
            d.bg(rgb(PRIMARY_FIXED))
                .border_color(rgb(PRIMARY))
                .text_color(rgb(PRIMARY))
        })
        .when(!is_open, |d| {
            d.bg(rgb(CARD))
                .border_color(rgba(OUTLINE_VAR, 0.7))
                .text_color(rgb(TEXT))
                .hover(|h| h.bg(rgb(SURF_HIGH)).border_color(rgb(PRIMARY)))
        })
        .relative()
        .child(
            gpui::canvas(
                |_, _, _| (),
                |bounds, _, _, _| TRIGGER_BOUNDS.with(|c| c.set(Some(bounds))),
            )
            .absolute()
            .size_full(),
        )
        .child(icon_disk(if is_open { PRIMARY } else { TEXT }, 13.))
        .child(
            div()
                .text_xs()
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .child(short),
        )
        .when(multi_vols, |d| {
            d.child(icon_chevron_down(
                if is_open { PRIMARY } else { OUTLINE },
                11.,
            ))
        })
        .on_click(cx.listener(|this, _, _, cx| {
            this.toggle_disk_volume_menu();
            cx.notify();
        }))
}

/// 磁盘切换下拉浮层菜单
pub fn render_disk_volume_dropdown(root: &Root, cx: &mut Context<Root>) -> Option<AnyElement> {
    if !root.disk.volume_menu_open || root.view != crate::ui::shell::View::Disk {
        return None;
    }
    let lang = root.language;
    let volumes = &root.disk.volumes;

    let title = tr_volume_picker_title(lang);
    let count_hint = tr_volume_available_count(lang, volumes.len());

    let items: Vec<AnyElement> = volumes
        .iter()
        .map(|v| {
            let active = root.disk.volume == *v;
            let vol_label = format_volume_label(v, lang);
            let space_info = root.disk.volume_space(v);
            let space_str = if let Some((tot, fre)) = space_info {
                let used = tot.saturating_sub(fre);
                tr_volume_used_of_total(lang, &fmt_size(used), &fmt_size(tot))
            } else {
                String::new()
            };

            let item_id = SharedString::from(format!("dropdown-vol-item-{}", v.display()));
            let v_clone = v.clone();

            let row = ListRow::new(item_id)
                .variant(RowVariant::Soft)
                .hover(RowHover::High)
                .inset(RowInset::X3Y2)
                .selected(active)
                .content(
                    div()
                        .flex()
                        .items_center()
                        .gap_2p5()
                        .min_w(px(0.))
                        .child(icon_badge(
                            icon_disk(if active { PRIMARY } else { TEXT }, 14.),
                            if active { PRIMARY_FIXED } else { SURF_LOW },
                            if active { PRIMARY } else { OUTLINE_VAR },
                            28.,
                        ))
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap(px(1.))
                                .min_w(px(0.))
                                .child(
                                    div()
                                        .text_xs()
                                        .font_weight(if active {
                                            gpui::FontWeight::BOLD
                                        } else {
                                            gpui::FontWeight::MEDIUM
                                        })
                                        .text_color(if active { rgb(PRIMARY) } else { rgb(TEXT) })
                                        .child(vol_label),
                                )
                                .when(!space_str.is_empty(), |d| {
                                    d.child(
                                        div().text_xs().text_color(rgb(OUTLINE)).child(space_str),
                                    )
                                }),
                        ),
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.switch_disk_volume(v_clone.clone(), cx);
                    cx.notify();
                }));
            let row = if active {
                row.trailing(
                    div()
                        .text_xs()
                        .font_weight(gpui::FontWeight::BOLD)
                        .text_color(rgb(PRIMARY))
                        .child("✓"),
                )
            } else {
                row
            };
            row.into_any_element()
        })
        .collect();

    // 行数多时只滚动列表，头部固定。
    let list = div()
        .id("disk-volume-dropdown-list")
        .max_h(px(LIST_MAX_H))
        .overflow_scroll()
        .flex()
        .flex_col()
        .gap_1()
        .children(items);

    let header = div()
        .flex()
        .items_center()
        .justify_between()
        .child(
            div()
                .text_xs()
                .font_weight(gpui::FontWeight::BOLD)
                .text_color(rgb(MUTED))
                .child(title),
        )
        .child(div().text_xs().text_color(rgb(OUTLINE)).child(count_hint));

    let trigger = trigger_bounds();
    let anchor = match trigger {
        Some(b) => point(b.origin.x, b.origin.y + b.size.height + px(TRIGGER_GAP)),
        None => point(px(FALLBACK_LEFT), px(FALLBACK_TOP)),
    };
    let estimated_h = (HEADER_H + volumes.len() as f32 * ROW_H).min(LIST_MAX_H + HEADER_H);

    let view = cx.entity();
    let menu = Menu::new("disk-volume-dropdown-card", anchor)
        .width(320.)
        .estimated_height(estimated_h)
        .border(MenuBorder::Soft)
        .header(header)
        // 触发钮自己的 click 负责 toggle；排除它，避免先被外部点击关掉又被 toggle 打开。
        .exclude_opt(trigger)
        .on_dismiss(move |_, app| {
            view.update(app, |this, cx| {
                this.dismiss_overlay_outside(crate::ui::shell::OverlayId::DiskVolume, cx);
                cx.notify();
            })
        })
        .child(list);

    Some(
        OverlayPolicy::POPUP.layer_at(
            menu,
            root.overlays
                .priority(crate::ui::shell::OverlayId::DiskVolume),
        ),
    )
}

thread_local! {
    /// 上一帧触发钮的窗口坐标；界面只在主线程绘制，所以用线程局部量即可。
    static TRIGGER_BOUNDS: Cell<Option<Bounds<Pixels>>> = const { Cell::new(None) };
}

fn trigger_bounds() -> Option<Bounds<Pixels>> {
    TRIGGER_BOUNDS.with(Cell::get)
}

/// 下拉锚点：触发钮左下角再留一点间隙。
const TRIGGER_GAP: f32 = 4.;
/// 触发钮还没布局过时的回退位置。
const FALLBACK_LEFT: f32 = 280.;
const FALLBACK_TOP: f32 = 168.;
const LIST_MAX_H: f32 = 320.;
const HEADER_H: f32 = 40.;
const ROW_H: f32 = 48.;
