//! 空间透镜内容视图：只读模型、布局与发送业务事件。
//! 托盘 / 窗口生命周期请修改 ui/tray_host.rs，不要放回本文件。
use crate::core::i18n::Language;
use crate::core::model::{fmt_mem, fmt_size};
use crate::ui::components::icons::{
    icon_bolt, icon_broom, icon_close, icon_disk, icon_pulse, icon_ram, icon_user_account,
};
use crate::ui::i18n::*;
use crate::ui::theme::*;
use crate::ui::tray_host::TraySession;
use crate::ui::tray_overview::{OverviewAction, TrayOverview};
use gpui::prelude::FluentBuilder;
use gpui::{
    div, px, rgb, AnyElement, Context, Entity, FontWeight, InteractiveElement, IntoElement,
    ParentElement, Render, StatefulInteractiveElement, Styled, Window,
};

pub struct TrayPopover {
    language: Language,
    model: Entity<TrayOverview>,
    session: TraySession<OverviewAction>,
}
impl TrayPopover {
    pub fn new(
        language: Language,
        model: Entity<TrayOverview>,
        session: TraySession<OverviewAction>,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.observe(&model, |_, _, cx| cx.notify()).detach();
        Self {
            language,
            model,
            session,
        }
    }
}

impl Render for TrayPopover {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let lang = self.language;
        let state = self.model.read(cx);

        div()
            .size_full()
            .bg(rgb(BG))
            .border_1()
            .border_color(rgba(OUTLINE_VAR, 0.6))
            .flex()
            .flex_col()
            .overflow_hidden()
            .text_color(rgb(TEXT))
            // 顶栏 Header
            .child(
                div()
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .justify_between()
                    .px_3()
                    .py_2()
                    .border_b_1()
                    .border_color(rgba(OUTLINE_VAR, 0.3))
                    .bg(rgb(BG))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(icon_disk(PRIMARY, 16.))
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(rgb(PRIMARY))
                                    .child(tr_tray_title(lang)),
                            ),
                    )
                    .child({
                        let session = self.session.clone();
                        div()
                            .debug_selector(|| "tray-close".into())
                            .cursor_pointer()
                            .p_1()
                            .rounded_md()
                            .hover(|s| s.bg(rgba(MUTED, 0.15)))
                            .on_mouse_down(gpui::MouseButton::Left, move |_, _, cx| {
                                session.dismiss(cx);
                            })
                            .child(icon_close(MUTED, 14.))
                    }),
            )
            // 中间内容主体
            .child(
                div()
                    .id("tray-content")
                    .debug_selector(|| "tray-content-viewport".into())
                    .flex_1()
                    .min_h(px(0.))
                    .overflow_y_scroll()
                    .p_3()
                    .flex()
                    .flex_col()
                    .gap_3()
                    // 1. 磁盘卷空间占用
                    .child(self.render_disk_volumes(lang, state))
                    // 本地账户与磁盘共用滚动区
                    .child(self.render_user_accounts(lang, state)),
            )
            .child(
                div()
                    .flex_shrink_0()
                    .px_3()
                    .py_2()
                    .child(self.render_hardware_status(lang, state)),
            )
            // 底部快捷操作栏
            .child(self.render_action_bar(lang, state))
    }
}

