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

/// `provide` 的值表：`TypeId -> 值`。
///
/// 用 `Rc<dyn Any>` 而非 `Box<dyn Any>`：子树每次物化都要**继承父链的一份快照**，
/// `Rc` 让这份继承退化成指针复制，不必深拷贝值本身。
pub(crate) type Provided = Vec<(TypeId, Rc<dyn Any>)>;

/// 节点树的构建上下文（对应 Kotlin `StateHolderI.buildChildren` 的容器）。
#[derive(Default)]
pub struct NodeContext {
    pub(crate) slots: Vec<ChildSlot>,
    /// 提供者注册表：`type_id -> value`。
    pub(crate) provided: Provided,
}

impl NodeContext {
    /// 创建空上下文。
    pub fn new() -> Self {
        Self::default()
    }

    /// 开一个"继承父链 provide 快照"的子上下文。
    ///
    /// 动态区域（`render_for_each` / `render_when`）的每一项、以及容器物化
    /// 子节点（`materialize`）都用它：子上下文**看得见祖先 provide 的值**，
    /// 但自己在里面 `provide` 只影响自己这一支——继承的是快照，不是共享表。
    pub(crate) fn inherit(provided: &Provided) -> Self {
        Self {
            slots: Vec::new(),
            provided: provided.clone(),
        }
    }

    /// 组件作用域：继承本上下文的 provide 快照，slots 独立。
    ///
    /// 组件在这里构建自己的子树（可 `provide` 覆盖、可 `consume` 祖先的值），
    /// 结束后由 [`Self::adopt`] 把产物并入父上下文。这样组件内的 `provide`
    /// 不会泄漏给**后面的兄弟节点**——作用域是"自己这一支"。
    pub fn child_scope(&self) -> Self {
        Self::inherit(&self.provided)
    }

    /// 把 [`Self::child_scope`] 的产物并入本上下文（保持顺序）。
    pub fn adopt(&mut self, scope: NodeContext) {
        self.adopt_slots(scope.into_slots());
    }

    /// 并入一批**已经盖好 provide 快照**的 slot，不再重新盖章。
    ///
    /// 用于"在子作用域里加工过"的节点：它们在子作用域里已经带上那一层
    /// provide（组件自己 provide 的值），搬回父上下文时不能覆盖掉。
    pub fn adopt_slots(&mut self, slots: Vec<ChildSlot>) {
        self.slots.extend(slots);
    }

    /// 追加子节点。接受 [`Node`] 与 [`Slot`]（`label` / `vbox` / `for_each`
    /// 这些 DSL 产物都是 `Slot`），也接受 `Vec<Node>` / 数组。
    ///
    /// 对应 ooc 的 `ctx.addNode(..)`：`Slot::Many` 的动态区域**就地展开**成
    /// 兄弟槽位，不会被包成一个中间节点（否则会脱离父容器布局）。
    pub fn child(&mut self, slot: impl crate::fc::Children) {
        slot.push_slots(self);
    }

    /// 追加快照节点（`add_node` 别名，对齐 ooc 的 `ctx.addNode`）。
    pub fn add_node(&mut self, slot: impl crate::fc::Children) {
        self.child(slot);
    }

    /// 追加一个原始 slot（保留 `GetNodes` 动态区域语义）。
    ///
    /// 供 DSL 层把 `render_for_each` 的动态区域直接注入父容器 ——
    /// 若把动态区域包成中间 `Node`，`flatten` 不会递归进它的子节点，
    /// 该区域将脱离父容器的一维布局（高度塌成 0、子项重叠）。
    pub fn push_slot(&mut self, slot: ChildSlot) {
        self.slots
            .push(crate::node::stamp_slot(slot, &self.provided));
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
        render_for_each(&mut self.slots, &self.provided, for_each, creater);
    }

    /// 追加动态区域：按条件信号切换分支。
    ///
    /// 对应反应式的 `if cond { A } else { B }`。见 [`crate::cond::render_when`]。
    ///
    /// - `cond` 在区域 memo 求值时执行，读取条件信号（自动追踪）。
    /// - `branch(cond_value, cx)` 仅在切换到该分支时执行一次，注入该分支子节点。
    pub fn render_when(
        &mut self,
        cond: impl Fn() -> bool + 'static,
        branch: impl Fn(bool, &mut NodeContext) + 'static,
    ) {
        crate::cond::render_when(&mut self.slots, &self.provided, cond, branch);
    }

    /// 提供值：本节点子树构建期间，后代节点可用 [`Self::consume`] 读取。
    ///
    /// 作用域是**当前上下文及其后代**：祖先 provide 的值会被子上下文继承
    /// （见 [`Self::inherit`]），所以在面板里 provide 一次，它内部的
    /// `for_each` 行、嵌套容器里的组件都能 consume 到。
    pub fn provide<T: 'static>(&mut self, value: T) {
        self.provided.push((TypeId::of::<T>(), Rc::new(value)));
    }

    /// 读取最近一次 [`Self::provide`] 的对应类型的值。
    pub fn consume<T: 'static>(&self) -> Option<&T> {
        self.provided.iter().rev().find_map(|(tid, v)| {
            if *tid == TypeId::of::<T>() {
                v.as_ref().downcast_ref::<T>()
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
        // 无需二次盖章：每个节点在 push 进 cx 时已盖上当时的 provide 快照
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
