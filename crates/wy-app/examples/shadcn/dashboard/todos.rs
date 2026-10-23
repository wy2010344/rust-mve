//! 待办清单卡片：筛选 tabs + 新待办输入 + checkbox 行（勾选/删除）+ 空态骨架屏。
//!
//! 列表放进滚动容器并 `grow: 1.0` 填满卡片剩余高度——条目多时视口内滚动
//! （悬浮滚动条 + 溢出裁剪），卡片高度始终与表单卡等高，底边对齐。
//!
//! # 信号模型（per-item，不是 React）
//!
//! - `App::todo_ids` 只管**结构**（有哪些项、什么顺序）；
//! - 每项的「完成」是**独立信号**（`App::todo_store`），勾选只写那一个；
//! - 行内文本/勾选态在**绘制期**读自己的信号 → 只有那一行重绘。
//!
//! 早先用一个 `Signal<Vec<Todo>>` 装下整个列表，勾选一次要重写整个 Vec，
//! 整张列表跟着重算——那是 React 式的粗粒度更新。
//!
//! # 行高固定
//!
//! 每行高度固定 [`ROW_H`]，不随卡片/窗口高度变化——窗口变高时多出来的
//! 空间归滚动容器，行的疏密保持一致（不然界面忽疏忽忽密，看着很怪）。

use std::rc::Rc;

use wy_mve::{for_each, rsx, scroll, spacer, AlignItem, Icon, Node, Slot};
use wy_signal::{GetValue, SetValue, Signal};

use crate::state::App;
use crate::tokens::Theme;
use crate::widgets::{button_sized, card, checkbox, separator, skeleton, tabs, text_label, Btn};

use super::input_field;

/// 单行高度（固定，不随容器高度变化）。
pub(super) const ROW_H: f32 = 30.0;

/// 单条待办：checkbox + 文本 + 删除。
///
/// 文本与勾选态都在**绘制期**读该项自己的信号——勾选只让这一行重绘，
/// 其余行不受影响（信号驱动的局部更新，不是整表重渲染）。
fn todo_row(
    theme: Theme,
    ids: Signal<Vec<u32>>,
    id: u32,
    done: Signal<bool>,
    text: Signal<String>,
) -> Node {
    let row_done = done;
    let row_text = text;
    let store_done = done;

    rsx! {
        hbox {
            gap: 10.0,
            height: ROW_H,
            // 交叉轴（垂直）居中：checkbox 与文字基线对齐
            align_items: AlignItem::Center,
            checkbox {
                theme: theme,
                checked: move || row_done.get(),
                on_change: move |v| store_done.set(v),
            }
            text_label {
                content: move || row_text.get(),
                color: {
                    let row_done2 = done;
                    move || {
                        if row_done2.get() {
                            theme.tokens().muted_fg
                        } else {
                            theme.tokens().foreground
                        }
                    }
                },
            }
            // 删除按钮推到行尾
            spacer(0.0, 0.0)
            spacer(0.0, 0.0)
            button_sized {
                theme: theme,
                label: "",
                variant: Btn::Ghost,
                w: 26.0,
                h: 24.0,
                on_click: move || {
                    // 删除只改**结构**信号（ids），不动其它项的状态
                    let mut l = ids.get();
                    l.retain(|x| *x != id);
                    ids.set(l);
                },
                icon: Some(Icon::X),
            }
        }
    }
    .into_node()
}

