//! Hello World 示例：演示 winit + wgpu + Vello 完整管线。
//!
//! 运行：`cargo run -p wy-app --example hello`

use wy_engine::runner::{run, WyApp};
use wy_render::{Color, Point, Rect, Scene};

struct HelloApp;

impl WyApp for HelloApp {
    fn draw(&mut self, scene: &mut Scene, width: f32, height: f32) {
        // 白色背景
        scene.fill_rect(Rect::new(0.0, 0.0, width, height), Color::WHITE);

        // 黑色大文字（测试文字渲染）
        scene.draw_text(Point::new(50.0, 100.0), "Hello, wy-ui!", 48.0, Color::BLACK);

        // 红色小文字（0xRRGGBBAA 格式：R=FF, G=00, B=00, A=FF）
        scene.draw_text(
            Point::new(50.0, 200.0),
            "文字渲染测试 ABC 123",
            24.0,
            Color::from_u32(0xFF_00_00_FF),
        );

        // 居中的蓝色矩形
        let rect_w = 200.0;
        let rect_h = 100.0;
        let x = (width - rect_w) / 2.0;
        let y = (height - rect_h) / 2.0;
        scene.fill_round_rect(
            Rect::new(x, y, rect_w, rect_h),
            8.0,
            Color::from_u32(0x33_66_CC_FF),
        );
    }

    fn on_resize(&mut self, width: f32, height: f32) {
        println!("Window resized: {width}x{height}");
    }
}

fn main() {
    println!("=== wy-ui hello world ===");
    println!("Close the window to exit.");
    run(HelloApp).unwrap();
}
