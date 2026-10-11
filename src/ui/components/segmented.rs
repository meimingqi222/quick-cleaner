//! 互斥选择控件：分段、页签、胶囊过滤共用。
//!
//! 外观以 [`SegmentStyle`] 区分；只收录至少两个调用点共用的外观。
//! 每个变体注释里的「文件:行号」是迁移前的原实现位置，行号以迁移前为准。

use crate::ui::components::ClickHandler;
use crate::ui::theme::{
    rgba, CARD, MUTED, ON_PRIMARY, OUTLINE_VAR, PRIMARY, PRIMARY_FIXED, SURF_HIGH, SURF_LOW, TEXT,
};
use gpui::{
    div, prelude::*, px, rgb, AnyElement, App, ClickEvent, Div, ElementId, FocusHandle, FontWeight,
    KeyDownEvent, SharedString, Stateful, Window,
};
use std::rc::Rc;

/// 视觉外观。每个变体对应一种与其他变体在像素上不同的原实现。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SegmentStyle {
    /// 浅灰底槽（带描边）内，选中项白色浮起。
    Raised,
    /// 等宽并排按钮，选中项主色实心；文案超长时截断。
    Solid,
    /// 描边胶囊，选中项浅主色底 + 主色描边；几何与配色可用 [`ChipLook`] 调整。
    Chip,
    /// 与 `Raised` 同构，但底槽无描边、项更窄、未选中项字重不加粗且悬停只变底色。
    Plain,
}

/// 胶囊水平内边距，按 Tailwind 步长命名。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChipPadX {
    /// `px_2`
    X2,
    /// `px_2p5`
    X2p5,
    /// `px_3`
    X3,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChipRadius {
    Md,
    Full,
}

/// 胶囊的可调维度。选中态固定为 `PRIMARY_FIXED` 底 + 主色描边 + 主色字，不在此处调。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ChipLook {
    pub pad_x: ChipPadX,
    /// 垂直内边距（px）。
    pub pad_y: f32,
    pub radius: ChipRadius,
    /// 未选中时的底色。
    pub rest_bg: u32,
    /// 未选中描边的 `OUTLINE_VAR` 透明度；`None` 表示未选中时不画描边（选中时仍有）。
    pub rest_border: Option<f32>,
    pub weight_sel: FontWeight,
    pub weight_rest: FontWeight,
    /// 未选中项悬停底色；`None` 表示不变。
    pub hover_bg: Option<u32>,
    /// 未选中项悬停字色；`None` 表示不变。
    pub hover_fg: Option<u32>,
}

impl ChipLook {
    /// `SegmentStyle::Chip` 的默认外观。
    pub const CHIP: ChipLook = ChipLook {
        pad_x: ChipPadX::X3,
        pad_y: 4.,
        radius: ChipRadius::Full,
        rest_bg: CARD,
        rest_border: Some(0.8),
        weight_sel: FontWeight::SEMIBOLD,
        weight_rest: FontWeight::NORMAL,
        hover_bg: Some(SURF_LOW),
        hover_fg: None,
    };
}

impl Default for ChipLook {
    fn default() -> Self {
        Self::CHIP
    }
}

impl SegmentStyle {
    fn default_gap(self) -> f32 {
        match self {
            SegmentStyle::Raised | SegmentStyle::Plain => 4.,
            SegmentStyle::Solid => 8.,
            SegmentStyle::Chip => 8.,
        }
    }
}

type IconFn = Rc<dyn Fn(bool) -> AnyElement>;

pub struct SegmentItem<T> {
    value: T,
    label: SharedString,
    icon: Option<IconFn>,
    badge: Option<SharedString>,
    disabled: bool,
}

impl<T> SegmentItem<T> {
    pub fn new(value: T, label: impl Into<SharedString>) -> Self {
        Self {
            value,
            label: label.into(),
            icon: None,
            badge: None,
            disabled: false,
        }
    }

    /// 参数是该项是否选中，调用方据此挑图标颜色。
    pub fn icon(mut self, icon: impl Fn(bool) -> AnyElement + 'static) -> Self {
        self.icon = Some(Rc::new(icon));
        self
    }

    /// 标签后的计数小胶囊（如进程数）。
    pub fn badge(mut self, badge: impl Into<SharedString>) -> Self {
        self.badge = Some(badge.into());
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
}

type SelectFn<T> = Rc<dyn Fn(&T, &mut Window, &mut App)>;

#[derive(IntoElement)]
pub struct SegmentedControl<T: Clone + PartialEq + 'static> {
    id: ElementId,
    items: Vec<SegmentItem<T>>,
    selected: T,
    on_select: SelectFn<T>,
    style: SegmentStyle,
    reselect: bool,
    focus: Option<FocusHandle>,
    gap: Option<f32>,
    wrap: bool,
    flex_none: bool,
    chip_look: ChipLook,
}

