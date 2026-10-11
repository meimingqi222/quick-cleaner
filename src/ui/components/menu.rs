//! 弹出菜单：`Menu` + `MenuItem` + `MenuSeparator`。
//!
//! 外观取自 apps / declutter 两处右键菜单；`Menu` 按 `OverlayPolicy::POPUP`
//! 渲染：自身 `occlude`，菜单以外的 mouse down（任意键）触发 `on_dismiss`，
//! 点击继续送达目的控件。没有全屏遮罩。
//!
//! `MenuItem` 的 `on_click` 之后是否关闭菜单由调用方决定（与现有两处一致：
//! 回调里自己关）。`disabled` 的项不挂 `on_click`，也没有 hover 与手型光标。

use crate::ui::components::overlay::{anchored_position, DismissHandler};
use crate::ui::components::ClickHandler;
use crate::ui::theme::*;
use gpui::{
    div, prelude::*, px, rgb, AnyElement, App, Bounds, ClickEvent, ElementId, FontWeight,
    ParentElement, Pixels, Point, SharedString, Window,
};
use std::rc::Rc;

/// 单行高度的估算（text_xs 行高 16 + py 7*2）加 gap 2，用于没有给高度时的避让。
const ITEM_H_ESTIMATE: f32 = 32.;
const HEADER_H_ESTIMATE: f32 = 56.;
const MENU_PAD: f32 = 8.;
/// 与旧实现 `clamp(10, ..)` 一致。
const EDGE_MARGIN: f32 = 10.;

/// 两处旧菜单描边色不同：apps 是不透明 `SURF_HIGHEST`，declutter 是
/// `OUTLINE_VAR` 0.45。做成参数，默认 `Soft`（与 popover 卡片同族）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum MenuBorder {
    /// `rgba(OUTLINE_VAR, 0.45)`，头部分隔线 0.3（declutter）。
    #[default]
    Soft,
    /// `SURF_HIGHEST`，头部分隔线 `rgba(OUTLINE_VAR, 0.4)`（apps）。
    Solid,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum MenuTone {
    /// `TEXT`，hover 灰底（apps 常规项）。
    #[default]
    Normal,
    /// `TEXT`，hover 灰底且字变 `PRIMARY`（declutter 项）。
    Accent,
    /// `PRIMARY` 粗体，hover 浅主色底（apps「强力深度清理」）。
    Primary,
    /// `MUTED`，hover 灰底（apps「复制路径」）。
    Muted,
    /// `ERROR`，hover 浅红底。
    Danger,
}

#[derive(IntoElement)]
pub struct MenuItem {
    id: ElementId,
    label: SharedString,
    icon: Option<AnyElement>,
    tone: MenuTone,
    disabled: bool,
    on_click: Option<ClickHandler>,
}

impl MenuItem {
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            icon: None,
            tone: MenuTone::Normal,
            disabled: false,
            on_click: None,
        }
    }

    pub fn icon(mut self, icon: impl IntoElement) -> Self {
        self.icon = Some(icon.into_any_element());
        self
    }

    pub fn tone(mut self, tone: MenuTone) -> Self {
        self.tone = tone;
        self
    }

    /// `Danger` 的简写。
    pub fn danger(self) -> Self {
        self.tone(MenuTone::Danger)
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
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

impl RenderOnce for MenuItem {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let weight = if self.tone == MenuTone::Primary {
            FontWeight::BOLD
        } else {
            FontWeight::MEDIUM
        };
        let el = div()
            .id(self.id)
            .px_3()
            .py(px(7.))
            .rounded_md()
            .flex()
            .items_center()
            .gap_2()
            .text_xs()
            .font_weight(weight);

        let el = if self.disabled {
            el.text_color(rgb(OUTLINE))
        } else {
            let el = match self.tone {
                MenuTone::Normal => el.text_color(rgb(TEXT)).hover(|h| h.bg(rgb(SURF_HIGH))),
                MenuTone::Accent => el
                    .text_color(rgb(TEXT))
                    .hover(|h| h.bg(rgb(SURF_HIGH)).text_color(rgb(PRIMARY))),
                MenuTone::Primary => el
                    .text_color(rgb(PRIMARY))
                    .hover(|h| h.bg(rgba(PRIMARY, 0.08))),
                MenuTone::Muted => el.text_color(rgb(MUTED)).hover(|h| h.bg(rgb(SURF_HIGH))),
                MenuTone::Danger => el.text_color(rgb(ERROR)).hover(|h| h.bg(rgba(ERROR, 0.08))),
            };
            let el = el.cursor_pointer();
            match self.on_click {
                Some(handler) => el.on_click(move |event, window, cx| handler(event, window, cx)),
                None => el,
            }
        };

        el.children(self.icon).child(self.label)
    }
}

