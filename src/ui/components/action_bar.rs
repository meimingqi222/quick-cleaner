//! 底部操作条外壳。按钮、汇总文案、开关都由调用方放进插槽。
//!
//! 两种布局：
//! - `capsule`：浮动胶囊，插槽按 summary、leading、actions 顺序作为胶囊的直接子项，
//!   间距由胶囊的 gap 决定，所以调用方不要再自己包一层 flex。
//! - `strip`：贴底整条，summary 在左，leading + actions 收进右侧一组。

use crate::ui::theme::{rgba, BG, CARD, OUTLINE_VAR};
use gpui::{div, prelude::*, px, rgb, AnyElement, App, ElementId, IntoElement, RenderOnce, Window};

/// 胶囊的尺寸预设。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CapsulePreset {
    /// 无外层底栏，直接放在页面里。
    Inline,
    /// 带 `BG` 底栏的标准尺寸。
    #[default]
    Docked,
    /// 带 `BG` 底栏的宽松尺寸。
    DockedRoomy,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Layout {
    Capsule(CapsulePreset),
    Strip,
}

#[derive(IntoElement)]
pub struct ActionBar {
    id: ElementId,
    layout: Layout,
    summary: Vec<AnyElement>,
    leading: Vec<AnyElement>,
    actions: Vec<AnyElement>,
}

impl ActionBar {
    fn new(id: impl Into<ElementId>, layout: Layout) -> Self {
        Self {
            id: id.into(),
            layout,
            summary: Vec::new(),
            leading: Vec::new(),
            actions: Vec::new(),
        }
    }

    pub fn capsule(id: impl Into<ElementId>, preset: CapsulePreset) -> Self {
        Self::new(id, Layout::Capsule(preset))
    }

    pub fn strip(id: impl Into<ElementId>) -> Self {
        Self::new(id, Layout::Strip)
    }

    /// 左侧汇总。可多次调用，每次追加一个子项。
    pub fn summary(mut self, el: impl IntoElement) -> Self {
        self.summary.push(el.into_any_element());
        self
    }

    /// 可选的附加动作（批量选择、处置开关、「取消全选」等）。
    pub fn leading(mut self, el: impl IntoElement) -> Self {
        self.leading.push(el.into_any_element());
        self
    }

    /// 主按钮。
    pub fn actions(mut self, el: impl IntoElement) -> Self {
        self.actions.push(el.into_any_element());
        self
    }
}

impl RenderOnce for ActionBar {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let Self {
            id,
            layout,
            summary,
            leading,
            actions,
        } = self;
        match layout {
            Layout::Strip => div()
                .id(id)
                .h(px(70.))
                .flex_none()
                .px_8()
                .bg(rgb(CARD))
                .border_t_1()
                .border_color(rgba(OUTLINE_VAR, 0.35))
                .shadow_md()
                .flex()
                .items_center()
                .justify_between()
                .child(div().children(summary))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_3()
                        .children(leading)
                        .children(actions),
                ),
            Layout::Capsule(preset) => {
                let pill = div()
                    .flex()
                    .items_center()
                    .rounded_full()
                    .bg(rgb(CARD))
                    .border_1()
                    .border_color(rgba(OUTLINE_VAR, 0.6));
                let pill = match preset {
                    CapsulePreset::Inline => pill.gap_4().px_4().py_2().shadow_lg(),
                    CapsulePreset::Docked => pill.gap_5().pl_5().pr(px(5.)).py(px(4.)).shadow_lg(),
                    CapsulePreset::DockedRoomy => {
                        pill.gap_6().pl_6().pr(px(6.)).py(px(6.)).shadow_xl()
                    }
                }
                .children(summary)
                .children(leading)
                .children(actions);

                let outer = div().id(id).flex().justify_center();
                match preset {
                    CapsulePreset::Inline => outer.items_center().child(pill),
                    CapsulePreset::Docked => outer
                        .flex_none()
                        .w_full()
                        .px_5()
                        .py_2p5()
                        .bg(rgb(BG))
                        .border_t_1()
                        .border_color(rgba(OUTLINE_VAR, 0.5))
                        .child(pill),
                    CapsulePreset::DockedRoomy => outer
                        .flex_none()
                        .w_full()
                        .px_8()
                        .py_2()
                        .bg(rgb(BG))
                        .border_t_1()
                        .border_color(rgba(OUTLINE_VAR, 0.5))
                        .child(pill),
                }
            }
        }
    }
}
