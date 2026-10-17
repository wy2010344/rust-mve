//! 待办清单卡片：新待办输入 + 添加按钮 + 带 `for_each` 的列表 / 空态提示。

use std::cell::RefCell;
use std::rc::Rc;

use wy_mve::{for_each, rsx, spacer, Node, Slot};
use wy_signal::{GetValue, SetValue, Signal};

use crate::state::{App, Todo};
use crate::tokens::{Theme, Tokens};
use crate::widgets::{badge, button, button_sized, card, text_label, Btn};

use super::{input_field, CARD2_H, CARD2_W};

/// 单行待办：徽章 + 文本都经闭包在绘制期读**活动 item**（`Rc<RefCell<Todo>>`），
/// 而非构建期快照——`for_each` 只对新增 key 建一次 holder，复用项靠这些 lambda 刷新。
fn todo_row(theme: Theme, todos: Signal<Vec<Todo>>, todo: Rc<RefCell<Todo>>) -> Node {
    let id = todo.borrow().id;
    rsx! {
        hbox {
            gap: 0.0,
            badge {
                theme: theme,
                text: {
                    let todo = Rc::clone(&todo);
                    move || {
                        if todo.borrow().done {
                            String::from("完成")
                        } else {
                            String::from("进行中")
                        }
                    }
                },
                variant: {
                    let todo = Rc::clone(&todo);
                    move || if todo.borrow().done { 1u8 } else { 0u8 }
                },
            }
            spacer(8.0, 0.0)
            text_label {
                content: {
                    let todo = Rc::clone(&todo);
                    move || todo.borrow().text.clone()
                },
                theme: theme,
                color: {
                    let todo = Rc::clone(&todo);
                    move |t: &Tokens| {
                        if todo.borrow().done {
                            t.muted_fg
                        } else {
                            t.foreground
                        }
                    }
                },
            }
            button_sized {
                theme: theme,
                label: move || String::from("删除"),
                variant: Btn::Destructive,
                w: 48.0,
                h: 26.0,
                on_click: move || {
                    let mut l = todos.get();
                    l.retain(|x| x.id != id);
                    todos.set(l);
                },
            }
        }
    }
    .into_node()
}

pub(super) fn todos_card(theme: Theme, app: &App) -> Node {
    let todos = app.todos;
    let next_id = app.next_id;
    let input = Signal::new(String::new());

    // 条件展示：空列表时才显示提示。隐藏节点不占布局空间（`node_size` 已过滤
    // `is_hidden`），故一个 `hide_fn` 即可动态显隐并触发重排。
    let mut empty_tip = rsx! {
        text_label { content: move || String::from("暂无待办，添加一个吧"), theme: theme, color: |t| t.muted_fg }
    }
    .into_node();
    empty_tip.hide_fn = Some(Rc::new(move || !todos.get().is_empty()));

    // 列表用 `for_each`（keyed）：节点树只建一次，增删项只动对应 holder。
    // 包成闭包 `list()` 便于作为单个子节点注入 `card` 的插槽（不必用块表达式）。
    let list = move || {
        let mut v: Vec<Slot> = Vec::new();
        v.push(for_each(
            move |emit| {
                for todo in todos.get() {
                    emit(todo.id, todo.clone());
                }
            },
            move |_k, todo_rc, _cx| {
                _cx.child(todo_row(theme, todos, todo_rc));
                _cx.child(spacer(0.0, 6.0));
            },
        ));
        v.push(empty_tip.into());
        wy_mve::vbox(v)
    };

    rsx! {
        card {
            theme: theme,
            w: CARD2_W,
            h: CARD2_H,
            text_label { content: move || String::from("待办清单"), size: 17.0, theme: theme, color: |t| t.foreground }
            spacer(0.0, 12.0)
            input_field(theme, input, "新的待办...")
            spacer(0.0, 8.0)
            button {
                theme: theme,
                label: move || String::from("添加"),
                variant: Btn::Secondary,
                on_click: move || {
                    let text = input.get().trim().to_string();
                    if !text.is_empty() {
                        let mut l = todos.get();
                        let id = next_id.get();
                        next_id.set(id + 1);
                        l.push(Todo { id, text, done: false });
                        todos.set(l);
                        input.set(String::new());
                    }
                },
            }
            spacer(0.0, 12.0)
            list()
        }
    }
    .into_node()
}
