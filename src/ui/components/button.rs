//! 通用按钮 / 图标按钮
//!
//! 取代 `buttons.rs` 里「外层 div 挂 on_click、内层只负责外观」的做法：
//! 外观和点击在同一个元素上，`disabled` / `busy` 时根本不挂 `on_click`，
//! 所以不会再出现「看起来灰了但仍然触发」。
//!
//! 键盘激活不需要自己监听按键：GPUI 的 `Interactivity` 在元素持有焦点时，会把
//! Enter / Space 的 key-up 转成同一批 `on_click` 监听器。因此只要给了
//! `focus_handle` 并且按钮处于可用状态，Enter / Space 与鼠标点击走同一回调，
//! 禁用时没有监听器，两种途径一起失效。
//!
//! 每个变体有默认尺寸（[`ButtonVariant::default_size`]），`.size()` 只在同一配色
//! 需要另一种几何时覆盖。

use crate::ui::components::tooltip::text_tooltip;
use crate::ui::components::ClickHandler;
use crate::ui::theme::*;
use gpui::{
    div, prelude::*, px, rgb, AnyElement, AnyView, App, ClickEvent, ElementId, FocusHandle,
    FontWeight, Hsla, ParentElement, SharedString, Stateful, Window,
};
use std::rc::Rc;

/// 禁用 / 忙碌时的整体透明度，与 `buttons.rs` 一致。
const INACTIVE_OPACITY: f32 = 0.38;

type TooltipBuilder = Rc<dyn Fn(&mut Window, &mut App) -> AnyView>;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ButtonVariant {
    /// 实心主色胶囊，hover 变亮。
    #[default]
    Primary,
    /// 实心错误色，用于不可逆操作。
    Danger,
    /// 描边透明底，hover 浅灰底 + 主色字。
    Ghost,
    /// 小型操作钮；配色由 `.tone()` 选，默认 [`ButtonTone::Neutral`]。
    Small,
    /// 浅灰底描边的矩形次要钮，hover 变浅。
    Tonal,
    /// 行内无边框小钮，hover 变主色容器底。
    Mini,
    /// 无底板的文字链接，默认主色字，hover 浅灰底；字色与悬停用 `.fg()` / `.hover_style()` 调。
    Link,
}

/// `Small` 的配色。封闭集合：新配色先加到这里，不要让页面直接传色值。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ButtonTone {
    /// SURF_HIGH / TEXT
    #[default]
    Neutral,
    /// SURF_LOW / TEXT
    Subtle,
    /// CARD / TEXT，放在灰底区域上
    Surface,
    /// SURF_HIGH / MUTED
    Muted,
    /// SURF_LOW / MUTED，开关的未选中态
    Quiet,
    /// PRIMARY_FIXED / PRIMARY，开关的选中态或推荐操作
    Accent,
    /// SURF_HIGH / ERROR
    Danger,
    /// CARD / ERROR
    DangerSurface,
}

impl ButtonTone {
    /// (底色, 字色)
    pub fn colors(self) -> (u32, u32) {
        match self {
            Self::Neutral => (SURF_HIGH, TEXT),
            Self::Subtle => (SURF_LOW, TEXT),
            Self::Surface => (CARD, TEXT),
            Self::Muted => (SURF_HIGH, MUTED),
            Self::Quiet => (SURF_LOW, MUTED),
            Self::Accent => (PRIMARY_FIXED, PRIMARY),
            Self::Danger => (SURF_HIGH, ERROR),
            Self::DangerSurface => (CARD, ERROR),
        }
    }
}

/// 只管几何（内边距、圆角、字号）；配色与字重由变体决定。
/// 各变体的默认尺寸见 [`ButtonVariant::default_size`]。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ButtonSize {
    /// px_5 / py_2 / 全圆角 / text_sm
    Pill,
    /// px_4 / py_2 / 全圆角 / text_sm
    PillSlim,
    /// px_3 / py 4px / rounded_md / text_xs
    Compact,
    /// px_4 / py_2 / rounded_lg / text_xs
    Rect,
    /// px_2 / py 2px / rounded_md / text_xs
    Mini,
    /// px_2 / py_1 / rounded_md / text_xs
    Slim,
    /// px_5 / py_2 / rounded_lg / text_sm
    Block,
    /// px_6 / py_2 / rounded_lg / text_sm
    BlockWide,
}

impl ButtonVariant {
    pub fn default_size(self) -> ButtonSize {
        match self {
            Self::Primary | Self::Danger => ButtonSize::Pill,
            Self::Ghost => ButtonSize::PillSlim,
            Self::Small => ButtonSize::Compact,
            Self::Tonal => ButtonSize::Rect,
            Self::Mini | Self::Link => ButtonSize::Mini,
        }
    }
}

