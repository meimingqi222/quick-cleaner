//! 徽章 / 胶囊 / 标签
//!
//! 只认中性色调，不认识领域类型：安全等级由调用方用
//! `theme::safety_color` / `safety_container` 或 `Safe→Primary / Caution→Caution /
//! Danger→Error` 转成 [`BadgeTone`]。
//!
//! 布局约束（PITFALLS P3）：徽章不能指望文字自己省略。徽章放在窄容器里时调
//! [`Badge::clip`]，靠 `overflow_hidden` 兜底裁剪；省略号只是锦上添花，gpui 的
//! `text_ellipsis` 在 nowrap 文本上会沿用首次 MaxContent 测量结果而不重新截断。
//! 文字过长的问题仍要在数据源头处理。

use crate::ui::components::ClickHandler;
use crate::ui::theme::*;
use gpui::{
    div, prelude::*, px, rgb, AnyElement, App, ClickEvent, ElementId, FontWeight, Hsla,
    IntoElement, Pixels, RenderOnce, SharedString, Window,
};
use std::rc::Rc;

/// 配色。`Custom` 的颜色可带透明度。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BadgeTone {
    /// SURF_HIGH / MUTED
    Neutral,
    /// SURF_LOW / MUTED，比 `Neutral` 更淡，不抢主信息
    NeutralSoft,
    /// PRIMARY_FIXED / PRIMARY
    Primary,
    /// CAUTION_CONTAINER / CAUTION
    Caution,
    /// ERROR_CONTAINER / ERROR
    Error,
    /// ACCENT_CONTAINER / ON_ACCENT_CONTAINER
    Accent,
    Custom {
        bg: Hsla,
        fg: Hsla,
    },
}

impl BadgeTone {
    /// 十六进制底色 + 前景色，不透明。
    pub fn custom(bg: u32, fg: u32) -> Self {
        Self::Custom {
            bg: rgb(bg).into(),
            fg: rgb(fg).into(),
        }
    }