impl<T: Clone + PartialEq + 'static> SegmentedControl<T> {
    pub fn new(
        id: impl Into<ElementId>,
        selected: T,
        on_select: impl Fn(&T, &mut Window, &mut App) + 'static,
    ) -> Self {
        Self {
            id: id.into(),
            items: Vec::new(),
            selected,
            on_select: Rc::new(on_select),
            style: SegmentStyle::Raised,
            reselect: true,
            focus: None,
            gap: None,
            wrap: false,
            flex_none: false,
            chip_look: ChipLook::CHIP,
        }
    }

    pub fn item(mut self, item: SegmentItem<T>) -> Self {
        self.items.push(item);
        self
    }

    pub fn items(mut self, items: impl IntoIterator<Item = SegmentItem<T>>) -> Self {
        self.items.extend(items);
        self
    }

    pub fn style(mut self, style: SegmentStyle) -> Self {
        self.style = style;
        self
    }

    /// 再次点击已选中项是否仍触发 `on_select`，默认触发。
    /// 回调不幂等、需要「再点一次重新生效」的场景（如风扇档位重新夺回控制权）不能关掉它。
    pub fn reselect(mut self, reselect: bool) -> Self {
        self.reselect = reselect;
        self
    }

    /// 传入后控件接管该焦点，聚焦时左右键在可用项间移动，首尾不循环。
    pub fn focus_handle(mut self, focus: FocusHandle) -> Self {
        self.focus = Some(focus);
        self
    }

    /// 覆盖项间距（px）；默认值随样式而定。
    pub fn gap(mut self, gap: f32) -> Self {
        self.gap = Some(gap);
        self
    }

    /// 空间不足时换行。
    pub fn wrap(mut self, wrap: bool) -> Self {
        self.wrap = wrap;
        self
    }

    /// 在 flex 父容器里不被压缩。
    pub fn flex_none(mut self) -> Self {
        self.flex_none = true;
        self
    }

    /// 只对 `SegmentStyle::Chip` 生效。
    pub fn chip_look(mut self, look: ChipLook) -> Self {
        self.chip_look = look;
        self
    }
}

/// 从 `current` 沿 `dir`（-1 / 1）找下一个可用项；没有当前项时从对应端点开始。
/// 到头返回 `None`，不循环。
fn step(enabled: &[bool], current: Option<usize>, dir: i32) -> Option<usize> {
    let mut i = match (current, dir) {
        (Some(c), d) => c as i32 + d,
        (None, d) if d > 0 => 0,
        (None, _) => enabled.len() as i32 - 1,
    };
    while i >= 0 && (i as usize) < enabled.len() {
        if enabled[i as usize] {
            return Some(i as usize);
        }
        i += dir;
    }
    None
}

