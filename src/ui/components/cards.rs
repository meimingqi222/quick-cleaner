//! 卡片容器控件

use crate::ui::theme::*;
use gpui::{div, prelude::*, rgb, Div};

/// 白色卡片：设计稿里所有内容块的统一容器
pub fn card() -> Div {
    div()
        .rounded_xl()
        .bg(rgb(CARD))
        .border_1()
        .border_color(rgba(OUTLINE_VAR, 0.55))
        .shadow_sm()
}
