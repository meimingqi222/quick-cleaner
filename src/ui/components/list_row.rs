//! 行的交互外壳：hover / 选中 / 禁用 / 点击 / 右键 / 双击。
//!
//! 只管外壳。进程行、软件行、磁盘行这类领域行以后用 `ListRow` 组合，
//! 不要把领域字段加进这里。

use crate::ui::components::ClickHandler;
use crate::ui::theme::{rgba, CARD, OUTLINE_VAR, PRIMARY, PRIMARY_FIXED};
use crate::ui::theme::{SURF_HIGH, SURF_HIGHEST, SURF_LOW};
use gpui::{
    div, prelude::*, px, rgb, AnyElement, App, ClickEvent, ElementId, IntoElement, MouseButton,
    MouseDownEvent, RenderOnce, Window,
};
use std::rc::Rc;

/// 悬停底色。对应 views 里三种 `hover(|h| h.bg(rgb(SURF_*)))`。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RowHover {
    #[default]
    Low,
    High,
    Highest,
    None,
}

impl RowHover {
    fn color(self) -> Option<u32> {
        match self {
            RowHover::Low => Some(SURF_LOW),
            RowHover::High => Some(SURF_HIGH),
            RowHover::Highest => Some(SURF_HIGHEST),
            RowHover::None => None,
        }
    }
}

/// 外观骨架。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RowVariant {
    /// 贴边列表行：可选底部分隔线，选中为浅主色底且不再悬停变色。
    #[default]
    Flat,
    /// 带边框的卡片行：选中为 `PRIMARY_FIXED` 浅底 + 主色边框。
    Boxed,
    /// 无边框的菜单项式行：选中为实心 `PRIMARY_FIXED` 底且不再悬停变色，默认 `rounded_lg`。
    Soft,
}

/// 内边距，按现有调用点命名。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RowInset {
    #[default]
    None,
    /// `p_2`
    P2,
    /// `px_2p5`
    X2p5,
    /// `px_3 py_2`
    X3Y2,
    /// `px_4`
    X4,
    /// `px_4 py_2`
    X4Y2,
    /// `px_4 py_2p5`
    X4Y2p5,
    /// `px_5`
    X5,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RowGap {
    None,
    /// `gap_2`
    Tight,
    /// `gap_2p5`
    Snug,
    /// `gap_3`
    #[default]
    Normal,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RowRadius {
    /// `Flat` 取无圆角，`Boxed` / `Soft` 取 `rounded_lg`。
    #[default]
    Auto,
    None,
    Md,
    Lg,
}

type MouseDownFn = Box<dyn Fn(&MouseDownEvent, &mut Window, &mut App) + 'static>;

#[derive(IntoElement)]
pub struct ListRow {
    id: ElementId,
    variant: RowVariant,
    hover: RowHover,
    inset: RowInset,
    gap: RowGap,
    radius: RowRadius,
    height: Option<f32>,
    divider: Option<f32>,
    rest_bg: Option<u32>,
    selected: bool,
    disabled: bool,
    flex_none: bool,
    leading: Option<AnyElement>,
    content: Option<AnyElement>,
    trailing: Option<AnyElement>,
    on_click: Option<ClickHandler>,
    on_double_click: Option<ClickHandler>,
    on_double_press: Option<MouseDownFn>,
    on_right_click: Option<MouseDownFn>,
}

impl ListRow {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            variant: RowVariant::Flat,
            hover: RowHover::Low,
            inset: RowInset::None,
            gap: RowGap::Normal,
            radius: RowRadius::Auto,
            height: None,
            divider: None,
            rest_bg: None,
            selected: false,
            disabled: false,
            flex_none: false,
            leading: None,
            content: None,
            trailing: None,
            on_click: None,
            on_double_click: None,
            on_double_press: None,
            on_right_click: None,
        }
    }

    pub fn variant(mut self, v: RowVariant) -> Self {
        self.variant = v;
        self
    }

    pub fn hover(mut self, h: RowHover) -> Self {
        self.hover = h;
        self
    }

    pub fn inset(mut self, i: RowInset) -> Self {
        self.inset = i;
        self
    }

    pub fn gap(mut self, g: RowGap) -> Self {
        self.gap = g;
        self
    }

    pub fn radius(mut self, r: RowRadius) -> Self {
        self.radius = r;
        self
    }

    /// 固定高度（px）。`uniform_list` 的行必须设。
    pub fn height(mut self, px: f32) -> Self {
        self.height = Some(px);
        self
    }

    /// 底部分隔线，参数是 `OUTLINE_VAR` 的透明度。只对 `Flat` 生效。
    pub fn divider(mut self, alpha: f32) -> Self {
        self.divider = Some(alpha);
        self
    }

    /// 未选中时的底色（如 junk 行的 `CARD`）。只对 `Flat` 生效。
    pub fn rest_bg(mut self, color: u32) -> Self {
        self.rest_bg = Some(color);
        self
    }

    pub fn selected(mut self, v: bool) -> Self {
        self.selected = v;
        self
    }

    /// 禁用后不触发任何回调，无 hover、无指针光标。透明度由调用方自己处理。
    pub fn disabled(mut self, v: bool) -> Self {
        self.disabled = v;
        self
    }

    pub fn flex_none(mut self) -> Self {
        self.flex_none = true;
        self
    }

    pub fn leading(mut self, el: impl IntoElement) -> Self {
        self.leading = Some(el.into_any_element());
        self
    }

    /// 占满剩余宽度，`min_w(0)` 以便内部文字截断。
    pub fn content(mut self, el: impl IntoElement) -> Self {
        self.content = Some(el.into_any_element());
        self
    }

    pub fn trailing(mut self, el: impl IntoElement) -> Self {
        self.trailing = Some(el.into_any_element());
        self
    }

    /// 只在左键 / 键盘点击时触发。gpui 的 `on_click` 连右键也会触发，
    /// 这里过滤掉，右键走 `on_right_click`。
    pub fn on_click(mut self, f: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        self.on_click = Some(Rc::new(f));
        self
    }

    /// 左键第二次点击（`click_count >= 2`）触发，单击回调照常先触发。
    pub fn on_double_click(
        mut self,
        f: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_double_click = Some(Rc::new(f));
        self
    }

    /// 左键第二次按下（`click_count >= 2`）时立即触发，不等抬起。
    /// 与 `on_double_click` 的区别只是时机；两者可同时设，但通常只需要一个。
    pub fn on_double_press(
        mut self,
        f: impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_double_press = Some(Box::new(f));
        self
    }

    /// 右键按下时触发，`event.position` 供弹出右键菜单定位。
    pub fn on_right_click(
        mut self,
        f: impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_right_click = Some(Box::new(f));
        self
    }
}

