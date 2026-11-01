//! 几何基元（对标 TS `canvasRender2/Geometry.ts`）：二维点、轴对齐矩形与可录制路径。
//!
//! [`Path2`] 与 TS 同构：命令序列录制 + 回放（曲线 `flatten` 成折线后交给
//! [`Scene`] 的 `fill_path` / `stroke_path`）+ 纯几何偶数规则命中（`contains`）。
//! TS 走 Canvas 2D 的 `beginPath` 录制、`isPointInPath` 判定；Rust 没有等价平台
//! 路径对象，所以录制/回放/命中全在自己手里（与 TS 的纯 JS 实现思路一致）。

use wy_render::{Color, Point, Rect, Scene};

use crate::NodeRef;

/// 二维点（对标 TS `PointF`）。
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PointF {
    pub x: f32,
    pub y: f32,
}

/// 路径命令（对标 TS `PathCommand2`）。
#[derive(Clone, Copy, Debug)]
pub enum PathCommand2 {
    MoveTo {
        x: f32,
        y: f32,
    },
    LineTo {
        x: f32,
        y: f32,
    },
    QuadTo {
        cx: f32,
        cy: f32,
        x: f32,
        y: f32,
    },
    CubicTo {
        cx1: f32,
        cy1: f32,
        cx2: f32,
        cy2: f32,
        x: f32,
        y: f32,
    },
    Close,
}

/// 曲线扁平化分段数（二次 / 三次贝塞尔近似成折线的段数）。
const QUAD_SEGMENTS: usize = 12;
const CUBIC_SEGMENTS: usize = 16;

/// 二次贝塞尔插值（t ∈ (0, 1]）。
fn quad_at(p0: PointF, p1: PointF, p2: PointF, t: f32) -> PointF {
    let u = 1.0 - t;
    PointF {
        x: u * u * p0.x + 2.0 * u * t * p1.x + t * t * p2.x,
        y: u * u * p0.y + 2.0 * u * t * p1.y + t * t * p2.y,
    }
}

/// 三次贝塞尔插值（t ∈ (0, 1]）。
fn cubic_at(p0: PointF, p1: PointF, p2: PointF, p3: PointF, t: f32) -> PointF {
    let u = 1.0 - t;
    let uu = u * u;
    let tt = t * t;
    PointF {
        x: uu * u * p0.x + 3.0 * uu * t * p1.x + 3.0 * u * tt * p2.x + tt * t * p3.x,
        y: uu * u * p0.y + 3.0 * uu * t * p1.y + 3.0 * u * tt * p2.y + tt * t * p3.y,
    }
}

/// 可录制路径（对标 TS `Path2`）。
///
/// 录制命令序列（无平台对象，可自由重建/拷贝），回放到 [`Scene`] 绘制，或用
/// [`Path2::contains`] 做纯几何命中判定。`flatten` 把曲线近似为折线——
/// 绘制回放与命中判定共用同一份折线。
#[derive(Clone, Debug, Default)]
pub struct Path2 {
    /// 实际的路径命令序列。
    commands: Vec<PathCommand2>,
}

