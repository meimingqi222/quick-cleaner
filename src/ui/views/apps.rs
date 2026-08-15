//! 软件管理与深度卸载视图 (Geek Uninstaller / CleanFlow 质感升级版)

use super::apps_components::{render_apps_list_card, ListBody};
use crate::core::apps::{
    AppFilterPreset, AppSortColumn, InstalledApp,
};
use crate::core::model::{fmt_size, truncate};
use crate::ui::components::cards::card;
use crate::ui::components::controls::{loading_state_view, page_heading};
use crate::ui::components::icons::*;
use crate::ui::theme::*;
use crate::ui::Root;
use gpui::{div, prelude::*, px, rgb, AnyElement, Context, SharedString, Window};

pub fn render_apps_view(root: &Root, window: &mut Window, cx: &mut Context<Root>) -> AnyElement {
    let total_apps = root.apps.len();
    let total_app_size: u64 = root.apps.iter().map(|a| a.estimated_size).sum();

    // 统计长期未用软件（未记录或最后使用距离现在 > 90 天）
    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let stale_apps_count = root
        .apps
        .iter()
        .filter(|a| a.last_used_raw == 0 || (now_secs.saturating_sub(a.last_used_raw) > 90 * 86400))
        .count();

    // 预设过滤 + 搜索 + 排序的结果由 Root 在 render 入口统一维护，
    // 这里只借用下标，不做任何拷贝。
    let display_apps: Vec<&InstalledApp> =
        root.apps_view.iter().filter_map(|&i| root.apps.get(i)).collect();

    // 顶部大标题与概览
    let header = div()
        .flex()
        .justify_between()
        .items_center()
        .gap_4()
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .child(page_heading(
                    "软件管理与深度卸载",
                    "查看本机已安装软件、占用体积与最后使用时间",
                )),
        );

    // 顶部 3 个指标卡片
    let stats_row = div()
        .flex()
        .gap_4()
        .w_full()
        .child(
            card()
                .flex_1()
                .min_w(px(140.))
                .p_4()
                .flex()
                .items_center()
                .gap_3()
                .child(icon_badge(icon_disk(PRIMARY, 20.), PRIMARY_FIXED, PRIMARY, 44.))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .justify_center()
                        .gap(px(1.))
                        .child(
                            div()
                                .text_xs()
                                .font_weight(gpui::FontWeight::MEDIUM)
                                .text_color(rgb(OUTLINE))
                                .child("估算总占用空间"),
                        )
                        .child(
                            div()
                                .text_xl()
                                .font_weight(gpui::FontWeight::BOLD)
                                .text_color(rgb(TEXT))
                                .child(fmt_size(total_app_size)),
                        ),
                ),
        )
        .child(
            card()
                .flex_1()
                .min_w(px(140.))
                .p_4()
                .flex()
                .items_center()
                .gap_3()
                .child(icon_badge(icon_apps(PRIMARY, 20.), PRIMARY_FIXED, PRIMARY, 44.))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .justify_center()
                        .gap(px(1.))
                        .child(
                            div()
                                .text_xs()
                                .font_weight(gpui::FontWeight::MEDIUM)
                                .text_color(rgb(OUTLINE))
                                .child("已安装应用总数"),
                        )
                        .child(
                            div()
                                .text_xl()
                                .font_weight(gpui::FontWeight::BOLD)
                                .text_color(rgb(TEXT))
                                .child(format!("{total_apps} 款")),
                        ),
                ),
        )
        .child(
            card()
                .flex_1()
                .min_w(px(140.))
                .p_4()
                .flex()
                .items_center()
                .gap_3()
                .child(icon_badge(icon_clock(CAUTION, 20.), CAUTION_CONTAINER, CAUTION, 44.))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .justify_center()
                        .gap(px(1.))
                        .child(
                            div()
                                .text_xs()
                                .font_weight(gpui::FontWeight::MEDIUM)
                                .text_color(rgb(OUTLINE))
                                .child("长期未用软件 (>90天)"),
                        )
                        .child(
                            div()
                                .text_xl()
                                .font_weight(gpui::FontWeight::BOLD)
                                .text_color(rgb(CAUTION))
                                .child(format!("{stale_apps_count} 款")),
                        ),
                ),
        );

    // 快速分类过滤预设标签
    let preset_buttons = AppFilterPreset::ALL.iter().map(|&p| {
        let active = root.apps_preset == p;
        div()
            .id(SharedString::from(format!("preset-app-{}", p.label())))
            .px_3()
            .py(px(4.))
            .rounded_full()
            .text_xs()
            .font_weight(if active {
                gpui::FontWeight::SEMIBOLD
            } else {
                gpui::FontWeight::NORMAL
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
                    .border_color(rgba(OUTLINE_VAR, 0.8))
                    .text_color(rgb(MUTED))
                    .hover(|h| h.bg(rgb(SURF_LOW)))
            })
            .child(p.label())
            .on_click(cx.listener(move |this, _, _, cx| {
                this.apps_preset = p;
                cx.notify();
            }))
    });

    let search_focused = root.apps_focus_handle.is_focused(window);
    let search_text = &root.apps_search;

    let cursor_bar = if search_focused {
        Some(
            div()
                .w(px(1.5))
                .h(px(13.))
                .flex_none()
                .rounded_full()
                .bg(rgb(PRIMARY)),
        )
    } else {
        None
    };

    let text_content = if search_text.is_empty() {
        div()
            .flex_1()
            .min_w(px(0.))
            .flex()
            .items_center()
            .gap(px(2.))
            .children(cursor_bar)
            .child(
                div()
                    .text_xs()
                    .text_color(rgb(OUTLINE))
                    .child("搜索已安装软件…"),
            )
    } else {
        div()
            .flex_1()
            .min_w(px(0.))
            .flex()
            .items_center()
            .gap(px(1.5))
            .child(
                div()
                    .text_xs()
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .text_color(rgb(TEXT))
                    .child(search_text.clone()),
            )
            .children(cursor_bar)
    };

    let search_box = div()
        .id("apps-search-box")
        .track_focus(&root.apps_focus_handle)
        // 输入处理器所借的 canvas 是绝对定位的，需要这个定位上下文
        .relative()
        .w(px(240.))
        .h(px(32.))
        .px_3()
        .rounded_full()
        .bg(rgb(SURF_LOW))
        .border_1()
        .when(search_focused, |d| {
            d.border_color(rgb(PRIMARY)).bg(rgb(CARD))
        })
        .when(!search_focused, |d| {
            d.border_color(rgba(OUTLINE_VAR, 0.6))
                .hover(|h| h.bg(rgb(SURF_HIGH)))
        })
        .flex()
        .items_center()
        .gap_2()
        .cursor_text()
        .child(icon_search(if search_focused { PRIMARY } else { OUTLINE }, 13.))
        .child(text_content)
        .when(!search_text.is_empty(), |d| {
            d.child(
                div()
                    .id("clear-search-btn")
                    .px_1()
                    .rounded_full()
                    .text_xs()
                    .text_color(rgb(OUTLINE))
                    .cursor_pointer()
                    .hover(|h| h.text_color(rgb(ERROR)))
                    .child("✕")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.search_clear();
                        cx.notify();
                    })),
            )
        })
        .on_click(cx.listener(|this, _, window, cx| {
            this.apps_focus_handle.focus(window);
            cx.notify();
        }))
        // 只处理编辑键。字符输入（含输入法组合）全部由 EntityInputHandler
        // 接管，见 ui::text_input——这里再追加一次会让每个字母输入两遍。
        .on_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, _, cx| {
            match event.keystroke.key.as_str() {
                "backspace" => {
                    this.search_backspace();
                    cx.notify();
                }
                "escape" => {
                    this.search_clear();
                    cx.notify();
                }
                _ => {}
            }
        }))
        // 把输入处理器挂到焦点上。必须在绘制阶段调用 Window::handle_input，
        // 所以借一个零尺寸 canvas 拿到 bounds 并在它的 paint 回调里注册。
        .child(
            gpui::canvas(
                move |bounds, _window, _cx| bounds,
                {
                    let handle = root.apps_focus_handle.clone();
                    let entity = cx.entity();
                    move |_, bounds: gpui::Bounds<gpui::Pixels>, window, cx| {
                        entity.update(cx, |this, _| {
                            this.apps_search_bounds = Some(bounds);
                        });
                        window.handle_input(
                            &handle,
                            gpui::ElementInputHandler::new(bounds, entity.clone()),
                            cx,
                        );
                    }
                },
            )
            .absolute()
            .size_full(),
        );

    let filter_stats_tag = div()
        .px_2()
        .py(px(3.))
        .rounded_md()
        .bg(rgb(SURF_HIGH))
        .text_xs()
        .font_weight(gpui::FontWeight::MEDIUM)
        .text_color(rgb(MUTED))
        .child(if root.apps_search.is_empty() {
            format!("共 {} 款", display_apps.len())
        } else {
            format!("匹配 {} / {} 款", display_apps.len(), total_apps)
        });

    let controls_bar = card()
        .p_3()
        .flex()
        .items_center()
        .justify_between()
        .gap_4()
        .child(
            div()
                .flex()
                .items_center()
                .gap_3()
                .child(
                    div()
                        .text_xs()
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(rgb(OUTLINE))
                        .child("快速分类:"),
                )
                .child(div().flex().items_center().gap_2().children(preset_buttons))
                .child(filter_stats_tag),
        )
        .child(search_box);

    // 辅助生成可点击排序列头
    let make_header_col = |col: AppSortColumn, title: String, width: Option<f32>, align_right: bool| {
        let active = root.apps_sort.column == col;
        let indicator = root.apps_sort.indicator(col);

        let mut item = div()
            .id(SharedString::from(format!("th-col-{}", col.label())))
            .py(px(5.))
            .px(px(4.))
            .rounded_md()
            .cursor_pointer()
            .flex()
            .items_center()
            .hover(|h| h.bg(rgb(SURF_HIGH)))
            .when(align_right, |d| d.justify_end())
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .child(
                        div()
                            .text_xs()
                            .font_weight(if active {
                                gpui::FontWeight::BOLD
                            } else {
                                gpui::FontWeight::SEMIBOLD
                            })
                            .text_color(if active {
                                rgb(PRIMARY)
                            } else {
                                rgb(TEXT)
                            })
                            .child(title),
                    )
                    .when(!indicator.is_empty(), |d| {
                        d.child(
                            div()
                                .text_xs()
                                .font_weight(gpui::FontWeight::BOLD)
                                .text_color(rgb(PRIMARY))
                                .child(indicator),
                        )
                    }),
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                this.apps_sort.toggle(col);
                cx.notify();
            }));

        if let Some(w) = width {
            item = item.w(px(w)).flex_none();
        } else {
            item = item.flex_1().min_w(px(0.));
        }
        item
    };

    // 表格头（全部支持点击正逆序排序，并动态显示当前列项目数）
    let app_name_header = format!("应用名称与版本 (共 {} 款)", display_apps.len());
    let table_header = div()
        .px_5()
        .py_2()
        .bg(rgb(SURF_LOW))
        .border_b_1()
        .border_color(rgba(OUTLINE_VAR, 0.5))
        .flex()
        .items_center()
        .gap_3()
        .child(make_header_col(AppSortColumn::Name, app_name_header, None, false))
        .child(make_header_col(AppSortColumn::Publisher, "开发者".into(), Some(130.), false))
        .child(make_header_col(AppSortColumn::LastUsed, "最后使用".into(), Some(110.), false))
        .child(make_header_col(AppSortColumn::InstallDate, "安装日期".into(), Some(100.), false))
        .child(make_header_col(AppSortColumn::Size, "占用大小".into(), Some(95.), true))
        .child(
            div()
                .w(px(190.))
                .flex_none()
                .text_center()
                .text_xs()
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(rgb(OUTLINE))
                .child("操作"),
        );

    // 软件列表行
    // 行内容按需渲染，这里只决定「是占位提示还是 N 行列表」
    let body = if root.apps_scanning {
        ListBody::Placeholder(loading_state_view(
            "正在智能检索已安装软件与空间占用",
            "深度测算软件安装目录、真实体积与最后使用时间",
            root.anim_phase,
        ))
    } else if display_apps.is_empty() {
        ListBody::Placeholder(
            div()
                .p_12()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap_2()
                .child(
                    div()
                        .text_base()
                        .font_weight(gpui::FontWeight::BOLD)
                        .text_color(rgb(MUTED))
                        .child("未找到匹配的已安装软件"),
                )
                .into_any_element(),
        )
    } else {
        ListBody::Rows(display_apps.len())
    };

    let filtered_size: u64 = display_apps.iter().map(|a| a.estimated_size).sum();
    let list_footer = div()
        .px_5()
        .py_2()
        .bg(rgb(SURF_LOW))
        .border_t_1()
        .border_color(rgba(OUTLINE_VAR, 0.4))
        .flex()
        .items_center()
        .justify_between()
        .text_xs()
        .text_color(rgb(OUTLINE))
        .child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .w(px(8.))
                        .h(px(8.))
                        .rounded_full()
                        .bg(rgb(PRIMARY)),
                )
                .child(if root.apps_search.is_empty() {
                    format!("当前列表展示 {} 款软件（总计 {} 款已装）", display_apps.len(), total_apps)
                } else {
                    format!("搜索匹配 {} 款软件（总计 {} 款）", display_apps.len(), total_apps)
                }),
        )
        .child(
            div()
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(rgb(TEXT))
                .child(format!("列表总占用: {}", fmt_size(filtered_size))),
        );

    let list_card = render_apps_list_card(root, table_header, body, list_footer, cx);

    div()
        .id("apps-view")
        .size_full()
        .min_w(px(0.))
        .p_8()
        .flex()
        .flex_col()
        .gap_4()
        .child(header)
        .child(stats_row)
        .child(controls_bar)
        .child(list_card)
        .into_any_element()
}

