/// 端口搜索匹配条目
struct PortMatchItem<'a> {
    port: u16,
    proc_name: &'a str,
    icon_source: Option<&'a std::path::Path>,
    pid: u32,
    is_system: bool,
}

/// 渲染微型进程图标（用于 Popover 条目内）
fn render_popover_icon(
    _proc_name: &str,
    icon_source: Option<&std::path::Path>,
    pid: u32,
    is_system: bool,
) -> AnyElement {
    const SIZE: f32 = 16.0;
    if let Some(image) = icon_source.and_then(try_get_icon) {
        let src = ImageSource::from(image);
        return img(src)
            .id(SharedString::from(format!("pop-icon-{pid}")))
            .w(px(SIZE))
            .h(px(SIZE))
            .rounded(px(3.))
            .flex_none()
            .into_any_element();
    }
    if is_system {
        div()
            .w(px(SIZE))
            .h(px(SIZE))
            .rounded(px(3.))
            .bg(rgb(SURF_HIGH))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .child(icon_gear(MUTED, 10.))
            .into_any_element()
    } else {
        div()
            .w(px(SIZE))
            .h(px(SIZE))
            .rounded(px(3.))
            .bg(rgba(PRIMARY, 0.12))
            .border_1()
            .border_color(rgba(PRIMARY, 0.25))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .child(icon_apps(PRIMARY, 10.))
            .into_any_element()
    }
}

/// 搜索框正下方的端口智能匹配浮层（Popover）
fn matching_ports<'a>(snap: &'a StatusSnapshot, search_text: &str) -> Vec<PortMatchItem<'a>> {
    // 1. 搜集所有正在监听/打开网络端口的进程条目
    let mut entries: Vec<PortMatchItem> = Vec::new();
    for proc in &snap.processes {
        let is_system = proc.pid <= 4
            || proc.name.eq_ignore_ascii_case("system")
            || proc.name.eq_ignore_ascii_case("registry")
            || proc.name.eq_ignore_ascii_case("smss.exe")
            || proc.name.eq_ignore_ascii_case("csrss.exe")
            || proc.name.eq_ignore_ascii_case("services.exe")
            || proc.name.eq_ignore_ascii_case("svchost.exe")
            || proc.name.eq_ignore_ascii_case("lsass.exe");

        for &port in &proc.ports {
            entries.push(PortMatchItem {
                port,
                proc_name: &proc.name,
                icon_source: proc.icon_source.as_deref(),
                pid: proc.pid,
                is_system,
            });
        }
    }
    // 按端口号升序排列
    entries.sort_by_key(|e| e.port);

    // 2. 判断输入是否含有端口前缀 ':'
    let (is_port_query, sub_query) = if let Some(idx) = search_text.find(':') {
        let sub = search_text[idx + 1..].trim();
        (true, sub)
    } else {
        (false, "")
    };

    // 3. 动态过滤
    if is_port_query && !sub_query.is_empty() {
        entries
            .into_iter()
            .filter(|e| {
                e.port.to_string().starts_with(sub_query)
                    || e.port.to_string().contains(sub_query)
                    || e.proc_name
                        .to_lowercase()
                        .contains(&sub_query.to_lowercase())
            })
            .collect()
    } else {
        entries
    }
}

