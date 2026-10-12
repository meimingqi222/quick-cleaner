//! 指标格：小字标签在上、加粗数值在下，可选左侧图标。
//!
//! 数值颜色由调用方按语义选（主色 / 正文 / 警示），组件不判断数值含义。

use crate::ui::components::cards::card;
use crate::ui::theme::*;
use gpui::{
    div, prelude::*, px, rgb, AnyElement, App, FontWeight, IntoElement, RenderOnce, SharedString,
    Window,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum StatTileStyle {
    /// 独立卡片（`card()`，px_4 / py_3，数值 text_base），用于页面顶部的指标行。
    #[default]
    Card,
    /// 卡片内的小方格（p_2，淡边框无阴影，数值 text_sm），用于详情面板。
    Inset,
}

#[derive(IntoElement)]
pub struct StatTile {
    label: SharedString,
    value: SharedString,
    value_color: u32,
    icon: Option<AnyElement>,
    style: StatTileStyle,
}

impl StatTile {
    pub fn new(label: impl Into<SharedString>, value: impl Into<SharedString>) -> Self {
        Self {
            label: label.into(),
            value: value.into(),
            value_color: TEXT,
            icon: None,
            style: StatTileStyle::default(),
        }
    }

    pub fn value_color(mut self, color: u32) -> Self {
        self.value_color = color;
        self
    }

    /// 左侧图标，通常是 `icon_badge(..)`。
    pub fn icon(mut self, icon: impl IntoElement) -> Self {
        self.icon = Some(icon.into_any_element());
        self
    }

    pub fn style(mut self, style: StatTileStyle) -> Self {
        self.style = style;
        self
    }
}

impl RenderOnce for StatTile {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let card_style = self.style == StatTileStyle::Card;
        let label_weight = if card_style {
            FontWeight::MEDIUM
        } else {
            FontWeight::NORMAL
        };
        let text = div()
            .flex()
            .flex_col()
            .justify_center()
            .gap(px(1.))
            .child(
                div()
                    .text_xs()
                    .font_weight(label_weight)
                    .text_color(rgb(OUTLINE))
                    .child(self.label),
            )
            .child(
                div()
                    .when(card_style, |d| d.text_base())
                    .when(!card_style, |d| d.text_sm())
                    .font_weight(FontWeight::BOLD)
                    .text_color(rgb(self.value_color))
                    .child(self.value),
            );

        let shell = match self.style {
            StatTileStyle::Card => card().min_w(px(140.)).px_4().py_3().gap_3(),
            StatTileStyle::Inset => div()
                .p_2()
                .rounded_md()
                .bg(rgb(CARD))
                .border_1()
                .border_color(rgba(OUTLINE_VAR, 0.3)),
        };
        shell
            .flex_1()
            .flex()
            .items_center()
            .children(self.icon)
            .child(text)
    }
}
