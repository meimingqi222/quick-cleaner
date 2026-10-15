//! 浮层栈：记录当前打开的浮层及其顺序，决定 Esc 只作用于最上层。
//!
//! 真值仍是 `Root` 上各浮层自己的状态；栈每帧由 `Root::sync_overlays` 推导，
//! 渲染优先级与关闭路由共享同一顺序，包括状态搜索候选。

use super::View;
use crate::ui::components::overlay::OverlayPolicy;
use crate::ui::Root;
use gpui::{Context, FocusHandle, KeyDownEvent, Window};

pub type CloseAction = fn(&mut Root, &mut Context<Root>);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OverlayId {
    Confirm,
    Residual,
    Fda,
    AppsMenu,
    DeclutterMenu,
    DiskVolume,
    Update,
    StatusSearch,
}

impl OverlayId {
    /// 各浮层的关闭策略。要让某个弹窗响应 Esc，只改这里。
    pub const fn policy(self) -> OverlayPolicy {
        match self {
            Self::Confirm | Self::Residual | Self::Fda | Self::Update => OverlayPolicy::MODAL,
            Self::AppsMenu | Self::DeclutterMenu | Self::DiskVolume | Self::StatusSearch => {
                OverlayPolicy::POPUP
            }
        }
    }

    fn close_action(self) -> CloseAction {
        match self {
            Self::AppsMenu => |root, _| root.close_context_menu(),
            Self::DeclutterMenu => |root, _| root.close_declutter_context_menu(),
            Self::DiskVolume => |root, _| root.close_disk_volume_menu(),
            Self::StatusSearch => |root, _| root.monitor.search_popover.dismiss(),
            Self::Update => Root::dismiss_update_dialog,
            Self::Confirm => |root, _| root.confirm = None,
            Self::Residual => |root, _| root.residual.result = None,
            Self::Fda => Root::close_fda_guide,
        }
    }
}

impl gpui_app_widgets::overlay_stack::OverlayIdentity for OverlayId {
    type Close = CloseAction;
    fn close_action(self) -> Self::Close {
        self.close_action()
    }
}
pub type OverlayEntry = gpui_app_widgets::overlay_stack::OverlayEntry<OverlayId>;
pub type OverlayStack = gpui_app_widgets::overlay_stack::OverlayStack<OverlayId>;
pub type EscapeOutcome = gpui_app_widgets::overlay_stack::EscapeOutcome<OverlayId>;
pub fn escape_target(stack: &[OverlayEntry], composing: bool) -> EscapeOutcome {
    gpui_app_widgets::overlay_stack::escape_target(stack, composing)
}

impl Root {
    fn visible_focus_target(&self, handle: &FocusHandle, window: &Window) -> bool {
        let visible = if *handle == self.apps.input.focus_handle {
            self.view == View::Apps && self.residual.uninstall.is_none()
        } else if *handle == self.search.input.focus_handle {
            self.view == View::Search
        } else if *handle == self.monitor.input.focus_handle {
            self.view == View::Status && self.monitor.tab == crate::ui::state::StatusTab::Processes
        } else {
            true
        };
        visible && self.root_focus.contains(handle, window)
    }
    /// 由各浮层自己的状态推导当前打开的浮层并同步到栈。
    /// 同级顺序：confirm < residual < FDA < apps 菜单 < declutter 菜单 < 磁盘下拉 < 更新弹窗。
    pub(crate) fn sync_overlays(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let mut open = [
            (
                OverlayId::StatusSearch,
                self.view == View::Status
                    && self
                        .monitor
                        .search_popover
                        .is_open(self.monitor.input.focus_handle.is_focused(window)),
            ),
            (OverlayId::Confirm, self.confirm.is_some()),
            (OverlayId::Residual, self.residual.result.is_some()),
            (OverlayId::Fda, self.show_fda_onboarding),
            (OverlayId::AppsMenu, self.apps.context_menu.is_some()),
            (
                OverlayId::DeclutterMenu,
                self.declutter.context_menu.is_some(),
            ),
            (
                OverlayId::DiskVolume,
                self.disk.volume_menu_open && self.view == View::Disk,
            ),
            (
                OverlayId::Update,
                self.update.show_dialog && self.update.status.wants_attention(),
            ),
        ]
        .into_iter()
        .filter(|(_, is_open)| *is_open)
        .map(|(id, _)| (id, id.policy()))
        .collect::<Vec<_>>();

        // 根自己的焦点不算「触发源」，否则恢复焦点会回到根。
        let focused = window.focused(cx).filter(|h| *h != self.root_focus);
        let restore = self
            .overlays
            .entries()
            .iter()
            .rev()
            .find(|entry| !open.iter().any(|(id, _)| *id == entry.id))
            .and_then(|entry| entry.restore_focus.clone());
        let orphaned = self.overlays.orphaned(&open);
        for id in orphaned.iter().rev() {
            self.close_overlay(*id, cx);
        }
        open.retain(|(id, _)| !orphaned.contains(id));
        let new_ids: Vec<_> = open
            .iter()
            .filter(|(id, _)| !self.overlays.entries().iter().any(|entry| entry.id == *id))
            .map(|(id, _)| *id)
            .collect();
        let parent = self
            .overlays
            .entries()
            .last()
            .filter(|entry| entry.policy.modal && open.iter().any(|(id, _)| *id == entry.id))
            .map(|entry| entry.id);
        self.overlays.sync(&open, focused);
        if let Some(parent) = parent {
            for id in new_ids {
                self.overlays.set_parent(id, parent);
            }
        }
        if !self.overlays.modal_on_top()
            && window
                .focused(cx)
                .is_none_or(|focus| focus == self.root_focus)
        {
            if let Some(restore) =
                restore.filter(|handle| self.visible_focus_target(handle, window))
            {
                window.focus(&restore);
            }
        }
    }

