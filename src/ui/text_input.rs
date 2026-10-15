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
    fill, point, px, rgb, size, App, Bounds, FontWeight, Hsla, LineLayout, Pixels, ShapedLine,
    SharedString, TextRun, UnderlineStyle, Window,
};
use std::ops::Range;
use std::sync::Arc;

use crate::ui::components::input::SearchTextHit;
use crate::ui::theme::{rgba, OUTLINE, PRIMARY, TEXT};

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

pub use gpui_app_widgets::text::*;

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

    /// 普通打字：没有选区、没有组合中范围，就在光标处插入。
    #[test]
    fn replace_inserts_at_caret() {
        let mut text = "ab".to_string();
        let (sel, marked) = replace_text(&mut text, &(1..1), &None, None, "X");
        assert_eq!(text, "aXb");
        assert_eq!(sel, 2..2);
        assert_eq!(marked, None);
    }

    /// 有选区时提交文本替换整个选区。
    #[test]
    fn replace_overwrites_selection() {
        let mut text = "abcd".to_string();
        let (sel, _) = replace_text(&mut text, &(1..3), &None, None, "X");
        assert_eq!(text, "aXd");
        assert_eq!(sel, 2..2);
    }

    /// 组合中范围优先于选区：输入法确认汉字时替换的是拼音串，
    /// 不是用户之前选中的那段——两者搞反会把已有文字吃掉。
    #[test]
    fn replace_prefers_marked_range_over_selection() {
        let mut text = "ni hao".to_string();
        let (sel, marked) = replace_text(&mut text, &(5..6), &Some(0..2), None, "你");
        assert_eq!(text, "你 hao");
        assert_eq!(sel, 3..3);
        assert_eq!(marked, None, "确认之后不再有组合中范围");
    }

    /// 输入法候选阶段：拼音串被标为组合中，选区落在候选窗给的子范围上。
    #[test]
    fn replace_and_mark_tracks_the_composing_run() {
        let mut text = String::new();
        let (sel, marked) = replace_and_mark_text(&mut text, &(0..0), &None, None, "ni", None);
        assert_eq!(text, "ni");
        assert_eq!(marked, Some(0..2));
        assert_eq!(sel, 2..2);

        // 继续输入：新的组合中文本替换掉上一段组合中文本
        let (sel, marked) =
            replace_and_mark_text(&mut text, &sel, &marked, None, "nihao", Some(&(1..3)));
        assert_eq!(text, "nihao");
        assert_eq!(marked, Some(0..5));
        assert_eq!(sel, 1..3, "选区来自候选窗给的 UTF-16 子范围");
    }

    /// 组合被清空（按 esc 或删光拼音）时组合中范围必须归零，
    /// 否则下一次输入会去替换一段已经不存在的文本。
    #[test]
    fn replace_and_mark_clears_marked_on_empty_text() {
        let mut text = "ni".to_string();
        let (sel, marked) = replace_and_mark_text(&mut text, &(2..2), &Some(0..2), None, "", None);
        assert_eq!(text, "");
        assert_eq!(marked, None);
        assert_eq!(sel, 0..0);
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
        assert_eq!(move_home(s, 3..3, true), Range { start: 3, end: 0 });
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