#[derive(IntoElement)]
pub struct Button {
    id: ElementId,
    variant: ButtonVariant,
    size: Option<ButtonSize>,
    label: Option<SharedString>,
    busy_label: Option<SharedString>,
    tone: ButtonTone,
    fg: Option<u32>,
    weight: Option<FontWeight>,
    hover: Option<(Option<Hsla>, Option<u32>)>,
    inactive_opacity: Option<f32>,
    flex_none: bool,
    tooltip: Option<TooltipBuilder>,
    disabled: bool,
    busy: bool,
    focus_handle: Option<FocusHandle>,
    on_click: Option<ClickHandler>,
    leading: Option<AnyElement>,
    trailing: Option<AnyElement>,
    children: Vec<AnyElement>,
}

impl Button {
    pub fn new(id: impl Into<ElementId>, variant: ButtonVariant) -> Self {
        Self {
            id: id.into(),
            variant,
            size: None,
            label: None,
            busy_label: None,
            tone: ButtonTone::default(),
            fg: None,
            weight: None,
            hover: None,
            inactive_opacity: None,
            flex_none: false,
            tooltip: None,
            disabled: false,
            busy: false,
            focus_handle: None,
            on_click: None,
            leading: None,
            trailing: None,
            children: Vec::new(),
        }
    }

    pub fn primary(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        Self::new(id, ButtonVariant::Primary).label(label)
    }

    pub fn danger(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        Self::new(id, ButtonVariant::Danger).label(label)
    }

    pub fn ghost(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        Self::new(id, ButtonVariant::Ghost).label(label)
    }

    pub fn small(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        Self::new(id, ButtonVariant::Small).label(label)
    }

    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// `busy` 时代替 `label` 显示；未设置则沿用 `label`。
    pub fn busy_label(mut self, label: impl Into<SharedString>) -> Self {
        self.busy_label = Some(label.into());
        self
    }

    pub fn size(mut self, size: ButtonSize) -> Self {
        self.size = Some(size);
        self
    }

    /// 仅 `Small` 使用，其他变体忽略。
    pub fn tone(mut self, tone: ButtonTone) -> Self {
        self.tone = tone;
        self
    }

    /// 覆盖字色，任何变体都生效；不影响 `.hover_style()` 给的悬停字色。
    pub fn fg(mut self, color: u32) -> Self {
        self.fg = Some(color);
        self
    }

    /// 覆盖字重，任何变体都生效。
    pub fn weight(mut self, weight: FontWeight) -> Self {
        self.weight = Some(weight);
        self
    }

    /// 整体替换变体的悬停样式：`bg` / `fg` 为 `None` 表示该项悬停时不变。
    /// 两者都是 `None` 即无悬停反馈，指针手型仍保留。
    pub fn hover_style(mut self, bg: Option<Hsla>, fg: Option<u32>) -> Self {
        self.hover = Some((bg, fg));
        self
    }

    /// 禁用 / 忙碌时的整体透明度，默认 0.38。
    pub fn inactive_opacity(mut self, opacity: f32) -> Self {
        self.inactive_opacity = Some(opacity);
        self
    }

    /// 在 flex 父容器里不被压缩。
    pub fn flex_none(mut self) -> Self {
        self.flex_none = true;
        self
    }

    /// 悬停提示，参数形如 `text_tooltip` / `path_tooltip` 的返回值。
    pub fn tooltip(mut self, build: impl Fn(&mut Window, &mut App) -> AnyView + 'static) -> Self {
        self.tooltip = Some(Rc::new(build));
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn busy(mut self, busy: bool) -> Self {
        self.busy = busy;
        self
    }

    /// 给了焦点句柄后，按钮持有焦点时 Enter / Space 触发同一个 `on_click`。
    pub fn focus_handle(mut self, handle: &FocusHandle) -> Self {
        self.focus_handle = Some(handle.clone());
        self
    }

    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Rc::new(handler));
        self
    }

    pub fn leading(mut self, element: impl IntoElement) -> Self {
        self.leading = Some(element.into_any_element());
        self
    }

    pub fn trailing(mut self, element: impl IntoElement) -> Self {
        self.trailing = Some(element.into_any_element());
        self
    }
}

