/// 进程表。九百来个进程全量铺开会让整页掉到个位数帧率，所以和软件管理表
/// 一样走 `uniform_list`：只构造视口内的十几行，配一条自绘滚动条。
fn render_process_table(
    root: &Root,
    snap: &StatusSnapshot,
    window: &mut Window,
    cx: &mut Context<Root>,
) -> Div {
    let lang = root.language;
    let sort = root.monitor.proc_sort;
    let row_count = root.monitor.proc_display_items.len();

    let has_scrollbar = scrollbar_present(
        &root.monitor.proc_scroll.0.borrow().base_handle,
        STATUS_PROCESS_TABLE_H,
        row_count as f32 * PROC_ROW_H,
    );
    let list = virtual_list(
        SharedString::from("status-proc-rows"),
        row_count,
        root.monitor.proc_scroll.clone(),
        cx.processor(|this, range: std::ops::Range<usize>, _window, cx| {
            let Some(snap) = this.monitor.snapshot.clone() else {
                return Vec::new();
            };
            let picked_items: Vec<crate::ui::state::ProcDisplayItem> = range
                .filter_map(|i| this.monitor.proc_display_items.get(i).cloned())
                .collect();
            // 图标按可见区间懒加载
            let mut icon_paths = Vec::new();
            for item in &picked_items {
                match item {
                    crate::ui::state::ProcDisplayItem::GroupHeader { icon_source, .. } => {
                        if let Some(src) = icon_source {
                            icon_paths.push(src.clone());
                        }
                    }
                    crate::ui::state::ProcDisplayItem::GroupChild { process_index, .. }
                    | crate::ui::state::ProcDisplayItem::Single { process_index } => {
                        if let Some(p) = snap.processes.get(*process_index) {
                            if let Some(src) = &p.icon_source {
                                icon_paths.push(src.clone());
                            }
                        }
                    }
                }
            }
            this.load_visible_process_icons(icon_paths, cx);
            picked_items
                .iter()
                .map(|item| render_process_row(this, item, &snap, cx))
                .collect()
        }),
    )
    .size_full();

    // 搜索框状态
    let search_focused = root.monitor.input.focus_handle.is_focused(window);
    let search_text = root.monitor.input.text.clone();
    let status_focus_handle = root.monitor.input.focus_handle.clone();
    let search_sel = root.monitor.input.sel.clone();
    let search_marked = root.monitor.input.marked.clone();
    let sel = crate::ui::text_input::clamp_to_boundary(&search_text, search_sel);

    let search_box_el = crate::ui::components::search_input::search_box(
        crate::ui::components::search_input::SearchBoxSpec {
            id: SharedString::from("status-proc-search-box"),
            focus_handle: &status_focus_handle,
            text: &search_text,
            placeholder: SharedString::from(tr_status_proc_search_placeholder(lang)),
            selection: sel,
            marked: search_marked,
            width: 210.,
            height: 28.,
            font_size: 11.5,
            cursor_h: 12.,
            focused: search_focused,
            cursor_visible: root.cursor_blink_visible,
            binding: crate::ui::adapters::input::STATUS_INPUT,
        },
        |this, cx| this.status_search_clear(cx),
        |this, cx| this.status_search_escape(cx),
        cx,
    );

    // 分类胶囊按钮组
    let current_tab = root.monitor.filter_tab;
    let filter_tabs = [
        (
            crate::core::status::ProcFilterTab::All,
            tr_status_tab_all(lang),
        ),
        (
            crate::core::status::ProcFilterTab::App,
            tr_status_tab_app(lang),
        ),
        (
            crate::core::status::ProcFilterTab::User,
            tr_status_tab_user(lang),
        ),
        (
            crate::core::status::ProcFilterTab::System,
            tr_status_tab_system(lang),
        ),
    ];
    let tab_buttons = SegmentedControl::new(
        "proc-tab",
        current_tab,
        cx.listener(|this, tab: &crate::core::status::ProcFilterTab, _, cx| {
            this.status_set_filter_tab(*tab, cx);
        }),
    )
    .style(SegmentStyle::Chip)
    .chip_look(ChipLook {
        pad_x: ChipPadX::X2p5,
        pad_y: 3.,
        radius: ChipRadius::Md,
        rest_border: Some(0.7),
        weight_rest: gpui::FontWeight::MEDIUM,
        hover_fg: Some(TEXT),
        ..ChipLook::CHIP
    })
    .gap(4.)
    .items(
        filter_tabs
            .into_iter()
            .map(|(tab, label)| SegmentItem::new(tab, label)),
    );

    let popover_el = root
        .monitor
        .search_popover
        .is_open(search_focused)
        .then(|| render_status_search_popover(root, snap, cx));

    let search_box_container = div()
        .relative()
        .child(search_box_el)
        .children(popover_el)
        .on_mouse_down(
            gpui::MouseButton::Left,
            cx.listener(|this, _, _, cx| {
                if this.monitor.search_popover.dismissed() {
                    this.monitor.search_popover.reopen();
                    cx.notify();
                }
            }),
        );

    let selected_proc = root
        .monitor
        .selected_proc_pid
        .and_then(|pid| snap.processes.iter().find(|p| p.pid == pid));

    let (filtered_cpu, filtered_mem): (f32, u64) = root
        .monitor
        .proc_view
        .iter()
        .filter_map(|&i| snap.processes.get(i))
        .fold((0.0, 0), |(cpu_acc, mem_acc), p| {
            (cpu_acc + p.cpu, mem_acc + p.mem_bytes)
        });

    let table_pane = div()
        .flex_1()
        .min_w(px(0.))
        .h_full()
        .flex()
        .flex_col()
        .child(
            div()
                .pl(px(TABLE_PAD_X))
                .pr(px(TABLE_PAD_X
                    + if has_scrollbar {
                        crate::ui::components::scroll::SCROLLBAR_W
                    } else {
                        0.
                    }))
                .py_2()
                .bg(rgb(SURF_LOW))
                .border_b_1()
                .border_color(rgba(OUTLINE_VAR, 0.4))
                .flex()
                .items_center()
                .child(sort_header(
                    tr_th_proc_name(lang).to_string(),
                    ProcSortKey::Name,
                    None,
                    sort,
                    cx,
                ))
                .child(sort_header(
                    tr_th_cpu_short(lang).to_string(),
                    ProcSortKey::Cpu,
                    Some(PROC_CPU_COL_W),
                    sort,
                    cx,
                ))
                .child(sort_header(
                    tr_th_mem_short(lang).to_string(),
                    ProcSortKey::Memory,
                    Some(PROC_MEM_COL_W),
                    sort,
                    cx,
                ))
                .child(sort_header(
                    tr_th_threads(lang).to_string(),
                    ProcSortKey::Threads,
                    Some(PROC_THREADS_COL_W),
                    sort,
                    cx,
                ))
                .child(sort_header(
                    tr_th_cpu_time(lang).to_string(),
                    ProcSortKey::CpuTime,
                    Some(PROC_CPU_TIME_COL_W),
                    sort,
                    cx,
                ))
                .child(sort_header(
                    tr_th_pid(lang).to_string(),
                    ProcSortKey::Pid,
                    Some(PROC_PID_COL_W),
                    sort,
                    cx,
                ))
                .child(sort_header(
                    tr_th_user(lang).to_string(),
                    ProcSortKey::User,
                    Some(PROC_USER_COL_W),
                    sort,
                    cx,
                )),
        )
        .child(ScrollArea::new(
            "status-scroll",
            root.monitor.proc_scroll.0.borrow().base_handle.clone(),
            STATUS_PROCESS_TABLE_H,
            row_count as f32 * PROC_ROW_H,
            list,
        ))
        // 底部状态栏：统计显示进程数及已筛选进程的总消耗
        .child(
            div()
                .px(px(TABLE_PAD_X))
                .py_1p5()
                .bg(rgb(SURF_LOW))
                .border_t_1()
                .border_color(rgba(OUTLINE_VAR, 0.4))
                .flex()
                .items_center()
                .justify_between()
                .text_xs()
                .text_color(rgb(MUTED))
                .child(tr_status_proc_count_detail(
                    lang,
                    root.monitor.proc_view.len(),
                    snap.process_count,
                ))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_3()
                        .child(format!("CPU: {:.1}%", filtered_cpu))
                        .child(format!(
                            "{}: {}",
                            tr_status_card_memory(lang),
                            fmt_mem(filtered_mem)
                        )),
                ),
        );

    card()
        .w_full()
        .flex_1()
        .min_h(px(0.))
        .overflow_hidden()
        .flex()
        .flex_col()
        // 顶部操作工具栏：标题与统计在左，搜索框与分类药丸在右
        .child(
            div()
                .relative()
                .px(px(TABLE_PAD_X))
                .py_2p5()
                .bg(rgb(CARD))
                .border_b_1()
                .border_color(rgba(OUTLINE_VAR, 0.3))
                .flex()
                .items_center()
                .justify_between()
                .gap_3()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2p5()
                        .child(
                            div()
                                .text_sm()
                                .font_weight(gpui::FontWeight::BOLD)
                                .text_color(rgb(TEXT))
                                .child(tr_status_processes(lang)),
                        )
                        .child(div().text_xs().text_color(rgb(MUTED)).child(
                            tr_status_proc_count_summary(
                                lang,
                                root.monitor.proc_view.len(),
                                snap.process_count,
                            ),
                        )),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(search_box_container)
                        .child(tab_buttons),
                ),
        )
        .child(
            div()
                .w_full()
                .flex_1()
                .min_h(px(0.))
                .flex()
                .flex_row()
                .child(table_pane)
                .when_some(selected_proc, |d, p| {
                    d.child(render_proc_detail_drawer(root, p, cx))
                }),
        )
}
