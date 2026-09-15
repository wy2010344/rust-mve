//! TextField 全链路示例：MVE + 焦点 + 键盘/IME 输入。
//!
//! 运行：`cargo run -p wy-app --example textfield`
//!
//! 演示：
//! - 两个输入框（姓名 / 密码），点击聚焦；
//! - Tab 在输入框与按钮间焦点遍历；
//! - 正常输入文字、退格、方向键、IME 组合输入；
//! - 密码框掩码显示；
//! - 输入内容实时同步到信号（on_change）。

use std::rc::Rc;

use wy_mve::{button, column_at, text_field, text_field_opts, Node, TextFieldOpts};
use wy_render::{Color, Point, Scene};
use wy_signal::{GetValue, SetValue, Signal};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let name = Signal::new(String::from("张"));
    let pass = Signal::new(String::from("secret123"));
    let log = Signal::new(String::new());

    let app = wy_engine::mve_integration::MveApp::new(move |cx| {
        let name = name.clone();
        let pass = pass.clone();
        let log = log.clone();
        cx.child(column_at(50.0, 50.0, move |cx| {
            let l = log.clone();
            cx.child(text_signal(move || l.get()));

            let n1 = name.clone();
            let n2 = name.clone();
            cx.child(text_field(move || n1.get(), move |t| n2.set(t)));

            let p1 = pass.clone();
            let p2 = pass.clone();
            cx.child(text_field_opts(
                move || p1.get(),
                move |t| p2.set(t),
                TextFieldOpts {
                    width: 220.0,
                    height: 32.0,
                    placeholder: "密码".into(),
                    password: true,
                    ..Default::default()
                },
            ));

            let l = log.clone();
            let n = name.clone();
            let p = pass.clone();
            cx.child(button(
                move || "显示输入".into(),
                move || {
                    let mut s = String::new();
                    s.push_str(&format!("name={}\n", n.get()));
                    s.push_str(&format!("pass={}\n", p.get()));
                    l.set(s);
                },
            ));
        }));
    });

    wy_engine::runner::run(app)
}

/// 主标签（简单文本信号 → Scene 文本图元）。
fn text_signal(label: impl Fn() -> String + 'static) -> Node {
    Node {
        draw_fn: Rc::new(move |scene| {
            let s = label();
            if let Some(scene) = scene.downcast_mut::<Scene>() {
                scene.draw_text(Point::new(0.0, 0.0), &s, 14.0, Color::BLACK);
            }
        }),
        ..Node::default()
    }
}