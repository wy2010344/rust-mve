//! 计数器示例：最简单的信号驱动 UI。
//!
//! 运行：`cargo run -p wy-app --example counter`
//!
//! ```text
//! ┌──────────────────┐
//! │    Count: 0      │
//! │  [−]  [+]        │
//! └──────────────────┘
//! ```

use wy_render::{Color, Point, Scene};
use wy_signal::{GetValue, SetValue, Signal};

/// 计数器 Widget：用信号驱动，每帧读取 count 并绘制。
struct Counter {
    count: Signal<i32>,
}

impl Counter {
    /// 创建计数器，返回 counter 和 increment/decrement 闭包。
    fn new() -> (Self, impl Fn(), impl Fn()) {
        let count = Signal::new(0);
        let inc = count.clone();
        let dec = count.clone();
        (
            Self { count },
            move || inc.set(inc.get() + 1),
            move || dec.set(dec.get() - 1),
        )
    }

    /// 绘制计数器到 Scene。
    fn draw(&self, scene: &mut Scene) {
        let w = 200.0f32;
        let h = 100.0f32;

        // 背景
        scene.fill_rect(
            wy_render::Rect::new(0.0, 0.0, w, h),
            Color::from_u32(0xFF_F0F0F0),
        );

        // 标题文字
        let title = format!("Count: {}", self.count.get());
        scene.draw_text(
            Point::new(16.0, 16.0),
            &title,
            24.0,
            Color::from_u32(0xFF_000000),
        );

        // 按钮背景（简化：两个矩形）
        scene.fill_round_rect(
            wy_render::Rect::new(16.0, 56.0, 60.0, 32.0),
            4.0,
            Color::from_u32(0xFF_D0D0D0),
        );
        scene.draw_text(
            Point::new(36.0, 62.0),
            "−",
            20.0,
            Color::from_u32(0xFF_333333),
        );

        scene.fill_round_rect(
            wy_render::Rect::new(90.0, 56.0, 60.0, 32.0),
            4.0,
            Color::from_u32(0xFF_D0D0D0),
        );
        scene.draw_text(
            Point::new(110.0, 62.0),
            "+",
            20.0,
            Color::from_u32(0xFF_333333),
        );
    }
}

fn main() {
    println!("=== wy-ui counter example ===\n");

    let (counter, increment, decrement) = Counter::new();

    // 初始状态
    let mut scene = Scene::new();
    counter.draw(&mut scene);
    println!("Initial state:");
    print_scene(&scene);

    // 点击 + 按钮
    increment();
    scene.clear();
    counter.draw(&mut scene);
    println!("\nAfter click +:");
    print_scene(&scene);

    // 再次点击 +
    increment();
    scene.clear();
    counter.draw(&mut scene);
    println!("\nAfter click + again:");
    print_scene(&scene);

    // 点击 - 按钮
    decrement();
    scene.clear();
    counter.draw(&mut scene);
    println!("\nAfter click -:");
    print_scene(&scene);
}

fn print_scene(scene: &Scene) {
    for (i, prim) in scene.iter().enumerate() {
        match prim {
            wy_render::Primitive::Rect { rect, color } => {
                println!(
                    "  [{i}] Rect({:.0},{:.0},{:.0},{:.0}) color={:#X}",
                    rect.x,
                    rect.y,
                    rect.width,
                    rect.height,
                    color.to_u32()
                );
            }
            wy_render::Primitive::RoundRect {
                rect,
                radius,
                color,
            } => {
                println!(
                    "  [{i}] RoundRect({:.0},{:.0},{:.0},{:.0} r={:.0}) color={:#X}",
                    rect.x,
                    rect.y,
                    rect.width,
                    rect.height,
                    radius,
                    color.to_u32()
                );
            }
            wy_render::Primitive::Text {
                point,
                text,
                font_size,
                color,
            } => {
                println!(
                    "  [{i}] Text(\"{}\") at ({:.0},{:.0}) size={:.0} color={:#X}",
                    text,
                    point.x,
                    point.y,
                    font_size,
                    color.to_u32()
                );
            }
            _ => {
                println!("  [{i}] {prim:?}");
            }
        }
    }
}