impl ParentElement for Button {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

fn apply_size(el: Stateful<gpui::Div>, size: ButtonSize) -> Stateful<gpui::Div> {
    match size {
        ButtonSize::Pill => el.px_5().py_2().rounded_full().text_sm(),
        ButtonSize::PillSlim => el.px_4().py_2().rounded_full().text_sm(),
        ButtonSize::Compact => el.px_3().py(px(4.)).rounded_md().text_xs(),
        ButtonSize::Rect => el.px_4().py_2().rounded_lg().text_xs(),
        ButtonSize::Mini => el.px_2().py(px(2.)).rounded_md().text_xs(),
        ButtonSize::Slim => el.px_2().py_1().rounded_md().text_xs(),
        ButtonSize::Block => el.px_5().py_2().rounded_lg().text_sm(),
        ButtonSize::BlockWide => el.px_6().py_2().rounded_lg().text_sm(),
    }
}

fn apply_variant(
    el: Stateful<gpui::Div>,
    variant: ButtonVariant,
    tone: ButtonTone,
    active: bool,
) -> Stateful<gpui::Div> {
    match variant {
        ButtonVariant::Primary => el
            .bg(rgb(PRIMARY))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(rgb(ON_PRIMARY))
            .when(active, |d| d.hover(|h| h.bg(rgb(PRIMARY_BRIGHT)))),
        ButtonVariant::Danger => el
            .bg(rgb(ERROR))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(rgb(ON_PRIMARY))
            .when(active, |d| d.hover(|h| h.bg(rgba(ERROR, 0.85)))),
        ButtonVariant::Ghost => el
            .border_1()
            .border_color(rgb(OUTLINE_VAR))
            .text_color(rgb(MUTED))
            .when(active, |d| {
                d.hover(|h| h.bg(rgb(SURF_LOW)).text_color(rgb(PRIMARY)))
            }),
        ButtonVariant::Small => {
            let (bg, fg) = tone.colors();
            el.bg(rgb(bg))
                .border_1()
                .border_color(rgba(OUTLINE_VAR, 0.6))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(rgb(fg))
                .when(active, |d| {
                    d.hover(|h| h.border_color(rgba(PRIMARY, 0.8)).opacity(0.9))
                })
        }
        ButtonVariant::Tonal => el
            .bg(rgb(SURF_HIGH))
            .border_1()
            .border_color(rgba(OUTLINE_VAR, 0.4))
            .font_weight(FontWeight::MEDIUM)
            .text_color(rgb(TEXT))
            .when(active, |d| d.hover(|h| h.bg(rgb(SURF_LOW)))),
        ButtonVariant::Mini => el
            .bg(rgb(SURF_HIGH))
            .font_weight(FontWeight::MEDIUM)
            .text_color(rgb(MUTED))
            .when(active, |d| {
                d.hover(|h| h.bg(rgb(PRIMARY_FIXED)).text_color(rgb(PRIMARY)))
            }),
        ButtonVariant::Link => el
            .text_color(rgb(PRIMARY))
            .when(active, |d| d.hover(|h| h.bg(rgb(SURF_HIGH)))),
    }
}

impl RenderOnce for Button {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let active = !self.disabled && !self.busy;
        let size = self.size.unwrap_or_else(|| self.variant.default_size());
        let label = if self.busy {
            self.busy_label.or(self.label)
        } else {
            self.label
        };
        let has_side = self.leading.is_some() || self.trailing.is_some();

        let base = div()
            .id(self.id)
            .flex()
            .items_center()
            .justify_center()
            .when(has_side, |d| d.gap_2());
        let hover_override = self.hover;
        let el = apply_variant(
            apply_size(base, size),
            self.variant,
            self.tone,
            active && hover_override.is_none(),
        )
        .when_some(self.fg, |d, c| d.text_color(rgb(c)))
        .when_some(self.weight, |d, w| d.font_weight(w))
        .when(self.flex_none, |d| d.flex_none());
        let el = match hover_override {
            Some((bg, fg)) if active && (bg.is_some() || fg.is_some()) => el.hover(move |h| {
                let h = match bg {
                    Some(c) => h.bg(c),
                    None => h,
                };
                match fg {
                    Some(c) => h.text_color(rgb(c)),
                    None => h,
                }
            }),
            _ => el,
        };

        // 禁用 / 忙碌时不挂 on_click：鼠标与键盘激活走同一批监听器，一并失效。
        let el = match (active, self.on_click) {
            (true, Some(handler)) => el
                .cursor_pointer()
                .on_click(move |event, window, cx| handler(event, window, cx)),
            (true, None) => el.cursor_pointer(),
            (false, _) => el.opacity(self.inactive_opacity.unwrap_or(INACTIVE_OPACITY)),
        };

