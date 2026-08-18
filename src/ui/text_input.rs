//! 输入法（IME）支持
//!
//! 光靠 `on_key_down` + `keystroke.key_char` 收不到中文。中文/日文/韩文
//! 输入法的工作方式是：用户按下的字母先进入**组合**状态（拼音串），
//! 由输入法窗口候选、确认后才产出真正的文字。这个过程走的是 Windows 的
//! `WM_IME_COMPOSITION` 消息，而 GPUI 只有在视图注册了输入处理器之后才会
//! 把它转发过来——否则应用只能看到原始按键，于是屏幕上出现 "tonghuashun"
//! 这样的拼音。
//!
//! 要接上，需要两件事：
//!
//! 1. 视图实现 [`gpui::EntityInputHandler`]（本模块）。
//! 2. 绘制阶段调用 `Window::handle_input` 把处理器挂到焦点上（见
//!    `views::apps` 里搜索框内的 `canvas`）。
//!
//! 注意：接上之后，**普通 ASCII 字符也会走这条路**——GPUI 的 Windows
//! 后端把 `WM_CHAR` 同样转成 `replace_text_in_range`。所以 `on_key_down`
//! 里绝不能再自己追加字符，否则每个字母都会输入两遍。
//!
//! # 偏移量的两套坐标系
//!
//! `EntityInputHandler` 的接口全部用 **UTF-16** 偏移（Windows 的原生
//! 单位），而 Rust 的 `String` 用**字节**偏移。两者对 ASCII 一致，对中文
//! 相差 3 倍（UTF-8 三字节 vs UTF-16 一个码元）。混用会直接 panic 在
//! 非字符边界上，所以下面每处转换都要显式做。

use gpui::{
    font, Bounds, Context, EntityInputHandler, LineLayout, Pixels, px, TextRun, UTF16Selection,
    Window,
};
use std::ops::Range;
use std::sync::Arc;

use crate::ui::Root;

/// 使用 GPUI 真实字体排版系统测量单行文本（支持系统任意字体、DPI 与字符集）。
pub fn layout_single_line_window(
    text: &str,
    font_size: f32,
    window: &mut Window,
) -> Arc<LineLayout> {
    let runs = if text.is_empty() {
        Vec::new()
    } else {
        vec![TextRun {
            len: text.len(),
            font: font(".ZedSans"),
            color: gpui::black(),
            background_color: None,
            underline: None,
            strikethrough: None,
        }]
    };
    window.text_system().layout_line(text, px(font_size), &runs, None)
}

/// 根据相对文本起点的 x 像素坐标，通过底层 DirectWrite/CoreText 真实排版精确计算对应的字符字节索引。
pub fn closest_index_for_x_layout(
    text: &str,
    rel_x: f32,
    font_size: f32,
    window: &mut Window,
) -> usize {
    if text.is_empty() || rel_x <= 0.0 {
        return 0;
    }
    let layout = layout_single_line_window(text, font_size, window);
    let idx = layout.closest_index_for_x(px(rel_x));
    clamp_to_boundary(text, idx..idx).start
}

/// 计算指定字符字节索引在排版中的精确 X 坐标（像素）。
pub fn x_for_index_layout(
    text: &str,
    index: usize,
    font_size: f32,
    window: &mut Window,
) -> f32 {
    if text.is_empty() || index == 0 {
        return 0.0;
    }
    let layout = layout_single_line_window(text, font_size, window);
    f32::from(layout.x_for_index(index.min(text.len())))
}

/// 把 UTF-16 偏移换算成字节偏移。
///
/// 越界时钳到字符串末尾——输入法偶尔会给出超出当前内容的范围（例如
/// 内容在异步回调里被清空过），直接切片会 panic。
pub fn offset_from_utf16(s: &str, utf16_offset: usize) -> usize {
    let mut utf16_count = 0usize;
    for (byte_idx, ch) in s.char_indices() {
        if utf16_count >= utf16_offset {
            return byte_idx;
        }
        utf16_count += ch.len_utf16();
    }
    s.len()
}

/// 把字节偏移换算成 UTF-16 偏移。
pub fn offset_to_utf16(s: &str, byte_offset: usize) -> usize {
    let clamped = byte_offset.min(s.len());
    s[..clamped].chars().map(char::len_utf16).sum()
}

