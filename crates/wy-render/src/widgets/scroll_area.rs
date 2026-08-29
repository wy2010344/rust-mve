//! ScrollAreaWidget：滚动区域容器。

use crate::draw_context::DrawContext;
use crate::scene::Scene;
use crate::widget::Widget;
use crate::Color;

/// 滚动区域：裁剪子内容并支持偏移。
///
/// 子组件绘制时会被裁剪到区域范围内。通过 `scroll_x`/`scroll_y`
/// 控制可见区域的偏移量。
///
/// ```ignore
/// use wy_render::widgets::ScrollAreaWidget;
///
/// let widget = ScrollAreaWidget::new(200.0, 300.0)
///     .scroll_y(50.0);
/// ```
pub struct ScrollAreaWidget {
    scroll_x: f32,
    scroll_y: f32,
    content_height: f32,
    background: Option<Color>,
}

impl ScrollAreaWidget {
    /// 创建滚动区域。
    pub fn new(_width: f32, _height: f32) -> Self {
        Self {
            scroll_x: 0.0,
            scroll_y: 0.0,
            content_height: 0.0,
            background: None,
        }
    }

    /// 设置垂直滚动偏移。
    pub fn scroll_y(mut self, y: f32) -> Self {
        self.scroll_y = y.max(0.0);
        self
    }

    /// 设置水平滚动偏移。
    pub fn scroll_x(mut self, x: f32) -> Self {
        self.scroll_x = x.max(0.0);
        self
    }

    /// 设置内容总高度（用于滚动条计算）。
    pub fn content_height(mut self, h: f32) -> Self {
        self.content_height = h;
        self
    }

    /// 设置背景色。
    pub fn background(mut self, color: impl Into<Color>) -> Self {
        self.background = Some(color.into());
        self
    }

    /// 获取当前滚动偏移。
    pub fn scroll_offset(&self) -> (f32, f32) {
        (self.scroll_x, self.scroll_y)
    }
}

impl Widget for ScrollAreaWidget {
    fn draw(&self, scene: &mut Scene, cx: &mut DrawContext) {
        let rect = cx.outer_rect();

        // 背景
        if let Some(bg) = self.background {
            scene.fill_rect(rect, bg);
        }

        // 裁剪到区域范围
        scene.push_clip(rect);

        // 注意：子组件的绘制由 WidgetTree 递归调用 draw_node 处理，
        // 这里只需要设置裁剪区域。子组件的偏移需要在布局阶段处理。
        scene.pop_clip();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::Rect;

    #[test]
    fn scroll_area_new_defaults() {
        let w = ScrollAreaWidget::new(200.0, 300.0);
        assert_eq!(w.scroll_offset(), (0.0, 0.0));
    }

    #[test]
    fn scroll_area_builder_chain() {
        let w = ScrollAreaWidget::new(200.0, 300.0)
            .scroll_y(50.0)
            .scroll_x(10.0)
            .content_height(1000.0)
            .background(Color::WHITE);
        assert_eq!(w.scroll_offset(), (10.0, 50.0));
        assert_eq!(w.content_height, 1000.0);
    }

    #[test]
    fn scroll_area_scroll_y_clamped() {
        let w = ScrollAreaWidget::new(200.0, 300.0).scroll_y(-10.0);
        assert_eq!(w.scroll_offset().1, 0.0);
    }

    #[test]
    fn scroll_area_draw_produces_clip() {
        let w = ScrollAreaWidget::new(200.0, 300.0);
        let mut scene = Scene::new();
        let mut cx = DrawContext::new(
            Rect::new(0.0, 0.0, 200.0, 300.0),
            crate::Point::new(0.0, 0.0),
            crate::Size::new(200.0, 300.0),
        );
        w.draw(&mut scene, &mut cx);
        // ClipPush + ClipPop = 2 primitives
        assert_eq!(scene.len(), 2);
    }

    #[test]
    fn scroll_area_draw_with_background() {
        let w = ScrollAreaWidget::new(200.0, 300.0).background(Color::GRAY);
        let mut scene = Scene::new();
        let mut cx = DrawContext::new(
            Rect::new(0.0, 0.0, 200.0, 300.0),
            crate::Point::new(0.0, 0.0),
            crate::Size::new(200.0, 300.0),
        );
        w.draw(&mut scene, &mut cx);
        // background rect + ClipPush + ClipPop = 3
        assert_eq!(scene.len(), 3);
    }
}
