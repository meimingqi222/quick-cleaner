//! 磁盘透镜左侧面板（透镜视图 + 面包屑 + 标签切换）

use super::disk_breakdown::{compute_breakdown, render_proportion_bar};
use super::disk_components::render_breakdown_row;
use super::disk_volume::render_volume_selector_button;
use crate::core::disk::{Node, ScanResult};
use crate::core::i18n::Language;
use crate::core::model::{fmt_size, truncate};
use crate::ui::components::cards::card;
use crate::ui::components::path_tooltip;
use crate::ui::components::donut::{render_donut, DonutSegment};
use crate::ui::i18n::*;
use crate::ui::theme::*;
use crate::ui::views::DiskTab;
use crate::ui::Root;
use gpui::{div, prelude::*, px, rgb, AnyElement, Context, IntoElement, SharedString};

pub(super) fn render_left_lens_pane(
    root: &Root,
    scan: &ScanResult,
    cx: &mut Context<Root>,
) -> AnyElement {
    let tree = &scan.tree;
    let cur = *root.disk.path.last().unwrap_or(&tree.root());
    // 目录列表、完整路径与保护状态都在 Root::refresh_render_caches 里算好，
    // 这里直接借用；以前每帧都要为每一行重新解析路径并跑一遍保护规则。
    let children: Vec<&Node> = root.disk.rows.iter().map(|r| &r.node).collect();

    // 调色盘（预设优雅高质感色彩）
    let colors = [
        0x0078d4, // 0: Primary Blue
        0x7547ab, // 1: Purple
        0x059669, // 2: Emerald Green
        0xd97706, // 3: Amber Orange
        0x10b981, // 4: Emerald Green (Free / Available Space)
        0x64748b, // 5: Slate Gray (Others)
    ];

    let lang = root.language;

    let total_cap_str = if let Some((tot, _)) = root.disk.space {
        match lang {
            Language::Zh => format!("{} 总容量", fmt_size(tot)),
            Language::En => format!("{} Total", fmt_size(tot)),
        }
    } else {
        match lang {
            // 整卷总量必须用 `unique_size` 而不是 `total_size`。
            //
            // NTFS 上一个文件可以从多个目录被硬链接进来（WinSxS 组件存储
            // 大量这么做），`total_size` 是**按路径的表观体积**——每个链接
            // 位置各计一次，见 `mft_scanner` 里 `total_size += hard_link_size`。
            // 那个口径对"这个目录占多大"是对的，但把它当整卷总量就会报出
            // 一个超过磁盘实际占用的数字。`unique_size` 是同一次扫描里已经
            // 算好的去重口径，直接用。
            //
            // macOS 侧两者恒等（`devscan::macos` 里 `unique_size = total_size`），
            // 所以这里不需要 `#[cfg]` 分支。
            Language::Zh => format!("{} 已扫描", fmt_size(scan.unique_size)),
            Language::En => format!("{} Scanned", fmt_size(scan.unique_size)),
        }
    };

    // 占比清单的计算按 Tab 分两种口径，单独拎出去了
    let (focus_title, focus_size_str, focus_count_str, breakdown) =
        compute_breakdown(root, scan, cur, &children, &colors);

    let ring_segments: Vec<DonutSegment> = breakdown
        .iter()
        .map(|it| DonutSegment {
            ratio: it.ratio as f32,
            color: it.color,
        })
        .collect();

    let proportion_bar = render_proportion_bar(&breakdown);

    let vol_btn = render_volume_selector_button(root, cx);

    // 列表项渲染（支持点击下钻联动）
    let breakdown_rows: Vec<_> = breakdown
        .iter()
        .enumerate()
        .map(|(i, item)| render_breakdown_row(item, i, cx))
        .collect();

    // 主环高亮色（取第一大项色彩）
    let dominant_color = breakdown.first().map(|b| b.color).unwrap_or(PRIMARY);

    let ring_widget = div()
        .w(px(176.))
        .h(px(176.))
        .relative()
        .child(
            div()
                .w(px(176.))
                .h(px(176.))
                .child(render_donut(ring_segments, 176.0, 14.0)),
        )
        .child(
            div()
                .absolute()
                .top(px(18.))
                .left(px(18.))
                .w(px(140.))
                .h(px(140.))
                .rounded_full()
                .bg(rgb(CARD))
                .shadow_md()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .p_2()
                .gap(px(2.))
                .child(
                    div()
                        .text_xs()
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(rgb(dominant_color))
                        .child(truncate(&focus_title, 16)),
                )
                .child(
                    div()
                        .text_xl()
                        .font_weight(gpui::FontWeight::BOLD)
                        .text_color(rgb(TEXT))
                        .child(focus_size_str),
                )
                .child(
                    div()
                        .text_xs()
                        .font_weight(gpui::FontWeight::MEDIUM)
                        .text_color(rgb(OUTLINE))
                        .text_center()
                        .child(focus_count_str),
                ),
        );

    card()
        .w(px(290.))
        .flex_none()
        .p_4()
        .flex()
        .flex_col()
        .justify_between()
        .gap_3()
        // 顶部盘符选择与总容量（固定单行高度，避免多磁盘时撑大卡片）
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .gap_2()
                .h(px(28.))
                .child(vol_btn)
                .child(
                    div()
                        .px_2()
                        .py(px(2.))
                        .rounded_md()
                        .bg(rgb(SURF_HIGH))
                        .text_xs()
                        .font_weight(gpui::FontWeight::MEDIUM)
                        .text_color(rgb(OUTLINE))
                        .child(total_cap_str),
                ),
        )
        // 中间当前目录空间透镜
        .child(
            div()
                .flex_1()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap_3()
                .py_2()
                .child(ring_widget)
                // 空间比例分布条
                .child(
                    div()
                        .w_full()
                        .px_2()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(
                            div()
                                .flex()
                                .justify_between()
                                .text_xs()
                                .text_color(rgb(OUTLINE))
                                .child(tr_space_breakdown(lang))
                                .child(tr_top_n_categories(lang, breakdown.len())),
                        )
                        .child(proportion_bar),
                ),
        )
        // 底部图例与子项明细（带色彩与占比，可直接点击下钻）
        .child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .pt_2()
                .border_t_1()
                .border_color(rgba(OUTLINE_VAR, 0.4))
                .children(breakdown_rows),
        )
        .into_any_element()
}

