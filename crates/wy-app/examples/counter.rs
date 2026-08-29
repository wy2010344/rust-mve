//! 计数器示例：信号驱动的 GUI 应用。
//!
//! 运行：`cargo run -p wy-app --example counter`
//!
//! 演示 WyApp trait + Signal + TrackEffect + 按需重绘。

use std::rc::Rc;
use wy_engine::runner::{run, WyApp};
use wy_render::{Color, Rect, Scene};
use wy_signal::{create_effect, GetValue, SetValue, Signal};

/// 计数器应用。
struct CounterApp {
    count: Signal<i32>,
}

impl CounterApp {
    fn new() -> Self {
        Self {
            count: Signal::new(0),
        }
    }
}

impl WyApp for CounterApp {
    fn setup(&mut self, request_redraw: Rc<dyn Fn()>) {
        // 当 count 变化时，触发重绘
        let count = self.count.clone();
        create_effect(move || {
            let _ = count.get(); // 注册依赖
            request_redraw();
        });
    }

    fn draw(&mut self, scene: &mut Scene, width: f32, height: f32) {
        let _ = (width, height);

        let w = 200.0f32;
        let h = 100.0f32;
        let x = (width - w) / 2.0;
        let y = (height - h) / 2.0;

        // 背景
        scene.fill_rect(Rect::new(x, y, w, h), Color::from_u32(0xFF_F0F0F0));

        // 标题
        let title = format!("Count: {}", self.count.get());
        scene.draw_text(
            wy_render::Point::new(x + 16.0, y + 16.0),
            &title,
            24.0,
            Color::from_u32(0xFF_000000),
        );

        // [−] 按钮
        scene.fill_round_rect(
            Rect::new(x + 16.0, y + 56.0, 60.0, 32.0),
            4.0,
            Color::from_u32(0xFF_D0D0D0),
        );
        scene.draw_text(
            wy_render::Point::new(x + 36.0, y + 62.0),
            "−",
            20.0,
            Color::from_u32(0xFF_333333),
        );

        // [+] 按钮
        scene.fill_round_rect(
            Rect::new(x + 90.0, y + 56.0, 60.0, 32.0),
            4.0,
            Color::from_u32(0xFF_D0D0D0),
        );
        scene.draw_text(
            wy_render::Point::new(x + 110.0, y + 62.0),
            "+",
            20.0,
            Color::from_u32(0xFF_333333),
        );
    }

    fn handle_event(&mut self, event: &winit::event::WindowEvent) -> bool {
        use winit::event::{ElementState, MouseButton, WindowEvent};
        use winit::keyboard::Key;

        match event {
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } => {
                // 简化：整个窗口区域点击都 toggle
                // 完整实现需要 hit test 确定点击了哪个按钮
                let val = self.count.get();
                self.count.set(val + 1);
                true
            }
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => {
                match &event.logical_key {
                    Key::Character(s) if s.as_str() == "+" => {
                        self.count.set(self.count.get() + 1);
                        true
                    }
                    Key::Character(s) if s.as_str() == "-" => {
                        self.count.set(self.count.get() - 1);
                        true
                    }
                    Key::Named(winit::keyboard::NamedKey::ArrowUp) => {
                        self.count.set(self.count.get() + 1);
                        true
                    }
                    Key::Named(winit::keyboard::NamedKey::ArrowDown) => {
                        self.count.set(self.count.get() - 1);
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
    run(CounterApp::new())
}
