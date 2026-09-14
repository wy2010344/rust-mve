//! 计数器示例：函数组件 + 信号驱动。
//!
//! 运行：`cargo run -p wy-app --example counter`
//!
//! 树结构只构建一次；文本与按钮在 draw 期读取 signal，
//! 按键 set 信号 → RedrawTracker 自动触发重绘。

use wy_mve::{button, column_at, row, text_signal, NodeContext};
use wy_signal::{GetValue, SetValue, Signal};

fn counter_ui(cx: &mut NodeContext, count: Signal<i32>) {
    cx.child(column_at(350.0, 250.0, move |cx| {
        let c = count.clone();
        cx.child(text_signal(move || Box::new(format!("Count: {}", c.get()))));

        let row_count = count.clone();
        cx.child(row(move |cx| {
            cx.child(button(move || "−".into(), {
                let s = row_count.clone();
                move || s.set(s.get() - 1)
            }));
            cx.child(button(move || "+".into(), {
                let s = row_count.clone();
                move || s.set(s.get() + 1)
            }));
        }));
    }));
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let count = Signal::new(0);
    let app = wy_engine::mve_integration::MveApp::new({
        let count = count.clone();
        move |cx| counter_ui(cx, count.clone())
    });
    wy_engine::runner::run(app)
}
