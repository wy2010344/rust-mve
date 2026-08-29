//! ButtonWidget：可点击的按钮。

use crate::draw_context::DrawContext;
use crate::event::PointerEvent;
use crate::scene::Scene;
use crate::widget::Widget;
use crate::Color;

/// 按钮组件：带标签的可点击区域。
///
/// 点击时调用 `on_click()` 可重写此方法执行自定义逻辑。
/// 默认绘制一个圆角矩形背景 + 居中文本。
///
/// ```ignore
/// use wy_render::widgets::ButtonWidget;
///
/// let widget = ButtonWidget::new("Click Me");
/// ```
pub struct ButtonWidget {
    label: String,
    font_size: f32,
    background: Color,
    hover_background: Color,
    text_color: Color,
    border_radius: f32,
    /// 内部状态：鼠标是否在按钮上。
    hovering: bool,
    /// 内部状态：鼠标是否按下。
    pressing: bool,
}

impl ButtonWidget {
    /// 创建按钮。
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            font_size: 14.0,
            background: Color::rgba(230, 230, 230, 255),
            hover_background: Color::rgba(210, 210, 210, 255),
            text_color: Color::BLACK,
            border_radius: 4.0,
            hovering: false,
            pressing: false,
        }
    }

    /// 设置标签文本。
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// 设置字体大小。
    pub fn font_size(mut self, size: f32) -> Self {
        self.font_size = size;
        self
    }

    /// 设置背景色。
    pub fn background(mut self, color: impl Into<Color>) -> Self {
        self.background = color.into();
        self
    }

    /// 设置悬停背景色。
    pub fn hover_background(mut self, color: impl Into<Color>) -> Self {
        self.hover_background = color.into();
        self
    }

    /// 设置文本颜色。
    pub fn text_color(mut self, color: impl Into<Color>) -> Self {
        self.text_color = color.into();
        self
    }

    /// 设置圆角半径。
    pub fn border_radius(mut self, radius: f32) -> Self {
        self.border_radius = radius;
        self
    }

    /// 获取按钮标签。
    pub fn label_text(&self) -> &str {
        &self.label
    }
}

impl Widget for ButtonWidget {
    fn draw(&self, scene: &mut Scene, cx: &mut DrawContext) {
        let rect = cx.outer_rect();
        let bg = if self.pressing || self.hovering {
            self.hover_background
        } else {
            self.background
        };

        // 背景
        if self.border_radius > 0.0 {
            scene.fill_round_rect(rect, self.border_radius, bg);
        } else {
            scene.fill_rect(rect, bg);
        }

        // 文本居中
        let text_x = rect.x + (rect.width - self.label.len() as f32 * self.font_size * 0.5) / 2.0;
        let text_y = rect.y + (rect.height - self.font_size) / 2.0;
        scene.draw_text(
            crate::Point::new(text_x, text_y),
            &self.label,
            self.font_size,
            self.text_color,
        );
    }

    fn on_pointer_down(&mut self, _event: &mut PointerEvent, _cx: &DrawContext) {
        self.pressing = true;
    }

    fn on_pointer_up(&mut self, _event: &mut PointerEvent, _cx: &DrawContext) {
        self.pressing = false;
    }

    fn on_pointer_move(&mut self, _event: &mut PointerEvent, cx: &DrawContext) {
        // 更新悬停状态
        self.hovering = cx
            .outer_rect()
            .contains(crate::Point::new(_event.x, _event.y));
    }

    fn on_click(&mut self, _cx: &DrawContext) {
        // 默认不做任何事，子类可重写
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::Rect;

    #[test]
    fn button_new_defaults() {
        let w = ButtonWidget::new("OK");
        assert_eq!(w.label_text(), "OK");
        assert_eq!(w.font_size, 14.0);
        assert!(!w.hovering);
        assert!(!w.pressing);
    }

    #[test]
    fn button_builder_chain() {
        let w = ButtonWidget::new("Submit")
            .font_size(16.0)
            .background(Color::BLUE)
            .hover_background(Color::RED)
            .text_color(Color::WHITE)
            .border_radius(8.0);
        assert_eq!(w.font_size, 16.0);
        assert_eq!(w.background, Color::BLUE);
        assert_eq!(w.hover_background, Color::RED);
        assert_eq!(w.text_color, Color::WHITE);
        assert_eq!(w.border_radius, 8.0);
    }

    #[test]
    fn button_draw_produces_rect_and_text() {
        let w = ButtonWidget::new("Click").border_radius(0.0);
        let mut scene = Scene::new();
        let mut cx = DrawContext::new(
            Rect::new(0.0, 0.0, 100.0, 40.0),
            crate::Point::new(0.0, 0.0),
            crate::Size::new(100.0, 40.0),
        );
        w.draw(&mut scene, &mut cx);
        assert_eq!(scene.len(), 2);
        assert!(matches!(
            scene.iter().next().unwrap(),
            crate::Primitive::Rect { .. }
        ));
        assert!(matches!(
            scene.iter().nth(1).unwrap(),
            crate::Primitive::Text { .. }
        ));
    }

    #[test]
    fn button_draw_round_rect_when_radius_nonzero() {
        let w = ButtonWidget::new("OK").border_radius(6.0);
        let mut scene = Scene::new();
        let mut cx = DrawContext::new(
            Rect::new(0.0, 0.0, 80.0, 30.0),
            crate::Point::new(0.0, 0.0),
            crate::Size::new(80.0, 30.0),
        );
        w.draw(&mut scene, &mut cx);
        assert!(matches!(
            scene.iter().next().unwrap(),
            crate::Primitive::RoundRect { .. }
        ));
    }

    #[test]
    fn button_pointer_down_sets_pressing() {
        let mut w = ButtonWidget::new("OK");
        assert!(!w.pressing);
        let mut event = PointerEvent::new(crate::event::PointerType::Down, 5.0, 5.0);
        let cx = DrawContext::new(
            Rect::new(0.0, 0.0, 100.0, 40.0),
            crate::Point::new(0.0, 0.0),
            crate::Size::new(100.0, 40.0),
        );
        Widget::on_pointer_down(&mut w, &mut event, &cx);
        assert!(w.pressing);
    }

    #[test]
    fn button_pointer_up_clears_pressing() {
        let mut w = ButtonWidget::new("OK");
        let mut event = PointerEvent::new(crate::event::PointerType::Up, 5.0, 5.0);
        let cx = DrawContext::new(
            Rect::new(0.0, 0.0, 100.0, 40.0),
            crate::Point::new(0.0, 0.0),
            crate::Size::new(100.0, 40.0),
        );
        w.pressing = true;
        Widget::on_pointer_up(&mut w, &mut event, &cx);
        assert!(!w.pressing);
    }

    #[test]
    fn button_focusable_is_false_by_default() {
        let w = ButtonWidget::new("OK");
        assert!(!w.focusable());
    }
}
