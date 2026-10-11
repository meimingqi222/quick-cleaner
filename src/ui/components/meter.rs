//! 进度条：`Meter`（单值）与 `ProportionBar`（多段占比）。
//!
//! 不定进度的流光扫描线（`progress::render_scan_line`）带动画，不在此列。

use crate::ui::theme::{SURF_HIGH, SURF_HIGHEST};
use gpui::{div, prelude::*, px, relative, rgb, App, IntoElement, Pixels, RenderOnce, Window};

/// 比例钳到 [0, 1]，NaN 当 0。
pub fn clamp_ratio(ratio: f32) -> f32 {
    if ratio.is_nan() {
        0.0
    } else {
        ratio.clamp(0.0, 1.0)
    }
}

/// 条高。数值就是 px，名字只是为了不在调用点写魔数。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MeterHeight {
    /// 3px：进程行 CPU 迷你条
    Hairline,
    /// 4px
    Slim,
    /// 6px：状态卡片、卸载进度
    #[default]
    Regular,
    /// 8px：清理进度
    Thick,
    /// 10px：磁盘占比条
    Heavy,
}

impl MeterHeight {
    pub fn px(self) -> f32 {
        match self {
            MeterHeight::Hairline => 3.,
            MeterHeight::Slim => 4.,
            MeterHeight::Regular => 6.,
            MeterHeight::Thick => 8.,
            MeterHeight::Heavy => 10.,
        }
    }
}

/// 轨道底色。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MeterTrack {
    #[default]
    Highest,
    High,
}

impl MeterTrack {
    fn color(self) -> u32 {
        match self {
            MeterTrack::Highest => SURF_HIGHEST,
            MeterTrack::High => SURF_HIGH,
        }
    }
}

/// 轨道圆角。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MeterShape {
    #[default]
    Pill,
    Flat,
}

#[derive(IntoElement)]
pub struct Meter {
    ratio: f32,
    color: u32,
    height: MeterHeight,
    track: MeterTrack,
    shape: MeterShape,
    width: Option<Pixels>,
    min_fill: Option<Pixels>,
}

impl Meter {
    /// `color` 取 theme 常量或调用方算出的状态色。
    pub fn new(ratio: f32, color: u32) -> Self {
        Self {
            ratio: clamp_ratio(ratio),
            color,
            height: MeterHeight::Regular,
            track: MeterTrack::Highest,
            shape: MeterShape::Pill,
            width: None,
            min_fill: None,
        }
    }

    pub fn height(mut self, h: MeterHeight) -> Self {
        self.height = h;
        self
    }

    pub fn track(mut self, t: MeterTrack) -> Self {
        self.track = t;
        self
    }

    pub fn shape(mut self, s: MeterShape) -> Self {
        self.shape = s;
        self
    }

    /// 固定轨道宽度；不设则 `w_full`。
    pub fn width(mut self, w: Pixels) -> Self {
        self.width = Some(w);
        self
    }

    /// 填充最小宽度，让很小的进度仍可见（清理进度条用 2px）。
    pub fn min_fill(mut self, w: Pixels) -> Self {
        self.min_fill = Some(w);
        self
    }
}

impl RenderOnce for Meter {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let pill = self.shape == MeterShape::Pill;
        let mut fill = div()
            .h_full()
            .bg(rgb(self.color))
            .w(relative(self.ratio))
            .when(pill, |d| d.rounded_full());
        if let Some(min) = self.min_fill {
            fill = fill.min_w(min);
        }
        div()
            .when_some(self.width, |d, w| d.w(w))
            .when(self.width.is_none(), |d| d.w_full())
            .h(px(self.height.px()))
            .bg(rgb(self.track.color()))
            .overflow_hidden()
            .when(pill, |d| d.rounded_full())
            .child(fill)
    }
}

/// 一段占比：`ratio` 是占整条的比例，`color` 是该段颜色。
#[derive(Clone, Copy, Debug)]
pub struct Segment {
    pub ratio: f32,
    pub color: u32,
}

/// 多段占比条。比例不超过 `MIN_VISIBLE` 的段不画。
#[derive(IntoElement)]
pub struct ProportionBar {
    segments: Vec<Segment>,
    height: MeterHeight,
    track: MeterTrack,
}

/// 0.5%：再小的段画出来不足一个像素，只会让轨道看起来被污染。
pub const MIN_VISIBLE: f32 = 0.005;

pub fn visible_segment(ratio: f32) -> bool {
    clamp_ratio(ratio) > MIN_VISIBLE
}

impl ProportionBar {
    pub fn new(segments: impl IntoIterator<Item = Segment>) -> Self {
        Self {
            segments: segments.into_iter().collect(),
            height: MeterHeight::Heavy,
            track: MeterTrack::High,
        }
    }

    pub fn height(mut self, h: MeterHeight) -> Self {
        self.height = h;
        self
    }

    pub fn track(mut self, t: MeterTrack) -> Self {
        self.track = t;
        self
    }
}

impl RenderOnce for ProportionBar {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        div()
            .w_full()
            .h(px(self.height.px()))
            .rounded_full()
            .overflow_hidden()
            .bg(rgb(self.track.color()))
            .flex()
            .children(
                self.segments
                    .into_iter()
                    .filter(|s| visible_segment(s.ratio))
                    .map(|s| {
                        div()
                            .h_full()
                            .flex_none()
                            .w(relative(clamp_ratio(s.ratio)))
                            .bg(rgb(s.color))
                    }),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ratio_is_clamped_and_nan_is_zero() {
        assert_eq!(clamp_ratio(-0.5), 0.0);
        assert_eq!(clamp_ratio(0.25), 0.25);
        assert_eq!(clamp_ratio(7.0), 1.0);
        assert_eq!(clamp_ratio(f32::NAN), 0.0);
        assert_eq!(clamp_ratio(f32::INFINITY), 1.0);
        assert_eq!(clamp_ratio(f32::NEG_INFINITY), 0.0);
    }

    #[test]
    fn meter_stores_the_clamped_ratio() {
        assert_eq!(Meter::new(f32::NAN, 0).ratio, 0.0);
        assert_eq!(Meter::new(3.0, 0).ratio, 1.0);
    }

    #[test]
    fn tiny_segments_are_not_drawn() {
        assert!(!visible_segment(0.005));
        assert!(!visible_segment(f32::NAN));
        assert!(visible_segment(0.0051));
    }
}