        el.when_some(self.focus_handle, |d, fh| d.track_focus(&fh))
            .when_some(self.tooltip, |d, build| {
                d.tooltip(move |window, cx| build(window, cx))
            })
            .children(self.leading)
            .children(label)
            .children(self.children)
            .children(self.trailing)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum IconButtonVariant {
    /// 无底板的小钮，字色默认 `OUTLINE`，hover 变 `ERROR`；内边距随内容。
    #[default]
    Clear,
    /// 28x28 圆角方块，`SURF_HIGH` 底，hover 加深。
    Tile,
}

#[derive(IntoElement)]
pub struct IconButton {
    id: ElementId,
    variant: IconButtonVariant,
    tooltip: Option<SharedString>,
    disabled: bool,
    focus_handle: Option<FocusHandle>,
    on_click: Option<ClickHandler>,
    children: Vec<AnyElement>,
}

impl IconButton {
    pub fn new(id: impl Into<ElementId>, variant: IconButtonVariant) -> Self {
        Self {
            id: id.into(),
            variant,
            tooltip: None,
            disabled: false,
            focus_handle: None,
            on_click: None,
            children: Vec::new(),
        }
    }

    pub fn tooltip(mut self, text: impl Into<SharedString>) -> Self {
        self.tooltip = Some(text.into());
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn focus_handle(mut self, handle: &FocusHandle) -> Self {
        self.focus_handle = Some(handle.clone());
        self
    }

    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Rc::new(handler));
        self
    }
}

impl ParentElement for IconButton {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl RenderOnce for IconButton {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let active = !self.disabled;
        let el = div().id(self.id);
        let el = match self.variant {
            IconButtonVariant::Clear => el
                .px_1()
                .rounded_full()
                .text_xs()
                .text_color(rgb(OUTLINE))
                .when(active, |d| d.hover(|h| h.text_color(rgb(ERROR)))),
            IconButtonVariant::Tile => el
                .w(px(28.))
                .h(px(28.))
                .rounded_md()
                .bg(rgb(SURF_HIGH))
                .flex()
                .items_center()
                .justify_center()
                .text_xs()
                .font_weight(FontWeight::BOLD)
                .text_color(rgb(MUTED))
                .when(active, |d| d.hover(|h| h.bg(rgb(SURF_HIGHEST)))),
        };
        let el = match (active, self.on_click) {
            (true, Some(handler)) => el
                .cursor_pointer()
                .on_click(move |event, window, cx| handler(event, window, cx)),
            (true, None) => el.cursor_pointer(),
            (false, _) => el.opacity(INACTIVE_OPACITY),
        };
        el.when_some(self.focus_handle, |d, fh| d.track_focus(&fh))
            .when_some(self.tooltip, |d, text| d.tooltip(text_tooltip(text)))
            .children(self.children)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{point, Context, Modifiers, Render, TestAppContext};

    struct Fixture {
        clicks: usize,
        disabled: bool,
        busy: bool,
        with_focus: bool,
        focus: FocusHandle,
    }

    impl Fixture {
        fn new(_window: &mut Window, cx: &mut Context<Self>) -> Self {
            Self {
                clicks: 0,
                disabled: false,
                busy: false,
                with_focus: false,
                focus: cx.focus_handle(),
            }
        }
    }

    impl Render for Fixture {
        fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let mut button = Button::primary("fixture-button", "Fixture button label")
                .busy_label("Working")
                .disabled(self.disabled)
                .busy(self.busy)
                .on_click(cx.listener(|this, _, _, _| this.clicks += 1));
            if self.with_focus {
                button = button.focus_handle(&self.focus);
            }
            div().size_full().child(button)
        }
    }

    const INSIDE: (f32, f32) = (30., 14.);

    fn click(cx: &mut gpui::VisualTestContext) {
        cx.simulate_click(point(px(INSIDE.0), px(INSIDE.1)), Modifiers::none());
    }

    fn focus(view: &gpui::Entity<Fixture>, cx: &mut gpui::VisualTestContext) {
        let fh = view.read_with(cx, |this, _| this.focus.clone());
        cx.update(|window, cx| {
            fh.focus(window);
            let _ = cx;
        });
        // is_focused 在绘制阶段求值，焦点变化后要先过一帧。
        cx.run_until_parked();
    }

    /// GPUI 在 KeyUp 上把 Enter/Space 转成点击，而 `simulate_keystrokes` 只发 KeyDown。
    fn press(cx: &mut gpui::VisualTestContext, key: &str) {
        cx.simulate_keystrokes(key);
        cx.simulate_event(gpui::KeyUpEvent {
            keystroke: gpui::Keystroke::parse(key).unwrap(),
        });
    }

