//! 计数器示例：函数组件 + 信号驱动。
//!
//! 运行：`cargo run -p wy-app --example counter`
//!
//! 用户只写组件组合，不写 draw/hit_test/event。

use wy_mve::{button, render_root, row, text_signal, NodeContext};
use wy_signal::{GetValue, SetValue, Signal};

fn counter_ui(cx: &mut NodeContext) {
    let count = Signal::new(0);

    let count_display = count.clone();
    cx.child(text_signal(move || Box::new(count_display.get())));

    cx.child(row(move |cx| {
        let c_minus = count.clone();
        cx.child(button(
            || "−".into(),
            move || c_minus.set(c_minus.get() - 1),
        ));
        let c_plus = count.clone();
        cx.child(button(|| "+".into(), move || c_plus.set(c_plus.get() + 1)));
    }));
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cache = render_root(counter_ui);
    let app = wy_engine::mve_integration::MveApp::from_cache(cache);
    wy_engine::runner::run(app)
}
