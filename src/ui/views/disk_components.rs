//! 磁盘透镜页面的局部组件

use crate::core::model::{fmt_size, truncate};
use crate::ui::components::badge::{Badge, BadgeTone};
use crate::ui::components::list_row::{ListRow, RowGap, RowInset, RowRadius};
use crate::ui::theme::*;
use crate::ui::Root;
use gpui::{div, prelude::*, px, rgb, AnyElement, Context, SharedString};

#[derive(Clone, Debug)]
pub(super) struct BreakdownItem {
    pub(super) name: String,
    pub(super) size: u64,
    pub(super) ratio: f64,
    pub(super) color: u32,
    pub(super) is_dir: bool,
    pub(super) idx: Option<u32>,
}

pub(super) fn render_breakdown_row(
    item: &BreakdownItem,
    i: usize,
    cx: &mut Context<Root>,
) -> AnyElement {
    let is_dir = item.is_dir;
    let idx = item.idx;
    let pct_str = format!("{:.1}%", item.ratio * 100.0);
    let color = item.color;
    let name = truncate(&item.name, 18);
    let size_str = fmt_size(item.size);

    let mut row = ListRow::new(SharedString::from(format!("bd-row-{i}")))
        .inset(RowInset::P2)
        .gap(RowGap::Tight)
        .radius(RowRadius::Lg)
        .content(
            div()
                .flex()
                .items_center()
                .gap_2()
                .min_w(px(0.))
                .flex_1()
                .child(
                    div()
                        .w(px(10.))
                        .h(px(10.))
                        .flex_none()
                        .rounded_full()
                        .bg(rgb(color)),
                )
                .child(
                    div().flex_1().min_w(px(0.)).flex().flex_col().child(
                        div()
                            .text_xs()
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .text_color(rgb(TEXT))
                            .child(name),
                    ),
                ),
        )
        .trailing(
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(
                    Badge::new(pct_str)
                        .tone(BadgeTone::Custom {
                            bg: rgb(SURF_HIGH).into(),
                            fg: rgb(color).into(),
                        })
                        .padding(8., 1.)
                        .weight(gpui::FontWeight::BOLD),
                )
                .child(
                    div()
                        .flex_none()
                        .text_right()
                        .text_xs()
                        .font_weight(gpui::FontWeight::BOLD)
                        .text_color(rgb(TEXT))
                        .child(size_str),
                ),
        );
    if let (true, Some(target_idx)) = (is_dir, idx) {
        row = row.on_click(cx.listener(move |this, _, _, cx| {
            this.enter_disk_node(target_idx, cx);
        }));
    }
    row.into_any_element()
}
