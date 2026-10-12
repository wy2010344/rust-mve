//! TextField 全链路示例：`rsx!` 声明式写法 + 焦点 + 键盘/IME 输入。
//!
//! 运行：`cargo run -p wy-app --example textfield`
//!
//! 演示：
//! - 两个输入框（姓名 / 密码），点击聚焦；
//! - Tab 在输入框与按钮间焦点遍历；
//! - 正常输入文字、退格、方向键、IME 组合输入；
//! - 密码框掩码显示；
//! - 输入内容实时同步到信号（on_change）。

use wy_mve::{rsx, text_field, text_field_opts, TextFieldOpts};
use wy_signal::{GetValue, SetValue, Signal};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let name = Signal::new(String::from("张"));
    let pass = Signal::new(String::from("secret123"));
    let log = Signal::new(String::new());

    let app = wy_engine::mve_integration::MveApp::new(move |cx| {
        let node = rsx! {
            vbox_at(50.0, 50.0) {
                label(log.get())
                text_field(move || name.get(), move |t| name.set(t))
                text_field_opts(
                    move || pass.get(),
                    move |t| pass.set(t),
                    TextFieldOpts {
                        width: 220.0,
                        height: 32.0,
                        placeholder: "密码".into(),
                        password: true,
                        ..Default::default()
                    }
                )
                btn {
                    label: "显示输入",
                    on_click: move || {
                        let mut s = String::new();
                        s.push_str(&format!("name={}\n", name.get()));
                        s.push_str(&format!("pass={}\n", pass.get()));
                        log.set(s);
                    }
                }
            }
        };
        cx.child(node.into_node());
    });

    wy_engine::runner::run(app)
}
