//! 开发环境与包管理视图实现（左右分栏 Master-Detail 架构）
//!
//! 采用桌面端专业分栏架构：
//! - 左侧（Master）：环境与运行时清单（Conda、Python 解释器/虚拟环境、Node 全局包、CLI 工具），
//!   紧凑高效，一屏纵览全局生态；
//! - 右侧（Detail）：当前环境档案与全高度沉浸式包列表，独占垂直视口，彻底杜绝嵌套滚动与高度受限；
//! - 底部（Bottom Bar）：跨生态多选实时汇总与一键批量安全移除。

use crate::core::dev_env::{DevAssetItem, DevAssetKind, PythonToolKind, SiteScope, VenvManager};
use crate::core::i18n::Language;
use crate::core::model::{fmt_size, Check};
use crate::ui::components::buttons::{danger_button, small_button};
use crate::ui::components::controls::{checkbox, loading_state_view};
use crate::ui::components::search_box::{search_box, SearchBoxSpec};
use crate::ui::components::{path_tooltip, text_tooltip};
use crate::ui::i18n::*;
use crate::ui::state::{DevEnvFilterTab, DevEnvState};
use crate::ui::theme::*;
use crate::ui::Root;
use gpui::{div, prelude::*, px, rgb, AnyElement, Context, SharedString, Window};
use std::time::SystemTime;

pub fn render_dev_environments_view(
    root: &Root,
    window: &mut Window,
    cx: &mut Context<Root>,
) -> AnyElement {
    let lang = root.language;
    let dev = &root.apps.dev;

    let conda_count = dev.conda_envs.len() + dev.python_envs.len();
    let node_count = dev.node_packages.len();
    let tools_count = dev.python_tools.len();
    let total_count = conda_count + node_count + tools_count;

    let busy = dev.scanning || dev.removing.is_some();
    let search_query = root.apps.input.text.trim().to_lowercase();

    // 搜索输入框规格
    let search_focused = root.apps.input.focus_handle.is_focused(window);
    let search_raw = root.apps.input.text.clone();
    let apps_focus_handle = root.apps.input.focus_handle.clone();
    let search_sel = root.apps.input.sel.clone();
    let search_marked = root.apps.input.marked.clone();
    let sel = crate::ui::text_input::clamp_to_boundary(&search_raw, search_sel);

    let search_bar = search_box(
        SearchBoxSpec {
            id: SharedString::from("dev-search-box"),
            focus_handle: &apps_focus_handle,
            text: &search_raw,
            placeholder: SharedString::from(tr_dev_search_packages_placeholder(lang)),
            selection: sel,
            marked: search_marked,
            width: 336.,
            height: 32.,
            font_size: 12.,
            cursor_h: 13.,
            focused: search_focused,
            cursor_visible: root.cursor_blink_visible,
            is_file_search: false,
        },
        |this, cx| {
            this.apps.input.text.clear();
            this.apps.input.sel = 0..0;
            cx.notify();
        },
        |this, cx| {
            this.text_input_delete(false, cx);
        },
        |this, cx| {
            this.apps.input.text.clear();
            this.apps.input.sel = 0..0;
            cx.notify();
        },
        |this, bounds| {
            this.apps.input.bounds = Some(bounds);
        },
        cx,
    );

    // 顶部紧凑概览行（自适应弹性布局：左侧标题与说明两行收拢，右侧指标胶囊与操作按钮固定保护）
    let header_row = div()
        .w_full()
        .flex()
        .items_center()
        .justify_between()
        .gap_4()
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .flex()
                .flex_col()
                .justify_center()
                .gap_0p5()
                .child(
                    div()
                        .text_sm()
                        .font_weight(gpui::FontWeight::BOLD)
                        .text_color(rgb(TEXT))
                        .child(tr_dev_overview_title(lang)),
                )
                .child(
                    div()
                        .id("dev-overview-desc")
                        .text_xs()
                        .text_color(rgb(OUTLINE))
                        .whitespace_nowrap()
                        .overflow_hidden()
                        .text_ellipsis()
                        .tooltip(text_tooltip(tr_dev_overview_desc(lang)))
                        .child(tr_dev_overview_desc(lang)),
                ),
        )
        .child(
            div()
                .flex_none()
                .flex()
                .items_center()
                .gap_2()
                .child(metric_pill(
                    tr_dev_total_assets(lang),
                    total_count.to_string(),
                    TEXT,
                ))
                .child(metric_pill(
                    tr_dev_total_exclusive(lang),
                    fmt_size(dev.total_exclusive_size),
                    PRIMARY,
                ))
                .child(metric_pill(
                    tr_dev_total_logical(lang),
                    fmt_size(dev.total_logical_size),
                    MUTED,
                ))
                .child(
                    div()
                        .id("dev-rescan-btn")
                        .flex_none()
                        .child(small_button(
                            tr_dev_btn_rescan(lang).to_string(),
                            SURF_HIGH,
                            TEXT,
                            !busy,
                        ))
                        .when(!busy, |d| {
                            d.on_click(cx.listener(|this, _, _, cx| {
                                this.start_dev_env_scan(cx);
                            }))
                        }),
                ),
        );

    // 若正处于全量扫描中，展示优雅加载态
    if dev.scanning {
        return div()
            .id("dev-envs-root")
            .flex_1()
            .min_h(px(0.))
            .flex()
            .flex_col()
            .gap_3()
            .child(header_row)
            .child(
                div()
                    .flex_1()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(loading_state_view(
                        tr_dev_overview_title(lang),
                        tr_dev_overview_desc(lang),
                        root.anim_phase,
                    )),
            )
            .into_any_element();
    }

    // 左侧 Master 列表与过滤
    let left_master_panel = render_master_panel(
        dev,
        &search_query,
        search_bar.into_any_element(),
        total_count,
        conda_count,
        node_count,
        tools_count,
        lang,
        busy,
        cx,
    );

    // 右侧 Detail 详情与包管理面板
    let right_detail_panel =
        render_detail_panel(dev, &search_query, root.anim_phase, lang, busy, cx);

    // 主分栏工作区
    let split_workspace = div()
        .flex_1()
        .min_h(px(0.))
        .flex()
        .flex_row()
        .gap_3()
        .child(left_master_panel)
        .child(right_detail_panel);

    // 底部悬浮批量操作栏
    let bottom_bar = render_dev_bottom_bar(dev, lang, busy, cx);

    div()
        .id("dev-envs-root")
        .flex_1()
        .min_h(px(0.))
        .flex()
        .flex_col()
        .gap_3()
        .child(header_row)
        .child(split_workspace)
        .child(bottom_bar)
        .into_any_element()
}