/// 项之间的细分隔线。
#[derive(IntoElement, Default)]
pub struct MenuSeparator;

impl RenderOnce for MenuSeparator {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        div()
            .h(px(1.))
            .mx_2()
            .my_1()
            .flex_none()
            .bg(rgba(OUTLINE_VAR, 0.4))
    }
}

#[derive(IntoElement)]
pub struct Menu {
    id: ElementId,
    anchor: Point<Pixels>,
    width: f32,
    estimated_height: Option<f32>,
    border: MenuBorder,
    header: Option<AnyElement>,
    on_dismiss: Option<DismissHandler>,
    excludes: Vec<Bounds<Pixels>>,
    children: Vec<AnyElement>,
}

fn in_any(bounds: &[Bounds<Pixels>], position: Point<Pixels>) -> bool {
    bounds.iter().any(|b| b.contains(&position))
}

impl Menu {
    /// `anchor` 是窗口坐标（通常是右键位置，需要偏移的话由调用方先减）。
    pub fn new(id: impl Into<ElementId>, anchor: Point<Pixels>) -> Self {
        Self {
            id: id.into(),
            anchor,
            width: 230.,
            estimated_height: None,
            border: MenuBorder::default(),
            header: None,
            on_dismiss: None,
            excludes: Vec::new(),
            children: Vec::new(),
        }
    }

    /// apps 菜单 230，declutter 菜单 250。
    pub fn width(mut self, width: f32) -> Self {
        self.width = width;
        self
    }

    /// 覆盖窗口边缘避让用的高度；默认按项数估算。
    pub fn estimated_height(mut self, height: f32) -> Self {
        self.estimated_height = Some(height);
        self
    }

    pub fn border(mut self, border: MenuBorder) -> Self {
        self.border = border;
        self
    }

    /// 标题区（文件名、应用名等）；底部自带分隔线。
    pub fn header(mut self, header: impl IntoElement) -> Self {
        self.header = Some(header.into_any_element());
        self
    }

    /// 落在该区域内的外部 mouse down 不触发 `on_dismiss`，可多次调用。
    /// 用于打开菜单的触发钮：由触发钮自己的 click 负责 toggle，避免先关后开。
    pub fn exclude(mut self, bounds: Bounds<Pixels>) -> Self {
        self.excludes.push(bounds);
        self
    }

    /// `exclude` 的 `Option` 版本；`None` 时不排除（触发钮尚未布局过）。
    pub fn exclude_opt(self, bounds: Option<Bounds<Pixels>>) -> Self {
        match bounds {
            Some(bounds) => self.exclude(bounds),
            None => self,
        }
    }

    /// 菜单自身以外的 mouse down（任意键）触发；菜单内部的点击不触发。
    pub fn on_dismiss(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_dismiss = Some(Rc::new(handler));
        self
    }
}

