//! 状态监控视图（Stitch「Dashboard - System Status with Fan Control」设计：
//! 健康概览 / CPU / GPU / 内存 / 磁盘 / 网络 / 电池 / 风扇温度八卡片
//! + 活动进程表）
//!
//! 卡片数量按硬件有无浮动：读不到 GPU 的机器少一张、没有电池的台式机少
//! 一张。少的那几格由 `card_rows` 补等宽占位，两排卡片的尺寸必须一致——
//! 大小不一的网格看起来就像布局坏了。

use crate::core::i18n::Language;
use crate::core::model::{commas, fmt_mem, fmt_size, truncate};
use crate::core::status::{gpu_labels, FanMode, StatusSnapshot, STATUS_PROCESS_TABLE_H};
use crate::ui::app_icons::try_get_icon;
use crate::ui::components::badge::{Badge, BadgeShape, BadgeSize, BadgeTone};
use crate::ui::components::cards::card;
use crate::ui::components::controls::page_heading;
use crate::ui::components::donut::{render_donut, DonutSegment};
use crate::ui::components::icons::*;
use crate::ui::components::meter::{Meter, MeterHeight};
use crate::ui::components::popover::{
    popover_list, popover_row, popover_surface, PopoverCandidate, PopoverSpec,
};
use crate::ui::components::scroll_area::{scrollbar_present, virtual_list, ScrollArea};
use crate::ui::components::segmented::{
    ChipLook, ChipPadX, ChipRadius, SegmentItem, SegmentStyle, SegmentedControl,
};
use crate::ui::components::text_tooltip;
use crate::ui::i18n::*;
use crate::ui::state::{ProcSort, ProcSortKey, StatusTab, STATUS_HISTORY_LEN};
use crate::ui::theme::*;
use crate::ui::Root;
use gpui::{
    div, img, prelude::*, px, relative, rgb, AnyElement, Context, Div, ImageSource, SharedString,
    Stateful, Window,
};

include!("status/cards.rs");
include!("status/process_rows.rs");
include!("status/suggestions.rs");
include!("status/process_table.rs");

/// 状态监控页入口。
pub fn render_status_view(root: &Root, window: &mut Window, cx: &mut Context<Root>) -> AnyElement {
    let lang = root.language;

    let Some(snap) = &root.monitor.snapshot else {
        return div()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_3()
            .child(icon_pulse(PRIMARY, 28.))
            .child(
                div()
                    .text_sm()
                    .text_color(rgb(MUTED))
                    .child(tr_status_loading(lang).to_string()),
            )
            .into_any_element();
    };

    match root.monitor.tab {
        StatusTab::Overview => div()
            .id("status-overview-scroll")
            .size_full()
            .min_w(px(0.))
            .overflow_scroll()
            .flex()
            .flex_col()
            .p_5()
            .gap_4()
            .child(page_heading(
                tr_status_overview_heading(lang),
                tr_status_overview_subheading(lang),
            ))
            .child(
                div().flex().flex_col().children(card_rows(
                    [
                        Some(render_health_card(root, snap)),
                        Some(render_cpu_card(snap, &root.monitor.cpu_history, root)),
                        (!snap.gpus.is_empty()).then(|| {
                            let selected = root
                                .monitor
                                .gpu_selected
                                .as_ref()
                                .and_then(|id| snap.gpus.iter().position(|g| &g.id == id))
                                .unwrap_or(0);
                            let history = root
                                .monitor
                                .gpu_history
                                .get(&snap.gpus[selected].id)
                                .map(Vec::as_slice)
                                .unwrap_or(&[]);
                            render_gpu_card(&snap.gpus, selected, history, root, cx)
                        }),
                        Some(render_memory_card(snap, root)),
                        Some(render_disk_card(root)),
                        Some(render_network_card(snap, root)),
                        snap.battery
                            .as_ref()
                            .map(|bat| render_battery_card(bat, root)),
                        Some(render_fan_card(snap, root, cx)),
                    ]
                    .into_iter()
                    .flatten()
                    .collect(),
                )),
            )
            .into_any_element(),
        StatusTab::Processes => {
            let total_threads: u64 = snap.processes.iter().map(|p| p.threads as u64).sum();
            div()
                .id("status-processes-page")
                .size_full()
                .min_w(px(0.))
                .p_5()
                .flex()
                .flex_col()
                .gap_3()
                .child(
                    div().flex().items_baseline().justify_between().child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(
                                div()
                                    .text_base()
                                    .font_weight(gpui::FontWeight::BOLD)
                                    .text_color(rgb(TEXT))
                                    .child(tr_status_proc_heading(lang)),
                            )
                            .child(div().text_xs().text_color(rgb(MUTED)).child(format!(
                                "{} · {}",
                                tr_status_proc_subheading(lang, snap.process_count),
                                tr_status_thread_count(lang, total_threads),
                            ))),
                    ),
                )
                .child(render_process_table(root, snap, window, cx))
                .into_any_element()
        }
    }
}

