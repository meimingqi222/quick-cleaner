//! 智能清理视图与操作底栏 (CleanFlow 质感分层清理)

use crate::core::categories::{CategoryId, Safety};
use crate::core::i18n::Language;
use crate::core::model::{commas, fmt_size, truncate, Check};
use crate::ui::components::buttons::{danger_button, small_button};
use crate::ui::components::cards::card;
use crate::ui::components::controls::{badge, checkbox, loading_state_view, page_heading};
use crate::ui::components::icons::*;
use crate::ui::components::scroll::{
    drag_capture, drag_to_offset, scroll_metrics, scrollbar, SCROLLBAR_W,
};
use crate::ui::i18n::*;
use crate::ui::theme::*;
use crate::ui::Root;
use gpui::{div, prelude::*, px, rgb, AnyElement, Context, Div, IntoElement, SharedString};

fn right_cell(width: f32, text: String, color: u32, bold: bool) -> Div {
    div()
        .w(px(width))
        .flex_none()
        .text_right()
        .text_color(rgb(color))
        .when(bold, |d| d.font_weight(gpui::FontWeight::SEMIBOLD))
        .child(text)
}

fn category_icon(cat: CategoryId, fg: u32, size: f32) -> AnyElement {
    match cat {
        CategoryId::SystemTemp => icon_trash(fg, size),
        CategoryId::UserTemp => icon_sparkle(fg, size),
        CategoryId::UserCache => icon_sparkle(fg, size),
        CategoryId::BrowserCache => icon_dashboard(fg, size),
        CategoryId::PackageCache => icon_apps(fg, size),
        CategoryId::UpdaterPackages => icon_apps(fg, size),
        CategoryId::Logs => icon_clock(fg, size),
        CategoryId::RecycleBin => icon_trash(fg, size),
        CategoryId::Thumbnails => icon_sparkle(fg, size),
        CategoryId::BrokenLoginItems => icon_shield(fg, size),
        CategoryId::AiAgents => icon_sparkle(fg, size),
        CategoryId::DevBuild => icon_gear(fg, size),
        CategoryId::DevWorktrees => icon_shield(fg, size),
        CategoryId::LocalSnapshots => icon_clock(fg, size),
        CategoryId::IosBackup => icon_apps(fg, size),
        CategoryId::OldIdeData => icon_apps(fg, size),
        CategoryId::DockerImages => icon_apps(fg, size),
    }
}

/// 单行条目的高度。uniform_list 要求行高一致，这里写死以便它精确布局。
const ITEM_ROW_H: f32 = 52.0;
/// 展开区最多占多高，超出部分自己滚。
const LIST_MAX_H: f32 = 420.0;

/// 渲染某个分类展开后的条目列表。
///
/// 用 `uniform_list` 而不是把所有行塞进容器：条目数可达上千，全量渲染
/// 会让整页滚动掉到个位数帧率。`uniform_list` 只构造视口内的那十几行。
/// 展开区行渲染要用的数据快照：`(序号, 路径, 标签, 截断路径, 文件数,
/// 体积, 占用徽标(文案, 是否应用级))`。
type ItemRowData = (
    usize,
    std::path::PathBuf,
    String,
    String,
    u64,
    u64,
    Option<(String, bool)>,
);

