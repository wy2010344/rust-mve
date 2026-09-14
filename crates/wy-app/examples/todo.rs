//! Todo List 示例：函数组件 + 动态列表 + 信号驱动。
//!
//! 运行：`cargo run -p wy-app --example todo`

use wy_mve::{button, column_at, render_root, row, text_signal, NodeContext};
use wy_signal::{GetValue, SetValue, Signal};

#[derive(Clone, PartialEq)]
struct Todo {
    id: usize,
    text: String,
    done: bool,
}

fn todo_ui(cx: &mut NodeContext, todos: Signal<Vec<Todo>>) {
    cx.child(column_at(300.0, 50.0, move |cx| {
        cx.child(text_signal(|| Box::new("Todo List")));

        // 添加按钮
        let add_todos = todos.clone();
        cx.child(button(
            || "+ Add".into(),
            move || {
                let mut list = add_todos.get();
                let id = list.len();
                list.push(Todo {
                    id,
                    text: format!("Item {}", id + 1),
                    done: false,
                });
                add_todos.set(list);
            },
        ));

        // todo 列表：每次 tree 构造时读 todos.get()（被 tracker 追踪）
        let list = todos.get();
        for todo in &list {
            let t = todo.clone();
            let toggle_todos = todos.clone();
            let del_todos = todos.clone();
            let del_id = todo.id;

            // Signal clone 是 Rc 引用计数，廉价
            // 每个闭包需要 own 一个 handle（Rust 所有权要求）
            cx.child(row(move |cx| {
                // 切换完成状态
                let tt = toggle_todos.clone();
                let tt_id = t.id;
                let tt_done = t.done;
                cx.child(button(
                    move || {
                        if tt_done {
                            "[x]".into()
                        } else {
                            "[ ]".into()
                        }
                    },
                    move || {
                        let mut list = tt.get();
                        if let Some(item) = list.iter_mut().find(|t| t.id == tt_id) {
                            item.done = !item.done;
                        }
                        tt.set(list);
                    },
                ));

                // todo 文本
                let text = t.text.clone();
                let is_done = t.done;
                cx.child(text_signal(move || {
                    let prefix = if is_done { "✓ " } else { "" };
                    Box::new(format!("{}{}", prefix, text))
                }));

                // 删除按钮
                let dt = del_todos.clone();
                cx.child(button(
                    || "×".into(),
                    move || {
                        let list = dt.get();
                        let filtered: Vec<Todo> =
                            list.into_iter().filter(|t| t.id != del_id).collect();
                        dt.set(filtered);
                    },
                ));
            }));
        }
    }));
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let todos = Signal::new(Vec::<Todo>::new());
    let cache = render_root({
        let todos = todos.clone();
        move |cx| todo_ui(cx, todos.clone())
    });
    let app = wy_engine::mve_integration::MveApp::from_cache(cache);
    wy_engine::runner::run(app)
}
