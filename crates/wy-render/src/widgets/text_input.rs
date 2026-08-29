//! TextInputWidget：单行文本输入框。

use crate::draw_context::DrawContext;
use crate::event::{Key, KeyEvent, PointerEvent};
use crate::scene::Scene;
use crate::widget::Widget;
use crate::Color;

/// 文本输入框组件：支持输入、删除、光标移动。
///
/// 需要配合焦点系统使用——点击时获得焦点，键盘事件由焦点管理器转发。
///
/// ```ignore
/// use wy_render::widgets::TextInputWidget;
///
/// let widget = TextInputWidget::new()
///     .placeholder("Enter text...")
///     .font_size(16.0);
/// ```
pub struct TextInputWidget {
    text: String,
    cursor_pos: usize,
    font_size: f32,
    background: Color,
    text_color: Color,
    border_color: Color,
    placeholder: String,
    focused: bool,
}

impl TextInputWidget {
    /// 创建空输入框。
    pub fn new() -> Self {
        Self {
            text: String::new(),
            cursor_pos: 0,
            font_size: 14.0,
            background: Color::WHITE,
            text_color: Color::BLACK,
            border_color: Color::rgba(180, 180, 180, 255),
            placeholder: String::new(),
            focused: false,
        }
    }

    /// 设置初始文本。
    pub fn text(mut self, text: impl Into<String>) -> Self {
        self.text = text.into();
        self
    }

    /// 设置占位符文本。
    pub fn placeholder(mut self, text: impl Into<String>) -> Self {
        self.placeholder = text.into();
        self
    }

    /// 设置字体大小。
    pub fn font_size(mut self, size: f32) -> Self {
        self.font_size = size;
        self
    }

    /// 获取当前文本内容。
    pub fn text_content(&self) -> &str {
        &self.text
    }

    /// 获取光标位置。
    pub fn cursor_position(&self) -> usize {
        self.cursor_pos
    }

    /// 设置焦点状态。
    pub fn set_focused(&mut self, focused: bool) {
        self.focused = focused;
    }

    /// 处理键盘输入。
    pub fn handle_key(&mut self, event: &KeyEvent) {
        match event.key {
            Key::Backspace => {
                if self.cursor_pos > 0 {
                    self.cursor_pos -= 1;
                    self.text.remove(self.cursor_pos);
                }
            }
            Key::Delete => {
                if self.cursor_pos < self.text.len() {
                    self.text.remove(self.cursor_pos);
                }
            }
            Key::ArrowLeft => {
                if self.cursor_pos > 0 {
                    self.cursor_pos -= 1;
                }
            }
            Key::ArrowRight => {
                if self.cursor_pos < self.text.len() {
                    self.cursor_pos += 1;
                }
            }
            Key::Char(ch) if event.pressed => {
                self.text.insert(self.cursor_pos, ch);
                self.cursor_pos += 1;
            }
            _ => {}
        }
    }
}

impl Default for TextInputWidget {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for TextInputWidget {
    fn draw(&self, scene: &mut Scene, cx: &mut DrawContext) {
        let rect = cx.outer_rect();

        // 边框
        let border_color = if self.focused {
            Color::rgba(0, 120, 212, 255)
        } else {
            self.border_color
        };
        scene.fill_rect(
            crate::Rect::new(rect.x, rect.y, rect.width, 1.0),
            border_color,
        );
        scene.fill_rect(
            crate::Rect::new(rect.x, rect.y + rect.height - 1.0, rect.width, 1.0),
            border_color,
        );
        scene.fill_rect(
            crate::Rect::new(rect.x, rect.y, 1.0, rect.height),
            border_color,
        );
        scene.fill_rect(
            crate::Rect::new(rect.x + rect.width - 1.0, rect.y, 1.0, rect.height),
            border_color,
        );

        // 背景
        scene.fill_rect(
            crate::Rect::new(
                rect.x + 1.0,
                rect.y + 1.0,
                rect.width - 2.0,
                rect.height - 2.0,
            ),
            self.background,
        );

        // 文本或占位符
        let display_text = if self.text.is_empty() {
            self.placeholder.as_str()
        } else {
            self.text.as_str()
        };
        let text_color = if self.text.is_empty() {
            Color::rgba(160, 160, 160, 255)
        } else {
            self.text_color
        };
        if !display_text.is_empty() {
            scene.draw_text(
                crate::Point::new(rect.x + 4.0, rect.y + (rect.height - self.font_size) / 2.0),
                display_text,
                self.font_size,
                text_color,
            );
        }

        // 光标
        if self.focused {
            let cursor_x = rect.x + 4.0 + self.cursor_pos as f32 * self.font_size * 0.6;
            scene.fill_rect(
                crate::Rect::new(cursor_x, rect.y + 4.0, 1.0, rect.height - 8.0),
                self.text_color,
            );
        }
    }

