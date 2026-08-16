//! 自绘垂直滚动条
//!
//! GPUI 的 `overflow_scroll` 只负责滚动行为，不画滚动条，所以长列表在
//! 视觉上完全看不出自己还能往下滚。这里把「从 `ScrollHandle` 推算滑块
//! 几何」这段容易写错的算术集中到一处，各列表复用同一套度量。
//!
//! 拖拽状态仍由各调用方存在 `Root` 上（每个列表要独立记录），本模块只
//! 提供度量、滑块外观，以及拖拽期间的窗口级事件捕获。

use crate::ui::theme::*;
use gpui::{
    canvas, div, prelude::*, px, rgb, Context, DispatchPhase, Div, Entity, IntoElement,
    MouseButton, MouseMoveEvent, MouseUpEvent, ScrollHandle, SharedString, Stateful,
};
use std::rc::Rc;

/// 滑块最小高度：再短就抓不住了。
const MIN_THUMB_H: f32 = 28.0;
/// 滚动条轨道宽度。
pub const SCROLLBAR_W: f32 = 12.0;

/// 一次布局下滚动条的几何。
#[derive(Clone, Copy, Debug)]
pub struct ScrollMetrics {
    /// 轨道高度（= 视口高度）
    pub track_h: f32,
    /// 滑块高度
    pub thumb_h: f32,
    /// 滑块顶端相对轨道的偏移
    pub thumb_top: f32,
    /// 最大可滚动距离
    pub max_offset: f32,
}

/// 由 `ScrollHandle` 推算滚动条几何；内容不溢出时返回 `None`。
///
/// `est_viewport` / `est_content` 用于首帧：布局跑完之前 `ScrollHandle`
/// 的 bounds 和 max_offset 都还是 0，只靠真实值会导致「内容明显溢出但
/// 滚动条要等一帧才出现」的闪烁。估算值只在真实值缺席时顶上。
pub fn scroll_metrics(
    handle: &ScrollHandle,
    est_viewport: f32,
    est_content: f32,
) -> Option<ScrollMetrics> {
    let viewport_h: f32 = handle.bounds().size.height.into();
    let max_scroll_y: f32 = handle.max_offset().height.into();
    let scroll_top: f32 = (-handle.offset().y).into();

    let viewport_eff = if viewport_h > 0.0 {
        viewport_h
    } else {
        est_viewport
    };
    let max_eff = if max_scroll_y > 0.0 {
        max_scroll_y
    } else {
        (est_content - est_viewport).max(0.0)
    };

    if max_eff <= 0.0 || viewport_eff <= 0.0 {
        return None;
    }

    let track_h = viewport_eff;
    let content_h = viewport_eff + max_eff;
    let thumb_h = ((viewport_eff / content_h) * track_h).clamp(MIN_THUMB_H, track_h);
    let thumb_top = (scroll_top.max(0.0) / max_eff) * (track_h - thumb_h);

    Some(ScrollMetrics {
        track_h,
        thumb_h,
        thumb_top: thumb_top.clamp(0.0, (track_h - thumb_h).max(0.0)),
        max_offset: max_eff,
    })
}

/// 拖拽滑块时的新滚动位置。
///
/// `start` 是按下时记录的 (鼠标 y, 当时的滚动偏移)，`mouse_y` 是当前位置。
/// 滑块行程比内容短，所以要按 `max_offset / travel` 放大位移。
pub fn drag_to_offset(handle: &ScrollHandle, start: (f32, f32), mouse_y: f32) -> Option<f32> {
    let viewport_h: f32 = handle.bounds().size.height.into();
    let max_scroll_y: f32 = handle.max_offset().height.into();
    if viewport_h <= 0.0 || max_scroll_y <= 0.0 {
        return None;
    }

    let content_h = viewport_h + max_scroll_y;
    let thumb_h = ((viewport_h / content_h) * viewport_h).clamp(MIN_THUMB_H, viewport_h);
    let travel = (viewport_h - thumb_h).max(1.0);
    let (start_mouse_y, start_scroll_top) = start;

    Some(
        (start_scroll_top + (mouse_y - start_mouse_y) / travel * max_scroll_y)
            .clamp(0.0, max_scroll_y),
    )
}

