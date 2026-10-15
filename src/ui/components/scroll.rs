//! 产品滚动条主题适配；度量与捕获由公共组件实现。
use gpui::{Div, SharedString, Stateful};
pub use gpui_app_widgets::scroll::{
    drag_capture, drag_to_offset, scroll_metrics, ScrollMetrics, SCROLLBAR_W,
};

pub fn style() -> gpui_app_widgets::scroll::ScrollbarStyle {
    use crate::ui::theme::*;
    gpui_app_widgets::scroll::ScrollbarStyle {
        track: rgba(OUTLINE_VAR, 0.14),
        thumb: gpui::rgb(OUTLINE).into(),
    }
}
pub fn scrollbar(
    id: impl Into<SharedString>,
    metrics: ScrollMetrics,
    on_down: impl FnOnce(Stateful<Div>) -> Stateful<Div>,
) -> Div {
    gpui_app_widgets::scroll::scrollbar(style(), id, metrics, on_down)
}
