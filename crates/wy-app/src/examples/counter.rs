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

use wy_render::{Widget, Scene, DrawContext};
use wy_render::widget::ChildBuilder;

/// 计数器 Widget。
struct Counter {
    count: i32,
}

impl Widget for Counter {
    fn children(&self, _cx: &mut ChildBuilder) {
        // TODO: 用信号系统实现响应式
        // cx.child(text(move || format!("Count: {}", count.get())));
    }

    fn draw(&self, scene: &mut Scene, cx: &mut DrawContext) {
        let w = cx.outer_width();
        let h = cx.outer_height();

        // 画背景
        scene.fill_round_rect(0.0, 0.0, w, h, 12.0, 0xFFFFFFFF);
        scene.fill_round_rect(0.0, 0.0, w, h, 12.0, 0xFFE0E0E0);

        // 画文字
        let text = format!("Count: {}", self.count);
        scene.draw_text(16.0, 16.0, &text, 24.0, 0xFF000000);
    }
}

fn main() {
    println!("wy-ui counter example");
    println!("Count: {}", Counter { count: 0 }.count);
}