    /// 输入法正在组合：Esc 属于输入法。
    fn ime_composing(&self) -> bool {
        self.search.input.marked.is_some()
            || self.apps.input.marked.is_some()
            || self.monitor.input.marked.is_some()
    }

    /// 根节点 Esc：只处理栈顶浮层。
    pub(crate) fn handle_root_escape(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if event.keystroke.key != "escape" {
            return false;
        }
        // Registration may have changed since the last frame (e.g. a second Esc).
        self.sync_overlays(window, cx);
        match escape_target(self.overlays.entries(), self.ime_composing()) {
            EscapeOutcome::Close(id) => {
                let restore = self.overlays.restore_focus(id);
                self.close_overlay(id, cx);
                // 外部点击关闭不走这里；这里只在没有别的焦点时归还。
                if let Some(handle) =
                    restore.filter(|handle| self.visible_focus_target(handle, window))
                {
                    let unowned = window
                        .focused(cx)
                        .is_none_or(|focused| focused == self.root_focus);
                    if unowned {
                        window.focus(&handle);
                    }
                }
                cx.notify();
                cx.stop_propagation();
            }
            EscapeOutcome::Swallow => cx.stop_propagation(),
            EscapeOutcome::PassThrough => return false,
        }
        true
    }

    /// 复用各浮层现有的关闭函数。模态弹窗当前策略不响应 Esc，不会走到这里。
    fn close_overlay(&mut self, id: OverlayId, cx: &mut Context<Self>) {
        for entry in self.overlays.remove_entries(id) {
            (entry.close)(self, cx);
        }
    }

