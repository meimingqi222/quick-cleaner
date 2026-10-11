//! Anchored overlays: input stays focused; the surface owns mouse isolation.

use crate::ui::theme::{rgba, CARD, OUTLINE_VAR, PRIMARY_FIXED};
use gpui::{
    div, prelude::*, px, rgb, AnyElement, Bounds, Context, Div, Pixels, Point, ScrollHandle,
    SharedString, Stateful, Window,
};

pub trait PopoverCandidate {
    fn id(&self) -> &str;
}

#[derive(Default)]
pub struct PopoverState {
    dismissed: bool,
    selected: Option<String>,
    pub scroll: ScrollHandle,
}

#[derive(Debug, PartialEq, Eq)]
pub enum PopoverKey {
    Unhandled,
    Changed,
    Confirm(usize),
    Dismissed,
}

impl PopoverState {
    pub fn is_open(&self, anchor_focused: bool) -> bool {
        anchor_focused && !self.dismissed
    }

    pub fn dismissed(&self) -> bool {
        self.dismissed
    }

    pub fn dismiss(&mut self) {
        self.dismissed = true;
    }

    pub fn reopen(&mut self) {
        self.dismissed = false;
    }

    pub fn reset_selection(&mut self) {
        self.selected = None;
        self.scroll.scroll_to_item(0);
    }

    pub fn edited(&mut self) {
        self.reopen();
        self.reset_selection();
    }

    pub fn selected_id(&self) -> Option<&str> {
        self.selected.as_deref()
    }

    pub fn selection<C: PopoverCandidate>(&self, items: &[C]) -> Option<usize> {
        candidate_index(self.selected_id(), items)
    }

    pub fn select(&mut self, id: &str) -> bool {
        if self.selected_id() == Some(id) {
            return false;
        }
        self.selected = Some(id.to_owned());
        true
    }

    pub fn handle_key<C: PopoverCandidate>(
        &mut self,
        key: &str,
        composing: bool,
        items: &[C],
    ) -> PopoverKey {
        if composing {
            return PopoverKey::Unhandled;
        }
        match key {
            "escape" if !self.dismissed => {
                self.dismiss();
                PopoverKey::Dismissed
            }
            "enter" if !self.dismissed => self
                .selection(items)
                .map_or(PopoverKey::Unhandled, PopoverKey::Confirm),
            "up" | "down" if !items.is_empty() => {
                let index = if self.dismissed {
                    if key == "up" {
                        items.len() - 1
                    } else {
                        0
                    }
                } else {
                    next_candidate(self.selected_id(), items, key == "up").unwrap()
                };
                self.reopen();
                self.select(items[index].id());
                self.scroll.scroll_to_item(index);
                PopoverKey::Changed
            }
            _ => PopoverKey::Unhandled,
        }
    }
}

pub fn candidate_index<C: PopoverCandidate>(selected: Option<&str>, items: &[C]) -> Option<usize> {
    if items.is_empty() {
        return None;
    }
    Some(
        items
            .iter()
            .position(|item| Some(item.id()) == selected)
            .unwrap_or(0),
    )
}

pub fn next_candidate<C: PopoverCandidate>(
    selected: Option<&str>,
    items: &[C],
    backwards: bool,
) -> Option<usize> {
    let current = candidate_index(selected, items)?;
    Some(if backwards {
        (current + items.len() - 1) % items.len()
    } else {
        (current + 1) % items.len()
    })
}

pub struct PopoverSpec {
    pub id: SharedString,
    pub width: f32,
    pub top: f32,
    pub max_height: f32,
    pub anchor_bounds: Option<Bounds<Pixels>>,
}

fn outside_anchor(anchor: Option<Bounds<Pixels>>, position: Point<Pixels>) -> bool {
    !anchor.is_some_and(|bounds| bounds.contains(&position))
}

pub fn popover_surface<T: 'static>(
    spec: PopoverSpec,
    on_dismiss: impl Fn(&mut T, &mut Window, &mut Context<T>) + 'static,
    cx: &mut Context<T>,
) -> Stateful<Div> {
    let mut surface = div()
        .id(spec.id)
        // stop_propagation alone does not change GPUI's hover/scroll hit testing.
        .occlude()
        .absolute()
        .top(px(spec.top))
        .left(px(0.))
        .w(px(spec.width))
        .max_h(px(spec.max_height))
        .bg(rgb(CARD))
        .border_1()
        .border_color(rgba(OUTLINE_VAR, 0.75))
        .rounded_xl()
        .shadow_xl()
        .overflow_hidden()
        .flex()
        .flex_col()
        .on_any_mouse_down(|_, _, cx| cx.stop_propagation())
        // Let child lists scroll first; prevent ancestor scroll containers from also moving.
        .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
        .on_mouse_down_out(
            cx.listener(move |this, event: &gpui::MouseDownEvent, window, cx| {
                if outside_anchor(spec.anchor_bounds, event.position) {
                    on_dismiss(this, window, cx);
                }
            }),
        );
    surface
        .interactivity()
        .on_any_mouse_up(|_, _, cx| cx.stop_propagation());
    surface
}

