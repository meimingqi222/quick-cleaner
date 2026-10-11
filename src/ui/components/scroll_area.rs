//! Scroll viewport with an isolated, window-captured scrollbar controller.
use crate::ui::components::cards::card;
use crate::ui::components::scroll::{
    drag_capture, drag_to_offset, scroll_metrics, scrollbar, SCROLLBAR_W,
};
use gpui::{
    canvas, div, point, prelude::*, px, AnyElement, App, Div, ElementId, MouseButton, ScrollHandle,
    UniformList, UniformListScrollHandle, Window,
};
use std::ops::Range;

pub fn scrollbar_present(handle: &ScrollHandle, viewport: f32, content: f32) -> bool {
    scroll_metrics(handle, viewport, content).is_some()
}

/// Variable-height, non-virtual content with the same scrollbar controller.
#[derive(IntoElement)]
pub struct ScrollView {
    id: ElementId,
    content: AnyElement,
    handle: Option<ScrollHandle>,
}
impl ScrollView {
    pub fn new(id: impl Into<ElementId>, content: impl IntoElement) -> Self {
        Self {
            id: id.into(),
            content: content.into_any_element(),
            handle: None,
        }
    }

    pub fn track_scroll(mut self, handle: ScrollHandle) -> Self {
        self.handle = Some(handle);
        self
    }
}
impl RenderOnce for ScrollView {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let handle = self.handle.unwrap_or_else(|| {
            window
                .use_keyed_state((self.id.clone(), "handle"), cx, |_, _| ScrollHandle::new())
                .read(cx)
                .clone()
        });
        let extent = window.use_keyed_state((self.id.clone(), "extent"), cx, |_, _| (0_f32, 0_f32));
        let owner = window.current_view();
        let (viewport, content) = *extent.read(cx);
        let measure_handle = handle.clone();
        let body = div()
            .id((self.id.clone(), "body"))
            .size_full()
            .overflow_scroll()
            .track_scroll(&handle)
            .child(self.content)
            .child(
                canvas(
                    |_, _, _| (),
                    move |_, _, _, cx| {
                        let height = f32::from(measure_handle.bounds().size.height);
                        let next = (
                            height,
                            height + f32::from(measure_handle.max_offset().height),
                        );
                        let changed = extent.update(cx, |current, cx| {
                            if *current != next {
                                *current = next;
                                cx.notify();
                                return true;
                            }
                            false
                        });
                        if changed {
                            cx.notify(owner);
                        }
                    },
                )
                .absolute()
                .size(px(0.)),
            );
        ScrollArea::new(self.id, handle, viewport, content, body)
    }
}

pub fn virtual_list<R: IntoElement>(
    id: impl Into<ElementId>,
    count: usize,
    handle: UniformListScrollHandle,
    render_range: impl Fn(Range<usize>, &mut Window, &mut App) -> Vec<R> + 'static,
) -> UniformList {
    gpui::uniform_list(id, count, render_range)
        .track_scroll(handle)
        .size_full()
}

#[derive(IntoElement)]
pub struct ScrollArea {
    id: ElementId,
    handle: ScrollHandle,
    viewport: f32,
    content_height: f32,
    content: AnyElement,
}

impl ScrollArea {
    pub fn new(
        id: impl Into<ElementId>,
        handle: ScrollHandle,
        viewport: f32,
        content_height: f32,
        content: impl IntoElement,
    ) -> Self {
        Self {
            id: id.into(),
            handle,
            viewport,
            content_height,
            content: content.into_any_element(),
        }
    }
}

impl RenderOnce for ScrollArea {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        // Keyed state is released when this viewport disappears, so a hidden list cannot keep dragging.
        let drag = window.use_keyed_state((self.id.clone(), "drag"), cx, |_, _| None::<(f32, f32)>);
        let metrics = scroll_metrics(&self.handle, self.viewport, self.content_height);
        let bar = metrics.map(|m| {
            let drag = drag.clone();
            let handle = self.handle.clone();
            scrollbar(format!("{:?}-thumb", self.id), m, |thumb| {
                thumb.on_mouse_down(MouseButton::Left, move |event, _, cx| {
                    let top = f32::from(-handle.offset().y).max(0.);
                    drag.update(cx, |start, cx| {
                        *start = Some((f32::from(event.position.y), top));
                        cx.notify();
                    });
                    cx.stop_propagation();
                })
            })
        });
        let handle = self.handle;
        div()
            .relative()
            .size_full()
            .flex_1()
            .min_h(px(0.))
            .overflow_hidden()
            .child(
                div()
                    .size_full()
                    .when(metrics.is_some(), |d| d.pr(px(SCROLLBAR_W)))
                    .child(self.content),
            )
            .children(bar)
            .child(drag_capture(
                drag,
                move |start, y, cx| {
                    let Some(start) = *start else {
                        return;
                    };
                    if let Some(top) = drag_to_offset(&handle, start, y) {
                        handle.set_offset(point(px(0.), px(-top)));
                        cx.notify();
                    }
                },
                |start, cx| {
                    if start.take().is_some() {
                        cx.notify();
                    }
                },
            ))
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

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{point, Context, Modifiers, Render, TestAppContext};
    use std::{cell::Cell, rc::Rc};

