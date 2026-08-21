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
    fill, point, px, rgb, size, App, Bounds, Context, EntityInputHandler, FontWeight, Hsla,
    LineLayout, Pixels, ShapedLine, SharedString, TextRun, UTF16Selection, UnderlineStyle, Window,
};
use std::ops::Range;
use std::sync::Arc;

use crate::ui::theme::{rgba, OUTLINE, PRIMARY, TEXT};
use crate::ui::{Root, SearchTextHit};

/// 搜索框绘制与命中测试共用的字体：系统 UI 字体 + Medium。
///
/// 不能用 `.ZedSans`（IBM Plex Sans）：界面 div 默认走 `.SystemUIFont`
/// （Windows 上是 Segoe UI），两套字宽不一致，点在 `cd` 之间会落到 `b` 后面。
pub fn search_box_font(window: &Window) -> gpui::Font {
    let mut font = window.text_style().font();
    font.weight = FontWeight::MEDIUM;
    font
}

fn search_text_run(len: usize, font: gpui::Font, color: Hsla, underline: bool) -> TextRun {
    TextRun {
        len,
        font,
        color,
        background_color: None,
        underline: underline.then_some(UnderlineStyle {
            color: Some(color),
            thickness: px(1.0),
            wavy: false,
        }),
        strikethrough: None,
    }
}

/// 把搜索框文本排成可绘制、可命中的单行。换行会让 `shape_line` panic，先压成空格。
pub fn shape_search_line(
    text: &str,
    font_size: f32,
    color: Hsla,
    marked: Option<&Range<usize>>,
    window: &Window,
) -> ShapedLine {
    let display = SharedString::from(text.replace('\n', " "));
    let font = search_box_font(window);
    let runs = if display.is_empty() {
        Vec::new()
    } else if let Some(marked) = marked {
        let marked = clamp_to_boundary(display.as_ref(), marked.clone());
        let mut runs = Vec::new();
        if marked.start > 0 {
            runs.push(search_text_run(marked.start, font.clone(), color, false));
        }
        if marked.end > marked.start {
            runs.push(search_text_run(
                marked.end - marked.start,
                font.clone(),
                color,
                true,
            ));
        }
        if marked.end < display.len() {
            runs.push(search_text_run(
                display.len() - marked.end,
                font,
                color,
                false,
            ));
        }
        runs
    } else {
        vec![search_text_run(display.len(), font, color, false)]
    };
    window
        .text_system()
        .shape_line(display, px(font_size), &runs, None)
}

/// 使用 GPUI 真实字体排版系统测量单行文本（字体/字重与屏幕绘制一致）。
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
            font: search_box_font(window),
            color: gpui::black(),
            background_color: None,
            underline: None,
            strikethrough: None,
        }]
    };
    window
        .text_system()
        .layout_line(text, px(font_size), &runs, None)
}

/// 在字符边界里找距离 `rel_x` 最近的那一处（含行尾）。
///
/// GPUI 的 `closest_index_for_x` 拿的是各字形左边缘，行尾不参与比较；
/// 这里按「字符左半边点到前、右半边点到后」走完整边界。
pub fn closest_boundary_for_x(
    text: &str,
    rel_x: f32,
    x_for_index: impl Fn(usize) -> f32,
    line_width: f32,
) -> usize {
    if text.is_empty() || rel_x <= 0.0 {
        return 0;
    }
    let mut best_idx = 0;
    let mut best_dist = rel_x.abs();
    for (idx, _) in text.char_indices().skip(1) {
        let dist = (x_for_index(idx) - rel_x).abs();
        if dist < best_dist {
            best_dist = dist;
            best_idx = idx;
        }
    }
    if (line_width - rel_x).abs() < best_dist {
        text.len()
    } else {
        clamp_to_boundary(text, best_idx..best_idx).start
    }
}

/// 根据相对文本起点的 x 像素坐标，计算对应的字符字节索引。
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
    closest_boundary_for_x(
        text,
        rel_x,
        |idx| f32::from(layout.x_for_index(idx)),
        f32::from(layout.width),
    )
}

/// 用上一帧画出来的文本区 bounds + ShapedLine 做命中；文本变了再现场 layout。
pub fn index_for_mouse_x(
    text: &str,
    mouse_x: f32,
    hit: Option<&SearchTextHit>,
    font_size: f32,
    window: &mut Window,
) -> usize {
    if text.is_empty() {
        return 0;
    }
    let Some(hit) = hit else {
        return text.len();
    };
    let rel_x = mouse_x - f32::from(hit.bounds.origin.x);
    if rel_x <= 0.0 {
        return 0;
    }
    if hit.line.text.as_ref() == text {
        closest_boundary_for_x(
            text,
            rel_x,
            |idx| f32::from(hit.line.x_for_index(idx)),
            f32::from(hit.line.width),
        )
    } else {
        closest_index_for_x_layout(text, rel_x, font_size, window)
    }
}

/// `paint_search_text` 的绘制参数。
pub struct SearchTextPaint<'a> {
    pub text: &'a str,
    pub placeholder: &'a str,
    pub sel: &'a Range<usize>,
    pub marked: Option<&'a Range<usize>>,
    pub font_size: f32,
    pub cursor_h: f32,
    pub focused: bool,
    pub cursor_visible: bool,
}

