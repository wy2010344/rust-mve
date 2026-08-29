//! TextWidget：显示文本，支持内在尺寸测量。

use crate::draw_context::DrawContext;
use crate::scene::Scene;
use crate::widget::Widget;
use crate::Color;

/// 文本组件：在指定位置绘制一段文字。
///
/// 支持 `measure()` 方法测量文本的自然尺寸（需要 Parley FontContext）。
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
    /// 固定宽度约束（None = 不换行，取自然宽度）
    max_width: Option<f32>,
}

impl TextWidget {
    /// 创建文本组件。
    pub fn new(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            font_size: 14.0,
            color: Color::BLACK,
            max_width: None,
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

    /// 设置最大宽度约束（超过则换行）。
    pub fn max_width(mut self, width: f32) -> Self {
        self.max_width = Some(width);
        self
    }

    /// 获取当前文本内容。
    pub fn content(&self) -> &str {
        &self.content
    }

    /// 测量文本的自然尺寸。
    ///
    /// 返回 `(width, height)` 像素值。需要 Parley FontContext 和 LayoutContext。
    /// 无 `max_width` 约束时文本不换行，返回单行尺寸。
    pub fn measure(
        &self,
        font_cx: &mut parley::FontContext,
        layout_cx: &mut parley::LayoutContext,
    ) -> (f32, f32) {
        if self.content.is_empty() {
            return (0.0, self.font_size * 1.2);
        }

        let brush = [
            self.color.red(),
            self.color.green(),
            self.color.blue(),
            self.color.alpha(),
        ];
        let display_scale = 1.0;
        let mut builder = layout_cx.ranged_builder(font_cx, &self.content, display_scale, false);
        builder.push_default(parley::StyleProperty::FontSize(self.font_size));
        builder.push_default(parley::StyleProperty::Brush(brush));

        let mut layout: parley::Layout<[u8; 4]> = builder.build(&self.content);
        layout.break_all_lines(self.max_width);
        layout.align(
            parley::Alignment::Start,
            parley::AlignmentOptions::default(),
        );

        // 使用 Parley Layout 提供的 width() 和 height()
        let width = layout.width();
        let height = layout.height().max(self.font_size * 1.2);

        (width, height)
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

    #[test]
    fn text_widget_measure_returns_positive_dimensions() {
        let w = TextWidget::new("Hello").font_size(16.0);
        let mut font_cx = parley::FontContext::new();
        let mut layout_cx = parley::LayoutContext::new();
        let (w, h) = w.measure(&mut font_cx, &mut layout_cx);
        assert!(w > 0.0, "width should be > 0, got {w}");
        assert!(h > 0.0, "height should be > 0, got {h}");
    }

    #[test]
    fn text_widget_measure_empty_string() {
        let w = TextWidget::new("").font_size(16.0);
        let mut font_cx = parley::FontContext::new();
        let mut layout_cx = parley::LayoutContext::new();
        let (w, h) = w.measure(&mut font_cx, &mut layout_cx);
        assert_eq!(w, 0.0);
        assert!(h > 0.0);
    }

    #[test]
    fn text_widget_measure_with_max_width() {
        let w = TextWidget::new("This is a long text that should wrap")
            .font_size(16.0)
            .max_width(100.0);
        let mut font_cx = parley::FontContext::new();
        let mut layout_cx = parley::LayoutContext::new();
        let (_w_single, h_single) = TextWidget::new("This is a long text that should wrap")
            .font_size(16.0)
            .measure(&mut font_cx, &mut layout_cx);
        let (_w_wrap, h_wrap) = w.measure(&mut font_cx, &mut layout_cx);
        // 换行后高度应更大（多行）
        assert!(
            h_wrap >= h_single,
            "wrapped height {h_wrap} should be >= single line {h_single}"
        );
    }

    #[test]
    fn text_widget_measure_larger_font_is_bigger() {
        let mut font_cx = parley::FontContext::new();
        let mut layout_cx = parley::LayoutContext::new();
        let (_, h12) = TextWidget::new("Hi")
            .font_size(12.0)
            .measure(&mut font_cx, &mut layout_cx);
        let (_, h24) = TextWidget::new("Hi")
            .font_size(24.0)
            .measure(&mut font_cx, &mut layout_cx);
        assert!(h24 > h12, "24px height {h24} should be > 12px height {h12}");
    }
}