/// 顶部小胶囊统计
fn metric_pill(label: &'static str, value: String, value_color: u32) -> AnyElement {
    div()
        .flex_none()
        .flex()
        .items_center()
        .gap_1p5()
        .px_2p5()
        .py(px(3.))
        .rounded_full()
        .bg(rgb(SURF_HIGH))
        .border_1()
        .border_color(rgba(OUTLINE_VAR, 0.4))
        .child(div().text_xs().text_color(rgb(OUTLINE)).child(label))
        .child(
            div()
                .text_xs()
                .font_weight(gpui::FontWeight::BOLD)
                .text_color(rgb(value_color))
                .child(value),
        )
        .into_any_element()
}

/// 左侧 Master 列表面板
#[allow(clippy::too_many_arguments)]
fn render_master_panel(
    dev: &DevEnvState,
    search_query: &str,
    search_bar: AnyElement,
    total_count: usize,
    conda_count: usize,
    node_count: usize,
    tools_count: usize,
    lang: Language,
    busy: bool,
    cx: &mut Context<Root>,
) -> AnyElement {
    let tabs = [
        (
            DevEnvFilterTab::All,
            format!("{} ({})", tr_dev_filter_all(lang), total_count),
        ),
        (
            DevEnvFilterTab::PythonConda,
            format!("{} ({})", tr_dev_filter_python(lang), conda_count),
        ),
        (
            DevEnvFilterTab::Node,
            format!("{} ({})", tr_dev_filter_node(lang), node_count),
        ),
        (
            DevEnvFilterTab::Tools,
            format!("{} ({})", tr_dev_filter_tools(lang), tools_count),
        ),
    ];

    let filter_tab_bar =
        div()
            .flex()
            .items_center()
            .flex_wrap()
            .gap_1()
            .children(tabs.into_iter().map(|(tab, label)| {
                let active = dev.filter_tab == tab;
                div()
                    .id(SharedString::from(format!("tab-filter-{tab:?}")))
                    .px_2()
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
                        d.bg(rgb(CARD))
                            .border_color(rgba(OUTLINE_VAR, 0.6))
                            .text_color(rgb(MUTED))
                            .hover(|h| h.bg(rgb(SURF_LOW)))
                    })
                    .child(label)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.apps.dev.filter_tab = tab;
                        cx.notify();
                    }))
            }));

    // 收集匹配过滤与搜索的条目
    let show_python = matches!(
        dev.filter_tab,
        DevEnvFilterTab::All | DevEnvFilterTab::PythonConda
    );
    let show_node = matches!(dev.filter_tab, DevEnvFilterTab::All | DevEnvFilterTab::Node);
    let show_tools = matches!(
        dev.filter_tab,
        DevEnvFilterTab::All | DevEnvFilterTab::Tools
    );

    let mut master_rows = div()
        .id("dev-master-scroll")
        .flex_1()
        .min_h(px(0.))
        .overflow_scroll()
        .p_2()
        .flex()
        .flex_col()
        .gap_1();

    let mut item_count = 0usize;

    if show_python {
        let mut py_items: Vec<&DevAssetItem> = Vec::new();
        py_items.extend(dev.conda_envs.iter());
        py_items.extend(dev.python_envs.iter());
        let filtered: Vec<&DevAssetItem> = py_items
            .into_iter()
            .filter(|i| match_asset(i, dev, search_query))
            .collect();
        if !filtered.is_empty() {
            master_rows = master_rows.child(section_sub_header(
                tr_dev_section_conda(lang),
                filtered.len(),
            ));
            for item in filtered {
                item_count += 1;
                let active = dev.active_env_id.as_deref() == Some(&item.id);
                let checked = dev.is_selected(&item.id);
                master_rows =
                    master_rows.child(render_master_row(item, active, checked, lang, busy, cx));
            }
        }
    }

    if show_node {
        let filtered: Vec<&DevAssetItem> = dev
            .node_packages
            .iter()
            .filter(|i| match_asset(i, dev, search_query))
            .collect();
        if !filtered.is_empty() {
            master_rows = master_rows.child(section_sub_header(
                tr_dev_section_node(lang),
                filtered.len(),
            ));
            for item in filtered {
                item_count += 1;
                let active = dev.active_env_id.as_deref() == Some(&item.id);
                let checked = dev.is_selected(&item.id);
                master_rows =
                    master_rows.child(render_master_row(item, active, checked, lang, busy, cx));
            }
        }
    }

    if show_tools {
        let filtered: Vec<&DevAssetItem> = dev
            .python_tools
            .iter()
            .filter(|i| match_asset(i, dev, search_query))
            .collect();
        if !filtered.is_empty() {
            master_rows = master_rows.child(section_sub_header(
                tr_dev_section_tools(lang),
                filtered.len(),
            ));
            for item in filtered {
                item_count += 1;
                let active = dev.active_env_id.as_deref() == Some(&item.id);
                let checked = dev.is_selected(&item.id);
                master_rows =
                    master_rows.child(render_master_row(item, active, checked, lang, busy, cx));
            }
        }
    }

    if item_count == 0 {
        master_rows = master_rows.child(
            div()
                .p_6()
                .flex()
                .items_center()
                .justify_center()
                .text_xs()
                .text_color(rgb(OUTLINE))
                .child(tr_dev_empty(lang)),
        );
    }

    div()
        .w(px(360.))
        .flex_none()
        .flex()
        .flex_col()
        .rounded_xl()
        .bg(rgb(CARD))
        .border_1()
        .border_color(rgba(OUTLINE_VAR, 0.5))
        .overflow_hidden()
        .child(
            div()
                .p_3()
                .border_b_1()
                .border_color(rgba(OUTLINE_VAR, 0.4))
                .flex()
                .flex_col()
                .gap_2()
                .child(filter_tab_bar)
                .child(search_bar),
        )
        .child(master_rows)
        .into_any_element()
}