impl<T: Clone + PartialEq + 'static> RenderOnce for SegmentedControl<T> {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let Self {
            id,
            items,
            selected,
            on_select,
            style,
            reselect,
            focus,
            gap,
            wrap,
            flex_none,
            chip_look,
        } = self;

        let nav: Vec<(T, bool)> = items
            .iter()
            .map(|it| (it.value.clone(), !it.disabled))
            .collect();
        let gap = px(gap.unwrap_or_else(|| style.default_gap()));
        let id_prefix = id.to_string();

        let mut container = div()
            .id(id)
            .flex()
            .items_center()
            .gap(gap)
            .when(wrap, |d| d.flex_wrap())
            .when(flex_none, |d| d.flex_none());
        container = match style {
            SegmentStyle::Raised => container
                .p(px(2.))
                .rounded_lg()
                .bg(rgb(SURF_LOW))
                .border_1()
                .border_color(rgba(OUTLINE_VAR, 0.4)),
            SegmentStyle::Plain => container.p(px(2.)).rounded_lg().bg(rgb(SURF_LOW)),
            SegmentStyle::Solid => container.w_full(),
            SegmentStyle::Chip => container,
        };

        if let Some(focus) = focus {
            let on_select = on_select.clone();
            let selected = selected.clone();
            container = container.track_focus(&focus).on_key_down(
                move |event: &KeyDownEvent, window, cx| {
                    let dir = match event.keystroke.key.as_str() {
                        "left" => -1,
                        "right" => 1,
                        _ => return,
                    };
                    let enabled: Vec<bool> = nav.iter().map(|(_, e)| *e).collect();
                    let current = nav.iter().position(|(v, _)| *v == selected);
                    if let Some(next) = step(&enabled, current, dir) {
                        on_select(&nav[next].0, window, cx);
                    }
                    cx.stop_propagation();
                },
            );
        }

        container.children(items.into_iter().enumerate().map(|(idx, item)| {
            let is_sel = item.value == selected;
            let disabled = item.disabled;
            let base = div()
                .id(SharedString::from(format!("{id_prefix}-{idx}")))
                .text_xs()
                .when(!disabled, |d| d.cursor_pointer());
            let has_extra = item.icon.is_some() || item.badge.is_some();
            let base = match style {
                SegmentStyle::Raised => raised_item(base, is_sel, disabled, has_extra),
                SegmentStyle::Solid => solid_item(base, is_sel, disabled, has_extra),
                SegmentStyle::Chip => chip_item(base, is_sel, disabled, has_extra, &chip_look),
                SegmentStyle::Plain => plain_item(base, is_sel, disabled, has_extra),
            };
            let badge = item.badge.map(|text| {
                div()
                    .px_1p5()
                    .py(px(0.5))
                    .rounded_full()
                    .text_xs()
                    .font_weight(FontWeight::BOLD)
                    .when(is_sel, |p| {
                        p.bg(rgb(PRIMARY_FIXED)).text_color(rgb(PRIMARY))
                    })
                    .when(!is_sel, |p| p.bg(rgb(SURF_HIGH)).text_color(rgb(MUTED)))
                    .child(text)
            });
            let icon = item.icon.map(|f| f(is_sel));
            let clickable = !disabled && (reselect || !is_sel);
            let on_select = on_select.clone();
            let value = item.value;
            base.children(icon)
                .child(item.label)
                .children(badge)
                .when(clickable, move |d| {
                    d.on_click(move |_: &ClickEvent, window, cx| on_select(&value, window, cx))
                })
        }))
    }
}

fn raised_item(d: Stateful<Div>, sel: bool, disabled: bool, extra: bool) -> Stateful<Div> {
    d.px_2p5()
        .py(px(3.))
        .rounded_md()
        .font_weight(if sel {
            FontWeight::BOLD
        } else {
            FontWeight::MEDIUM
        })
        .when(extra, |d| d.flex().items_center().gap_1p5())
        .when(sel, |d| {
            d.bg(rgb(CARD)).text_color(rgb(PRIMARY)).shadow_sm()
        })
        .when(!sel && !disabled, |d| {
            d.text_color(rgb(MUTED))
                .hover(|h| h.bg(rgb(SURF_HIGH)).text_color(rgb(TEXT)))
        })
        .when(!sel && disabled, |d| d.text_color(rgba(MUTED, 0.5)))
}

fn solid_item(d: Stateful<Div>, sel: bool, disabled: bool, extra: bool) -> Stateful<Div> {
    d.flex_1()
        .min_w(px(0.))
        .truncate()
        .py(px(6.))
        .rounded_lg()
        .font_weight(FontWeight::SEMIBOLD)
        .text_center()
        .when(extra, |d| {
            d.flex().items_center().justify_center().gap_1p5()
        })
        .when(sel, |d| d.bg(rgb(PRIMARY)).text_color(rgb(ON_PRIMARY)))
        .when(!sel && !disabled, |d| {
            d.bg(rgb(SURF_LOW))
                .text_color(rgb(MUTED))
                .hover(|h| h.bg(rgb(SURF_HIGH)))
        })
        .when(!sel && disabled, |d| {
            d.bg(rgb(SURF_LOW)).text_color(rgba(MUTED, 0.5))
        })
}

fn plain_item(d: Stateful<Div>, sel: bool, disabled: bool, extra: bool) -> Stateful<Div> {
    d.px_2()
        .py(px(3.))
        .rounded_md()
        .font_weight(if sel {
            FontWeight::BOLD
        } else {
            FontWeight::NORMAL
        })
        .when(extra, |d| d.flex().items_center().gap_1p5())
        .when(sel, |d| {
            d.bg(rgb(CARD)).text_color(rgb(PRIMARY)).shadow_sm()
        })
        .when(!sel && !disabled, |d| {
            d.text_color(rgb(MUTED)).hover(|h| h.bg(rgb(SURF_HIGH)))
        })
        .when(!sel && disabled, |d| d.text_color(rgba(MUTED, 0.5)))
}

