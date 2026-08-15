//! 多段圆环图控件

use gpui::{prelude::*, px, rgb, AnyElement};

#[derive(Clone, Debug)]
pub struct DonutSegment {
    pub ratio: f32,
    pub color: u32,
}

/// 绘制高精度的多段圆环图（Donut Chart），支持各分类与空闲空间比例
pub fn render_donut(segments: Vec<DonutSegment>, size_px: f32, thickness_px: f32) -> AnyElement {
    gpui::canvas(
        |_bounds, _window, _cx| {},
        move |bounds, _state, window, _cx| {
            let center_x = bounds.origin.x + px(size_px / 2.0);
            let center_y = bounds.origin.y + px(size_px / 2.0);
            let r_out = size_px / 2.0;
            let r_in = r_out - thickness_px;

            let point_on_ring = |radius: f32, angle: f32| {
                gpui::point(
                    center_x + px(radius * angle.cos()),
                    center_y + px(radius * angle.sin()),
                )
            };

            // 1. 先绘制底色轨道圆环 (平滑浅灰背景)。
            // gpui::Path::line_to 是三角扇，不适合带内孔的凹多边形；
            // PathBuilder 会用真实多边形填充，避免圆环接缝处缺块。
            let full_steps = 64;
            let mut outer_points = Vec::with_capacity(full_steps);
            let mut inner_points = Vec::with_capacity(full_steps);
            for i in 0..full_steps {
                let a = std::f32::consts::TAU * (i as f32 / full_steps as f32);
                outer_points.push(point_on_ring(r_out, a));
                inner_points.push(point_on_ring(r_in, a));
            }
            inner_points.reverse();

            let mut base_builder = gpui::PathBuilder::fill();
            base_builder.add_polygon(&outer_points, true);
            base_builder.add_polygon(&inner_points, true);
            if let Ok(base_path) = base_builder.build() {
                window.paint_path(base_path, rgb(0xe2e8f0));
            }

            // 2. 依次绘制各个占比分段扇区。
            // 某些文件系统统计值可能因硬链接或扫描边界略微超过总容量，
            // 先把整组比例压回一个圆周，避免最后一段绕过 12 点后覆盖前面的颜色。
            let ratio_sum: f32 = segments.iter().map(|seg| seg.ratio.max(0.0)).sum();
            let ratio_scale = if ratio_sum > 1.0 {
                1.0 / ratio_sum
            } else {
                1.0
            };
            let mut cur_angle = -std::f32::consts::FRAC_PI_2; // 从 12 点钟方向起始

            for seg in &segments {
                let ratio = seg.ratio.max(0.0) * ratio_scale;
                let span = ratio * std::f32::consts::TAU;
                let segment_start = cur_angle;
                let segment_end = cur_angle + span;
                cur_angle = segment_end;

                if ratio <= 0.002 || segment_end <= segment_start {
                    continue;
                }

                // 相邻多边形共享边界经过抗锯齿后可能露出约 1px 底色；
                // 现在已是真实多边形填充，只保留亚像素级重叠遮住接缝，不会再出现大面积缺块。
                let seam_overlap = (0.75 / r_out).min(0.01);
                let a0 = segment_start - seam_overlap;
                let a1 = segment_end + seam_overlap;
                let draw_span = a1 - a0;

                let seg_steps =
                    ((draw_span / std::f32::consts::TAU) * 64.0).ceil().max(4.0) as usize;
                let mut points = Vec::with_capacity((seg_steps + 1) * 2);
                for i in 0..=seg_steps {
                    let a = a0 + draw_span * (i as f32 / seg_steps as f32);
                    points.push(point_on_ring(r_out, a));
                }
                for i in (0..=seg_steps).rev() {
                    let a = a0 + draw_span * (i as f32 / seg_steps as f32);
                    points.push(point_on_ring(r_in, a));
                }

                let mut builder = gpui::PathBuilder::fill();
                builder.add_polygon(&points, true);
                if let Ok(path) = builder.build() {
                    window.paint_path(path, rgb(seg.color));
                }
            }
        },
    )
    .into_any_element()
}