    pub(crate) fn dismiss_overlay_outside(
        &mut self,
        id: OverlayId,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.overlays.is_top(id) && id.policy().close_on_outside {
            self.close_overlay(id, cx);
            cx.notify();
            return true;
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{div, prelude::*, Context, KeyDownEvent, Render, TestAppContext, Window};

    const MODAL: OverlayPolicy = OverlayPolicy::MODAL;
    const POPUP: OverlayPolicy = OverlayPolicy::POPUP;

    fn stack_of(open: &[(OverlayId, OverlayPolicy)]) -> OverlayStack {
        let mut stack = OverlayStack::default();
        stack.sync(open, None);
        stack
    }

    fn ids(stack: &OverlayStack) -> Vec<OverlayId> {
        stack.entries().iter().map(|e| e.id).collect()
    }

    #[test]
    fn policy_table_matches_baseline() {
        for id in [
            OverlayId::Confirm,
            OverlayId::Residual,
            OverlayId::Fda,
            OverlayId::Update,
        ] {
            assert_eq!(id.policy(), MODAL, "{id:?}");
        }
        for id in [
            OverlayId::AppsMenu,
            OverlayId::DeclutterMenu,
            OverlayId::DiskVolume,
        ] {
            assert_eq!(id.policy(), POPUP, "{id:?}");
        }
    }

    #[test]
    fn empty_stack_passes_through() {
        assert_eq!(escape_target(&[], false), EscapeOutcome::PassThrough);
        assert_eq!(escape_target(&[], true), EscapeOutcome::PassThrough);
    }

    #[test]
    fn top_modal_swallows_escape() {
        let stack = stack_of(&[(OverlayId::Confirm, MODAL)]);
        assert_eq!(
            escape_target(stack.entries(), false),
            EscapeOutcome::Swallow
        );
    }

    #[test]
    fn top_popup_closes() {
        let stack = stack_of(&[(OverlayId::AppsMenu, POPUP)]);
        assert_eq!(
            escape_target(stack.entries(), false),
            EscapeOutcome::Close(OverlayId::AppsMenu)
        );
    }

    #[test]
    fn popup_over_modal_closes_only_the_popup() {
        let stack = stack_of(&[(OverlayId::Confirm, MODAL), (OverlayId::DiskVolume, POPUP)]);
        assert_eq!(
            escape_target(stack.entries(), false),
            EscapeOutcome::Close(OverlayId::DiskVolume)
        );
    }

    #[test]
    fn modal_over_popup_swallows_instead_of_closing_the_popup() {
        let stack = stack_of(&[(OverlayId::AppsMenu, POPUP), (OverlayId::Update, MODAL)]);
        assert_eq!(
            escape_target(stack.entries(), false),
            EscapeOutcome::Swallow
        );
    }

    #[test]
    fn only_the_top_popup_closes() {
        let stack = stack_of(&[
            (OverlayId::AppsMenu, POPUP),
            (OverlayId::DeclutterMenu, POPUP),
        ]);
        assert_eq!(
            escape_target(stack.entries(), false),
            EscapeOutcome::Close(OverlayId::DeclutterMenu)
        );
    }

    #[test]
    fn composing_passes_through_for_any_top() {
        for top in [(OverlayId::Confirm, MODAL), (OverlayId::AppsMenu, POPUP)] {
            let stack = stack_of(&[top]);
            assert_eq!(
                escape_target(stack.entries(), true),
                EscapeOutcome::PassThrough
            );
        }
    }

    #[test]
    fn sync_keeps_order_pushes_new_and_drops_closed() {
        let mut stack = stack_of(&[(OverlayId::Confirm, MODAL), (OverlayId::AppsMenu, POPUP)]);
        // 入参顺序变化不影响已有条目的位置。
        stack.sync(
            &[
                (OverlayId::AppsMenu, POPUP),
                (OverlayId::DiskVolume, POPUP),
                (OverlayId::Confirm, MODAL),
            ],
            None,
        );
        assert_eq!(
            ids(&stack),
            [
                OverlayId::Confirm,
                OverlayId::AppsMenu,
                OverlayId::DiskVolume
            ]
        );
        stack.sync(&[(OverlayId::DiskVolume, POPUP)], None);
        assert_eq!(ids(&stack), [OverlayId::DiskVolume]);
        stack.sync(&[], None);
        assert!(stack.is_empty());
    }

    #[test]
    fn reopened_overlay_goes_back_on_top() {
        let mut stack = stack_of(&[(OverlayId::AppsMenu, POPUP), (OverlayId::Update, MODAL)]);
        stack.sync(&[(OverlayId::Update, MODAL)], None);
        stack.sync(
            &[(OverlayId::Update, MODAL), (OverlayId::AppsMenu, POPUP)],
            None,
        );
        assert_eq!(ids(&stack), [OverlayId::Update, OverlayId::AppsMenu]);
    }

    #[test]
    fn nested_close_is_children_first_and_render_order_matches_escape() {
        let mut stack = stack_of(&[
            (OverlayId::Confirm, MODAL),
            (OverlayId::StatusSearch, POPUP),
        ]);
        assert!(stack.set_parent(OverlayId::StatusSearch, OverlayId::Confirm));
        assert!(!stack.set_parent(OverlayId::Confirm, OverlayId::StatusSearch));
        stack.sync(
            &[
                (OverlayId::StatusSearch, POPUP),
                (OverlayId::Confirm, MODAL),
            ],
            None,
        );
        assert_eq!(stack.entries().len(), 2);
        assert!(stack.priority(OverlayId::StatusSearch) > stack.priority(OverlayId::Confirm));
        assert_eq!(
            escape_target(stack.entries(), false),
            EscapeOutcome::Close(OverlayId::StatusSearch)
        );
        assert_eq!(
            stack.remove(OverlayId::Confirm),
            [OverlayId::StatusSearch, OverlayId::Confirm]
        );
        assert!(stack.is_empty());
    }

    #[test]
    fn persistent_top_prevents_escape_reaching_the_popup_below() {
        let stack = stack_of(&[
            (OverlayId::AppsMenu, POPUP),
            (OverlayId::Update, OverlayPolicy::PERSISTENT),
        ]);
        assert_eq!(
            escape_target(stack.entries(), false),
            EscapeOutcome::Swallow
        );
    }

    #[test]
    fn disappearing_parent_removes_open_children_without_registering_them_again() {
        let mut stack = stack_of(&[
            (OverlayId::Residual, MODAL),
            (OverlayId::Confirm, MODAL),
            (OverlayId::StatusSearch, POPUP),
        ]);
        assert!(stack.set_parent(OverlayId::Confirm, OverlayId::Residual));
        assert!(stack.set_parent(OverlayId::StatusSearch, OverlayId::Confirm));
        let open = [
            (OverlayId::Confirm, MODAL),
            (OverlayId::StatusSearch, POPUP),
        ];
        assert_eq!(
            stack.orphaned(&open),
            [OverlayId::Confirm, OverlayId::StatusSearch]
        );
        stack.sync(&open, None);
        assert!(stack.is_empty());
    }

    struct Fixture {
        a: FocusHandle,
        b: FocusHandle,
        stack: OverlayStack,
        closed: Vec<OverlayId>,
    }

    impl Fixture {
        fn new(_window: &mut Window, cx: &mut Context<Self>) -> Self {
            Self {
                a: cx.focus_handle(),
                b: cx.focus_handle(),
                stack: OverlayStack::default(),
                closed: Vec::new(),
            }
        }
    }

    impl Render for Fixture {
        fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            div()
                .size_full()
                .track_focus(&self.a)
                .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                    if event.keystroke.key != "escape" {
                        return;
                    }
                    match escape_target(this.stack.entries(), false) {
                        EscapeOutcome::Close(id) => {
                            this.closed.push(id);
                            let open: Vec<_> = this
                                .stack
                                .entries()
                                .iter()
                                .filter(|e| e.id != id)
                                .map(|e| (e.id, e.policy))
                                .collect();
                            this.stack.sync(&open, None);
                            cx.stop_propagation();
                        }
                        EscapeOutcome::Swallow => cx.stop_propagation(),
                        EscapeOutcome::PassThrough => {}
                    }
                }))
        }
    }

    #[gpui::test]
    fn sync_snapshots_focus_only_when_opening(cx: &mut TestAppContext) {
        let (view, cx) = cx.add_window_view(Fixture::new);
        let (a, b) = view.read_with(cx, |f, _| (f.a.clone(), f.b.clone()));
        view.update(cx, |f, _| {
            f.stack
                .sync(&[(OverlayId::AppsMenu, POPUP)], Some(a.clone()));
            // 之后焦点变了，已有条目的快照不变，新条目拿到新焦点。
            f.stack.sync(
                &[(OverlayId::AppsMenu, POPUP), (OverlayId::DiskVolume, POPUP)],
                Some(b.clone()),
            );
        });
        view.read_with(cx, |f, _| {
            assert_eq!(f.stack.restore_focus(OverlayId::AppsMenu), Some(a.clone()));
            assert_eq!(
                f.stack.restore_focus(OverlayId::DiskVolume),
                Some(b.clone())
            );
            assert_eq!(f.stack.restore_focus(OverlayId::Confirm), None);
        });
    }

    #[gpui::test]
    fn closed_overlay_does_not_restore_an_input_from_the_previous_page(cx: &mut TestAppContext) {
        let (root, cx) = cx.add_window_view(|_, cx| {
            let mut root = Root::new(cx);
            root.show_fda_onboarding = false;
            root
        });
        root.update(cx, |this, cx| {
            this.view = View::Apps;
            cx.notify();
        });
        root.update_in(cx, |this, window, cx| {
            this.apps.input.focus_handle.focus(window);
            cx.notify();
        });
        root.update_in(cx, |this, window, cx| {
            assert!(
                this.apps.input.focus_handle.is_focused(window),
                "fixture must begin with the old input focused"
            );
            this.overlays.sync(
                &[(OverlayId::AppsMenu, POPUP)],
                Some(this.apps.input.focus_handle.clone()),
            );
            this.view = View::Search;
            this.root_focus.focus(window);
            cx.notify();
        });
        root.update_in(cx, |this, window, _| {
            assert!(this.root_focus.is_focused(window));
            assert!(
                !this.apps.input.focus_handle.is_focused(window),
                "removed page input must not regain focus"
            );
        });
    }

    #[gpui::test]
    fn escape_closes_only_the_top_popup_and_modal_swallows(cx: &mut TestAppContext) {
        let (view, cx) = cx.add_window_view(Fixture::new);
        let a = view.read_with(cx, |f, _| f.a.clone());
        cx.update(|window, _| a.focus(window));
        view.update(cx, |f, _| {
            f.stack.sync(
                &[
                    (OverlayId::Confirm, MODAL),
                    (OverlayId::AppsMenu, POPUP),
                    (OverlayId::DiskVolume, POPUP),
                ],
                None,
            );
        });

        cx.simulate_keystrokes("escape");
        view.read_with(cx, |f, _| {
            assert_eq!(f.closed, [OverlayId::DiskVolume]);
            assert_eq!(f.stack.entries().len(), 2);
        });
        cx.simulate_keystrokes("escape");
        view.read_with(cx, |f, _| {
            assert_eq!(f.closed, [OverlayId::DiskVolume, OverlayId::AppsMenu]);
        });
        // 只剩模态：Esc 被吞，什么都不关。
        cx.simulate_keystrokes("escape");
        view.read_with(cx, |f, _| {
            assert_eq!(f.closed.len(), 2);
            assert_eq!(ids(&f.stack), [OverlayId::Confirm]);
        });
    }
}