impl Path2 {
    /// 新建空路径。
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// 空路径，或只有一个 `MoveTo`（无实际绘制内容）。
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.commands.is_empty()
            || (self.commands.len() == 1 && matches!(self.commands[0], PathCommand2::MoveTo { .. }))
    }

    /// 最后一个有效命令的终点（`Close` 不改变当前位置）。
    fn last_position(&self) -> PointF {
        for c in self.commands.iter().rev() {
            match c {
                PathCommand2::MoveTo { x, y }
                | PathCommand2::LineTo { x, y }
                | PathCommand2::QuadTo { x, y, .. }
                | PathCommand2::CubicTo { x, y, .. } => return PointF { x: *x, y: *y },
                PathCommand2::Close => {}
            }
        }
        PointF { x: 0.0, y: 0.0 }
    }

    #[must_use]
    pub fn last_x(&self) -> f32 {
        self.last_position().x
    }

    #[must_use]
    pub fn last_y(&self) -> f32 {
        self.last_position().y
    }

    pub fn move_to(&mut self, x: f32, y: f32) -> &mut Self {
        self.commands.push(PathCommand2::MoveTo { x, y });
        self
    }

    pub fn line_to(&mut self, x: f32, y: f32) -> &mut Self {
        self.commands.push(PathCommand2::LineTo { x, y });
        self
    }

    /// 二次贝塞尔曲线：控制点 `(cx, cy)`，终点 `(x, y)`。
    pub fn quad_to(&mut self, cx: f32, cy: f32, x: f32, y: f32) -> &mut Self {
        self.commands.push(PathCommand2::QuadTo { cx, cy, x, y });
        self
    }

    /// 三次贝塞尔曲线：两个控制点，终点 `(x, y)`。
    pub fn cubic_to(
        &mut self,
        cx1: f32,
        cy1: f32,
        cx2: f32,
        cy2: f32,
        x: f32,
        y: f32,
    ) -> &mut Self {
        self.commands.push(PathCommand2::CubicTo {
            cx1,
            cy1,
            cx2,
            cy2,
            x,
            y,
        });
        self
    }

    pub fn close(&mut self) -> &mut Self {
        self.commands.push(PathCommand2::Close);
        self
    }

    pub fn reset(&mut self) -> &mut Self {
        self.commands.clear();
        self
    }

    /// 扁平化成子路径折线（曲线按固定段数近似；`Close` 隐含回到子路径起点）。
    fn flatten(&self) -> Vec<Vec<PointF>> {
        let mut polygons: Vec<Vec<PointF>> = Vec::new();
        let mut current: Vec<PointF> = Vec::new();
        let mut point = PointF { x: 0.0, y: 0.0 };
        let mut sub_start = PointF { x: 0.0, y: 0.0 };
        for c in &self.commands {
            match c {
                PathCommand2::MoveTo { x, y } => {
                    if !current.is_empty() {
                        polygons.push(std::mem::take(&mut current));
                    }
                    current = vec![PointF { x: *x, y: *y }];
                    point = PointF { x: *x, y: *y };
                    sub_start = point;
                }
                PathCommand2::LineTo { x, y } => {
                    let to = PointF { x: *x, y: *y };
                    current.push(to);
                    point = to;
                }
                PathCommand2::QuadTo { cx, cy, x, y } => {
                    let ctrl = PointF { x: *cx, y: *cy };
                    let to = PointF { x: *x, y: *y };
                    for i in 1..=QUAD_SEGMENTS {
                        current.push(quad_at(point, ctrl, to, i as f32 / QUAD_SEGMENTS as f32));
                    }
                    point = to;
                }
                PathCommand2::CubicTo {
                    cx1,
                    cy1,
                    cx2,
                    cy2,
                    x,
                    y,
                } => {
                    let c1 = PointF { x: *cx1, y: *cy1 };
                    let c2 = PointF { x: *cx2, y: *cy2 };
                    let to = PointF { x: *x, y: *y };
                    for i in 1..=CUBIC_SEGMENTS {
                        current.push(cubic_at(
                            point,
                            c1,
                            c2,
                            to,
                            i as f32 / CUBIC_SEGMENTS as f32,
                        ));
                    }
                    point = to;
                }
                PathCommand2::Close => {
                    point = sub_start;
                }
            }
        }
        if !current.is_empty() {
            polygons.push(current);
        }
        polygons
    }

    /// 点 `(x, y)` 是否落在填充区域内：命令序列扁平化成子路径折线后，按**偶数规则**
    /// 射线判定（对标 TS `Path2.contains` / Kotlin `pathHitTest` 的平台实现）。
    #[must_use]
    pub fn contains(&self, x: f32, y: f32) -> bool {
        let polygons = self.flatten();
        let mut crossings = 0i32;
        for points in &polygons {
            let n = points.len();
            if n < 2 {
                continue;
            }
            let mut j = n - 1;
            for i in 0..n {
                let a = points[i];
                let b = points[j];
                // 半开区间规则：(y1 > py) != (y2 > py)，与顶点/水平边命中无关。
                if (a.y > y) != (b.y > y) {
                    let intersect_x = a.x + (y - a.y) * (b.x - a.x) / (b.y - a.y);
                    if intersect_x > x {
                        crossings += 1;
                    }
                }
                j = i;
            }
        }
        crossings % 2 == 1
    }

    /// 回放填充到 [`Scene`]（扁平化后的每个子路径当作闭合多边形）。
    pub fn fill_into(&self, scene: &mut Scene, color: impl Into<Color>) {
        let color = color.into();
        for poly in self.flatten() {
            if poly.len() >= 3 {
                scene.fill_path(poly.iter().map(|p| Point { x: p.x, y: p.y }), color);
            }
        }
    }

    /// 回放描边到 [`Scene`]（扁平化后的每个子路径闭合后描边）。
    pub fn stroke_into(&self, scene: &mut Scene, width: f32, color: impl Into<Color>) {
        let color = color.into();
        for poly in self.flatten() {
            if poly.len() >= 2 {
                let mut pts: Vec<Point> = poly.iter().map(|p| Point { x: p.x, y: p.y }).collect();
                // 闭合：把起点补到末尾（对齐 TS `ctx2.stroke()` 闭合轮廓）。
                pts.push(pts[0]);
                scene.stroke_path(pts, width, color);
            }
        }
    }
}