fn render_category_items(
    root: &Root,
    summary: &crate::core::scanner::CategorySummary,
    cx: &mut Context<Root>,
) -> AnyElement {
    let id = summary.category;
    let count = summary.items.len();
    if count == 0 {
        return div()
            .px_5()
            .py_4()
            .border_t_1()
            .border_color(rgba(OUTLINE_VAR, 0.3))
            .text_xs()
            .text_color(rgb(OUTLINE))
            .child(tr_category_empty(root.language))
            .into_any_element();
    }

    let Some(handle) = root.junk.scroll.get(&id).cloned() else {
        return div().into_any_element();
    };
    let list_h = (count as f32 * ITEM_ROW_H).min(LIST_MAX_H);

    // 先算滚动条：列表需要据此决定是否给它让出右侧宽度，
    // 否则最右边的「大小」列会被滑块压住。
    let base = handle.0.borrow().base_handle.clone();
    let metrics = scroll_metrics(&base, list_h, count as f32 * ITEM_ROW_H);

    let list = gpui::uniform_list(
        SharedString::from(format!("junk-items-{id:?}")),
        count,
        cx.processor(move |this, range: std::ops::Range<usize>, _window, cx| {
            let lang = this.language;
            let Some(summary) = this.junk.categories.iter().find(|c| c.category == id) else {
                return Vec::new();
            };
            // 先把这一段要用的数据拷出来，避免在 cx.listener 闭包里继续借用 this。
            // 双语标签在这里就按当前语言定下来，行渲染不必再认识 Text。
            let rows: Vec<ItemRowData> = range
                .filter_map(|i| {
                    let item = summary.items.get(i)?;
                    let busy = item
                        .busy
                        .as_ref()
                        .and_then(|b| b.badge())
                        .map(|(text, app_level)| (text.get(lang).to_string(), app_level));
                    Some((
                        i,
                        item.path.clone(),
                        item.label.get(lang).to_string(),
                        truncate(&item.path.to_string_lossy(), 70),
                        item.file_count,
                        item.size,
                        busy,
                    ))
                })
                .collect();

            rows.into_iter()
                .map(|(i, path, label, path_text, file_count, size, busy)| {
                    let checked = this.junk.selected.contains(&path);
                    item_row(
                        i, path, label, path_text, file_count, size, busy, checked, lang, cx,
                    )
                })
                .collect()
        }),
    )
    .track_scroll(handle.clone())
    .h(px(list_h))
    .when(metrics.is_some(), |l| l.pr(px(SCROLLBAR_W)));

    let bar = metrics.map(|m| {
        scrollbar(
            SharedString::from(format!("junk-thumb-{id:?}")),
            m,
            |thumb| {
                thumb.on_mouse_down(
                    gpui::MouseButton::Left,
                    cx.listener(move |this, event: &gpui::MouseDownEvent, _, cx| {
                        let Some(h) = this.junk.scroll.get(&id) else {
                            return;
                        };
                        let top: f32 = (-h.0.borrow().base_handle.offset().y).into();
                        let mouse_y: f32 = event.position.y.into();
                        this.junk.scroll_drag = Some((id, mouse_y, top.max(0.0)));
                        cx.notify();
                    }),
                )
            },
        )
    });

    div()
        .relative()
        .h(px(list_h))
        .border_t_1()
        .border_color(rgba(OUTLINE_VAR, 0.4))
        .bg(rgb(CARD))
        .child(list)
        .children(bar)
        .into_any_element()
}

/// 展开区里的单行条目（统一纯净背景、行底微分割线、整行可点交互）
#[allow(clippy::too_many_arguments)]
fn item_row(
    idx: usize,
    path: std::path::PathBuf,
    label: String,
    path_text: String,
    file_count: u64,
    size: u64,
    busy: Option<(String, bool)>,
    checked: bool,
    lang: Language,
    cx: &mut Context<Root>,
) -> AnyElement {
    let dim = size == 0;
    let toggle_path = path.clone();
    let exclude_target = path.clone();

    div()
        .id(SharedString::from(format!("item-{idx}")))
        .w_full()
        .h(px(ITEM_ROW_H))
        .flex()
        .items_center()
        .gap_3()
        .px_5()
        .bg(rgb(CARD))
        .hover(|h| h.bg(rgb(SURF_LOW)))
        .cursor_pointer()
        .border_b_1()
        .border_color(rgba(OUTLINE_VAR, 0.25))
        .on_click(cx.listener(move |this, _, _, cx| {
            this.toggle_item(&toggle_path);
            cx.notify();
        }))
        .child(
            div()
                .id(SharedString::from(format!("cb-item-{idx}")))
                .flex_none()
                .child(checkbox(if checked { Check::On } else { Check::Off })),
        )
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .flex()
                .flex_col()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(
                            div()
                                .text_sm()
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .text_color(rgb(TEXT))
                                .child(label),
                        )
                        // 占用徽标：应用级是橙色提示，纯系统占用偏阻断语义用红色
                        .children(busy.map(|(text, app_level)| {
                            if app_level {
                                badge(text, CAUTION_CONTAINER, CAUTION)
                            } else {
                                badge(text, ERROR_CONTAINER, ERROR)
                            }
                        })),
                )
                .child(div().text_xs().text_color(rgb(OUTLINE)).child(path_text)),
        )
        .child(right_cell(
            85.,
            if file_count > 0 {
                tr_file_count(lang, &commas(file_count))
            } else {
                String::from("—")
            },
            OUTLINE,
            false,
        ))
        .child(right_cell(
            95.,
            if size > 0 {
                fmt_size(size)
            } else {
                String::from("0 B")
            },
            if dim {
                OUTLINE
            } else if size >= 1024 * 1024 * 1024 {
                PRIMARY
            } else {
                TEXT
            },
            !dim,
        ))
        // 「永久排除」：把这个路径拉进用户白名单，从列表移走、任何清理
        // 通道永远跳过。点击必须 stop_propagation——否则排除的同时会把
        // 这条目当成普通点击勾选上，与「永远别碰」的意图恰好相反。
        .child(
            div()
                .id(SharedString::from(format!("excl-{idx}")))
                .flex_none()
                .px_2()
                .py_1()
                .rounded_md()
                .text_xs()
                .text_color(rgb(MUTED))
                .cursor_pointer()
                .hover(|h| h.text_color(rgb(ERROR)).bg(rgba(0xdc2626, 0.10)))
                .child(tr_exclude(lang))
                .on_click(cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    this.exclude_path(&exclude_target, cx);
                })),
        )
        .into_any_element()
}