/// 左侧列表中的分类分割小标题
fn section_sub_header(title: &'static str, count: usize) -> AnyElement {
    div()
        .px_2()
        .pt_2()
        .pb_1()
        .flex()
        .items_center()
        .justify_between()
        .text_xs()
        .font_weight(gpui::FontWeight::BOLD)
        .text_color(rgb(MUTED))
        .child(format!("{title} ({count})"))
        .into_any_element()
}

/// 匹配条目（环境自身名字、路径、或内部包含的包名）
fn match_asset(item: &DevAssetItem, dev: &DevEnvState, query: &str) -> bool {
    if query.is_empty() {
        return true;
    }
    if item.display_name().to_lowercase().contains(query) {
        return true;
    }
    if item.path.to_string_lossy().to_lowercase().contains(query) {
        return true;
    }
    if let Some(packages) = dev.site_packages.get(&item.id) {
        if packages
            .iter()
            .any(|p| p.display_name().to_lowercase().contains(query))
        {
            return true;
        }
    }
    false
}

/// 左侧 Master 单行环境组件（高度紧凑，约 52px）
fn render_master_row(
    item: &DevAssetItem,
    active: bool,
    checked: bool,
    lang: Language,
    busy: bool,
    cx: &mut Context<Root>,
) -> AnyElement {
    let can_remove = crate::core::dev_env::remove::can_remove(item);
    let item_id = item.id.clone();
    let select_id = item.id.clone();

    let cb = if can_remove {
        div()
            .id(gpui::ElementId::Name(
                format!("cb-master-{}", item.id).into(),
            ))
            .flex_none()
            .child(checkbox(if checked { Check::On } else { Check::Off }))
            .when(!busy, |d| {
                d.on_click(cx.listener(move |this, _, _, cx| {
                    this.apps.dev.toggle_select(&item_id);
                    cx.notify();
                }))
            })
    } else {
        div()
            .id(gpui::ElementId::Name(
                format!("cb-master-ph-{}", item.id).into(),
            ))
            .w(px(16.))
            .h(px(16.))
            .flex_none()
    };

    let (badge_text, badge_bg, badge_fg): (SharedString, u32, u32) = match &item.kind {
        DevAssetKind::CondaEnv { is_base: true, .. } => {
            (tr_dev_badge_base(lang).into(), PRIMARY_FIXED, PRIMARY)
        }
        DevAssetKind::CondaEnv { .. } => ("Conda".into(), SURF_HIGH, TEXT),
        DevAssetKind::PythonInterpreter { scope, .. } => match scope {
            SiteScope::Install => ("System".into(), SURF_HIGH, OUTLINE),
            SiteScope::User => ("User".into(), PRIMARY_FIXED, PRIMARY),
        },
        DevAssetKind::VirtualEnv { manager, .. } => {
            (manager_name(*manager).into(), SURF_HIGH, TEXT)
        }
        DevAssetKind::NodeGlobalPackage { manager, .. } => {
            (manager.clone().into(), SURF_HIGH, TEXT)
        }
        DevAssetKind::PythonTool { tool_kind, .. } => match tool_kind {
            PythonToolKind::Uv => ("uv".into(), PRIMARY_FIXED, PRIMARY),
            PythonToolKind::Pipx => ("pipx".into(), SURF_HIGH, TEXT),
        },
        DevAssetKind::PipPackage { .. } => ("pip".into(), SURF_HIGH, TEXT),
    };

    let sub_desc = match &item.kind {
        DevAssetKind::PythonInterpreter { package_count, .. }
        | DevAssetKind::VirtualEnv { package_count, .. }
        | DevAssetKind::CondaEnv { package_count, .. } => match package_count {
            Some(count) => tr_dev_detail_packages(lang, *count),
            None => item.path.display().to_string(),
        },
        DevAssetKind::NodeGlobalPackage { version, .. } => format!("v{version}"),
        DevAssetKind::PythonTool { executables, .. } => {
            if executables.is_empty() {
                item.path.display().to_string()
            } else {
                executables.join(", ")
            }
        }
        _ => item.path.display().to_string(),
    };

    let size_text = fmt_size(item.size.exclusive_reclaimable_bytes);

    div()
        .id(gpui::ElementId::Name(
            format!("row-master-{}", item.id).into(),
        ))
        .flex()
        .items_center()
        .justify_between()
        .gap_2()
        .p_2()
        .rounded_lg()
        .cursor_pointer()
        .border_1()
        .when(active, |d| {
            d.bg(rgba(PRIMARY_FIXED, 0.45)).border_color(rgb(PRIMARY))
        })
        .when(!active, |d| {
            d.bg(rgb(CARD))
                .border_color(rgba(OUTLINE_VAR, 0.2))
                .hover(|h| h.bg(rgb(SURF_LOW)))
        })
        .on_click(cx.listener(move |this, _, _, cx| {
            this.select_active_dev_env(select_id.clone(), cx);
        }))
        .child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .flex_1()
                .min_w(px(0.))
                .child(cb)
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .flex()
                        .flex_col()
                        .gap(px(1.))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_2()
                                .child(
                                    div()
                                        .text_xs()
                                        .font_weight(gpui::FontWeight::SEMIBOLD)
                                        .text_color(rgb(TEXT))
                                        .whitespace_nowrap()
                                        .overflow_hidden()
                                        .text_ellipsis()
                                        .child(item.display_name().to_string()),
                                )
                                .child(badge_chip(badge_text, badge_bg, badge_fg)),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(rgb(OUTLINE))
                                .whitespace_nowrap()
                                .overflow_hidden()
                                .text_ellipsis()
                                .child(sub_desc),
                        ),
                ),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap_1()
                .flex_none()
                .child(
                    div()
                        .text_xs()
                        .font_weight(gpui::FontWeight::MEDIUM)
                        .text_color(if active { rgb(PRIMARY) } else { rgb(MUTED) })
                        .child(size_text),
                )
                .when(active, |d| {
                    d.child(div().text_xs().text_color(rgb(PRIMARY)).child("▸"))
                }),
        )
        .into_any_element()
}

