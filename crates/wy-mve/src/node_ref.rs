//! 提前释放的「最终生效几何」句柄 [`NodeRef`] + [`node_of`]。
//!
//! 对应 TS `mve-dom-helper/canvasRender2` 的 `nodeOf`：
//!
//! ```ts
//! export function nodeOf(callback: (ref: StoreRef<Node2>) => B) {
//!   const ref = storeRef<Node2>(undefined!);
//!   return callback(ref)(ref);
//! }
//! ```
//!
//! 它的**目的**是：把最终生效的 node 在构造期就释放出来，供构造参数去引用——
//! `draw` 要知道自己多大、`width` 可能依赖最终 `height`、`padding` 可能依赖最终
//! `width`、事件里也要访问最终生效的参数。
//!
//! Rust 侧保留了同一个「**两遍传递**」：`ref` 一遍交给回调闭包（供 `draw` /
//! 事件引用），一遍灌进组件本身（当它要填充的那个节点）——见 [`node_of`]。
//! 区别只是它落在**类型系统**里：中间那层是 [`Ref2<Node2>`](crate::Ref2) 的输出
//! 槽，而不是 JS 里那个先建出来的空对象。
//!
//! # 为什么只释放"几何"而不是整个 node
//!
//! TS 释放整个 `{} as Node2`，是因为它只有一个对象、`width`/`padding`/`draw`
//! 全挂在上面。Rust 有类型系统，可以只把**最终生效的那一部分**释放出来：
//! 身份就是 `Node::geom` 那个共享的 `Rc<Cell<NodeGeom>>`（`Node` clone 时
//! `Rc` 共享，引擎写树里的那个、组件读手里的这个，是同一个 cell）。
//!
//! # 求值时机（与 TS 一致，不做任何保护）
//!
//! `NodeRef` 是**直读**：读到什么就是什么。
//!
//! - 在 `draw` / 事件回调里读 = 本帧最终生效值（引擎在布局盒分配之后、
//!   绘制子树之前写入）；
//! - 在 `width` / `height` / `padding` 这类**布局期**求值的闭包里读 = cell 里
//!   当前的值（首次布局前是 `ZERO`）。
//!
//! **引擎不做常量快照、不做重入检测、不做"上一帧值"补偿**——TS 也不做：
//! 在 `width()` 里写 `node.height()` 会 `size(node,'y') → argSize('y') →
//! createLayout → innerSize → node.height()` 直接栈溢出。写错了是写的人的锅，
//! 写对了就没有循环。这里保持同样的语义。

use std::cell::Cell;
use std::rc::Rc;

use crate::canvas_render2::fc::{NodeApply, Ref2};
use crate::primitives::LayoutBox;

/// 节点**最终生效**的几何：外包尺寸 + 生效内边距 + 绝对位置。
///
/// 由引擎在「布局盒分配之后、绘制子树之前」写入（见 `draw_node`）。
/// `Copy`，因为要放进 `Cell`。
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct NodeGeom {
    /// 最终生效的外包宽（含 padding）。
    pub width: f32,
    /// 最终生效的外包高（含 padding）。
    pub height: f32,
    /// 最终生效的左内边距（四向各自独立，对应 TS `paddingInlineStart()`）。
    pub pad_left: f32,
    /// 最终生效的右内边距（TS `paddingInlineEnd()`）。
    pub pad_right: f32,
    /// 最终生效的上内边距（TS `paddingBlockStart()`）。
    pub pad_top: f32,
    /// 最终生效的下内边距（TS `paddingBlockEnd()`）。
    pub pad_bottom: f32,
    /// 外包左上角的**绝对（画布）横坐标**，对应 TS `Node2.absoluteX`。
    ///
    /// 沿祖先链累加得出：每个节点 = 父的绝对原点 + 自己分到的布局盒偏移 +
    /// 自身 `pos_x` 偏移。滚动/裁剪**不参与**（它是几何真值，不是可见区域）。
    pub abs_x: f32,
    /// 外包左上角的**绝对（画布）纵坐标**，对应 TS `Node2.absoluteY`。
    pub abs_y: f32,
}

impl NodeGeom {
    /// 全零几何（尚未布局）。
    pub const ZERO: Self = Self {
        width: 0.0,
        height: 0.0,
        pad_left: 0.0,
        pad_right: 0.0,
        pad_top: 0.0,
        pad_bottom: 0.0,
        abs_x: 0.0,
        abs_y: 0.0,
    };