/// 批量选择工具栏上的一个动作：(标签, 元素 ID, 是否为当前状态, 点击后执行什么)
type BatchAction = (&'static str, &'static str, bool, fn(&mut Root));

/// 批量选择工具栏。
///
/// 扫描完默认只勾「推荐」那一套，但用户常常想一次性全清、或者把默认
/// 勾选整体取消掉再自己挑。逐个类别点复选框太麻烦，这里给四个动作。
fn render_selection_toolbar(root: &Root, cx: &mut Context<Root>) -> Div {
    let lang = root.language;
    let total = root.total_item_count();
    let picked = root.selected_count();
    let enabled = root.junk.scanned && !root.clean.running;
    let is_recommended = root.selection_is_recommended();

    let actions: [BatchAction; 4] = [
        (
            tr_batch_rec(lang),
            "rec",
            is_recommended,
            Root::select_recommended,
        ),
        (
            tr_batch_all(lang),
            "all",
            picked == total && total > 0,
            Root::select_every,
        ),
        (
            tr_batch_invert(lang),
            "invert",
            false,
            Root::invert_selection,
        ),
        (
            tr_batch_clear(lang),
            "clear",
            picked == 0,
            Root::select_none,
        ),
    ];

    let buttons: Vec<_> = actions
        .into_iter()
        .map(|(label, id_key, active, action)| {
            div()
                .id(SharedString::from(format!("sel-{id_key}")))
                .child(small_button(
                    label.to_string(),
                    if active { PRIMARY_FIXED } else { SURF_LOW },
                    if active { PRIMARY } else { MUTED },
                    enabled,
                ))
                .when(enabled, |d| {
                    d.on_click(cx.listener(move |this, _, _, cx| {
                        action(this);
                        cx.notify();
                    }))
                })
        })
        .collect();

    let count_text = match lang {
        Language::Zh => format!("共 {total} 项"),
        Language::En => format!("{total} items"),
    };

    div()
        .flex()
        .items_center()
        .gap_2()
        // 与左侧「已选择清理 N 项」之间的分隔
        .child(
            div()
                .w(px(1.))
                .h(px(24.))
                .flex_none()
                .bg(rgba(OUTLINE_VAR, 0.7))
                .mr_1(),
        )
        .children(buttons)
        .child(
            div()
                .text_xs()
                .text_color(rgb(OUTLINE))
                .ml_1()
                .child(count_text),
        )
}

