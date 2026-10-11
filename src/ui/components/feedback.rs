//! 空状态与加载占位
//!
//! 文案、图标、动画相位都由调用方传入；组件不起定时器，`phase` 通常是
//! `Root::anim_phase`，由外层的全局动画驱动重绘。

use crate::ui::theme::*;
use gpui::{
    div, prelude::*, px, rgb, AnyElement, App, FontWeight, IntoElement, RenderOnce, SharedString,
    Window,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EmptyStateStyle {
    /// 带边框的整卡占位（p_12、rounded_2xl、CARD 底）
    Card,
    /// 无边框的居中大标题（p_12，标题 text_base BOLD MUTED）
    Plain,
    /// 列表里的一行小字提示（p_6，text_xs OUTLINE）
    Inline,
}

#[derive(IntoElement)]
pub struct EmptyState {
    title: SharedString,
    description: Option<SharedString>,
    icon: Option<AnyElement>,
    action: Option<AnyElement>,
    style: EmptyStateStyle,
    fill: bool,
    flush: bool,
}

impl EmptyState {
    pub fn new(title: impl Into<SharedString>) -> Self {
        Self {
            title: title.into(),
            description: None,
            icon: None,
            action: None,
            style: EmptyStateStyle::Card,
            fill: false,
            flush: false,
        }
    }

    pub fn style(mut self, style: EmptyStateStyle) -> Self {
        self.style = style;
        self
    }

    pub fn description(mut self, text: impl Into<SharedString>) -> Self {
        self.description = Some(text.into());
        self
    }

    /// 图标插槽。`Inline` 不显示图标。
    pub fn icon(mut self, el: impl IntoElement) -> Self {
        self.icon = Some(el.into_any_element());
        self
    }

    /// 字形图标（emoji 等），按 text_3xl 渲染。
    pub fn glyph(self, text: impl Into<SharedString>) -> Self {
        self.icon(div().text_3xl().child(text.into()))
    }

    /// 说明下方的操作插槽（按钮等）。`Inline` 不显示。
    pub fn action(mut self, el: impl IntoElement) -> Self {
        self.action = Some(el.into_any_element());
        self
    }

    /// 撑满父容器剩余空间（flex_1）。
    pub fn fill(mut self) -> Self {
        self.fill = true;
        self
    }

    /// `Inline` 去掉 `p_6`，由父容器负责留白。其他样式忽略。
    pub fn flush(mut self) -> Self {
        self.flush = true;
        self
    }
}

impl RenderOnce for EmptyState {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let root = div().flex().items_center().justify_center();
        let root = if self.fill { root.flex_1() } else { root };

        match self.style {
            EmptyStateStyle::Inline => root
                .when(!self.flush, |d| d.p_6())
                .text_xs()
                .text_color(rgb(OUTLINE))
                .child(self.title),
            style => {
                let root = root.flex_col();
                let root = match style {
                    EmptyStateStyle::Card => root
                        .p_12()
                        .gap_3()
                        .rounded_2xl()
                        .bg(rgb(CARD))
                        .border_1()
                        .border_color(rgb(SURF_HIGH)),
                    _ => root.p_12().gap_2(),
                };
                let title = match style {
                    EmptyStateStyle::Card => div()
                        .text_sm()
                        .font_weight(FontWeight::BOLD)
                        .text_color(rgb(TEXT)),
                    _ => div()
                        .text_base()
                        .font_weight(FontWeight::BOLD)
                        .text_color(rgb(MUTED)),
                };
                root.children(self.icon)
                    .child(title.child(self.title))
                    .children(
                        self.description
                            .map(|d| div().text_xs().text_color(rgb(MUTED)).child(d)),
                    )
                    .children(self.action)
            }
        }
    }
}

/// 八瓣轨道旋转指示器；相位由调用方的动画帧驱动，组件不自起定时器。
#[derive(IntoElement)]
pub struct Spinner {
    phase: usize,
    size: f32,
    color: u32,
}

impl Spinner {
    pub fn new(phase: usize) -> Self {
        Self {
            phase,
            size: 24.,
            color: PRIMARY,
        }
    }

    pub fn size(mut self, size: f32) -> Self {
        self.size = size;
        self
    }

    pub fn color(mut self, color: u32) -> Self {
        self.color = color;
        self
    }
}

