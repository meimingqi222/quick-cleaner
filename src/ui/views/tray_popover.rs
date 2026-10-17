//! 空间透镜内容视图：只读模型、布局与发送业务事件。
//! 托盘 / 窗口生命周期请修改 ui/tray_host.rs，不要放回本文件。
use crate::core::i18n::Language;
use crate::core::model::{fmt_mem, fmt_size};
use crate::ui::components::icons::{
    icon_apps, icon_bolt, icon_chevron_right, icon_close, icon_declutter, icon_disk, icon_pulse,
    icon_ram, icon_search, icon_trash,
};
use crate::ui::i18n::*;
use crate::ui::shell::View;
use crate::ui::theme::*;
use crate::ui::tray_host::TraySession;
use crate::ui::tray_overview::{OverviewAction, TrayOverview};
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
                    .px_3()
                    .pt_3()
                    .pb_2()
                    .flex()
                    .flex_col()
                    .gap_2()
                    // 磁盘卷空间占用
                    .child(self.render_disk_volumes(lang, state)),
            )
            .child(
                div()
                    .flex_shrink_0()
                    .px_3()
                    .py_1p5()
                    .child(self.render_hardware_status(lang, state)),
            )
            // 底部快捷操作栏
            .child(self.render_action_bar(lang, state))
    }
}

impl TrayPopover {
    /// 渲染磁盘卷进度条（支持 1~3 卷单列大卡与 4+ 卷双列自适应 Bento 网格）
    fn render_disk_volumes(&self, lang: Language, state: &TrayOverview) -> AnyElement {
        let mut list = div()
            .debug_selector(|| "tray-disk-list".into())
            .flex()
            .flex_col()
            .gap_1p5();

        if state.volumes.is_empty() {
            list = list.child(
                div()
                    .text_xs()
                    .text_color(rgb(MUTED))
                    .child(tr_tray_loading(lang)),
            );
        } else if state.volumes.len() <= 3 {
            for vol in &state.volumes {
                list = list.child(self.render_single_disk_card(vol, lang));
            }
        } else {
            for chunk in state.volumes.chunks(2) {
                let mut row = div().flex().gap_1p5();
                for vol in chunk {
                    row = row.child(self.render_compact_disk_card(vol));
                }
                if chunk.len() == 1 {
                    row = row.child(div().flex_1());
                }
                list = list.child(row);
            }
        }

        let is_multi = state.volumes.len() >= 4;
        let mut header = div().flex().items_center().justify_between().child(
            div()
                .text_xs()
                .font_weight(FontWeight::BOLD)
                .text_color(rgba(TEXT, 0.7))
                .child(tr_tray_disk_usage(lang)),
        );
        if is_multi {
            header = header.child(
                div()
                    .text_xs()
                    .text_color(rgb(MUTED))
                    .child(tr_tray_volume_count(lang, state.volumes.len())),
            );
        }

        div()
            .flex()
            .flex_col()
            .gap_1()
            .child(header)
            .child(list)
            .into_any_element()
    }

