//! 产品主题适配；公共组件持有滚动和拖拽行为。
use super::{cards::card, scroll::style};
use gpui::{prelude::*, App, Div, ElementId, ScrollHandle, Window};
pub use gpui_app_widgets::scroll_area::{scrollbar_present, virtual_list};

#[derive(IntoElement)]
pub struct ScrollView(gpui_app_widgets::scroll_area::ScrollView);
impl ScrollView {
    pub fn new(id: impl Into<ElementId>, content: impl IntoElement) -> Self {
        Self(gpui_app_widgets::scroll_area::ScrollView::new(
            id,
            style(),
            content,
        ))
    }
    pub fn track_scroll(self, handle: ScrollHandle) -> Self {
        Self(self.0.track_scroll(handle))
    }
}
impl RenderOnce for ScrollView {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        self.0
    }
}
#[derive(IntoElement)]
pub struct ScrollArea(gpui_app_widgets::scroll_area::ScrollArea);
impl ScrollArea {
    pub fn new(
        id: impl Into<ElementId>,
        handle: ScrollHandle,
        viewport: f32,
        content_height: f32,
        content: impl IntoElement,
    ) -> Self {
        Self(gpui_app_widgets::scroll_area::ScrollArea::new(
            id,
            style(),
            handle,
            viewport,
            content_height,
            content,
        ))
    }
}
impl RenderOnce for ScrollArea {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        self.0
    }
}
pub fn table_shell(
    header: impl IntoElement,
    body: impl IntoElement,
    footer: impl IntoElement,
) -> Div {
    card()
        .flex_1()
        .overflow_hidden()
        .flex()
        .flex_col()
        .child(header)
        .child(body)
        .child(footer)
}