pub fn range_from_utf16(s: &str, r: &Range<usize>) -> Range<usize> {
    let start = offset_from_utf16(s, r.start);
    let end = offset_from_utf16(s, r.end);
    start.min(end)..start.max(end)
}

pub fn range_to_utf16(s: &str, r: &Range<usize>) -> Range<usize> {
    offset_to_utf16(s, r.start)..offset_to_utf16(s, r.end)
}

/// 把字节范围钳进合法的字符边界，避免切片 panic。
pub fn clamp_to_boundary(s: &str, r: Range<usize>) -> Range<usize> {
    let mut start = r.start.min(s.len());
    let mut end = r.end.min(s.len());
    while start > 0 && !s.is_char_boundary(start) {
        start -= 1;
    }
    while end < s.len() && !s.is_char_boundary(end) {
        end += 1;
    }
    start.min(end)..start.max(end)
}

/// 文件搜索框的选区钳位（公开给 mod.rs 调用）。
pub fn clamp_search_sel(s: &str, r: Range<usize>) -> Range<usize> {
    clamp_to_boundary(s, r)
}

/// 光标向左移动。
/// - 没有 shift 且有选区：光标跳到选区起点
/// - 没有 shift 且无选区：向左退一个字符
/// - 有 shift：选区向左扩展
pub fn move_left(text: &str, sel: Range<usize>, shift: bool, _ctrl: bool) -> Range<usize> {
    let sel = clamp_to_boundary(text, sel);
    if !shift {
        if sel.start != sel.end {
            sel.start..sel.start
        } else if sel.start > 0 {
            let prev = text[..sel.start]
                .char_indices()
                .next_back()
                .map(|(i, _)| i)
                .unwrap_or(0);
            prev..prev
        } else {
            0..0
        }
    } else {
        let prev = if sel.start > 0 {
            text[..sel.start]
                .char_indices()
                .next_back()
                .map(|(i, _)| i)
                .unwrap_or(0)
        } else {
            0
        };
        prev..sel.end
    }
}

/// 光标向右移动。
/// - 没有 shift 且有选区：光标跳到选区终点
/// - 没有 shift 且无选区：向右进一个字符
/// - 有 shift：选区向右扩展
pub fn move_right(text: &str, sel: Range<usize>, shift: bool, _ctrl: bool) -> Range<usize> {
    let sel = clamp_to_boundary(text, sel);
    if !shift {
        if sel.start != sel.end {
            sel.end..sel.end
        } else if sel.end < text.len() {
            let next = text[sel.end..]
                .char_indices()
                .nth(1)
                .map(|(i, _)| sel.end + i)
                .unwrap_or(text.len());
            next..next
        } else {
            text.len()..text.len()
        }
    } else {
        let next = if sel.end < text.len() {
            text[sel.end..]
                .char_indices()
                .nth(1)
                .map(|(i, _)| sel.end + i)
                .unwrap_or(text.len())
        } else {
            text.len()
        };
        sel.start..next
    }
}

/// 移动到开头。
pub fn move_home(_text: &str, sel: Range<usize>, shift: bool) -> Range<usize> {
    if shift {
        0..sel.end
    } else {
        0..0
    }
}

/// 移动到末尾。
pub fn move_end(text: &str, sel: Range<usize>, shift: bool) -> Range<usize> {
    if shift {
        sel.start..text.len()
    } else {
        text.len()..text.len()
    }
}

/// Delete 键：向后删除一个字符或删除选区。
pub fn delete_forward(text: &mut String, sel: Range<usize>) -> Range<usize> {
    let sel = clamp_to_boundary(text, sel);
    if sel.start != sel.end {
        text.replace_range(sel.clone(), "");
        sel.start..sel.start
    } else if sel.start < text.len() {
        let next = text[sel.start..]
            .char_indices()
            .nth(1)
            .map(|(i, _)| sel.start + i)
            .unwrap_or(text.len());
        text.replace_range(sel.start..next, "");
        sel.start..sel.start
    } else {
        sel
    }
}

/// Backspace 键：向前删除一个字符或删除选区。
pub fn delete_backward(text: &mut String, sel: Range<usize>) -> Range<usize> {
    let sel = clamp_to_boundary(text, sel);
    if sel.start != sel.end {
        text.replace_range(sel.clone(), "");
        sel.start..sel.start
    } else if sel.start > 0 {
        let prev = text[..sel.start]
            .char_indices()
            .next_back()
            .map(|(i, _)| i)
            .unwrap_or(0);
        text.replace_range(prev..sel.start, "");
        prev..prev
    } else {
        sel
    }
}

