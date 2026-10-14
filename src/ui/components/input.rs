//! Host-bound text input: state ownership is supplied by each adapter.
use gpui::{Context, KeyDownEvent, Window};

pub trait InputHost<T: 'static> {
    fn input<'a>(&self, owner: &'a T) -> &'a TextInputState;
    fn input_mut<'a>(&self, owner: &'a mut T) -> &'a mut TextInputState;
    fn changed(&self, owner: &mut T, cx: &mut Context<T>);
    fn activity(&self, owner: &mut T, cx: &mut Context<T>);
}

pub struct InputBinding<T: 'static> {
    pub read: fn(&T) -> &TextInputState,
    pub write: fn(&mut T) -> &mut TextInputState,
    pub on_change: fn(&mut T, &mut Context<T>),
    pub on_activity: fn(&mut T, &mut Context<T>),
    pub on_key: fn(&mut T, &KeyDownEvent, &mut Window, &mut Context<T>) -> bool,
    pub register: fn(&mut T, InputBinding<T>, &mut Window),
    pub font_size: f32,
}
impl<T: 'static> Copy for InputBinding<T> {}
impl<T: 'static> Clone for InputBinding<T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T: 'static> InputBinding<T> {
    pub fn focused(self, owner: &T, window: &Window) -> Option<Self> {
        self.input(owner)
            .focus_handle
            .is_focused(window)
            .then_some(self)
    }

    pub fn commit(
        self,
        owner: &mut T,
        range: Option<&std::ops::Range<usize>>,
        text: &str,
        cx: &mut Context<T>,
    ) {
        self.input_mut(owner).replace(range, text);
        self.changed(owner, cx);
        self.activity(owner, cx);
    }

    pub fn compose(
        self,
        owner: &mut T,
        range: Option<&std::ops::Range<usize>>,
        text: &str,
        selected: Option<&std::ops::Range<usize>>,
        cx: &mut Context<T>,
    ) {
        self.input_mut(owner)
            .replace_and_mark(range, text, selected);
        if self.input(owner).marked.is_some() {
            cx.notify();
        } else {
            self.changed(owner, cx);
        }
        self.activity(owner, cx);
    }
}
impl<T: 'static> InputHost<T> for InputBinding<T> {
    fn input<'a>(&self, owner: &'a T) -> &'a TextInputState {
        (self.read)(owner)
    }
    fn input_mut<'a>(&self, owner: &'a mut T) -> &'a mut TextInputState {
        (self.write)(owner)
    }
    fn changed(&self, owner: &mut T, cx: &mut Context<T>) {
        (self.on_change)(owner, cx);
    }
    fn activity(&self, owner: &mut T, cx: &mut Context<T>) {
        (self.on_activity)(owner, cx);
    }
}
pub struct SearchTextHit {
    pub bounds: gpui::Bounds<gpui::Pixels>,
    pub line: gpui::ShapedLine,
}

/// 一个单行文本输入框的全部状态。
///
/// Apps 搜索框和文件搜索框本来各带一套完全相同的字段，`text_input.rs`
/// 的每个方法、`search_box.rs` 的每个回调都要写一遍
/// `if is_file_search { …search… } else { …apps… }`——两份实现已经开始
///各自漂移。现在两边共用这一个结构体，分支只剩「拿哪一个」。
pub struct TextInputState {
    pub text: String,
    /// 有向字节端点 anchor..caret，允许 start > end；切片/绘制用 selection()。
    pub sel: std::ops::Range<usize>,
    /// 输入法正在组合中的那段文本的字节范围（拼音串，尚未确认）
    pub marked: Option<std::ops::Range<usize>>,
    /// 输入框最近一次绘制的位置，用来定位输入法候选窗口
    pub bounds: Option<gpui::Bounds<gpui::Pixels>>,
    /// 文本区真实排版（绘制与点击命中共用）
    pub text_hit: Option<SearchTextHit>,
    /// 文本拖拽选区的锚点字节偏移（鼠标按下时的位置）
    pub text_drag: Option<usize>,
    pub focus_handle: gpui::FocusHandle,
}

impl TextInputState {
    pub fn new(focus_handle: gpui::FocusHandle) -> Self {
        Self {
            text: String::new(),
            sel: 0..0,
            marked: None,
            bounds: None,
            text_hit: None,
            text_drag: None,
            focus_handle,
        }
    }

    /// 光标（或选区）的字节范围，已钳到合法的字符边界。
    pub fn selection(&self) -> std::ops::Range<usize> {
        crate::ui::text_input::clamp_to_boundary(&self.text, self.sel.clone())
    }

    pub fn utf16_selection(&self) -> gpui::UTF16Selection {
        gpui::UTF16Selection {
            range: crate::ui::text_input::range_to_utf16(&self.text, &self.selection()),
            reversed: self.sel.start > self.sel.end,
        }
    }