    /// 内容区尺寸（外包 − 两侧内边距），对应 TS `innerSize(node, d)`。
    #[must_use]
    pub fn inner(&self) -> (f32, f32) {
        (
            (self.width - self.pad_left - self.pad_right).max(0.0),
            (self.height - self.pad_top - self.pad_bottom).max(0.0),
        )
    }

    /// 内容区左上角的绝对坐标（外包绝对原点 + 起始内边距）。
    #[must_use]
    pub fn abs_inner(&self) -> (f32, f32) {
        (self.abs_x + self.pad_left, self.abs_y + self.pad_top)
    }

    /// 把「画布绝对坐标」转成自己的**局部坐标**（`draw` 里的坐标系）。
    ///
    /// 用于把全局事件坐标（鼠标、另一个节点的绝对位置）换算进来。
    #[must_use]
    pub fn to_local(&self, abs_x: f32, abs_y: f32) -> (f32, f32) {
        (abs_x - self.abs_x, abs_y - self.abs_y)
    }

    /// 把自己局部坐标转成画布绝对坐标（[`Self::to_local`] 的逆）。
    #[must_use]
    pub fn to_abs(&self, x: f32, y: f32) -> (f32, f32) {
        (self.abs_x + x, self.abs_y + y)
    }
}

/// 提前释放的节点句柄：**构造期拿到，绘制/布局/事件期读到最终生效值**。
///
/// 就是 TS `nodeOf` 里那个先建出来的 `node` 对象——Rust 版只暴露
/// "最终几何"这一部分。
///
/// `Clone` 是浅拷贝：所有副本共享同一个 cell，因此能在多个闭包
/// （`draw` / `on_click` / `width` ...）里各捕获一份，读到的都是同一个值。
#[derive(Clone, Debug)]
pub struct NodeRef {
    cell: Rc<Cell<NodeGeom>>,
}

impl NodeRef {
    /// 新建一个尚未绑定的句柄（几何全零）。
    ///
    /// 通常不由用户直接调——走 [`node_of`]，或让 `node2` 的 `node` prop 缺省。
    #[must_use]
    pub fn new() -> Self {
        Self {
            cell: Rc::new(Cell::new(NodeGeom::ZERO)),
        }
    }

    /// 取出共享 cell（供组件把它接到 `Node::geom` 上）。
    #[must_use]
    pub fn cell(&self) -> Rc<Cell<NodeGeom>> {
        Rc::clone(&self.cell)
    }

    /// 用已有 cell 包一个句柄（`Node2` 与引擎 IR 共享同一份几何时用）。
    #[must_use]
    pub fn from_cell(cell: Rc<Cell<NodeGeom>>) -> Self {
        Self { cell }
    }

    /// 最终生效几何。
    #[must_use]
    pub fn geom(&self) -> NodeGeom {
        self.cell.get()
    }

    /// 最终生效的布局盒（局部坐标，原点在自身左上角）。
    #[must_use]
    pub fn box_(&self) -> LayoutBox {
        let g = self.cell.get();
        LayoutBox::new(0.0, 0.0, g.width, g.height)
    }

    /// 最终生效的外包宽。
    #[must_use]
    pub fn width(&self) -> f32 {
        self.cell.get().width
    }

    /// 最终生效的外包高。
    #[must_use]
    pub fn height(&self) -> f32 {
        self.cell.get().height
    }

    /// 最终生效的起始横向内边距（左），对应 TS `paddingInlineStart()`。
    #[must_use]
    pub fn pad_left(&self) -> f32 {
        self.cell.get().pad_left
    }

    /// 最终生效的结束横向内边距（右）。
    #[must_use]
    pub fn pad_right(&self) -> f32 {
        self.cell.get().pad_right
    }

    /// 最终生效的起始纵向内边距（上），对应 TS `paddingBlockStart()`。
    #[must_use]
    pub fn pad_top(&self) -> f32 {
        self.cell.get().pad_top
    }

    /// 最终生效的结束纵向内边距（下）。
    #[must_use]
    pub fn pad_bottom(&self) -> f32 {
        self.cell.get().pad_bottom
    }

    /// 最终生效的内容区尺寸 `(w, h)`（已扣掉两侧内边距）。
    #[must_use]
    pub fn inner(&self) -> (f32, f32) {
        self.cell.get().inner()
    }

