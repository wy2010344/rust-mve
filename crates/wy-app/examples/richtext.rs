//! RichEditableTextNode 全链路示例：`rsx!` 声明式写法 + 焦点 + 键盘/IME + 样式操作。
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
//!
//! 编辑器构造返回 `(Node, 共享内核)` 元组（按钮要直接拿内核 `style_range`），
//! 无法走 rsx! 的单节点组件调用，故保留函数式构造；其余节点用 `rsx!`。

use std::rc::Rc;

use wy_mve::{column_at, rich_editable_opts, rich_text_opts, rsx, text_styled, RichTextOpts};
use wy_render::Color;
use wy_signal::{GetValue, SetValue, Signal};
use wy_text::TextStyle;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let doc = Signal::new(String::new());
    let log = Signal::new(String::new());
    let pass = Signal::new(String::from("secret123"));

    let app = wy_engine::mve_integration::MveApp::new(move |cx| {
        cx.child(column_at(30.0, 30.0, move |cx| {
            // 编辑器在闭包内构造（返回 `(Node, 共享内核)` 元组，局部变量才能 move 进树）
            let editor_opts = RichTextOpts {
                width: 400.0,
                height: 100.0,
                font_size: 15.0,
                placeholder: "点击此处输入...".into(),
                ..Default::default()
            };
            let d1 = doc;
            let d2 = doc;
            let (editor_node, core_rc) =
                rich_editable_opts(move || d1.get(), move |t| d2.set(t), editor_opts);
            let c1 = core_rc;

            cx.child(text_styled(
                || "Rich Text 编辑器 Demo".into(),
                18.0,
                Color::BLACK,
            ));
            cx.child(editor_node);

            // 样式按钮行 + 密码区 + 日志：同一列
            cx.child(
                rsx! {
                    vbox {
                        // 显式 8.0：对齐外层 `column()` 简写的间距契约
                        // （Node 默认 gap 已是 0，与 Kotlin 上游一致）
                        gap: 8.0,
                        hbox {
                            gap: 8.0,
                            // `let` 混入 rsx：逐按钮克隆共享内核（Rc 非 Copy）
                            let c = Rc::clone(&c1);
                            btn {
                                label: "红色",
                                on_click: move || {
                                    let mut core = c.borrow_mut();
                                    let (s, e) = (core.sel_start(), core.sel_end());
                                    if s < e {
                                        core.style_range(
                                            s,
                                            e,
                                            Some(TextStyle::normal().with_color(0xFFFF0000)),
                                        );
                                    }
                                    log.set("已应用红色".into());
                                }
                            }
                            let c = Rc::clone(&c1);
                            btn {
                                label: "蓝色",
                                on_click: move || {
                                    let mut core = c.borrow_mut();
                                    let (s, e) = (core.sel_start(), core.sel_end());
                                    if s < e {
                                        core.style_range(
                                            s,
                                            e,
                                            Some(TextStyle::normal().with_color(0xFF0000FF)),
                                        );
                                    }
                                }
                            }
                            let c = Rc::clone(&c1);
                            btn {
                                label: "清除样式",
                                on_click: move || {
                                    let mut core = c.borrow_mut();
                                    let (s, e) = (core.sel_start(), core.sel_end());
                                    if s < e {
                                        core.style_range(s, e, None);
                                    }
                                }
                            }
                            let c = Rc::clone(&c1);
                            btn {
                                label: "撤销",
                                on_click: move || c.borrow_mut().undo(),
                            }
                            let c = Rc::clone(&c1);
                            btn {
                                label: "重做",
                                on_click: move || c.borrow_mut().redo(),
                            }
                        }

                        label("密码框:")
                        rich_text_opts(
                            move || pass.get(),
                            move |t| pass.set(t),
                            RichTextOpts {
                                width: 200.0,
                                height: 32.0,
                                placeholder: "密码".into(),
                                password: true,
                                ..Default::default()
                            }
                        )
                        label(log.get())
                    }
                }
                .into_node(),
            );
        }));
    });

    wy_engine::runner::run(app)
}
