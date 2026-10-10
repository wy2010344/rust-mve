//! Todo List 示例：`rsx!` 声明式写法 + 每项独立信号。
//!
//! 运行：`cargo run -p wy-app --example todo`
//!
//! 复刻 Kotlin `DemoList.kt` 的 MVE 模式：
//! - 列表用 `for_each`，key = item id，holder 按 key 复用（构建一次）；
//! - 每项在 body 里 `create_signal` 自己的状态，draw / 事件只读写项信号 →
//!   增删不改项信号，改项不重列列表，天然局部重绘；
//! - 删除写回主列表，holder 随 key 移除而销毁。
//!
//! 对比 `demo.rs` 的函数式写法：这里用 `rsx!` 宏省掉闭包/逗号/`.into()`。

use wy_mve::rsx;
use wy_signal::{create_signal, GetValue, SetValue, Signal};

#[derive(Clone, PartialEq, Debug)]
struct Todo {
    id: usize,
    text: String,
    done: bool,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let todos = Signal::new(Vec::<Todo>::new());

    let app = wy_engine::mve_integration::MveApp::new(move |cx| {
        let todos = todos.clone();
        let add_todos = todos.clone();
        let candidates = todos.clone();
        let todos_for_del = todos.clone();

        let node = rsx! {
            vbox_at(300.0, 50.0) {
                btn {
                    label: "+ Add",
                    on_click: move || {
                        let mut list = add_todos.get();
                        let id = list.len();
                        list.push(Todo {
                            id,
                            text: format!("Item {}", id + 1),
                            done: false,
                        });
                        add_todos.set(list);
                    }
                }

                for_each(candidates, |t: &Todo| t.id) { |todo_ref, item_cx| {
                    // 每项一个信号：项的 draw / 事件都读它 → 该项局部重绘
                    let item = create_signal(todo_ref.borrow().clone());

                    let done_sig = item.clone();
                    let toggle = item.clone();
                    let text_item = item.clone();
                    let del_todos = todos_for_del.clone();

                    hbox {
                        btn {
                            label: if done_sig.get().done { "[x]" } else { "[ ]" },
                            on_click: move || {
                                let mut t = toggle.get();
                                t.done = !t.done;
                                toggle.set(t);
                            }
                        }
                        label(format!("{}{}", if text_item.get().done { "✓ " } else { "" },
                                     text_item.get().text))
                        btn {
                            label: "×",
                            on_click: move || {
                                let mut list = del_todos.get();
                                list.retain(|t| t.id != todo_ref.borrow().id);
                                del_todos.set(list);
                            }
                        }
                    }
                }}
            }
        };
        cx.child(node.into_node());
    });

    wy_engine::runner::run(app)
}
