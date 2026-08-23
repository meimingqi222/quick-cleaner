//! 磁盘透镜右侧面板（浏览器 + 行渲染 + 清理栏）

use crate::core::disk::{ScanResult, SizeTree};
use crate::core::i18n::Language;
use crate::core::model::{fmt_size, truncate, Check};
use crate::ui::components::buttons::small_button;
use crate::ui::components::cards::card;
use crate::ui::components::controls::checkbox;
use crate::ui::components::icons::*;
use crate::ui::components::path_tooltip;
use crate::ui::i18n::*;
use crate::ui::theme::*;
use crate::ui::views::DiskTab;
use crate::ui::{DiskRow, Root};
use gpui::{div, prelude::*, px, rgb, AnyElement, Context, IntoElement, SharedString};

use super::disk_left::{render_breadcrumbs, render_tab_switch};

/// 跨目录已选状态的胶囊提示，带一键清空。
///
/// 勾选是跨目录累计的，用户下钻几层之后很容易忘了别的目录里还留着选中项，
/// 这个提示就是防止「确认删除」时删出预期之外的东西。
fn render_cross_folder_badge(root: &Root, cx: &mut Context<Root>) -> Option<AnyElement> {
    let lang = root.language;

    // 跨目录已选状态提示与一键清空标签（紧凑胶囊设计）
    let total_selected_count = root.disk_selected_count();
    let total_selected_size = root.disk_selected_size();
    let cross_folder_badge = if total_selected_count > 0 {
        let badge_text = match lang {
            Language::Zh => format!(
                "已选 {total_selected_count} 项 ({})",
                fmt_size(total_selected_size)
            ),
            Language::En => format!(
                "{total_selected_count} items ({})",
                fmt_size(total_selected_size)
            ),
        };
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
                .child(badge_text)
                .child(div().font_weight(gpui::FontWeight::BOLD).child("✕"))
                .on_click(cx.listener(|this, _, _, cx| {
                    this.clear_disk_selection();
                    cx.notify();
                }))
                .into_any_element(),
        )
    } else {
        None
    };

    cross_folder_badge
}

pub(super) fn render_right_browser_pane(
    root: &Root,
    scan: &ScanResult,
    cx: &mut Context<Root>,
) -> AnyElement {
    let lang = root.language;
    let tree = &scan.tree;
    let depth = root.disk.path.len();

    let crumbs = render_breadcrumbs(root, scan, cx);

    let tab_switch = render_tab_switch(root, cx);

    let cross_folder_badge = render_cross_folder_badge(root, cx);

    let btn_parent_text = match lang {
        Language::Zh => "← 上级",
        Language::En => "← Up",
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
                            btn_parent_text.to_string(),
                            SURF_HIGH,
                            TEXT,
                            depth > 1,
                        ))
                        .on_click(cx.listener(|this, _, _, cx| {
                            if this.disk.path.len() > 1 {
                                this.disk.path.pop();
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
    let drillable = root.disk.tab == DiskTab::Tree;
    let rows: Vec<AnyElement> = if root.disk.rows.is_empty() {
        let hint = match (drillable, lang) {
            (true, Language::Zh) => "此目录为空",
            (true, Language::En) => "Directory is empty",
            (false, Language::Zh) => "未找到大文件",
            (false, Language::En) => "No large files found",
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
        root.disk
            .rows
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
                            this.disk.sel.set(p, *sz, !all_on);
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
                .child(if lang == Language::Zh {
                    "名称"
                } else {
                    "Name"
                }),
        )
        .child(
            div()
                .w(px(90.))
                .flex_none()
                .text_right()
                .text_xs()
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(rgb(OUTLINE))
                .child(if lang == Language::Zh {
                    "大小"
                } else {
                    "Size"
                }),
        )
        .child(
            div()
                .w(px(40.))
                .flex_none()
                .text_center()
                .text_xs()
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(rgb(OUTLINE))
                .child(if lang == Language::Zh {
                    "操作"
                } else {
                    "Action"
                }),
        );

    // 当前位置完整路径：面包屑折叠时它是唯一可靠的「我在哪」线索。
    // 只在目录树视图显示（大文件榜跨目录，没有单一位置）。超宽不换行、
    // hover 出全文，右侧一键复制。
    let path_bar = drillable.then(|| {
        let full = root
            .current_disk_full_path()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();
        let (copy_label, copied_status) = match lang {
            Language::Zh => ("复制路径", crate::core::i18n::Text::new("路径已复制", "Path copied")),
            Language::En => ("Copy path", crate::core::i18n::Text::new("路径已复制", "Path copied")),
        };
        let full_for_copy = full.clone();
        div()
            .id("disk-full-path-bar")
            .px_4()
            .py(px(4.))
            .flex()
            .items_center()
            .gap_2()
            .border_b_1()
            .border_color(rgba(OUTLINE_VAR, 0.3))
            .bg(rgb(SURF))
            .child(
                div()
                    .id("disk-full-path")
                    .flex_1()
                    .min_w(px(0.))
                    .overflow_hidden()
                    .text_xs()
                    .text_color(rgb(OUTLINE))
                    .whitespace_nowrap()
                    .child(full)
                    .tooltip(path_tooltip(&full_for_copy)),
            )
            .child(
                div()
                    .id("disk-copy-path-btn")
                    .flex_none()
                    .child(small_button(copy_label.to_string(), SURF_HIGH, TEXT, true))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        cx.write_to_clipboard(gpui::ClipboardItem::new_string(
                            full_for_copy.clone(),
                        ));
                        this.status = copied_status.clone();
                        cx.notify();
                    })),
            )
    });

    card()
        .flex_1()
        .min_w(px(0.))
        .overflow_hidden()
        .flex()
        .flex_col()
        .child(top_bar)
        .children(path_bar)
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
/// 行内的图标 + 名称列。目录可以直接点进去下钻。
fn render_lens_row_name(
    root: &Root,
    tree: &SizeTree,
    row: &DiskRow,
    drillable: bool,
    cx: &mut Context<Root>,
) -> AnyElement {
    let lang = root.language;
    let n = &row.node;
    let idx = n.idx;
    let is_dir = n.is_dir;
    let size = n.size;
    let path_str = row.path.to_string_lossy().to_string();
    let protected = row.protected;

    // 目录树视图显示节点名（根节点没有名字，用盘符代替）；
    // 大文件榜跨目录，只有完整路径才说得清是哪个文件。
    let display_name = if drillable {
        if n.name.is_empty() {
            match lang {
                Language::Zh => format!("{}: 根目录", tree.volume()),
                Language::En => format!("{}: Root", tree.volume()),
            }
        } else {
            n.name.clone()
        }
    } else {
        path_str.clone()
    };

    let protected_label = match lang {
        Language::Zh => "系统保护项目",
        Language::En => "System Protected",
    };

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
                            .child(protected_label),
                    )
                }),
        )
        .when(drillable && is_dir, |d| {
            d.on_click(cx.listener(move |this, _, _, cx| {
                this.enter_disk_node(idx, cx);
            }))
        })
        // 树视图行只显示名字；hover 出完整路径，深层级下不再猜「这是
        // 哪个 Library / 哪个 JetBrains」。
        .tooltip(path_tooltip(&path_str))
        .into_any_element()
}