impl TrayPopover {
    /// 渲染磁盘卷进度条
    fn render_disk_volumes(&self, lang: Language, state: &TrayOverview) -> AnyElement {
        let mut list = div()
            .debug_selector(|| "tray-disk-list".into())
            .flex()
            .flex_col()
            .gap_2();
        if state.volumes.is_empty() {
            list = list.child(div().text_xs().child(tr_tray_loading(lang)));
        }

        for vol in &state.volumes {
            let used = vol.total_bytes.saturating_sub(vol.free_bytes);
            let pct = if vol.total_bytes > 0 {
                (used as f64 / vol.total_bytes as f64).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let pct_int = (pct * 100.0).round() as u32;

            let bar_color = if pct > 0.95 {
                ERROR
            } else if pct > 0.85 {
                AMBER
            } else {
                PRIMARY
            };

            list = list.child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .text_xs()
                            .child(
                                div()
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(rgb(TEXT))
                                    .child(vol.label.clone()),
                            )
                            .child(div().text_color(rgb(MUTED)).child(format!(
                                "{} / {}",
                                fmt_size(used),
                                fmt_size(vol.total_bytes)
                            ))),
                    )
                    // 进度条槽
                    .child(
                        div()
                            .w_full()
                            .h(px(6.))
                            .rounded_full()
                            .bg(rgb(SURF_HIGHEST))
                            .overflow_hidden()
                            .child(
                                div()
                                    .w(gpui::relative(pct as f32))
                                    .h_full()
                                    .rounded_full()
                                    .bg(rgb(bar_color)),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .text_xs()
                            .text_color(rgb(MUTED))
                            .child(tr_tray_disk_free(lang, &fmt_size(vol.free_bytes)))
                            .child(format!("{pct_int}%")),
                    ),
            );
        }

        div()
            .flex()
            .flex_col()
            .gap_1()
            .child(
                div()
                    .text_xs()
                    .font_weight(FontWeight::BOLD)
                    .text_color(rgba(TEXT, 0.7))
                    .child(tr_tray_disk_usage(lang)),
            )
            .child(
                div()
                    .p_2()
                    .rounded_lg()
                    .bg(rgb(BG))
                    .border_1()
                    .border_color(rgba(OUTLINE_VAR, 0.3))
                    .child(list),
            )
            .into_any_element()
    }

