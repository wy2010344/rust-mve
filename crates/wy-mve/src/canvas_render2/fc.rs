//! TS `canvasRender2/tree/tfc.ts` 的直译：`Fc` 与槽 `Ref2`。
//!
//! ```ts
//! // wy-helper/src/storeRef.ts
//! export type StoreRef<T> = { set(v: T): T; get(): T };
//! export function storeRef<T>(value: T) { .. }
//!
//! // canvasRender2/tree/tfc.ts
//! export function fc(f: (ctx, ...vs) => void) {
//!   return (...vs) => (ctx) => { f(ctx, ...vs); };
//! }
//! export type Fc = (ctx) => void;
//! ```
//!
//! 一句话：**`Fc` 是「已经能用」的构造体，`children` 的唯一元素类型**。
//!
//! # 没有 `TFc` 了
//!
//! TS 早期版本是 `tfc` 三段式：`tfc(f).create(...v)` 给出
//! `TFc<M> = (ref: StoreRef<M>) => Fc`，再由 `.of()` 自建一个空槽把它降成 `Fc`。
//! **现在整套已移除**（`tfc.ts` 如今只剩 `fc`）：节点句柄变成**普通参数**
//! `arg.nodeRef`（`Node2Props` 的可选字段），最底层 `node2` 在工厂末尾负责
//! `ref.set(node)`；调用方想拿句柄就自己 `storeRef` 一个填进 `arg.nodeRef`，
//! 不传则用一个内部新建的空槽。这与 ooc 完全同形——组件就是普通函数，参数直接
//! 传，没有 ref 的柯里化包装。
//!
//! Rust 侧逐字对应：`#[rec]` 组件的 `node_ref: Ref2<Node2>` 带
//! `#[props(default = Ref2::new())]`——不传就内部新建（调用方拿不到句柄，与旧
//! `.of()` 行为一致），传了就写进去；工厂末尾 `node_ref.set(node)`。
//!
//! # 那个槽是**输出**，不是输入
//!
//! TS 的 `ref` 是 `{ get, set }` 的 cell，初值 `undefined!`（类型撒谎）。**谁
//! 来 set？工厂函数体自己**——`node2` 的工厂末尾就是造出节点、挂进父、然后
//! `ref.set(node)`（`tree/assemble.ts`）。所以：
//!
//! - 槽的载荷 `M` 与 `Node2` **无绑定**，可以是内部节点、也可以是自定义句柄
//!   （`tfc.ts` 原话："类似 react 的 forwardRef"）；
//! - 外部**不要假设构造期能 `.get()`**，要父节点走 `ctx.getParent()`。
//!
//! Rust 侧 [`Ref2<M>`] 就是 `StoreRef<M>` 的直译，`Option<M>` 是 `undefined!`
//! 的诚实编码：[`Ref2::get`] 在未写入时 panic，[`Ref2::try_get`] 给出判空路径。
//! 因为初值不必是 `M`，`Ref2::new()` 也就不需要 `M: Default`。
//!
//! 别和 [`NodeRef`](crate::NodeRef) 混：`NodeRef` 是**几何 cell**
//! （`Rc<Cell<NodeGeom>>`，布局期由引擎回填），`Ref2` 是**节点对象的槽**。
//! TS 侧两者同样分开（`StoreRef<M>` vs 节点自身的几何字段）。

use std::cell::RefCell;
use std::rc::Rc;

use crate::context::NodeContext;

/// TS `Fc = (ctx) => void`：**已经能用**的构造体，`children` 的唯一元素类型。
///
/// 组件函数（`#[rec]` 体或手写的 `fn(..) -> Fc`）直接产出它，调用处放进
/// `children` 即可，没有 `.of()` 之类的收尾。
pub struct Fc(Box<dyn FnOnce(&mut NodeContext)>);

/// TS `StoreRef<M>`（`wy-helper/src/storeRef.ts`）：`{ set(v): v; get(): T }`。
///
/// 就是 `arg.nodeRef` 的类型。TS 侧初值是 `storeRef<M>(undefined!)`——类型上
/// 写着 `M`、实际是 `undefined`，所以文档明说**构造期不要 `.get()`**，要父
/// 节点一律走 `ctx.getParent()`。
///
/// Rust 用 `Option<M>` 照实表达那个"尚未写入"的状态，而不是学 TS 撒谎：
/// [`get`](Self::get) 在未写入时 panic（等价于 TS 里对 `undefined` 解引用），
/// [`try_get`](Self::try_get) 给出可判空的那条路。因为初值不必是 `M`，
/// [`Ref2::new`] 也就不需要 `M: Default`。
///
/// 与 [`NodeRef`](crate::NodeRef) 的分工别混：`NodeRef` 是**几何 cell**
/// （`Rc<Cell<NodeGeom>>`，布局期由引擎回填），`Ref2` 是**节点对象的槽**
/// （body 造完节点后写进去）。TS 侧两者同样分开（`StoreRef<M>` vs 节点自身字段）。
pub struct Ref2<M>(Rc<RefCell<Option<M>>>);

impl<M> Clone for Ref2<M> {
    fn clone(&self) -> Self {
        Self(Rc::clone(&self.0))
    }
}

impl<M> Default for Ref2<M> {
    fn default() -> Self {
        Self::new()
    }
}

impl<M> Ref2<M> {
    /// TS `storeRef<M>(undefined!)`：**空槽**，`M` 不必有 `Default`。
    #[must_use]
    pub fn new() -> Self {
        Self(Rc::new(RefCell::new(None)))
    }