impl Root {
    /// 当前搜索框里光标（或选区）的字节范围，已钳到合法边界。
    fn search_selection(&self) -> Range<usize> {
        clamp_to_boundary(&self.apps.search, self.apps.search_sel.clone())
    }

    /// 文件搜索框的选区（字节范围，已钳边界）。
    fn file_search_selection(&self) -> Range<usize> {
        clamp_to_boundary(&self.search.query, self.search.sel.clone())
    }

    /// 把光标收到末尾。内容被外部改动（清空按钮、切换筛选）后要调一次，
    /// 否则残留的旧偏移会指到字符串外面。
    pub fn reset_search_caret(&mut self) {
        let end = self.apps.search.len();
        self.apps.search_sel = end..end;
        self.apps.search_marked = None;
    }

    /// 退格：删掉光标前的一个字符或整个选区。
    pub fn search_backspace(&mut self) {
        self.apps.search_sel = delete_backward(&mut self.apps.search, self.apps.search_sel.clone());
        self.apps.search_marked = None;
    }

    pub fn apps_search_delete(&mut self) {
        self.apps.search_sel = delete_forward(&mut self.apps.search, self.apps.search_sel.clone());
        self.apps.search_marked = None;
    }

    pub fn apps_search_move_left(&mut self, shift: bool, ctrl: bool) {
        self.apps.search_sel = move_left(&self.apps.search, self.apps.search_sel.clone(), shift, ctrl);
        self.apps.search_marked = None;
    }

    pub fn apps_search_move_right(&mut self, shift: bool, ctrl: bool) {
        self.apps.search_sel = move_right(&self.apps.search, self.apps.search_sel.clone(), shift, ctrl);
        self.apps.search_marked = None;
    }

    pub fn apps_search_move_home(&mut self, shift: bool) {
        self.apps.search_sel = move_home(&self.apps.search, self.apps.search_sel.clone(), shift);
        self.apps.search_marked = None;
    }

    pub fn apps_search_move_end(&mut self, shift: bool) {
        self.apps.search_sel = move_end(&self.apps.search, self.apps.search_sel.clone(), shift);
        self.apps.search_marked = None;
    }

    pub fn file_search_move_left(&mut self, shift: bool, ctrl: bool, cx: &mut Context<Self>) {
        self.search.sel = move_left(&self.search.query, self.search.sel.clone(), shift, ctrl);
        self.search.marked = None;
        cx.notify();
    }

    pub fn file_search_move_right(&mut self, shift: bool, ctrl: bool, cx: &mut Context<Self>) {
        self.search.sel = move_right(&self.search.query, self.search.sel.clone(), shift, ctrl);
        self.search.marked = None;
        cx.notify();
    }

    pub fn file_search_move_home(&mut self, shift: bool, cx: &mut Context<Self>) {
        self.search.sel = move_home(&self.search.query, self.search.sel.clone(), shift);
        self.search.marked = None;
        cx.notify();
    }

    pub fn file_search_move_end(&mut self, shift: bool, cx: &mut Context<Self>) {
        self.search.sel = move_end(&self.search.query, self.search.sel.clone(), shift);
        self.search.marked = None;
        cx.notify();
    }

    pub fn file_search_delete(&mut self, cx: &mut Context<Self>) {
        self.search.sel = delete_forward(&mut self.search.query, self.search.sel.clone());
        self.search.marked = None;
        self.search_input_changed(cx);
    }

    pub fn search_clear(&mut self) {
        self.apps.search.clear();
        self.reset_search_caret();
    }

    /// 判断当前 IME 输入应该路由到哪个搜索框。
    /// 两个焦点句柄都不在焦点时默认走 apps 搜索框（保持向后兼容）。
    fn file_search_focused(&self, window: &Window) -> bool {
        self.search.focus_handle.is_focused(window)
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
        if self.file_search_focused(window) {
            let range = clamp_to_boundary(
                &self.search.query,
                range_from_utf16(&self.search.query, &range_utf16),
            );
            actual_range.replace(range_to_utf16(&self.search.query, &range));
            Some(self.search.query[range].to_string())
        } else {
            let range = clamp_to_boundary(
                &self.apps.search,
                range_from_utf16(&self.apps.search, &range_utf16),
            );
            actual_range.replace(range_to_utf16(&self.apps.search, &range));
            Some(self.apps.search[range].to_string())
        }
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        if self.file_search_focused(window) {
            Some(UTF16Selection {
                range: range_to_utf16(&self.search.query, &self.file_search_selection()),
                reversed: false,
            })
        } else {
            Some(UTF16Selection {
                range: range_to_utf16(&self.apps.search, &self.search_selection()),
                reversed: false,
            })
        }
    }