/// 在文本区 canvas 上画出文字、选区、光标，并返回这份排版供下一帧点击使用。
pub fn paint_search_text(
    window: &mut Window,
    cx: &mut App,
    bounds: Bounds<Pixels>,
    spec: SearchTextPaint<'_>,
) -> SearchTextHit {
    let SearchTextPaint {
        text,
        placeholder,
        sel,
        marked,
        font_size,
        cursor_h,
        focused,
        cursor_visible,
    } = spec;
    let line_height = bounds.size.height;
    let is_empty = text.is_empty();
    let color = Hsla::from(rgb(if is_empty { OUTLINE } else { TEXT }));
    let paint_src = if is_empty { placeholder } else { text };
    let marked = if is_empty { None } else { marked };
    let line = shape_search_line(paint_src, font_size, color, marked, window);

    if focused && !is_empty && sel.start < sel.end {
        let x1 = f32::from(line.x_for_index(sel.start.min(line.len())));
        let x2 = f32::from(line.x_for_index(sel.end.min(line.len())));
        let sel_h = font_size + 4.0;
        let y_off = ((f32::from(line_height) - sel_h) / 2.0).max(0.0);
        window.paint_quad(fill(
            Bounds::new(
                point(bounds.origin.x + px(x1), bounds.origin.y + px(y_off)),
                size(px((x2 - x1).max(2.0)), px(sel_h)),
            ),
            rgba(PRIMARY, 0.28),
        ));
    }

    let _ = line.paint(bounds.origin, line_height, window, cx);

    if focused && sel.start == sel.end && cursor_visible {
        let caret_x = if is_empty {
            0.0
        } else {
            f32::from(line.x_for_index(sel.start.min(line.len())))
        };
        let y_off = ((f32::from(line_height) - cursor_h) / 2.0).max(0.0);
        window.paint_quad(fill(
            Bounds::new(
                point(bounds.origin.x + px(caret_x), bounds.origin.y + px(y_off)),
                size(px(1.5), px(cursor_h)),
            ),
            rgb(PRIMARY),
        ));
    }

    let stored = if is_empty {
        shape_search_line("", font_size, color, None, window)
    } else {
        line
    };
    SearchTextHit {
        bounds,
        line: stored,
    }
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

/// 光标向左移动。
/// - 没有 shift 且有选区：光标跳到选区起点
/// - 没有 shift 且无选区：向左退一个字符
/// - 有 shift：选区向左扩展
pub fn move_left(text: &str, sel: Range<usize>, shift: bool) -> Range<usize> {
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
pub fn move_right(text: &str, sel: Range<usize>, shift: bool) -> Range<usize> {
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

    pub fn apps_search_move_left(&mut self, shift: bool) {
        self.apps.search_sel = move_left(&self.apps.search, self.apps.search_sel.clone(), shift);
        self.apps.search_marked = None;
    }

    pub fn apps_search_move_right(&mut self, shift: bool) {
        self.apps.search_sel = move_right(&self.apps.search, self.apps.search_sel.clone(), shift);
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

    pub fn file_search_move_left(&mut self, shift: bool, cx: &mut Context<Self>) {
        self.search.sel = move_left(&self.search.query, self.search.sel.clone(), shift);
        self.search.marked = None;
        cx.notify();
    }

    pub fn file_search_move_right(&mut self, shift: bool, cx: &mut Context<Self>) {
        self.search.sel = move_right(&self.search.query, self.search.sel.clone(), shift);
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
        range_utf16: Range<usize>,
        element_bounds: Bounds<Pixels>,
        window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let (text, hit, box_bounds) = if self.file_search_focused(window) {
            (
                self.search.query.as_str(),
                self.search.text_hit.as_ref(),
                self.search.bounds,
            )
        } else {
            (
                self.apps.search.as_str(),
                self.apps.text_hit.as_ref(),
                self.apps.search_bounds,
            )
        };
        if let Some(hit) = hit {
            let range = clamp_to_boundary(text, range_from_utf16(text, &range_utf16));
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
        Some(box_bounds.unwrap_or(element_bounds))
    }

    fn character_index_for_point(
        &mut self,
        point: gpui::Point<Pixels>,
        window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        let (text, hit, font_size) = if self.file_search_focused(window) {
            (
                self.search.query.as_str(),
                self.search.text_hit.as_ref(),
                13.0,
            )
        } else {
            (self.apps.search.as_str(), self.apps.text_hit.as_ref(), 12.0)
        };
        let byte_idx = index_for_mouse_x(text, f32::from(point.x), hit, font_size, window);
        Some(offset_to_utf16(text, byte_idx))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closest_boundary_picks_nearest_char_edge() {
        // 模拟 "abcd"，每字 8px：a=0-8, b=8-16, c=16-24, d=24-32
        let text = "abcd";
        let x_of = |idx: usize| idx as f32 * 8.0;
        let width = 32.0;
        // 点在 cd 之间（x=24）→ c 后面（index 3）
        assert_eq!(closest_boundary_for_x(text, 24.0, x_of, width), 3);
        // 点在 c 的右半（x=21）→ c 后面
        assert_eq!(closest_boundary_for_x(text, 21.0, x_of, width), 3);
        // 点在 c 的左半（x=18）→ b 后面
        assert_eq!(closest_boundary_for_x(text, 18.0, x_of, width), 2);
        // 点在 b 与 c 交界（x=16）→ b 后面
        assert_eq!(closest_boundary_for_x(text, 16.0, x_of, width), 2);
        // 点在 d 的右半 → 行尾
        assert_eq!(closest_boundary_for_x(text, 29.0, x_of, width), 4);
        // 点在起点左侧
        assert_eq!(closest_boundary_for_x(text, -3.0, x_of, width), 0);
    }

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
        let r1 = move_left(s, len..len, false);
        assert_eq!(&s[r1.start..], "f");

        // 跨越中文字符向左
        let idx_zhong = s.find("中").unwrap();
        let idx_wen = s.find("文").unwrap();
        let r2 = move_left(s, idx_wen..idx_wen, false);
        assert_eq!(r2, idx_zhong..idx_zhong);

        // 向右移动
        let r3 = move_right(s, idx_zhong..idx_zhong, false);
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
}
