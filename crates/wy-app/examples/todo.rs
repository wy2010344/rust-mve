//! Todo List 示例：`renderForEach` + 每项独立信号。
//!
//! 运行：`cargo run -p wy-app --example todo`
//!
//! 复刻 Kotlin `DemoList.kt` 的 MVE 模式：
//! - 列表用 `cx.render_for_each`，key = item id，holder 按 key 复用（构建一次）；
//! - 每项在 creater 里 `create_signal` 自己的状态，draw / 事件只读写项信号 →
//!   增删不改项信号，改项不重列列表，天然局部重绘；
//! - 删除写回主列表，holder 随 key 移除而销毁。

use wy_mve::{button, column_at, row, text_signal};
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

        cx.child(column_at(300.0, 50.0, move |cx| {
            let add_todos = add_todos.clone();
            let candidates = candidates.clone();
            let todos_for_del = todos_for_del.clone();

            // 添加按钮：写主列表
            cx.child(button(
                move || "+ Add".into(),
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

            // 列表：按 key（=id）复用 holder
            cx.render_for_each(
                move |emit| {
                    for todo in candidates.get() {
                        emit(todo.id, todo);
                    }
                },
                move |id, value, item_cx| {
                    // 每项一个信号：项的 draw / 事件都读它 → 该项局部重绘
                    let item = create_signal(value.borrow().clone());

                    let label = item.clone();
                    let toggle = item.clone();
                    let text_item = item.clone();
                    let del_todos = todos_for_del.clone();
                    item_cx.child(row(move |cx| {
                        let label = label.clone();
                        let toggle = toggle.clone();
                        let text_item = text_item.clone();
                        let del_todos = del_todos.clone();

                        // 切换完成：写项信号（不触碰主列表）
                        cx.child(button(
                            move || {
                                if label.get().done {
                                    "[x]".into()
                                } else {
                                    "[ ]".into()
                                }
                            },
                            move || {
                                let mut t = toggle.get();
                                t.done = !t.done;
                                toggle.set(t);
                            },
                        ));

                        // 文本：draw 期读项信号
                        cx.child(text_signal(move || {
                            let t = text_item.get();
                            let prefix = if t.done { "✓ " } else { "" };
                            Box::new(format!("{prefix}{}", t.text))
                        }));

                        // 删除：写主列表 → 该项 holder 被销毁
                        cx.child(button(
                            move || "×".into(),
                            move || {
                                let mut list = del_todos.get();
                                list.retain(|t| t.id != id);
                                del_todos.set(list);
                            },
                        ));
                    }));
                },
            );
        }));
    });

    wy_engine::runner::run(app)
}
