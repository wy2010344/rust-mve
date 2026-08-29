//! ToggleWidget：开关组件。

use crate::draw_context::DrawContext;
use crate::scene::Scene;
use crate::widget::Widget;
use crate::Color;

/// 开关组件：显示一个可以切换开/关状态的拨动开关。
///
/// ```ignore
/// use wy_render::widgets::ToggleWidget;
///
/// let widget = ToggleWidget::new(true);
/// ```
pub struct ToggleWidget {
    on: bool,
    height: f32,
    on_color: Color,
    off_color: Color,
    thumb_color: Color,
}

impl ToggleWidget {
    /// 创建开关，初始状态由 `on` 指定。
    pub fn new(on: bool) -> Self {
        Self {
            on,
            height: 24.0,
            on_color: Color::rgba(0, 120, 212, 255),
            off_color: Color::rgba(180, 180, 180, 255),
            thumb_color: Color::WHITE,
        }
    }

    /// 切换状态。
    pub fn toggle(&mut self) {
        self.on = !self.on;
    }

    /// 获取当前状态。
    pub fn is_on(&self) -> bool {
        self.on
    }

    /// 设置开/关颜色。
    pub fn colors(mut self, on: impl Into<Color>, off: impl Into<Color>) -> Self {
        self.on_color = on.into();
        self.off_color = off.into();
        self
    }
}

impl Widget for ToggleWidget {
    fn draw(&self, scene: &mut Scene, cx: &mut DrawContext) {
        let rect = cx.outer_rect();
        let bg = if self.on {
            self.on_color
        } else {
            self.off_color
        };

        // 轨道
        scene.fill_round_rect(rect, self.height / 2.0, bg);

        // 滑块
        let thumb_size = self.height - 4.0;
        let thumb_x = if self.on {
            rect.x + rect.width - thumb_size - 2.0
        } else {
            rect.x + 2.0
        };
        let thumb_y = rect.y + 2.0;
        scene.fill_round_rect(
            crate::Rect::new(thumb_x, thumb_y, thumb_size, thumb_size),
            thumb_size / 2.0,
            self.thumb_color,
        );
    }

    fn on_click(&mut self, _cx: &DrawContext) {
        self.on = !self.on;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::Rect;

    #[test]
    fn toggle_new_defaults() {
        let w = ToggleWidget::new(true);
        assert!(w.is_on());
        let w = ToggleWidget::new(false);
        assert!(!w.is_on());
    }

    #[test]
    fn toggle_toggle_flips_state() {
        let mut w = ToggleWidget::new(false);
        w.toggle();
        assert!(w.is_on());
        w.toggle();
        assert!(!w.is_on());
    }

    #[test]
    fn toggle_on_click_flips() {
        let mut w = ToggleWidget::new(false);
        let cx = DrawContext::new(
            Rect::new(0.0, 0.0, 44.0, 24.0),
            crate::Point::new(0.0, 0.0),
            crate::Size::new(44.0, 24.0),
        );
        Widget::on_click(&mut w, &cx);
        assert!(w.is_on());
    }

    #[test]
    fn toggle_draw_produces_round_rects() {
        let w = ToggleWidget::new(true);
        let mut scene = Scene::new();
        let mut cx = DrawContext::new(
            Rect::new(0.0, 0.0, 44.0, 24.0),
            crate::Point::new(0.0, 0.0),
            crate::Size::new(44.0, 24.0),
        );
        w.draw(&mut scene, &mut cx);
        assert_eq!(scene.len(), 2); // track + thumb
    }
}
