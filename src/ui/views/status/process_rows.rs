/// 进程行的图标：优先用进程所属应用的真实图标，缓存未命中（还没提取完、
/// 或者这个进程压根不属于任何 .app）时回退到首字母方块。
///
/// 图标是异步提取的，第一拍多半还没就绪——回退块和图标同尺寸同位置，
/// 换上真图标时行高不跳。
/// 进程图标渲染：优先用进程真实图标，若提取中或无图标则回退到首字母色块。
fn process_icon_from_source(
    icon_source: Option<&std::path::Path>,
    name: &str,
    pid: u32,
    size: f32,
) -> AnyElement {
    if let Some(image) = icon_source.and_then(crate::ui::app_icons::try_get_icon) {
        return img(ImageSource::from(image))
            .id(SharedString::from(format!("proc-icon-{pid}")))
            .w(px(size))
            .h(px(size))
            .rounded(px(3.))
            .flex_none()
            .into_any_element();
    }
    div()
        .w(px(size))
        .h(px(size))
        .flex_none()
        .rounded(px(3.))
        .bg(rgb(SURF_LOW))
        .border_1()
        .border_color(rgba(OUTLINE_VAR, 0.5))
        .flex()
        .items_center()
        .justify_center()
        .text_xs()
        .font_weight(gpui::FontWeight::BOLD)
        .text_color(rgb(MUTED))
        .child(name.chars().take(2).collect::<String>().to_uppercase())
        .into_any_element()
}

/// 格式化进程启动时间（本地时间 + 相对时长）
fn fmt_proc_start_time(start_time: u64, lang: Language) -> String {
    if start_time == 0 {
        return "-".into();
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let ago_secs = now.saturating_sub(start_time);
    let ago_str = if ago_secs < 60 {
        tr_status_proc_started_just_now(lang).to_string()
    } else if ago_secs < 3600 {
        tr_status_proc_started_minutes_ago(lang, ago_secs / 60)
    } else if ago_secs < 86400 {
        tr_status_proc_started_hours_ago(lang, ago_secs / 3600)
    } else {
        tr_status_proc_started_days_ago(lang, ago_secs / 86400)
    };
    match chrono::DateTime::from_timestamp(start_time as i64, 0) {
        Some(dt) => {
            let local = dt.with_timezone(&chrono::Local);
            let stamp = local
                .format(tr_status_proc_start_stamp_format(lang))
                .to_string();
            tr_status_proc_started_at(lang, &stamp, &ago_str)
        }
        None => ago_str,
    }
}

/// 本地化进程状态文本
fn localize_proc_status(status_text: &str, lang: Language) -> &'static str {
    match status_text {
        "Run" | "Running" => tr_status_proc_state_running(lang),
        "Sleep" | "Sleeping" => tr_status_proc_state_sleeping(lang),
        "Idle" => tr_status_proc_state_idle(lang),
        "Stop" | "Stopped" => tr_status_proc_state_stopped(lang),
        "Zombie" => tr_status_proc_state_zombie(lang),
        _ => tr_status_proc_state_normal(lang),
    }
}

/// 迷你 CPU 趋势条形波形图（24 点）
fn render_mini_sparkline(history: &[f32], max_val: f32, color: u32, height: f32) -> AnyElement {
    const COUNT: usize = 24;
    let mut bars = div().flex().items_end().gap(px(2.)).h(px(height)).w_full();
    for i in 0..COUNT {
        let val = if i + history.len() >= COUNT {
            history[i + history.len() - COUNT]
        } else {
            0.0
        };
        let ratio = if max_val > 0.0 {
            (val / max_val).clamp(0.08, 1.0)
        } else {
            0.08
        };
        let h = (height * ratio).max(2.5);
        bars = bars.child(
            div()
                .flex_1()
                .h(px(h))
                .rounded(px(1.5))
                .bg(rgba(color, 0.4 + 0.6 * ratio)),
        );
    }
    bars.into_any_element()
}

