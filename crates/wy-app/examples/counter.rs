//! 计数器示例：信号驱动的 GUI 应用。
//!
//! 运行：`cargo run -p wy-app --example counter`
//!
//! 用闭包属性模拟 Kotlin 的计算属性：
//! - `count()` 每次调用都读 `signal.get()`，天然实时
//! - 框架层 RedrawTracker 自动追踪信号依赖，信号变化自动触发重绘
//! - draw() 调用 `self.count()` 拿最新值，无中间状态

use wy_engine::runner::{run, WyApp};
use wy_render::{Color, Point, Rect, Scene};
use wy_signal::{GetValue, SetValue, Signal};

const BTN_W: f32 = 60.0;
const BTN_H: f32 = 40.0;
const BTN_GAP: f32 = 12.0;

struct CounterApp {
    count_signal: Signal<i32>,
    cursor: (f32, f32),
    win_size: (f32, f32),
}

impl CounterApp {
    fn new() -> Self {
        Self {
            count_signal: Signal::new(0),
            cursor: (0.0, 0.0),
            win_size: (800.0, 600.0),
        }
    }

    /// 计算属性：每次调用都读信号，等价于 Kotlin 的 `val count get() = signal.value`
    fn count(&self) -> i32 {
        self.count_signal.get()
    }

    fn btn_rects(&self) -> (Rect, Rect) {
        let cx = self.win_size.0 / 2.0;
        let cy = self.win_size.1 / 2.0;
        let total_w = BTN_W * 2.0 + BTN_GAP;
        let sx = cx - total_w / 2.0;
        let by = cy - BTN_H / 2.0 + 30.0;
        (
            Rect::new(sx, by, BTN_W, BTN_H),
            Rect::new(sx + BTN_W + BTN_GAP, by, BTN_W, BTN_H),
        )
    }

    fn hit_test(&self, x: f32, y: f32) -> Option<&str> {
        let (minus, plus) = self.btn_rects();
        if x >= minus.x && x < minus.x + minus.width && y >= minus.y && y < minus.y + minus.height {
            Some("minus")
        } else if x >= plus.x && x < plus.x + plus.width && y >= plus.y && y < plus.y + plus.height
        {
            Some("plus")
        } else {
            None
        }
    }
}

impl WyApp for CounterApp {
    fn draw(&self, scene: &mut Scene, width: f32, height: f32) {
        let cx = width / 2.0;
        let cy = height / 2.0;

        // 调用计算属性，每次读最新值
        let num = format!("{}", self.count());
        scene.draw_text(Point::new(cx - 10.0, cy - 10.0), &num, 32.0, Color::BLACK);

        let (minus, plus) = self.btn_rects();

        scene.fill_round_rect(minus, 6.0, Color::from_u32(0xFF_DCDCDC));
        scene.stroke_round_rect(minus, 6.0, Color::from_u32(0xFF_B0B0B0), 1.5);
        scene.draw_text(
            Point::new(minus.x + 22.0, minus.y + 10.0),
            "−",
            20.0,
            Color::BLACK,
        );

        scene.fill_round_rect(plus, 6.0, Color::from_u32(0xFF_DCDCDC));
        scene.stroke_round_rect(plus, 6.0, Color::from_u32(0xFF_B0B0B0), 1.5);
        scene.draw_text(
            Point::new(plus.x + 22.0, plus.y + 10.0),
            "+",
            20.0,
            Color::BLACK,
        );
    }

    fn on_resize(&mut self, width: f32, height: f32) {
        self.win_size = (width, height);
    }

    fn handle_event(&mut self, event: &winit::event::WindowEvent) -> bool {
        use winit::event::{ElementState, MouseButton, WindowEvent};

        match event {
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor = (position.x as f32, position.y as f32);
                false
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } => {
                let (x, y) = self.cursor;
                match self.hit_test(x, y) {
                    Some("minus") => {
                        self.count_signal.set(self.count() - 1);
                        true
                    }
                    Some("plus") => {
                        self.count_signal.set(self.count() + 1);
                        true
                    }
                    _ => false,
                }
            }
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => {
                use winit::keyboard::Key;
                match &event.logical_key {
                    Key::Character(s) if s.as_str() == "+" || s.as_str() == "=" => {
                        self.count_signal.set(self.count() + 1);
                        true
                    }
                    Key::Character(s) if s.as_str() == "-" || s.as_str() == "_" => {
                        self.count_signal.set(self.count() - 1);
                        true
                    }
                    Key::Named(winit::keyboard::NamedKey::ArrowUp) => {
                        self.count_signal.set(self.count() + 1);
                        true
                    }
                    Key::Named(winit::keyboard::NamedKey::ArrowDown) => {
                        self.count_signal.set(self.count() - 1);
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