impl ParentElement for Menu {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl RenderOnce for Menu {
    fn render(self, window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let height = self.estimated_height.unwrap_or_else(|| {
            MENU_PAD
                + self.children.len() as f32 * ITEM_H_ESTIMATE
                + if self.header.is_some() {
                    HEADER_H_ESTIMATE
                } else {
                    0.
                }
        });
        let at = anchored_position(
            self.anchor,
            gpui::size(px(self.width), px(height)),
            window.viewport_size(),
            px(EDGE_MARGIN),
        );
        let (border_color, divider) = match self.border {
            MenuBorder::Soft => (rgba(OUTLINE_VAR, 0.45), rgba(OUTLINE_VAR, 0.3)),
            MenuBorder::Solid => (rgb(SURF_HIGHEST).into(), rgba(OUTLINE_VAR, 0.4)),
        };

        let mut menu = div()
            .id(self.id)
            // stop_propagation 不改变 hover / 滚轮命中，必须 occlude（P53）。
            .occlude()
            .absolute()
            .left(at.x)
            .top(at.y)
            .w(px(self.width))
            .p_1()
            .rounded_xl()
            .bg(rgb(CARD))
            .border_1()
            .border_color(border_color)
            .shadow_xl()
            .flex()
            .flex_col()
            .gap(px(2.))
            .on_any_mouse_down(|_, _, cx| cx.stop_propagation())
            .on_scroll_wheel(|_, _, cx| cx.stop_propagation());
        menu.interactivity()
            .on_any_mouse_up(|_, _, cx| cx.stop_propagation());

        if let Some(handler) = self.on_dismiss {
            let excludes = self.excludes;
            menu = menu.on_mouse_down_out(move |event, window, cx| {
                if !in_any(&excludes, event.position) {
                    handler(window, cx);
                }
            });
        }

        let menu = menu.when_some(self.header, |d, header| {
            d.child(
                div()
                    .px_3()
                    .py_2()
                    .border_b_1()
                    .border_color(divider)
                    .flex()
                    .flex_col()
                    .child(header),
            )
        });
        // 调用方把返回值交给 `OverlayPolicy::POPUP.layer(..)` / `overlay_layer(..)`。
        menu.children(self.children)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{point, Context, Modifiers, Render, TestAppContext};

    struct Fixture {
        table_clicks: usize,
        open: bool,
        dismissed: usize,
        picked: usize,
        disabled_picked: usize,
    }

    impl Fixture {
        fn new(_: &mut Window, _: &mut Context<Self>) -> Self {
            Self {
                table_clicks: 0,
                open: true,
                dismissed: 0,
                picked: 0,
                disabled_picked: 0,
            }
        }
    }

    impl Render for Fixture {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let view = cx.entity();
            let menu = self.open.then(|| {
                let dismiss_view = view.clone();
                crate::ui::components::overlay::overlay_layer(
                    Menu::new("fixture-menu", point(px(50.), px(50.)))
                        .width(100.)
                        .exclude(Bounds::new(
                            point(px(300.), px(300.)),
                            gpui::size(px(50.), px(50.)),
                        ))
                        .on_dismiss(move |_, app| {
                            dismiss_view.update(app, |this, cx| {
                                this.dismissed += 1;
                                this.open = false;
                                cx.notify();
                            })
                        })
                        .child(MenuItem::new("item-a", "Alpha").on_click(cx.listener(
                            |this, _, _, cx| {
                                this.picked += 1;
                                cx.notify();
                            },
                        )))
                        .child(MenuItem::new("item-b", "Beta").disabled(true).on_click(
                            cx.listener(|this, _, _, cx| {
                                this.disabled_picked += 1;
                                cx.notify();
                            }),
                        )),
                )
            });
            div()
                .relative()
                .size_full()
                .child(
                    div()
                        .id("fixture-table")
                        .absolute()
                        .w(px(600.))
                        .h(px(600.))
                        .on_any_mouse_down(cx.listener(|this, _, _, _| this.table_clicks += 1)),
                )
                .children(menu)
        }
    }

    // 菜单左上角 (50,50)；项高约 30，间距 2：第一项中心约 y=70，第二项约 y=102。
    const ITEM_A: (f32, f32) = (80., 70.);
    const ITEM_B: (f32, f32) = (80., 102.);

    #[gpui::test]
    fn enabled_item_fires_and_disabled_does_not(cx: &mut TestAppContext) {
        let (view, cx) = cx.add_window_view(Fixture::new);
        cx.simulate_click(point(px(ITEM_B.0), px(ITEM_B.1)), Modifiers::none());
        view.read_with(cx, |this, _| {
            assert_eq!(this.disabled_picked, 0, "disabled item must not fire");
            assert_eq!(this.picked, 0);
            assert!(this.open, "click inside the menu is not an outside click");
            assert_eq!(this.table_clicks, 0, "menu occludes the layer below");
        });
        cx.simulate_click(point(px(ITEM_A.0), px(ITEM_A.1)), Modifiers::none());
        view.read_with(cx, |this, _| {
            assert_eq!(this.picked, 1);
            assert_eq!(this.dismissed, 0);
        });
    }

    #[gpui::test]
    fn click_inside_excluded_bounds_does_not_dismiss(cx: &mut TestAppContext) {
        let (view, cx) = cx.add_window_view(Fixture::new);
        cx.simulate_click(point(px(320.), px(320.)), Modifiers::none());
        view.read_with(cx, |this, _| {
            assert_eq!(this.dismissed, 0, "excluded region is not an outside click");
            assert!(this.open);
            assert!(this.table_clicks > 0, "click still reaches the trigger");
        });
    }

    #[test]
    fn in_any_checks_every_region() {
        let a = Bounds::new(point(px(0.), px(0.)), gpui::size(px(10.), px(10.)));
        let b = Bounds::new(point(px(100.), px(100.)), gpui::size(px(10.), px(10.)));
        assert!(in_any(&[a, b], point(px(105.), px(105.))));
        assert!(in_any(&[a, b], point(px(5.), px(5.))));
        assert!(!in_any(&[a, b], point(px(50.), px(50.))));
        assert!(!in_any(&[], point(px(5.), px(5.))));
    }

    #[gpui::test]
    fn outside_click_dismisses_and_reaches_target(cx: &mut TestAppContext) {
        let (view, cx) = cx.add_window_view(Fixture::new);
        cx.simulate_click(point(px(400.), px(400.)), Modifiers::none());
        view.read_with(cx, |this, _| {
            assert_eq!(this.dismissed, 1);
            assert!(!this.open);
            assert!(this.table_clicks > 0, "popup must not swallow the click");
        });
    }
}
