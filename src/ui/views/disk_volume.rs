//! 磁盘透镜卷选择器

use crate::core::disk::VolumeId;
use crate::core::i18n::Language;
use crate::core::model::fmt_size;
use crate::platform::get_volume_space;
use crate::ui::components::icons::*;
use crate::ui::theme::*;
use crate::ui::Root;
use gpui::{div, prelude::*, px, rgb, AnyElement, Context, IntoElement, SharedString};

pub(super) fn format_volume_label(vol: &VolumeId, lang: Language) -> String {
    let raw = vol.display();
    #[cfg(windows)]
    {
        match lang {
            Language::Zh => format!("{raw} 盘"),
            Language::En => format!("Drive {raw}"),
        }
    }
    #[cfg(not(windows))]
    {
        if raw == "/" {
            match lang {
                Language::Zh => "系统盘 (/)".to_string(),
                Language::En => "System (/)".to_string(),
            }
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
    if !root.disk.volume_menu_open || root.view != crate::ui::components::sidebar::View::Disk {
        return None;
    }
    let lang = root.language;
    let volumes = &root.disk.volumes;

    let title = match lang {
        Language::Zh => "选择要分析的磁盘",
        Language::En => "Select Drive to Analyze",
    };
    let count_hint = format!(
        "{} {}",
        volumes.len(),
        match lang {
            Language::Zh => "个可用磁盘",
            Language::En => "available",
        }
    );

    let items: Vec<AnyElement> = volumes
        .iter()
        .map(|v| {
            let active = root.disk.volume == *v;
            let vol_label = format_volume_label(v, lang);
            let space_info = get_volume_space(v);
            let space_str = if let Some((tot, fre)) = space_info {
                let used = tot.saturating_sub(fre);
                match lang {
                    Language::Zh => format!("已用 {} / 共 {}", fmt_size(used), fmt_size(tot)),
                    Language::En => format!("Used {} / Total {}", fmt_size(used), fmt_size(tot)),
                }
            } else {
                String::new()
            };

            let item_id = SharedString::from(format!("dropdown-vol-item-{}", v.display()));
            let v_clone = v.clone();

            div()
                .id(item_id)
                .flex()
                .items_center()
                .justify_between()
                .gap_3()
                .px_3()
                .py_2()
                .rounded_lg()
                .cursor_pointer()
                .when(active, |d| d.bg(rgb(PRIMARY_FIXED)))
                .when(!active, |d| d.hover(|h| h.bg(rgb(SURF_HIGH))))
                .child(
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
                .when(active, |d| {
                    d.child(
                        div()
                            .text_xs()
                            .font_weight(gpui::FontWeight::BOLD)
                            .text_color(rgb(PRIMARY))
                            .child("✓"),
                    )
                })
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.switch_disk_volume(v_clone.clone(), cx);
                    cx.notify();
                }))
                .into_any_element()
        })
        .collect();

    let dropdown_card = div()
        .id("disk-volume-dropdown-card")
        .w(px(320.))
        .max_h(px(380.))
        .overflow_scroll()
        .bg(rgb(CARD))
        .border_1()
        .border_color(rgba(OUTLINE_VAR, 0.7))
        .rounded_xl()
        .shadow_lg()
        .p_2()
        .flex()
        .flex_col()
        .gap_1()
        .child(
            div()
                .px_3()
                .py_1p5()
                .flex()
                .items_center()
                .justify_between()
                .border_b_1()
                .border_color(rgba(OUTLINE_VAR, 0.3))
                .child(
                    div()
                        .text_xs()
                        .font_weight(gpui::FontWeight::BOLD)
                        .text_color(rgb(MUTED))
                        .child(title),
                )
                .child(div().text_xs().text_color(rgb(OUTLINE)).child(count_hint)),
        )
        .children(items);

    // 浮层全屏遮罩与定位
    let overlay = div()
        .id("disk-volume-dropdown-backdrop")
        .absolute()
        .inset_0()
        .on_mouse_down(
            gpui::MouseButton::Left,
            cx.listener(|this, _, _, cx| {
                this.close_disk_volume_menu();
                cx.notify();
            }),
        )
        .child(
            div()
                .absolute()
                .top(px(168.))
                .left(px(280.))
                .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| {
                    cx.stop_propagation();
                })
                .child(dropdown_card),
        );

    Some(overlay.into_any_element())
}
