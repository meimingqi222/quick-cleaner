//! 磁盘透镜页面的局部组件

use crate::core::model::{fmt_size, truncate};
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

    div()
        .id(SharedString::from(format!("bd-row-{i}")))
        .flex()
        .items_center()
        .justify_between()
        .gap_2()
        .p_2()
        .rounded_lg()
        .hover(|h| h.bg(rgb(SURF_LOW)))
        .when(is_dir && idx.is_some(), |d| d.cursor_pointer())
        .child(
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
        .child(
            div()
                .px_2()
                .py(px(1.))
                .rounded_md()
                .bg(rgb(SURF_HIGH))
                .text_xs()
                .font_weight(gpui::FontWeight::BOLD)
                .text_color(rgb(color))
                .child(pct_str),
        )
        .child(
            div()
                .flex_none()
                .text_right()
                .text_xs()
                .font_weight(gpui::FontWeight::BOLD)
                .text_color(rgb(TEXT))
                .child(size_str),
        )
        .when(is_dir && idx.is_some(), |d| {
            let target_idx = idx.unwrap();
            d.on_click(cx.listener(move |this, _, _, cx| {
                this.disk.path.push(target_idx);
                cx.notify();
            }))
        })
        .into_any_element()
}