/// 渲染软件条目悬浮右键上下文菜单（支持打开所在目录、常规卸载、强力深度清理、复制安装路径）
pub fn render_apps_context_menu(root: &Root, cx: &mut Context<Root>) -> Option<AnyElement> {
    let menu = root.apps_context_menu.as_ref()?;
    let app = menu.app.clone();
    let has_uninstaller = app.uninstall_string.is_some() || app.quiet_uninstall_string.is_some();
    let has_location = app.install_location.as_ref().map(|p| p.exists()).unwrap_or(false);
    let app_loc = app.install_location.clone();

    // 限制菜单弹出位置不超出视口边界
    let x = (menu.x - 10.).max(10.).min(1040.);
    let y = (menu.y - 10.).max(10.).min(620.);

    let app_for_uninst = app.clone();
    let app_for_resid = app.clone();
    let app_for_loc = app_loc.clone();
    let app_for_copy = app.clone();
    let app_name_for_loc = app.name.clone();

    let menu_view = div()
        .id("apps-context-menu-backdrop")
        .absolute()
        .inset_0()
        .on_mouse_down(gpui::MouseButton::Left, cx.listener(|this, _, _, cx| {
            this.close_context_menu();
            cx.notify();
        }))
        .on_mouse_down(gpui::MouseButton::Right, cx.listener(|this, _, _, cx| {
            this.close_context_menu();
            cx.notify();
        }))
        .child(
            card()
                .id("apps-context-menu-card")
                .absolute()
                .left(px(x))
                .top(px(y))
                .w(px(230.))
                .p_1()
                .rounded_xl()
                .bg(rgb(CARD))
                .border_1()
                .border_color(rgb(SURF_HIGHEST))
                .shadow_xl()
                .flex()
                .flex_col()
                .gap(px(2.))
                .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| {
                    cx.stop_propagation();
                })
                .child(
                    div()
                        .px_3()
                        .py_2()
                        .border_b_1()
                        .border_color(rgba(OUTLINE_VAR, 0.4))
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .text_xs()
                                .font_weight(gpui::FontWeight::BOLD)
                                .text_color(rgb(TEXT))
                                .child(truncate(&app.name, 24)),
                        )
                        .when(!app.version.is_empty(), |d| {
                            d.child(
                                div()
                                    .text_xs()
                                    .text_color(rgb(OUTLINE))
                                    .child(format!("版本 {}", app.version)),
                            )
                        }),
                )
                // 1. 打开安装目录
                .child(
                    div()
                        .id("ctx-open-folder")
                        .px_3()
                        .py(px(7.))
                        .rounded_md()
                        .cursor_pointer()
                        .flex()
                        .items_center()
                        .gap_2()
                        .text_xs()
                        .font_weight(gpui::FontWeight::MEDIUM)
                        .when(has_location, |d| {
                            d.text_color(rgb(TEXT))
                                .hover(|h| h.bg(rgb(SURF_HIGH)))
                        })
                        .when(!has_location, |d| {
                            d.text_color(rgb(OUTLINE))
                        })
                        .child("打开安装目录")
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if let Some(ref loc) = app_for_loc {
                                crate::platform::reveal_in_explorer(loc);
                            } else {
                                this.status = format!("软件「{app_name_for_loc}」无独立安装路径");
                            }
                            this.close_context_menu();
                            cx.notify();
                        })),
                )
                // 2. 官方常规卸载
                .child(
                    div()
                        .id("ctx-uninstall")
                        .px_3()
                        .py(px(7.))
                        .rounded_md()
                        .cursor_pointer()
                        .flex()
                        .items_center()
                        .gap_2()
                        .text_xs()
                        .font_weight(gpui::FontWeight::MEDIUM)
                        .when(has_uninstaller, |d| {
                            d.text_color(rgb(TEXT))
                                .hover(|h| h.bg(rgb(SURF_HIGH)))
                        })
                        .when(!has_uninstaller, |d| {
                            d.text_color(rgb(OUTLINE))
                        })
                        .child("官方常规卸载")
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.close_context_menu();
                            this.request_uninstall_app(app_for_uninst.clone(), cx);
                        })),
                )
                // 3. 强力深度清理
                .child(
                    div()
                        .id("ctx-residual-clean")
                        .px_3()
                        .py(px(7.))
                        .rounded_md()
                        .cursor_pointer()
                        .flex()
                        .items_center()
                        .gap_2()
                        .text_xs()
                        .font_weight(gpui::FontWeight::BOLD)
                        .text_color(rgb(PRIMARY))
                        .hover(|h| h.bg(rgba(PRIMARY, 0.08)))
                        .child("强力残留清理")
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.close_context_menu();
                            this.start_residual_scan(app_for_resid.clone(), cx);
                        })),
                )
                // 4. 复制安装路径
                .child(
                    div()
                        .id("ctx-copy-path")
                        .px_3()
                        .py(px(7.))
                        .rounded_md()
                        .cursor_pointer()
                        .flex()
                        .items_center()
                        .gap_2()
                        .text_xs()
                        .font_weight(gpui::FontWeight::MEDIUM)
                        .text_color(rgb(MUTED))
                        .hover(|h| h.bg(rgb(SURF_HIGH)))
                        .child("复制安装路径")
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if let Some(ref loc) = app_for_copy.install_location {
                                this.status = format!("已获取安装路径：{}", loc.display());
                            } else {
                                this.status = format!("软件「{}」无独立安装路径", app_for_copy.name);
                            }
                            this.close_context_menu();
                            cx.notify();
                        })),
                ),
        );

    Some(menu_view.into_any_element())
}