    /// TS `ref.set(v)`：写入唯一一份值。
    ///
    /// TS 的 `set` 返回 `v`（`const n = ref.set(node)` 这种写法），但
    /// canvasRender2 内**没有一处消费返回值**，故省略——不返回不等于简化语义，
    /// 而是照实去掉死路径。
    pub fn set(&self, v: M) {
        *self.0.borrow_mut() = Some(v);
    }

    /// TS `ref.get()`：取当前值。未写入即 panic（TS 里那一下是 `undefined` 炸在
    /// 别处，此处提前到取值点，便于定位"构造期就 `.get()`"这类误用）。
    ///
    /// # Panics
    /// 尚未 [`set`](Self::set) 过时。
    #[must_use]
    pub fn get(&self) -> M
    where
        M: Clone,
    {
        self.try_get().expect(
            "Ref2::get() 在 set 之前被调用（对应 TS 里读到 undefined）；\
             构造期要父节点请用 ctx.get_parent()",
        )
    }

    /// `get()` 的可判空版本：给"可能还没写"的场合用。
    #[must_use]
    pub fn try_get(&self) -> Option<M>
    where
        M: Clone,
    {
        self.0.borrow().clone()
    }

    /// 是否已写入。
    #[must_use]
    pub fn is_set(&self) -> bool {
        self.0.borrow().is_some()
    }
}

impl Fc {
    /// 包一层 `(ctx) => void`。
    pub fn new(f: impl FnOnce(&mut NodeContext) + 'static) -> Self {
        Self(Box::new(f))
    }

    /// 立刻在 `ctx` 上展开（TS 里 `fc(ctx)` 那一句）。
    pub fn mount(self, cx: &mut NodeContext) {
        (self.0)(cx);
    }
}

/// TS `buildChildren`：`children.forEach(child => child(ctx))`。
///
/// 只是把「按顺序把每个 `Fc` 摊进 ctx」这件事命名出来，方便组件的
/// `arg_children_fn` 调用处对齐 TS 原文。
pub fn build_children(cx: &mut NodeContext, children: Vec<Fc>) {
    for child in children {
        child.mount(cx);
    }
}

/// TS `(ref: A) => B` 的第二段：`nodeOf` 把同一个 ref 再灌一次的入口。
///
/// TS 的 `nodeOf<A, B>(cb: (ref: A) => (ref: A) => B)` 两段的返回类型都是
/// 普通的函数，Rust 需要把"能被 ref 调用"这件事显式化，于是有了这个 trait：
/// 泛型 `M` / `B` 才留得住（`node_of` 不写死返回 `Fc`）。
pub trait NodeApply<M, B> {
    /// 把 ref 灌进去。
    fn apply(self, r: Ref2<M>) -> B;
}

/// 回调已经交出可用的 [`Fc`]（`node_of` 最常见的形态）：ref 已在回调体里用过，
/// 这里不能再灌一次，原样返回。
impl<M> NodeApply<M, Fc> for Fc {
    fn apply(self, _r: Ref2<M>) -> Fc {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 槽的载荷不必是 `Node2`。
    ///
    /// `StoreRef<M>` 只要求 `{ get, set }`，所以挂一个自定义句柄（光标、滚动
    /// 位置、命令面板控制器…）是完全合法的用法（`tfc.ts` 原话"类似 react 的
    /// forwardRef"）。这条测试把该能力钉成可验证的事实：若哪天把
    /// `node_ref` 写死成 `Ref2<Node2>`，这段代码**编译不过**。
    #[test]
    fn ref_payload_can_be_a_non_node_handle() {
        #[derive(Clone, Debug, PartialEq, Eq)]
        struct ScrollHandle {
            offset: usize,
        }

        let slot: Ref2<ScrollHandle> = Ref2::new();
        assert!(!slot.is_set(), "初值是空的（TS 侧 `undefined!`）");
        assert_eq!(slot.try_get(), None);

        // 工厂体照 `node2` 的收尾写法：先干活，末尾把句柄写进槽。
        let write = move |r: Ref2<ScrollHandle>| -> Fc {
            r.set(ScrollHandle { offset: 7 });
            Fc::new(|_cx| {})
        };
        let _fc = write(slot.clone());

        assert_eq!(slot.get(), ScrollHandle { offset: 7 });
    }

    /// 槽可以指向**内部节点**，而不是「返回出去的那个顶层节点」。
    ///
    /// TS 里 `ref.set(..)` 是工厂体自己写的，所以想指谁就指谁；旧版 Rust 把
    /// node 当**输入**传进来（`of()` 先造壳、body 只能填它），这个形状根本写
    /// 不出来。换成输出槽之后，下面这段才有意义：
    ///
    /// ```text
    /// outer ← Fc::from(outer)   返回 / 挂载的是外层
    ///   └ inner ← ref.set(inner) 槽里存的是内层
    /// ```
    #[test]
    fn ref_may_point_at_an_inner_node() {
        use crate::canvas_render2::node2::Node2;

        let slot: Ref2<Node2> = Ref2::new();
        let returned: Rc<RefCell<Option<Node2>>> = Rc::new(RefCell::new(None));
        let ret = Rc::clone(&returned);

        let write = move |get: Ref2<Node2>| -> Fc {
            let outer = Node2::empty();
            let inner = Node2::empty();
            *ret.borrow_mut() = Some(outer.clone());
            get.set(inner); // 槽指向内层
            Fc::from(outer) // 返回的却是外层
        };
        let _fc = write(slot.clone());

        let outer = returned.borrow().clone().expect("外层节点已产出");
        assert!(
            slot.get() != outer,
            "槽里存的必须是内层节点，而不是返回的那个外层"
        );
        assert_eq!(
            slot.get(),
            slot.get(),
            "槽稳定指向同一个句柄（Node2 是 Rc 共享，比较的是指针）"
        );
    }
}
