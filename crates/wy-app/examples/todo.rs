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
    // id 单调递增，永不复用：删除中间项后新 id 不会与已存在的项冲突，
    // for_each 的 key 才始终唯一（用 list.len() 会重复）。
    let next_id = Signal::new(0usize);

    let app = wy_engine::mve_integration::MveApp::new(move |cx| {
        let node = rsx! {
            vbox_at(300.0, 50.0) {
                btn {
                    label: "+ Add",
                    on_click: move || {
                        let id = next_id.get();
                        next_id.set(id + 1);
                        let mut list = todos.get();
                        list.push(Todo {
                            id,
                            text: format!("Item {}", id + 1),
                            done: false,
                        });
                        todos.set(list);
                    }
                }

                for_each(todos, |t: &Todo| t.id) { |todo_ref, _cx| {
                    // 每项一个信号：项的 draw / 事件都读它 → 该项局部重绘
                    // Signal 是 Copy，下面多处闭包可直接共用，无需 clone。
                    let item = create_signal(todo_ref.borrow().clone());

                    hbox {
                        btn {
                            label: if item.get().done { "[x]" } else { "[ ]" },
                            on_click: move || {
                                let mut t = item.get();
                                t.done = !t.done;
                                item.set(t);
                            }
                        }
                        label(format!("{}{}", if item.get().done { "✓ " } else { "" },
                                     item.get().text))
                        btn {
                            label: "×",
                            on_click: move || {
                                let mut list = todos.get();
                                list.retain(|t| t.id != todo_ref.borrow().id);
                                todos.set(list);
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