fn search_syntax_items(lang: Language) -> [(&'static str, &'static str, &'static str); 6] {
    [
        (
            "msedge, \"NVIDIA Overlay\"",
            tr_status_search_hint_multi_desc(lang),
            "msedge, \"NVIDIA Overlay\"",
        ),
        ("cpu >", tr_status_search_hint_cpu_desc(lang), "cpu > "),
        ("mem >", tr_status_search_hint_mem_desc(lang), "mem > "),
        ("user:", tr_status_search_hint_user_desc(lang), "user:"),
        (":3000", tr_status_search_hint_port_desc(lang), ":"),
        (
            "-helper",
            tr_status_search_hint_exclude_desc(lang),
            "-helper",
        ),
    ]
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
enum CandidateContent {
    #[default]
    Query,
    Syntax,
    Process {
        source: Option<std::path::PathBuf>,
        pid: u32,
        system: bool,
    },
    Port {
        source: Option<std::path::PathBuf>,
        pid: u32,
        system: bool,
        port: u16,
    },
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct SearchCandidate {
    pub id: String,
    pub query: String,
    pub keep_open: bool,
    label: String,
    detail: String,
    content: CandidateContent,
}

impl PopoverCandidate for SearchCandidate {
    fn id(&self) -> &str {
        &self.id
    }
}

pub(crate) fn status_search_candidates(root: &Root) -> Vec<SearchCandidate> {
    let Some(snap) = root.monitor.snapshot.as_ref() else {
        return Vec::new();
    };
    search_candidates(
        snap,
        &root.monitor.proc_view,
        &root.monitor.input.text,
        root.language,
    )
}

fn search_candidates(
    snap: &StatusSnapshot,
    view: &[usize],
    text: &str,
    lang: Language,
) -> Vec<SearchCandidate> {
    if text.trim().starts_with(':') || text.trim().to_lowercase().starts_with("port:") {
        return matching_ports(snap, text)
            .into_iter()
            .take(80)
            .map(|p| SearchCandidate {
                id: format!("pop-port-{}-{}", p.port, p.pid),
                query: format!(":{}", p.port),
                keep_open: false,
                label: p.proc_name.into(),
                detail: format!("#{}", p.pid),
                content: CandidateContent::Port {
                    source: p.icon_source.map(std::path::Path::to_path_buf),
                    pid: p.pid,
                    system: p.is_system,
                    port: p.port,
                },
            })
            .collect();
    }
    let mut candidates = Vec::new();
    if !text.trim().is_empty() {
        candidates.push(SearchCandidate {
            id: "status-search-current-query".into(),
            query: text.into(),
            keep_open: false,
            label: tr_status_search_query(lang, text),
            detail: String::new(),
            content: CandidateContent::Query,
        });
        candidates.extend(
            process_search_groups(&snap.processes, view)
                .into_iter()
                .take(8)
                .map(|(p, count)| SearchCandidate {
                    id: format!("status-search-process-{}", p.name.to_lowercase()),
                    query: format!("\"{}\"", p.name),
                    keep_open: false,
                    label: p.name.clone(),
                    detail: tr_status_search_process_count(lang, count),
                    content: CandidateContent::Process {
                        source: p.icon_source.clone(),
                        pid: p.pid,
                        system: p.pid <= 4,
                    },
                }),
        );
    }
    candidates.extend(
        search_syntax_items(lang)
            .into_iter()
            .enumerate()
            .filter_map(|(i, (example, desc, query))| {
                syntax_completion(query, text).map(|completion| (i, example, desc, completion))
            })
            .map(|(i, example, desc, query)| SearchCandidate {
                id: format!("syntax-hint-{i}"),
                label: if example.starts_with("mem") || example.starts_with("cpu") {
                    query.trim_end().to_owned()
                } else {
                    example.into()
                },
                query,
                keep_open: true,
                detail: desc.into(),
                content: CandidateContent::Syntax,
            }),
    );
    let lower = text.trim().to_ascii_lowercase();
    if ["mem", "cpu", "user"]
        .iter()
        .any(|field| lower.starts_with(field))
    {
        candidates.sort_by_key(|item| item.content != CandidateContent::Syntax);
    }
    candidates
}

#[cfg(test)]
fn candidate_index(selected: Option<&str>, candidates: &[SearchCandidate]) -> Option<usize> {
    crate::ui::components::popover::candidate_index(selected, candidates)
}

#[cfg(test)]
fn next_search_candidate(
    selected: Option<&str>,
    candidates: &[SearchCandidate],
    backwards: bool,
) -> Option<usize> {
    crate::ui::components::popover::next_candidate(selected, candidates, backwards)
}

fn syntax_completion(template: &str, query: &str) -> Option<String> {
    let compact = query
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect::<String>()
        .to_lowercase();
    for field in ["mem", "cpu"] {
        if template.starts_with(field) {
            if let Some(rest) = compact.strip_prefix(field) {
                return match rest {
                    "" => Some(format!("{field} > ")),
                    ">" | "<" | ">=" | "<=" => Some(format!("{field} {rest} ")),
                    _ => None,
                };
            }
        }
    }
    template
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect::<String>()
        .to_lowercase()
        .contains(&compact)
        .then(|| template.to_owned())
}

#[cfg(test)]
fn syntax_hint_matches(example: &str, query: &str) -> bool {
    syntax_completion(example, query).is_some()
}

fn process_search_groups<'a>(
    processes: &'a [crate::core::status::ProcInfo],
    view: &[usize],
) -> Vec<(&'a crate::core::status::ProcInfo, usize)> {
    let mut groups = std::collections::BTreeMap::new();
    for proc in view.iter().filter_map(|&i| processes.get(i)) {
        let group = groups.entry(proc.name.to_lowercase()).or_insert((proc, 0));
        group.1 += 1;
    }
    groups.into_values().collect()
}

fn render_status_search_popover(
    root: &Root,
    snap: &StatusSnapshot,
    cx: &mut Context<Root>,
) -> AnyElement {
    let lang = root.language;
    let text = &root.monitor.input.text;
    let port_query =
        text.trim().starts_with(':') || text.trim().to_lowercase().starts_with("port:");
    let candidates = status_search_candidates(root);
    let selected = root.monitor.search_popover.selection(&candidates);
    let rows = candidates
        .into_iter()
        .enumerate()
        .map(|(index, item)| {
            let mut row = popover_row(
                item.id.clone().into(),
                selected == Some(index),
                |this: &mut Root| &mut this.monitor.search_popover,
                cx,
            );
            match &item.content {
                CandidateContent::Query => {
                    row = row.child(icon_search(PRIMARY, 12.));
                }
                CandidateContent::Syntax => {}
                CandidateContent::Process {
                    source,
                    pid,
                    system,
                } => {
                    row = row.child(render_popover_icon(
                        &item.label,
                        source.as_deref(),
                        *pid,
                        *system,
                    ));
                }
                CandidateContent::Port {
                    source,
                    pid,
                    system,
                    port,
                } => {
                    row = row
                        .child(
                            div()
                                .flex_none()
                                .px_1p5()
                                .rounded_md()
                                .text_xs()
                                .text_color(rgb(PRIMARY))
                                .child(format!(":{port}")),
                        )
                        .child(render_popover_icon(
                            &item.label,
                            source.as_deref(),
                            *pid,
                            *system,
                        ));
                }
            }
            row = row
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .truncate()
                        .text_xs()
                        .text_color(rgb(TEXT))
                        .child(item.label.clone()),
                )
                .when(!item.detail.is_empty(), |d| {
                    d.child(
                        div()
                            .flex_none()
                            .text_xs()
                            .text_color(rgb(OUTLINE))
                            .child(item.detail.clone()),
                    )
                });
            row.on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(move |this, _, _, cx| {
                    this.status_choose_search_candidate(&item.query, item.keep_open, cx);
                    cx.stop_propagation();
                }),
            )
            .into_any_element()
        })
        .collect::<Vec<_>>();
    let empty = rows.is_empty();
    let sub_query = text.split_once(':').map_or("", |(_, q)| q.trim());
    let list = popover_list(
        "status-search-candidates".into(),
        &root.monitor.search_popover,
        240.,
    )
    .children(rows)
    .when(empty, |d| {
        d.child(
            div()
                .px_2()
                .py_4()
                .text_xs()
                .text_color(rgb(OUTLINE))
                .child(if sub_query.is_empty() {
                    tr_status_popover_no_ports(lang).to_string()
                } else {
                    tr_status_popover_no_matching_ports(lang, sub_query)
                }),
        )
    });
    let header = div()
        .flex_none()
        .px_3()
        .py_2()
        .flex()
        .items_center()
        .justify_between()
        .text_xs()
        .font_weight(gpui::FontWeight::BOLD)
        .text_color(rgb(MUTED))
        .child(if port_query {
            tr_status_popover_ports(lang)
        } else {
            tr_status_search_hint_title(lang)
        })
        .when(port_query, |d| {
            d.child(matching_ports(snap, text).len().to_string())
        });
    crate::ui::components::overlay::OverlayPolicy::POPUP.layer_at(
        popover_surface(
            PopoverSpec {
                id: "status-proc-search-popover".into(),
                width: 320.,
                top: 34.,
                max_height: 320.,
                anchor_bounds: root.monitor.input.bounds,
            },
            |this: &mut Root, window, cx| {
                if this.dismiss_overlay_outside(crate::ui::shell::OverlayId::StatusSearch, cx)
                    && this.monitor.input.focus_handle.is_focused(window)
                {
                    window.blur();
                }
                cx.notify();
            },
            cx,
        )
        .child(header)
        .child(list)
        .when(port_query, |d| {
            d.child(
                div()
                    .flex_none()
                    .px_3()
                    .py_2()
                    .text_xs()
                    .text_color(rgb(OUTLINE))
                    .child(tr_status_popover_tip(lang)),
            )
        }),
        root.overlays
            .priority(crate::ui::shell::OverlayId::StatusSearch),
    )
}
