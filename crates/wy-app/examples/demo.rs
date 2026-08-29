//! 综合示例：展示 WidgetTree + Widgets + Theme + Signal + compute_layout 的完整用法。
//!
//! 运行：`cargo run -p wy-app --example demo`
//!
//! 功能：
//! - TextInput + Button（添加任务）
//! - Toggle（深色模式切换）
//! - Slider（字号调节）
//! - 信号驱动按需重绘

use std::rc::Rc;
use wy_engine::runner::{run, WyApp};
use wy_render::theme::Theme;
use wy_render::{Color, Rect, Scene};
use wy_signal::{create_effect, GetValue, SetValue, Signal};

/// Demo 应用状态。
struct DemoApp {
    input_text: Signal<String>,
    items: Signal<Vec<String>>,
    dark_mode: Signal<bool>,
    font_size: Signal<f32>,
    theme: Theme,
    mouse_x: f64,
    mouse_y: f64,
}

impl DemoApp {
    fn new() -> Self {
        Self {
            input_text: Signal::new(String::new()),
            items: Signal::new(vec![
                "学习 Rust".into(),
                "构建 UI 框架".into(),
                "写测试".into(),
                "性能优化".into(),
            ]),
            dark_mode: Signal::new(false),
            font_size: Signal::new(14.0),
            theme: Theme::light(),
            mouse_x: 0.0,
            mouse_y: 0.0,
        }
    }
}

impl WyApp for DemoApp {
    fn setup(&mut self, request_redraw: Rc<dyn Fn()>) {
        let items = self.items.clone();
        let dark = self.dark_mode.clone();
        let fs = self.font_size.clone();
        create_effect(move || {
            let _ = items.get();
            let _ = dark.get();
            let _ = fs.get();
            request_redraw();
        });
    }

    fn draw(&mut self, scene: &mut Scene, width: f32, height: f32) {
        self.theme = if self.dark_mode.get() {
            Theme::dark()
        } else {
            Theme::light()
        };

        let t = self.theme;
        let padding = t.sizes.padding;
        let fs = self.font_size.get();

        // 背景
        scene.fill_rect(Rect::new(0.0, 0.0, width, height), t.colors.background);

        // 标题
        scene.draw_text(
            wy_render::Point::new(padding, padding),
            "wy-framework Demo",
            fs + 8.0,
            t.colors.text,
        );
        scene.draw_text(
            wy_render::Point::new(padding, padding + 32.0),
            "Tab: focus | Enter: add | Click: toggle/slider",
            11.0,
            t.colors.text_secondary,
        );

        let input_y = padding + 60.0;
        let input_h = 32.0;

        // 输入框背景
        scene.fill_rect(
            Rect::new(padding, input_y, width - padding * 2.0 - 90.0, input_h),
            t.colors.input_background,
        );
        scene.fill_rect(
            Rect::new(padding, input_y, width - padding * 2.0 - 90.0, 1.0),
            t.colors.border,
        );
        scene.fill_rect(
            Rect::new(
                padding,
                input_y + input_h - 1.0,
                width - padding * 2.0 - 90.0,
                1.0,
            ),
            t.colors.border,
        );

        let text = self.input_text.get();
        if !text.is_empty() {
            scene.draw_text(
                wy_render::Point::new(padding + 6.0, input_y + 8.0),
                &text,
                fs,
                t.colors.text,
            );
        } else {
            scene.draw_text(
                wy_render::Point::new(padding + 6.0, input_y + 8.0),
                "输入新任务...",
                fs,
                t.colors.text_secondary,
            );
        }

        // Add 按钮
        let btn_x = width - padding - 82.0;
        scene.fill_round_rect(
            Rect::new(btn_x, input_y, 82.0, input_h),
            t.sizes.border_radius,
            t.colors.primary,
        );
        scene.draw_text(
            wy_render::Point::new(btn_x + 24.0, input_y + 8.0),
            "+ Add",
            fs,
            Color::WHITE,
        );

        // === 控制区域 ===
        let ctrl_y = input_y + input_h + 16.0;

        // Toggle
        scene.draw_text(
            wy_render::Point::new(padding, ctrl_y + 4.0),
            "Dark Mode",
            fs,
            t.colors.text,
        );
        let toggle_x = width - padding - 48.0;
        let is_dark = self.dark_mode.get();
        let toggle_bg = if is_dark {
            t.colors.primary
        } else {
            t.colors.border
        };
        scene.fill_round_rect(Rect::new(toggle_x, ctrl_y, 44.0, 24.0), 12.0, toggle_bg);
        // 圆点（用小圆角矩形模拟）
        let dot_x = if is_dark {
            toggle_x + 24.0
        } else {
            toggle_x + 2.0
        };
        scene.fill_round_rect(
            Rect::new(dot_x, ctrl_y + 2.0, 20.0, 20.0),
            10.0,
            Color::WHITE,
        );

        // Slider
        let slider_y = ctrl_y + 40.0;
        scene.draw_text(
            wy_render::Point::new(padding, slider_y + 6.0),
            &format!("Font Size: {:.0}px", fs),
            fs,
            t.colors.text,
        );
        let slider_x = width - padding - 120.0;
        let slider_w = 120.0;
        // 轨道
        scene.fill_round_rect(
            Rect::new(slider_x, slider_y + 10.0, slider_w, 4.0),
            2.0,
            t.colors.border,
        );
        // 填充
        let ratio = (fs - 10.0) / 20.0;
        let fill_w = slider_w * ratio.clamp(0.0, 1.0);
        scene.fill_round_rect(
            Rect::new(slider_x, slider_y + 10.0, fill_w, 4.0),
            2.0,
            t.colors.primary,
        );
        // 滑块
        scene.fill_round_rect(
            Rect::new(slider_x + fill_w - 7.0, slider_y + 5.0, 14.0, 14.0),
            7.0,
            t.colors.primary,
        );

        // === 任务列表 ===
        let list_y = slider_y + 48.0;
        let items = self.items.get();
        scene.draw_text(
            wy_render::Point::new(padding, list_y),
            &format!("Tasks ({})", items.len()),
            fs,
            t.colors.text,
        );

        if items.is_empty() {
            scene.draw_text(
                wy_render::Point::new(padding, list_y + 28.0),
                "No tasks yet. Add one above!",
                fs,
                t.colors.text_secondary,
            );
            return;
        }

        let mut item_y = list_y + 28.0;
        let item_h = 32.0;

        for (i, item) in items.iter().enumerate() {
            if item_y + item_h > height - padding {
                // 滚动溢出指示器
                scene.fill_round_rect(
                    Rect::new(
                        padding,
                        height - padding - 24.0,
                        width - padding * 2.0,
                        20.0,
                    ),
                    6.0,
                    t.colors.button_background,
                );
                scene.draw_text(
                    wy_render::Point::new(padding + 8.0, height - padding - 20.0),
                    &format!("... {} more", items.len() - i),
                    11.0,
                    t.colors.text_secondary,
                );
                break;
            }

            scene.fill_round_rect(
                Rect::new(padding, item_y, width - padding * 2.0, item_h),
                t.sizes.border_radius,
                t.colors.button_background,
            );
            scene.draw_text(
                wy_render::Point::new(padding + 8.0, item_y + 8.0),
                &format!("{}.", i + 1),
                fs,
                t.colors.text_secondary,
            );
            scene.draw_text(
                wy_render::Point::new(padding + 32.0, item_y + 8.0),
                item,
                fs,
                t.colors.text,
            );
            item_y += item_h + 8.0;
        }
    }