pub(super) fn todos_card(theme: Theme, app: &App) -> Node {
    let ids = app.todo_ids;
    let next_id = app.next_id;
    let filter = app.todo_filter;
    let input = app.todo_input;

    // 空态：骨架占位 + 提示（列表非空时隐藏）
    let mut empty_tip = rsx! {
        vbox {
            gap: 8.0,
            skeleton { theme: theme, h: 14.0 }
            skeleton { theme: theme, h: 14.0 }
            text_label { content: "暂无待办，添加一个吧", color: move || theme.tokens().muted_fg }
        }
    }
    .into_node();
    empty_tip.hide_fn = Some(Rc::new(move || !ids.get().is_empty()));

    // 列表用 `for_each`（keyed）：节点树只建一次，增删项只动对应 holder。
    //
    // **筛选在 memo 内读取**：`filter.get()` 必须写在 `for_each` 的源闭包里
    // （memo 求值时才会执行），否则 memo 追踪不到它——改筛选不重算，
    // 表现为「筛选无效」。
    let visible = {
        let ids = app.todo_ids;
        let filter = app.todo_filter;
        let store = app.todo_store.clone();
        move || {
            let f = filter.get();
            let all = ids.get();
            all.into_iter()
                .filter(|id| {
                    let done = store.done_get(*id);
                    match f {
                        1 => !done,
                        2 => done,
                        _ => true,
                    }
                })
                .collect::<Vec<u32>>()
        }
    };

    let list = {
        // 闭包要 'static：把 `app` 里的字段拷出来（Signal 是 Copy 句柄，
        // store 是 Rc 共享），不捕获 `&App` 引用。
        let store = app.todo_store.clone();
        let ids_sig = app.todo_ids;
        move || {
            let mut v: Vec<Slot> = Vec::new();
            v.push(for_each(
                move |emit| {
                    for id in visible() {
                        emit(id, id);
                    }
                },
                move |_k, id_rc, _cx| {
                    let id = *id_rc.borrow();
                    // 每项的 per-item 信号：从 store 取（首次创建，后续复用）
                    let done = store.done_of(id);
                    let text = store.text_of(id);
                    _cx.child(todo_row(theme, ids_sig, id, done, text));
                    _cx.child(rsx! { separator { theme: theme } }.into_node());
                },
            ));
            v.push(empty_tip.into());
            rsx! {
                vbox {
                    gap: 0.0,
                    // 关键：默认对齐是 Center，会让每条记录**居中**（而不是左对齐铺满）
                    align_items: AlignItem::Stretch,
                    ..v
                }
            }
            .into_node()
        }
    };

    rsx! {
        // 尺寸完全交给 flex（同表单卡）：等宽等高、行高填满剩余窗口
        card {
            theme: theme,
            grow: 1.0,
            size: (0.0, 0.0),
            // 标题行：标题 + 计数（信号驱动）
            hbox {
                gap: 8.0,
                align_items: AlignItem::Center,
                text_label { content: "待办清单", font_size: 17.0, color: move || theme.tokens().foreground }
                spacer(8.0, 0.0)
                text_label {
                    content: {
                        let ids = app.todo_ids;
                        let store = app.todo_store.clone();
                        move || {
                            let l = ids.get();
                            let done = l.iter().filter(|id| store.done_get(**id)).count();
                            format!("共 {} 项 · 完成 {}", l.len(), done)
                        }
                    },
                    color: move || theme.tokens().muted_fg,
                }
            }
            spacer(0.0, 12.0)
            // 筛选 tabs：全部 / 进行中 / 已完成
            tabs {
                theme: theme,
                items: vec![String::from("全部"), String::from("进行中"), String::from("已完成")],
                active: filter,
            }
            spacer(0.0, 8.0)
            hbox {
                gap: 8.0,
                align_items: AlignItem::Center,
                input_field(theme, input, "新的待办...", 1.0, "input-todo")
                button_sized {
                    theme: theme,
                    label: "添加",
                    variant: Btn::Secondary,
                    w: 56.0, h: 34.0,
                    tag: "todo-add",
                    on_click: {
                        let store = app.todo_store.clone();
                        let ids = app.todo_ids;
                        move || {
                            let text = input.get().trim().to_string();
                            if !text.is_empty() {
                                let id = next_id.get();
                                next_id.set(id + 1);
                                // 新项的 per-item 信号建好后写初值
                                let done = store.done_of(id);
                                done.set(false);
                                let t = store.text_of(id);
                                t.set(text);
                                let mut l = ids.get();
                                l.push(id);
                                ids.set(l);
                                input.set(String::new());
                            }
                        }
                    },
                }
            }
            spacer(0.0, 12.0)
            // 列表滚动容器：grow 填满卡片剩余高度（行高固定，不随条目数变化）
            scroll {
                h: 48.0,
                grow: 1.0,
                gap: 0.0,
                list()
            }
        }
    }
    .into_node()
}
