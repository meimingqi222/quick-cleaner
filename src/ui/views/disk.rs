//! 磁盘分析视图（Disk Lens 空间透镜与智能层级浏览器）

use crate::core::disk::{MftScan, MftTree, Node};
use crate::core::model::{fmt_size, truncate, Check};
use crate::ui::components::buttons::small_button;
use crate::ui::components::cards::card;
use crate::ui::components::controls::{checkbox, loading_state_view, page_heading};
use crate::ui::components::donut::{render_donut, DonutSegment};
use super::disk_components::{render_breakdown_row, BreakdownItem};
use crate::ui::components::icons::*;
use crate::ui::theme::*;
use crate::ui::{DiskRow, Root};
use gpui::{
    div, prelude::*, px, rgb, AnyElement, Context, IntoElement, SharedString,
};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DiskTab {
    Tree,
    Files,
}


pub fn render_disk_view(root: &Root, cx: &mut Context<Root>) -> AnyElement {
    let header = div()
        .flex()
        .items_center()
        .justify_between()
        .gap_4()
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .child(page_heading(
                    "Disk Lens 磁盘透镜",
                    "分析磁盘各层级空间占用，定位大文件与冗余目录",
                )),
        );

    let body = if root.mft_scanning {
        loading_state_view(
            &format!("正在深度分析磁盘 {} 空间占用", root.disk_volume),
            "快速索引全盘文件结构与体积分布，请稍候",
            root.anim_phase,
        )
    } else if let Some(ref err) = root.mft_error {
        div()
            .flex_1()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_3()
            .p_12()
            .child(icon_badge(icon_trash(ERROR, 24.), ERROR_CONTAINER, ERROR, 56.))
            .child(
                div()
                    .text_base()
                    .font_weight(gpui::FontWeight::BOLD)
                    .text_color(rgb(ERROR))
                    .child(format!("磁盘分析失败：{err}")),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(rgb(OUTLINE))
                    .child("请确保以管理员权限运行，或切换至其他可用盘符重试"),
            )
            .into_any_element()
    } else if let Some(ref scan) = root.mft {
        render_disk_lens_panes(root, scan, cx)
    } else {
        let vol_buttons: Vec<_> = root.volumes.iter().map(|&v| {
            let active = root.disk_volume == v;
            div()
                .id(SharedString::from(format!("init-vol-pill-{v}")))
                .px_4()
                .py_2()
                .rounded_lg()
                .text_sm()
                .font_weight(if active {
                    gpui::FontWeight::BOLD
                } else {
                    gpui::FontWeight::MEDIUM
                })
                .cursor_pointer()
                .border_1()
                .when(active, |d| {
                    d.bg(rgb(PRIMARY_FIXED))
                        .border_color(rgb(PRIMARY))
                        .text_color(rgb(PRIMARY))
                })
                .when(!active, |d| {
                    d.bg(rgb(CARD))
                        .border_color(rgba(OUTLINE_VAR, 0.6))
                        .text_color(rgb(TEXT))
                        .hover(|h| h.bg(rgb(SURF_HIGH)))
                })
                .child(format!("{v}: 盘 (NTFS)"))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.switch_disk_volume(v, cx);
                }))
        }).collect();

        div()
            .flex_1()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_4()
            .p_12()
            .child(icon_badge(icon_disk(PRIMARY, 28.), PRIMARY_FIXED, PRIMARY, 64.))
            .child(
                div()
                    .text_base()
                    .font_weight(gpui::FontWeight::BOLD)
                    .text_color(rgb(TEXT))
                    .child(format!("选择磁盘并开始深度分析（当前选择 {}: 盘）", root.disk_volume)),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .children(vol_buttons),
            )
            .child(
                div()
                    .id("start-mft-scan-btn")
                    .pt_2()
                    .child(crate::ui::components::buttons::primary_button(
                        format!("开始分析 {}: 盘空间占用", root.disk_volume),
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
fn render_disk_lens_panes(root: &Root, scan: &MftScan, cx: &mut Context<Root>) -> AnyElement {
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

/// 左侧：Disk Lens 环形透镜与容量分类（与右侧当前目录完全联动）
fn render_left_lens_pane(root: &Root, scan: &MftScan, cx: &mut Context<Root>) -> AnyElement {
    let tree = &scan.tree;
    let cur = *root.disk_path.last().unwrap_or(&tree.root());
    let cur_size = tree.size_of(cur);
    let cur_name = if cur == tree.root() {
        format!("{}: 根目录", tree.volume())
    } else {
        tree.name_of(cur)
    };
    // 目录列表、完整路径与保护状态都在 Root::refresh_render_caches 里算好，
    // 这里直接借用；以前每帧都要为每一行重新解析路径并跑一遍保护规则。
    let children: Vec<&Node> = root.disk_rows.iter().map(|r| &r.node).collect();

    // 调色盘（预设优雅高质感色彩）
    let colors = [
        0x0078d4, // 0: Primary Blue
        0x7547ab, // 1: Purple
        0x059669, // 2: Emerald Green
        0xd97706, // 3: Amber Orange
        0x10b981, // 4: Emerald Green (Free / Available Space)
        0x64748b, // 5: Slate Gray (Others)
    ];

    // 盘符选择标签组
    let volume_pills: Vec<_> = root.volumes.iter().map(|&v| {
        let active = root.disk_volume == v;
        div()
            .id(SharedString::from(format!("vol-pill-{v}")))
            .px_3()
            .py(px(3.))
            .rounded_full()
            .text_xs()
            .font_weight(if active {
                gpui::FontWeight::BOLD
            } else {
                gpui::FontWeight::MEDIUM
            })
            .cursor_pointer()
            .border_1()
            .when(active, |d| {
                d.bg(rgb(PRIMARY_FIXED))
                    .border_color(rgb(PRIMARY))
                    .text_color(rgb(PRIMARY))
            })
            .when(!active, |d| {
                d.bg(rgb(SURF_LOW))
                    .border_color(rgba(OUTLINE_VAR, 0.6))
                    .text_color(rgb(MUTED))
                    .hover(|h| h.bg(rgb(SURF_HIGH)))
            })
            .child(format!("{v}: 盘"))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.switch_disk_volume(v, cx);
            }))
    }).collect();

    let total_cap_str = if let Some((tot, _)) = root.disk_space {
        format!("{} 总容量", fmt_size(tot))
    } else {
        format!("{} 已扫描", fmt_size(scan.total_size))
    };

    // 根据当前 Tab 计算占比清单与圆环扇区
    let (focus_title, focus_size_str, focus_count_str, breakdown) = match root.disk_tab {
        DiskTab::Tree => {
            let is_root = cur == tree.root();
            if is_root && root.disk_space.is_some() {
                let (tot, fre) = root.disk_space.unwrap();
                let used = tot.saturating_sub(fre);
                let used_pct = ((used as f64 / tot.max(1) as f64) * 100.0).round() as u64;

                let mut items = Vec::new();
                let mut top_sum = 0u64;

                // 小于 1% 的根目录项直接并入“其他已用”，保证圆环、占比条和图例完全一致，
                // 避免 0.2%/0.6% 这类肉眼难辨的顶部细条和白缝。
                let min_visible_ratio = 0.01;
                let visible_children: Vec<_> = children
                    .iter()
                    .filter(|c| (c.size as f64 / tot.max(1) as f64) >= min_visible_ratio)
                    .take(4)
                    .collect();

                for (i, c) in visible_children.iter().enumerate() {
                    let ratio = (c.size as f64 / tot.max(1) as f64).clamp(0.0, 1.0);
                    top_sum += c.size;
                    items.push(BreakdownItem {
                        name: c.name.clone(),
                        size: c.size,
                        ratio,
                        color: colors[i % (colors.len() - 2)],
                        is_dir: c.is_dir,
                        idx: Some(c.idx),
                    });
                }

                if used > top_sum && used - top_sum > 1024 * 1024 {
                    let rem = used - top_sum;
                    let ratio = (rem as f64 / tot.max(1) as f64).clamp(0.0, 1.0);
                    items.push(BreakdownItem {
                        name: format!(
                            "其他已用 {} 项",
                            children.len().saturating_sub(visible_children.len())
                        ),
                        size: rem,
                        ratio,
                        color: colors[5], // Slate Gray
                        is_dir: false,
                        idx: None,
                    });
                }

                // 空闲可用空间条目（以翡翠绿清晰展现）
                if fre > 0 {
                    let free_ratio = (fre as f64 / tot.max(1) as f64).clamp(0.0, 1.0);
                    items.push(BreakdownItem {
                        name: "空闲可用空间".to_string(),
                        size: fre,
                        ratio: free_ratio,
                        color: colors[4], // 0x10b981 Emerald Green
                        is_dir: false,
                        idx: None,
                    });
                }

                (
                    cur_name,
                    fmt_size(tot),
                    format!("已用 {used_pct}% · 空闲 {}", fmt_size(fre)),
                    items,
                )
            } else {
                let total_children_size: u64 = children.iter().map(|c| c.size).sum();
                let base_size = total_children_size.max(cur_size).max(1);

                let mut items = Vec::new();
                let mut top_sum = 0u64;

                for (i, c) in children.iter().take(4).enumerate() {
                    let ratio = (c.size as f64 / base_size as f64).clamp(0.0, 1.0);
                    top_sum += c.size;
                    items.push(BreakdownItem {
                        name: if c.name.is_empty() {
                            format!("{}: 根目录", tree.volume())
                        } else {
                            c.name.clone()
                        },
                        size: c.size,
                        ratio,
                        color: colors[i % (colors.len() - 2)],
                        is_dir: c.is_dir,
                        idx: Some(c.idx),
                    });
                }

                if cur_size > top_sum && cur_size - top_sum > 1024 {
                    let rem = cur_size - top_sum;
                    let ratio = (rem as f64 / base_size as f64).clamp(0.0, 1.0);
                    items.push(BreakdownItem {
                        name: format!("其他 {} 项", children.len().saturating_sub(4)),
                        size: rem,
                        ratio,
                        color: colors[5],
                        is_dir: false,
                        idx: None,
                    });
                }

                (
                    cur_name,
                    fmt_size(cur_size),
                    format!("当前目录共 {} 个子项", children.len()),
                    items,
                )
            }
        }
        DiskTab::Files => {
            let files = tree.largest_files(200);
            let total_files_size: u64 = files.iter().map(|f| f.size).sum();
            let base_size = total_files_size.max(1);

            // 按后缀类型聚类
            let mut media_sz = 0u64;
            let mut bin_sz = 0u64;
            let mut arch_sz = 0u64;
            let mut doc_sz = 0u64;
            let mut other_sz = 0u64;

            for f in &files {
                let ext = std::path::Path::new(&f.name)
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("")
                    .to_ascii_lowercase();

                match ext.as_str() {
                    "mp4" | "mkv" | "avi" | "mov" | "flv" | "mp3" | "wav" => media_sz += f.size,
                    "exe" | "dll" | "sys" | "node" | "pak" | "bin" | "msi" => bin_sz += f.size,
                    "zip" | "rar" | "7z" | "tar" | "gz" | "iso" | "vmdk" | "vhdx" => {
                        arch_sz += f.size
                    }
                    "dat" | "db" | "log" | "txt" | "pdf" | "docx" | "xlsx" | "sqlite" => {
                        doc_sz += f.size
                    }
                    _ => other_sz += f.size,
                }
            }

            let cat_data = [
                ("媒体视频 (Media)", media_sz, colors[0]),
                ("程序/动态库 (Bin)", bin_sz, colors[1]),
                ("压缩镜像 (Archive)", arch_sz, colors[2]),
                ("文档数据 (Data)", doc_sz, colors[3]),
                ("其他文件 (Others)", other_sz, colors[5]),
            ];

            let items: Vec<BreakdownItem> = cat_data
                .into_iter()
                .filter(|(_, sz, _)| *sz > 0)
                .map(|(name, sz, col)| BreakdownItem {
                    name: name.to_string(),
                    size: sz,
                    ratio: (sz as f64 / base_size as f64).clamp(0.0, 1.0),
                    color: col,
                    is_dir: false,
                    idx: None,
                })
                .collect();

            (
                "全盘大文件分布".to_string(),
                fmt_size(total_files_size),
                format!("前 {} 个大文件汇总", files.len()),
                items,
            )
        }
    };

    let ring_segments: Vec<DonutSegment> = breakdown
        .iter()
        .map(|it| DonutSegment {
            ratio: it.ratio as f32,
            color: it.color,
        })
        .collect();

    // 动态多彩占比条
    let mut proportion_bar = div()
        .w_full()
        .h(px(10.))
        .rounded_full()
        .overflow_hidden()
        .bg(rgb(SURF_HIGH))
        .flex();

    for item in &breakdown {
        let pct = (item.ratio * 100.0) as f32;
        if pct > 0.5 {
            proportion_bar = proportion_bar.child(
                div()
                    .h_full()
                    .flex_none()
                    .w(gpui::relative(item.ratio as f32))
                    .bg(rgb(item.color)),
            );
        }
    }

    // 列表项渲染（支持点击下钻联动）
    let breakdown_rows = breakdown
        .iter()
        .enumerate()
        .map(|(i, item)| render_breakdown_row(item, i, cx));

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
        // 顶部盘符选择与总容量
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .gap_2()
                .child(div().flex().items_center().gap_2().children(volume_pills))
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
                                .child("空间占比分布")
                                .child(format!("Top {} 分类", breakdown.len())),
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
fn render_right_browser_pane(root: &Root, scan: &MftScan, cx: &mut Context<Root>) -> AnyElement {
    let tree = &scan.tree;
    let depth = root.disk_path.len();

    // 面包屑导航（单行自适应，防止小窗口折行挤压）
    let mut crumbs = div()
        .flex()
        .items_center()
        .gap_1()
        .flex_1()
        .min_w(px(0.))
        .overflow_hidden();

    let max_display_crumbs = 3;
    let path_len = root.disk_path.len();
    let display_indices: Vec<(usize, u32)> = if path_len <= max_display_crumbs {
        root.disk_path.iter().copied().enumerate().collect()
    } else {
        let mut list = vec![(0, root.disk_path[0])];
        for i in (path_len - 2)..path_len {
            list.push((i, root.disk_path[i]));
        }
        list
    };

    let mut had_ellipsis = false;
    for &(i, idx) in &display_indices {
        let is_root = idx == tree.root();
        let last = i + 1 == depth;
        let crumb_name = if is_root {
            format!("{}: 根目录", tree.volume())
        } else {
            truncate(&tree.name_of(idx), 12)
        };

        if path_len > max_display_crumbs && i > 0 && !had_ellipsis {
            had_ellipsis = true;
            crumbs = crumbs.child(
                div()
                    .px_1()
                    .text_xs()
                    .text_color(rgb(OUTLINE))
                    .child("… ›"),
            );
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
                    d.text_color(rgb(TEXT))
                        .font_weight(gpui::FontWeight::BOLD)
                })
                .when(!last, |d| {
                    d.text_color(rgb(PRIMARY)).hover(|h| h.bg(rgb(SURF_HIGH)))
                })
                .child(crumb_name)
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.disk_path.truncate(i + 1);
                    cx.notify();
                })),
        );
        if !last {
            crumbs = crumbs.child(
                div()
                    .text_xs()
                    .text_color(rgb(OUTLINE))
                    .child("›"),
            );
        }
    }

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
                .font_weight(if root.disk_tab == DiskTab::Tree {
                    gpui::FontWeight::BOLD
                } else {
                    gpui::FontWeight::NORMAL
                })
                .cursor_pointer()
                .when(root.disk_tab == DiskTab::Tree, |d| {
                    d.bg(rgb(CARD))
                        .text_color(rgb(PRIMARY))
                        .shadow_sm()
                })
                .when(root.disk_tab != DiskTab::Tree, |d| {
                    d.text_color(rgb(MUTED)).hover(|h| h.bg(rgb(SURF_HIGH)))
                })
                .child("目录树")
                .on_click(cx.listener(|this, _, _, cx| {
                    this.disk_tab = DiskTab::Tree;
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
                .font_weight(if root.disk_tab == DiskTab::Files {
                    gpui::FontWeight::BOLD
                } else {
                    gpui::FontWeight::NORMAL
                })
                .cursor_pointer()
                .when(root.disk_tab == DiskTab::Files, |d| {
                    d.bg(rgb(CARD))
                        .text_color(rgb(PRIMARY))
                        .shadow_sm()
                })
                .when(root.disk_tab != DiskTab::Files, |d| {
                    d.text_color(rgb(MUTED)).hover(|h| h.bg(rgb(SURF_HIGH)))
                })
                .child("全盘大文件")
                .on_click(cx.listener(|this, _, _, cx| {
                    this.disk_tab = DiskTab::Files;
                    cx.notify();
                })),
        );

    // 跨目录已选状态提示与一键清空标签（紧凑胶囊设计）
    let total_selected_count = root.disk_selected_count();
    let total_selected_size = root.disk_selected_size();
    let cross_folder_badge = if total_selected_count > 0 {
        Some(
            div()
                .id("cross-folder-clear-btn")
                .px_2()
                .py(px(3.))
                .rounded_full()
                .bg(rgb(ERROR_CONTAINER))
                .text_xs()
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(rgb(ERROR))
                .flex()
                .items_center()
                .gap_1()
                .cursor_pointer()
                .flex_none()
                .hover(|h| h.bg(rgba(ERROR, 0.2)))
                .child(format!("已选 {total_selected_count} 项 ({})", fmt_size(total_selected_size)))
                .child(
                    div()
                        .font_weight(gpui::FontWeight::BOLD)
                        .child("✕"),
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.clear_disk_selection();
                    cx.notify();
                })),
        )
    } else {
        None
    };

    let top_bar = div()
        .px_4()
        .py_2()
        .min_h(px(46.))
        .flex()
        .items_center()
        .justify_between()
        .gap_3()
        .border_b_1()
        .border_color(rgba(OUTLINE_VAR, 0.4))
        .bg(rgb(SURF_LOW))
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .flex()
                .items_center()
                .gap_2()
                .overflow_hidden()
                .child(
                    div()
                        .id("disk-up-btn")
                        .flex_none()
                        .child(small_button(
                            String::from("← 上级"),
                            SURF_HIGH,
                            TEXT,
                            depth > 1,
                        ))
                        .on_click(cx.listener(|this, _, _, cx| {
                            if this.disk_path.len() > 1 {
                                this.disk_path.pop();
                                cx.notify();
                            }
                        })),
                )
                .child(crumbs),
        )
        .child(
            div()
                .flex_none()
                .flex()
                .items_center()
                .gap_2()
                .children(cross_folder_badge)
                .child(tab_switch),
        );

    // 当前视图可勾选项与行元素。行数据（含完整路径、保护状态）由
    // Root::refresh_render_caches 按「盘符 + 目录 + 标签页 + 树版本」缓存，
    // 目录没变时整帧不做任何路径解析。
    let selectable_items: Vec<(std::path::PathBuf, u64)> = root.disk_selectable();
    let drillable = root.disk_tab == DiskTab::Tree;
    let rows: Vec<AnyElement> = if root.disk_rows.is_empty() {
        let hint = if drillable {
            "此目录为空"
        } else {
            "未找到大文件"
        };
        vec![div()
            .p_8()
            .flex()
            .items_center()
            .justify_center()
            .text_sm()
            .text_color(rgb(OUTLINE))
            .child(hint)
            .into_any_element()]
    } else {
        root.disk_rows
            .iter()
            .map(|row| render_lens_row(root, tree, row, drillable, cx))
            .collect()
    };

    // 计算当前视图全选勾选状态（考虑父级勾选继承与子级反选）
    let total_selectable = selectable_items.len();
    let selected_in_view = selectable_items
        .iter()
        .filter(|(p, _)| root.is_disk_item_selected(p))
        .count();

    let header_check_state = Check::from_counts(selected_in_view, total_selectable);

    // 表头（包含全选复选框）
    let list_header = div()
        .px_4()
        .py_2()
        .flex()
        .items_center()
        .gap_3()
        .bg(rgb(SURF))
        .border_b_1()
        .border_color(rgba(OUTLINE_VAR, 0.3))
        .child(
            div()
                .id("disk-select-all-header")
                .w(px(20.))
                .h(px(20.))
                .flex_none()
                .cursor_pointer()
                .child(checkbox(header_check_state))
                .on_click(cx.listener({
                    let items = selectable_items.clone();
                    let all_on = header_check_state == Check::On;
                    move |this, _, _, cx| {
                        for (p, sz) in &items {
                            this.disk_sel.set(p, *sz, !all_on);
                        }
                        cx.notify();
                    }
                })),
        )
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .text_xs()
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(rgb(OUTLINE))
                .child("名称"),
        )
        .child(
            div()
                .w(px(90.))
                .flex_none()
                .text_right()
                .text_xs()
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(rgb(OUTLINE))
                .child("大小"),
        )
        .child(
            div()
                .w(px(40.))
                .flex_none()
                .text_center()
                .text_xs()
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(rgb(OUTLINE))
                .child("操作"),
        );

    card()
        .flex_1()
        .min_w(px(0.))
        .overflow_hidden()
        .flex()
        .flex_col()
        .child(top_bar)
        .child(list_header)
        .child(
            div()
                .id("disk-browser-rows")
                .flex_1()
                .overflow_scroll()
                .flex()
                .flex_col()
                .children(rows),
        )
        .into_any_element()
}

