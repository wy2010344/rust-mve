//! 应用状态：全部用信号承载，事件只写信号，绘制期读取自动追踪。
//!
//! # 待办项为什么要拆成「结构信号 + 每项信号」
//!
//! 早先的写法是 `todos: Signal<Vec<Todo>>`——一个信号装下整个列表。
//! 勾选其中一项要 `todos.set(整个新 Vec)`，于是：
//! - 列表 memo 的依赖（整个 Vec）变了 → 整张列表重算；
//! - 所有项的 `done` 都"可能变了" → 无法只重绘那一行。
//!
//! 这正是 **React 式**的写法：改一个状态，整棵树重渲染。
//! MVE 的模型应该是**信号驱动到最小粒度**：
//! - `todo_ids: Signal<Vec<u32>>` 只管**结构**（有哪些项、什么顺序）；
//! - `todo_done: Signal<HashMap<u32, Signal<bool>>>` 每项一个独立信号，
//!   勾选只写那一个信号 → 只有那一行的绘制闭包失效。
//!
//! 这样「勾选一项」的影响面就精确到一行，和 Kotlin MVE 的
//! `EachValue` + per-item signal 一致。

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use wy_signal::{GetValue, SetValue, Signal};

/// 待办列表的状态容器。
///
/// 每项的完成状态是**独立信号**（id → 信号），首次创建该 id 时建好、
/// 之后一直复用——这样 `for_each` 的 holder 与 per-item 信号一一对应，
/// 勾选只重绘那一行。
///
/// 容器本身用 `Rc<RefCell<..>>` 共享：构建期要往里**新增**项的信号
/// （新增待办），而持有者只拿到 `&App`——可变共享比整份状态塞进一个
/// 信号更贴合 MVE 的粒度。
#[derive(Clone, Default)]
pub struct TodoStore {
    inner: Rc<RefCell<HashMap<u32, ItemSignals>>>,
}

/// 单项的两个信号。
#[derive(Clone)]
struct ItemSignals {
    done: Signal<bool>,
    text: Signal<String>,
}

impl TodoStore {
    /// 取（或建）某项的完成状态信号。
    pub fn done_of(&self, id: u32) -> Signal<bool> {
        self.entry(id).done
    }

    /// 取（或建）某项的文本信号。
    pub fn text_of(&self, id: u32) -> Signal<String> {
        self.entry(id).text
    }

    /// 取（或建）某项的信号组。
    fn entry(&self, id: u32) -> ItemSignals {
        let mut map = self.inner.borrow_mut();
        map.entry(id)
            .or_insert_with(|| ItemSignals {
                done: Signal::new(false),
                text: Signal::new(String::new()),
            })
            .clone()
    }

    /// 移除某项的信号（删除项时清理）。
    #[allow(dead_code)]
    pub fn remove(&self, id: u32) {
        self.inner.borrow_mut().remove(&id);
    }

    /// 某项的完成状态（只读，不建信号）。
    pub fn done_get(&self, id: u32) -> bool {
        self.inner
            .borrow()
            .get(&id)
            .map(|s| s.done.get())
            .unwrap_or(false)
    }
}

pub struct App {
    pub theme_dark: Signal<bool>,
    pub name: Signal<String>,
    pub email: Signal<String>,
    pub message: Signal<String>,
    pub status: Signal<String>,
    /// 待办项的**结构**（id 顺序）：只随增删变化。
    pub todo_ids: Signal<Vec<u32>>,
    /// 每项的 per-item 信号（完成态 / 文本）：勾选只写其中一项。
    pub todo_store: TodoStore,
    pub next_id: Signal<u32>,
    pub visits: Signal<u32>,
    /// 模态对话框开关。
    pub dialog_open: Signal<bool>,
    /// 下拉菜单开关 + 面板锚点（触发按钮的窗口坐标，事件期捕获）。
    pub menu_open: Signal<bool>,
    pub menu_anchor: Signal<(f32, f32)>,
    /// 待办筛选：0 全部 / 1 进行中 / 2 已完成。
    pub todo_filter: Signal<usize>,
    /// 待办输入框的当前文本（新增待办的草稿）。
    pub todo_input: Signal<String>,
}

impl App {
    pub fn new() -> Self {
        let store = TodoStore::default();
        let seed = [
            (1u32, String::from("设计系统 tokens"), true),
            (2u32, String::from("rsx 宏记录式参数"), true),
            (3u32, String::from("shadcn 仪表盘示例"), false),
        ];
        let mut ids = Vec::new();
        for (id, text, done) in seed {
            ids.push(id);
            store.done_of(id).set(done);
            store.text_of(id).set(text);
        }
        Self {
            theme_dark: Signal::new(false),
            name: Signal::new(String::new()),
            email: Signal::new(String::new()),
            message: Signal::new(String::new()),
            status: Signal::new(String::from("填写表单后提交")),
            todo_ids: Signal::new(ids),
            todo_store: store,
            next_id: Signal::new(4),
            visits: Signal::new(128),
            dialog_open: Signal::new(false),
            menu_open: Signal::new(false),
            menu_anchor: Signal::new((0.0, 0.0)),
            todo_filter: Signal::new(0),
            todo_input: Signal::new(String::new()),
        }
    }

    /// 当前筛选下应显示的待办 id（按结构顺序）。
    #[allow(dead_code)]
    ///
    /// 读取 `todo_filter` 与各项的 `done` 信号——**在 memo 内调用**
    /// 才能被正确追踪（否则改筛选不触发重算，"筛选无效"）。
    pub fn visible_todo_ids(&self) -> Vec<u32> {
        let filter = self.todo_filter.get();
        self.todo_ids
            .get()
            .into_iter()
            .filter(|id| {
                let done = self.todo_store.done_get(*id);
                match filter {
                    1 => !done,
                    2 => done,
                    _ => true,
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seed_values_persist_in_store() {
        let app = App::new();
        // 种子数据：1、2 已完成，3 未完成
        assert!(app.todo_store.done_get(1), "id=1 应为已完成");
        assert!(app.todo_store.done_get(2), "id=2 应为已完成");
        assert!(!app.todo_store.done_get(3), "id=3 应为未完成");
        assert_eq!(app.todo_store.text_of(3).get(), "shadcn 仪表盘示例");
    }

    #[test]
    fn visible_ids_respect_filter() {
        let app = App::new();
        assert_eq!(app.visible_todo_ids().len(), 3, "全部：3 项");
        app.todo_filter.set(1);
        assert_eq!(app.visible_todo_ids().len(), 1, "进行中：1 项");
        app.todo_filter.set(2);
        assert_eq!(app.visible_todo_ids().len(), 2, "已完成：2 项");
    }

    #[test]
    fn per_item_signal_only_affects_that_item() {
        let app = App::new();
        let done = app.todo_store.done_of(3);
        done.set(true);
        assert!(app.todo_store.done_get(3));
        assert!(app.todo_store.done_get(1), "其它项不受影响");
        assert!(app.todo_store.done_get(2), "其它项不受影响");
    }
}
