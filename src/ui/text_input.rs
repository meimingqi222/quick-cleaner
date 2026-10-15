//! 输入法（IME）支持的产品适配。
//!
//! 光靠 `on_key_down` + `keystroke.key_char` 收不到中文：输入法先进入**组合**状态
//! （拼音串），确认后才产出文字，GPUI 只有在注册了输入处理器之后才会把这些消息转发过来。
//! 注册、转发、排版、命中和 UTF-16 偏移换算都在 `gpui_app_widgets::input` /
//! `gpui_app_widgets::text`；这里只保留搜索框的字重与配色。
//!
//! 注意：接上输入处理器之后，**普通 ASCII 字符也会走这条路**——GPUI 的 Windows
//! 后端把 `WM_CHAR` 同样转成 `replace_text_in_range`。所以 `on_key_down`
//! 里绝不能再自己追加字符，否则每个字母都会输入两遍。

use crate::ui::components::input::InputPaintStyle;
use crate::ui::theme::{rgba, OUTLINE, PRIMARY, TEXT};
use gpui::{rgb, FontWeight, Hsla};

/// 搜索框绘制与命中测试共用的字重；字体族沿用界面的系统 UI 字体。
///
/// 不能换成 `.ZedSans`（IBM Plex Sans）：界面 div 默认走 `.SystemUIFont`
/// （Windows 上是 Segoe UI），两套字宽不一致，点在 `cd` 之间会落到 `b` 后面。
pub const SEARCH_FONT_WEIGHT: FontWeight = FontWeight::MEDIUM;

pub fn search_paint_style() -> InputPaintStyle {
    InputPaintStyle {
        text: Hsla::from(rgb(TEXT)),
        placeholder: Hsla::from(rgb(OUTLINE)),
        selection: rgba(PRIMARY, 0.28),
        caret: Hsla::from(rgb(PRIMARY)),
    }
}

pub use gpui_app_widgets::text::*;