    fn marked_text_range(
        &self,
        window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        if self.file_search_focused(window) {
            self.search
                .marked
                .as_ref()
                .map(|r| range_to_utf16(&self.search.query, r))
        } else {
            self.apps
                .search_marked
                .as_ref()
                .map(|r| range_to_utf16(&self.apps.search, r))
        }
    }

    fn unmark_text(&mut self, window: &mut Window, _cx: &mut Context<Self>) {
        if self.file_search_focused(window) {
            self.search.marked = None;
        } else {
            self.apps.search_marked = None;
        }
    }

    /// 提交文本：普通打字与输入法确认后的汉字都走这里。
    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.file_search_focused(window) {
            let range = range_utf16
                .as_ref()
                .map(|r| range_from_utf16(&self.search.query, r))
                .or_else(|| self.search.marked.clone())
                .unwrap_or_else(|| self.file_search_selection());
            let range = clamp_to_boundary(&self.search.query, range);

            self.search.query.replace_range(range.clone(), new_text);
            let caret = range.start + new_text.len();
            self.search.sel = caret..caret;
            self.search.marked = None;
            self.search_input_changed(cx);
        } else {
            let range = range_utf16
                .as_ref()
                .map(|r| range_from_utf16(&self.apps.search, r))
                .or_else(|| self.apps.search_marked.clone())
                .unwrap_or_else(|| self.search_selection());
            let range = clamp_to_boundary(&self.apps.search, range);

            self.apps.search.replace_range(range.clone(), new_text);
            let caret = range.start + new_text.len();
            self.apps.search_sel = caret..caret;
            self.apps.search_marked = None;
            cx.notify();
        }
        self.poke_cursor_blink(cx);
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
        if self.file_search_focused(window) {
            let range = range_utf16
                .as_ref()
                .map(|r| range_from_utf16(&self.search.query, r))
                .or_else(|| self.search.marked.clone())
                .unwrap_or_else(|| self.file_search_selection());
            let range = clamp_to_boundary(&self.search.query, range);

            self.search.query.replace_range(range.clone(), new_text);
            self.search.marked = if new_text.is_empty() {
                None
            } else {
                Some(range.start..range.start + new_text.len())
            };

            let composed = &self.search.query[range.start..range.start + new_text.len()];
            self.search.sel = match new_selected_range_utf16.as_ref() {
                Some(r) => {
                    let inner = range_from_utf16(composed, r);
                    range.start + inner.start..range.start + inner.end
                }
                None => {
                    let caret = range.start + new_text.len();
                    caret..caret
                }
            };
            self.search_input_changed(cx);
        } else {
            let range = range_utf16
                .as_ref()
                .map(|r| range_from_utf16(&self.apps.search, r))
                .or_else(|| self.apps.search_marked.clone())
                .unwrap_or_else(|| self.search_selection());
            let range = clamp_to_boundary(&self.apps.search, range);

            self.apps.search.replace_range(range.clone(), new_text);
            self.apps.search_marked = if new_text.is_empty() {
                None
            } else {
                Some(range.start..range.start + new_text.len())
            };

            let composed = &self.apps.search[range.start..range.start + new_text.len()];
            self.apps.search_sel = match new_selected_range_utf16.as_ref() {
                Some(r) => {
                    let inner = range_from_utf16(composed, r);
                    range.start + inner.start..range.start + inner.end
                }
                None => {
                    let caret = range.start + new_text.len();
                    caret..caret
                }
            };
            cx.notify();
        }
        self.poke_cursor_blink(cx);
    }

    /// 输入法候选窗口的定位锚点。
    fn bounds_for_range(
        &mut self,
        _range_utf16: Range<usize>,
        element_bounds: Bounds<Pixels>,
        window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        if self.file_search_focused(window) {
            Some(self.search.bounds.unwrap_or(element_bounds))
        } else {
            Some(self.apps.search_bounds.unwrap_or(element_bounds))
        }
    }

    fn character_index_for_point(
        &mut self,
        point: gpui::Point<Pixels>,
        window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        if self.file_search_focused(window) {
            let bounds = self.search.bounds?;
            let text = &self.search.query;
            let text_start_x: f32 = f32::from(bounds.origin.x) + 34.0;
            let rel_x: f32 = f32::from(point.x) - text_start_x;
            let byte_idx = char_index_from_x(text, rel_x, 14.0);
            // EntityInputHandler 要求 UTF-16 索引
            Some(offset_to_utf16(text, byte_idx))
        } else {
            let bounds = self.apps.search_bounds?;
            let text = &self.apps.search;
            let text_start_x: f32 = f32::from(bounds.origin.x) + 33.0;
            let rel_x: f32 = f32::from(point.x) - text_start_x;
            let byte_idx = char_index_from_x(text, rel_x, 12.0);
            Some(offset_to_utf16(text, byte_idx))
        }
    }
}

