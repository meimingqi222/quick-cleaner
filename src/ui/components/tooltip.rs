//! 悬停全文提示
//!
//! 检索结果、面包屑这类格子宽度不够，名字会被截断。GPUI 的 `.tooltip()`
//! 在指针停下约 500ms 后弹出一个 `AnyView`；这里提供统一的浅色卡片样式，
//! 长路径按分隔符折行，避免一条撑出窗口。

use crate::ui::theme::*;
use gpui::{
    div, prelude::*, px, rgb, AnyView, App, Context, FontWeight, IntoElement, Render, SharedString,
    Window,
};

struct TextTooltip {
    text: SharedString,
}

impl Render for TextTooltip {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("text-tooltip")
            .px_3()
            .py_2()
            .max_w(px(520.))
            .rounded_md()
            .bg(rgb(CARD))
            .border_1()
            .border_color(rgba(OUTLINE_VAR, 0.85))
            .shadow_lg()
            .text_xs()
            .font_weight(FontWeight::MEDIUM)
            .text_color(rgb(TEXT))
            .whitespace_normal()
            .child(self.text.clone())
    }
}

/// 在 `/`、`\` 后插入零宽空格，让本来没有空格的路径能在 `max_w` 内折行。
fn breakable_path(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + s.len() / 8);
    for ch in s.chars() {
        out.push(ch);
        if ch == '/' || ch == '\\' {
            out.push('\u{200B}');
        }
    }
    out
}

/// 给某个已 `.id(...)` 的元素挂全文悬停提示。
pub fn text_tooltip(text: impl Into<SharedString>) -> impl Fn(&mut Window, &mut App) -> AnyView {
    let text = text.into();
    move |_, cx| cx.new(|_| TextTooltip { text: text.clone() }).into()
}

/// 路径专用：折行后再交给 [`text_tooltip`]。
pub fn path_tooltip(path: &str) -> impl Fn(&mut Window, &mut App) -> AnyView {
    text_tooltip(breakable_path(path))
}
