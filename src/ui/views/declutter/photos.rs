//! 相似照片与连拍视图 (Similar Photos Gallery)

use super::common::{render_empty_state_card, render_unified_nav_header};
use super::DeclutterTab;
use crate::core::declutter::clean_declutter_items;
use crate::core::i18n::Language;
use crate::core::model::fmt_size;
use crate::ui::components::controls::checkbox;
use crate::ui::components::icons::{icon_folder_large, icon_sparkle, icon_star};
use crate::ui::theme::*;
use crate::ui::Root;
use gpui::prelude::*;
use gpui::{div, img, px, rgb, AnyElement, Context, SharedString};
use std::path::PathBuf;

pub fn render_similar_photos_tab(root: &Root, cx: &mut Context<Root>) -> AnyElement {
    let lang = root.language;
    let state = &root.declutter;

    let total_cleanable: u64 = state.photo_groups.iter().map(|g| g.cleanable_size()).sum();
    let total_selected_count: usize = state
        .photo_groups
        .iter()
        .flat_map(|g| &g.photos)
        .filter(|p| p.selected)
        .count();

    let tab_nav = render_unified_nav_header(
        DeclutterTab::SimilarPhotos,
        match lang {
            Language::Zh => "相似图片",
            Language::En => "Similar Photos",
        },
        lang,
        cx,
    );

    let display_groups: Vec<_> = state.photo_groups.iter().take(30).enumerate().collect();

    let groups_view: Vec<AnyElement> = if display_groups.is_empty() {
        vec![render_empty_state_card(
            "🖼️",
            match lang {
                Language::Zh => "暂未发现相似或连拍冗余照片",
                Language::En => "No similar or burst photos found",
            },
            match lang {
                Language::Zh => "您的相册非常整洁，未发现连拍或高重复度照片。",
                Language::En => "Your photo library is clean without redundant bursts.",
            },
        )]
    } else {
        display_groups
            .into_iter()
            .map(|(g_idx, group)| {
                let group_title = if lang == Language::Zh {
                    &group.title_zh
                } else {
                    &group.title_en
                };

                let is_expanded = state.expanded_photo_groups.contains(&g_idx);

                // 拆分：最佳品质原件 (Best Shot) 与 待清理副本 (Redundant Copies)
                let best_photo = group.photos.iter().enumerate().find(|(_, p)| p.is_best_shot)
                    .or_else(|| group.photos.iter().enumerate().next());

                let redundant_photos: Vec<(usize, &crate::core::declutter::PhotoItem)> = group
                    .photos
                    .iter()
                    .enumerate()
                    .filter(|(idx, p)| {
                        if let Some((best_idx, _)) = best_photo {
                            *idx != best_idx
                        } else {
                            !p.is_best_shot
                        }
                    })
                    .collect();

                let redundant_total_count = redundant_photos.len();
                let display_redundant: Vec<_> = if is_expanded || redundant_total_count <= 4 {
                    redundant_photos.clone()
                } else {
                    redundant_photos.iter().take(4).copied().collect()
                };

                // 1. 渲染最佳品质原件卡片 (Hero Card)
                let best_card_element = if let Some((best_p_idx, best_p)) = best_photo {
                    let photo_path = best_p.path.clone();
                    let photo_name = best_p.filename.clone();
                    let photo_path_for_menu = best_p.path.clone();
                    let ext = best_p
                        .path
                        .extension()
                        .and_then(|e| e.to_str())
                        .unwrap_or("")
                        .to_lowercase();
                    let can_render_img = matches!(
                        ext.as_str(),
                        "jpg" | "jpeg" | "png" | "webp" | "bmp" | "gif"
                    ) && best_p.size <= 8_000_000;

                    div()
                        .w(px(250.))
                        .flex_none()
                        .rounded_xl()
                        .border_2()
                        .border_color(rgba(PRIMARY, 0.9))
                        .bg(rgb(PRIMARY_FIXED))
                        .overflow_hidden()
                        .flex()
                        .flex_col()
                        .on_mouse_down(
                            gpui::MouseButton::Right,
                            cx.listener(move |this, event: &gpui::MouseDownEvent, _, cx| {
                                let x: f32 = event.position.x.into();
                                let y: f32 = event.position.y.into();
                                this.open_declutter_context_menu(
                                    photo_path_for_menu.clone(),
                                    photo_name.clone(),
                                    x,
                                    y,
                                );
                                cx.notify();
                            }),
                        )
                        .child(
                            // 缩略图
                            div()
                                .id(SharedString::from(format!("best-thumb-{g_idx}-{best_p_idx}")))
                                .h(px(135.))
                                .w_full()
                                .bg(if can_render_img {
                                    rgb(SURF_HIGH)
                                } else {
                                    rgb(best_p.bg_gradient_seed)
                                })
                                .relative()
                                .overflow_hidden()
                                .cursor_pointer()
                                .when(can_render_img, |d| {
                                    d.child(img(photo_path.clone()).size_full())
                                })
                                .when(!can_render_img, |d| {
                                    d.flex()
                                        .items_center()
                                        .justify_center()
                                        .child(
                                            div()
                                                .flex()
                                                .flex_col()
                                                .items_center()
                                                .gap_1()
                                                .child(icon_folder_large(0xffffff, 24.))
                                                .child(
                                                    div()
                                                        .text_xs()
                                                        .font_weight(gpui::FontWeight::BOLD)
                                                        .text_color(rgb(0xffffff))
                                                        .child(ext.to_uppercase()),
                                                ),
                                        )
                                })
                                .child(
                                    // 最佳品质徽章
                                    div()
                                        .absolute()
                                        .top_2()
                                        .left_2()
                                        .px_2()
                                        .py(px(2.))
                                        .rounded_md()
                                        .bg(rgb(PRIMARY))
                                        .text_xs()
                                        .font_weight(gpui::FontWeight::BOLD)
                                        .text_color(rgb(ON_PRIMARY))
                                        .flex()
                                        .items_center()
                                        .gap_1()
                                        .child(icon_star(0xffffff, 11.))
                                        .child(match lang {
                                            Language::Zh => "最佳品质 (已保留)",
                                            Language::En => "Best Quality (Kept)",
                                        }),
                                )
                                .child(
                                    // 浮层操作按钮
                                    div()
                                        .absolute()
                                        .bottom_2()
                                        .right_2()
                                        .flex()
                                        .items_center()
                                        .gap_1()
                                        .child(
                                            div()
                                                .id(SharedString::from(format!("btn-reveal-best-{g_idx}-{best_p_idx}")))
                                                .px_2()
                                                .py(px(2.))
                                                .rounded_md()
                                                .bg(rgba(0x000000, 0.75))
                                                .hover(|h| h.bg(rgba(0x000000, 0.95)))
                                                .text_xs()
                                                .font_weight(gpui::FontWeight::MEDIUM)
                                                .text_color(rgb(0xffffff))
                                                .cursor_pointer()
                                                .child("定位")
                                                .on_click({
                                                    let p = photo_path.clone();
                                                    cx.listener(move |_, _event: &gpui::ClickEvent, _, cx| {
                                                        cx.stop_propagation();
                                                        crate::platform::reveal_in_explorer(&p);
                                                    })
                                                }),
                                        )
                                        .child(
                                            div()
                                                .id(SharedString::from(format!("btn-preview-best-{g_idx}-{best_p_idx}")))
                                                .px_2()
                                                .py(px(2.))
                                                .rounded_md()
                                                .bg(rgba(0x000000, 0.75))
                                                .hover(|h| h.bg(rgba(0x000000, 0.95)))
                                                .text_xs()
                                                .font_weight(gpui::FontWeight::MEDIUM)
                                                .text_color(rgb(0xffffff))
                                                .cursor_pointer()
                                                .child("看图")
                                                .on_click({
                                                    let p = photo_path.clone();
                                                    cx.listener(move |_, _event: &gpui::ClickEvent, _, cx| {
                                                        cx.stop_propagation();
                                                        crate::platform::open_in_default_app(&p);
                                                    })
                                                }),
                                        ),
                                )
                                .on_click({
                                    let p = photo_path.clone();
                                    cx.listener(move |_, _event: &gpui::ClickEvent, _, cx| {
                                        cx.stop_propagation();
                                        crate::platform::open_in_default_app(&p);
                                    })
                                }),
                        )
                        .child(
                            div()
                                .p_3()
                                .flex()
                                .flex_col()
                                .gap_1()
                                .child(
                                    div()
                                        .text_xs()
                                        .font_weight(gpui::FontWeight::BOLD)
                                        .text_color(rgb(TEXT))
                                        .overflow_hidden()
                                        .child(best_p.filename.clone()),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .justify_between()
                                        .text_xs()
                                        .text_color(rgb(MUTED))
                                        .child(format!("{} x {}", best_p.dimensions.0, best_p.dimensions.1))
                                        .child(fmt_size(best_p.size)),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(rgb(MUTED))
                                        .child(best_p.date_str.clone()),
                                ),
                        )
                        .into_any_element()
                } else {
                    div().into_any_element()
                };

                // 2. 渲染待清理副本卡片网格 (Redundant Copies Grid)
                let redundant_card_elements: Vec<AnyElement> = display_redundant
                    .into_iter()
                    .map(|(p_idx, photo)| {
                        let is_sel = photo.selected;
                        let photo_path = photo.path.clone();
                        let photo_name = photo.filename.clone();
                        let photo_path_for_menu = photo.path.clone();

                        let ext = photo
                            .path
                            .extension()
                            .and_then(|e| e.to_str())
                            .unwrap_or("")
                            .to_lowercase();
                        let can_render_img = matches!(
                            ext.as_str(),
                            "jpg" | "jpeg" | "png" | "webp" | "bmp" | "gif"
                        ) && photo.size <= 8_000_000;

                        div()
                            .id(SharedString::from(format!("redundant-card-{g_idx}-{p_idx}")))
                            .w(px(205.))
                            .flex_none()
                            .rounded_xl()
                            .border_1()
                            .border_color(if is_sel {
                                rgba(ERROR, 0.9)
                            } else {
                                rgba(OUTLINE_VAR, 0.4)
                            })
                            .bg(rgb(CARD))
                            .overflow_hidden()
                            .flex()
                            .flex_col()
                            .on_mouse_down(
                                gpui::MouseButton::Right,
                                cx.listener(move |this, event: &gpui::MouseDownEvent, _, cx| {
                                    let x: f32 = event.position.x.into();
                                    let y: f32 = event.position.y.into();
                                    this.open_declutter_context_menu(
                                        photo_path_for_menu.clone(),
                                        photo_name.clone(),
                                        x,
                                        y,
                                    );
                                    cx.notify();
                                }),
                            )
                            .child(
                                div()
                                    .id(SharedString::from(format!("photo-thumb-{g_idx}-{p_idx}")))
                                    .h(px(115.))
                                    .w_full()
                                    .bg(if can_render_img {
                                        rgb(SURF_HIGH)
                                    } else {
                                        rgb(photo.bg_gradient_seed)
                                    })
                                    .relative()
                                    .overflow_hidden()
                                    .cursor_pointer()
                                    .when(can_render_img, |d| {
                                        d.child(img(photo_path.clone()).size_full())
                                    })
                                    .when(!can_render_img, |d| {
                                        d.flex()
                                            .items_center()
                                            .justify_center()
                                            .child(
                                                div()
                                                    .flex()
                                                    .flex_col()
                                                    .items_center()
                                                    .gap_1()
                                                    .child(icon_folder_large(0xffffff, 20.))
                                                    .child(
                                                        div()
                                                            .text_xs()
                                                            .font_weight(gpui::FontWeight::BOLD)
                                                            .text_color(rgb(0xffffff))
                                                            .child(ext.to_uppercase()),
                                                    ),
                                            )
                                    })
                                    .child(
                                        // 待清理状态小标签
                                        div()
                                            .absolute()
                                            .top_2()
                                            .left_2()
                                            .px_2()
                                            .py(px(2.))
                                            .rounded_md()
                                            .bg(if is_sel {
                                                rgba(ERROR, 0.9)
                                            } else {
                                                rgba(0x000000, 0.6)
                                            })
                                            .text_xs()
                                            .font_weight(gpui::FontWeight::BOLD)
                                            .text_color(rgb(0xffffff))
                                            .child(if is_sel {
                                                match lang {
                                                    Language::Zh => "待清理",
                                                    Language::En => "To Clean",
                                                }
                                            } else {
                                                match lang {
                                                    Language::Zh => "已保留",
                                                    Language::En => "Kept",
                                                }
                                            }),
                                    )
                                    .child(
                                        div()
                                            .absolute()
                                            .bottom_2()
                                            .right_2()
                                            .flex()
                                            .items_center()
                                            .gap_1()
                                            .child(
                                                div()
                                                    .id(SharedString::from(format!("btn-reveal-red-{g_idx}-{p_idx}")))
                                                    .px_2()
                                                    .py(px(2.))
                                                    .rounded_md()
                                                    .bg(rgba(0x000000, 0.75))
                                                    .hover(|h| h.bg(rgba(0x000000, 0.95)))
                                                    .text_xs()
                                                    .font_weight(gpui::FontWeight::MEDIUM)
                                                    .text_color(rgb(0xffffff))
                                                    .cursor_pointer()
                                                    .child("定位")
                                                    .on_click({
                                                        let p = photo_path.clone();
                                                        cx.listener(move |_, _event: &gpui::ClickEvent, _, cx| {
                                                            cx.stop_propagation();
                                                            crate::platform::reveal_in_explorer(&p);
                                                        })
                                                    }),
                                            )
                                            .child(
                                                div()
                                                    .id(SharedString::from(format!("btn-preview-red-{g_idx}-{p_idx}")))
                                                    .px_2()
                                                    .py(px(2.))
                                                    .rounded_md()
                                                    .bg(rgba(0x000000, 0.75))
                                                    .hover(|h| h.bg(rgba(0x000000, 0.95)))
                                                    .text_xs()
                                                    .font_weight(gpui::FontWeight::MEDIUM)
                                                    .text_color(rgb(0xffffff))
                                                    .cursor_pointer()
                                                    .child("看图")
                                                    .on_click({
                                                        let p = photo_path.clone();
                                                        cx.listener(move |_, _event: &gpui::ClickEvent, _, cx| {
                                                            cx.stop_propagation();
                                                            crate::platform::open_in_default_app(&p);
                                                        })
                                                    }),
                                            ),
                                    )
                                    .on_click({
                                        let p = photo_path.clone();
                                        cx.listener(move |_, _event: &gpui::ClickEvent, _, cx| {
                                            cx.stop_propagation();
                                            crate::platform::open_in_default_app(&p);
                                        })
                                    }),
                            )
                            .child(
                                div()
                                    .p_3()
                                    .flex()
                                    .flex_col()
                                    .gap_2()
                                    .child(
                                        div()
                                            .id(SharedString::from(format!("photo-sel-{g_idx}-{p_idx}")))
                                            .flex()
                                            .items_center()
                                            .gap_2()
                                            .cursor_pointer()
                                            .child(checkbox(if is_sel {
                                                crate::core::model::Check::On
                                            } else {
                                                crate::core::model::Check::Off
                                            }))
                                            .child(
                                                div()
                                                    .text_xs()
                                                    .font_weight(gpui::FontWeight::BOLD)
                                                    .text_color(rgb(if is_sel { ERROR } else { TEXT }))
                                                    .overflow_hidden()
                                                    .child(photo.filename.clone()),
                                            )
                                            .on_click(cx.listener(move |this, _event: &gpui::ClickEvent, _, cx| {
                                                cx.stop_propagation();
                                                if let Some(g) = this.declutter.photo_groups.get_mut(g_idx) {
                                                    if let Some(p) = g.photos.get_mut(p_idx) {
                                                        p.selected = !p.selected;
                                                        cx.notify();
                                                    }
                                                }
                                            })),
                                    )
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .justify_between()
                                            .text_xs()
                                            .child(
                                                div()
                                                    .text_color(rgb(MUTED))
                                                    .child(format!("{} x {}", photo.dimensions.0, photo.dimensions.1)),
                                            )
                                            .child(
                                                div()
                                                    .font_weight(gpui::FontWeight::BOLD)
                                                    .text_color(rgb(if is_sel { ERROR } else { TEXT }))
                                                    .child(fmt_size(photo.size)),
                                            ),
                                    ),
                            )
                            .into_any_element()
                    })
                    .collect();

                let group_cleanable: u64 = group.cleanable_size();
                let group_sel_count: usize = group.photos.iter().filter(|p| p.selected).count();

                div()
                    .flex_none()
                    .rounded_xl()
                    .bg(rgb(CARD))
                    .border_1()
                    .border_color(rgba(OUTLINE_VAR, 0.4))
                    .shadow_sm()
                    .overflow_hidden()
                    .flex()
                    .flex_col()
                    .child(
                        // 组标题栏
                        div()
                            .px_6()
                            .py_3()
                            .bg(rgb(SURF_LOW))
                            .border_b_1()
                            .border_color(rgba(OUTLINE_VAR, 0.25))
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_3()
                                    .child(
                                        div()
                                            .w(px(26.))
                                            .h(px(26.))
                                            .rounded_full()
                                            .bg(rgb(PRIMARY_FIXED))
                                            .text_xs()
                                            .font_weight(gpui::FontWeight::BOLD)
                                            .text_color(rgb(PRIMARY))
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .child(group.index_str.clone()),
                                    )
                                    .child(
                                        div()
                                            .text_sm()
                                            .font_weight(gpui::FontWeight::BOLD)
                                            .text_color(rgb(TEXT))
                                            .child(group_title.clone()),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(rgb(MUTED))
                                            .child(if lang == Language::Zh {
                                                if group_sel_count > 0 {
                                                    format!("• 共 {} 张照片 (待清理 {} 张 · 可释放 {})", group.photos.len(), group_sel_count, fmt_size(group_cleanable))
                                                } else {
                                                    format!("• 共 {} 张照片 (全部已保留)", group.photos.len())
                                                }
                                            } else {
                                                if group_sel_count > 0 {
                                                    format!("• {} photos ({} to clean · {})", group.photos.len(), group_sel_count, fmt_size(group_cleanable))
                                                } else {
                                                    format!("• {} photos (all kept)", group.photos.len())
                                                }
                                            }),
                                    ),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .child(
                                        div()
                                            .id(SharedString::from(format!("btn-pick-best-{g_idx}")))
                                            .px_2()
                                            .py(px(2.))
                                            .rounded_md()
                                            .bg(rgb(SURF_HIGH))
                                            .hover(|h| h.bg(rgb(PRIMARY_FIXED)).text_color(rgb(PRIMARY)))
                                            .text_xs()
                                            .font_weight(gpui::FontWeight::BOLD)
                                            .text_color(rgb(PRIMARY))
                                            .cursor_pointer()
                                            .child(match lang {
                                                Language::Zh => "★ 仅保留最佳",
                                                Language::En => "★ Keep Best Only",
                                            })
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                if let Some(g) = this.declutter.photo_groups.get_mut(g_idx) {
                                                    for p in &mut g.photos {
                                                        p.selected = !p.is_best_shot;
                                                    }
                                                    cx.notify();
                                                }
                                            })),
                                    )
                                    .child(
                                        div()
                                            .id(SharedString::from(format!("btn-keep-all-{g_idx}")))
                                            .px_2()
                                            .py(px(2.))
                                            .rounded_md()
                                            .bg(rgb(SURF_HIGH))
                                            .hover(|h| h.bg(rgb(SURF_HIGH)).text_color(rgb(TEXT)))
                                            .text_xs()
                                            .font_weight(gpui::FontWeight::MEDIUM)
                                            .text_color(rgb(MUTED))
                                            .cursor_pointer()
                                            .child(match lang {
                                                Language::Zh => "全部保留",
                                                Language::En => "Keep All",
                                            })
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                if let Some(g) = this.declutter.photo_groups.get_mut(g_idx) {
                                                    for p in &mut g.photos {
                                                        p.selected = false;
                                                    }
                                                    cx.notify();
                                                }
                                            })),
                                    ),
                            ),
                    )
                    .child(
                        // 主客对比双栏布局
                        div()
                            .p_6()
                            .flex()
                            .gap_6()
                            .child(best_card_element)
                            .child(
                                div()
                                    .flex_1()
                                    .flex()
                                    .flex_col()
                                    .gap_3()
                                    .child(
                                        div()
                                            .text_xs()
                                            .font_weight(gpui::FontWeight::BOLD)
                                            .text_color(rgb(MUTED))
                                            .child(match lang {
                                                Language::Zh => format!("待清理相似副本 (共 {} 张)：", redundant_total_count),
                                                Language::En => format!("Redundant Copies ({}):", redundant_total_count),
                                            }),
                                    )
                                    .child(
                                        div()
                                            .flex()
                                            .flex_wrap()
                                            .gap_4()
                                            .children(redundant_card_elements)
                                            .when(redundant_total_count > 4, |d| {
                                                d.child(
                                                    div()
                                                        .id(SharedString::from(format!("btn-toggle-expand-{g_idx}")))
                                                        .w(px(205.))
                                                        .h(px(180.))
                                                        .rounded_xl()
                                                        .border_1()
                                                        .border_color(rgba(PRIMARY, 0.4))
                                                        .bg(rgb(SURF_LOW))
                                                        .hover(|h| h.bg(rgb(PRIMARY_FIXED)))
                                                        .flex()
                                                        .flex_col()
                                                        .items_center()
                                                        .justify_center()
                                                        .gap_2()
                                                        .cursor_pointer()
                                                        .child(
                                                            div()
                                                                .text_sm()
                                                                .font_weight(gpui::FontWeight::BOLD)
                                                                .text_color(rgb(PRIMARY))
                                                                .child(if is_expanded {
                                                                    match lang {
                                                                        Language::Zh => "▲ 收起".to_string(),
                                                                        Language::En => "▲ Collapse".to_string(),
                                                                    }
                                                                } else {
                                                                    match lang {
                                                                        Language::Zh => format!("+ 查看其余 {} 张...", redundant_total_count - 4),
                                                                        Language::En => format!("+ {} more...", redundant_total_count - 4),
                                                                    }
                                                                }),
                                                        )
                                                        .on_click(cx.listener(move |this, _, _, cx| {
                                                            if this.declutter.expanded_photo_groups.contains(&g_idx) {
                                                                this.declutter.expanded_photo_groups.remove(&g_idx);
                                                            } else {
                                                                this.declutter.expanded_photo_groups.insert(g_idx);
                                                            }
                                                            cx.notify();
                                                        })),
                                                )
                                            }),
                                    ),
                            ),
                    )
                    .into_any_element()
            })
            .collect()
    };

    div()
        .id("declutter-photos-view")
        .size_full()
        .flex()
        .flex_col()
        .child(
            div()
                .id("declutter-photos-scroll")
                .flex_1()
                .min_h(px(0.))
                .overflow_scroll()
                .p_8()
                .flex()
                .flex_col()
                .gap_6()
                .child(tab_nav)
                .child(
                    div()
                        .flex_none()
                        .flex()
                        .items_end()
                        .justify_between()
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap_1()
                                .max_w(px(560.))
                                .child(
                                    div()
                                        .text_xs()
                                        .font_weight(gpui::FontWeight::BOLD)
                                        .text_color(rgb(PRIMARY))
                                        .child(match lang {
                                            Language::Zh => "◧ 冗余整理",
                                            Language::En => "◧ DECLUTTER MODULE",
                                        }),
                                )
                                .child(
                                    div()
                                        .text_2xl()
                                        .font_weight(gpui::FontWeight::BOLD)
                                        .text_color(rgb(TEXT))
                                        .child(match lang {
                                            Language::Zh => "相似图片整理",
                                            Language::En => "Similar Photos",
                                        }),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(rgb(MUTED))
                                        .child(match lang {
                                            Language::Zh => "审查视觉上高度相似或连拍的图片组。系统已自动为您标出每组最佳品质的照片，只需一键清理冗余版本。",
                                            Language::En => "Review grouped images that appear visually identical or highly similar. We've highlighted the highest quality version in each group.",
                                        }),
                                )
                        )
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_3()
                                .child(
                                    div()
                                        .id("btn-smart-select-photos")
                                        .px_5()
                                        .py_2()
                                        .rounded_full()
                                        .bg(rgb(SURF_HIGH))
                                        .border_1()
                                        .border_color(rgba(OUTLINE_VAR, 0.4))
                                        .hover(|h| h.bg(rgb(SURF_LOW)))
                                        .cursor_pointer()
                                        .text_sm()
                                        .font_weight(gpui::FontWeight::MEDIUM)
                                        .text_color(rgb(TEXT))
                                        .flex()
                                        .items_center()
                                        .gap_2()
                                        .child(icon_sparkle(PRIMARY, 16.))
                                        .child(match lang {
                                            Language::Zh => "✨ 自动挑选最佳",
                                            Language::En => "✨ Smart Select All",
                                        })
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.declutter.auto_pick_best_photos();
                                            cx.notify();
                                        })),
                                )
                                .child(
                                    div()
                                        .id("btn-delete-selected-photos")
                                        .px_5()
                                        .py_2()
                                        .rounded_full()
                                        .bg(rgb(ERROR))
                                        .when(total_selected_count > 0, |d| {
                                            d.hover(|h| h.opacity(0.9)).cursor_pointer()
                                        })
                                        .when(total_selected_count == 0, |d| d.opacity(0.4))
                                        .text_sm()
                                        .font_weight(gpui::FontWeight::BOLD)
                                        .text_color(rgb(ON_PRIMARY))
                                        .child(format!(
                                            "{} ({})",
                                            match lang {
                                                Language::Zh => "清理已选",
                                                Language::En => "Delete Selected",
                                            },
                                            fmt_size(total_cleanable)
                                        ))
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            let paths_to_delete: Vec<PathBuf> = this
                                                .declutter
                                                .photo_groups
                                                .iter()
                                                .flat_map(|g| &g.photos)
                                                .filter(|p| p.selected)
                                                .map(|p| p.path.clone())
                                                .collect();

                                            if !paths_to_delete.is_empty() {
                                                let report = clean_declutter_items(&paths_to_delete, true);
                                                for g in &mut this.declutter.photo_groups {
                                                    g.photos.retain(|p| !p.selected);
                                                }
                                                this.declutter.photo_groups.retain(|g| g.photos.len() >= 2);
                                                this.status = crate::core::i18n::Text::new(
                                                    format!(
                                                        "已清理 {} 张相似照片，释放 {}",
                                                        report.deleted_files,
                                                        fmt_size(report.freed_bytes)
                                                    ),
                                                    format!(
                                                        "Cleaned {} similar photos, freed {}",
                                                        report.deleted_files,
                                                        fmt_size(report.freed_bytes)
                                                    ),
                                                );
                                                cx.notify();
                                            }
                                        })),
                                ),
                        ),
                )
                .children(groups_view),
        )
        .into_any_element()
}
