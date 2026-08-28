//! 基础几何类型：Point、Size、Rect。
//!
//! 为避免引入外部几何库（AGENTS.md 第九条），提供绘制与命中所需的最小集合。

/// 二维坐标点。
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct Point {
    /// X 坐标（逻辑像素）。
    pub x: f32,
    /// Y 坐标（逻辑像素）。
    pub y: f32,
}

impl Point {
    /// 从坐标构造点。
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
}

/// 二维尺寸。
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct Size {
    /// 宽度（逻辑像素）。
    pub width: f32,
    /// 高度（逻辑像素）。
    pub height: f32,
}

impl Size {
    /// 从宽高构造尺寸。
    pub const fn new(width: f32, height: f32) -> Self {
        Self { width, height }
    }
}

/// 轴对齐矩形：以左上角为原点的位置与尺寸。
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct Rect {
    /// 左上角 X 坐标。
    pub x: f32,
    /// 左上角 Y 坐标。
    pub y: f32,
    /// 宽度。
    pub width: f32,
    /// 高度。
    pub height: f32,
}

impl Rect {
    /// 构造矩形。
    pub const fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    /// 零尺寸矩形。
    pub const fn zero() -> Self {
        Self::new(0.0, 0.0, 0.0, 0.0)
    }

    /// 矩形是否为空（宽或高为非正数）。
    pub fn is_empty(self) -> bool {
        self.width <= 0.0 || self.height <= 0.0
    }

    /// 矩形左边缘。
    pub fn left(self) -> f32 {
        self.x
    }

    /// 矩形右边缘。
    pub fn right(self) -> f32 {
        self.x + self.width
    }

    /// 矩形上边缘。
    pub fn top(self) -> f32 {
        self.y
    }

    /// 矩形下边缘。
    pub fn bottom(self) -> f32 {
        self.y + self.height
    }

    /// 中心点。
    pub fn center(self) -> Point {
        Point::new(self.x + self.width * 0.5, self.y + self.height * 0.5)
    }

    /// 点是否位于矩形内（含边界）。
    pub fn contains(self, p: Point) -> bool {
        p.x >= self.x && p.x <= self.right() && p.y >= self.y && p.y <= self.bottom()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rect_edges() {
        let r = Rect::new(10.0, 20.0, 100.0, 50.0);
        assert_eq!(r.left(), 10.0);
        assert_eq!(r.top(), 20.0);
        assert_eq!(r.right(), 110.0);
        assert_eq!(r.bottom(), 70.0);
        assert_eq!(r.center(), Point::new(60.0, 45.0));
    }

    #[test]
    fn rect_is_empty_when_nonpositive() {
        assert!(Rect::new(0.0, 0.0, 0.0, 10.0).is_empty());
        assert!(Rect::new(0.0, 0.0, 10.0, -1.0).is_empty());
        assert!(!Rect::new(0.0, 0.0, 10.0, 10.0).is_empty());
    }

    #[test]
    fn rect_contains_point() {
        let r = Rect::new(0.0, 0.0, 100.0, 100.0);
        assert!(r.contains(Point::new(50.0, 50.0)));
        assert!(r.contains(Point::new(0.0, 0.0)));
        assert!(r.contains(Point::new(100.0, 100.0)));
        assert!(!r.contains(Point::new(101.0, 50.0)));
        assert!(!r.contains(Point::new(50.0, -1.0)));
    }
}