#[cfg(test)]
mod search_popover_tests {
    use super::*;

    #[test]
    fn process_suggestions_group_only_filtered_processes() {
        let proc = crate::core::status::ProcInfo {
            pid: 100,
            start_time: 0,
            unique_id: None,
            name: "msedge".into(),
            cpu: 0.,
            mem_bytes: 0,
            icon_source: None,
            ports: vec![],
            threads: 0,
            cpu_time_ms: 0,
            user: None,
            exe_path: None,
            ..Default::default()
        };
        let mut same_name = proc.clone();
        same_name.pid = 101;
        same_name.name = "MSEDGE".into();
        let mut other = proc.clone();
        other.pid = 102;
        other.name = "Memory Compression".into();
        let processes = vec![proc, same_name, other];
        let groups = process_search_groups(&processes, &[0, 1]);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].1, 2);
        assert_eq!(groups[0].0.name, "msedge");
        let groups = process_search_groups(&processes, &[2]);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].0.name, "Memory Compression");
        assert!(process_search_groups(&processes, &[]).is_empty());
    }

    #[test]
    fn popover_keyboard_navigation_wraps_and_survives_filter_changes() {
        let candidates = vec![
            SearchCandidate {
                id: "query".into(),
                query: "mem".into(),
                keep_open: false,
                ..Default::default()
            },
            SearchCandidate {
                id: "process".into(),
                query: "\"Memory Compression\"".into(),
                keep_open: false,
                ..Default::default()
            },
            SearchCandidate {
                id: "syntax".into(),
                query: "mem > 1 GB".into(),
                keep_open: true,
                ..Default::default()
            },
        ];
        assert_eq!(candidate_index(None, &candidates), Some(0));
        assert_eq!(next_search_candidate(None, &candidates, false), Some(1));
        assert_eq!(
            next_search_candidate(Some("query"), &candidates, true),
            Some(2)
        );
        assert_eq!(
            next_search_candidate(Some("syntax"), &candidates, false),
            Some(0)
        );
        assert_eq!(candidate_index(Some("process"), &candidates[1..]), Some(0));
        assert_eq!(
            candidate_index(Some("vanished-process"), &candidates),
            Some(0)
        );
        assert_eq!(next_search_candidate(None, &[], true), None);
        assert_eq!(
            next_search_candidate(None, &candidates[..1], false),
            Some(0)
        );
        let chosen = &candidates[candidate_index(Some("syntax"), &candidates).unwrap()];
        assert_eq!(chosen.query, "mem > 1 GB");
        assert!(chosen.keep_open);
        assert!(!candidates[1].keep_open);
    }

    #[test]
    fn typing_filters_syntax_suggestions() {
        assert!(syntax_hint_matches("mem > 1 GB", "mem"));
        assert!(!syntax_hint_matches("cpu > 10", "mem"));
        assert!(syntax_hint_matches("msedge, \"NVIDIA Overlay\"", "MS"));
        assert!(syntax_hint_matches("cpu > 10", ""));
        assert!(!syntax_hint_matches("mem > 1 GB", "no-such-process"));
    }

    #[test]
    fn search_completion_leaves_threshold_values_to_the_user() {
        let snap = StatusSnapshot::default();
        for (text, expected) in [
            ("mem", "mem > "),
            ("mem>", "mem > "),
            ("mem<=", "mem <= "),
            ("cpu", "cpu > "),
            ("user", "user:"),
        ] {
            let items = search_candidates(&snap, &[], text, Language::En);
            let item = items
                .iter()
                .find(|item| item.content == CandidateContent::Syntax)
                .expect(text);
            assert_eq!(items[0].content, CandidateContent::Syntax, "{text}");
            assert_eq!(item.query, expected, "{text}");
            assert!(item.keep_open);
        }
        for text in ["mem>1", "mem > 512 MB", "cpu>20", "user:SYSTEM"] {
            let items = search_candidates(&snap, &[], text, Language::En);
            assert!(
                !items
                    .iter()
                    .any(|item| item.content == CandidateContent::Syntax),
                "completed input must not be replaced: {text}"
            );
        }
    }
}
