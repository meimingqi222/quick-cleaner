//! Routes platform input to the binding registered by the focused field.
use crate::ui::components::input::{InputBinding, InputHost};
use crate::ui::text_input::*;
use crate::ui::Root;
use gpui::{point, px, Bounds, Context, EntityInputHandler, Pixels, UTF16Selection, Window};
use std::ops::Range;
impl Root {
    pub fn register_input(&mut self, binding: InputBinding<Self>, window: &mut Window) {
        if binding.input(self).focus_handle.is_focused(window) {
            self.active_input = Some(binding);
        }
    }
    fn focused_input(&self, window: &Window) -> Option<InputBinding<Self>> {
        self.active_input
            .and_then(|binding| binding.focused(self, window))
    }
}
impl EntityInputHandler for Root {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        actual_range: &mut Option<Range<usize>>,
        window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<String> {
        let binding = self.focused_input(window)?;
        let input = binding.input(self);
        let range = clamp_to_boundary(&input.text, range_from_utf16(&input.text, &range_utf16));
        actual_range.replace(range_to_utf16(&input.text, &range));
        Some(input.text[range].to_string())
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        let binding = self.focused_input(window)?;
        let input = binding.input(self);
        Some(UTF16Selection {
            range: range_to_utf16(&input.text, &input.selection()),
            reversed: false,
        })
    }

    fn marked_text_range(
        &self,
        window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        let binding = self.focused_input(window)?;
        let input = binding.input(self);
        input
            .marked
            .as_ref()
            .map(|r| range_to_utf16(&input.text, r))
    }

    fn unmark_text(&mut self, window: &mut Window, _cx: &mut Context<Self>) {
        let Some(binding) = self.focused_input(window) else {
            return;
        };
        binding.input_mut(self).marked = None;
    }

    /// 提交文本：普通打字与输入法确认后的汉字都走这里。
    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(binding) = self.focused_input(window) else {
            return;
        };
        binding.commit(self, range_utf16.as_ref(), new_text, cx);
    }

    /// 组合中的文本：输入法候选阶段的拼音串，尚未确认。
    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_range_utf16: Option<Range<usize>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(binding) = self.focused_input(window) else {
            return;
        };
        binding.compose(
            self,
            range_utf16.as_ref(),
            new_text,
            new_selected_range_utf16.as_ref(),
            cx,
        );
    }

    /// 输入法候选窗口的定位锚点。
    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        element_bounds: Bounds<Pixels>,
        window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let binding = self.focused_input(window)?;
        let input = binding.input(self);
        if let Some(hit) = input.text_hit.as_ref() {
            let range = clamp_to_boundary(&input.text, range_from_utf16(&input.text, &range_utf16));
            let x1 = hit.line.x_for_index(range.start.min(hit.line.len()));
            let x2 = hit.line.x_for_index(range.end.min(hit.line.len()));
            return Some(Bounds::from_corners(
                point(hit.bounds.origin.x + x1, hit.bounds.origin.y),
                point(
                    hit.bounds.origin.x + x2.max(x1 + px(1.0)),
                    hit.bounds.origin.y + hit.bounds.size.height,
                ),
            ));
        }
        Some(input.bounds.unwrap_or(element_bounds))
    }

    fn character_index_for_point(
        &mut self,
        point: gpui::Point<Pixels>,
        window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        let binding = self.focused_input(window)?;
        let font_size = binding.font_size;
        let input = binding.input(self);
        let byte_idx = index_for_mouse_x(
            &input.text,
            f32::from(point.x),
            input.text_hit.as_ref(),
            font_size,
            window,
        );
        Some(offset_to_utf16(&input.text, byte_idx))
    }
}
