//! 矢量图标绘制（纯绘制辅助，与节点体系无关）。

use wy_mve::{Icon, Scene};
use wy_render::{Color, Point};

/// 在 `(ox, oy)` 处画一个 `side` 边长的矢量图标。
///
/// 图标是**路径集合**而不是字体字形：`filled` 的走填充，其余按线宽描边。
/// 因为不依赖字体，所以任何主题 / 缩放都画得出来。
pub fn draw_icon_at(s: &mut Scene, ic: Icon, ox: f32, oy: f32, side: f32, color: Color) {
    let (paths, filled) = ic.icon_paths();
    for seg in paths {
        let pts: Vec<Point> = seg
            .chunks(2)
            .map(|c| Point::new(ox + c[0] * side, oy + c[1] * side))
            .collect();
        if filled {
            s.fill_path(pts, color);
        } else {
            s.stroke_path(pts, (side * 0.09).max(1.0), color);
        }
    }
}