impl RenderOnce for ListRow {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let Self {
            id,
            variant,
            hover,
            inset,
            gap,
            radius,
            height,
            divider,
            rest_bg,
            selected,
            disabled,
            flex_none,
            leading,
            content,
            trailing,
            on_click,
            on_double_click,
            on_double_press,
            on_right_click,
        } = self;

        let boxed = variant == RowVariant::Boxed;
        let soft = variant == RowVariant::Soft;
        let clickable = !disabled && (on_click.is_some() || on_double_click.is_some());
        let pointer = clickable || (!disabled && on_double_press.is_some());

        let mut row = div()
            .id(id)
            .w_full()
            .flex()
            .items_center()
            .when(flex_none, |d| d.flex_none())
            .when_some(height, |d, h| d.h(px(h)));

        row = match gap {
            RowGap::None => row,
            RowGap::Tight => row.gap_2(),
            RowGap::Snug => row.gap_2p5(),
            RowGap::Normal => row.gap_3(),
        };
        row = match inset {
            RowInset::None => row,
            RowInset::P2 => row.p_2(),
            RowInset::X2p5 => row.px_2p5(),
            RowInset::X3Y2 => row.px_3().py_2(),
            RowInset::X4 => row.px_4(),
            RowInset::X4Y2 => row.px_4().py_2(),
            RowInset::X4Y2p5 => row.px_4().py_2p5(),
            RowInset::X5 => row.px_5(),
        };
        row = match (radius, boxed || soft) {
            (RowRadius::None, _) | (RowRadius::Auto, false) => row,
            (RowRadius::Md, _) => row.rounded_md(),
            (RowRadius::Lg, _) | (RowRadius::Auto, true) => row.rounded_lg(),
        };

        // 选中态不再悬停变色：选中色本身就是反馈，叠 hover 会让底色跳动。
        let hover_color = if disabled || selected {
            None
        } else {
            hover.color()
        };

        if boxed {
            row = row.border_1();
            row = if selected {
                row.bg(rgba(PRIMARY_FIXED, 0.45)).border_color(rgb(PRIMARY))
            } else {
                row.bg(rgb(CARD)).border_color(rgba(OUTLINE_VAR, 0.2))
            };
        } else if soft {
            if selected {
                row = row.bg(rgb(PRIMARY_FIXED));
            }
        } else {
            if let Some(alpha) = divider {
                row = row.border_b_1().border_color(rgba(OUTLINE_VAR, alpha));
            }
            if selected {
                row = row.bg(rgba(PRIMARY, 0.05));
            } else if let Some(bg) = rest_bg {
                row = row.bg(rgb(bg));
            }
        }
        if let Some(color) = hover_color {
            row = row.hover(move |h| h.bg(rgb(color)));
        }

        if disabled {
            row = row.cursor_not_allowed();
        } else if pointer {
            row = row.cursor_pointer();
        }

        if clickable {
            row = row.on_click(move |event, window, cx| {
                if !event.standard_click() {
                    return;
                }
                if let Some(f) = &on_click {
                    f(event, window, cx);
                }
                if event.click_count() >= 2 {
                    if let Some(f) = &on_double_click {
                        f(event, window, cx);
                    }
                }
            });
        }
        if let Some(f) = on_double_press.filter(|_| !disabled) {
            row = row.on_mouse_down(MouseButton::Left, move |event, window, cx| {
                if event.click_count >= 2 {
                    f(event, window, cx);
                }
            });
        }
        if let Some(f) = on_right_click.filter(|_| !disabled) {
            row = row.on_mouse_down(MouseButton::Right, move |event, window, cx| {
                f(event, window, cx)
            });
        }