fn chip_item(
    d: Stateful<Div>,
    sel: bool,
    disabled: bool,
    extra: bool,
    look: &ChipLook,
) -> Stateful<Div> {
    apply_chip(d, sel, disabled, look).when(extra, |d| d.flex().items_center().gap_1p5())
}

fn apply_chip(d: Stateful<Div>, sel: bool, disabled: bool, look: &ChipLook) -> Stateful<Div> {
    let ChipLook {
        pad_x,
        pad_y,
        radius,
        rest_bg,
        rest_border,
        weight_sel,
        weight_rest,
        hover_bg,
        hover_fg,
    } = *look;
    let d = match pad_x {
        ChipPadX::X2 => d.px_2(),
        ChipPadX::X2p5 => d.px_2p5(),
        ChipPadX::X3 => d.px_3(),
    }
    .py(px(pad_y))
    .when(radius == ChipRadius::Md, |d| d.rounded_md())
    .when(radius == ChipRadius::Full, |d| d.rounded_full())
    .when(sel || rest_border.is_some(), |d| d.border_1())
    .font_weight(if sel { weight_sel } else { weight_rest });
    if sel {
        return d
            .bg(rgb(PRIMARY_FIXED))
            .border_color(rgb(PRIMARY))
            .text_color(rgb(PRIMARY));
    }
    let d = d
        .bg(rgb(rest_bg))
        .when_some(rest_border, |d, a| d.border_color(rgba(OUTLINE_VAR, a)));
    if disabled {
        return d.text_color(rgba(MUTED, 0.5));
    }
    let d = d.text_color(rgb(MUTED));
    if hover_bg.is_none() && hover_fg.is_none() {
        return d;
    }
    d.hover(move |h| {
        let h = match hover_bg {
            Some(c) => h.bg(rgb(c)),
            None => h,
        };
        match hover_fg {
            Some(c) => h.text_color(rgb(c)),
            None => h,
        }
    })
}

/// 单个可点击的胶囊：由点击者决定下一状态（开关、循环筛选），不是互斥组。
/// 外观与 `SegmentStyle::Chip` 共用 [`ChipLook`]。
#[derive(IntoElement)]
pub struct ToggleChip {
    id: ElementId,
    label: SharedString,
    active: bool,
    look: ChipLook,
    icon: Option<IconFn>,
    on_click: Option<ClickHandler>,
}

