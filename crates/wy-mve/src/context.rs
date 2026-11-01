//! 节点树的构建上下文。
//!
//! `NodeContext` 是 `Fc` 收到的唯一参数：TS `canvasRender2` 里那个 `ctx`。
//!
//! 它只承担两件事：
//! 1. **父指针**（TS `ctx.getParent()`）——`Node2::mount` 靠它认父；
//!    动态区域（`for_each_node` / `if_node`）的 creater 在惰性求值时线程局部
//!    父栈已弹空，所以父指针必须挂在上下文上随闭包带走。
//! 2. **上下文传值**（`provide` / `consume`）——目前只有全局鼠标状态
//!    （`tree::global_mouse`）用它；业务代码一律走显式参数。
//!
//! v1 的 `slots: Vec<ChildSlot>`、`node2_only` 开关、`render_for_each` 的
//! holder 分支都随旧声明层一起删除：纯 `Node2` 树里节点直接挂在父节点的
//! `children` 上，不再有"先攒 IR、再由引擎读"的那一层。

use std::any::{Any, TypeId};
use std::rc::Rc;

use crate::canvas_render2::node2::Node2;
use crate::canvas_render2::Fc;

/// 上下文提供值列表：`(类型 id, 值)`。
///
/// 用 `Rc<dyn Any>` 而非 `Box<dyn Any>`：子上下文要**继承父链的一份快照**，
/// `Rc` 让这份继承退化成指针复制，不必深拷贝值本身。
pub(crate) type Provided = Vec<(TypeId, Rc<dyn Any>)>;

/// 节点树的构建上下文（对应 TS canvasRender2 的 `ctx`）。
#[derive(Default)]
pub struct NodeContext {
    /// 提供者注册表：`type_id -> value`。
    pub(crate) provided: Provided,
    /// 当前正在展开子节点的声明层父节点（`canvas_render2::Node2`）。
    ///
    /// 对应 TS `ctx.getParent()`。静态子节点走线程局部的 `ParentScope` 就够；
    /// 但**动态区域**（`for_each_node` / `if_node`）的 creater 是在惰性求值时才
    /// 跑的，那时线程局部栈早已弹空——所以父指针必须挂在上下文上、随动态区域
    /// 一路带进 creater 的子上下文，`Node2::mount` 才能把动态生成的行登记进
    /// 父的 `children()`。
    pub(crate) node2_parent: Option<Node2>,
}

impl NodeContext {
    /// 创建空上下文。
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// 开一个"继承父链 provide 快照"的子上下文（带父 Node2 指针）。
    ///
    /// 动态区域（`for_each_node` / `if_node`）的每一项、以及容器展开子节点都用它：
    /// 子上下文**看得见祖先 provide 的值**，但自己在里面 `provide` 只影响自己这一支
    /// ——继承的是快照，不是共享表。
    pub(crate) fn inherit_with_parent(provided: &Provided, parent: Option<Node2>) -> Self {
        Self {
            provided: provided.clone(),
            node2_parent: parent,
        }
    }

    /// 把一个已经构造好的子树挂进本上下文（TS `ctx.addNode` / `fc(ctx)`）。
    ///
    /// 对应 TS 的 `ctx.addNode(node)`：`Fc` 就是 `(ctx) => void`，展开即挂载。
    pub fn child(&mut self, fc: impl Into<Fc>) {
        fc.into().mount(self);
    }

    /// 提供值：本节点子树构建期间，后代节点可用 [`Self::consume`] 读取。
    ///
    /// 作用域是**当前上下文及其后代**：祖先 provide 的值会被子上下文继承
    /// （见 `Self::inherit_with_parent`），所以在面板里 provide 一次，它内部的
    /// 动态行、嵌套容器里的组件都能 consume 到。
    pub fn provide<T: 'static>(&mut self, value: T) {
        self.provided.push((TypeId::of::<T>(), Rc::new(value)));
    }

    /// 读取最近一次 [`Self::provide`] 的对应类型的值。
    #[must_use]
    pub fn consume<T: 'static>(&self) -> Option<&T> {
        self.provided.iter().rev().find_map(|(tid, v)| {
            if *tid == TypeId::of::<T>() {
                v.as_ref().downcast_ref::<T>()
            } else {
                None
            }
        })
    }

    /// 当前父节点（TS `ctx.getParent()`）。
    ///
    /// 动态区域内部返回的是**区域所属的那个父节点**；根上下文里为 `None`。
    #[must_use]
    pub fn parent(&self) -> Option<Node2> {
        self.node2_parent.clone()
    }

    /// 在当前父节点下挂一段 keyed 列表（TS `ctx.renderForEach`）。
    ///
    /// 父节点为空时（根上下文里直接调用）无动作——动态区域必须挂在某个容器下，
    /// 没有容器就没有"它该占哪一段"。
    pub fn render_for_each<K, T>(
        &mut self,
        for_each: impl Fn(&mut dyn FnMut(K, T)) + 'static,
        creater: impl Fn(K, Rc<std::cell::RefCell<T>>, &mut NodeContext) + 'static,
    ) where
        K: Eq + std::hash::Hash + Clone + 'static,
        T: Clone + PartialEq + 'static,
    {
        if let Some(parent) = self.node2_parent.clone() {
            crate::foreach::render_for_each(&parent, &self.provided, for_each, creater);
        }
    }

    /// 在当前父节点下挂一个条件分支（TS `ifNode`）。
    pub fn render_when(
        &mut self,
        cond: impl Fn() -> bool + 'static,
        branch: impl Fn(bool, &mut NodeContext) + 'static,
    ) {
        if let Some(parent) = self.node2_parent.clone() {
            crate::foreach::render_when(&parent, &self.provided, cond, branch);
        }
    }
}