    // —— 绝对位置（TS `Node2.absoluteX / absoluteY`）——

    /// 外包左上角的绝对（画布）坐标 `(x, y)`。
    ///
    /// 与 TS 的 `absolutePosition` 同义：把祖先链上的偏移全部累加后的真值。
    /// 在 `draw` / 事件回调里读 = 本帧最终落点。
    #[must_use]
    pub fn abs(&self) -> (f32, f32) {
        let g = self.cell.get();
        (g.abs_x, g.abs_y)
    }

    /// 外包左上角的绝对横坐标（TS `absoluteX`）。
    #[must_use]
    pub fn abs_x(&self) -> f32 {
        self.cell.get().abs_x
    }

    /// 外包左上角的绝对纵坐标（TS `absoluteY`）。
    #[must_use]
    pub fn abs_y(&self) -> f32 {
        self.cell.get().abs_y
    }

    /// 内容区左上角的绝对坐标（外包绝对原点 + 内边距）。
    #[must_use]
    pub fn abs_inner(&self) -> (f32, f32) {
        self.cell.get().abs_inner()
    }

    /// 绝对坐标下的布局盒（外包）。
    #[must_use]
    pub fn abs_box(&self) -> LayoutBox {
        let g = self.cell.get();
        LayoutBox::new(g.abs_x, g.abs_y, g.width, g.height)
    }

    /// 把画布绝对坐标换算成自己的局部坐标（`draw` 里的坐标系）。
    #[must_use]
    pub fn to_local(&self, abs_x: f32, abs_y: f32) -> (f32, f32) {
        self.cell.get().to_local(abs_x, abs_y)
    }

    /// 把自己局部坐标换算成画布绝对坐标。
    #[must_use]
    pub fn to_abs(&self, x: f32, y: f32) -> (f32, f32) {
        self.cell.get().to_abs(x, y)
    }
}

impl Default for NodeRef {
    fn default() -> Self {
        Self::new()
    }
}

/// 提前释放 ref（TS `nodeOf` 的 Rust 版）。
///
/// 与 TS **逐字同构**：TS 是 `callback(node)(node)`——**同一个 `StoreRef` 两遍**：
/// 一遍交给回调（供 `draw` / `width` / 事件在**运行期**读"最终生效的我"），
/// 一遍灌进工厂本身（工厂末尾的 `ref.set(node)` 写的就是这同一个槽）。
///
/// ```ignore
/// node_of(|get| node2! {
///     node_ref: get.clone(),          // rec 之后 ref 是普通参数，必须显式交给工厂
///     width: get.get().height() * 2.0,
///     draw:  move |s, _n| { let b = get.get().box_(); .. },
/// })
/// ```
///
/// 槽由 [`node_of`] 分配，但**要显式写进工厂的 `node_ref`**——`rec` 之后宏不做
/// 任何隐式注入，`node_ref` 就是普通参数（不传则内部新建、调用方拿不到）。
///
/// # 构造期不要 `get()`
///
/// 槽初值是空的（TS 侧同样是 `storeRef(undefined!)`），`set` 发生在工厂体末尾。
/// 所以在 `width` / `draw` / 事件回调里 `get.get()` 安全（那时节点已定型），
/// 而在体里**构造期直接读**会 panic —— 正是 TS 文档反复强调的那条
/// （要父节点一律走 `ctx.getParent()`）。
pub fn node_of<M, G, T, B>(callback: G) -> B
where
    G: FnOnce(Ref2<M>) -> T,
    T: NodeApply<M, B>,
{
    // TS: `const node = storeRef<A>(undefined!); return callback(node)(node)`
    // —— **立即**两步都做完，且两遍是同一个槽。
    let r = Ref2::<M>::new();
    callback(r.clone()).apply(r)
}

/// [`node_of`] 的**驼峰别名**，逐字对应 TS 的 `nodeOf`。
///
/// Rust 惯例是 snake_case，主名仍是 [`node_of`]；这个别名只为让移植 TS 代码时
/// 能按原名书写。
#[allow(non_snake_case)]
#[must_use]
pub fn nodeOf<M, G, T, B>(callback: G) -> B
where
    G: FnOnce(Ref2<M>) -> T,
    T: NodeApply<M, B>,
{
    node_of(callback)
}