/// 右侧：智能层级与文件列表浏览器
/// 面包屑导航。
///
/// 折叠态只显示「首段 + … + 末两段」，深路径（如 ~/Library/Application
/// Support 有六层）下说不清自己在哪，也认不出相邻层级——把「…」做成
/// 可点击的展开开关，展开后显示全部层级；每段 hover 有到该层为止的
/// 完整路径提示，末段（当前位置）提示完整绝对路径。
pub(super) fn render_breadcrumbs(
    root: &Root,
    scan: &ScanResult,
    cx: &mut Context<Root>,
) -> gpui::Div {
    let lang = root.language;
    let tree = &scan.tree;
    let depth = root.disk.path.len();

    let mut crumbs = div()
        .flex()
        .items_center()
        .gap_1()
        .flex_1()
        .min_w(px(0.))
        .overflow_hidden();

    let max_display_crumbs = 3;
    let path_len = root.disk.path.len();
    let expanded = root.disk.crumbs_expanded || path_len <= max_display_crumbs;
    let display_indices: Vec<(usize, u32)> = if expanded {
        root.disk.path.iter().copied().enumerate().collect()
    } else {
        let mut list = vec![(0, root.disk.path[0])];
        for i in (path_len - 2)..path_len {
            list.push((i, root.disk.path[i]));
        }
        list
    };

    // 逐段累积路径：每段的提示是「从根到这一层」的完整路径。
    let mount = tree.volume().mount_point().to_path_buf();
    let mut acc = mount.clone();

    for (pos, &(i, idx)) in display_indices.iter().enumerate() {
        let is_root = idx == tree.root();
        let last = i + 1 == depth;
        if pos > 0 && !is_root {
            acc = acc.join(tree.name_of(idx));
        }
        let tooltip_text = acc.to_string_lossy().to_string();
        let crumb_name = if is_root {
            match lang {
                Language::Zh => format!("{}: 根目录", tree.volume()),
                Language::En => format!("{}: Root", tree.volume()),
            }
        } else {
            truncate(&tree.name_of(idx), 16)
        };

        // 折叠时第二段之前插入可点击的「…」：hover 显示完整路径，
        // 点击展开全部层级。
        if pos == 1 && !expanded {
            crumbs = crumbs
                .child(
                    div()
                        .id("crumb-expand")
                        .px_2()
                        .py(px(2.))
                        .rounded_md()
                        .text_xs()
                        .flex_none()
                        .cursor_pointer()
                        .text_color(rgb(PRIMARY))
                        .hover(|h| h.bg(rgb(SURF_HIGH)))
                        .child("…")
                        .tooltip(path_tooltip(&tooltip_text))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.disk.crumbs_expanded = true;
                            cx.notify();
                        })),
                )
                .child(div().text_xs().text_color(rgb(OUTLINE)).child("›"));
        }

        crumbs = crumbs.child(
            div()
                .id(SharedString::from(format!("crumb-{i}-{idx}")))
                .px_2()
                .py(px(2.))
                .rounded_md()
                .text_xs()
                .cursor_pointer()
                .flex_none()
                .when(last, |d| {
                    d.text_color(rgb(TEXT)).font_weight(gpui::FontWeight::BOLD)
                })
                .when(!last, |d| {
                    d.text_color(rgb(PRIMARY)).hover(|h| h.bg(rgb(SURF_HIGH)))
                })
                .child(crumb_name)
                .tooltip(path_tooltip(&tooltip_text))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.disk.path.truncate(i + 1);
                    cx.notify();
                })),
        );
        if !last {
            crumbs = crumbs.child(div().text_xs().text_color(rgb(OUTLINE)).child("›"));
        }
    }

    // 展开态且层级确实很多时，末尾给个收起开关，别让长路径常驻挤占工具栏。
    if expanded && path_len > max_display_crumbs {
        crumbs = crumbs.child(
            div()
                .id("crumb-collapse")
                .px_2()
                .py(px(2.))
                .rounded_md()
                .text_xs()
                .flex_none()
                .cursor_pointer()
                .text_color(rgb(OUTLINE))
                .hover(|h| h.bg(rgb(SURF_HIGH)))
                .child("‹ 收起")
                .on_click(cx.listener(|this, _, _, cx| {
                    this.disk.crumbs_expanded = false;
                    cx.notify();
                })),
        );
    }

    crumbs
}