/// 迷你内存趋势条形波形图（24 点）
fn render_mini_mem_sparkline(history: &[u64], color: u32, height: f32) -> AnyElement {
    const COUNT: usize = 24;
    let max_bytes = history.iter().copied().max().unwrap_or(1).max(1) as f32;
    let mut bars = div().flex().items_end().gap(px(2.)).h(px(height)).w_full();
    for i in 0..COUNT {
        let val = if i + history.len() >= COUNT {
            history[i + history.len() - COUNT] as f32
        } else {
            0.0
        };
        let ratio = (val / max_bytes).clamp(0.08, 1.0);
        let h = (height * ratio).max(2.5);
        bars = bars.child(
            div()
                .flex_1()
                .h(px(h))
                .rounded(px(1.5))
                .bg(rgba(color, 0.4 + 0.6 * ratio)),
        );
    }
    bars.into_any_element()
}

/// 详细信息键值对单行
fn detail_kv(label: &str, value: &str) -> Div {
    div()
        .flex()
        .items_center()
        .justify_between()
        .text_xs()
        .py(px(2.))
        .child(div().text_color(rgb(OUTLINE)).child(label.to_string()))
        .child(
            div()
                .font_weight(gpui::FontWeight::MEDIUM)
                .text_color(rgb(TEXT))
                .child(value.to_string()),
        )
}

/// 渲染端口胶囊徽章
fn render_ports_badge(ports: &[u16]) -> Option<Div> {
    if ports.is_empty() {
        return None;
    }
    let mut badges = div().flex().items_center().gap_1();
    for &p in ports.iter().take(2) {
        badges = badges.child(
            Badge::new(format!(":{p}"))
                .shape(BadgeShape::Tag)
                .size(BadgeSize::Tiny)
                .weight(gpui::FontWeight::SEMIBOLD)
                .tone(BadgeTone::Custom {
                    bg: rgba(PRIMARY, 0.12),
                    fg: rgb(PRIMARY).into(),
                }),
        );
    }
    if ports.len() > 2 {
        badges = badges.child(
            Badge::new(format!("+{}", ports.len() - 2))
                .shape(BadgeShape::Tag)
                .padding(3., 0.5)
                .weight(gpui::FontWeight::NORMAL)
                .tone(BadgeTone::NeutralSoft),
        );
    }
    Some(badges)
}

/// 进程表左右内边距。表头要按它对齐，所以取成常量。
const TABLE_PAD_X: f32 = 16.0;

/// 进程表的行高。`uniform_list` 要求等高，紧凑排布（32px）避免大字老年机感。
const PROC_ROW_H: f32 = 32.0;
const PROC_CPU_COL_W: f32 = 72.0;
const PROC_MEM_COL_W: f32 = 80.0;
const PROC_THREADS_COL_W: f32 = 54.0;
const PROC_CPU_TIME_COL_W: f32 = 74.0;
const PROC_PID_COL_W: f32 = 58.0;
const PROC_USER_COL_W: f32 = 82.0;

/// 可排序的列表头。当前排序列显示方向箭头，点击切换。
fn sort_header(
    label: String,
    key: ProcSortKey,
    width: Option<f32>,
    sort: ProcSort,
    cx: &mut Context<Root>,
) -> Stateful<Div> {
    let active = sort.key == key;
    let numeric = width.is_some();
    let base = div().id(SharedString::from(format!("proc-sort-{key:?}")));
    let base = match width {
        Some(w) => base.w(px(w)).flex_none().justify_end(),
        None => base.flex_1().min_w(px(0.)).justify_start(),
    };
    base.flex()
        .items_center()
        .gap(px(2.))
        .text_xs()
        .font_weight(if active {
            gpui::FontWeight::BOLD
        } else {
            gpui::FontWeight::MEDIUM
        })
        .text_color(rgb(match (active, numeric) {
            (true, _) => PRIMARY,
            (false, true) => OUTLINE,
            (false, false) => TEXT,
        }))
        .cursor_pointer()
        .hover(|h| h.text_color(rgb(if active { PRIMARY } else { MUTED })))
        .child(label)
        .when(active, |d| d.child(if sort.desc { " ↓" } else { " ↑" }))
        .on_click(cx.listener(move |this, _, _, cx| this.sort_processes(key, cx)))
}