/// 精确估算单个字符在 UI 渲染时的宽度（基于常见系统无衬线字体比例）
pub fn char_width(ch: char, font_size: f32) -> f32 {
    if ch.is_ascii() {
        match ch {
            'i' | 'l' | '|' | '!' | ':' | ';' | '.' | ',' | '\'' | '`' => font_size * 0.28,
            ' ' | 'j' | 't' | 'I' | '[' | ']' | '(' | ')' | '{' | '}' => font_size * 0.35,
            'f' | 'r' | '-' | '"' => font_size * 0.40,
            's' | 'z' | 'x' | 'c' | 'k' | 'v' | 'y' => font_size * 0.48,
            'a' | 'b' | 'd' | 'e' | 'g' | 'h' | 'n' | 'o' | 'p' | 'q' | 'u' => font_size * 0.53,
            '0'..='9' => font_size * 0.54,
            'M' | 'W' => font_size * 0.84,
            'C' | 'D' | 'G' | 'O' | 'Q' | 'U' => font_size * 0.70,
            'A'..='Z' => font_size * 0.65,
            'm' | 'w' => font_size * 0.78,
            '@' | '%' | '&' | '#' => font_size * 0.85,
            _ => font_size * 0.55,
        }
    } else if (ch >= '\u{4E00}' && ch <= '\u{9FFF}')
        || (ch >= '\u{3400}' && ch <= '\u{4DBF}')
        || (ch >= '\u{F900}' && ch <= '\u{FAFF}')
        || (ch >= '\u{3000}' && ch <= '\u{303F}')
        || (ch >= '\u{FF00}' && ch <= '\u{FFEF}')
    {
        // CJK 汉字与全角标点
        font_size * 1.0
    } else {
        font_size * 0.8
    }
}

/// 根据点击位置相对于文本起点的 X 偏移，估算对应的字节偏移。
pub fn char_index_from_x_with_cursor(
    text: &str,
    rel_x: f32,
    font_size: f32,
    existing_cursor: Option<usize>,
) -> usize {
    if rel_x <= 0.0 {
        return 0;
    }
    let mut acc_x = 0.0f32;
    let mut last_idx = 0;
    for (i, ch) in text.char_indices() {
        if let Some(cursor_pos) = existing_cursor {
            if i == cursor_pos {
                acc_x += 1.5;
            }
        }
        let char_w = char_width(ch, font_size);
        if acc_x + char_w * 0.5 >= rel_x {
            return i;
        }
        acc_x += char_w;
        last_idx = i + ch.len_utf8();
    }
    last_idx
}