/// 渲染单行文件 / 文件夹（支持勾选、下钻与安全删除）
fn render_lens_row(
    root: &Root,
    tree: &MftTree,
    row: &DiskRow,
    drillable: bool,
    cx: &mut Context<Root>,
) -> AnyElement {
    let n = &row.node;
    let idx = n.idx;
    let is_dir = n.is_dir;
    let path = row.path.clone();
    let path_str = path.to_string_lossy().to_string();
    let protected = row.protected;
    let is_selected = root.is_disk_item_selected(&path);

    let display_name = if drillable {
        if n.name.is_empty() {
            format!("{}: 根目录", tree.volume())
        } else {
            n.name.clone()
        }
    } else {
        path_str.clone()
    };

    let p_for_cb = path.clone();
    let p_for_del = path.clone();
    let size = n.size;

    div()
        .id(SharedString::from(format!("lens-row-{idx}")))
        .flex()
        .items_center()
        .gap_3()
        .px_4()
        .py_2()
        .border_b_1()
        .border_color(rgba(OUTLINE_VAR, 0.25))
        .hover(|h| h.bg(rgb(SURF_LOW)))
        // 勾选框（受保护的系统核心文件禁用勾选）
        .child(
            div()
                .id(SharedString::from(format!("cb-lens-{idx}")))
                .w(px(20.))
                .flex_none()
                .cursor_pointer()
                .when(protected, |d| d.opacity(0.3).cursor_not_allowed())
                .child(checkbox(if is_selected {
                    Check::On
                } else {
                    Check::Off
                }))
                .when(!protected, |d| {
                    d.on_click(cx.listener(move |this, _, _, cx| {
                        this.toggle_disk_item(&p_for_cb, size);
                        cx.notify();
                    }))
                }),
        )
        // 图标与名称（点击目录可直接下钻）
        .child(
            div()
                .id(SharedString::from(format!("drill-{idx}")))
                .flex_1()
                .min_w(px(0.))
                .flex()
                .items_center()
                .gap_3()
                .when(drillable && is_dir, |d| d.cursor_pointer())
                .child(
                    div()
                        .w(px(28.))
                        .h(px(28.))
                        .flex_none()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(if is_dir {
                            icon_badge(icon_dashboard(PRIMARY, 14.), PRIMARY_FIXED, PRIMARY, 28.)
                        } else if size >= 1024 * 1024 * 1024 {
                            icon_badge(icon_trash(ERROR, 14.), ERROR_CONTAINER, ERROR, 28.)
                        } else {
                            icon_badge(icon_disk(PRIMARY, 14.), PRIMARY_FIXED, PRIMARY, 28.)
                        }),
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
                                .font_weight(if is_dir {
                                    gpui::FontWeight::SEMIBOLD
                                } else {
                                    gpui::FontWeight::NORMAL
                                })
                                .text_color(rgb(TEXT))
                                .child(truncate(&display_name, 56)),
                        )
                        .when(protected, |d| {
                            d.child(
                                div()
                                    .text_xs()
                                    .text_color(rgb(OUTLINE))
                                    .child("系统保护项目"),
                            )
                        }),
                )
                .when(drillable && is_dir, |d| {
                    d.on_click(cx.listener(move |this, _, _, cx| {
                        this.disk_path.push(idx);
                        cx.notify();
                    }))
                }),
        )
        // 大小
        .child(
            div()
                .w(px(90.))
                .flex_none()
                .text_right()
                .text_sm()
                .font_weight(gpui::FontWeight::BOLD)
                .text_color(if size >= 1024 * 1024 * 1024 {
                    rgb(PRIMARY)
                } else {
                    rgb(TEXT)
                })
                .child(fmt_size(size)),
        )
        // 进入或删除按钮
        .child(
            div()
                .w(px(40.))
                .flex_none()
                .flex()
                .justify_center()
                .when(drillable && is_dir, |d| {
                    d.child(
                        div()
                            .id(SharedString::from(format!("enter-dir-{idx}")))
                            .px_2()
                            .py(px(2.))
                            .rounded_md()
                            .text_xs()
                            .font_weight(gpui::FontWeight::BOLD)
                            .text_color(rgb(PRIMARY))
                            .cursor_pointer()
                            .hover(|h| h.bg(rgb(SURF_HIGH)))
                            .child("›")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.disk_path.push(idx);
                                cx.notify();
                            })),
                    )
                })
                .when(!is_dir && !protected, |d| {
                    d.child(
                        div()
                            .id(SharedString::from(format!("del-file-{idx}")))
                            .px_2()
                            .py(px(2.))
                            .rounded_md()
                            .text_xs()
                            .text_color(rgb(ERROR))
                            .cursor_pointer()
                            .hover(|h| h.bg(rgb(ERROR_CONTAINER)))
                            .child("删除")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.request_clean_path(p_for_del.clone(), size, cx);
                            })),
                    )
                }),
        )
        .into_any_element()
}