    /// 把光标收到末尾。内容被外部改动（清空按钮、切换筛选）后要调一次，
    /// 否则残留的旧偏移会指到字符串外面。
    pub fn reset_caret(&mut self) {
        let end = self.text.len();
        self.sel = end..end;
        self.marked = None;
    }

    pub fn clear(&mut self) {
        self.text.clear();
        self.reset_caret();
    }

    pub fn set_text(&mut self, text: &str) {
        self.text = text.to_string();
        self.reset_caret();
    }

    /// 提交文本：普通打字与输入法确认后的汉字都走这里。
    pub fn replace(&mut self, range_utf16: Option<&std::ops::Range<usize>>, new_text: &str) {
        let (sel, marked) = crate::ui::text_input::replace_text(
            &mut self.text,
            &self.sel,
            &self.marked,
            range_utf16,
            new_text,
        );
        self.sel = sel;
        self.marked = marked;
    }

    /// 组合中的文本：输入法候选阶段的拼音串，尚未确认。
    pub fn replace_and_mark(
        &mut self,
        range_utf16: Option<&std::ops::Range<usize>>,
        new_text: &str,
        new_selected_range_utf16: Option<&std::ops::Range<usize>>,
    ) {
        let (sel, marked) = crate::ui::text_input::replace_and_mark_text(
            &mut self.text,
            &self.sel,
            &self.marked,
            range_utf16,
            new_text,
            new_selected_range_utf16,
        );
        self.sel = sel;
        self.marked = marked;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{div, prelude::*, Render, TestAppContext};

    struct Host {
        a: TextInputState,
        b: TextInputState,
        changes: usize,
    }
    const A: InputBinding<Host> = InputBinding {
        read: |h| &h.a,
        write: |h| &mut h.a,
        on_change: |h, cx| {
            h.changes += 1;
            cx.notify();
        },
        on_activity: |_, _| {},
        on_key: |_, _, _, _| false,
        register: |_, _, _| {},
        font_size: 12.,
    };
    const B: InputBinding<Host> = InputBinding {
        read: |h| &h.b,
        write: |h| &mut h.b,
        ..A
    };
    impl Render for Host {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
                .child(div().track_focus(&self.a.focus_handle))
                .child(div().track_focus(&self.b.focus_handle))
        }
    }

    #[gpui::test]
    fn directed_selection_reports_utf16_and_replaces_exact_text(cx: &mut TestAppContext) {
        let (view, cx) = cx.add_window_view(|_, cx| Host {
            a: TextInputState::new(cx.focus_handle()),
            b: TextInputState::new(cx.focus_handle()),
            changes: 0,
        });
        view.update(cx, |h, cx| {
            h.a.set_text("a中😀b");
            h.a.sel = std::ops::Range { start: 8, end: 1 };
            assert_eq!(h.a.selection(), 1..8);
            let selected = h.a.utf16_selection();
            assert_eq!(selected.range, 1..4);
            assert!(selected.reversed);
            A.compose(h, None, "zhong", Some(&(5..5)), cx);
            assert_eq!(h.a.text, "azhongb");
            assert_eq!(h.a.marked, Some(1..6));
            A.commit(h, None, "中", cx);
            assert_eq!(h.a.text, "a中b");
            assert_eq!(h.a.sel, 4..4);
            assert!(!h.a.utf16_selection().reversed);
            assert!(h.a.marked.is_none());
            h.a.clear();
            assert_eq!(h.a.sel, 0..0);
        });
    }

    #[gpui::test]
    fn bindings_isolate_fields_and_commit_composition_once(cx: &mut TestAppContext) {
        let (view, cx) = cx.add_window_view(|_, cx| Host {
            a: TextInputState::new(cx.focus_handle()),
            b: TextInputState::new(cx.focus_handle()),
            changes: 0,
        });
        view.update(cx, |h, cx| {
            A.input_mut(h).set_text("🙂");
            assert_eq!(h.changes, 0, "programmatic assignment must not recurse");
            A.compose(h, None, "zhong", Some(&(5..5)), cx);
            assert_eq!(h.changes, 0, "preedit must not execute filtering");
            A.commit(h, None, "中", cx);
            assert_eq!(h.a.text, "🙂中");
            assert_eq!(h.changes, 1);
            assert!(h.a.marked.is_none());
            B.commit(h, None, "文件", cx);
            assert_eq!(h.b.text, "文件");
            assert_eq!(h.a.text, "🙂中");
            assert_eq!(h.changes, 2);
        });
        let a = view.read_with(cx, |h, _| h.a.focus_handle.clone());
        cx.update(|window, _| a.focus(window));
        view.update_in(cx, |h, window, _| {
            assert!(A.focused(h, window).is_some());
            assert!(B.focused(h, window).is_none());
            window.blur();
            assert!(
                A.focused(h, window).is_none(),
                "inactive binding must not accept platform edits"
            );
        });
    }
}
