//! 浮层基础：关闭策略、统一层级、模态遮罩、窗口边缘避让。
//!
//! 三种预设（见 `docs/UI_MIGRATION.md` 第 2 节的现状基线）：
//!
//! | 预设 | 阻断后层 | 外部点击 | Esc |
//! | --- | --- | --- | --- |
//! | `MODAL` | 是（遮罩 `occlude`） | 不关闭 | 不关闭 |
//! | `DISMISSABLE_MODAL` | 是 | 关闭 | 关闭 |
//! | `POPUP` | 否，只有浮层自身 `occlude` | 关闭，点击继续送达目的控件 | 关闭 |
//!
//! `MODAL` 与现有四个弹窗（confirm / residual / FDA / updater）的基线一致。
//! `POPUP` 沿用 `popover.rs` 的做法：`occlude()` 隔离 hover 与滚轮，
//! `on_mouse_down_out` 关闭而不吞掉外部点击（PITFALLS P53）。
//!
//! 定位约定：浮层内容用 `absolute` 摆放，窗口坐标，因此挂载点必须是铺满窗口的
//! 定位容器（`Root::render` 的根 div 即是）。

use crate::ui::components::popover::popover_overlay;
use crate::ui::theme::rgba;
use gpui::{
    deferred, div, point, prelude::*, px, AnyElement, App, ElementId, FocusHandle, ParentElement,
    Pixels, Point, Size, Window,
};
use std::rc::Rc;

/// 关闭回调。调用方通常用 `cx.entity()` 克隆后在里面 `update` 自己的视图。
pub type DismissHandler = Rc<dyn Fn(&mut Window, &mut App)>;

/// `deferred` 优先级：模态在 popup 之上；popup 值与 `popover_overlay` 的 100 相同。
pub const PRIORITY_POPUP: usize = 100;
pub const PRIORITY_MODAL: usize = 200;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OverlayPolicy {
    /// 阻断后层的 hover / 点击 / 滚轮。
    pub modal: bool,
    pub close_on_outside: bool,
    pub close_on_escape: bool,
}

impl OverlayPolicy {
    pub const MODAL: Self = Self {
        modal: true,
        close_on_outside: false,
        close_on_escape: false,
    };
    pub const DISMISSABLE_MODAL: Self = Self {
        modal: true,
        close_on_outside: true,
        close_on_escape: true,
    };
    pub const POPUP: Self = Self {
        modal: false,
        close_on_outside: true,
        close_on_escape: true,
    };

    pub const fn close_on_outside(mut self, value: bool) -> Self {
        self.close_on_outside = value;
        self
    }

    pub const fn close_on_escape(mut self, value: bool) -> Self {
        self.close_on_escape = value;
        self
    }

    pub const fn priority(self) -> usize {
        if self.modal {
            PRIORITY_MODAL
        } else {
            PRIORITY_POPUP
        }
    }

    /// IME 组合中的 Esc 属于输入法，不关闭浮层。
    pub fn key_dismisses(self, key: &str, composing: bool) -> bool {
        self.close_on_escape && !composing && key == "escape"
    }

    /// 按策略把内容放进 deferred 层。
    pub fn layer(self, content: impl IntoElement) -> AnyElement {
        if self.modal {
            deferred(content)
                .priority(self.priority())
                .into_any_element()
        } else {
            overlay_layer(content)
        }
    }
}

/// popup 级 deferred 层，直接复用 `popover_overlay`（后续可与其合并）。
pub fn overlay_layer(content: impl IntoElement) -> AnyElement {
    popover_overlay(content)
}

/// 全窗口模态遮罩：`occlude` 挡住后层的 hover / 点击 / 滚轮，内容居中。
///
/// 只有策略的 `close_on_outside` 为真且给了 `on_dismiss` 时，点击遮罩才关闭；
/// Esc 同理，且需要 `focus_handle`（按键只在焦点链内触发）。
/// 内容里不应触发外部关闭的区域（卡片）必须自己拦截 mouse down，
/// `DialogFrame` 已经这样做。
#[derive(IntoElement)]
pub struct ModalBackdrop {
    id: ElementId,
    policy: OverlayPolicy,
    on_dismiss: Option<DismissHandler>,
    focus_handle: Option<FocusHandle>,
    children: Vec<AnyElement>,
}

impl ModalBackdrop {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            policy: OverlayPolicy::MODAL,
            on_dismiss: None,
            focus_handle: None,
            children: Vec::new(),
        }
    }

    pub fn policy(mut self, policy: OverlayPolicy) -> Self {
        self.policy = policy;
        self
    }

    pub fn on_dismiss(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_dismiss = Some(Rc::new(handler));
        self
    }

    pub fn focus_handle(mut self, handle: &FocusHandle) -> Self {
        self.focus_handle = Some(handle.clone());
        self
    }
}

impl ParentElement for ModalBackdrop {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl RenderOnce for ModalBackdrop {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let policy = self.policy;
        let mut backdrop = div()
            .id(self.id)
            .absolute()
            .inset_0()
            .occlude()
            .bg(rgba(0x000000, 0.45))
            .flex()
            .items_center()
            .justify_center();