/// 底部悬浮批量清理 Action Bar
pub fn render_disk_clean_bar(root: &Root, cx: &mut Context<Root>) -> Option<AnyElement> {
    if root.disk_selected_count() == 0 {
        return None;
    }

    let count = root.disk_selected_count();
    let size = root.disk_selected_size();

    Some(
        div()
            .flex_none()
            .w_full()
            .px_8()
            .py_2()
            .bg(rgb(BG))
            .border_t_1()
            .border_color(rgba(OUTLINE_VAR, 0.5))
            .flex()
            .justify_center()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_6()
                    .pl_6()
                    .pr(px(6.))
                    .py(px(6.))
                    .rounded_full()
                    .bg(rgb(CARD))
                    .border_1()
                    .border_color(rgba(OUTLINE_VAR, 0.6))
                    .shadow_xl()
                    .child(
                        div()
                            .w(px(36.))
                            .h(px(36.))
                            .rounded_full()
                            .bg(rgb(ERROR_CONTAINER))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(icon_trash(ERROR, 16.)),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .text_xs()
                                    .font_weight(gpui::FontWeight::MEDIUM)
                                    .text_color(rgb(OUTLINE))
                                    .child("已选择要清理的磁盘项目"),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .font_weight(gpui::FontWeight::BOLD)
                                    .text_color(rgb(TEXT))
                                    .child(format!("{count} 项已选中")),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .items_end()
                            .child(
                                div()
                                    .text_base()
                                    .font_weight(gpui::FontWeight::BOLD)
                                    .text_color(rgb(ERROR))
                                    .child(fmt_size(size)),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(rgb(OUTLINE))
                                    .child("待彻底释放"),
                            ),
                    )
                    .child(
                        div()
                            .id("clear-disk-selection-btn")
                            .child(small_button(
                                String::from("清空选择"),
                                SURF_HIGH,
                                TEXT,
                                true,
                            ))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.clear_disk_selection();
                                cx.notify();
                            })),
                    )
                    .child(
                        div()
                            .id("clean-disk-selected")
                            .child(crate::ui::components::buttons::danger_button(
                                String::from("永久删除选中项"),
                                !root.cleaning,
                            ))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.request_clean_disk_selected(cx);
                            })),
                    ),
            )
            .into_any_element(),
    )
}