/// 右侧 Detail 详情与全高包管理面板
fn render_detail_panel(
    dev: &DevEnvState,
    search_query: &str,
    anim_phase: usize,
    lang: Language,
    busy: bool,
    cx: &mut Context<Root>,
) -> AnyElement {
    let Some(active_item) = dev.active_env() else {
        return div()
            .flex_1()
            .min_w(px(0.))
            .flex()
            .flex_col()
            .rounded_xl()
            .bg(rgb(CARD))
            .border_1()
            .border_color(rgba(OUTLINE_VAR, 0.5))
            .items_center()
            .justify_center()
            .child(
                div()
                    .text_sm()
                    .text_color(rgb(OUTLINE))
                    .child(tr_dev_detail_empty_selection(lang)),
            )
            .into_any_element();
    };

    let can_remove = crate::core::dev_env::remove::can_remove(active_item);
    let active_id = active_item.id.clone();

    // 格式化展示路径
    let display_path = {
        let raw = active_item.path.display().to_string();
        #[cfg(windows)]
        {
            raw.replace('/', "\\")
        }
        #[cfg(not(windows))]
        {
            raw
        }
    };

    let age_text = match active_item.last_change {
        Some(evidence) => {
            crate::core::declutter::format_age_text(evidence.age_days(SystemTime::now()))
                .get(lang)
                .to_string()
        }
        None => tr_dev_timestamp_unknown(lang).to_string(),
    };

    // 头部环境概览（Detail Header）
    let header_card = div()
        .p_4()
        .bg(rgb(SURF_LOW))
        .border_b_1()
        .border_color(rgba(OUTLINE_VAR, 0.4))
        .flex()
        .flex_col()
        .gap_3()
        .child(
            div()
                .flex()
                .items_start()
                .justify_between()
                .gap_3()
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_2()
                                .child(
                                    div()
                                        .text_base()
                                        .font_weight(gpui::FontWeight::BOLD)
                                        .text_color(rgb(TEXT))
                                        .child(active_item.display_name().to_string()),
                                )
                                .when(active_item.is_active_env, |d| {
                                    d.child(badge_chip(
                                        tr_dev_badge_active(lang),
                                        CAUTION_CONTAINER,
                                        CAUTION,
                                    ))
                                })
                                .when(!can_remove, |d| {
                                    d.child(badge_chip(
                                        tr_dev_badge_display_only(lang),
                                        CARD,
                                        OUTLINE,
                                    ))
                                }),
                        )
                        .child(
                            div()
                                .id(gpui::ElementId::Name(
                                    format!("dev-detail-path-{}", active_item.id).into(),
                                ))
                                .text_xs()
                                .text_color(rgb(OUTLINE))
                                .whitespace_nowrap()
                                .overflow_hidden()
                                .text_ellipsis()
                                .tooltip(path_tooltip(&display_path))
                                .child(display_path.clone()),
                        ),
                )
                .when(can_remove, |d| {
                    let target = active_item.clone();
                    d.child(
                        div()
                            .id(gpui::ElementId::Name(
                                format!("dev-remove-header-{}", active_item.id).into(),
                            ))
                            .child(small_button(
                                tr_dev_btn_remove(lang).to_string(),
                                CARD,
                                ERROR,
                                !busy,
                            ))
                            .when(!busy, |d| {
                                d.on_click(cx.listener(move |this, _, _, cx| {
                                    this.request_remove_dev_asset(target.clone(), cx);
                                }))
                            }),
                    )
                }),
        )
        // 关键指标卡片横排
        .child(
            div()
                .flex()
                .gap_4()
                .child(detail_metric_box(
                    tr_dev_col_exclusive(lang),
                    fmt_size(active_item.size.exclusive_reclaimable_bytes),
                    PRIMARY,
                ))
                .child(detail_metric_box(
                    tr_dev_col_size(lang),
                    fmt_size(active_item.size.logical_bytes),
                    TEXT,
                ))
                .child(detail_metric_box(
                    tr_dev_col_last_change(lang),
                    age_text,
                    MUTED,
                )),
        );

    // 内容区：判断是否支持包列表（即 Python 用户包目录或虚拟环境）
    let is_package_container = matches!(
        &active_item.kind,
        DevAssetKind::PythonInterpreter { scope, site_packages: Some(_), .. } if *scope == SiteScope::User
    ) || matches!(&active_item.kind, DevAssetKind::VirtualEnv { .. });

    let content_area: AnyElement = if is_package_container {
        // 如果正在加载
        if dev.loading_site.as_deref() == Some(&active_id) {
            div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .child(loading_state_view(
                    tr_dev_site_loading(lang),
                    tr_dev_overview_desc(lang),
                    anim_phase,
                ))
                .into_any_element()
        } else {
            let packages = dev.site_packages.get(&active_id);
            match packages {
                Some(pkgs) if pkgs.is_empty() => div()
                    .flex_1()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_xs()
                    .text_color(rgb(OUTLINE))
                    .child(tr_dev_site_empty(lang))
                    .into_any_element(),
                Some(pkgs) => {
                    let total_pkgs = pkgs.len();
                    let visible_pkgs: Vec<&DevAssetItem> = pkgs
                        .iter()
                        .filter(|p| {
                            search_query.is_empty()
                                || p.display_name().to_lowercase().contains(search_query)
                        })
                        .collect();

                    let site_id_all = active_id.clone();
                    let site_id_invert = active_id.clone();

                    let toolbar = div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .px_4()
                        .py_2()
                        .border_b_1()
                        .border_color(rgba(OUTLINE_VAR, 0.3))
                        .bg(rgb(CARD))
                        .child(
                            div()
                                .text_xs()
                                .font_weight(gpui::FontWeight::MEDIUM)
                                .text_color(rgb(MUTED))
                                .child(tr_dev_site_matched(lang, visible_pkgs.len(), total_pkgs)),
                        )
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_2()
                                .child(
                                    div()
                                        .id(gpui::ElementId::Name(
                                            format!("btn-detail-all-{}", active_id).into(),
                                        ))
                                        .child(small_button(
                                            tr_dev_site_select_all(lang).to_string(),
                                            SURF_LOW,
                                            TEXT,
                                            !busy,
                                        ))
                                        .when(!busy, |d| {
                                            d.on_click(cx.listener(move |this, _, _, cx| {
                                                this.apps
                                                    .dev
                                                    .select_site_packages(&site_id_all, true);
                                                cx.notify();
                                            }))
                                        }),
                                )
                                .child(
                                    div()
                                        .id(gpui::ElementId::Name(
                                            format!("btn-detail-invert-{}", active_id).into(),
                                        ))
                                        .child(small_button(
                                            tr_dev_batch_invert(lang).to_string(),
                                            SURF_LOW,
                                            TEXT,
                                            !busy,
                                        ))
                                        .when(!busy, |d| {
                                            d.on_click(cx.listener(move |this, _, _, cx| {
                                                this.apps.dev.invert_site_packages(&site_id_invert);
                                                cx.notify();
                                            }))
                                        }),
                                ),
                        );

                    let mut pkg_scroll_list = div()
                        .id(gpui::ElementId::Name(
                            format!("detail-pkg-scroll-{}", active_id).into(),
                        ))
                        .flex_1()
                        .min_h(px(0.))
                        .overflow_scroll()
                        .p_3()
                        .flex()
                        .flex_col()
                        .gap_1();

                    for package in visible_pkgs {
                        let checked = dev.is_selected(&package.id);
                        pkg_scroll_list = pkg_scroll_list.child(render_package_row(
                            package,
                            Some(&active_item.path),
                            checked,
                            lang,
                            busy,
                            cx,
                        ));
                    }

                    div()
                        .flex_1()
                        .min_h(px(0.))
                        .flex()
                        .flex_col()
                        .child(toolbar)
                        .child(pkg_scroll_list)
                        .into_any_element()
                }
                None => div()
                    .flex_1()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_xs()
                    .text_color(rgb(OUTLINE))
                    .child(tr_dev_site_loading(lang))
                    .into_any_element(),
            }
        }
    } else {
        // 普通资产档案面板（Node 全局包 / CLI 工具等）
        render_asset_profile(active_item, &display_path, lang)
    };

    div()
        .flex_1()
        .min_w(px(0.))
        .flex()
        .flex_col()
        .rounded_xl()
        .bg(rgb(CARD))
        .border_1()
        .border_color(rgba(OUTLINE_VAR, 0.5))
        .overflow_hidden()
        .child(header_card)
        .child(content_area)
        .into_any_element()
}