/// 进程行渲染：支持 GroupHeader (折叠父组)、GroupChild (展开子项)、Single (单进程)。
fn render_process_row(
    root: &Root,
    item: &crate::ui::state::ProcDisplayItem,
    snap: &StatusSnapshot,
    cx: &mut Context<Root>,
) -> AnyElement {
    let selected_pid = root.monitor.selected_proc_pid;

    match item {
        crate::ui::state::ProcDisplayItem::GroupHeader {
            group_key,
            name,
            icon_source,
            count,
            expanded,
            total_cpu,
            total_mem,
            total_threads,
            total_cpu_time_ms,
            representative_pid,
            user,
            child_indices: _,
        } => {
            let pid = *representative_pid;
            let is_selected = selected_pid == Some(pid);
            let gkey = group_key.clone();
            let expanded_val = *expanded;

            div()
                .id(SharedString::from(format!("grp-row-{gkey}")))
                .h(px(PROC_ROW_H))
                .w_full()
                .flex()
                .items_center()
                .px(px(TABLE_PAD_X))
                .border_b_1()
                .border_color(rgba(OUTLINE_VAR, 0.35))
                .cursor_pointer()
                .when(is_selected, |d| d.bg(rgba(PRIMARY, 0.15)))
                .when(!is_selected, |d| d.hover(|h| h.bg(rgb(SURF_LOW))))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.select_proc_item(pid, cx);
                }))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_1p5()
                        .min_w(px(0.))
                        .flex_1()
                        .child(
                            // 展开/折叠箭头
                            div()
                                .id(SharedString::from(format!("toggle-grp-{gkey}")))
                                .w(px(16.))
                                .h(px(16.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded_sm()
                                .cursor_pointer()
                                .hover(|h| h.bg(rgb(SURF_HIGH)))
                                .child(if expanded_val {
                                    icon_chevron_down(MUTED, 10.)
                                } else {
                                    icon_chevron_right(MUTED, 10.)
                                })
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.toggle_proc_group(gkey.clone(), cx);
                                    cx.stop_propagation();
                                })),
                        )
                        .child(process_icon_from_source(
                            icon_source.as_deref(),
                            name,
                            pid,
                            18.,
                        ))
                        .child(
                            div()
                                .min_w(px(0.))
                                .flex()
                                .items_center()
                                .gap_1p5()
                                .child(
                                    div()
                                        .text_xs()
                                        .font_weight(gpui::FontWeight::SEMIBOLD)
                                        .text_color(rgb(TEXT))
                                        .child(truncate(name, 28).to_string()),
                                )
                                .child(
                                    div()
                                        .px_1p5()
                                        .py(px(0.5))
                                        .rounded_md()
                                        .bg(rgba(OUTLINE_VAR, 0.5))
                                        .text_xs()
                                        .font_weight(gpui::FontWeight::SEMIBOLD)
                                        .text_color(rgb(MUTED))
                                        .child(count.to_string()),
                                ),
                        ),
                )
                .child(cpu_cell(*total_cpu, PROC_CPU_COL_W))
                .child(
                    div()
                        .w(px(PROC_MEM_COL_W))
                        .flex_none()
                        .text_right()
                        .text_xs()
                        .text_color(rgb(TEXT))
                        .child(fmt_mem(*total_mem)),
                )
                .child(
                    div()
                        .w(px(PROC_THREADS_COL_W))
                        .flex_none()
                        .text_right()
                        .text_xs()
                        .text_color(rgb(OUTLINE))
                        .child(total_threads.to_string()),
                )
                .child(
                    div()
                        .w(px(PROC_CPU_TIME_COL_W))
                        .flex_none()
                        .text_right()
                        .text_xs()
                        .text_color(rgb(OUTLINE))
                        .child(crate::core::status::fmt_cpu_time(*total_cpu_time_ms)),
                )
                .child(
                    div()
                        .w(px(PROC_PID_COL_W))
                        .flex_none()
                        .text_right()
                        .text_xs()
                        .text_color(rgb(OUTLINE))
                        .child("-"),
                )
                .child(
                    div()
                        .w(px(PROC_USER_COL_W))
                        .flex_none()
                        .text_right()
                        .text_xs()
                        .text_color(rgb(OUTLINE))
                        .child(truncate(user.as_deref().unwrap_or("-"), 12).to_string()),
                )
                .into_any_element()
        }
        crate::ui::state::ProcDisplayItem::GroupChild { process_index, .. } => {
            let Some(proc) = snap.processes.get(*process_index) else {
                return div().into_any_element();
            };
            let pid = proc.pid;
            let is_selected = selected_pid == Some(pid);

            let ports_badge = render_ports_badge(&proc.ports);

            div()
                .id(SharedString::from(format!("child-row-{pid}")))
                .h(px(PROC_ROW_H))
                .w_full()
                .flex()
                .items_center()
                .px(px(TABLE_PAD_X))
                .border_b_1()
                .border_color(rgba(OUTLINE_VAR, 0.35))
                .cursor_pointer()
                .when(is_selected, |d| d.bg(rgba(PRIMARY, 0.15)))
                .when(!is_selected, |d| d.hover(|h| h.bg(rgb(SURF_LOW))))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.select_proc_item(pid, cx);
                }))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_1p5()
                        .min_w(px(0.))
                        .flex_1()
                        .child(
                            // 缩进短线指示
                            div()
                                .w(px(16.))
                                .flex_none()
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(
                                    div()
                                        .w(px(2.))
                                        .h(px(8.))
                                        .rounded_full()
                                        .bg(rgba(OUTLINE_VAR, 0.6)),
                                ),
                        )
                        .child(process_icon_from_source(
                            proc.icon_source.as_deref(),
                            &proc.name,
                            pid,
                            16.,
                        ))
                        .child(
                            div()
                                .min_w(px(0.))
                                .flex()
                                .items_center()
                                .gap_1p5()
                                .child(
                                    div()
                                        .text_xs()
                                        .font_weight(gpui::FontWeight::NORMAL)
                                        .text_color(rgb(TEXT))
                                        .child(truncate(&proc.name, 30).to_string()),
                                )
                                .children(ports_badge),
                        ),
                )
                .child(cpu_cell(proc.cpu, PROC_CPU_COL_W))
                .child(
                    div()
                        .w(px(PROC_MEM_COL_W))
                        .flex_none()
                        .text_right()
                        .text_xs()
                        .text_color(rgb(TEXT))
                        .child(fmt_mem(proc.mem_bytes)),
                )
                .child(
                    div()
                        .w(px(PROC_THREADS_COL_W))
                        .flex_none()
                        .text_right()
                        .text_xs()
                        .text_color(rgb(OUTLINE))
                        .child(proc.threads.to_string()),
                )
                .child(
                    div()
                        .w(px(PROC_CPU_TIME_COL_W))
                        .flex_none()
                        .text_right()
                        .text_xs()
                        .text_color(rgb(OUTLINE))
                        .child(crate::core::status::fmt_cpu_time(proc.cpu_time_ms)),
                )
                .child(
                    div()
                        .w(px(PROC_PID_COL_W))
                        .flex_none()
                        .text_right()
                        .text_xs()
                        .text_color(rgb(OUTLINE))
                        .child(proc.pid.to_string()),
                )
                .child(
                    div()
                        .w(px(PROC_USER_COL_W))
                        .flex_none()
                        .text_right()
                        .text_xs()
                        .text_color(rgb(OUTLINE))
                        .child(truncate(proc.user.as_deref().unwrap_or("-"), 12).to_string()),
                )
                .into_any_element()
        }
        crate::ui::state::ProcDisplayItem::Single { process_index } => {
            let Some(proc) = snap.processes.get(*process_index) else {
                return div().into_any_element();
            };
            let pid = proc.pid;
            let is_selected = selected_pid == Some(pid);

            let ports_badge = render_ports_badge(&proc.ports);

            div()
                .id(SharedString::from(format!("single-row-{pid}")))
                .h(px(PROC_ROW_H))
                .w_full()
                .flex()
                .items_center()
                .px(px(TABLE_PAD_X))
                .border_b_1()
                .border_color(rgba(OUTLINE_VAR, 0.35))
                .cursor_pointer()
                .when(is_selected, |d| d.bg(rgba(PRIMARY, 0.15)))
                .when(!is_selected, |d| d.hover(|h| h.bg(rgb(SURF_LOW))))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.select_proc_item(pid, cx);
                }))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_1p5()
                        .min_w(px(0.))
                        .flex_1()
                        .child(div().w(px(16.)).flex_none()) // 对齐占位
                        .child(process_icon_from_source(
                            proc.icon_source.as_deref(),
                            &proc.name,
                            pid,
                            18.,
                        ))
                        .child(
                            div()
                                .min_w(px(0.))
                                .flex()
                                .items_center()
                                .gap_1p5()
                                .child(
                                    div()
                                        .text_xs()
                                        .font_weight(gpui::FontWeight::MEDIUM)
                                        .text_color(rgb(TEXT))
                                        .child(truncate(&proc.name, 32).to_string()),
                                )
                                .children(ports_badge),
                        ),
                )
                .child(cpu_cell(proc.cpu, PROC_CPU_COL_W))
                .child(
                    div()
                        .w(px(PROC_MEM_COL_W))
                        .flex_none()
                        .text_right()
                        .text_xs()
                        .text_color(rgb(TEXT))
                        .child(fmt_mem(proc.mem_bytes)),
                )
                .child(
                    div()
                        .w(px(PROC_THREADS_COL_W))
                        .flex_none()
                        .text_right()
                        .text_xs()
                        .text_color(rgb(OUTLINE))
                        .child(proc.threads.to_string()),
                )
                .child(
                    div()
                        .w(px(PROC_CPU_TIME_COL_W))
                        .flex_none()
                        .text_right()
                        .text_xs()
                        .text_color(rgb(OUTLINE))
                        .child(crate::core::status::fmt_cpu_time(proc.cpu_time_ms)),
                )
                .child(
                    div()
                        .w(px(PROC_PID_COL_W))
                        .flex_none()
                        .text_right()
                        .text_xs()
                        .text_color(rgb(OUTLINE))
                        .child(proc.pid.to_string()),
                )
                .child(
                    div()
                        .w(px(PROC_USER_COL_W))
                        .flex_none()
                        .text_right()
                        .text_xs()
                        .text_color(rgb(OUTLINE))
                        .child(truncate(proc.user.as_deref().unwrap_or("-"), 12).to_string()),
                )
                .into_any_element()
        }
    }
}

