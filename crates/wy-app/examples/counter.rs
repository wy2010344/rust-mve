//! 计数器示例：`rsx!` 声明式写法 + 信号驱动。
//!
//! 运行：`cargo run -p wy-app --example counter`
//!
//! 树结构只构建一次；`label(format!(..))` 由宏包成闭包在 draw 期求值，
//! 读信号自动追踪 → 按键 set 信号后该文本局部重绘。

use wy_mve::{rsx, NodeContext};
use wy_signal::{GetValue, SetValue, Signal};

fn counter_ui(cx: &mut NodeContext, count: Signal<i32>) {
    let node = rsx! {
        vbox_at(350.0, 250.0) {
            label(format!("Count: {}", count.get()))
            hbox {
                // 显式 8.0：Node 默认 gap 是 0（对齐 Kotlin 上游），
                // 要间距必须自己写
                gap: 8.0,
                btn { label: "−", on_click: move || count.set(count.get() - 1) }
                btn { label: "+", on_click: move || count.set(count.get() + 1) }
            }
        }
    };
    cx.child(node.into_node());
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let count = Signal::new(0);
    let app = wy_engine::mve_integration::MveApp::new(move |cx| counter_ui(cx, count));
    wy_engine::runner::run(app)
}