/// 头部指标小方格
fn detail_metric_box(label: &'static str, value: String, val_color: u32) -> AnyElement {
    div()
        .flex_1()
        .p_2()
        .rounded_md()
        .bg(rgb(CARD))
        .border_1()
        .border_color(rgba(OUTLINE_VAR, 0.3))
        .flex()
        .flex_col()
        .gap(px(1.))
        .child(div().text_xs().text_color(rgb(OUTLINE)).child(label))
        .child(
            div()
                .text_sm()
                .font_weight(gpui::FontWeight::BOLD)
                .text_color(rgb(val_color))
                .child(value),
        )
        .into_any_element()
}

/// 独立资产的详细档案视图（不需要展示子包时）
fn render_asset_profile(item: &DevAssetItem, display_path: &str, lang: Language) -> AnyElement {
    let mut profile = div().flex_1().min_h(px(0.)).p_6().flex().flex_col().gap_4();

    profile = profile.child(
        div()
            .flex()
            .flex_col()
            .gap_1()
            .child(
                div()
                    .text_xs()
                    .font_weight(gpui::FontWeight::BOLD)
                    .text_color(rgb(OUTLINE))
                    .child(tr_dev_detail_path(lang)),
            )
            .child(
                div()
                    .p_3()
                    .rounded_md()
                    .bg(rgb(SURF_LOW))
                    .border_1()
                    .border_color(rgba(OUTLINE_VAR, 0.3))
                    .text_xs()
                    .font_family(".SystemUIFont")
                    .text_color(rgb(TEXT))
                    .child(display_path.to_string()),
            ),
    );

    if let DevAssetKind::PythonTool { executables, .. } = &item.kind {
        if !executables.is_empty() {
            profile = profile.child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .text_xs()
                            .font_weight(gpui::FontWeight::BOLD)
                            .text_color(rgb(OUTLINE))
                            .child(tr_dev_detail_executables(lang)),
                    )
                    .child(
                        div()
                            .p_3()
                            .rounded_md()
                            .bg(rgb(SURF_LOW))
                            .border_1()
                            .border_color(rgba(OUTLINE_VAR, 0.3))
                            .flex()
                            .flex_wrap()
                            .gap_2()
                            .children(executables.iter().map(|exe| {
                                div()
                                    .px_2()
                                    .py_1()
                                    .rounded_md()
                                    .bg(rgb(CARD))
                                    .border_1()
                                    .border_color(rgba(OUTLINE_VAR, 0.4))
                                    .text_xs()
                                    .text_color(rgb(PRIMARY))
                                    .child(exe.clone())
                            })),
                    ),
            );
        }
    }

    if let DevAssetKind::NodeGlobalPackage { bin_shims, .. } = &item.kind {
        if !bin_shims.is_empty() {
            profile = profile.child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .text_xs()
                            .font_weight(gpui::FontWeight::BOLD)
                            .text_color(rgb(OUTLINE))
                            .child(tr_dev_detail_executables(lang)),
                    )
                    .child(
                        div()
                            .p_3()
                            .rounded_md()
                            .bg(rgb(SURF_LOW))
                            .border_1()
                            .border_color(rgba(OUTLINE_VAR, 0.3))
                            .flex()
                            .flex_wrap()
                            .gap_2()
                            .children(bin_shims.iter().map(|shim| {
                                div()
                                    .px_2()
                                    .py_1()
                                    .rounded_md()
                                    .bg(rgb(CARD))
                                    .border_1()
                                    .border_color(rgba(OUTLINE_VAR, 0.4))
                                    .text_xs()
                                    .text_color(rgb(TEXT))
                                    .child(shim.clone())
                            })),
                    ),
            );
        }
    }

    profile.into_any_element()
}