pub fn char_index_from_x(text: &str, rel_x: f32, font_size: f32) -> usize {
    char_index_from_x_with_cursor(text, rel_x, font_size, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf16_and_byte_offsets_agree_on_ascii() {
        let s = "hello";
        for i in 0..=5 {
            assert_eq!(offset_from_utf16(s, i), i);
            assert_eq!(offset_to_utf16(s, i), i);
        }
    }

    /// 中文每字 UTF-8 三字节、UTF-16 一个码元，两套偏移必须能互转。
    #[test]
    fn utf16_and_byte_offsets_differ_on_cjk() {
        let s = "同花顺";
        assert_eq!(s.len(), 9);
        assert_eq!(offset_from_utf16(s, 0), 0);
        assert_eq!(offset_from_utf16(s, 1), 3);
        assert_eq!(offset_from_utf16(s, 2), 6);
        assert_eq!(offset_from_utf16(s, 3), 9);

        assert_eq!(offset_to_utf16(s, 0), 0);
        assert_eq!(offset_to_utf16(s, 3), 1);
        assert_eq!(offset_to_utf16(s, 9), 3);
    }

    #[test]
    fn offsets_clamp_instead_of_panicking() {
        let s = "同花顺";
        // 输入法偶尔会给出超出当前内容的范围，必须钳住而不是切片 panic
        assert_eq!(offset_from_utf16(s, 999), s.len());
        assert_eq!(offset_to_utf16(s, 999), 3);
    }

    #[test]
    fn clamp_snaps_to_char_boundaries() {
        let s = "同花顺";
        // 4 落在第二个字的中间，必须被拉到合法边界
        let r = clamp_to_boundary(s, 4..5);
        assert!(s.is_char_boundary(r.start) && s.is_char_boundary(r.end));
        assert_eq!(&s[r], "花");
    }

    #[test]
    fn mixed_ascii_and_cjk_round_trips() {
        let s = "ab同c花";
        for (byte, _) in s.char_indices().chain(std::iter::once((s.len(), ' '))) {
            let u = offset_to_utf16(s, byte);
            assert_eq!(offset_from_utf16(s, u), byte, "字节 {byte} 往返失败");
        }
    }

    #[test]
    fn test_move_left_right_home_end() {
        let s = "abc中文def";
        // 从末尾向左移动
        let len = s.len();
        let r1 = move_left(s, len..len, false, false);
        assert_eq!(&s[r1.start..], "f");
        
        // 跨越中文字符向左
        let idx_zhong = s.find("中").unwrap();
        let idx_wen = s.find("文").unwrap();
        let r2 = move_left(s, idx_wen..idx_wen, false, false);
        assert_eq!(r2, idx_zhong..idx_zhong);

        // 向右移动
        let r3 = move_right(s, idx_zhong..idx_zhong, false, false);
        assert_eq!(r3, idx_wen..idx_wen);

        // Home / End
        assert_eq!(move_home(s, 5..5, false), 0..0);
        assert_eq!(move_end(s, 0..0, false), len..len);

        // Shift 选区扩展
        assert_eq!(move_home(s, 5..5, true), 0..5);
        assert_eq!(move_end(s, 2..2, true), 2..len);
    }

    #[test]
    fn test_delete_forward_and_backward() {
        let mut s = "a中文b".to_string();
        // 光标在 "中" 前，向后删除 "中"
        let idx_zhong = s.find("中").unwrap();
        let next_sel = delete_forward(&mut s, idx_zhong..idx_zhong);
        assert_eq!(s, "a文b");
        assert_eq!(next_sel, idx_zhong..idx_zhong);

        // 退格删除 "a"
        let next_sel2 = delete_backward(&mut s, 1..1);
        assert_eq!(s, "文b");
        assert_eq!(next_sel2, 0..0);

        // 删除选区
        let mut s2 = "hello world".to_string();
        let next_sel3 = delete_backward(&mut s2, 5..11);
        assert_eq!(s2, "hello");
        assert_eq!(next_sel3, 5..5);
    }

    #[test]
    fn test_char_index_from_x_numbers() {
        let text = "4444";
        let font_size = 14.0;

        // 点击在第 2 个 '4' 和第 3 个 '4' 之间（约 15.12px 左右）
        // 缝隙前后宽容度：12.0px ~ 18.0px 均应准确判定为 index 2
        assert_eq!(char_index_from_x_with_cursor(text, 14.0, font_size, None), 2);
        assert_eq!(char_index_from_x_with_cursor(text, 15.12, font_size, None), 2);
        assert_eq!(char_index_from_x_with_cursor(text, 16.0, font_size, None), 2);

        // 如果之前光标在末尾（index 4），再次点击第 2 和第 3 个 '4' 之间
        assert_eq!(char_index_from_x_with_cursor(text, 15.12, font_size, Some(4)), 2);

        // 点击在最前面
        assert_eq!(char_index_from_x_with_cursor(text, 1.0, font_size, None), 0);
        // 点击在最后面
        assert_eq!(char_index_from_x_with_cursor(text, 40.0, font_size, None), 4);
    }
}