/// 「目录树 / 全盘大文件」两个视图之间的切换。
pub(super) fn render_tab_switch(root: &Root, cx: &mut Context<Root>) -> gpui::Div {
    let lang = root.language;

    // 视图切换（目录树 / 全盘大文件）
    let tab_switch = div()
        .flex()
        .items_center()
        .gap_1()
        .p(px(2.))
        .rounded_lg()
        .bg(rgb(SURF_LOW))
        .flex_none()
        .child(
            div()
                .id("tab-tree")
                .px_2()
                .py(px(3.))
                .rounded_md()
                .text_xs()
                .font_weight(if root.disk.tab == DiskTab::Tree {
                    gpui::FontWeight::BOLD
                } else {
                    gpui::FontWeight::NORMAL
                })
                .cursor_pointer()
                .when(root.disk.tab == DiskTab::Tree, |d| {
                    d.bg(rgb(CARD)).text_color(rgb(PRIMARY)).shadow_sm()
                })
                .when(root.disk.tab != DiskTab::Tree, |d| {
                    d.text_color(rgb(MUTED)).hover(|h| h.bg(rgb(SURF_HIGH)))
                })
                .child(tr_tab_tree(lang))
                .on_click(cx.listener(|this, _, _, cx| {
                    this.disk.tab = DiskTab::Tree;
                    cx.notify();
                })),
        )
        .child(
            div()
                .id("tab-files")
                .px_2()
                .py(px(3.))
                .rounded_md()
                .text_xs()
                .font_weight(if root.disk.tab == DiskTab::Files {
                    gpui::FontWeight::BOLD
                } else {
                    gpui::FontWeight::NORMAL
                })
                .cursor_pointer()
                .when(root.disk.tab == DiskTab::Files, |d| {
                    d.bg(rgb(CARD)).text_color(rgb(PRIMARY)).shadow_sm()
                })
                .when(root.disk.tab != DiskTab::Files, |d| {
                    d.text_color(rgb(MUTED)).hover(|h| h.bg(rgb(SURF_HIGH)))
                })
                .child(tr_tab_files(lang))
                .on_click(cx.listener(|this, _, _, cx| {
                    this.disk.tab = DiskTab::Files;
                    cx.notify();
                })),
        );

    tab_switch
}