    /// 渲染单列大卡片（1~3 个磁盘时使用）
    fn render_single_disk_card(
        &self,
        vol: &crate::ui::tray_overview::TrayVolumeInfo,
        lang: Language,
    ) -> AnyElement {
        let used = vol.total_bytes.saturating_sub(vol.free_bytes);
        let pct = if vol.total_bytes > 0 {
            (used as f64 / vol.total_bytes as f64).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let pct_int = (pct * 100.0).round() as u32;

        let bar_color = if pct > 0.90 {
            ERROR
        } else if pct > 0.75 {
            AMBER
        } else {
            PRIMARY
        };

        let session = self.session.clone();
        let vol_label = vol.label.clone();

        div()
            .p_2()
            .rounded_md()
            .bg(rgb(CARD))
            .border_1()
            .border_color(rgba(OUTLINE_VAR, 0.35))
            .hover(|s| s.bg(rgb(SURF_LOW)).border_color(rgb(PRIMARY)))
            .cursor_pointer()
            .on_mouse_down(gpui::MouseButton::Left, move |_, _, cx| {
                session.emit(OverviewAction::AnalyzeDisk(vol_label.clone()), cx);
            })
            .flex()
            .flex_col()
            .gap_1()
            // 第一行：[C:] 盘符 + 已用/总量 + 剩余空间 (百分比)
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .text_xs()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_1p5()
                            .child(
                                div()
                                    .px_1p5()
                                    .py_0p5()
                                    .rounded(px(3.))
                                    .bg(rgba(PRIMARY, 0.12))
                                    .text_color(rgb(PRIMARY))
                                    .font_weight(FontWeight::BOLD)
                                    .child(vol.label.clone()),
                            )
                            .child(div().text_color(rgb(MUTED)).child(format!(
                                "{} / {}",
                                fmt_size(used),
                                fmt_size(vol.total_bytes)
                            ))),
                    )
                    .child(
                        div()
                            .text_color(if pct > 0.90 { rgb(ERROR) } else { rgb(MUTED) })
                            .font_weight(if pct > 0.90 {
                                FontWeight::BOLD
                            } else {
                                FontWeight::NORMAL
                            })
                            .child(format!(
                                "{} ({pct_int}%)",
                                tr_tray_disk_free(lang, &fmt_size(vol.free_bytes))
                            )),
                    ),
            )
            // 第二行：进度条 (拉伸填满) + 右侧 "分析 ›"
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .flex_1()
                            .h(px(5.))
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
                            .gap_0p5()
                            .text_xs()
                            .text_color(rgb(PRIMARY))
                            .font_weight(FontWeight::MEDIUM)
                            .child(tr_tray_analyze_hint(lang))
                            .child(icon_chevron_right(PRIMARY, 10.)),
                    ),
            )
            .into_any_element()
    }

    /// 渲染双列紧凑卡片（>= 4 个磁盘时自动切换）
    fn render_compact_disk_card(
        &self,
        vol: &crate::ui::tray_overview::TrayVolumeInfo,
    ) -> AnyElement {
        let used = vol.total_bytes.saturating_sub(vol.free_bytes);
        let pct = if vol.total_bytes > 0 {
            (used as f64 / vol.total_bytes as f64).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let pct_int = (pct * 100.0).round() as u32;

        let bar_color = if pct > 0.90 {
            ERROR
        } else if pct > 0.75 {
            AMBER
        } else {
            PRIMARY
        };

        let session = self.session.clone();
        let vol_label = vol.label.clone();

        div()
            .flex_1()
            .min_w(px(0.))
            .p_2()
            .rounded_md()
            .bg(rgb(CARD))
            .border_1()
            .border_color(rgba(OUTLINE_VAR, 0.35))
            .hover(|s| s.bg(rgb(SURF_LOW)).border_color(rgb(PRIMARY)))
            .cursor_pointer()
            .on_mouse_down(gpui::MouseButton::Left, move |_, _, cx| {
                session.emit(OverviewAction::AnalyzeDisk(vol_label.clone()), cx);
            })
            .flex()
            .flex_col()
            .gap_1()
            // 第一行：[C:] 33.8G (90%)
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .text_xs()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_1()
                            .child(
                                div()
                                    .px_1()
                                    .py_0p5()
                                    .rounded(px(3.))
                                    .bg(rgba(PRIMARY, 0.12))
                                    .text_color(rgb(PRIMARY))
                                    .font_weight(FontWeight::BOLD)
                                    .child(vol.label.clone()),
                            )
                            .child(div().text_color(rgb(MUTED)).child(fmt_size(vol.free_bytes))),
                    )
                    .child(
                        div()
                            .text_color(if pct > 0.90 { rgb(ERROR) } else { rgb(MUTED) })
                            .font_weight(if pct > 0.90 {
                                FontWeight::BOLD
                            } else {
                                FontWeight::NORMAL
                            })
                            .child(format!("{pct_int}%")),
                    ),
            )
            // 第二行：进度条 (拉伸填满) + 右侧小箭头
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_1p5()
                    .child(
                        div()
                            .flex_1()
                            .h(px(4.))
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
                            .text_color(rgb(PRIMARY))
                            .child(icon_chevron_right(PRIMARY, 10.)),
                    ),
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

        let session = self.session.clone();
        let status_jump = self.session.clone();

        div()
            .debug_selector(|| "tray-hardware".into())
            .flex()
            .flex_col()
            .gap_1()
            .child(
                div().flex().items_center().justify_between().child(
                    div()
                        .text_xs()
                        .font_weight(FontWeight::BOLD)
                        .text_color(rgba(TEXT, 0.7))
                        .child(tr_tray_hardware_status(lang)),
                ),
            )
            .child(
                div()
                    .flex()
                    .gap_2()
                    // 内存状态卡片
                    .child({
                        let is_purged = state.purged;
                        let release = session.clone();
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .overflow_hidden()
                            .p_2()
                            .rounded_lg()
                            .bg(rgb(CARD))
                            .border_1()
                            .border_color(rgba(OUTLINE_VAR, 0.35))
                            .flex()
                            .flex_col()
                            .justify_between()
                            .gap_1p5()
                            .debug_selector(|| "tray-memory".into())
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_1p5()
                                    .child(icon_ram(PRIMARY, 15.))
                                    .child(
                                        div()
                                            .text_xs()
                                            .font_weight(FontWeight::BOLD)
                                            .text_color(rgb(TEXT))
                                            .child(tr_tray_memory_label(lang, mem_pct)),
                                    ),
                            )
                            .child(div().text_xs().text_color(rgb(MUTED)).child(format!(
                                "{} / {}",
                                fmt_mem(state.memory_used),
                                fmt_mem(state.memory_total)
                            )))
                            .child(
                                div()
                                    .h(px(24.))
                                    .w_full()
                                    .rounded_md()
                                    .border_1()
                                    .border_color(if is_purged {
                                        rgba(SUCCESS, 1.0)
                                    } else {
                                        rgba(PRIMARY, 0.4)
                                    })
                                    .bg(if is_purged {
                                        rgba(SUCCESS_CONTAINER, 1.0)
                                    } else {
                                        rgba(PRIMARY, 0.08)
                                    })
                                    .hover(|s| s.bg(rgba(PRIMARY, 0.16)))
                                    .cursor_pointer()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .gap_1()
                                    .text_xs()
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(if is_purged {
                                        rgb(SUCCESS)
                                    } else {
                                        rgb(PRIMARY)
                                    })
                                    .on_mouse_down(gpui::MouseButton::Left, move |_, _, cx| {
                                        release.emit(OverviewAction::ReleaseMemory, cx)
                                    })
                                    .child(icon_bolt(
                                        if is_purged { SUCCESS } else { PRIMARY },
                                        12.,
                                    ))
                                    .child(if state.purging {
                                        tr_tray_purging(lang)
                                    } else if state.purge_failed {
                                        tr_tray_purge_failed(lang)
                                    } else if is_purged {
                                        tr_tray_memory_purged(lang)
                                    } else {
                                        tr_tray_purge_memory(lang)
                                    }),
                            )
                    })
                    // CPU / 温度状态卡片
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .overflow_hidden()
                            .p_2()
                            .rounded_lg()
                            .bg(rgb(CARD))
                            .border_1()
                            .border_color(rgba(OUTLINE_VAR, 0.35))
                            .hover(|s| s.bg(rgb(SURF_LOW)).border_color(rgb(PRIMARY)))
                            .cursor_pointer()
                            .on_mouse_down(gpui::MouseButton::Left, move |_, _, cx| {
                                status_jump.emit(OverviewAction::OpenView(View::Status), cx);
                            })
                            .flex()
                            .flex_col()
                            .justify_between()
                            .gap_1p5()
                            .debug_selector(|| "tray-cpu".into())
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_1p5()
                                    .child(icon_pulse(SUCCESS, 15.))
                                    .child(
                                        div()
                                            .text_xs()
                                            .font_weight(FontWeight::BOLD)
                                            .text_color(rgb(TEXT))
                                            .child(format!("CPU {}%", state.cpu_pct)),
                                    ),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(if state.temp_c.is_some_and(|t| t > 80.0) {
                                        rgb(AMBER)
                                    } else {
                                        rgb(MUTED)
                                    })
                                    .child(if let Some(t) = state.temp_c {
                                        format!("{t:.0}°C")
                                    } else {
                                        tr_tray_unavailable(lang).to_string()
                                    }),
                            )
                            .child(
                                div()
                                    .h(px(24.))
                                    .w_full()
                                    .rounded_md()
                                    .bg(rgba(SURF_HIGHEST, 0.5))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .gap_1()
                                    .text_xs()
                                    .text_color(rgb(MUTED))
                                    .child(tr_tray_view_processes(lang))
                                    .child(icon_chevron_right(MUTED, 10.)),
                            ),
                    ),
            )
            .into_any_element()
    }

    /// 渲染底部操作按钮
    fn render_action_bar(&self, lang: Language, _state: &TrayOverview) -> AnyElement {
        let session = self.session.clone();
        let open_main = session.clone();
        let quit = session.clone();

        div()
            .debug_selector(|| "tray-actions".into())
            .flex_shrink_0()
            .px_3()
            .py_2()
            .bg(rgb(BG))
            .border_t_1()
            .border_color(rgba(OUTLINE_VAR, 0.3))
            .flex()
            .flex_col()
            .gap_2()
            // 快捷功能直达区 (4 栅格 Bento 网格)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .text_xs()
                            .font_weight(FontWeight::BOLD)
                            .text_color(rgba(TEXT, 0.7))
                            .child(tr_tray_quick_tools(lang)),
                    )
                    .child(
                        div()
                            .flex()
                            .gap_1p5()
                            // 1. 智能清理 (包含 tray-scan 选择器以保证已有测试兼容)
                            .child({
                                let scan = session.clone();
                                div()
                                    .flex_1()
                                    .py_1p5()
                                    .px_1()
                                    .rounded_md()
                                    .bg(rgb(CARD))
                                    .border_1()
                                    .border_color(rgba(OUTLINE_VAR, 0.4))
                                    .hover(|s| s.bg(rgb(SURF_LOW)).border_color(rgb(PRIMARY)))
                                    .cursor_pointer()
                                    .flex()
                                    .flex_col()
                                    .items_center()
                                    .justify_center()
                                    .gap_1()
                                    .debug_selector(|| "tray-scan".into())
                                    .on_mouse_down(gpui::MouseButton::Left, move |_, _, cx| {
                                        scan.emit(OverviewAction::ScanJunk, cx);
                                    })
                                    .child(icon_trash(PRIMARY, 15.))
                                    .child(
                                        div()
                                            .text_xs()
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(rgb(TEXT))
                                            .child(tr_view_junk(lang)),
                                    )
                            })
                            // 2. 软件卸载
                            .child({
                                let apps = session.clone();
                                div()
                                    .flex_1()
                                    .py_1p5()
                                    .px_1()
                                    .rounded_md()
                                    .bg(rgb(CARD))
                                    .border_1()
                                    .border_color(rgba(OUTLINE_VAR, 0.4))
                                    .hover(|s| s.bg(rgb(SURF_LOW)).border_color(rgb(PRIMARY)))
                                    .cursor_pointer()
                                    .flex()
                                    .flex_col()
                                    .items_center()
                                    .justify_center()
                                    .gap_1()
                                    .on_mouse_down(gpui::MouseButton::Left, move |_, _, cx| {
                                        apps.emit(OverviewAction::OpenView(View::Apps), cx);
                                    })
                                    .child(icon_apps(PRIMARY, 15.))
                                    .child(
                                        div()
                                            .text_xs()
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(rgb(TEXT))
                                            .child(tr_view_apps(lang)),
                                    )
                            })
                            // 3. 极速搜索
                            .child({
                                let search = session.clone();
                                div()
                                    .flex_1()
                                    .py_1p5()
                                    .px_1()
                                    .rounded_md()
                                    .bg(rgb(CARD))
                                    .border_1()
                                    .border_color(rgba(OUTLINE_VAR, 0.4))
                                    .hover(|s| s.bg(rgb(SURF_LOW)).border_color(rgb(PRIMARY)))
                                    .cursor_pointer()
                                    .flex()
                                    .flex_col()
                                    .items_center()
                                    .justify_center()
                                    .gap_1()
                                    .on_mouse_down(gpui::MouseButton::Left, move |_, _, cx| {
                                        search.emit(OverviewAction::OpenView(View::Search), cx);
                                    })
                                    .child(icon_search(PRIMARY, 15.))
                                    .child(
                                        div()
                                            .text_xs()
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(rgb(TEXT))
                                            .child(tr_view_search(lang)),
                                    )
                            })
                            // 4. 杂物清理
                            .child({
                                let declutter = session.clone();
                                div()
                                    .flex_1()
                                    .py_1p5()
                                    .px_1()
                                    .rounded_md()
                                    .bg(rgb(CARD))
                                    .border_1()
                                    .border_color(rgba(OUTLINE_VAR, 0.4))
                                    .hover(|s| s.bg(rgb(SURF_LOW)).border_color(rgb(PRIMARY)))
                                    .cursor_pointer()
                                    .flex()
                                    .flex_col()
                                    .items_center()
                                    .justify_center()
                                    .gap_1()
                                    .on_mouse_down(gpui::MouseButton::Left, move |_, _, cx| {
                                        declutter
                                            .emit(OverviewAction::OpenView(View::Declutter), cx);
                                    })
                                    .child(icon_declutter(PRIMARY, 15.))
                                    .child(
                                        div()
                                            .text_xs()
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(rgb(TEXT))
                                            .child(tr_view_declutter(lang)),
                                    )
                            }),
                    ),
            )
            // 打开主界面主按钮
            .child(
                div()
                    .w_full()
                    .h(px(30.))
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
                div().flex().justify_center().child(
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
        let host = crate::ui::tray_host::TrayHost::new(360., 560.);
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
        visual.simulate_resize(gpui::size(px(360.), px(560.)));
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
            let visual = fixture_with_volumes(cx, language, 10);
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
            assert!(footer.bottom() <= px(560.));
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
            bounds.bottom() <= px(560.),
            "footer extends past viewport: {bounds:?}"
        );
    }

    #[gpui::test]
    fn tray_two_disks_fit_completely_without_scrolling(cx: &mut TestAppContext) {
        for language in [Language::Zh, Language::En] {
            let visual = fixture_with_volumes(cx, language, 2);
            let viewport = visual.debug_bounds("tray-content-viewport").unwrap();
            let disks = visual.debug_bounds("tray-disk-list").unwrap();
            assert!(
                disks.bottom() <= viewport.bottom() - px(6.),
                "disks must fit completely inside viewport with safety margin: disks={disks:?}, viewport={viewport:?}"
            );
        }
    }

    #[gpui::test]
    fn tray_adaptive_grid_fits_four_and_six_volumes_without_scrolling(cx: &mut TestAppContext) {
        for language in [Language::Zh, Language::En] {
            for count in [4, 6] {
                let visual = fixture_with_volumes(cx, language, count);
                let viewport = visual.debug_bounds("tray-content-viewport").unwrap();
                let disks = visual.debug_bounds("tray-disk-list").unwrap();
                assert!(
                    disks.bottom() <= viewport.bottom() - px(6.),
                    "adaptive {count} disks must fit completely inside viewport: disks={disks:?}, viewport={viewport:?}"
                );
            }
        }
    }
}