/// 提取包相对所属环境的差异目录名，彻底剔除长达百字符的完全重复公共前缀。
fn format_dist_relative_name(
    dist_path: &std::path::Path,
    parent_env_path: Option<&std::path::Path>,
) -> String {
    if let Some(parent) = parent_env_path {
        if let Ok(rel) = dist_path.strip_prefix(parent) {
            let s = rel.to_string_lossy().to_string();
            if !s.is_empty() {
                #[cfg(windows)]
                {
                    return s.replace('/', "\\");
                }
                #[cfg(not(windows))]
                {
                    return s;
                }
            }
        }
    }
    // 降级：如果不能 strip_prefix，直接显示末尾目录名（例如 APScheduler-3.11.0.dist-info）
    if let Some(file_name) = dist_path.file_name() {
        return file_name.to_string_lossy().to_string();
    }
    dist_path.display().to_string()
}

/// 右侧包列表中每个 pip 包单行组件（高度约 40px，紧凑干净）
fn render_package_row(
    package: &DevAssetItem,
    parent_env_path: Option<&std::path::Path>,
    checked: bool,
    lang: Language,
    busy: bool,
    cx: &mut Context<Root>,
) -> AnyElement {
    let version = match &package.kind {
        DevAssetKind::PipPackage { version, .. } => version.clone(),
        _ => None,
    };
    let display_full_path = {
        let raw = package.path.display().to_string();
        #[cfg(windows)]
        {
            raw.replace('/', "\\")
        }
        #[cfg(not(windows))]
        {
            raw
        }
    };
    let rel_dist_name = format_dist_relative_name(&package.path, parent_env_path);

    let age_text = package.last_change.map(|evidence| {
        crate::core::declutter::format_age_text(evidence.age_days(SystemTime::now()))
            .get(lang)
            .to_string()
    });

    let can_remove = crate::core::dev_env::remove::can_remove(package);
    let pkg_id = package.id.clone();
    let cb = if can_remove {
        div()
            .id(gpui::ElementId::Name(
                format!("cb-detail-pkg-{}", package.id).into(),
            ))
            .flex_none()
            .child(checkbox(if checked { Check::On } else { Check::Off }))
            .when(!busy, |d| {
                d.on_click(cx.listener(move |this, _, _, cx| {
                    this.apps.dev.toggle_select(&pkg_id);
                    cx.notify();
                }))
            })
    } else {
        div()
            .id(gpui::ElementId::Name(
                format!("cb-detail-pkg-ph-{}", package.id).into(),
            ))
            .w(px(20.))
            .h(px(20.))
            .flex_none()
    };

    let mut row = div()
        .flex()
        .items_center()
        .justify_between()
        .gap_3()
        .p_2()
        .pl_3()
        .rounded_md()
        .bg(rgb(CARD))
        .border_1()
        .border_color(rgba(OUTLINE_VAR, 0.2))
        .hover(|h| h.bg(rgb(SURF_LOW)))
        .child(cb)
        .child({
            div()
                .flex()
                .flex_col()
                .gap(px(2.))
                .flex_1()
                .min_w(px(0.))
                .overflow_hidden()
                // 第一行：包名加粗 + 版本独立微胶囊 + 安装时间
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(
                            div()
                                .id(gpui::ElementId::Name(
                                    format!("dev-pkg-name-{}", package.id).into(),
                                ))
                                .whitespace_nowrap()
                                .overflow_hidden()
                                .text_ellipsis()
                                .text_sm()
                                .font_weight(gpui::FontWeight::BOLD)
                                .text_color(rgb(TEXT))
                                .child(package.display_name().to_string()),
                        )
                        .children(version.map(|v| {
                            div()
                                .px(px(6.))
                                .py(px(1.))
                                .rounded_full()
                                .bg(rgb(SURF_HIGH))
                                .border_1()
                                .border_color(rgba(OUTLINE_VAR, 0.4))
                                .text_xs()
                                .font_weight(gpui::FontWeight::MEDIUM)
                                .text_color(rgb(PRIMARY))
                                .child(format!("v{v}"))
                        }))
                        .children(age_text.map(|age| {
                            div()
                                .text_xs()
                                .text_color(rgb(OUTLINE))
                                .child(format!("· {age}"))
                        })),
                )
                // 第二行：精炼差异目录名，鼠标悬停 Tooltip 显示完整绝对路径
                .child(
                    div()
                        .id(gpui::ElementId::Name(
                            format!("dev-pkg-path-{}", package.id).into(),
                        ))
                        .whitespace_nowrap()
                        .overflow_hidden()
                        .text_ellipsis()
                        .text_xs()
                        .text_color(rgb(MUTED))
                        .tooltip(path_tooltip(&display_full_path))
                        .child(format!("📁 {rel_dist_name}")),
                )
        });

    if can_remove {
        let id = gpui::ElementId::Name(format!("dev-pkg-remove-{}", package.id).into());
        let target = package.clone();
        row = row.child(
            div()
                .id(id)
                .child(small_button(
                    tr_dev_btn_remove(lang).to_string(),
                    SURF_HIGH,
                    ERROR,
                    !busy,
                ))
                .when(!busy, |d| {
                    d.on_click(cx.listener(move |this, _, _, cx| {
                        this.request_remove_dev_asset(target.clone(), cx);
                    }))
                }),
        );
    }

    row.into_any_element()
}

