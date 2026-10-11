//! 对话框外壳：模态遮罩 + 居中卡片 + 标题 / 正文 / 按钮区插槽。
//!
//! 默认 `OverlayPolicy::MODAL`：外部点击和 Esc 都不关闭，与现有四个弹窗
//! （confirm / residual / FDA / updater）的基线一致。需要时显式换策略并给
//! `on_dismiss`；Esc 还需要 `focus_handle`（按键只在焦点链内触发）。
//!
//! 返回的元素已经是 deferred 层（`PRIORITY_MODAL`），直接 `.child(dialog)` 到
//! 铺满窗口的定位容器里即可。

use crate::ui::components::cards::card;
use crate::ui::components::overlay::{ModalBackdrop, OverlayPolicy};
use crate::ui::theme::*;
use gpui::{
    deferred, div, prelude::*, px, rgb, AnyElement, App, ElementId, FocusHandle, FontWeight,
    ParentElement, SharedString, Window,
};

/// 现有弹窗有两套卡片几何：confirm / residual 是宽松的，updater 是紧凑的。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum DialogDensity {
    /// `p_6` / `gap_3` / `shadow_2xl`（confirm）。
    #[default]
    Roomy,
    /// `p_5` / `gap_4` / 卡片默认阴影（updater）。
    Compact,
}

#[derive(IntoElement)]
pub struct DialogFrame {
    id: ElementId,
    width: f32,
    policy: OverlayPolicy,
    density: DialogDensity,
    gap: Option<f32>,
    max_height: Option<f32>,
    priority: Option<usize>,
    focus_handle: Option<FocusHandle>,
    on_dismiss: Option<crate::ui::components::overlay::DismissHandler>,
    header: Option<AnyElement>,
    footer: Option<AnyElement>,
    children: Vec<AnyElement>,
}

impl DialogFrame {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            width: 460.,
            policy: OverlayPolicy::MODAL,
            density: DialogDensity::default(),
            gap: None,
            max_height: None,
            priority: None,
            focus_handle: None,
            on_dismiss: None,
            header: None,
            footer: None,
            children: Vec::new(),
        }
    }

    /// confirm 460、updater 420。
    pub fn width(mut self, width: f32) -> Self {
        self.width = width;
        self
    }

    pub fn density(mut self, density: DialogDensity) -> Self {
        self.density = density;
        self
    }

    /// 覆盖密度预设的子元素间距（residual / FDA 用 16，预设是 12）。
    pub fn gap(mut self, gap: f32) -> Self {
        self.gap = Some(gap);
        self
    }

    /// 卡片最大高度（residual 为 580）。
    pub fn max_height(mut self, max_height: f32) -> Self {
        self.max_height = Some(max_height);
        self
    }

    /// 覆盖 deferred 优先级。只对模态策略生效；同层叠加（如 updater 压在其它弹窗上）
    /// 用 `PRIORITY_MODAL + 1`，不要依赖挂载顺序。
    pub fn priority(mut self, priority: usize) -> Self {
        self.priority = Some(priority);
        self
    }

    pub fn policy(mut self, policy: OverlayPolicy) -> Self {
        self.policy = policy;
        self
    }

    pub fn on_dismiss(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_dismiss = Some(std::rc::Rc::new(handler));
        self
    }

    pub fn focus_handle(mut self, handle: &FocusHandle) -> Self {
        self.focus_handle = Some(handle.clone());
        self
    }

    /// 纯文字标题（`text_lg` 粗体 `TEXT`）。要图标徽章等就用 [`Self::header`]。
    pub fn title(self, title: impl Into<SharedString>) -> Self {
        self.header(
            div()
                .text_lg()
                .font_weight(FontWeight::BOLD)
                .text_color(rgb(TEXT))
                .child(title.into()),
        )
    }

    pub fn header(mut self, header: impl IntoElement) -> Self {
        self.header = Some(header.into_any_element());
        self
    }

    /// 按钮区（ActionBar 或一排 `Button`），渲染在正文之后。
    pub fn footer(mut self, footer: impl IntoElement) -> Self {
        self.footer = Some(footer.into_any_element());
        self
    }
}

/// 子元素即正文。
impl ParentElement for DialogFrame {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl RenderOnce for DialogFrame {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let policy = self.policy;
        let card = card().w(px(self.width)).flex().flex_col();
        let card = match self.density {
            DialogDensity::Roomy => card.shadow_2xl().p_6().gap_3(),
            DialogDensity::Compact => card.p_5().gap_4(),
        };
        let card = match self.gap {
            Some(gap) => card.gap(px(gap)),
            None => card,
        };
        let card = match self.max_height {
            Some(h) => card.max_h(px(h)),
            None => card,
        };
        // 卡片内的点击不能落到遮罩上触发外部关闭。
        let card = card
            .id(self.id.clone())
            .on_any_mouse_down(|_, _, cx| cx.stop_propagation())
            .children(self.header)
            .children(self.children)
            .children(self.footer);

