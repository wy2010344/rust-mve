//! `renderForEach`：按 key 复用的列表渲染（复刻 Kotlin `EachValue`/`ForEachModal`）。
//!
//! # 模型
//!
//! - 列表是个 [`Memo`](wy_signal::Memo)：计算闭包内 `for_each(&mut |k, v| ...)` 产出的
//!   是 `ForEachModal`——本轮**存活**与**新建**的 holder、以及**过期**（复用不上的旧 holder）。
//! - 每个 holder（`EachInner`，复刻 `EachValue`）持有 key / value / index，并在**首次**被
//!   复用时调用一次 `creater` 构建子节点（闭包一次性执行，从不重建）。
//! - 树一旦建好，后续只在信号变化时重算 memo；per-item 的交互由每项内部信号
//!   在 `draw` 期读取驱动局部重绘，列表本身不再重建节点。
//! - 复用不上的旧 holder 走 `destroyed` 生命周期（Kotlin 的 `destroy()`）。

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use wy_signal::{GetValue, Memo};

use crate::context::NodeContext;
use crate::node::ChildSlot;

/// 每个列表项的 holder（复刻 Kotlin `EachValue`）。
struct EachInner<K, T> {
    key: K,
    value: Rc<RefCell<T>>,
    index: Cell<usize>,
    nodes: RefCell<Option<Vec<ChildSlot>>>,
    destroyed: Cell<bool>,
}

/// 跨轮保活的 key → holder 池。
type HolderPool<K, T> = Rc<RefCell<Vec<Rc<EachInner<K, T>>>>>;

/// 列表 memo 的输出（复刻 Kotlin `ForEachModal`）。
struct ForEachModal<K, T> {
    /// 本轮活跃（按顺序）holder。
    active: Vec<Rc<EachInner<K, T>>>,
    /// 本轮新建且尚未构建的 holder（→ 触发 `creater`）。
    fresh: Vec<Rc<EachInner<K, T>>>,
    /// 本轮复用不上、待销毁的旧 holder。
    stale: Vec<Rc<EachInner<K, T>>>,
}

impl<K, T> Clone for ForEachModal<K, T> {
    fn clone(&self) -> Self {
        Self {
            active: self.active.clone(),
            fresh: self.fresh.clone(),
            stale: self.stale.clone(),
        }
    }
}

impl<K: PartialEq, T: PartialEq> PartialEq for ForEachModal<K, T> {
    fn eq(&self, other: &Self) -> bool {
        self.active.len() == other.active.len()
            && self.active.iter().zip(other.active.iter()).all(|(a, b)| {
                a.key == b.key
                    && a.index.get() == b.index.get()
                    && *a.value.borrow() == *b.value.borrow()
            })
    }
}

/// 追加一个 `renderForEach` 动态区域到 `slots`。
///
/// - `for_each` 在列表 memo 求值时执行，读取源信号，并经 `emit(key, value)` 发出每一项。
/// - `creater` 在每个 key **首次**出现时执行，构建该项的子节点（只一次）。
///
/// key 复用：同一 key 的 holder 跨轮复用，其 `creater` 不重复执行，节点 identity 不变，
/// 从而 memo 短路、per-item 信号驱动局部重绘。
pub fn render_for_each<K, T>(
    slots: &mut Vec<ChildSlot>,
    for_each: impl Fn(&mut dyn FnMut(K, T)) + 'static,
    creater: impl Fn(K, Rc<RefCell<T>>, &mut NodeContext) + 'static,
) where
    K: Eq + std::hash::Hash + Clone + 'static,
    T: Clone + PartialEq + 'static,
{
    // 跨轮保活的 key → holder 池。
    let prev: HolderPool<K, T> = Rc::new(RefCell::new(Vec::new()));

    let list = Memo::new({
        let prev = Rc::clone(&prev);
        move || {
            // 按 key 分组上一轮 holder，本轮优先复用。
            let mut pool: HashMap<K, Vec<Rc<EachInner<K, T>>>> = HashMap::new();
            for e in prev.borrow().iter() {
                pool.entry(e.key.clone()).or_default().push(Rc::clone(e));
            }

            let mut active: Vec<Rc<EachInner<K, T>>> = Vec::new();
            let mut fresh: Vec<Rc<EachInner<K, T>>> = Vec::new();
            let mut index = 0usize;
            for_each(&mut |key, value| {
                let holder = match pool.get_mut(&key).and_then(|v| v.pop()) {
                    Some(e) => e,
                    None => {
                        let e = Rc::new(EachInner {
                            key: key.clone(),
                            value: Rc::new(RefCell::new(value.clone())),
                            index: Cell::new(index),
                            nodes: RefCell::new(None),
                            destroyed: Cell::new(false),
                        });
                        fresh.push(Rc::clone(&e));
                        e
                    }
                };
                *holder.value.borrow_mut() = value;
                holder.index.set(index);
                index += 1;
                active.push(holder);
            });

            let stale: Vec<Rc<EachInner<K, T>>> = pool.into_values().flatten().collect();
            *prev.borrow_mut() = active.clone();
            ForEachModal {
                active,
                fresh,
                stale,
            }
        }
    });

    // 复刻 Kotlin `EachValue` 生命周期：stale 销毁 + fresh 首次构建。
    let list_lifecycle = list.clone();
    list_lifecycle.after(move |modal| {
        for e in modal.stale.iter() {
            e.destroyed.set(true);
        }
        for e in modal.fresh.iter() {
            let mut cx = NodeContext::new();
            creater(e.key.clone(), Rc::clone(&e.value), &mut cx);
            *e.nodes.borrow_mut() = Some(cx.into_slots());
        }
    });

    // 动态区域：实时把活跃 holder 的节点展开成 `ChildSlot` 列表。
    let region = list.clone();
    let get_nodes: Rc<dyn Fn() -> Vec<ChildSlot>> = Rc::new(move || {
        let modal = region.get();
        let mut out = Vec::new();
        for e in modal.active.iter() {
            if let Some(ns) = e.nodes.borrow().as_ref() {
                out.extend(ns.iter().cloned());
            }
        }
        out
    });
    slots.push(ChildSlot::GetNodes(get_nodes));
}
