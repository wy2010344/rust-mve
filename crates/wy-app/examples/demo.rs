//! 综合示例：展示 WidgetTree + Widgets + Theme + Signal 的完整用法。
//!
//! 运行：`cargo run -p wy-app --example demo`
//!
//! 功能：
//! - 文本输入框 + 添加按钮
//! - 点击按钮将输入文本添加到列表
//! - 信号驱动按需重绘
//! - WidgetTree 自动事件分发

use std::rc::Rc;
use wy_engine::runner::{run, WyApp};
use wy_render::theme::Theme;
use wy_render::{Color, Rect, Scene};
use wy_signal::{create_effect, GetValue, SetValue, Signal};

/// Demo 应用状态。
struct DemoApp {
    input_text: Signal<String>,
    items: Signal<Vec<String>>,
    theme: Theme,
    cursor_x: f64,
    cursor_y: f64,
}

impl DemoApp {
    fn new() -> Self {
        Self {
            input_text: Signal::new(String::new()),
            items: Signal::new(Vec::new()),
            theme: Theme::light(),
            cursor_x: 0.0,
            cursor_y: 0.0,
        }
    }
}

impl WyApp for DemoApp {
    fn setup(&mut self, request_redraw: Rc<dyn Fn()>) {
        // 信号变化时触发重绘
        let input = self.input_text.clone();
        let items = self.items.clone();
        create_effect(move || {
            let _ = input.get();
            let _ = items.get();
            request_redraw();
        });
    }

    fn draw(&mut self, scene: &mut Scene, width: f32, height: f32) {
        let t = &self.theme;
        let padding = t.sizes.padding;
        let spacing = t.sizes.spacing;

        // 背景
        scene.fill_rect(Rect::new(0.0, 0.0, width, height), t.colors.background);

        // 标题
        scene.draw_text(
            wy_render::Point::new(padding, padding),
            "Demo App",
            t.sizes.font_size + 6.0,
            t.colors.text,
        );

        // 输入区域 y 坐标
        let input_y = padding + 36.0;
        let input_h = 28.0;

        // 输入框背景
        scene.fill_rect(
            Rect::new(padding, input_y, width - padding * 2.0 - 80.0, input_h),
            t.colors.input_background,
        );
        // 输入框边框
        scene.fill_rect(
            Rect::new(padding, input_y, width - padding * 2.0 - 80.0, 1.0),
            t.colors.border,
        );
        scene.fill_rect(
            Rect::new(
                padding,
                input_y + input_h - 1.0,
                width - padding * 2.0 - 80.0,
                1.0,
            ),
            t.colors.border,
        );

        // 输入文本
        let text = self.input_text.get();
        if !text.is_empty() {
            scene.draw_text(
                wy_render::Point::new(padding + 4.0, input_y + 6.0),
                &text,
                t.sizes.font_size,
                t.colors.text,
            );
        } else {
            scene.draw_text(
                wy_render::Point::new(padding + 4.0, input_y + 6.0),
                "Type something...",
                t.sizes.font_size,
                t.colors.text_secondary,
            );
        }

        // 添加按钮
        let btn_x = width - padding - 72.0;
        scene.fill_round_rect(
            Rect::new(btn_x, input_y, 72.0, input_h),
            t.sizes.border_radius,
            t.colors.primary,
        );
        scene.draw_text(
            wy_render::Point::new(btn_x + 20.0, input_y + 6.0),
            "Add",
            t.sizes.font_size,
            Color::WHITE,
        );

        // 列表标题
        let list_y = input_y + input_h + spacing * 2.0;
        scene.draw_text(
            wy_render::Point::new(padding, list_y),
            &format!("Items ({})", self.items.get().len()),
            t.sizes.font_size,
            t.colors.text,
        );

        // 列表项
        let items = self.items.get();
        let mut item_y = list_y + 24.0;
        for (i, item) in items.iter().enumerate() {
            // 项背景
            scene.fill_round_rect(
                Rect::new(padding, item_y, width - padding * 2.0, 28.0),
                t.sizes.border_radius,
                t.colors.button_background,
            );
            // 项文本
            scene.draw_text(
                wy_render::Point::new(padding + 8.0, item_y + 6.0),
                &format!("{}. {}", i + 1, item),
                t.sizes.font_size,
                t.colors.text,
            );
            item_y += 28.0 + spacing;
        }
    }

    fn handle_event(&mut self, event: &winit::event::WindowEvent) -> bool {
        use winit::event::{ElementState, MouseButton, WindowEvent};
        use winit::keyboard::Key;

        match event {
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor_x = position.x;
                self.cursor_y = position.y;
                false
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } => {
                let t = &self.theme;
                let padding = t.sizes.padding;
                let input_h = 28.0;
                let input_y = padding + 36.0;

                // 检查是否点击了"Add"按钮
                let btn_x = 400.0; // 假设窗口宽度 400
                if self.cursor_x >= btn_x
                    && self.cursor_x <= btn_x + 72.0
                    && self.cursor_y >= input_y as f64
                    && self.cursor_y <= (input_y + input_h) as f64
                {
                    let text = self.input_text.get();
                    if !text.is_empty() {
                        let mut items = self.items.get();
                        items.push(text.to_string());
                        self.items.set(items);
                        self.input_text.set(String::new());
                    }
                    return true;
                }
                false
            }
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => {
                match &event.logical_key {
                    Key::Character(s) if s.as_str() == "\r" => {
                        // Enter: 添加到列表
                        let text = self.input_text.get();
                        if !text.is_empty() {
                            let mut items = self.items.get();
                            items.push(text.to_string());
                            self.items.set(items);
                            self.input_text.set(String::new());
                        }
                        true
                    }
                    Key::Named(winit::keyboard::NamedKey::Backspace) => {
                        let mut text = self.input_text.get();
                        text.pop();
                        self.input_text.set(text);
                        true
                    }
                    Key::Character(s) => {
                        let mut text = self.input_text.get();
                        text.push_str(s);
                        self.input_text.set(text);
                        true
                    }
                    _ => false,
                }
            }
            _ => false,
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    run(DemoApp::new())
}
