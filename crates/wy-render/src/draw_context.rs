//! DrawContext：draw 调用的上下文，提供布局信息。

use crate::math::{Point, Rect, Size};
use crate::theme::Theme;

/// DrawContext：draw 调用的上下文，提供布局信息。
///
/// 组件在 `draw()` 中通过它读取自身（及内容区）的几何信息，据此决定如何把
/// 内容映射到 [`crate::Scene`]。布局结果由 `wy-layout` 填充。
#[derive(Clone, Debug)]
pub struct DrawContext {
    outer: Rect,
    inner: Point,
    inner_shape: Size,
    theme: Option<Theme>,
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
            theme: None,
        }
    }

    /// 带主题的上下文构造。
    pub fn with_theme(outer: Rect, inner: Point, inner_shape: Size, theme: Theme) -> Self {
        Self {
            outer,
            inner,
            inner_shape,
            theme: Some(theme),
        }
    }

    /// 获取主题引用（如果有）。
    pub fn theme(&self) -> Option<&Theme> {
        self.theme.as_ref()
    }

    /// 获取主题（如果有），否则返回默认浅色主题。
    pub fn theme_or_default(&self) -> Theme {
        self.theme.unwrap_or(Theme::light())
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
            theme: None,
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
        assert!(cx.theme().is_none());
    }

    #[test]
    fn draw_context_with_theme() {
        let theme = Theme::dark();
        let cx = DrawContext::with_theme(
            Rect::new(0.0, 0.0, 100.0, 100.0),
            Point::new(0.0, 0.0),
            Size::new(100.0, 100.0),
            theme,
        );
        assert!(cx.theme().is_some());
        assert_eq!(
            cx.theme().unwrap().colors.background,
            Theme::dark().colors.background
        );
        // theme_or_default 返回传入的主题
        assert_eq!(
            cx.theme_or_default().colors.background,
            Theme::dark().colors.background
        );
    }

    #[test]
    fn draw_context_theme_or_default_falls_back() {
        let cx = DrawContext::default();
        assert!(cx.theme().is_none());
        // 没有主题时返回浅色默认
        assert_eq!(
            cx.theme_or_default().colors.background,
            Theme::light().colors.background
        );
    }
}