    /// (底色, 前景色)
    pub fn colors(self) -> (Hsla, Hsla) {
        let hex = |bg: u32, fg: u32| (rgb(bg).into(), rgb(fg).into());
        match self {
            Self::Neutral => hex(SURF_HIGH, MUTED),
            Self::NeutralSoft => hex(SURF_LOW, MUTED),
            Self::Primary => hex(PRIMARY_FIXED, PRIMARY),
            Self::Caution => hex(CAUTION_CONTAINER, CAUTION),
            Self::Error => hex(ERROR_CONTAINER, ERROR),
            Self::Accent => hex(ACCENT_CONTAINER, ON_ACCENT_CONTAINER),
            Self::Custom { bg, fg } => (bg, fg),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BadgeShape {
    /// rounded_full
    Pill,
    /// rounded_md
    Rounded,
    /// 3px 小圆角，端口号这类紧凑标签
    Tag,
}

/// 内边距档位（水平, 垂直）。其他值用 [`Badge::padding`] 覆盖。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BadgeSize {
    /// 8 / 2
    Regular,
    /// 6 / 1
    Compact,
    /// 4 / 0.5
    Tiny,
}

impl BadgeSize {
    pub fn padding(self) -> (f32, f32) {
        match self {
            Self::Regular => (8., 2.),
            Self::Compact => (6., 1.),
            Self::Tiny => (4., 0.5),
        }
    }
}

#[derive(IntoElement)]
pub struct Badge {
    text: SharedString,
    tone: BadgeTone,
    shape: BadgeShape,
    size: BadgeSize,
    padding: Option<(f32, f32)>,
    weight: FontWeight,
    border: Option<Hsla>,
    gap: f32,
    leading: Option<AnyElement>,
    trailing: Option<AnyElement>,
    clip: bool,
    no_shrink: bool,
    click: Option<(ElementId, Hsla, ClickHandler)>,
}

impl Badge {
    pub fn new(text: impl Into<SharedString>) -> Self {
        Self {
            text: text.into(),
            tone: BadgeTone::Neutral,
            shape: BadgeShape::Rounded,
            size: BadgeSize::Regular,
            padding: None,
            weight: FontWeight::MEDIUM,
            border: None,
            gap: 4.,
            leading: None,
            trailing: None,
            clip: false,
            no_shrink: false,
            click: None,
        }
    }

    pub fn tone(mut self, tone: BadgeTone) -> Self {
        self.tone = tone;
        self
    }

    pub fn shape(mut self, shape: BadgeShape) -> Self {
        self.shape = shape;
        self
    }

    pub fn size(mut self, size: BadgeSize) -> Self {
        self.size = size;
        self
    }

    /// 覆盖 `size` 的内边距（水平, 垂直，px）。
    pub fn padding(mut self, x: f32, y: f32) -> Self {
        self.padding = Some((x, y));
        self
    }

    pub fn weight(mut self, weight: FontWeight) -> Self {
        self.weight = weight;
        self
    }

    pub fn border(mut self, color: Hsla) -> Self {
        self.border = Some(color);
        self
    }

    /// 文字与前后插槽的间距（px）。
    pub fn gap(mut self, gap: f32) -> Self {
        self.gap = gap;
        self
    }

    /// 文字前的插槽：图标，或「标签 + 数值」里的标签段。
    pub fn leading(mut self, el: impl IntoElement) -> Self {
        self.leading = Some(el.into_any_element());
        self
    }

    pub fn trailing(mut self, el: impl IntoElement) -> Self {
        self.trailing = Some(el.into_any_element());
        self
    }

    /// 窄容器里的徽章：可收缩，装不下就裁剪（P3）。
    ///
    /// `truncate` 里的 `overflow_hidden` 同时把自动最小宽度归零，否则文字的
    /// 最小内容宽度会顶着徽章不缩。只有文字直接是子节点时才会出省略号。
    pub fn clip(mut self) -> Self {
        self.clip = true;
        self
    }

    /// 禁止被 flex 挤压。
    pub fn no_shrink(mut self) -> Self {
        self.no_shrink = true;
        self
    }

    /// 可点击徽章（如「已选 N 项 ✕」一键清空）。`hover_bg` 是悬停底色。
    pub fn on_click(
        mut self,
        id: impl Into<ElementId>,
        hover_bg: Hsla,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.click = Some((id.into(), hover_bg, Rc::new(handler)));
        self
    }
}

impl RenderOnce for Badge {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let (bg, fg) = self.tone.colors();
        let (px_x, px_y) = self.padding.unwrap_or_else(|| self.size.padding());
        let has_slots = self.leading.is_some() || self.trailing.is_some();

        let base = div()
            .px(px(px_x))
            .py(px(px_y))
            .bg(bg)
            .text_xs()
            .font_weight(self.weight)
            .text_color(fg);
        let mut el = match self.shape {
            BadgeShape::Pill => base.rounded_full(),
            BadgeShape::Rounded => base.rounded_md(),
            BadgeShape::Tag => base.rounded(px(3.)),
        };
        if let Some(color) = self.border {
            el = el.border_1().border_color(color);
        }
        if self.no_shrink {
            el = el.flex_none();
        }
        if self.clip {
            el = el.flex_shrink().min_w(px(0.)).truncate();
        }
        if has_slots {
            let gap: Pixels = px(self.gap);
            el = el.flex().items_center().gap(gap);
        }
        let el = el
            .children(self.leading)
            .child(self.text)
            .children(self.trailing);

        match self.click {
            Some((id, hover_bg, handler)) => el
                .id(id)
                .cursor_pointer()
                .hover(move |h| h.bg(hover_bg))
                .on_click(move |event, window, cx| handler(event, window, cx))
                .into_any_element(),
            None => el.into_any_element(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{Context, Render, TestAppContext};

    #[test]
    fn tone_maps_to_theme_colors() {
        let hex = |c: u32| Hsla::from(rgb(c));
        assert_eq!(
            BadgeTone::Caution.colors(),
            (hex(CAUTION_CONTAINER), hex(CAUTION))
        );
        assert_eq!(
            BadgeTone::Error.colors(),
            (hex(ERROR_CONTAINER), hex(ERROR))
        );
        assert_eq!(
            BadgeTone::Primary.colors(),
            (hex(PRIMARY_FIXED), hex(PRIMARY))
        );
        assert_eq!(BadgeTone::Neutral.colors(), (hex(SURF_HIGH), hex(MUTED)));
        assert_eq!(BadgeTone::NeutralSoft.colors(), (hex(SURF_LOW), hex(MUTED)));
        assert_eq!(
            BadgeTone::custom(CARD, OUTLINE).colors(),
            (hex(CARD), hex(OUTLINE))
        );
    }

    #[test]
    fn size_presets_match_legacy_call_sites() {
        assert_eq!(BadgeSize::Regular.padding(), (8., 2.));
        assert_eq!(BadgeSize::Compact.padding(), (6., 1.));
        assert_eq!(BadgeSize::Tiny.padding(), (4., 0.5));
    }

    struct Fixture;

    impl Render for Fixture {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
                .w(px(120.))
                .flex()
                .gap_2()
                .child(Badge::new("plain"))
                .child(
                    Badge::new("pill")
                        .shape(BadgeShape::Pill)
                        .tone(BadgeTone::Primary)
                        .size(BadgeSize::Compact)
                        .border(rgba(OUTLINE_VAR, 0.4))
                        .leading(div().child("L"))
                        .trailing(div().child("T")),
                )
                .child(
                    Badge::new("a very long operating system name")
                        .tone(BadgeTone::NeutralSoft)
                        .clip(),
                )
                .child(Badge::new("x").no_shrink().on_click(
                    "badge-click",
                    rgba(ERROR, 0.2),
                    |_, _, _| {},
                ))
        }
    }

    #[gpui::test]
    fn badges_draw_without_panic(cx: &mut TestAppContext) {
        let (_view, cx) = cx.add_window_view(|_, _| Fixture);
        cx.run_until_parked();
    }
}