    fn handle_event(&mut self, event: &winit::event::WindowEvent) -> bool {
        use winit::event::{ElementState, MouseButton, WindowEvent};
        use winit::keyboard::Key;

        match event {
            WindowEvent::CursorMoved { position, .. } => {
                self.mouse_x = position.x;
                self.mouse_y = position.y;
                false
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } => {
                let t = self.theme;
                let padding = t.sizes.padding;
                let width = 400.0;
                let input_y = padding + 60.0;
                let input_h = 32.0;

                // Add 按钮
                let btn_x = width - padding - 82.0;
                if self.mouse_x >= btn_x as f64
                    && self.mouse_x <= (btn_x + 82.0) as f64
                    && self.mouse_y >= input_y as f64
                    && self.mouse_y <= (input_y + input_h) as f64
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

                // Toggle
                let ctrl_y = input_y + input_h + 16.0;
                let toggle_x = width - padding - 48.0;
                if self.mouse_x >= toggle_x as f64
                    && self.mouse_x <= (toggle_x + 44.0) as f64
                    && self.mouse_y >= ctrl_y as f64
                    && self.mouse_y <= (ctrl_y + 24.0) as f64
                {
                    self.dark_mode.set(!self.dark_mode.get());
                    return true;
                }

                // Slider
                let slider_y = ctrl_y + 40.0;
                let slider_x = width - padding - 120.0;
                if self.mouse_x >= slider_x as f64
                    && self.mouse_x <= (slider_x + 120.0) as f64
                    && self.mouse_y >= (slider_y + 4.0) as f64
                    && self.mouse_y <= (slider_y + 20.0) as f64
                {
                    let ratio = ((self.mouse_x - slider_x as f64) / 120.0) as f32;
                    let new_fs = 10.0 + ratio.clamp(0.0, 1.0) * 20.0;
                    self.font_size.set(new_fs);
                    return true;
                }

                false
            }
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => {
                match &event.logical_key {
                    Key::Character(s) if s.as_str() == "\r" => {
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