/// 第 `i` 瓣在当前相位的 (不透明度, 圆点直径)。
fn orbit_dot(phase: usize, i: usize, count: usize) -> (f32, f32) {
    let shift = (phase + (count - i)) % count;
    let opacity = match shift {
        0 => 1.0,
        1 => 0.85,
        2 => 0.65,
        3 => 0.45,
        4 => 0.30,
        5 => 0.20,
        6 => 0.12,
        _ => 0.06,
    };
    let dot = if shift <= 1 {
        8.0
    } else if shift <= 3 {
        7.0
    } else {
        6.0
    };
    (opacity, dot)
}

impl RenderOnce for Spinner {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let count = 8;
        let radius = (self.size - 12.) / 2.0;
        let center = self.size / 2.0;
        let mut container = div()
            .w(px(self.size))
            .h(px(self.size))
            .flex_none()
            .relative();

        for i in 0..count {
            let angle = (i as f32) * (std::f32::consts::PI * 2.0 / count as f32);
            let x = center + radius * angle.cos() - 4.0;
            let y = center + radius * angle.sin() - 4.0;
            let (opacity, dot) = orbit_dot(self.phase, i, count);
            container = container.child(
                div()
                    .absolute()
                    .left(px(x))
                    .top(px(y))
                    .w(px(dot))
                    .h(px(dot))
                    .rounded_full()
                    .bg(rgba(self.color, opacity)),
            );
        }

        // 中心呼吸光晕
        let pulse = 0.12 + 0.10 * (((self.phase % 12) as f32 / 12.0) * std::f32::consts::PI).sin();
        container.child(
            div()
                .absolute()
                .left(px(center - 14.0))
                .top(px(center - 14.0))
                .w(px(28.0))
                .h(px(28.0))
                .rounded_full()
                .bg(rgba(self.color, pulse)),
        )
    }
}

/// 标题后循环出现的省略点；定宽补空格，避免标题随相位抖动。
fn loading_dots(phase: usize) -> &'static str {
    match (phase / 4) % 4 {
        1 => ".  ",
        2 => ".. ",
        3 => "...",
        _ => "   ",
    }
}

/// 整块加载占位：撑满父容器并居中。
#[derive(IntoElement)]
pub struct LoadingState {
    title: SharedString,
    description: Option<SharedString>,
    phase: usize,
}

impl LoadingState {
    pub fn new(title: impl Into<SharedString>, phase: usize) -> Self {
        Self {
            title: title.into(),
            description: None,
            phase,
        }
    }

    pub fn description(mut self, text: impl Into<SharedString>) -> Self {
        self.description = Some(text.into());
        self
    }
}

impl RenderOnce for LoadingState {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        div()
            .flex_1()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_4()
            .p_12()
            .child(Spinner::new(self.phase).size(64.0))
            .child(
                div()
                    .text_lg()
                    .font_weight(FontWeight::BOLD)
                    .text_color(rgb(PRIMARY))
                    .child(format!("{}{}", self.title, loading_dots(self.phase))),
            )
            .children(self.description.map(|d| {
                div()
                    .text_xs()
                    .font_weight(FontWeight::NORMAL)
                    .text_color(rgb(MUTED))
                    .child(d)
            }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{Context, Render, TestAppContext};

    #[test]
    fn dots_cycle_every_four_phases() {
        assert_eq!(loading_dots(0), "   ");
        assert_eq!(loading_dots(4), ".  ");
        assert_eq!(loading_dots(8), ".. ");
        assert_eq!(loading_dots(12), "...");
        assert_eq!(loading_dots(16), "   ");
    }

    #[test]
    fn orbit_head_is_brightest_and_tail_fades() {
        // phase 0 时第 0 瓣 shift = 0
        assert_eq!(orbit_dot(0, 0, 8), (1.0, 8.0));
        assert_eq!(orbit_dot(0, 1, 8), (0.06, 6.0));
        assert_eq!(orbit_dot(1, 1, 8), (1.0, 8.0));
    }

    struct Fixture;

    impl Render for Fixture {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
                .size_full()
                .flex()
                .flex_col()
                .child(
                    EmptyState::new("title")
                        .description("desc")
                        .glyph("x")
                        .action(div().child("go")),
                )
                .child(
                    EmptyState::new("plain")
                        .style(EmptyStateStyle::Plain)
                        .fill(),
                )
                .child(EmptyState::new("inline").style(EmptyStateStyle::Inline))
                .child(
                    EmptyState::new("flush")
                        .style(EmptyStateStyle::Inline)
                        .flush()
                        .fill(),
                )
                .child(LoadingState::new("loading", 5).description("sub"))
        }
    }

    #[gpui::test]
    fn feedback_draws_without_panic(cx: &mut TestAppContext) {
        let (_view, cx) = cx.add_window_view(|_, _| Fixture);
        cx.run_until_parked();
    }
}