        if let Some(handler) = self.on_dismiss {
            if policy.close_on_outside {
                let handler = handler.clone();
                backdrop = backdrop.on_any_mouse_down(move |_, window, cx| handler(window, cx));
            }
            if policy.close_on_escape {
                if let Some(handle) = &self.focus_handle {
                    backdrop = backdrop.track_focus(handle);
                }
                backdrop = backdrop.on_key_down(move |event, window, cx| {
                    if policy.key_dismisses(&event.keystroke.key, false) {
                        handler(window, cx);
                        cx.stop_propagation();
                    }
                });
            }
        }
        backdrop.children(self.children)
    }
}

/// 把浮层摆在锚点旁并避开窗口边缘（窗口坐标，纯函数）。
///
/// 每个轴独立处理：
/// 1. 默认把左上角放在锚点；
/// 2. 右/下方放不下（含 `margin`）时，翻到锚点另一侧（右/下边缘贴锚点）；
///    翻转后仍越过左/上边缘则不翻转；
/// 3. 最后钳位到 `[margin, window - size - margin]`；
///    浮层比窗口还大时贴 `margin`。
pub fn anchored_position(
    anchor: Point<Pixels>,
    size: Size<Pixels>,
    window: Size<Pixels>,
    margin: Pixels,
) -> Point<Pixels> {
    let margin = f32::from(margin).max(0.);
    point(
        px(place_axis(
            f32::from(anchor.x),
            f32::from(size.width),
            f32::from(window.width),
            margin,
        )),
        px(place_axis(
            f32::from(anchor.y),
            f32::from(size.height),
            f32::from(window.height),
            margin,
        )),
    )
}

fn place_axis(anchor: f32, extent: f32, window: f32, margin: f32) -> f32 {
    let max = window - extent - margin;
    if max < margin {
        return margin;
    }
    let start = if anchor + extent + margin > window && anchor - extent >= margin {
        anchor - extent
    } else {
        anchor
    };
    start.clamp(margin, max)
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::size;

    #[test]
    fn fits_is_unchanged() {
        assert_eq!(place_axis(100., 200., 1000., 10.), 100.);
    }

    #[test]
    fn overflow_flips_to_the_other_side_of_anchor() {
        // 900 + 200 + 10 > 1000，翻到 700..900。
        assert_eq!(place_axis(900., 200., 1000., 10.), 700.);
    }

    #[test]
    fn overflow_without_room_to_flip_clamps() {
        // 翻转得 -50 < margin，不翻转，钳位到 1000 - 300 - 10。
        assert_eq!(place_axis(250., 300., 400., 10.), 90.);
        assert_eq!(place_axis(50., 300., 400., 10.), 50.);
    }

    #[test]
    fn exact_edge_counts_as_fit() {
        assert_eq!(place_axis(790., 200., 1000., 10.), 790.);
        assert_eq!(place_axis(791., 200., 1000., 10.), 591.);
    }

    #[test]
    fn negative_or_small_anchor_clamps_to_margin() {
        assert_eq!(place_axis(-30., 100., 500., 10.), 10.);
        assert_eq!(place_axis(0., 100., 500., 10.), 10.);
    }

    #[test]
    fn larger_than_window_pins_to_margin() {
        assert_eq!(place_axis(100., 600., 500., 10.), 10.);
        assert_eq!(place_axis(100., 480., 500., 10.), 10.);
    }

    #[test]
    fn zero_margin_and_negative_margin() {
        assert_eq!(place_axis(900., 100., 1000., 0.), 900.);
        assert_eq!(place_axis(950., 100., 1000., 0.), 850.);
        let p = anchored_position(
            point(px(950.), px(10.)),
            size(px(100.), px(50.)),
            size(px(1000.), px(600.)),
            px(-5.),
        );
        assert_eq!(f32::from(p.x), 850.);
        assert_eq!(f32::from(p.y), 10.);
    }

    #[test]
    fn axes_are_independent() {
        let p = anchored_position(
            point(px(1000.), px(590.)),
            size(px(230.), px(120.)),
            size(px(1040.), px(620.)),
            px(10.),
        );
        assert_eq!(f32::from(p.x), 770.);
        assert_eq!(f32::from(p.y), 470.);
        let q = anchored_position(
            point(px(20.), px(30.)),
            size(px(230.), px(120.)),
            size(px(1040.), px(620.)),
            px(10.),
        );
        assert_eq!((f32::from(q.x), f32::from(q.y)), (20., 30.));
    }

    #[test]
    fn policy_presets_match_baseline() {
        let m = OverlayPolicy::MODAL;
        assert!(m.modal && !m.close_on_outside && !m.close_on_escape);
        let d = OverlayPolicy::DISMISSABLE_MODAL;
        assert!(d.modal && d.close_on_outside && d.close_on_escape);
        let p = OverlayPolicy::POPUP;
        assert!(!p.modal && p.close_on_outside && p.close_on_escape);
        assert!(m.priority() > p.priority());
        assert_eq!(p.priority(), PRIORITY_POPUP);
    }

    #[test]
    fn escape_respects_policy_and_ime() {
        assert!(!OverlayPolicy::MODAL.key_dismisses("escape", false));
        assert!(OverlayPolicy::POPUP.key_dismisses("escape", false));
        assert!(!OverlayPolicy::POPUP.key_dismisses("escape", true));
        assert!(!OverlayPolicy::POPUP.key_dismisses("enter", false));
        assert!(OverlayPolicy::MODAL
            .close_on_escape(true)
            .key_dismisses("escape", false));
    }
}
