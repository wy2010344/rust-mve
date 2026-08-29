//! 性能基准测试：测量框架各模块的性能。
//!
//! 运行：`cargo run -p wy-app --example bench --release`

use std::time::Instant;
use wy_render::theme::Theme;
use wy_render::widget_tree::WidgetTree;
use wy_render::widgets::ButtonWidget;
use wy_render::{Color, Rect, Scene};
use wy_signal::{GetValue, SetValue, Signal};

fn main() {
    println!("=== wy-framework 性能基准测试 ===\n");

    bench_signal();
    bench_scene();
    bench_widget_tree_hit_test();
    bench_layout_computation();
    bench_widget_tree_draw();

    println!("\n=== 基准测试完成 ===");
}

fn bench_signal() {
    print!("Signal 读写: ");
    let start = Instant::now();
    let signal = Signal::new(0i32);
    for i in 0..100_000 {
        signal.set(i);
        let _ = signal.get();
    }
    let elapsed = start.elapsed();
    println!(
        "{:.2?} ({:.0} ops/ms)",
        elapsed,
        200_000.0 / elapsed.as_secs_f64() / 1000.0
    );
}

fn bench_scene() {
    print!("Scene 构建 (1000 图元): ");
    let start = Instant::now();
    let mut scene = Scene::new();
    for i in 0..1000 {
        scene.fill_rect(
            Rect::new(i as f32, 0.0, 10.0, 10.0),
            Color::rgba(i as u8, 0, 0, 255),
        );
    }
    let elapsed = start.elapsed();
    println!(
        "{:.2?} ({:.0} rects/ms)",
        elapsed,
        1000.0 / elapsed.as_secs_f64() / 1000.0
    );
}

fn bench_widget_tree_hit_test() {
    print!("WidgetTree 命中测试 (100 节点, 10000 次): ");

    struct Leaf;
    impl wy_render::Widget for Leaf {
        fn draw(&self, _s: &mut Scene, _cx: &mut wy_render::DrawContext) {}
    }

    // 构建一个 100 节点的树（10 层 × 10 子节点）
    struct Branch;
    impl wy_render::Widget for Branch {
        fn children(&self, cx: &mut wy_render::widget::ChildBuilder) {
            for _ in 0..10 {
                cx.add_child(Leaf);
            }
        }
        fn draw(&self, _s: &mut Scene, _cx: &mut wy_render::DrawContext) {}
    }

    let mut tree = WidgetTree::new(Branch);
    // 设置布局：根节点 100x100，子节点垂直堆叠
    tree.compute_layout_vertical(100.0, 100.0, |idx| if idx == 0 { 100.0 } else { 10.0 });

    let start = Instant::now();
    for i in 0..10_000 {
        let x = (i % 100) as f32;
        let y = ((i * 7) % 100) as f32;
        let _ = tree.hit_test(x, y);
    }
    let elapsed = start.elapsed();
    println!(
        "{:.2?} ({:.0} hits/ms)",
        elapsed,
        10_000.0 / elapsed.as_secs_f64() / 1000.0
    );
}

fn bench_layout_computation() {
    print!("布局计算 (100 节点): ");

    struct Leaf;
    impl wy_render::Widget for Leaf {
        fn draw(&self, _s: &mut Scene, _cx: &mut wy_render::DrawContext) {}
    }

    struct Parent;
    impl wy_render::Widget for Parent {
        fn children(&self, cx: &mut wy_render::widget::ChildBuilder) {
            for _ in 0..10 {
                cx.add_child(Leaf);
            }
        }
        fn draw(&self, _s: &mut Scene, _cx: &mut wy_render::DrawContext) {}
    }

    let start = Instant::now();
    for _ in 0..1000 {
        let mut tree = WidgetTree::new(Parent);
        tree.compute_layout_vertical(400.0, 800.0, |_| 40.0);
    }
    let elapsed = start.elapsed();
    println!(
        "{:.2?} ({:.0} layouts/ms)",
        elapsed,
        1000.0 / elapsed.as_secs_f64() / 1000.0
    );
}

fn bench_widget_tree_draw() {
    print!("WidgetTree 绘制 (100 节点, 1000 帧): ");

    struct Panel;
    impl wy_render::Widget for Panel {
        fn children(&self, cx: &mut wy_render::widget::ChildBuilder) {
            for i in 0..10 {
                cx.add_child(ButtonWidget::new(format!("Button {i}")));
            }
        }
        fn draw(&self, scene: &mut Scene, cx: &mut wy_render::DrawContext) {
            scene.fill_rect(cx.outer_rect(), Color::GRAY);
        }
    }

    let mut tree = WidgetTree::new(Panel);
    tree.set_theme(Theme::light());
    tree.compute_layout_vertical(400.0, 400.0, |idx| if idx == 0 { 400.0 } else { 36.0 });

    let start = Instant::now();
    for _ in 0..1000 {
        let mut scene = Scene::new();
        tree.draw_scene(&mut scene);
    }
    let elapsed = start.elapsed();
    println!(
        "{:.2?} ({:.0} frames/ms)",
        elapsed,
        1000.0 / elapsed.as_secs_f64() / 1000.0
    );
}