    fn focusable(&self) -> bool {
        true
    }

    fn on_pointer_down(&mut self, _event: &mut PointerEvent, _cx: &DrawContext) {
        // 点击时获得焦点（由 WidgetTree 焦点系统处理）
    }

    fn on_click(&mut self, _cx: &DrawContext) {
        // 获得焦点
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::Rect;

    #[test]
    fn input_new_defaults() {
        let w = TextInputWidget::new();
        assert_eq!(w.text_content(), "");
        assert_eq!(w.cursor_position(), 0);
        assert_eq!(w.font_size, 14.0);
    }

    #[test]
    fn input_builder_chain() {
        let w = TextInputWidget::new()
            .text("hello")
            .placeholder("type here")
            .font_size(16.0);
        assert_eq!(w.text_content(), "hello");
        assert_eq!(w.placeholder, "type here");
        assert_eq!(w.font_size, 16.0);
    }

    #[test]
    fn input_focusable_is_true() {
        let w = TextInputWidget::new();
        assert!(w.focusable());
    }

    #[test]
    fn input_handle_key_insert_char() {
        let mut w = TextInputWidget::new();
        let event = KeyEvent::new(Key::Char('a'), true);
        w.handle_key(&event);
        assert_eq!(w.text_content(), "a");
        assert_eq!(w.cursor_position(), 1);
    }

    #[test]
    fn input_handle_key_backspace() {
        let mut w = TextInputWidget::new().text("abc");
        w.set_focused(true);
        w.cursor_pos = 2;
        let event = KeyEvent::new(Key::Backspace, true);
        w.handle_key(&event);
        assert_eq!(w.text_content(), "ac");
        assert_eq!(w.cursor_position(), 1);
    }

    #[test]
    fn input_handle_key_delete() {
        let mut w = TextInputWidget::new().text("abc");
        w.cursor_pos = 1;
        let event = KeyEvent::new(Key::Delete, true);
        w.handle_key(&event);
        assert_eq!(w.text_content(), "ac");
        assert_eq!(w.cursor_position(), 1);
    }

    #[test]
    fn input_handle_key_arrow_left() {
        let mut w = TextInputWidget::new().text("abc");
        w.cursor_pos = 2;
        let event = KeyEvent::new(Key::ArrowLeft, true);
        w.handle_key(&event);
        assert_eq!(w.cursor_position(), 1);
    }

    #[test]
    fn input_handle_key_arrow_right() {
        let mut w = TextInputWidget::new().text("abc");
        w.cursor_pos = 1;
        let event = KeyEvent::new(Key::ArrowRight, true);
        w.handle_key(&event);
        assert_eq!(w.cursor_position(), 2);
    }

    #[test]
    fn input_cursor_clamps_at_bounds() {
        let mut w = TextInputWidget::new().text("abc");
        w.cursor_pos = 0;
        let left = KeyEvent::new(Key::ArrowLeft, true);
        w.handle_key(&left);
        assert_eq!(w.cursor_position(), 0);

        w.cursor_pos = 3;
        let right = KeyEvent::new(Key::ArrowRight, true);
        w.handle_key(&right);
        assert_eq!(w.cursor_position(), 3);
    }

    #[test]
    fn input_draw_shows_border_and_text() {
        let w = TextInputWidget::new().text("hi");
        let mut scene = Scene::new();
        let mut cx = DrawContext::new(
            Rect::new(10.0, 10.0, 200.0, 30.0),
            crate::Point::new(10.0, 10.0),
            crate::Size::new(200.0, 30.0),
        );
        w.draw(&mut scene, &mut cx);
        // 4 borders + 1 background + 1 text = 6
        assert_eq!(scene.len(), 6);
    }

    #[test]
    fn input_draw_shows_placeholder_when_empty() {
        let w = TextInputWidget::new().placeholder("hint");
        let mut scene = Scene::new();
        let mut cx = DrawContext::new(
            Rect::new(0.0, 0.0, 200.0, 30.0),
            crate::Point::new(0.0, 0.0),
            crate::Size::new(200.0, 30.0),
        );
        w.draw(&mut scene, &mut cx);
        assert_eq!(scene.len(), 6);
    }
}