    /// 渲染硬件核心状态（内存、CPU负载与温度）
    fn render_hardware_status(&self, lang: Language, state: &TrayOverview) -> AnyElement {
        let mem_pct = if state.memory_total > 0 {
            ((state.memory_used as f64 / state.memory_total as f64) * 100.0).round() as u32
        } else {
            0
        };

        div()
            .debug_selector(|| "tray-hardware".into())
            .flex()
            .flex_col()
            .gap_1()
            .child(
                div()
                    .text_xs()
                    .font_weight(FontWeight::BOLD)
                    .text_color(rgba(TEXT, 0.7))
                    .child(tr_tray_hardware_status(lang)),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(rgb(MUTED))
                    .child(if state.memory_total == 0 {
                        tr_tray_loading(lang)
                    } else {
                        tr_tray_refresh_interval(lang)
                    }),
            )
            .child(
                div()
                    .flex()
                    .gap_2()
                    // 内存状态卡片
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .overflow_hidden()
                            .p_2()
                            .rounded_lg()
                            .bg(rgb(BG))
                            .border_1()
                            .border_color(rgba(OUTLINE_VAR, 0.3))
                            .flex()
                            .items_center()
                            .gap_2()
                            .debug_selector(|| "tray-memory".into())
                            .child(icon_ram(PRIMARY, 16.))
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .child(
                                        div()
                                            .text_xs()
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(rgb(TEXT))
                                            .child(tr_tray_memory_label(lang, mem_pct)),
                                    )
                                    .child(div().text_xs().text_color(rgb(MUTED)).child(format!(
                                        "{} / {}",
                                        fmt_mem(state.memory_used),
                                        fmt_mem(state.memory_total)
                                    )))
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(rgb(MUTED))
                                            .child(tr_tray_swap(lang, &fmt_mem(state.swap_used))),
                                    ),
                            ),
                    )
                    // CPU / 温度状态卡片
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .overflow_hidden()
                            .p_2()
                            .rounded_lg()
                            .bg(rgb(BG))
                            .border_1()
                            .border_color(rgba(OUTLINE_VAR, 0.3))
                            .flex()
                            .items_center()
                            .gap_2()
                            .debug_selector(|| "tray-cpu".into())
                            .child(icon_pulse(SUCCESS, 16.))
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .child(
                                        div()
                                            .text_xs()
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(rgb(TEXT))
                                            .child(format!("CPU {}%", state.cpu_pct)),
                                    )
                                    .child(div().text_xs().text_color(rgb(MUTED)).child(
                                        if let Some(t) = state.temp_c {
                                            format!("{t:.0}°C")
                                        } else {
                                            tr_tray_unavailable(lang).to_string()
                                        },
                                    )),
                            ),
                    ),
            )
            .into_any_element()
    }

    /// 渲染本地账户与目录
    fn render_user_accounts(&self, lang: Language, state: &TrayOverview) -> AnyElement {
        let mut list = div().flex().flex_col().gap_1();

        for acc in &state.accounts {
            let size_str = acc
                .size_bytes
                .map(fmt_size)
                .unwrap_or_else(|| acc.path.display().to_string());

            list = list.child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .px_1()
                    .py_0p5()
                    .text_xs()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_1p5()
                            .child(icon_user_account(MUTED, 13.))
                            .child(
                                div()
                                    .font_weight(if acc.is_current {
                                        FontWeight::BOLD
                                    } else {
                                        FontWeight::NORMAL
                                    })
                                    .text_color(rgb(TEXT))
                                    .child(acc.name.clone()),
                            )
                            .when(acc.is_current, |el| {
                                el.child(
                                    div()
                                        .px_1()
                                        .rounded(px(3.))
                                        .bg(rgb(PRIMARY_FIXED))
                                        .text_color(rgb(PRIMARY))
                                        .font_weight(FontWeight::MEDIUM)
                                        .child(tr_tray_current_user_tag(lang)),
                                )
                            }),
                    )
                    .child(
                        div()
                            .text_color(rgb(MUTED))
                            .font_weight(FontWeight::MEDIUM)
                            .max_w(px(155.))
                            .truncate()
                            .child(size_str),
                    ),
            );
        }

        div()
            .flex()
            .flex_col()
            .gap_1()
            .child(
                div()
                    .text_xs()
                    .font_weight(FontWeight::BOLD)
                    .text_color(rgba(TEXT, 0.7))
                    .child(tr_tray_user_accounts(lang)),
            )
            .child(
                div()
                    .p_2()
                    .rounded_lg()
                    .bg(rgb(BG))
                    .border_1()
                    .border_color(rgba(OUTLINE_VAR, 0.3))
                    .child(list),
            )
            .into_any_element()
    }

    /// 渲染底部操作按钮
    fn render_action_bar(&self, lang: Language, state: &TrayOverview) -> AnyElement {
        let session = self.session.clone();
        let open_main = session.clone();
        let quit = session.clone();

        div()
            .debug_selector(|| "tray-actions".into())
            .flex_shrink_0()
            .p_3()
            .bg(rgb(BG))
            .border_t_1()
            .border_color(rgba(OUTLINE_VAR, 0.3))
            .flex()
            .flex_col()
            .gap_2()
            // 两个快捷次级操作
            .child(
                div()
                    .flex()
                    .gap_2()
                    // 释放内存按钮
                    .child({
                        let is_purged = state.purged;
                        let release = session.clone();
                        div()
                            .flex_1()
                            .h(px(28.))
                            .rounded_md()
                            .border_1()
                            .border_color(rgba(OUTLINE_VAR, 0.5))
                            .bg(if is_purged {
                                rgb(SUCCESS_CONTAINER)
                            } else {
                                rgb(CARD)
                            })
                            .hover(|s| s.bg(rgb(SURF_LOW)))
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .justify_center()
                            .gap_1p5()
                            .text_xs()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(if is_purged { rgb(SUCCESS) } else { rgb(TEXT) })
                            .on_mouse_down(gpui::MouseButton::Left, move |_, _, cx| {
                                release.emit(OverviewAction::ReleaseMemory, cx)
                            })
                            .child(icon_bolt(if is_purged { SUCCESS } else { PRIMARY }, 13.))
                            .child(if state.purging {
                                tr_tray_purging(lang)
                            } else if state.purge_failed {
                                tr_tray_purge_failed(lang)
                            } else if is_purged {
                                tr_tray_memory_purged(lang)
                            } else {
                                tr_tray_purge_memory(lang)
                            })
                    })
                    // 快速扫描按钮
                    .child({
                        let scan = session.clone();
                        div()
                            .flex_1()
                            .h(px(28.))
                            .rounded_md()
                            .border_1()
                            .border_color(rgba(OUTLINE_VAR, 0.5))
                            .bg(rgb(CARD))
                            .hover(|s| s.bg(rgb(SURF_LOW)))
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .justify_center()
                            .gap_1p5()
                            .text_xs()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(rgb(TEXT))
                            .debug_selector(|| "tray-scan".into())
                            .on_mouse_down(gpui::MouseButton::Left, move |_, _, cx| {
                                scan.emit(OverviewAction::ScanJunk, cx);
                            })
                            .child(icon_broom(PRIMARY, 13.))
                            .child(tr_tray_quick_scan(lang))
                    }),
            )
            // 打开主界面主按钮
            .child(
                div()
                    .w_full()
                    .h(px(32.))
                    .rounded_md()
                    .bg(rgb(PRIMARY))
                    .hover(|s| s.bg(rgb(PRIMARY_BRIGHT)))
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_xs()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(rgb(ON_PRIMARY))
                    .debug_selector(|| "tray-main".into())
                    .on_mouse_down(gpui::MouseButton::Left, move |_, _, cx| {
                        open_main.emit(OverviewAction::OpenMain, cx);
                    })
                    .child(tr_tray_open_main(lang)),
            )
            // 退出程序小按钮
            .child(
                div().flex().justify_center().pt_1().child(
                    div()
                        .text_xs()
                        .text_color(rgb(MUTED))
                        .hover(|s| s.text_color(rgb(ERROR)))
                        .cursor_pointer()
                        .on_mouse_down(gpui::MouseButton::Left, move |_, _, cx| {
                            quit.emit(OverviewAction::Quit, cx);
                        })
                        .child(tr_tray_quit(lang)),
                ),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::user_accounts::LocalAccount;
    use crate::ui::tray_overview::TrayVolumeInfo;
    use gpui::AppContext;
    use gpui::{Modifiers, TestAppContext};
    use std::sync::Arc;

    fn fixture(cx: &mut TestAppContext) -> &mut gpui::VisualTestContext {
        fixture_with_volumes(cx, Language::En, 2)
    }

    fn fixture_with_volumes(
        cx: &mut TestAppContext,
        language: Language,
        volumes: usize,
    ) -> &mut gpui::VisualTestContext {
        let host = crate::ui::tray_host::TrayHost::new(340., 430.);
        let (_, visual) = cx.add_window_view(|window, cx| {
            host.mount(
                window,
                cx,
                Arc::new(|_: OverviewAction, _: &mut gpui::App| {}),
                |session, _, cx| {
                    let model = cx.new(|_| {
                        let mut state = TrayOverview::new(session.lifetime());
                        state.memory_total = 32 * 1024_u64.pow(3);
                        state.memory_used = 25 * 1024_u64.pow(3);
                        state.cpu_pct = 42;
                        state.temp_c = Some(65.);
                        state.volumes = (0..volumes)
                            .map(|i| TrayVolumeInfo {
                                label: format!("Volume {i}"),
                                total_bytes: 1000,
                                free_bytes: 500,
                            })
                            .collect();
                        state.accounts = (0..3)
                            .map(|i| LocalAccount {
                                name: format!("user{i}"),
                                path: std::path::PathBuf::new(),
                                is_current: i == 0,
                                size_bytes: Some(100),
                            })
                            .collect();
                        state
                    });
                    cx.new(|cx| TrayPopover::new(language, model, session, cx))
                },
            )
        });
        visual.simulate_resize(gpui::size(px(340.), px(430.)));
        visual.run_until_parked();
        visual
    }

    #[gpui::test]
    fn tray_close_button_removes_its_window(cx: &mut TestAppContext) {
        let visual = fixture(cx);
        let bounds = visual.debug_bounds("tray-close").unwrap();
        visual.simulate_mouse_down(bounds.center(), gpui::MouseButton::Left, Modifiers::none());
        assert!(visual.windows().is_empty());
    }

    #[gpui::test]
    fn tray_main_button_removes_its_window(cx: &mut TestAppContext) {
        let visual = fixture(cx);
        let bounds = visual.debug_bounds("tray-main").unwrap();
        visual.simulate_mouse_down(bounds.center(), gpui::MouseButton::Left, Modifiers::none());
        assert!(visual.windows().is_empty());
    }

    #[gpui::test]
    fn tray_scan_button_removes_its_window(cx: &mut TestAppContext) {
        let visual = fixture(cx);
        let bounds = visual.debug_bounds("tray-scan").unwrap();
        visual.simulate_mouse_down(bounds.center(), gpui::MouseButton::Left, Modifiers::none());
        assert!(visual.windows().is_empty());
    }

    #[gpui::test]
    fn tray_escape_closes_window(cx: &mut TestAppContext) {
        let visual = fixture(cx);
        visual.simulate_keystrokes("escape");
        assert!(visual.windows().is_empty());
    }

    #[gpui::test]
    fn tray_deactivation_closes_window(cx: &mut TestAppContext) {
        let visual = fixture(cx);
        visual.update(|window, _| window.activate_window());
        visual.run_until_parked();
        visual.deactivate_window();
        assert!(visual.windows().is_empty());
    }

    #[gpui::test]
    fn tray_hardware_stays_visible_with_many_volumes(cx: &mut TestAppContext) {
        for language in [Language::Zh, Language::En] {
            let visual = fixture_with_volumes(cx, language, 6);
            let footer = visual.debug_bounds("tray-actions").unwrap();
            let hardware = visual.debug_bounds("tray-hardware").unwrap();
            for selector in ["tray-hardware", "tray-memory", "tray-cpu"] {
                let bounds = visual.debug_bounds(selector).unwrap();
                assert!(bounds.size.height > px(0.), "{selector} must have height");
                assert!(
                    bounds.top() >= px(0.) && bounds.bottom() <= footer.top(),
                    "{selector} is hidden behind disk overflow: {bounds:?}; footer: {footer:?}"
                );
            }
            let content = visual.debug_bounds("tray-content-viewport").unwrap();
            assert!(
                content.size.height > px(50.),
                "disks must retain a usable scroll viewport"
            );
            assert!(
                content.bottom() <= hardware.top(),
                "disk scrolling must not cover hardware"
            );
            assert!(
                hardware.size.height < px(170.),
                "hardware must leave room for disks"
            );
            assert!(footer.bottom() <= px(430.));
            let disk_before = visual.debug_bounds("tray-disk-list").unwrap();
            visual.simulate_event(gpui::ScrollWheelEvent {
                position: content.center(),
                delta: gpui::ScrollDelta::Pixels(gpui::point(px(0.), px(-180.))),
                modifiers: Modifiers::none(),
                touch_phase: gpui::TouchPhase::Moved,
            });
            visual.run_until_parked();
            let disk_after = visual.debug_bounds("tray-disk-list").unwrap();
            assert!(
                disk_after.top() < disk_before.top(),
                "additional disks must remain reachable by scrolling"
            );
            assert_eq!(
                visual.debug_bounds("tray-hardware").unwrap(),
                hardware,
                "scrolling disks must not move hardware"
            );
        }
    }

    #[gpui::test]
    fn tray_footer_stays_inside_small_window(cx: &mut TestAppContext) {
        let visual = fixture(cx);
        let bounds = visual.debug_bounds("tray-actions").unwrap();
        assert!(
            bounds.bottom() <= px(430.),
            "footer extends past viewport: {bounds:?}"
        );
    }
}