pub fn popover_list(id: SharedString, state: &PopoverState, max_height: f32) -> Stateful<Div> {
    div()
        .id(id)
        .flex_1()
        .min_h(px(0.))
        .max_h(px(max_height))
        .overflow_y_scroll()
        .track_scroll(&state.scroll)
        .px_2()
        .py_1()
        .flex()
        .flex_col()
        .gap_1()
}

pub fn popover_row<T: 'static>(
    id: SharedString,
    selected: bool,
    state: fn(&mut T) -> &mut PopoverState,
    cx: &mut Context<T>,
) -> Stateful<Div> {
    let hover_id = id.clone();
    div()
        .id(id)
        .w_full()
        .min_h(px(30.))
        .flex_none()
        .px_2()
        .py_1()
        .rounded_md()
        .flex()
        .items_center()
        .gap_2()
        .cursor_pointer()
        .when(selected, |d| d.bg(rgb(PRIMARY_FIXED)))
        .on_hover(cx.listener(move |this, hovered, _, cx| {
            if *hovered && state(this).select(hover_id.as_ref()) {
                cx.notify();
            }
        }))
}

pub fn popover_overlay(content: impl IntoElement) -> AnyElement {
    gpui::deferred(content).priority(100).into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{
        point, Modifiers, MouseButton, Render, ScrollDelta, ScrollWheelEvent, TestAppContext,
        TouchPhase,
    };

    impl PopoverCandidate for String {
        fn id(&self) -> &str {
            self
        }
    }

    impl PopoverCandidate for &str {
        fn id(&self) -> &str {
            self
        }
    }

    #[test]
    fn keyboard_lifecycle_preserves_ime_and_stable_selection() {
        let items = ["query", "process", "syntax"];
        let mut state = PopoverState::default();
        assert!(state.is_open(true));
        assert!(!state.is_open(false));
        assert_eq!(
            state.handle_key("down", true, &items),
            PopoverKey::Unhandled
        );
        assert_eq!(
            state.handle_key("enter", true, &items),
            PopoverKey::Unhandled
        );
        assert_eq!(state.selection(&items), Some(0));
        assert_eq!(state.handle_key("down", false, &items), PopoverKey::Changed);
        assert_eq!(state.selection(&items), Some(1));
        assert_eq!(state.selection(&items[1..]), Some(0));
        assert_eq!(
            state.handle_key("enter", false, &items),
            PopoverKey::Confirm(1)
        );
        assert_eq!(
            state.handle_key("escape", false, &items),
            PopoverKey::Dismissed
        );
        assert_eq!(
            state.handle_key("enter", false, &items),
            PopoverKey::Unhandled
        );
        assert_eq!(state.handle_key("down", false, &items), PopoverKey::Changed);
        assert_eq!(state.selection(&items), Some(0));
        state.edited();
        assert_eq!(state.selection(&["different"]), Some(0));
        assert_eq!(
            state.handle_key("down", false, &[] as &[&str]),
            PopoverKey::Unhandled
        );
        assert!(outside_anchor(None, point(px(1.), px(1.))));
        let bounds = Bounds::new(point(px(0.), px(0.)), gpui::size(px(100.), px(25.)));
        assert!(!outside_anchor(Some(bounds), point(px(10.), px(10.))));
        assert!(outside_anchor(Some(bounds), point(px(150.), px(10.))));
    }

    struct OverlayFixture {
        hover: bool,
        table_clicks: usize,
        table_scrolls: usize,
        ancestor_scrolls: usize,
        chosen: usize,
        open: bool,
        state: PopoverState,
        focus: gpui::FocusHandle,
        items: Vec<String>,
        confirmed: Option<usize>,
    }

    impl OverlayFixture {
        fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
            let focus = cx.focus_handle();
            focus.focus(window);
            Self {
                hover: false,
                table_clicks: 0,
                table_scrolls: 0,
                ancestor_scrolls: 0,
                chosen: 0,
                open: true,
                state: PopoverState::default(),
                focus,
                items: (0..12).map(|i| format!("fixture-row-{i}")).collect(),
                confirmed: None,
            }
        }
    }

    impl Render for OverlayFixture {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let overlay = self.open.then(|| {
                let selected = self.state.selection(&self.items);
                let list = popover_list("fixture-list".into(), &self.state, 90.)
                    .h(px(90.))
                    .children(self.items.iter().enumerate().map(|(i, id)| {
                        popover_row(
                            id.clone().into(),
                            selected == Some(i),
                            |this: &mut Self| &mut this.state,
                            cx,
                        )
                        .h(px(30.))
                        .on_click(cx.listener(|this, _, _, _| this.chosen += 1))
                    }));
                popover_overlay(
                    popover_surface(
                        PopoverSpec {
                            id: "fixture-popover".into(),
                            width: 120.,
                            top: 34.,
                            max_height: 100.,
                            anchor_bounds: Some(Bounds::new(
                                point(px(0.), px(0.)),
                                gpui::size(px(120.), px(28.)),
                            )),
                        },
                        |this: &mut Self, _, cx| {
                            this.open = false;
                            cx.notify();
                        },
                        cx,
                    )
                    .child(list),
                )
            });
            div()
                .relative()
                .size_full()
                .track_focus(&self.focus)
                .on_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, _, cx| {
                    match this
                        .state
                        .handle_key(&event.keystroke.key, false, &this.items)
                    {
                        PopoverKey::Confirm(i) => this.confirmed = Some(i),
                        PopoverKey::Dismissed => this.open = false,
                        PopoverKey::Changed => {}
                        PopoverKey::Unhandled => return,
                    }
                    cx.notify();
                }))
                .on_scroll_wheel(cx.listener(|this, _, _, _| this.ancestor_scrolls += 1))
                .child(
                    div()
                        .id("fixture-table")
                        .absolute()
                        .w(px(300.))
                        .h(px(300.))
                        .hover(|d| d.bg(rgb(0xff0000)))
                        .on_hover(cx.listener(|this, hovered, _, _| this.hover = *hovered))
                        .on_any_mouse_down(cx.listener(|this, _, _, _| this.table_clicks += 1))
                        .on_scroll_wheel(cx.listener(|this, _, _, _| this.table_scrolls += 1)),
                )
                .children(overlay)
        }
    }

    #[gpui::test]
    fn overlay_blocks_underlying_hover_clicks_and_scroll(cx: &mut TestAppContext) {
        let (view, cx) = cx.add_window_view(OverlayFixture::new);
        let inside = point(px(50.), px(60.));
        cx.simulate_mouse_move(inside, None, Modifiers::none());
        view.read_with(cx, |this, _| {
            assert!(!this.hover, "popover must occlude table hover")
        });
        cx.simulate_click(inside, Modifiers::none());
        cx.simulate_mouse_down(inside, MouseButton::Right, Modifiers::none());
        cx.simulate_mouse_up(inside, MouseButton::Right, Modifiers::none());
        cx.simulate_event(ScrollWheelEvent {
            position: inside,
            delta: ScrollDelta::Pixels(point(px(0.), px(-60.))),
            modifiers: Modifiers::none(),
            touch_phase: TouchPhase::Moved,
        });
        view.read_with(cx, |this, _| {
            assert_eq!(this.chosen, 1, "overlay rows remain clickable");
            assert_eq!(this.table_clicks, 0);
            assert_eq!(this.table_scrolls, 0);
            assert_eq!(this.ancestor_scrolls, 0);
            assert!(
                this.state.scroll.offset().y < px(0.),
                "overlay list still scrolls"
            );
        });
        cx.simulate_click(point(px(20.), px(15.)), Modifiers::none());
        view.read_with(cx, |this, _| {
            assert!(this.open, "anchor clicks must not dismiss")
        });
        cx.simulate_click(point(px(200.), px(200.)), Modifiers::none());
        cx.simulate_mouse_move(point(px(220.), px(200.)), None, Modifiers::none());
        view.read_with(cx, |this, _| {
            assert!(!this.open);
            assert!(this.hover, "table hover resumes outside the overlay");
            assert!(
                this.table_clicks > 0,
                "outside click must reach its destination"
            );
        });
    }
    #[gpui::test]
    fn keyboard_scrolls_to_selected_row_and_escape_dismisses(cx: &mut TestAppContext) {
        let (view, cx) = cx.add_window_view(OverlayFixture::new);
        cx.simulate_keystrokes("up");
        view.read_with(cx, |this, _| {
            assert_eq!(this.state.selection(&this.items), Some(11));
            assert!(
                this.state.scroll.offset().y < px(0.),
                "keyboard selection must be visible"
            );
        });
        cx.simulate_keystrokes("enter");
        view.read_with(cx, |this, _| assert_eq!(this.confirmed, Some(11)));
        cx.simulate_keystrokes("escape");
        view.read_with(cx, |this, _| assert!(!this.open));
    }
}
