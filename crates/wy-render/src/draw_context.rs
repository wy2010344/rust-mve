//! DrawContext：draw 调用的上下文，提供布局信息。

use crate::math::{Point, Rect, Size};

/// DrawContext：draw 调用的上下文，提供布局信息。
///
/// 组件在 `draw()` 中通过它读取自身（及内容区）的几何信息，据此决定如何把
/// 内容映射到 [`crate::Scene`]。布局结果由 `wy-layout` 填充。
#[derive(Clone, Debug)]
pub struct DrawContext {
    outer: Rect,
    inner: Point,
    inner_shape: Size,
}

impl DrawContext {
    /// 从布局结果构造上下文。
    ///
    /// - `outer`：节点**外框**（含 padding/margin，即布局系统分配的区域）。
    /// - `inner`：内容区左上角（已叠加本节点 padding 偏移）。
    /// - `inner_shape`：内容区可用尺寸。
    pub fn new(outer: Rect, inner: Point, inner_shape: Size) -> Self {
        Self {
            outer,
            inner,
            inner_shape,
        }
    }

    /// 节点外框矩形。
    pub fn outer_rect(&self) -> Rect {
        self.outer
    }

    /// 节点外框宽度（含 padding/margin）。
    pub fn outer_width(&self) -> f32 {
        self.outer.width
    }

    /// 节点外框高度（含 padding/margin）。
    pub fn outer_height(&self) -> f32 {
        self.outer.height
    }

    /// 内容区左上角（已含 padding 偏移，尚未叠加 margin）。
    pub fn inner_origin(&self) -> Point {
        self.inner
    }

    /// 内容区 X 偏移（含 padding）。
    pub fn inner_x(&self) -> f32 {
        self.inner.x
    }

    /// 内容区 Y 偏移（含 padding）。
    pub fn inner_y(&self) -> f32 {
        self.inner.y
    }

    /// 内容区可用尺寸。
    pub fn inner_shape(&self) -> Size {
        self.inner_shape
    }

    /// 内容区宽度。
    pub fn inner_width(&self) -> f32 {
        self.inner_shape.width
    }

    /// 内容区高度。
    pub fn inner_height(&self) -> f32 {
        self.inner_shape.height
    }
}

impl Default for DrawContext {
    fn default() -> Self {
        Self {
            outer: Rect::zero(),
            inner: Point::new(0.0, 0.0),
            inner_shape: Size::new(0.0, 0.0),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn draw_context_reports_outer_and_inner() {
        let cx = DrawContext::new(
            Rect::new(10.0, 20.0, 120.0, 60.0),
            Point::new(15.0, 25.0),
            Size::new(110.0, 50.0),
        );
        assert_eq!(cx.outer_rect(), Rect::new(10.0, 20.0, 120.0, 60.0));
        assert_eq!(cx.outer_width(), 120.0);
        assert_eq!(cx.outer_height(), 60.0);
        assert_eq!(cx.inner_x(), 15.0);
        assert_eq!(cx.inner_y(), 25.0);
        assert_eq!(cx.inner_width(), 110.0);
        assert_eq!(cx.inner_height(), 50.0);
    }

    #[test]
    fn draw_context_default_is_zero() {
        let cx = DrawContext::default();
        assert_eq!(cx.outer_width(), 0.0);
        assert_eq!(cx.inner_height(), 0.0);
    }
}