/// 行尾的操作列：目录给「进入」，文件给「删除」；受保护项两者都不给。
fn render_lens_row_actions(
    root: &Root,
    row: &DiskRow,
    drillable: bool,
    cx: &mut Context<Root>,
) -> AnyElement {
    let lang = root.language;
    let n = &row.node;
    let idx = n.idx;
    let is_dir = n.is_dir;
    let size = n.size;
    let p_for_del = row.path.clone();
    let protected = row.protected;
    let delete_label = match lang {
        Language::Zh => "删除",
        Language::En => "Delete",
    };

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
                        this.enter_disk_node(idx, cx);
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
                    .child(delete_label)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.request_clean_path(p_for_del.clone(), size, cx);
                    })),
            )
        })
        .into_any_element()
}

fn render_lens_row(
    root: &Root,
    tree: &SizeTree,
    row: &DiskRow,
    drillable: bool,
    cx: &mut Context<Root>,
) -> AnyElement {
    let n = &row.node;
    let idx = n.idx;
    let path = row.path.clone();
    let protected = row.protected;
    let is_selected = root.is_disk_item_selected(&path);
    let p_for_cb = path.clone();
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
                .child(checkbox(if is_selected { Check::On } else { Check::Off }))
                .when(!protected, |d| {
                    d.on_click(cx.listener(move |this, _, _, cx| {
                        this.toggle_disk_item(&p_for_cb, size);
                        cx.notify();
                    }))
                }),
        )
        // 图标与名称（点击目录可直接下钻）
        .child(render_lens_row_name(root, tree, row, drillable, cx))
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
        .child(render_lens_row_actions(root, row, drillable, cx))
        .into_any_element()
}

/// 底部悬浮批量清理 Action Bar
pub fn render_disk_clean_bar(root: &Root, cx: &mut Context<Root>) -> Option<AnyElement> {
    if root.disk_selected_count() == 0 {
        return None;
    }

    let lang = root.language;
    let count = root.disk_selected_count();
    let size = root.disk_selected_size();

    let items_count_label = match lang {
        Language::Zh => format!("{count} 项已选中"),
        Language::En => format!("{count} items selected"),
    };

    let to_recycle = root.settings.delete_to_recycle_bin;

    // 处置方式开关。放在确认按钮旁边，按下去之前就能看清是永久删除还是
    // 送回收站——这是这条工具栏上唯一不可撤销的决定。
    let recycle_toggle = div()
        .id("toggle-recycle-bin")
        .flex()
        .items_center()
        .gap_2()
        .cursor_pointer()
        .child(checkbox(if to_recycle { Check::On } else { Check::Off }))
        .child(
            div()
                .text_xs()
                .text_color(rgb(if to_recycle { TEXT } else { OUTLINE }))
                .child(tr_recycle_toggle(lang)),
        )
        .on_click(cx.listener(|this, _, _, cx| {
            this.toggle_recycle_bin(cx);
        }));

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
                                    .child(if lang == Language::Zh {
                                        "已选择要清理的磁盘项目"
                                    } else {
                                        "Selected Disk Items"
                                    }),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .font_weight(gpui::FontWeight::BOLD)
                                    .text_color(rgb(TEXT))
                                    .child(items_count_label),
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
                                    .child(if to_recycle {
                                        tr_to_be_recycled(lang)
                                    } else {
                                        tr_to_be_freed(lang)
                                    }),
                            ),
                    )
                    .child(recycle_toggle)
                    .child(
                        div()
                            .id("clear-disk-selection-btn")
                            .child(small_button(
                                tr_btn_clear_sel(lang).to_string(),
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
                                tr_btn_confirm_delete(lang).to_string(),
                                !root.clean.running,
                            ))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.request_clean_disk_selected(cx);
                            })),
                    ),
            )
            .into_any_element(),
    )
}