impl ToggleChip {
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>, active: bool) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            active,
            look: ChipLook::CHIP,
            icon: None,
            on_click: None,
        }
    }

    pub fn look(mut self, look: ChipLook) -> Self {
        self.look = look;
        self
    }

    /// 参数是当前是否激活，调用方据此挑图标颜色。
    pub fn icon(mut self, icon: impl Fn(bool) -> AnyElement + 'static) -> Self {
        self.icon = Some(Rc::new(icon));
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

impl RenderOnce for ToggleChip {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let has_icon = self.icon.is_some();
        let icon = self.icon.map(|f| f(self.active));
        let el = apply_chip(
            div().id(self.id).text_xs().cursor_pointer(),
            self.active,
            false,
            &self.look,
        )
        .when(has_icon, |d| d.flex().items_center().gap_1p5())
        .children(icon)
        .child(self.label);
        match self.on_click {
            Some(f) => el.on_click(move |e, w, cx| f(e, w, cx)),
            None => el,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{point, Context, Modifiers, Render, TestAppContext};

    #[test]
    fn step_skips_disabled_and_stops_at_ends() {
        let e = [true, false, true];
        assert_eq!(step(&e, Some(0), 1), Some(2));
        assert_eq!(step(&e, Some(2), 1), None);
        assert_eq!(step(&e, Some(2), -1), Some(0));
        assert_eq!(step(&e, Some(0), -1), None);
        assert_eq!(step(&e, None, 1), Some(0));
        assert_eq!(step(&e, None, -1), Some(2));
        assert_eq!(step(&[false, false], Some(0), 1), None);
    }

    struct Fixture {
        selected: u8,
        picks: Vec<u8>,
        reselect: bool,
        focus: FocusHandle,
    }

    impl Fixture {
        fn new(reselect: bool) -> impl FnOnce(&mut Window, &mut Context<Self>) -> Self {
            move |window, cx| {
                let focus = cx.focus_handle();
                focus.focus(window);
                Self {
                    selected: 0,
                    picks: Vec::new(),
                    reselect,
                    focus,
                }
            }
        }
    }

    impl Render for Fixture {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            div().w(px(300.)).child(
                SegmentedControl::new(
                    "fixture-seg",
                    self.selected,
                    cx.listener(|this, v: &u8, _, cx| {
                        this.selected = *v;
                        this.picks.push(*v);
                        cx.notify();
                    }),
                )
                .style(SegmentStyle::Solid)
                .reselect(self.reselect)
                .focus_handle(self.focus.clone())
                .item(SegmentItem::new(0u8, "a"))
                .item(SegmentItem::new(1u8, "b").disabled(true))
                .item(SegmentItem::new(2u8, "c")),
            )
        }
    }

    // Solid 三等分 300px：中心 x 约为 50 / 150 / 250
    #[gpui::test]
    fn click_selects_and_disabled_is_inert(cx: &mut TestAppContext) {
        let (view, cx) = cx.add_window_view(Fixture::new(true));
        cx.simulate_click(point(px(150.), px(10.)), Modifiers::none());
        view.read_with(cx, |t, _| assert!(t.picks.is_empty()));
        cx.simulate_click(point(px(250.), px(10.)), Modifiers::none());
        view.read_with(cx, |t, _| assert_eq!(t.picks, vec![2]));
        cx.simulate_click(point(px(250.), px(10.)), Modifiers::none());
        view.read_with(cx, |t, _| {
            assert_eq!(t.picks, vec![2, 2], "默认重复点击仍触发")
        });
    }

    #[gpui::test]
    fn reselect_off_ignores_selected_click(cx: &mut TestAppContext) {
        let (view, cx) = cx.add_window_view(Fixture::new(false));
        cx.simulate_click(point(px(50.), px(10.)), Modifiers::none());
        view.read_with(cx, |t, _| assert!(t.picks.is_empty()));
        cx.simulate_click(point(px(250.), px(10.)), Modifiers::none());
        view.read_with(cx, |t, _| assert_eq!(t.picks, vec![2]));
    }

    #[gpui::test]
    fn arrow_keys_skip_disabled_without_wrapping(cx: &mut TestAppContext) {
        let (view, cx) = cx.add_window_view(Fixture::new(true));
        cx.simulate_keystrokes("right");
        view.read_with(cx, |t, _| assert_eq!(t.picks, vec![2], "跳过禁用项"));
        cx.simulate_keystrokes("right");
        view.read_with(cx, |t, _| assert_eq!(t.picks, vec![2], "末尾不循环"));
        cx.simulate_keystrokes("left");
        view.read_with(cx, |t, _| assert_eq!(t.picks, vec![2, 0]));
        cx.simulate_keystrokes("left");
        view.read_with(cx, |t, _| assert_eq!(t.picks, vec![2, 0], "开头不循环"));
    }

    struct ChipFixture {
        toggles: usize,
        picks: Vec<u8>,
    }

    impl Render for ChipFixture {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let look = ChipLook {
                pad_x: ChipPadX::X2p5,
                radius: ChipRadius::Md,
                rest_border: None,
                hover_bg: None,
                ..ChipLook::CHIP
            };
            div()
                .w(px(300.))
                .flex()
                .flex_col()
                .child(
                    ToggleChip::new("fixture-toggle", "toggle", false)
                        .look(look)
                        .icon(|_| div().w(px(8.)).h(px(8.)).into_any_element())
                        .on_click(cx.listener(|this, _, _, _| this.toggles += 1)),
                )
                .child(
                    SegmentedControl::new(
                        "fixture-plain",
                        0u8,
                        cx.listener(|this, v: &u8, _, _| this.picks.push(*v)),
                    )
                    .style(SegmentStyle::Plain)
                    .chip_look(look)
                    .flex_none()
                    .item(SegmentItem::new(0u8, "a"))
                    .item(SegmentItem::new(1u8, "b")),
                )
        }
    }

    #[gpui::test]
    fn toggle_chip_click_and_plain_segment_select(cx: &mut TestAppContext) {
        let (view, cx) = cx.add_window_view(|_, _| ChipFixture {
            toggles: 0,
            picks: Vec::new(),
        });
        cx.simulate_click(point(px(10.), px(8.)), Modifiers::none());
        view.read_with(cx, |t, _| assert_eq!(t.toggles, 1));
        // Plain 槽在 ToggleChip 下方；第二项位于槽右侧
        cx.simulate_click(point(px(40.), px(36.)), Modifiers::none());
        view.read_with(cx, |t, _| assert_eq!(t.picks, vec![1]));
    }

    #[test]
    fn chip_default_look_is_chip_const() {
        assert_eq!(ChipLook::default(), ChipLook::CHIP);
        assert_eq!(ChipLook::CHIP.rest_border, Some(0.8));
    }
}