/// 侧滑进程详情抽屉面板（对标 Pulse 设计）
fn render_proc_detail_drawer(
    root: &Root,
    proc: &crate::core::status::ProcInfo,
    cx: &mut Context<Root>,
) -> Div {
    let lang = root.language;
    let pid = proc.pid;
    let start_time = proc.start_time;
    let unique_id = proc.unique_id;
    let name = proc.name.clone();

    div()
        .w(px(330.))
        .flex_none()
        .h_full()
        .bg(rgb(CARD))
        .border_l_1()
        .border_color(rgba(OUTLINE_VAR, 0.4))
        .flex()
        .flex_col()
        .overflow_hidden()
        .child(
            div()
                .flex_1()
                .min_h(px(0.))
                .overflow_hidden()
                .p_4()
                .flex()
                .flex_col()
                .gap_3()
                // 顶部 Header：大图标 + 标题 + 副标题 + 关闭按钮
                .child(
                    div()
                        .flex()
                        .items_start()
                        .justify_between()
                        .gap_2()
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_2p5()
                                .min_w(px(0.))
                                .child(process_icon_from_source(
                                    proc.icon_source.as_deref(),
                                    &proc.name,
                                    pid,
                                    36.,
                                ))
                                .child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .min_w(px(0.))
                                        .child(
                                            div()
                                                .text_sm()
                                                .font_weight(gpui::FontWeight::BOLD)
                                                .text_color(rgb(TEXT))
                                                .truncate()
                                                .child(proc.name.clone()),
                                        )
                                        .child(
                                            div()
                                                .text_xs()
                                                .text_color(rgb(MUTED))
                                                .truncate()
                                                .child(format!(
                                                    "PID {} · {}",
                                                    proc.pid,
                                                    proc.user.as_deref().unwrap_or("-")
                                                )),
                                        ),
                                ),
                        )
                        .child(
                            div()
                                .id("btn-close-detail")
                                .w(px(22.))
                                .h(px(22.))
                                .rounded_md()
                                .flex()
                                .items_center()
                                .justify_center()
                                .cursor_pointer()
                                .hover(|h| h.bg(rgb(SURF_HIGH)))
                                .child(icon_close(MUTED, 14.))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.close_proc_detail(cx);
                                })),
                        ),
                )
                // 快捷操作栏：退出 / 强制退出 / 打开位置
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child({
                            let n = name.clone();
                            div()
                                .id("btn-detail-quit")
                                .flex_1()
                                .py_1()
                                .rounded_md()
                                .bg(rgb(SURF_LOW))
                                .border_1()
                                .border_color(rgba(OUTLINE_VAR, 0.6))
                                .flex()
                                .items_center()
                                .justify_center()
                                .text_xs()
                                .font_weight(gpui::FontWeight::MEDIUM)
                                .text_color(rgb(TEXT))
                                .cursor_pointer()
                                .hover(|h| h.bg(rgb(SURF_HIGH)))
                                .child(tr_status_proc_exit(lang))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.request_kill_process(
                                        pid,
                                        start_time,
                                        unique_id,
                                        n.clone(),
                                        cx,
                                    );
                                }))
                        })
                        .child({
                            let n = name.clone();
                            div()
                                .id("btn-detail-force-quit")
                                .flex_1()
                                .py_1()
                                .rounded_md()
                                .bg(rgba(ERROR, 0.1))
                                .border_1()
                                .border_color(rgba(ERROR, 0.35))
                                .flex()
                                .items_center()
                                .justify_center()
                                .text_xs()
                                .font_weight(gpui::FontWeight::MEDIUM)
                                .text_color(rgb(ERROR))
                                .cursor_pointer()
                                .hover(|h| h.bg(rgba(ERROR, 0.2)))
                                .child(tr_status_proc_force_exit(lang))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.force_kill_process(
                                        pid,
                                        start_time,
                                        unique_id,
                                        n.clone(),
                                        cx,
                                    );
                                }))
                        })
                        .when(proc.exe_path.is_some(), |d| {
                            let path_buf =
                                std::path::PathBuf::from(proc.exe_path.as_ref().unwrap());
                            d.child(
                                div()
                                    .id("btn-detail-reveal")
                                    .w(px(28.))
                                    .h(px(28.))
                                    .rounded_md()
                                    .bg(rgb(SURF_LOW))
                                    .border_1()
                                    .border_color(rgba(OUTLINE_VAR, 0.6))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .cursor_pointer()
                                    .hover(|h| h.bg(rgb(SURF_HIGH)))
                                    .child(icon_folder(MUTED, 13.))
                                    .tooltip(text_tooltip(tr_status_proc_reveal(lang)))
                                    .on_click(cx.listener(move |_, _, _, _| {
                                        crate::platform::reveal_in_explorer(&path_buf);
                                    })),
                            )
                        }),
                )
                // CPU 性能卡片与走势
                .child(
                    div()
                        .rounded_lg()
                        .bg(rgba(PRIMARY, 0.08))
                        .border_1()
                        .border_color(rgba(PRIMARY, 0.18))
                        .p_2p5()
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
                                        .text_xs()
                                        .font_weight(gpui::FontWeight::BOLD)
                                        .text_color(rgb(PRIMARY))
                                        .child("CPU"),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .font_weight(gpui::FontWeight::BOLD)
                                        .text_color(rgb(TEXT))
                                        .child(format!("{:.1}%", proc.cpu)),
                                ),
                        )
                        .child(render_mini_sparkline(
                            &root.monitor.selected_proc_cpu_history,
                            100.0,
                            PRIMARY,
                            24.,
                        )),
                )
                // 内存性能卡片与走势
                .child(
                    div()
                        .rounded_lg()
                        .bg(rgba(SUCCESS_BRIGHT, 0.08))
                        .border_1()
                        .border_color(rgba(SUCCESS_BRIGHT, 0.2))
                        .p_2p5()
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
                                        .text_xs()
                                        .font_weight(gpui::FontWeight::BOLD)
                                        .text_color(rgb(SUCCESS))
                                        .child(tr_status_card_memory(lang)),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .font_weight(gpui::FontWeight::BOLD)
                                        .text_color(rgb(TEXT))
                                        .child(fmt_mem(proc.mem_bytes)),
                                ),
                        )
                        .child(render_mini_mem_sparkline(
                            &root.monitor.selected_proc_mem_history,
                            SUCCESS_BRIGHT,
                            24.,
                        )),
                )
                // 详细信息列表
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .pt_1()
                        .child(
                            div()
                                .text_xs()
                                .font_weight(gpui::FontWeight::BOLD)
                                .text_color(rgb(MUTED))
                                .mb_1()
                                .child(tr_status_proc_detail_title(lang)),
                        )
                        .child(detail_kv(
                            tr_th_cpu_time(lang),
                            &crate::core::status::fmt_cpu_time(proc.cpu_time_ms),
                        ))
                        .child(detail_kv(tr_th_threads(lang), &proc.threads.to_string()))
                        .child(detail_kv(
                            tr_status_proc_rss(lang),
                            &fmt_mem(proc.mem_bytes),
                        ))
                        .child(detail_kv(
                            tr_status_proc_vms(lang),
                            &fmt_mem(proc.virtual_mem_bytes),
                        ))
                        .child(detail_kv(
                            tr_status_proc_disk_read(lang),
                            &format!(
                                "{} · {}",
                                fmt_size(proc.disk_read_bps as u64) + "/s",
                                fmt_mem(proc.disk_read_bytes)
                            ),
                        ))
                        .child(detail_kv(
                            tr_status_proc_disk_write(lang),
                            &format!(
                                "{} · {}",
                                fmt_size(proc.disk_written_bps as u64) + "/s",
                                fmt_mem(proc.disk_written_bytes)
                            ),
                        ))
                        .child(detail_kv(
                            tr_status_proc_start_time(lang),
                            &fmt_proc_start_time(proc.start_time, lang),
                        ))
                        .child(detail_kv(
                            tr_status_proc_status(lang),
                            localize_proc_status(&proc.status_text, lang),
                        ))
                        .when(proc.parent_pid.is_some(), |d| {
                            let ppid = proc.parent_pid.unwrap();
                            let pname = proc.parent_name.as_deref().unwrap_or("process");
                            d.child(
                                div()
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .text_xs()
                                    .py(px(2.))
                                    .child(
                                        div()
                                            .text_color(rgb(OUTLINE))
                                            .child(tr_status_proc_parent(lang)),
                                    )
                                    .child(
                                        div()
                                            .id(SharedString::from(format!("parent-proc-{ppid}")))
                                            .flex()
                                            .items_center()
                                            .gap_1()
                                            .text_color(rgb(PRIMARY))
                                            .cursor_pointer()
                                            .hover(|h| h.underline())
                                            .child(icon_folder(PRIMARY, 11.))
                                            .child(format!("{pname} {ppid}"))
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                this.select_proc_item(ppid, cx);
                                            })),
                                    ),
                            )
                        }),
                )
                // 命令行卡片
                .when(proc.cmd_line.is_some(), |d| {
                    let cmd = proc.cmd_line.as_ref().unwrap().clone();
                    let cmd_clone = cmd.clone();
                    let copied = root.monitor.cmd_copied_feedback;
                    d.child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1p5()
                            .pt_1()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .child(
                                        div()
                                            .text_xs()
                                            .font_weight(gpui::FontWeight::BOLD)
                                            .text_color(rgb(MUTED))
                                            .child(tr_status_proc_cmd(lang)),
                                    )
                                    .child(
                                        div()
                                            .id("btn-copy-cmd")
                                            .flex()
                                            .items_center()
                                            .gap_1()
                                            .px_1p5()
                                            .py(px(1.))
                                            .rounded_md()
                                            .bg(rgb(SURF_LOW))
                                            .hover(|h| h.bg(rgb(SURF_HIGH)))
                                            .cursor_pointer()
                                            .child(icon_copy(
                                                if copied { PRIMARY } else { MUTED },
                                                11.,
                                            ))
                                            .child(
                                                div()
                                                    .text_xs()
                                                    .text_color(rgb(if copied {
                                                        PRIMARY
                                                    } else {
                                                        MUTED
                                                    }))
                                                    .child(if copied {
                                                        tr_status_proc_copied(lang)
                                                    } else {
                                                        tr_status_proc_copy_cmd(lang)
                                                    }),
                                            )
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                this.copy_proc_cmd(cmd_clone.clone(), cx);
                                            })),
                                    ),
                            )
                            .child(
                                div()
                                    .p_2()
                                    .rounded_md()
                                    .bg(rgb(SURF_LOW))
                                    .border_1()
                                    .border_color(rgba(OUTLINE_VAR, 0.4))
                                    .text_xs()
                                    .text_color(rgb(TEXT))
                                    .child(truncate(&cmd, 160).to_string()),
                            ),
                    )
                }),
        )
}
