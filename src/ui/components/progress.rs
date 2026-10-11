//! 进度条与扫描动画控件

use crate::ui::theme::*;
use gpui::{div, prelude::*, px, rgb, Animation, AnimationExt as _, Div, SharedString};
use std::time::Duration;

/// 顶部扫描进度指示条（现代无缝流光指示条）
pub fn render_scan_line() -> Div {
    div()
        .flex_none()
        .w_full()
        .h(px(3.))
        .bg(rgb(SURF_HIGHEST))
        .relative()
        .overflow_hidden()
        .child(
            div()
                .absolute()
                .top_0()
                .bottom_0()
                .rounded_full()
                .bg(rgb(PRIMARY))
                .with_animation(
                    SharedString::from("scan-progress"),
                    Animation::new(Duration::from_millis(1600)).repeat(),
                    |bar, delta| {
                        let phase = delta * std::f32::consts::FRAC_PI_2;
                        let head = phase.sin();
                        let tail = 1.0 - phase.cos();
                        let span = 1.36;
                        let margin = 0.18;
                        let x_head = -margin + span * head;
                        let x_tail = -margin + span * tail;
                        let left = x_tail;
                        let width = (x_head - x_tail).max(0.01);

                        bar.left(gpui::relative(left)).w(gpui::relative(width))
                    },
                ),
        )
}