    struct Fixture {
        a: UniformListScrollHandle,
        b: UniformListScrollHandle,
        rendered: Rc<Cell<usize>>,
    }

    struct NormalFixture {
        handle: ScrollHandle,
    }
    impl Render for NormalFixture {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().w(px(200.)).h(px(120.)).child(
                ScrollView::new(
                    "normal",
                    div().flex().flex_col().children((0..30).map(|i| {
                        div()
                            .flex_none()
                            .h(px(28. + (i % 3) as f32))
                            .child(i.to_string())
                    })),
                )
                .track_scroll(self.handle.clone()),
            )
        }
    }

    #[gpui::test]
    fn normal_scroll_view_measures_content_and_supports_shared_drag(cx: &mut TestAppContext) {
        let (view, cx) = cx.add_window_view(|_, _| NormalFixture {
            handle: ScrollHandle::new(),
        });
        cx.run_until_parked();
        view.read_with(cx, |this, _| {
            assert!(this.handle.max_offset().height > px(500.));
            assert_eq!(this.handle.bounds().size.height, px(120.));
        });
        cx.simulate_mouse_down(
            point(px(194.), px(12.)),
            MouseButton::Left,
            Modifiers::none(),
        );
        cx.simulate_mouse_move(
            point(px(450.), px(90.)),
            Some(MouseButton::Left),
            Modifiers::none(),
        );
        view.read_with(cx, |this, _| assert!(this.handle.offset().y < px(0.)));
        cx.simulate_mouse_up(
            point(px(450.), px(90.)),
            MouseButton::Left,
            Modifiers::none(),
        );
    }
    impl Render for Fixture {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let view = |id, handle: &UniformListScrollHandle| {
                let count = self.rendered.clone();
                let list = virtual_list(id, 10_000, handle.clone(), move |range, _, _| {
                    count.set(count.get() + range.len());
                    range
                        .map(|i| div().h(px(30.)).child(i.to_string()))
                        .collect::<Vec<_>>()
                });
                div().w(px(200.)).h(px(120.)).child(ScrollArea::new(
                    id,
                    handle.0.borrow().base_handle.clone(),
                    120.,
                    300_000.,
                    list,
                ))
            };
            div()
                .size_full()
                .flex()
                .child(view("list-a", &self.a))
                .child(view("list-b", &self.b))
        }
    }
    #[gpui::test]
    fn virtualized_lists_drag_outside_and_keep_independent_offsets(cx: &mut TestAppContext) {
        let (view, cx) = cx.add_window_view(|_, _| Fixture {
            a: UniformListScrollHandle::new(),
            b: UniformListScrollHandle::new(),
            rendered: Rc::new(Cell::new(0)),
        });
        view.read_with(cx, |this, _| {
            assert!(
                this.rendered.get() < 100,
                "only visible rows should be built"
            )
        });
        cx.simulate_mouse_down(
            point(px(194.), px(12.)),
            MouseButton::Left,
            Modifiers::none(),
        );
        cx.simulate_mouse_move(
            point(px(450.), px(90.)),
            Some(MouseButton::Left),
            Modifiers::none(),
        );
        view.read_with(cx, |this, _| {
            assert!(this.a.0.borrow().base_handle.offset().y < px(0.));
            assert_eq!(this.b.0.borrow().base_handle.offset().y, px(0.));
        });
        cx.simulate_mouse_up(
            point(px(450.), px(90.)),
            MouseButton::Left,
            Modifiers::none(),
        );
        let before = view.read_with(cx, |this, _| this.a.0.borrow().base_handle.offset());
        cx.simulate_mouse_move(point(px(450.), px(110.)), None, Modifiers::none());
        view.read_with(cx, |this, _| {
            assert_eq!(this.a.0.borrow().base_handle.offset(), before)
        });
    }
}