        let mut backdrop = ModalBackdrop::new((self.id, "backdrop"))
            .policy(policy)
            .child(card);
        if let Some(handle) = &self.focus_handle {
            backdrop = backdrop.focus_handle(handle);
        }
        if let Some(handler) = self.on_dismiss {
            backdrop = backdrop.on_dismiss(move |window, cx| handler(window, cx));
        }
        match self.priority {
            Some(priority) if policy.modal => {
                deferred(backdrop).priority(priority).into_any_element()
            }
            _ => policy.layer(backdrop),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{point, Context, Modifiers, Render, TestAppContext};

    struct Fixture {
        policy: OverlayPolicy,
        table_clicks: usize,
        dismissed: usize,
        card_clicks: usize,
    }

    impl Fixture {
        fn new(_: &mut Window, _: &mut Context<Self>) -> Self {
            Self {
                policy: OverlayPolicy::MODAL,
                table_clicks: 0,
                dismissed: 0,
                card_clicks: 0,
            }
        }
    }

    impl Render for Fixture {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let view = cx.entity();
            div()
                .relative()
                .size_full()
                .child(
                    div()
                        .id("fixture-table")
                        .absolute()
                        .size_full()
                        .on_any_mouse_down(cx.listener(|this, _, _, _| this.table_clicks += 1)),
                )
                .child(
                    DialogFrame::new("fixture-dialog")
                        .width(200.)
                        .policy(self.policy)
                        .title("Title")
                        .on_dismiss(move |_, app| {
                            view.update(app, |this, cx| {
                                this.dismissed += 1;
                                cx.notify();
                            })
                        })
                        .child(
                            div()
                                .id("fixture-body")
                                .h(px(60.))
                                .on_click(cx.listener(|this, _, _, _| this.card_clicks += 1)),
                        ),
                )
        }
    }

    // 窗口中心：卡片宽 200 居中，必然落在卡片里。
    fn center(cx: &mut gpui::VisualTestContext) -> gpui::Point<gpui::Pixels> {
        let size = cx.update(|window, _| window.viewport_size());
        point(
            px(f32::from(size.width) / 2.),
            px(f32::from(size.height) / 2.),
        )
    }

    #[gpui::test]
    fn modal_blocks_layer_below_and_ignores_outside_click(cx: &mut TestAppContext) {
        let (view, cx) = cx.add_window_view(Fixture::new);
        cx.simulate_click(point(px(5.), px(5.)), Modifiers::none());
        view.read_with(cx, |this, _| {
            assert_eq!(this.dismissed, 0, "default policy never closes on outside");
            assert_eq!(this.table_clicks, 0, "backdrop occludes the layer below");
        });
    }

    // 两个模态叠放：高优先级的在上，点击落在它的遮罩上，不会触达低优先级弹窗。
    struct Stacked {
        lower_clicks: usize,
        upper_clicks: usize,
    }

    impl Render for Stacked {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            div()
                .relative()
                .size_full()
                // 低优先级的后挂载：只靠 priority 也要压在下面。
                .child(
                    DialogFrame::new("upper")
                        .width(200.)
                        .priority(crate::ui::components::overlay::PRIORITY_MODAL + 1)
                        .child(
                            div()
                                .id("upper-body")
                                .h(px(60.))
                                .on_click(cx.listener(|this, _, _, _| this.upper_clicks += 1)),
                        ),
                )
                .child(
                    DialogFrame::new("lower").width(200.).child(
                        div()
                            .id("lower-body")
                            .h(px(60.))
                            .on_click(cx.listener(|this, _, _, _| this.lower_clicks += 1)),
                    ),
                )
        }
    }

    #[gpui::test]
    fn priority_override_keeps_dialog_on_top_regardless_of_mount_order(cx: &mut TestAppContext) {
        let (view, cx) = cx.add_window_view(|_, _| Stacked {
            lower_clicks: 0,
            upper_clicks: 0,
        });
        let at = center(cx);
        cx.simulate_click(at, Modifiers::none());
        view.read_with(cx, |this, _| {
            assert_eq!(this.upper_clicks, 1);
            assert_eq!(this.lower_clicks, 0);
        });
    }

    #[gpui::test]
    fn modal_card_click_neither_dismisses_nor_leaks_to_layer_below(cx: &mut TestAppContext) {
        let (view, cx) = cx.add_window_view(Fixture::new);
        let at = center(cx);
        cx.simulate_click(at, Modifiers::none());
        view.read_with(cx, |this, _| {
            assert_eq!(this.dismissed, 0);
            assert_eq!(this.table_clicks, 0);
            assert_eq!(this.card_clicks, 1);
        });
    }

    #[gpui::test]
    fn dismissable_policy_closes_on_backdrop_but_not_on_card(cx: &mut TestAppContext) {
        let (view, cx) = cx.add_window_view(Fixture::new);
        view.update(cx, |this, cx| {
            this.policy = OverlayPolicy::DISMISSABLE_MODAL;
            cx.notify();
        });
        let at = center(cx);
        cx.simulate_click(at, Modifiers::none());
        view.read_with(cx, |this, _| {
            assert_eq!(this.dismissed, 0, "card click is not an outside click");
        });
        cx.simulate_click(point(px(5.), px(5.)), Modifiers::none());
        view.read_with(cx, |this, _| {
            assert_eq!(this.dismissed, 1);
            assert_eq!(this.table_clicks, 0);
        });
    }
}