/// 轴对齐矩形（对标 TS `RectF`）。
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RectF {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

impl RectF {
    #[must_use]
    pub fn new(left: f32, top: f32, right: f32, bottom: f32) -> Self {
        Self {
            left,
            top,
            right,
            bottom,
        }
    }

    #[must_use]
    pub fn width(&self) -> f32 {
        self.right - self.left
    }

    #[must_use]
    pub fn height(&self) -> f32 {
        self.bottom - self.top
    }

    #[must_use]
    pub fn center_x(&self) -> f32 {
        (self.left + self.right) / 2.0
    }

    #[must_use]
    pub fn center_y(&self) -> f32 {
        (self.top + self.bottom) / 2.0
    }

    #[must_use]
    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.left && x <= self.right && y >= self.top && y <= self.bottom
    }

    #[must_use]
    pub fn intersects(&self, other: RectF) -> bool {
        self.left < other.right
            && self.right > other.left
            && self.top < other.bottom
            && self.bottom > other.top
    }

    #[must_use]
    pub fn offset(&self, dx: f32, dy: f32) -> RectF {
        RectF::new(
            self.left + dx,
            self.top + dy,
            self.right + dx,
            self.bottom + dy,
        )
    }

    #[must_use]
    pub fn inflate(&self, delta: f32) -> RectF {
        RectF::new(
            self.left - delta,
            self.top - delta,
            self.right + delta,
            self.bottom + delta,
        )
    }
}

/// 矩形并集（SelectionManager.selected_rect 的聚合用）。
#[must_use]
pub fn union_rect(a: RectF, b: RectF) -> RectF {
    RectF::new(
        a.left.min(b.left),
        a.top.min(b.top),
        a.right.max(b.right),
        a.bottom.max(b.bottom),
    )
}

/// 圆角矩形路径助手（demo/组件常用；Kotlin 侧由各组件自行录制）。
/// 以 `MoveTo` / `LineTo` / `CubicTo` 录制四角圆弧（k ≈ 0.5523 近似）。
pub fn append_rounded_rect(
    path: &mut Path2,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    radius: f32,
) -> &mut Path2 {
    let r = (radius.max(0.0)).min((width.min(height)) / 2.0);
    let k = 0.5523 * r;
    let right = x + width;
    let bottom = y + height;
    path.move_to(x + r, y);
    path.line_to(right - r, y);
    path.cubic_to(right - r + k, y, right, y + r - k, right, y + r);
    path.line_to(right, bottom - r);
    path.cubic_to(
        right,
        bottom - r + k,
        right - r + k,
        bottom,
        right - r,
        bottom,
    );
    path.line_to(x + r, bottom);
    path.cubic_to(x + r - k, bottom, x, bottom - r + k, x, bottom - r);
    path.line_to(x, y + r);
    path.cubic_to(x, y + r - k, x + r - k, y, x + r, y);
    path.close();
    path
}

/// 铺满除边框外的整个盒子（含 padding 区）；对应 TS `fillOuterRect`。
pub fn fill_outer_rect(node: &NodeRef, scene: &mut Scene, color: impl Into<Color>) {
    let g = node.geom();
    if g.width <= 0.0 || g.height <= 0.0 {
        return;
    }
    scene.fill_rect(Rect::new(0.0, 0.0, g.width, g.height), color);
}

/// 只铺内容区（已扣掉两侧内边距）；对应 TS `fillInnerRect`。
pub fn fill_inner_rect(node: &NodeRef, scene: &mut Scene, color: impl Into<Color>) {
    let g = node.geom();
    let x = g.pad_left;
    let y = g.pad_top;
    let w = (g.width - g.pad_left - g.pad_right).max(0.0);
    let h = (g.height - g.pad_top - g.pad_bottom).max(0.0);
    if w <= 0.0 || h <= 0.0 {
        return;
    }
    scene.fill_rect(Rect::new(x, y, w, h), color);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path2_rounded_rect_hit() {
        let mut p = Path2::new();
        append_rounded_rect(&mut p, 0.75, 0.75, 16.5, 16.5, 4.0);
        // 圆心附近命中
        assert!(p.contains(8.0, 8.0));
        // 圆角外侧（盒子角外）不命中
        assert!(!p.contains(0.0, 0.0));
        // flatten 后的多边形顶点数合理（>2）
        let polys = p.flatten();
        assert!(polys.iter().all(|q| q.len() >= 3));
    }

    #[test]
    fn rectf_basics() {
        let r = RectF::new(0.0, 0.0, 10.0, 20.0);
        assert_eq!(r.width(), 10.0);
        assert_eq!(r.height(), 20.0);
        assert!(r.contains(5.0, 10.0));
        assert!(!r.contains(11.0, 10.0));
        let u = union_rect(r, RectF::new(5.0, 5.0, 30.0, 30.0));
        assert_eq!(u, RectF::new(0.0, 0.0, 30.0, 30.0));
    }
}
