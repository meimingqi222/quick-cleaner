//! 磁盘分析视图（Disk Lens 空间透镜与智能层级浏览器）

use super::disk_left::render_left_lens_pane;
use super::disk_right::render_right_browser_pane;
use super::disk_volume::format_volume_label;
use crate::core::disk::ScanResult;
use crate::core::i18n::Language;
use crate::core::model::fmt_size;
use crate::platform::get_volume_space;
use crate::ui::components::controls::{loading_state_view, page_heading};
use crate::ui::components::icons::*;
use crate::ui::i18n::*;
use crate::ui::theme::*;
use crate::ui::Root;
use gpui::{div, prelude::*, px, rgb, AnyElement, Context, IntoElement, SharedString};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DiskTab {
    Tree,
    Files,
}

pub fn render_disk_view(root: &Root, cx: &mut Context<Root>) -> AnyElement {
    let lang = root.language;
    let header = div().flex().items_center().justify_between().gap_4().child(
        div().flex_1().min_w(px(0.)).child(page_heading(
            tr_disk_heading(lang),
            tr_disk_subheading(lang),
        )),
    );

    let (loading_title, loading_sub) = match lang {
        Language::Zh => (
            format!("正在深度分析磁盘 {} 空间占用", root.disk.volume),
            "快速索引全盘文件结构与体积分布，请稍候".to_string(),
        ),
        Language::En => (
            format!("Analyzing storage for drive {}…", root.disk.volume),
            if cfg!(target_os = "macos") {
                "Scanning file hierarchy and sizes, please wait".to_string()
            } else {
                "Indexing NTFS file hierarchy and sizes, please wait".to_string()
            },
        ),
    };

    let body = if root.disk.scanning {
        loading_state_view(&loading_title, &loading_sub, root.anim_phase)
    } else if let Some(ref err) = root.disk.error {
        let err_hint = match lang {
            Language::Zh => "请确保以管理员权限运行，或切换至其他可用盘符重试",
            Language::En => "Please ensure running as administrator or switch to another drive",
        };
        let err_prefix = match lang {
            Language::Zh => "磁盘分析失败：",
            Language::En => "Disk analysis failed: ",
        };
        div()
            .flex_1()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_3()
            .p_12()
            .child(icon_badge(
                icon_trash(ERROR, 24.),
                ERROR_CONTAINER,
                ERROR,
                56.,
            ))
            .child(
                div()
                    .text_base()
                    .font_weight(gpui::FontWeight::BOLD)
                    .text_color(rgb(ERROR))
                    .child(format!("{err_prefix}{}", tr_scan_error(lang, err))),
            )
            .child(div().text_xs().text_color(rgb(OUTLINE)).child(err_hint))
            .into_any_element()
    } else if let Some(ref scan) = root.disk.mft {
        render_disk_lens_panes(root, scan, cx)
    } else {
        let prompt_title = match lang {
            Language::Zh => "选择要分析的磁盘并开始深度扫描",
            Language::En => "Select a drive to analyze storage hierarchy",
        };
        let current_label = format_volume_label(&root.disk.volume, lang);
        let btn_scan_text = match lang {
            Language::Zh => format!("开始分析 {current_label} 空间占用"),
            Language::En => format!("Analyze Storage for {current_label}"),
        };

        let vol_cards: Vec<_> = root
            .disk
            .volumes
            .iter()
            .map(|v| {
                let active = root.disk.volume == *v;
                let vol_label = format_volume_label(v, lang);
                let space_info = get_volume_space(v);
                let space_str = if let Some((tot, fre)) = space_info {
                    let used = tot.saturating_sub(fre);
                    match lang {
                        Language::Zh => {
                            format!("已用 {} / 共 {}", fmt_size(used), fmt_size(tot))
                        }
                        Language::En => {
                            format!("Used {} / Total {}", fmt_size(used), fmt_size(tot))
                        }
                    }
                } else {
                    String::new()
                };

                let item_id = SharedString::from(format!("init-vol-card-{}", v.display()));
                let v_clone = v.clone();

                div()
                    .id(item_id)
                    .w(px(220.))
                    .px_4()
                    .py_3()
                    .rounded_xl()
                    .cursor_pointer()
                    .border_2()
                    .when(active, |d| {
                        d.bg(rgb(PRIMARY_FIXED)).border_color(rgb(PRIMARY))
                    })
                    .when(!active, |d| {
                        d.bg(rgb(CARD))
                            .border_color(rgba(OUTLINE_VAR, 0.6))
                            .hover(|h| h.bg(rgb(SURF_HIGH)).border_color(rgba(PRIMARY, 0.5)))
                    })
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(icon_badge(
                        icon_disk(if active { PRIMARY } else { TEXT }, 18.),
                        if active { PRIMARY_FIXED } else { SURF_LOW },
                        if active { PRIMARY } else { OUTLINE_VAR },
                        36.,
                    ))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(1.))
                            .min_w(px(0.))
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(gpui::FontWeight::BOLD)
                                    .text_color(if active { rgb(PRIMARY) } else { rgb(TEXT) })
                                    .child(vol_label),
                            )
                            .when(!space_str.is_empty(), |d| {
                                d.child(div().text_xs().text_color(rgb(OUTLINE)).child(space_str))
                            }),
                    )
                    .on_click({
                        cx.listener(move |this, _, _, cx| {
                            this.switch_disk_volume(v_clone.clone(), cx);
                        })
                    })
            })
            .collect();

        div()
            .flex_1()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_5()
            .p_12()
            .child(icon_badge(
                icon_disk(PRIMARY, 28.),
                PRIMARY_FIXED,
                PRIMARY,
                64.,
            ))
            .child(
                div()
                    .text_base()
                    .font_weight(gpui::FontWeight::BOLD)
                    .text_color(rgb(TEXT))
                    .child(prompt_title),
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .justify_center()
                    .gap_3()
                    .max_w(px(720.))
                    .children(vol_cards),
            )
            .child(
                div()
                    .id("start-mft-scan-btn")
                    .pt_2()
                    .child(crate::ui::components::buttons::primary_button(
                        btn_scan_text,
                        true,
                    ))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.start_mft_scan(cx);
                    })),
            )
            .into_any_element()
    };

    div()
        .id("disk-scroll")
        .size_full()
        .min_w(px(0.))
        .overflow_scroll()
        .p_8()
        .flex()
        .flex_col()
        .gap_5()
        .child(header)
        .child(body)
        .into_any_element()
}

/// 渲染 Disk Lens 左右双栏结构
fn render_disk_lens_panes(root: &Root, scan: &ScanResult, cx: &mut Context<Root>) -> AnyElement {
    let left_pane = render_left_lens_pane(root, scan, cx);
    let right_pane = render_right_browser_pane(root, scan, cx);

    div()
        .flex_1()
        .flex()
        .gap_6()
        .w_full()
        .min_h(px(520.))
        .child(left_pane)
        .child(right_pane)
        .into_any_element()
}
