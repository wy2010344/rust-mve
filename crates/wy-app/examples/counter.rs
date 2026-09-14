//! 计数器示例：函数组件 + 信号驱动。
//!
//! 运行：`cargo run -p wy-app --example counter`
//!
//! 信号在组件函数外创建（持久化），组件函数通过参数接收。
//! 用户只写组件组合，不写 draw/hit_test/event。

use wy_mve::{button, render_root, row, text_signal, NodeContext};
use wy_signal::{GetValue, SetValue, Signal};

fn counter_ui(cx: &mut NodeContext, count: Signal<i32>) {
    let count_display = count.clone();
    cx.child(text_signal(move || Box::new(count_display.get())));

    let count_row = count.clone();
    cx.child(row(move |cx| {
        let c_minus = count_row.clone();
        cx.child(button(
            || "−".into(),
            move || c_minus.set(c_minus.get() - 1),
        ));
        let c_plus = count_row.clone();
        cx.child(button(|| "+".into(), move || c_plus.set(c_plus.get() + 1)));
    }));
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let count = Signal::new(0);
    let count_clone = count.clone();

    let cache = render_root(move |cx| {
        counter_ui(cx, count_clone.clone());
    });
    let app = wy_engine::mve_integration::MveApp::from_cache(cache);
    wy_engine::runner::run(app)
}