        row.children(leading)
            .children(content.map(|c| div().flex_1().min_w(px(0.)).child(c)))
            .children(trailing)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{point, Context, Modifiers, MouseUpEvent, Render, TestAppContext};

    #[derive(Default)]
    struct Fixture {
        clicks: usize,
        doubles: usize,
        rights: Vec<f32>,
        presses: usize,
        disabled_hits: usize,
    }

    impl Render for Fixture {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            div()
                .size_full()
                .child(
                    ListRow::new("live")
                        .height(40.)
                        .content("live")
                        .on_click(cx.listener(|this, _, _, _| this.clicks += 1))
                        .on_double_click(cx.listener(|this, _, _, _| this.doubles += 1))
                        .on_double_press(cx.listener(|this, _, _, _| this.presses += 1))
                        .on_right_click(cx.listener(|this, e: &MouseDownEvent, _, _| {
                            this.rights.push(e.position.x.into());
                        })),
                )
                .child(
                    ListRow::new("dead")
                        .height(40.)
                        .disabled(true)
                        .content("dead")
                        .on_click(cx.listener(|this, _, _, _| this.disabled_hits += 1))
                        .on_double_click(cx.listener(|this, _, _, _| this.disabled_hits += 1))
                        .on_double_press(cx.listener(|this, _, _, _| this.disabled_hits += 1))
                        .on_right_click(cx.listener(|this, _, _, _| this.disabled_hits += 1)),
                )
        }
    }

    fn live() -> gpui::Point<gpui::Pixels> {
        point(px(30.), px(20.))
    }

    fn dead() -> gpui::Point<gpui::Pixels> {
        point(px(30.), px(60.))
    }

    #[gpui::test]
    fn click_fires_and_disabled_does_not(cx: &mut TestAppContext) {
        let (view, cx) = cx.add_window_view(|_, _| Fixture::default());
        cx.simulate_click(live(), Modifiers::none());
        cx.simulate_click(dead(), Modifiers::none());
        view.read_with(cx, |f, _| {
            assert_eq!(f.clicks, 1);
            assert_eq!(f.doubles, 0);
            assert_eq!(f.disabled_hits, 0);
        });
    }

    #[gpui::test]
    fn right_click_reports_position_and_skips_on_click(cx: &mut TestAppContext) {
        let (view, cx) = cx.add_window_view(|_, _| Fixture::default());
        cx.simulate_mouse_down(live(), MouseButton::Right, Modifiers::none());
        cx.simulate_mouse_up(live(), MouseButton::Right, Modifiers::none());
        cx.simulate_mouse_down(dead(), MouseButton::Right, Modifiers::none());
        cx.simulate_mouse_up(dead(), MouseButton::Right, Modifiers::none());
        view.read_with(cx, |f, _| {
            assert_eq!(f.rights, vec![30.0]);
            assert_eq!(f.clicks, 0, "右键不能当成单击");
            assert_eq!(f.disabled_hits, 0);
        });
    }

    #[gpui::test]
    fn second_click_fires_double_click(cx: &mut TestAppContext) {
        let (view, cx) = cx.add_window_view(|_, _| Fixture::default());
        cx.simulate_click(live(), Modifiers::none());
        cx.simulate_event(MouseDownEvent {
            position: live(),
            modifiers: Modifiers::none(),
            button: MouseButton::Left,
            click_count: 2,
            first_mouse: false,
        });
        cx.simulate_event(MouseUpEvent {
            position: live(),
            modifiers: Modifiers::none(),
            button: MouseButton::Left,
            click_count: 2,
        });
        view.read_with(cx, |f, _| {
            assert_eq!(f.clicks, 2);
            assert_eq!(f.doubles, 1);
        });
    }

    #[gpui::test]
    fn double_press_fires_on_second_mouse_down_before_release(cx: &mut TestAppContext) {
        let (view, cx) = cx.add_window_view(|_, _| Fixture::default());
        cx.simulate_click(live(), Modifiers::none());
        view.read_with(cx, |f, _| assert_eq!(f.presses, 0, "首次按下不触发"));
        cx.simulate_event(MouseDownEvent {
            position: live(),
            modifiers: Modifiers::none(),
            button: MouseButton::Left,
            click_count: 2,
            first_mouse: false,
        });
        view.read_with(cx, |f, _| assert_eq!(f.presses, 1, "第二次按下立即触发"));
        cx.simulate_event(MouseDownEvent {
            position: dead(),
            modifiers: Modifiers::none(),
            button: MouseButton::Left,
            click_count: 2,
            first_mouse: false,
        });
        view.read_with(cx, |f, _| assert_eq!(f.disabled_hits, 0));
    }
}
