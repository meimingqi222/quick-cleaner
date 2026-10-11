//! Business input bindings; adding a field does not add a target enum.
use crate::ui::components::input::InputBinding;
use crate::ui::Root;
use gpui::{Context, KeyDownEvent, Window};
fn key(this: &mut Root, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Root>) -> bool {
    this.handle_root_escape(event, window, cx)
}
fn status_key(
    this: &mut Root,
    event: &KeyDownEvent,
    window: &mut Window,
    cx: &mut Context<Root>,
) -> bool {
    if key(this, event, window, cx) {
        return true;
    }
    if matches!(event.keystroke.key.as_str(), "up" | "down" | "enter")
        && this.monitor.input.marked.is_none()
    {
        this.status_search_candidate_key(&event.keystroke.key, cx);
        return true;
    }
    false
}
pub const APPS_INPUT: InputBinding<Root> = InputBinding {
    read: |this| &this.apps.input,
    write: |this| &mut this.apps.input,
    on_change: |_, cx| cx.notify(),
    on_activity: Root::poke_cursor_blink,
    on_key: key,
    register: Root::register_input,
    font_size: 12.,
};
pub const FILE_INPUT: InputBinding<Root> = InputBinding {
    read: |this| &this.search.input,
    write: |this| &mut this.search.input,
    on_change: Root::search_input_changed,
    on_activity: Root::poke_cursor_blink,
    on_key: key,
    register: Root::register_input,
    font_size: 13.,
};
pub const STATUS_INPUT: InputBinding<Root> = InputBinding {
    read: |this| &this.monitor.input,
    write: |this| &mut this.monitor.input,
    on_change: |this, cx| {
        this.monitor.search_popover.edited();
        this.rebuild_proc_view();
        cx.notify();
    },
    on_activity: Root::poke_cursor_blink,
    on_key: status_key,
    register: Root::register_input,
    font_size: 12.,
};
impl Root {
    pub fn search_backspace(&mut self) {
        let input = &mut self.apps.input;
        input.sel = crate::ui::text_input::delete_backward(&mut input.text, input.sel.clone());
        input.marked = None;
    }
    pub fn search_clear(&mut self) {
        self.apps.input.clear();
    }
}