/// 「排除清单」面板：列出白名单条目并提供移除入口。
///
/// 「排除」只有单向入口时，误排除一次就得手改 settings.json——旧版本
/// 写入的条目还可能是被压小写的路径，在 JSON 里都难以认领。这里给每条
/// 一个「移除」按钮，保护表始终可逆。
fn render_exclusions_panel(root: &Root, cx: &mut Context<Root>) -> AnyElement {
    let lang = root.language;
    let entries = &root.settings.whitelist;

    let mut panel = card()
        .p_4()
        .flex()
        .flex_col()
        .gap_2()
        .child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(icon_shield(MUTED, 14.))
                .child(
                    div()
                        .text_xs()
                        .text_color(rgb(OUTLINE))
                        .child(tr_exclusions_hint(lang)),
                ),
        );

    if entries.is_empty() {
        panel = panel.child(
            div()
                .text_sm()
                .text_color(rgb(MUTED))
                .py_2()
                .child(tr_exclusions_empty(lang)),
        );
    } else {
        for (idx, path) in entries.iter().enumerate() {
            let target = std::path::PathBuf::from(path);
            panel = panel.child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .px_2()
                    .py_1()
                    .rounded_md()
                    .bg(rgb(SURF_LOW))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .text_xs()
                            .text_color(rgb(TEXT))
                            .child(truncate(path, 90)),
                    )
                    // 移除只撤这一条（精确匹配）；被释放的路径要等下次扫描
                    // 才重新出现在列表里，状态栏会说明。
                    .child(
                        div()
                            .id(SharedString::from(format!("wl-remove-{idx}")))
                            .flex_none()
                            .px_2()
                            .py_1()
                            .rounded_md()
                            .text_xs()
                            .text_color(rgb(MUTED))
                            .cursor_pointer()
                            .hover(|h| h.text_color(rgb(ERROR)).bg(rgba(0xdc2626, 0.10)))
                            .child(tr_exclusion_remove(lang))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.remove_exclusion(&target, cx);
                            })),
                    ),
            );
        }
    }
    panel.into_any_element()
}

