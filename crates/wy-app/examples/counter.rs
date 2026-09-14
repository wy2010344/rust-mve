//! 计数器示例：函数组件 + 信号驱动。
//!
//! 运行：`cargo run -p wy-app --example counter`

use wy_mve::{button, column_at, render_root, row, text_signal, NodeContext};
use wy_signal::{GetValue, SetValue, Signal};

fn counter_ui(cx: &mut NodeContext, count: Signal<i32>) {
    cx.child(column_at(350.0, 250.0, move |cx| {
        // text_signal 在构造时读信号（被 tracker 追踪）
        let c = count.clone();
        cx.child(text_signal(move || Box::new(format!("Count: {}", c.get()))));

        let row_count = count.clone();
        cx.child(row(move |cx| {
            cx.child(button(|| "−".into(), {
                let s = row_count.clone();
                move || s.set(s.get() - 1)
            }));
            cx.child(button(|| "+".into(), {
                let s = row_count.clone();
                move || s.set(s.get() + 1)
            }));
        }));
    }));
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let count = Signal::new(0);
    let cache = render_root({
        let count = count.clone();
        move |cx| counter_ui(cx, count.clone())
    });
    let app = wy_engine::mve_integration::MveApp::from_cache(cache);
    wy_engine::runner::run(app)
}
