//! RichEditableTextNode 全链路示例：MVE + 焦点 + 键盘/IME + 样式操作。
//!
//! 运行：`cargo run -p wy-app --example richtext`
//!
//! 演示：
//! - 多行富文本编辑器：选中文字后点击"红色"/"蓝色"按钮，观察选区颜色变化；
//! - 继续打字/退格时，样式自动继承（新字符继承插入点左侧样式）；
//! - Ctrl+Z/Y 撤销重做（样式段和文本一起还原）；
//! - Ctrl+←/→ 词级跳转，Home/End 行首/行尾；
//! - 密码输入框（掩码 `•` 显示）；
//! - 实时日志输出当前文本。

use std::rc::Rc;

use wy_mve::{button, column_at, rich_editable_opts, rich_text_opts, row, text, Node, RichTextOpts};
use wy_render::{Color, Point, Scene};
use wy_signal::{GetValue, SetValue, Signal};
use wy_text::TextStyle;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let doc = Signal::new(String::new());
    let log = Signal::new(String::new());
    let pass = Signal::new(String::from("secret123"));

    let app = wy_engine::mve_integration::MveApp::new(move |cx| {
        let doc = doc.clone();
        let log = log.clone();
        let pass = pass.clone();
        cx.child(column_at(30.0, 30.0, move |cx| {
            // ---- 标题 ----
            cx.child(Node {
                draw_fn: Rc::new(|scene| {
                    if let Some(s) = scene.downcast_mut::<Scene>() {
                        s.draw_text(
                            Point::new(0.0, 0.0),
                            "Rich Text 编辑器 Demo",
                            18.0,
                            Color::BLACK,
                        );
                    }
                }),
                width: 200.0,
                height: 24.0,
                ..Node::default()
            });

            // ---- 多行富文本编辑器（共享内核句柄 → 按钮可直接 style_range） ----
            let editor_opts = RichTextOpts {
                width: 400.0,
                height: 100.0,
                font_size: 15.0,
                placeholder: "点击此处输入...".into(),
                ..Default::default()
            };
            let d1 = doc.clone();
            let d2 = doc.clone();
            let (editor_node, core_rc) = rich_editable_opts(
                move || d1.get(),
                move |t| d2.set(t),
                editor_opts,
            );
            cx.child(editor_node);

            // ---- 样式按钮行 ----
            let c1 = core_rc.clone();
            let d_log = log.clone();
            cx.child(row(move |cx| {
                // 红色：把选区样式设为红色
                let c = Rc::clone(&c1);
                let lg = d_log.clone();
                cx.child(button(move || "红色".into(), move || {
                    {
                        let mut core = c.borrow_mut();
                        let (s, e) = (core.sel_start(), core.sel_end());
                        if s < e {
                            core.buffer_mut().style_range(
                                s,
                                e,
                                Some(TextStyle::normal().with_color(0xFFFF0000)),
                            );
                        }
                    }
                    lg.set("已应用红色".into());
                }));

                // 蓝色
                let c = Rc::clone(&c1);
                cx.child(button(move || "蓝色".into(), move || {
                    let mut core = c.borrow_mut();
                    let (s, e) = (core.sel_start(), core.sel_end());
                    if s < e {
                        core.buffer_mut().style_range(
                            s,
                            e,
                            Some(TextStyle::normal().with_color(0xFF0000FF)),
                        );
                    }
                }));

                // 清除选区样式（回退基础样式）
                let c = Rc::clone(&c1);
                cx.child(button(move || "清除样式".into(), move || {
                    let mut core = c.borrow_mut();
                    let (s, e) = (core.sel_start(), core.sel_end());
                    if s < e {
                        core.buffer_mut().style_range(s, e, None);
                    }
                }));

                // 撤销
                let c = Rc::clone(&c1);
                cx.child(button(move || "撤销".into(), move || {
                    c.borrow_mut().undo();
                }));

                // 重做
                let c = Rc::clone(&c1);
                cx.child(button(move || "重做".into(), move || {
                    c.borrow_mut().redo();
                }));
            }));

            // ---- 密码框标签 ----
            cx.child(text(|| "密码框:".into()));

            // ---- 密码输入 ----
            let p1 = pass.clone();
            let p2 = pass.clone();
            cx.child(rich_text_opts(
                move || p1.get(),
                move |t| p2.set(t),
                RichTextOpts {
                    width: 200.0,
                    height: 32.0,
                    placeholder: "密码".into(),
                    password: true,
                    ..Default::default()
                },
            ));

            // ---- 日志 ----
            let log_sig = log.clone();
            cx.child(text(move || log_sig.get()));
        }));
    });

    wy_engine::runner::run(app)
}