pub fn render_junk_view(root: &Root, cx: &mut Context<Root>) -> AnyElement {
    let lang = root.language;
    let total = root.total_cleanable();

    let found = card()
        .flex_none()
        .px_5()
        .py_3()
        .flex()
        .items_center()
        .gap_4()
        .child(
            div()
                .flex()
                .flex_col()
                .child(
                    div()
                        .text_xs()
                        .font_weight(gpui::FontWeight::MEDIUM)
                        .text_color(rgb(OUTLINE))
                        .child(tr_found_cleanable(lang)),
                )
                .child(
                    div()
                        .text_xl()
                        .font_weight(gpui::FontWeight::BOLD)
                        .text_color(rgb(if total > 0 { ERROR } else { PRIMARY }))
                        .child(if root.junk.scanned {
                            fmt_size(total)
                        } else {
                            String::from("—")
                        }),
                ),
        )
        .child(
            div()
                .w(px(36.))
                .h(px(36.))
                .flex_none()
                .rounded_full()
                .bg(rgb(if total > 0 {
                    ERROR_CONTAINER
                } else {
                    PRIMARY_FIXED
                }))
                .flex()
                .items_center()
                .justify_center()
                .child(icon_trash(if total > 0 { ERROR } else { PRIMARY }, 18.)),
        );

    let (heading_title, heading_sub) = match lang {
        Language::Zh => (
            "智能清理",
            "可重建的应用、浏览器、包管理与应用更新包缓存默认已勾选；临时数据与开发产物需手动勾选",
        ),
        Language::En => (
            "Smart Clean",
            "Rebuildable app, browser, package-manager and update-package caches are selected; temp data and dev builds require manual selection",
        ),
    };

    let header = div()
        .flex()
        .items_center()
        .justify_between()
        .gap_4()
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .child(page_heading(heading_title, heading_sub)),
        )
        // 排除清单开关：保护表必须可查可撤，不然单向排除成了陷阱
        .child(
            div()
                .id("exclusions-toggle")
                .flex_none()
                .child(small_button(
                    tr_exclusions_toggle(lang, root.settings.whitelist.len()),
                    if root.show_exclusions {
                        PRIMARY_FIXED
                    } else {
                        SURF_LOW
                    },
                    if root.show_exclusions { PRIMARY } else { MUTED },
                    true,
                ))
                .on_click(cx.listener(|this, _, _, cx| {
                    this.show_exclusions = !this.show_exclusions;
                    cx.notify();
                })),
        )
        .child(found);

    let mut cards: Vec<AnyElement> = Vec::new();
    for summary in &root.junk.categories {
        let id = summary.category;
        let size = summary.total_size;
        let safety = id.safety();
        let expanded = root.junk.expanded.contains(&id);
        let state = root.cat_check(summary);
        let dim = size == 0;

        let head = div()
            .flex()
            .items_center()
            .gap_4()
            .px_5()
            .py_4()
            .child(
                div()
                    .id(SharedString::from(format!(
                        "cb-{}",
                        id.name_lang(Language::En)
                    )))
                    .flex_none()
                    .cursor_pointer()
                    .child(checkbox(state))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.toggle_category(id);
                        cx.notify();
                    })),
            )
            .child(
                div()
                    .id(SharedString::from(format!(
                        "row-{}",
                        id.name_lang(Language::En)
                    )))
                    .flex_1()
                    .min_w(px(0.))
                    .flex()
                    .items_center()
                    .gap_4()
                    .cursor_pointer()
                    .child(icon_badge(
                        category_icon(id, safety_color(safety), 18.),
                        safety_container(safety),
                        safety_color(safety),
                        38.,
                    ))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .flex()
                            .flex_col()
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
                                            .child(id.name_lang(lang)),
                                    )
                                    // 开发者类目默认不勾选，用徽标说明「要自己勾」，
                                    // 免得用户以为扫出来了却没被清掉是 bug
                                    .when(id.is_developer(), |d| {
                                        d.child(badge(
                                            tr_need_manual_select(lang).into(),
                                            safety_container(safety),
                                            safety_color(safety),
                                        ))
                                    })
                                    // 预算耗尽时如实说明这一类没数完，
                                    // 否则一个精确的数字会掩盖不精确的事实
                                    .when(summary.partial, |d| {
                                        d.child(badge(
                                            tr_partial_scan(lang).into(),
                                            safety_container(Safety::Caution),
                                            safety_color(Safety::Caution),
                                        ))
                                    }),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(rgb(MUTED))
                                    .child(id.desc_lang(lang)),
                            ),
                    )
                    .child(
                        div()
                            .text_sm()
                            .font_weight(gpui::FontWeight::BOLD)
                            .text_color(rgb(if dim { OUTLINE } else { safety_color(safety) }))
                            // 构建产物走第二阶段异步检索，没跑完之前显示「检索中」
                            // 而不是 0 B——后者看起来像「扫过了，没东西」。
                            .child(if root.junk.discovering && id.is_discovered() {
                                String::from(tr_discovering(lang))
                            } else if size > 0 {
                                // `≥`：预算耗尽时扫到的都是真的，只是可能
                                // 还有没扫到的，数字只能算下界。
                                if summary.partial {
                                    format!("≥ {}", fmt_size(size))
                                } else {
                                    fmt_size(size)
                                }
                            } else {
                                String::from("0 B")
                            }),
                    )
                    .child(
                        div()
                            .w(px(16.))
                            .flex_none()
                            .text_xs()
                            .font_weight(gpui::FontWeight::BOLD)
                            .text_color(rgb(OUTLINE))
                            .child(if expanded { "▾" } else { "▸" }),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.toggle_expand(id);
                        cx.notify();
                    })),
            );

        // 展开区用虚拟化列表：「项目构建产物」这一类在开发机上能有近千条，
        // 全量铺开会让整页滚动直接卡死。uniform_list 只渲染可见的那几行。
        let sub_list = if expanded {
            Some(render_category_items(root, summary, cx))
        } else {
            None
        };

        cards.push(
            card()
                .overflow_hidden()
                .child(head)
                .children(sub_list)
                .into_any_element(),
        );
    }

    let mut skipped_banner: Option<AnyElement> = None;
    if !root.clean.last_failed.is_empty() {
        skipped_banner = Some(
            card()
                .p_4()
                .bg(rgb(ERROR_CONTAINER))
                .border_color(rgba(ERROR, 0.4))
                .flex()
                .flex_col()
                .gap_2()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(
                            div()
                                .text_sm()
                                .font_weight(gpui::FontWeight::BOLD)
                                .text_color(rgb(ERROR))
                                .child(tr_last_clean_skipped(lang, root.clean.last_failed.len())),
                        )
                        .child(
                            div()
                                .id("failed-details-toggle")
                                .text_xs()
                                .font_weight(gpui::FontWeight::BOLD)
                                .text_color(rgb(ERROR))
                                .cursor_pointer()
                                .child(tr_toggle_details(lang, root.clean.show_failed_details))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.clean.show_failed_details =
                                        !this.clean.show_failed_details;
                                    cx.notify();
                                })),
                        ),
                )
                .when(root.clean.show_failed_details, |d| {
                    d.child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .p_2()
                            .rounded_md()
                            .bg(rgba(CARD, 0.6))
                            .children(root.clean.last_failed.iter().take(20).map(|p| {
                                div()
                                    .text_xs()
                                    .text_color(rgb(ERROR))
                                    .child(truncate(&p.to_string_lossy(), 80))
                                    .into_any_element()
                            })),
                    )
                })
                .into_any_element(),
        );
    }

    let (loading_title, loading_sub) = match lang {
        Language::Zh => (
            "正在全面扫描系统冗余垃圾",
            "安全检索系统临时缓存、应用日志与回收站，准备释放空间",
        ),
        Language::En => (
            "Scanning system junk files…",
            "Safely checking system temp, caches, logs and Recycle Bin",
        ),
    };

    let body: AnyElement = if root.junk.scanning {
        loading_state_view(loading_title, loading_sub, root.anim_phase)
    } else {
        div()
            .flex()
            .flex_col()
            .gap_3()
            .children(cards)
            .into_any_element()
    };

    div()
        .id("junk-scroll")
        .size_full()
        .min_w(px(0.))
        .overflow_scroll()
        .p_8()
        .flex()
        .flex_col()
        .gap_5()
        .child(header)
        .children(root.show_exclusions.then(|| render_exclusions_panel(root, cx)))
        .children(skipped_banner)
        .child(body)
        // 滚动条拖拽的 move/up 走窗口级监听：鼠标拖出滑块、拖出整页
        // 甚至拖出窗口，事件都还接得住。
        .child(drag_capture(
            cx.entity(),
            |this, mouse_y, cx| {
                let Some((id, start_y, start_top)) = this.junk.scroll_drag else {
                    return;
                };
                let Some(handle) = this.junk.scroll.get(&id) else {
                    return;
                };
                let base = handle.0.borrow().base_handle.clone();
                if let Some(new_top) = drag_to_offset(&base, (start_y, start_top), mouse_y) {
                    base.set_offset(gpui::point(px(0.0), px(-new_top)));
                    cx.notify();
                }
            },
            |this, cx| {
                if this.junk.scroll_drag.take().is_some() {
                    cx.notify();
                }
            },
        ))
        .into_any_element()
}

