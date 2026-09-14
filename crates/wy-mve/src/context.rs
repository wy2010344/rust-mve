//! `NodeContext`：构建树的上下文 + `render_root` 入口。
//!
//! # 构建一次模型
//!
//! `render_root(callback)` 一次性执行 callback，把顶层节点存成静态 slots；
//! 动态列表区域用 [`render_for_each`] 封成 memo，只在信号变化时重算。
//! 顶层不可变 `slots` 与列表 memo 由 `Root` 的 `target` memo 合成当前扁平节点列表，
//! 结构与 Kotlin `TargetStateHolder.renderRoot` 的 `target` 一致。

use std::any::{Any, TypeId};
use std::cell::RefCell;
use std::rc::Rc;

use wy_signal::{GetValue, Memo};

use crate::foreach::render_for_each;
use crate::node::{flatten, ChildSlot, Node};

/// 节点树的构建上下文（对应 Kotlin `StateHolderI.buildChildren` 的容器）。
#[derive(Default)]
pub struct NodeContext {
    pub(crate) slots: Vec<ChildSlot>,
    /// 提供者注册表：`type_id -> value`。
    pub(crate) provided: Vec<(TypeId, Box<dyn Any>)>,
}

impl NodeContext {
    /// 创建空上下文。
    pub fn new() -> Self {
        Self::default()
    }

    /// 追加快照节点。
    pub fn child(&mut self, node: Node) {
        self.slots.push(ChildSlot::Node(node));
    }

    /// 追加快照节点（`add_node` 别名）。
    pub fn add_node(&mut self, node: Node) {
        self.child(node);
    }

    /// 追加动态区域：按 key 复用的列表渲染。
    ///
    /// 对应 Kotlin `StateHolderI.renderForEach`。见 [`render_for_each`]。
    pub fn render_for_each<K, T>(
        &mut self,
        for_each: impl Fn(&mut dyn FnMut(K, T)) + 'static,
        creater: impl Fn(K, Rc<RefCell<T>>, &mut NodeContext) + 'static,
    ) where
        K: Eq + std::hash::Hash + Clone + 'static,
        T: Clone + PartialEq + 'static,
    {
        render_for_each(&mut self.slots, for_each, creater);
    }

    /// 提供值：构建该节点子树时可供兄弟节点通过 [`Self::consume`] 读取。
    pub fn provide<T: 'static>(&mut self, value: T) {
        self.provided.push((TypeId::of::<T>(), Box::new(value)));
    }

    /// 读取最近一次 [`Self::provide`] 的对应类型的值。
    pub fn consume<T: 'static>(&self) -> Option<&T> {
        self.provided.iter().rev().find_map(|(tid, v)| {
            if *tid == TypeId::of::<T>() {
                v.downcast_ref::<T>()
            } else {
                None
            }
        })
    }

    /// 取当前收集的 slots。
    pub fn slots(&self) -> &[ChildSlot] {
        &self.slots
    }

    /// 取当前收集的 slots（`slots` 别名，历史兼容）。
    pub fn nodes(&self) -> &[ChildSlot] {
        &self.slots
    }

    /// 取走收集的 slots。
    pub fn into_slots(self) -> Vec<ChildSlot> {
        self.slots
    }
}

/// `render_root(callback)` 的产物：持有目标 memo，给出当前节点列表。
///
/// 树结构构建一次；`target` 是"快照 + 动态区域"合成的扁平节点列表 memo，
/// 结构变化（列表增删）时重算，绘制期直接读取。
pub struct Root {
    /// 顶层节点列表 memo。
    target: Memo<Vec<Node>>,
}

impl Root {
    /// 当前扁平化节点列表（读取会自动追踪依赖）。
    pub fn nodes(&self) -> Vec<Node> {
        self.target.get()
    }

    /// 目标 memo（供宿主动画循环/追踪）。
    pub fn target(&self) -> Memo<Vec<Node>> {
        self.target.clone()
    }
}

/// 构建根节点树（复刻 Kotlin `TargetStateHolder.renderRoot`）。
///
/// - `callback` **只执行一次**，产物为顶层快照 slots 与动态列表区域。
/// - 返回的 `Root.target` 实时合成当前扁平节点列表。
pub fn render_root(callback: impl Fn(&mut NodeContext) + 'static) -> Root {
    let top: Rc<RefCell<Vec<ChildSlot>>> = Rc::new(RefCell::new(Vec::new()));
    {
        let mut cx = NodeContext::new();
        callback(&mut cx);
        *top.borrow_mut() = cx.into_slots();
    }

    let top2 = Rc::clone(&top);
    let target = Memo::new(move || {
        let slots = top2.borrow();
        let mut out = Vec::new();
        flatten(&slots, &mut out);
        out
    });

    Root { target }
}