/// 滚动条轨道 + 滑块。调用方把 `on_thumb_down` 接到自己的拖拽状态上。
///
/// 需要放进一个 `relative()` 容器里，覆盖在滚动区域之上。
pub fn scrollbar(
    thumb_id: impl Into<SharedString>,
    m: ScrollMetrics,
    on_thumb_down: impl FnOnce(Stateful<Div>) -> Stateful<Div>,
) -> Div {
    div()
        .absolute()
        .top(px(0.))
        .right(px(0.))
        .bottom(px(0.))
        .w(px(SCROLLBAR_W))
        .bg(rgba(OUTLINE_VAR, 0.14))
        .child(on_thumb_down(
            div()
                .id(thumb_id.into())
                .absolute()
                .right(px(2.))
                .top(px(m.thumb_top))
                .w(px(8.))
                .h(px(m.thumb_h))
                .rounded_full()
                .bg(rgb(OUTLINE))
                .opacity(0.9)
                .cursor_pointer(),
        ))
}

/// 把滑块拖拽的 move / up 注册成**窗口级**监听，返回一个零尺寸元素。
///
/// GPUI 的 `div().on_mouse_move` 只在鼠标命中该元素时才会触发。拖滚动条
/// 的时候鼠标很容易滑出列表——往右一点就出了卡片边界——事件随即断流，
/// 表现就是「拖一段卡住，鼠标绕回来又猛地跳一截」。挂到哪个祖先容器上
/// 都躲不掉这个问题，只是边界远近的差别。
///
/// 这里借 `canvas` 拿到 paint 阶段，把监听直接挂在窗口上，不再经过命中
/// 测试。鼠标松开的事件万一丢了（比如在窗口外抬起），下一次不带左键的
/// 移动也会把拖拽收尾，不会卡在「一直跟着鼠标」的状态。
///
/// 返回值绝对定位且尺寸为 0，塞进任意容器都不影响布局。
pub fn drag_capture<T: 'static>(
    entity: Entity<T>,
    on_move: impl Fn(&mut T, f32, &mut Context<T>) + 'static,
    on_up: impl Fn(&mut T, &mut Context<T>) + 'static,
) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        move |_, _, window, _| {
            let on_up = Rc::new(on_up);
            let (move_entity, move_on_up) = (entity.clone(), on_up.clone());

            window.on_mouse_event(move |event: &MouseMoveEvent, phase, _window, cx| {
                if phase != DispatchPhase::Bubble {
                    return;
                }
                if event.pressed_button == Some(MouseButton::Left) {
                    let mouse_y: f32 = event.position.y.into();
                    move_entity.update(cx, |this, cx| on_move(this, mouse_y, cx));
                } else {
                    // 左键已经不在按下状态，说明抬起事件没送到，就地收尾。
                    move_entity.update(cx, |this, cx| move_on_up(this, cx));
                }
            });

            window.on_mouse_event(move |event: &MouseUpEvent, phase, _window, cx| {
                if phase == DispatchPhase::Bubble && event.button == MouseButton::Left {
                    entity.update(cx, |this, cx| on_up(this, cx));
                }
            });
        },
    )
    .absolute()
    .size(px(0.))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 布局尚未跑过时用估算值，保证滚动条首帧就能出现。
    #[test]
    fn falls_back_to_estimates_before_first_layout() {
        let handle = ScrollHandle::new();
        let m = scroll_metrics(&handle, 340.0, 340.0 * 4.0).expect("内容溢出时应有滚动条");
        assert_eq!(m.track_h, 340.0);
        assert_eq!(m.max_offset, 340.0 * 3.0);
        // 视口占内容 1/4，滑块也该是轨道的 1/4
        assert!((m.thumb_h - 85.0).abs() < 0.5);
    }

    #[test]
    fn no_scrollbar_when_content_fits() {
        let handle = ScrollHandle::new();
        assert!(scroll_metrics(&handle, 340.0, 200.0).is_none());
    }

    #[test]
    fn thumb_never_shrinks_below_grabbable_size() {
        let handle = ScrollHandle::new();
        // 5 万像素内容配 300 像素视口，等比算出来的滑块不到 2px
        let m = scroll_metrics(&handle, 300.0, 50_000.0).unwrap();
        assert!(m.thumb_h >= MIN_THUMB_H);
        assert!(m.thumb_top + m.thumb_h <= m.track_h + 0.01);
    }

    #[test]
    fn drag_needs_real_layout() {
        let handle = ScrollHandle::new();
        // 没有真实布局就没法换算行程，必须返回 None 而不是瞎猜
        assert!(drag_to_offset(&handle, (0.0, 0.0), 50.0).is_none());
    }
}
