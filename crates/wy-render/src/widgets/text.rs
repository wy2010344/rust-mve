//! TextWidget：显示文本。

use crate::draw_context::DrawContext;
use crate::scene::Scene;
use crate::widget::Widget;
use crate::Color;

/// 文本组件：在指定位置绘制一段文字。
///
/// ```ignore
/// use wy_render::widgets::TextWidget;
///
/// let widget = TextWidget::new("Hello, World!")
///     .font_size(18.0)
///     .color(Color::BLACK);
/// ```
pub struct TextWidget {
    content: String,
    font_size: f32,
    color: Color,
}

impl TextWidget {
    /// 创建文本组件。
    pub fn new(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            font_size: 14.0,
            color: Color::BLACK,
        }
    }

    /// 设置字体大小。
    pub fn font_size(mut self, size: f32) -> Self {
        self.font_size = size;
        self
    }

    /// 设置文本颜色。
    pub fn color(mut self, color: impl Into<Color>) -> Self {
        self.color = color.into();
        self
    }

    /// 获取当前文本内容。
    pub fn content(&self) -> &str {
        &self.content
    }
}

impl Widget for TextWidget {
    fn draw(&self, scene: &mut Scene, cx: &mut DrawContext) {
        let rect = cx.outer_rect();
        // 文本绘制在组件区域的左上角
        scene.draw_text(
            crate::Point::new(rect.x, rect.y),
            &self.content,
            self.font_size,
            self.color,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::Rect;

    #[test]
    fn text_widget_new_defaults() {
        let w = TextWidget::new("hello");
        assert_eq!(w.content(), "hello");
        assert_eq!(w.font_size, 14.0);
        assert_eq!(w.color, Color::BLACK);
    }

    #[test]
    fn text_widget_builder_chain() {
        let w = TextWidget::new("test").font_size(20.0).color(Color::RED);
        assert_eq!(w.font_size, 20.0);
        assert_eq!(w.color, Color::RED);
    }

    #[test]
    fn text_widget_draw_produces_text_primitive() {
        let w = TextWidget::new("hello").font_size(16.0).color(Color::BLUE);
        let mut scene = Scene::new();
        let mut cx = DrawContext::new(
            Rect::new(10.0, 20.0, 100.0, 30.0),
            crate::Point::new(10.0, 20.0),
            crate::Size::new(100.0, 30.0),
        );
        w.draw(&mut scene, &mut cx);
        assert_eq!(scene.len(), 1);
        let prim = scene.iter().next().unwrap();
        match prim {
            crate::Primitive::Text {
                point,
                text,
                font_size,
                color,
            } => {
                assert_eq!(point.x, 10.0);
                assert_eq!(point.y, 20.0);
                assert_eq!(text, "hello");
                assert_eq!(*font_size, 16.0);
                assert_eq!(*color, Color::BLUE);
            }
            other => panic!("expected Text, got {other:?}"),
        }
    }
}