pub fn render_clean_bar(root: &Root, cx: &mut Context<Root>) -> impl IntoElement {
    let lang = root.language;
    let size = root.selected_size();
    let count = root.selected_count();
    let enabled = root.junk.scanned && !root.clean.running && !root.junk.scanning && count > 0;

    let items_label = match lang {
        Language::Zh => format!("({count} 项)"),
        Language::En => format!("({count} items)"),
    };

    let clean_btn_text = if root.clean.running {
        tr_cleaning(lang).to_string()
    } else {
        tr_clean_now(lang).to_string()
    };

    div()
        .flex_none()
        .w_full()
        .px_8()
        .py_3()
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
                .shadow_lg()
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
                                    "已选择清理"
                                } else {
                                    "Selected for Cleaning"
                                }),
                        )
                        .child(
                            div()
                                .flex()
                                .items_baseline()
                                .gap_2()
                                .child(
                                    div()
                                        .text_lg()
                                        .font_weight(gpui::FontWeight::BOLD)
                                        .text_color(rgb(TEXT))
                                        .child(fmt_size(size)),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .font_weight(gpui::FontWeight::MEDIUM)
                                        .text_color(rgb(OUTLINE))
                                        .child(items_label),
                                ),
                        ),
                )
                .child(render_selection_toolbar(root, cx))
                .child(
                    div()
                        .id("clean-now")
                        .child(danger_button(clean_btn_text, enabled))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.request_clean_selected(cx);
                        })),
                ),
        )
}