    fn clicks(view: &gpui::Entity<Fixture>, cx: &mut gpui::VisualTestContext) -> usize {
        view.read_with(cx, |this, _| this.clicks)
    }

    struct LinkFixture {
        clicks: usize,
        disabled: bool,
    }

    impl Render for LinkFixture {
        fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            div().size_full().child(
                Button::new("link-fixture", ButtonVariant::Link)
                    .label("Link fixture label")
                    .fg(OUTLINE)
                    .weight(FontWeight::BOLD)
                    .hover_style(Some(rgba(ERROR_CONTAINER, 1.)), Some(ERROR))
                    .size(ButtonSize::Slim)
                    .flex_none()
                    .disabled(self.disabled)
                    .on_click(cx.listener(|this, _, _, _| this.clicks += 1)),
            )
        }
    }

    #[gpui::test]
    fn link_with_style_overrides_clicks_unless_disabled(cx: &mut TestAppContext) {
        let (view, cx) = cx.add_window_view(|_, _| LinkFixture {
            clicks: 0,
            disabled: false,
        });
        cx.simulate_click(point(px(30.), px(10.)), Modifiers::none());
        view.read_with(cx, |t, _| assert_eq!(t.clicks, 1));
        view.update(cx, |t, cx| {
            t.disabled = true;
            cx.notify();
        });
        cx.run_until_parked();
        cx.simulate_click(point(px(30.), px(10.)), Modifiers::none());
        view.read_with(cx, |t, _| assert_eq!(t.clicks, 1, "禁用后不再触发"));
    }

    #[test]
    fn new_sizes_are_distinct_from_existing() {
        let all = [
            ButtonSize::Pill,
            ButtonSize::PillSlim,
            ButtonSize::Compact,
            ButtonSize::Rect,
            ButtonSize::Mini,
            ButtonSize::Slim,
            ButtonSize::Block,
            ButtonSize::BlockWide,
        ];
        for (i, a) in all.iter().enumerate() {
            for b in &all[i + 1..] {
                assert_ne!(a, b);
            }
        }
        assert_eq!(ButtonVariant::Link.default_size(), ButtonSize::Mini);
    }

    #[gpui::test]
    fn click_fires_callback(cx: &mut TestAppContext) {
        let (view, cx) = cx.add_window_view(Fixture::new);
        click(cx);
        assert_eq!(clicks(&view, cx), 1);
    }

    #[gpui::test]
    fn disabled_ignores_mouse_and_keyboard(cx: &mut TestAppContext) {
        let (view, cx) = cx.add_window_view(Fixture::new);
        view.update(cx, |this, cx| {
            cx.notify();
            this.disabled = true;
            this.with_focus = true;
        });
        cx.run_until_parked();
        focus(&view, cx);
        click(cx);
        press(cx, "enter");
        press(cx, "space");
        assert_eq!(clicks(&view, cx), 0);

        view.update(cx, |this, cx| {
            this.disabled = false;
            cx.notify();
        });
        click(cx);
        assert_eq!(clicks(&view, cx), 1, "same hit point works once enabled");
        press(cx, "enter");
        assert_eq!(clicks(&view, cx), 2, "focus was effective all along");
    }

    #[gpui::test]
    fn busy_ignores_mouse_and_keyboard(cx: &mut TestAppContext) {
        let (view, cx) = cx.add_window_view(Fixture::new);
        view.update(cx, |this, cx| {
            cx.notify();
            this.busy = true;
            this.with_focus = true;
        });
        cx.run_until_parked();
        focus(&view, cx);
        click(cx);
        press(cx, "enter");
        press(cx, "space");
        assert_eq!(clicks(&view, cx), 0);
    }

    #[gpui::test]
    fn focused_button_activates_on_enter_and_space(cx: &mut TestAppContext) {
        let (view, cx) = cx.add_window_view(Fixture::new);
        view.update(cx, |this, cx| {
            this.with_focus = true;
            cx.notify();
        });
        cx.run_until_parked();
        focus(&view, cx);
        press(cx, "enter");
        assert_eq!(clicks(&view, cx), 1);
        press(cx, "space");
        assert_eq!(clicks(&view, cx), 2);
    }

    #[gpui::test]
    fn unfocused_button_ignores_keyboard(cx: &mut TestAppContext) {
        let (view, cx) = cx.add_window_view(Fixture::new);
        view.update(cx, |this, cx| {
            this.with_focus = true;
            cx.notify();
        });
        cx.run_until_parked();
        press(cx, "enter");
        assert_eq!(clicks(&view, cx), 0);
    }
}
