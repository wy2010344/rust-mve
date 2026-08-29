//! SliderWidget：滑块组件。

use crate::draw_context::DrawContext;
use crate::scene::Scene;
use crate::widget::Widget;
use crate::Color;

/// 滑块组件：在范围内选择一个数值。
///
/// ```ignore
/// use wy_render::widgets::SliderWidget;
///
/// let widget = SliderWidget::new(0.0, 100.0, 50.0);
/// ```
pub struct SliderWidget {
    min: f32,
    max: f32,
    value: f32,
    track_height: f32,
    track_color: Color,
    fill_color: Color,
    thumb_color: Color,
}

impl SliderWidget {
    /// 创建滑块，指定范围和当前值。
    pub fn new(min: f32, max: f32, value: f32) -> Self {
        Self {
            min,
            max,
            value: value.clamp(min, max),
            track_height: 4.0,
            track_color: Color::rgba(200, 200, 200, 255),
            fill_color: Color::rgba(0, 120, 212, 255),
            thumb_color: Color::WHITE,
        }
    }

    /// 获取当前值。
    pub fn value(&self) -> f32 {
        self.value
    }

    /// 设置值（自动 clamp 到范围内）。
    pub fn set_value(&mut self, v: f32) {
        self.value = v.clamp(self.min, self.max);
    }

    /// 计算值的归一化位置（0.0 ~ 1.0）。
    fn normalized(&self) -> f32 {
        if self.max <= self.min {
            0.0
        } else {
            (self.value - self.min) / (self.max - self.min)
        }
    }
}

impl Widget for SliderWidget {
    fn draw(&self, scene: &mut Scene, cx: &mut DrawContext) {
        let rect = cx.outer_rect();
        let track_y = rect.y + (rect.height - self.track_height) / 2.0;

        // 轨道背景
        scene.fill_round_rect(
            crate::Rect::new(rect.x, track_y, rect.width, self.track_height),
            self.track_height / 2.0,
            self.track_color,
        );

        // 已填充部分
        let fill_width = rect.width * self.normalized();
        if fill_width > 0.0 {
            scene.fill_round_rect(
                crate::Rect::new(rect.x, track_y, fill_width, self.track_height),
                self.track_height / 2.0,
                self.fill_color,
            );
        }

        // 滑块圆点
        let thumb_size = 16.0;
        let thumb_x = rect.x + fill_width - thumb_size / 2.0;
        let thumb_y = rect.y + (rect.height - thumb_size) / 2.0;
        scene.fill_round_rect(
            crate::Rect::new(thumb_x, thumb_y, thumb_size, thumb_size),
            thumb_size / 2.0,
            self.thumb_color,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::Rect;

    #[test]
    fn slider_new_defaults() {
        let w = SliderWidget::new(0.0, 100.0, 50.0);
        assert_eq!(w.value(), 50.0);
    }

    #[test]
    fn slider_value_clamped() {
        let w = SliderWidget::new(0.0, 100.0, 150.0);
        assert_eq!(w.value(), 100.0);
        let w = SliderWidget::new(0.0, 100.0, -10.0);
        assert_eq!(w.value(), 0.0);
    }

    #[test]
    fn slider_set_value() {
        let mut w = SliderWidget::new(0.0, 100.0, 50.0);
        w.set_value(75.0);
        assert_eq!(w.value(), 75.0);
        w.set_value(200.0);
        assert_eq!(w.value(), 100.0);
    }

    #[test]
    fn slider_normalized() {
        let w = SliderWidget::new(0.0, 100.0, 50.0);
        assert_eq!(w.normalized(), 0.5);
        let w = SliderWidget::new(10.0, 20.0, 15.0);
        assert_eq!(w.normalized(), 0.5);
    }

    #[test]
    fn slider_draw_produces_primitives() {
        let w = SliderWidget::new(0.0, 100.0, 50.0);
        let mut scene = Scene::new();
        let mut cx = DrawContext::new(
            Rect::new(0.0, 0.0, 200.0, 20.0),
            crate::Point::new(0.0, 0.0),
            crate::Size::new(200.0, 20.0),
        );
        w.draw(&mut scene, &mut cx);
        assert_eq!(scene.len(), 3); // track + fill + thumb
    }
}
