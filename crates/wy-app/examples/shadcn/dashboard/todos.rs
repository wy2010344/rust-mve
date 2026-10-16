//! 待办清单卡片：新待办输入 + 添加按钮 + 带 `for_each` 的列表 / 空态提示。

use std::cell::RefCell;
use std::rc::Rc;

use wy_mve::{for_each, hbox, spacer, vbox, Node, Slot};
use wy_signal::{GetValue, SetValue, Signal};

use crate::state::{App, Todo};
use crate::tokens::Theme;
use crate::widgets::{badge, button, button_sized, card, text_label, Btn};

use super::{input_field, CARD2_H, CARD2_W};

/// 单行待办：徽章 + 文本都经闭包在绘制期读**活动 item**（`Rc<RefCell<Todo>>`），
/// 而非构建期快照——`for_each` 只对新增 key 建一次 holder，复用项靠这些 lambda 刷新。
fn todo_row(theme: Theme, todos: Signal<Vec<Todo>>, todo: Rc<RefCell<Todo>>) -> Node {
    let id = todo.borrow().id;
    let del = button_sized(
        theme,
        move || String::from("删除"),
        Btn::Destructive,
        48.0,
        26.0,
        move || {
            let mut l = todos.get();
            l.retain(|x| x.id != id);
            todos.set(l);
        },
    );

    let badge_text = Rc::clone(&todo);
    let badge_variant = Rc::clone(&todo);
    let label_text = Rc::clone(&todo);
    let label_fg = Rc::clone(&todo);
    hbox(vec![
        badge(
            theme,
            move || {
                if badge_text.borrow().done {
                    String::from("完成")
                } else {
                    String::from("进行中")
                }
            },
            move || if badge_variant.borrow().done { 1 } else { 0 },
        ),
        spacer(8.0, 0.0),
        text_label(
            move || label_text.borrow().text.clone(),
            14.0,
            theme,
            move |t| {
                if label_fg.borrow().done {
                    t.muted_fg
                } else {
                    t.foreground
                }
            },
        ),
        del,
    ])
}

pub(super) fn todos_card(theme: Theme, app: &App) -> Node {
    let todos = app.todos;
    let next_id = app.next_id;
    let input = Signal::new(String::new());
    let add = {
        let (todos, next_id, input) = (todos, next_id, input);
        button(
            theme,
            move || String::from("添加"),
            Btn::Secondary,
            move || {
                let text = input.get().trim().to_string();
                if !text.is_empty() {
                    let mut l = todos.get();
                    let id = next_id.get();
                    next_id.set(id + 1);
                    l.push(Todo {
                        id,
                        text,
                        done: false,
                    });
                    todos.set(l);
                    input.set(String::new());
                }
            },
        )
    };

    // 条件展示：空列表时才显示提示。隐藏节点不占布局空间（`node_size` 已过滤
    // `is_hidden`），故一个 `hide_fn` 即可动态显隐并触发重排，无需 `for_each`。
    let mut empty_tip = text_label(
        move || String::from("暂无待办，添加一个吧"),
        13.0,
        theme,
        |t| t.muted_fg,
    );
    empty_tip.hide_fn = Some(Rc::new(move || !todos.get().is_empty()));

    card(
        theme,
        CARD2_W,
        CARD2_H,
        vec![
            text_label(
                move || String::from("待办清单"),
                17.0,
                theme,
                |t| t.foreground,
            ),
            spacer(0.0, 12.0),
            input_field(theme, input, "新的待办..."),
            spacer(0.0, 8.0),
            add,
            spacer(0.0, 12.0),
            // 列表用 `for_each`（keyed）：节点树只建一次，增删项只动对应 holder。
            vbox({
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
                v
            }),
        ],
    )
}
