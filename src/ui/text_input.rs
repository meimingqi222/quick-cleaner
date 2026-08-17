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

use gpui::{Bounds, Context, EntityInputHandler, Pixels, UTF16Selection, Window};
use std::ops::Range;

use crate::ui::Root;

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
fn clamp_to_boundary(s: &str, r: Range<usize>) -> Range<usize> {
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

impl Root {
    /// 当前搜索框里光标（或选区）的字节范围，已钳到合法边界。
    fn search_selection(&self) -> Range<usize> {
        clamp_to_boundary(&self.apps.search, self.apps.search_sel.clone())
    }

    /// 把光标收到末尾。内容被外部改动（清空按钮、切换筛选）后要调一次，
    /// 否则残留的旧偏移会指到字符串外面。
    pub fn reset_search_caret(&mut self) {
        let end = self.apps.search.len();
        self.apps.search_sel = end..end;
        self.apps.search_marked = None;
    }

    /// 退格：删掉光标前的一个**字符**（不是一个字节）。
    ///
    /// 中文一个字占 3 字节，按字节退会把 UTF-8 序列截断成非法串。
    pub fn search_backspace(&mut self) {
        let sel = self.search_selection();
        if sel.start != sel.end {
            self.apps.search.replace_range(sel.clone(), "");
            self.apps.search_sel = sel.start..sel.start;
        } else if sel.start > 0 {
            let prev = self.apps.search[..sel.start]
                .char_indices()
                .next_back()
                .map(|(i, _)| i)
                .unwrap_or(0);
            self.apps.search.replace_range(prev..sel.start, "");
            self.apps.search_sel = prev..prev;
        }
        self.apps.search_marked = None;
    }

    pub fn search_clear(&mut self) {
        self.apps.search.clear();
        self.reset_search_caret();
    }
}

impl EntityInputHandler for Root {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        actual_range: &mut Option<Range<usize>>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<String> {
        let range = clamp_to_boundary(
            &self.apps.search,
            range_from_utf16(&self.apps.search, &range_utf16),
        );
        actual_range.replace(range_to_utf16(&self.apps.search, &range));
        Some(self.apps.search[range].to_string())
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: range_to_utf16(&self.apps.search, &self.search_selection()),
            reversed: false,
        })
    }

    fn marked_text_range(
        &self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        self.apps
            .search_marked
            .as_ref()
            .map(|r| range_to_utf16(&self.apps.search, r))
    }

    fn unmark_text(&mut self, _window: &mut Window, _cx: &mut Context<Self>) {
        self.apps.search_marked = None;
    }

    /// 提交文本：普通打字与输入法确认后的汉字都走这里。
    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
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

    /// 组合中的文本：输入法候选阶段的拼音串，尚未确认。
    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_range_utf16: Option<Range<usize>>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
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

        // 输入法给的选区是相对于这段组合文本的
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

    /// 输入法候选窗口的定位锚点。
    ///
    /// 返回搜索框自身的位置，候选窗口就会贴着它显示；返回 None 或错误
    /// 位置会让候选框跑到屏幕左上角。
    fn bounds_for_range(
        &mut self,
        _range_utf16: Range<usize>,
        element_bounds: Bounds<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        Some(self.apps.search_bounds.unwrap_or(element_bounds))
    }

    fn character_index_for_point(
        &mut self,
        _point: gpui::Point<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        // 搜索框不支持点击定位光标，光标恒在末尾
        None
    }
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
}
