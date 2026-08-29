//! ContainerWidget：带背景色的容器。

use crate::draw_context::DrawContext;
use crate::scene::Scene;
use crate::widget::Widget;
use crate::Color;

/// 容器组件：绘制一个填充背景的矩形。
///
/// 用于分组和装饰子组件。默认无背景色（透明）。
///
/// ```ignore
/// use wy_render::widgets::ContainerWidget;
///
/// let widget = ContainerWidget::new()
///     .background(Color::from_rgba(255, 255, 255, 255))
///     .border_radius(8.0);
/// ```
pub struct ContainerWidget {
    background: Option<Color>,
    border_radius: f32,
}

impl ContainerWidget {
    /// 创建空容器（透明背景）。
    pub fn new() -> Self {
        Self {
            background: None,
            border_radius: 0.0,
        }
    }

    /// 设置背景色。
    pub fn background(mut self, color: impl Into<Color>) -> Self {
        self.background = Some(color.into());
        self
    }

    /// 设置圆角半径。
    pub fn border_radius(mut self, radius: f32) -> Self {
        self.border_radius = radius;
        self
    }
}

impl Default for ContainerWidget {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for ContainerWidget {
    fn draw(&self, scene: &mut Scene, cx: &mut DrawContext) {
        if let Some(color) = self.background {
            let rect = cx.outer_rect();
            if self.border_radius > 0.0 {
                scene.fill_round_rect(rect, self.border_radius, color);
            } else {
                scene.fill_rect(rect, color);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::Rect;

    #[test]
    fn container_new_defaults() {
        let w = ContainerWidget::new();
        assert!(w.background.is_none());
        assert_eq!(w.border_radius, 0.0);
    }

    #[test]
    fn container_builder_chain() {
        let w = ContainerWidget::new()
            .background(Color::WHITE)
            .border_radius(4.0);
        assert_eq!(w.background, Some(Color::WHITE));
        assert_eq!(w.border_radius, 4.0);
    }

    #[test]
    fn container_draw_no_background() {
        let w = ContainerWidget::new();
        let mut scene = Scene::new();
        let mut cx = DrawContext::new(
            Rect::new(0.0, 0.0, 100.0, 100.0),
            crate::Point::new(0.0, 0.0),
            crate::Size::new(100.0, 100.0),
        );
        w.draw(&mut scene, &mut cx);
        assert_eq!(scene.len(), 0);
    }

    #[test]
    fn container_draw_rect_background() {
        let w = ContainerWidget::new().background(Color::RED);
        let mut scene = Scene::new();
        let mut cx = DrawContext::new(
            Rect::new(5.0, 5.0, 200.0, 100.0),
            crate::Point::new(5.0, 5.0),
            crate::Size::new(200.0, 100.0),
        );
        w.draw(&mut scene, &mut cx);
        assert_eq!(scene.len(), 1);
        let prim = scene.iter().next().cloned().unwrap();
        match prim {
            crate::Primitive::Rect { rect, color } => {
                assert_eq!(rect, Rect::new(5.0, 5.0, 200.0, 100.0));
                assert_eq!(color, Color::RED);
            }
            other => panic!("expected Rect, got {other:?}"),
        }
    }

    #[test]
    fn container_draw_round_rect_background() {
        let w = ContainerWidget::new()
            .background(Color::BLUE)
            .border_radius(8.0);
        let mut scene = Scene::new();
        let mut cx = DrawContext::new(
            Rect::new(0.0, 0.0, 50.0, 50.0),
            crate::Point::new(0.0, 0.0),
            crate::Size::new(50.0, 50.0),
        );
        w.draw(&mut scene, &mut cx);
        assert_eq!(scene.len(), 1);
        let prim = scene.iter().next().cloned().unwrap();
        match prim {
            crate::Primitive::RoundRect {
                rect,
                radius,
                color,
            } => {
                assert_eq!(rect, Rect::new(0.0, 0.0, 50.0, 50.0));
                assert_eq!(radius, 8.0);
                assert_eq!(color, Color::BLUE);
            }
            other => panic!("expected RoundRect, got {other:?}"),
        }
    }
}