/// 底部常驻悬浮胶囊操作栏
fn render_dev_bottom_bar(
    dev: &DevEnvState,
    lang: Language,
    busy: bool,
    cx: &mut Context<Root>,
) -> AnyElement {
    let (count, bytes) = dev.selection_summary();
    let enabled = count > 0 && !busy;

    div()
        .flex()
        .items_center()
        .justify_center()
        .child(
            div()
                .flex()
                .items_center()
                .gap_4()
                .px_4()
                .py_2()
                .rounded_full()
                .bg(rgb(CARD))
                .border_1()
                .border_color(rgba(OUTLINE_VAR, 0.6))
                .shadow_lg()
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .text_xs()
                                .text_color(rgb(OUTLINE))
                                .child(tr_dev_batch_bar_selected(lang)),
                        )
                        .child(
                            div()
                                .text_sm()
                                .font_weight(gpui::FontWeight::BOLD)
                                .text_color(if count > 0 { rgb(TEXT) } else { rgb(MUTED) })
                                .child(if count > 0 {
                                    format!("{count} 项 · {}", fmt_size(bytes))
                                } else {
                                    "0 项".to_string()
                                }),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(div().w(px(1.)).h(px(20.)).bg(rgb(OUTLINE_VAR)))
                        .child(
                            div()
                                .id("dev-sel-all")
                                .child(small_button(
                                    tr_dev_batch_select_all(lang).to_string(),
                                    SURF_LOW,
                                    TEXT,
                                    !busy,
                                ))
                                .when(!busy, |d| {
                                    d.on_click(cx.listener(|this, _, _, cx| {
                                        this.apps.dev.select_all();
                                        cx.notify();
                                    }))
                                }),
                        )
                        .child(
                            div()
                                .id("dev-sel-invert")
                                .child(small_button(
                                    tr_dev_batch_invert(lang).to_string(),
                                    SURF_LOW,
                                    TEXT,
                                    !busy,
                                ))
                                .when(!busy, |d| {
                                    d.on_click(cx.listener(|this, _, _, cx| {
                                        this.apps.dev.invert_selection();
                                        cx.notify();
                                    }))
                                }),
                        )
                        .child(
                            div()
                                .id("dev-sel-clear")
                                .child(small_button(
                                    tr_dev_batch_clear(lang).to_string(),
                                    SURF_LOW,
                                    TEXT,
                                    !busy && count > 0,
                                ))
                                .when(!busy && count > 0, |d| {
                                    d.on_click(cx.listener(|this, _, _, cx| {
                                        this.apps.dev.clear_selection();
                                        cx.notify();
                                    }))
                                }),
                        ),
                )
                .child(
                    div()
                        .id("dev-batch-remove-btn")
                        .child(danger_button(
                            tr_dev_batch_bar_remove_btn(lang, count),
                            enabled,
                        ))
                        .when(enabled, |d| {
                            d.on_click(cx.listener(|this, _, _, cx| {
                                this.request_remove_selected_dev_assets(cx);
                            }))
                        }),
                ),
        )
        .into_any_element()
}

fn badge_chip(text: impl Into<SharedString>, bg: u32, fg: u32) -> AnyElement {
    div()
        .px(px(6.))
        .py(px(1.))
        .rounded_full()
        .bg(rgb(bg))
        .text_xs()
        .font_weight(gpui::FontWeight::MEDIUM)
        .text_color(rgb(fg))
        .child(text.into())
        .into_any_element()
}

fn manager_name(manager: VenvManager) -> &'static str {
    match manager {
        VenvManager::Virtualenvwrapper => "virtualenvwrapper",
        VenvManager::Poetry => "poetry",
    }
}
