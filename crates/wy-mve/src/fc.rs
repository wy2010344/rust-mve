//! FC 风格 DSL：结构化组件声明，取代「回调 + `cx`」样板。
//!
//! 动机：OOC 侧的 `dom` / `forEach` 用**剩余参数**接收子节点，
//! 组件作者写 `button(on_click, text(...))` 而不是 `button(..., |cx| cx.child(...))`。
//! 本模块把同一形态落到 Rust 的 `NodeContext` 上。
//!
//! # 两种写法等价
//!
//! ```ignore
//! // 现状：闭包 + 手动 child
//! render_root(|cx| {
//!     cx.child(column(|cx| {
//!         cx.child(text(|| "标题".into()));
//!     }));
//! });
//!
//! // 本模块：结构化调用，剩余参数即子节点
//! render_root(|cx| {
//!     cx.child(vbox([label(|| "标题".into())]));
//! });
//! ```
//!
//! 两者产出**同样的** `ChildSlot` 序列；DSL 只省掉嵌套闭包和 `cx`。
//!
//! # 关键约束
//!
//! - 子节点求值仍在 `arg_children_fn`（构建期）内发生，memo 语义不变。
//! - 参数里的信号闭包照旧在 draw / memo 期被追踪。
//! - 需要 `cx` 的场景（`render_for_each` / `provide`）用 [`ChildArg::new`]。

use std::rc::Rc;

use crate::context::NodeContext;
use crate::node::{flatten, ChildSlot, Layout, Node};

/// 子节点构造体：拿到 `&mut NodeContext` 后自由构建。
type ChildBody = Box<dyn FnOnce(&mut NodeContext)>;

/// 「稍后以 `cx` 构造」的子节点。
///
/// 在**不手写 `&mut NodeContext` 参数**的前提下，让需要 `cx` 的场景
/// （`render_for_each` / `provide` / `consume`）依然可写。
///
/// 构造出的 slot（含 `GetNodes` 动态区域）**直接注入父容器**，
/// 不会被包成中间 `Node` —— 否则 `flatten` 不递归进中间节点，
/// 动态区域会脱离父容器的一维布局。
pub struct ChildArg(ChildBody);

impl ChildArg {
    /// 把构造体推迟到 `cx` 可用时才执行。
    pub fn new(f: impl FnOnce(&mut NodeContext) + 'static) -> Self {
        Self(Box::new(f))
    }

    /// 在父上下文中就地展开 slot。
    ///
    /// 公开供 `rsx!` 使用：给组件标签挂布局属性（`grow` / `width`…）时，
    /// 需要先把 ctx 组件跑进一个"自己的作用域"，再把产出的节点搬回父上下文。
    pub fn push_slots(self, cx: &mut NodeContext) {
        (self.0)(cx);
    }
}

/// DSL 子节点统一类型：容器参数数组的元素类型。
///
/// Rust 数组要求元素同类型，所以 `[text(..), for_each(..)]` 里的
/// `Node` 与 `ChildArg` 需要统一到 `Slot` 才能放进同一个数组。
/// `vbox` / `hbox` 等通过 [`Slot::from`] 自动转换（`Into<Slot>`）。
///
/// `Node` 是句柄型结构（字段以 `Rc` 为主，克隆共享），拆箱只多一次堆
/// 分配，故不 `Box`。
#[allow(clippy::large_enum_variant)]
pub enum Slot {
    /// 单个已构造节点。
    One(Node),
    /// 惰性子节点（`for_each` 等动态区域）。
    Many(ChildArg),
}

impl Slot {
    /// 取回节点（惰性子节点先物化）。
    ///
    /// `rsx!` 返回 [`Slot`]，作为顶层节点时用 `.into_node()` 转回 [`Node`]。
    #[must_use]
    pub fn into_node(self) -> Node {
        match self {
            Slot::One(n) => n,
            Slot::Many(c) => {
                let mut inner = NodeContext::new();
                c.push_slots(&mut inner);
                let mut nodes = Vec::new();
                flatten(&inner.into_slots(), &mut nodes);
                nodes.pop().unwrap_or_default()
            }
        }
    }
}

impl From<Node> for Slot {
    fn from(n: Node) -> Self {
        Slot::One(n)
    }
}

impl From<ChildArg> for Slot {
    fn from(c: ChildArg) -> Self {
        Slot::Many(c)
    }
}

/// 把闭包参数类型钉死为 `bool`（`rsx!` 的 `on_hover` 用）。
///
/// 没有它，`on_hover: move |entered| { .. }` 无法编译：宏把用户闭包存进
/// `let __f = ..` 再 move 进 `Rc<dyn Fn(bool)>`，而 Rust 不会从"被捕获后再调用"
/// 反推闭包参数类型（实测 `let f = |x| ..; let g = move || f(true);` 报 E0282）。
#[doc(hidden)]
pub fn __fn_bool<F: Fn(bool)>(f: F) -> F {
    f
}

/// 把闭包参数类型钉死为 `&mut PointerEvent`（`on_wheel` / 拖拽三件套用）。
#[doc(hidden)]
pub fn __fn_ptr<F: Fn(&mut crate::PointerEvent)>(f: F) -> F {
    f
}

/// 把闭包参数钉死为 `(f32, f32)`（`on_move` 用：节点内局部坐标 x/y）。
#[doc(hidden)]
pub fn __fn_xy<F: Fn(f32, f32)>(f: F) -> F {
    f
}

/// 把闭包参数钉死为 `&mut KeyEvent`（`on_key` 用）。
#[doc(hidden)]
pub fn __fn_key<F: Fn(&mut crate::KeyEvent) -> bool>(f: F) -> F {
    f
}

/// `#[component]` 函数体的返回值约定（带 `ctx` 时）。
///
/// ctx 组件的 body 在挂载期执行，两种写法都合法：
/// - **产出节点**：`-> impl Into<Slot>`（`Node` / `Slot` / `Vec<Node>`），由框架注入父容器；
/// - **纯注入**：以 `;` 结尾返回 `()`，节点已通过 `ctx.child(..)` 自行注入。
pub trait IntoSlot {
    /// 转成可注入的 [`Slot`]（`()` → 空注入）。
    fn into_slot(self) -> Slot;
}

impl IntoSlot for Slot {
    fn into_slot(self) -> Slot {
        self
    }
}

impl IntoSlot for Node {
    fn into_slot(self) -> Slot {
        Slot::One(self)
    }
}

impl IntoSlot for ChildArg {
    fn into_slot(self) -> Slot {
        Slot::Many(self)
    }
}

impl IntoSlot for () {
    fn into_slot(self) -> Slot {
        Slot::Many(ChildArg::new(|_| {}))
    }
}

impl Children for Slot {
    fn push_slots(self, cx: &mut NodeContext) {
        match self {
            Slot::One(n) => cx.child(n),
            Slot::Many(c) => c.push_slots(cx),
        }
    }
}

impl<T: Into<Slot>, const N: usize> Children for [T; N] {
    fn push_slots(self, cx: &mut NodeContext) {
        for c in self {
            c.into().push_slots(cx);
        }
    }
}

impl<T: Into<Slot>> Children for Vec<T> {
    fn push_slots(self, cx: &mut NodeContext) {
        for c in self {
            c.into().push_slots(cx);
        }
    }
}

impl<T: Into<Slot>> Children for Option<T> {
    fn push_slots(self, cx: &mut NodeContext) {
        if let Some(v) = self {
            v.into().push_slots(cx);
        }
    }
}

/// 把一批 slot 注入 `cx`（宏内部用）。
pub fn push_all(slots: impl IntoIterator<Item = Slot>, cx: &mut NodeContext) {
    for s in slots {
        s.push_slots(cx);
    }
}

/// 一组子节点参数：注入父容器的 slots。
///
/// 接受 [`Slot`]（`Node`/`ChildArg` 经 `Into` 自动转换）、`Option`、`Vec`、数组。
///
/// 关键：保留 `ChildSlot` 而非压平成 `Node`，否则 `render_for_each`
/// 的动态区域会被包进中间节点，脱离父容器布局。
pub trait Children {
    /// 把子节点注入 `cx`。
    fn push_slots(self, cx: &mut NodeContext);
}

impl Children for ChildSlot {
    fn push_slots(self, cx: &mut NodeContext) {
        cx.push_slot(self);
    }
}

impl Children for Node {
    fn push_slots(self, cx: &mut NodeContext) {
        cx.push_slot(ChildSlot::Node(self));
    }
}

/// 收集子节点为 `Node` 列表（已展平动态区域）。
#[must_use]
pub fn collect(children: impl Children) -> Vec<Node> {
    let mut cx = NodeContext::new();
    children.push_slots(&mut cx);
    let mut out = Vec::new();
    flatten(&cx.into_slots(), &mut out);
    out
}

/// 把子节点参数包进布局容器。
fn box_with(layout: Option<Layout>, children: impl Children + 'static) -> Node {
    // `arg_children_fn` 是 `Fn`（可能被多次调用），而 `Children` 是一次性的。
    // 用 `RefCell<Option<..>>` 延迟消费；Node 惰性 memo 保证只 materialize 一次，
    // 所以正常路径下 `take()` 必定成功（重复调用是防御性兜底）。
    let children = std::cell::RefCell::new(Some(children));
    Node {
        layout,
        arg_children_fn: Rc::new(move |cx: &mut NodeContext| {
            let Some(children) = children.borrow_mut().take() else {
                return;
            };
            children.push_slots(cx);
        }),
        ..Node::default()
    }
}

/// 竖排容器（主轴 Y，间距 8）。
///
/// # Examples
///
/// ```
/// use wy_mve::{vbox, text, button};
/// let node = vbox([
///     text(|| "标题".into()),
///     button(|| "确定".into(), || {}),
/// ]);
/// # let _ = node;
/// ```
#[must_use]
pub fn vbox(children: impl Children + 'static) -> Node {
    box_with(Some(Layout::Column), children)
}

/// 横排容器（主轴 X，间距 8）。
#[must_use]
pub fn hbox(children: impl Children + 'static) -> Node {
    box_with(Some(Layout::Row), children)
}

/// 自定义间距的竖排容器。
#[must_use]
pub fn vbox_gap(gap: f32, children: impl Children + 'static) -> Node {
    let mut n = box_with(Some(Layout::Column), children);
    n.gap = gap;
    n
}

/// 自定义间距的横排容器。
#[must_use]
pub fn hbox_gap(gap: f32, children: impl Children + 'static) -> Node {
    let mut n = box_with(Some(Layout::Row), children);
    n.gap = gap;
    n
}

/// 带绝对定位的竖排容器。
#[must_use]
pub fn vbox_at(x: f32, y: f32, children: impl Children + 'static) -> Node {
    let mut n = vbox(children);
    n.x = x;
    n.y = y;
    n
}

/// 带绝对定位的横排容器。
#[must_use]
pub fn hbox_at(x: f32, y: f32, children: impl Children + 'static) -> Node {
    let mut n = hbox(children);
    n.x = x;
    n.y = y;
    n
}

/// 无布局的透明分组：只把子节点平铺进父级。
#[must_use]
pub fn group(children: impl Children + 'static) -> Node {
    box_with(None, children)
}

/// 可聚焦容器（进 Tab 序）。
#[must_use]
pub fn focusable(children: impl Children + 'static) -> Node {
    let mut n = group(children);
    n.focusable = true;
    n
}

/// 焦点陷阱容器：Tab 焦点不逃逸此子树。
#[must_use]
pub fn focus_trap(children: impl Children + 'static) -> Node {
    let mut n = group(children);
    n.focus_trap = true;
    n
}

/// 隐藏节点：不参与布局与命中。
#[must_use]
pub fn hidden(mut node: Node) -> Node {
    node.hidden = true;
    node
}

/// 文本子节点（[`Slot`] 版）：值是闭包，draw 期求值并自动追踪信号。
///
/// 与 [`crate::text`] 等价，返回 [`Slot`] 以便和 [`for_each`] 混在同一数组。
#[must_use]
pub fn label(content: impl Fn() -> String + 'static) -> Slot {
    Slot::One(crate::text(content))
}

/// 按钮子节点（[`Slot`] 版）：标签闭包 + 点击回调。
#[must_use]
pub fn btn(label: impl Fn() -> String + 'static, on_click: impl Fn() + 'static) -> Slot {
    Slot::One(crate::button(label, on_click))
}

/// 供 `rsx!` 宏使用：列表展开为 [`Slot`]。
///
/// 与 [`for_each`] 同语义，但 creater 接收 `Rc<RefCell<T>>` 值 + `cx`，
/// 返回的 [`Slot`] 直接注入父容器（保留 `GetNodes` 动态区域）。
pub fn for_each_slots<K, T>(
    each: impl Fn(&mut dyn FnMut(K, T)) + 'static,
    creater: impl Fn(Rc<std::cell::RefCell<T>>, &mut NodeContext) + 'static,
) -> Slot
where
    K: Eq + std::hash::Hash + Clone + 'static,
    T: Clone + PartialEq + 'static,
{
    Slot::Many(ChildArg::new(move |cx| {
        cx.render_for_each(each, move |_k, v, cx| creater(v, cx));
    }))
}

// ============================================================================
// rsx! 宏内部别名（由 wy-mve-macros 生成代码调用）
// ============================================================================

// 别名保持 rsx! 里的标签原样（`fc__btn` 等），非 snake_case 是刻意为之。
#[allow(non_snake_case)]
mod macro_alias {
    use super::{Children, Node, Slot};

    /// `rsx! { hbox { .. } }` → `fc::hbox`。
    pub use super::hbox as fc__hbox;
    /// `rsx! { vbox { .. } }` → `fc::vbox`。
    pub use super::vbox as fc__vbox;

    /// `rsx! { hbox_gap(n) { .. } }`。
    #[must_use]
    pub fn fc__hbox_gap(gap: f32, children: impl Children + 'static) -> Node {
        super::hbox_gap(gap, children)
    }
    /// `rsx! { vbox_gap(n) { .. } }`。
    #[must_use]
    pub fn fc__vbox_gap(gap: f32, children: impl Children + 'static) -> Node {
        super::vbox_gap(gap, children)
    }
    /// `rsx! { vbox_at(x, y) { .. } }`。
    #[must_use]
    pub fn fc__vbox_at(x: f32, y: f32, children: impl Children + 'static) -> Node {
        super::vbox_at(x, y, children)
    }
    /// `rsx! { hbox_at(x, y) { .. } }`。
    #[must_use]
    pub fn fc__hbox_at(x: f32, y: f32, children: impl Children + 'static) -> Node {
        super::hbox_at(x, y, children)
    }
    /// `rsx! { group { .. } }`。
    #[must_use]
    pub fn fc__group(children: impl Children + 'static) -> Node {
        super::group(children)
    }
    /// `rsx! { focusable { .. } }`。
    #[must_use]
    pub fn fc__focusable(children: impl Children + 'static) -> Node {
        super::focusable(children)
    }
    /// `rsx! { focus_trap { .. } }`。
    #[must_use]
    pub fn fc__focus_trap(children: impl Children + 'static) -> Node {
        super::focus_trap(children)
    }
    /// `rsx! { node { .. } }` —— **自绘容器（匿名组件）**：既能画自己，也能装子节点。
    ///
    /// 对应 Kotlin 的 `object : Node() { override val … }` / `Box { drawBg(); content() }`：
    /// 不需要名字、就地定义、就地使用，且动态属性（`width: gap.get()` 这类表达式）
    /// 自动包成 `*_fn`，天然就是「每次重绘按帧重算」的 getter 语义——
    /// 即 `#[getter]` 在 `node` 上**已经内置**，不必再写注解。
    ///
    /// 两种用法互不冲突：
    /// - **纯叶子**（不带子节点）：只承载 `draw` / 尺寸 / 事件，尺寸优先取声明值或
    ///   `measure`（与以往一致）。
    /// - **自绘容器**（带子节点）：子节点按 `Column` 在自身的布局盒内排布，
    ///   `draw` 画背景、子组件叠在上层——一个"卡片/面板"就出来了，
    ///   且 `for_each` / 其它 `#[component]` 都能平滑嵌套进来。
    #[must_use]
    pub fn fc__node(children: impl Children + 'static) -> Node {
        // `arg_children_fn` 是 `Fn`（可能被多次调用），而 `Children` 是一次性的，
        // 用 `RefCell<Option>` 延迟消费（与 `box_with` 同款防御）。
        let children = std::cell::RefCell::new(Some(children));
        Node {
            layout: Some(crate::node::Layout::Column),
            arg_children_fn: std::rc::Rc::new(move |cx: &mut crate::NodeContext| {
                if let Some(children) = children.borrow_mut().take() {
                    children.push_slots(cx);
                }
            }),
            ..Node::default()
        }
    }
    /// `rsx! { btn { .. } }` → `fc::btn`。
    #[must_use]
    pub fn fc__btn(label: impl Fn() -> String + 'static, on_click: impl Fn() + 'static) -> Slot {
        super::btn(label, on_click)
    }
    /// `rsx! { label(expr) }` → `fc::label`。
    #[must_use]
    pub fn fc__label(content: impl Fn() -> String + 'static) -> Slot {
        super::label(content)
    }
    /// `rsx! { text_field { value: .., on_change: .., .. } }`。
    ///
    /// 必填位置参数 + opts 记录式（`..Default::default()`）。
    #[must_use]
    pub fn fc__text_field(
        value: impl Fn() -> String + 'static,
        on_change: impl Fn(String) + 'static,
        opts: crate::text_field::TextFieldOpts,
    ) -> Slot {
        Slot::One(crate::text_field::text_field_opts(value, on_change, opts))
    }
    /// `rsx! { rich_text { value: .., on_change: .., .. } }`。
    #[must_use]
    pub fn fc__rich_text(
        value: impl Fn() -> String + 'static,
        on_change: impl Fn(String) + 'static,
        opts: crate::rich_text::RichTextOpts,
    ) -> Slot {
        Slot::One(crate::rich_text::rich_text_opts(value, on_change, opts))
    }
}

pub use macro_alias::*;

/// 追加动态区域：按条件信号切换分支（[`Slot`] 版，供 `rsx!` 使用）。
///
/// 反应式的 `if cond { A } else { B }`：`cond` 变化 → 旧分支销毁、新分支重建。
/// `branch(cond_value, cx)` 只在切换到该分支时执行一次，把该分支子节点注入 `cx`。
///
/// # Examples
///
/// ```
/// use wy_mve::{vbox, when, label};
/// use wy_signal::{create_signal, GetValue};
/// let show = create_signal(true);
/// let node = vbox([
///     when(
///         move || show.get(),
///         |c, cx| {
///             if c {
///                 cx.child(label(|| "有".into()).into_node());
///             } else {
///                 cx.child(label(|| "无".into()).into_node());
///             }
///         },
///     ),
/// ]);
/// # let _ = node;
/// ```
#[must_use]
pub fn when(
    cond: impl Fn() -> bool + 'static,
    branch: impl Fn(bool, &mut NodeContext) + 'static,
) -> Slot {
    Slot::Many(ChildArg::new(move |cx| {
        cx.render_when(cond, branch);
    }))
}

/// `#[component]` 生成的 Props 结构体需实现的 trait：暴露其 builder 类型。
///
/// 让 `rsx!` 无需在调用处写出 `Props` 类型名——经 [`__component`] 由组件函数
/// 反推 `Props`，从而避免「必须额外 import 组件的 Props 类型」这一负担。
pub trait Props: Sized {
    /// 该 Props 对应的 builder 类型（`#[component]` 生成，`Default` 可建）。
    type Builder: Default;
}

/// `rsx!` 内部使用：给定组件函数与其 builder 闭包，构造并调用组件。
///
/// `f` 的签名 `fn(P) -> R` 令编译器从组件函数反推出 `P`（Props），
/// 进而确定闭包参数类型 `P::Builder`，于是调用处无需命名 `Props`。
#[doc(hidden)]
pub fn __component<P, R>(f: fn(P) -> R, build: impl FnOnce(P::Builder) -> P) -> R
where
    P: Props,
{
    f(build(P::Builder::default()))
}

/// 按 `render_for_each` 展开列表子节点（keyed diff）。
///
/// 返回 [`ChildArg`] 而非 `Node`：动态区域（`GetNodes` slot）**直接注入父容器**。
/// 若包成中间 `Node`，`flatten` 不递归进它的子节点，列表项会脱离父容器布局
/// （高度塌成 0、所有项重叠在一行）。
///
/// # Examples
///
/// ```
/// use wy_mve::{vbox, label, for_each};
/// let node = vbox([
///     label(|| "head".into()),
///     for_each(
///         |f| (0..3).for_each(|i| f(i.to_string(), i)),
///         |_k, _v, cx| {
///             cx.child(label(|| "item".into()).into_node())
///         },
///     ),
/// ]);
/// # let _ = node;
/// ```
#[must_use]
pub fn for_each<K, T>(
    each: impl Fn(&mut dyn FnMut(K, T)) + 'static,
    creater: impl Fn(K, Rc<std::cell::RefCell<T>>, &mut NodeContext) + 'static,
) -> Slot
where
    K: Eq + std::hash::Hash + Clone + 'static,
    T: Clone + PartialEq + 'static,
{
    Slot::Many(ChildArg::new(move |cx| {
        cx.render_for_each(each, creater);
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node::{children_nodes, materialize};
    use crate::{button, render_root, text};
    use wy_signal::{GetValue, Signal};

    // ===== rsx! 宏行为测试 =====

    // ===== `#[component]` 的 ctx 形态（对应 ooc `fc` 的 ctx 形参） =====

    /// 首参 `ctx: &mut NodeContext` → body 推迟到挂载期，可自行注入子节点。
    ///
    /// 这是 ooc `fc $ [ctx, ... => ctx.addNode(..)]` 的等价形态：
    /// 组件不再"返回一个节点"，而是"往父容器里放节点"。
    #[test]
    fn ctx_component_injects_children_at_mount() {
        use crate::{component, label, rsx};

        #[component]
        fn pair(ctx: &mut NodeContext) {
            ctx.child(label(|| "a".into()));
            ctx.child(label(|| "b".into()));
        }

        let node = rsx! {
            vbox {
                pair {}
            }
        }
        .into_node();
        assert_eq!(children_nodes(&node).len(), 2, "ctx.child 注入两个子节点");
    }

    /// ctx 组件的普通参数仍走 Props（只是不进 builder 的字段语义变了）。
    #[test]
    fn ctx_component_still_takes_props() {
        use crate::{component, label, rsx};

        #[component]
        fn titled(ctx: &mut NodeContext, title: String) {
            ctx.child(label(move || title.clone()));
        }

        let node = rsx! {
            vbox {
                titled { title: "hello".to_string() }
            }
        }
        .into_node();
        assert_eq!(children_nodes(&node).len(), 1);
    }

    /// `#[getter]`：调用处既能传裸值也能传闭包，框架都按取值闭包处理。
    ///
    /// 这是「属性即表达式、自动按帧重算」的落地：组件作者在 prop 上标
    /// `#[getter]`，调用处就不必再写 `move || ...`。
    #[test]
    fn getter_prop_accepts_value_or_closure() {
        use crate::{component, rsx, PointerEvent};
        use std::cell::RefCell;
        use std::rc::Rc;

        let sink: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));

        #[component]
        fn tagger(#[getter] text: String, sink: Rc<RefCell<Vec<String>>>) {
            rsx! {
                node {
                    on_click: move || sink.borrow_mut().push(text()),
                }
            }
            .into_node()
        }

        // 裸值：`"hi".to_string()` 应被自动包成 `move || "hi".to_string()`
        let s1 = Rc::clone(&sink);
        let v = rsx! {
            vbox {
                tagger { text: "hi".to_string(), sink: s1 }
            }
        }
        .into_node();
        let leaf = children_nodes(&v);
        leaf[0].run_on_click(&mut PointerEvent::new(1.0, 1.0));
        assert_eq!(
            *sink.borrow(),
            vec![String::from("hi")],
            "裸值 → 自动取值闭包"
        );

        // 闭包：原有 `|| ...` 写法继续可用
        sink.borrow_mut().clear();
        let s2 = Rc::clone(&sink);
        let c = rsx! {
            vbox {
                tagger { text: || "cb".to_string(), sink: s2 }
            }
        }
        .into_node();
        let leaf = children_nodes(&c);
        leaf[0].run_on_click(&mut PointerEvent::new(1.0, 1.0));
        assert_eq!(*sink.borrow(), vec![String::from("cb")], "闭包写法继续可用");
    }

    /// ctx 组件内 `render_for_each`：信号变化 → 动态区域重建。
    ///
    /// 普通组件做不到这件事——它拿不到 ctx，只能由调用方把 `for_each(..)`
    /// 当 children 传进来。
    #[test]
    fn ctx_component_render_for_each_tracks_signal() {
        use crate::{component, label, rsx};
        use wy_signal::{create_signal, SetValue};

        #[component]
        fn rows(ctx: &mut NodeContext, source: impl Fn() -> Vec<String>) {
            ctx.render_for_each(
                move |emit| {
                    for s in source() {
                        emit(s.clone(), s);
                    }
                },
                |_k, item, cx| cx.child(label(move || item.borrow().clone())),
            );
        }

        let data = create_signal(vec![String::from("x")]);
        let node = rsx! {
            vbox {
                rows { source: move || data.get() }
            }
        }
        .into_node();
        assert_eq!(children_nodes(&node).len(), 1);
        data.set(vec![String::from("x"), String::from("y")]);
        assert_eq!(children_nodes(&node).len(), 2, "信号变化 → 区域重建");
    }

    // ===== `provide` / `consume`：跨层级透传 =====

    /// `provide` 的值要能穿过**嵌套容器**到达深层组件。
    ///
    /// 容器是延迟物化的（`materialize` 在绘制期才跑），那时父上下文早已消失，
    /// 所以 `provided` 必须随节点往下盖一层快照——这条测试就是盯这条链。
    #[test]
    fn provided_value_reaches_deep_child() {
        use crate::{component, label, rsx};
        use std::cell::RefCell;

        // 最里层：只靠 consume 取值，不接任何 prop
        #[component]
        fn leaf(ctx: &mut NodeContext, sink: Rc<RefCell<Vec<String>>>) {
            let v = ctx.consume::<String>().cloned().unwrap_or_default();
            sink.borrow_mut().push(v);
            ctx.child(label(String::new));
        }

        let seen: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
        let sink = Rc::clone(&seen);
        let node = render_root(move |cx| {
            cx.provide(String::from("deep"));
            cx.child(rsx! {
                vbox {
                    vbox {
                        vbox {
                            leaf { sink: Rc::clone(&sink) }
                        }
                    }
                }
            });
        })
        .nodes();
        // 逐层触发物化：provide 的继承正是发生在这一步
        let l1 = children_nodes(&node[0]);
        let l2 = children_nodes(&l1[0]);
        let _ = children_nodes(&l2[0]);
        assert_eq!(*seen.borrow(), vec![String::from("deep")]);
    }

    /// `provide` 的值要能进到 `for_each` 的**每一项**里。
    ///
    /// 列表项是动态区域，构建时机比普通子节点更晚（信号变化才建），
    /// 这条测试保证它们拿到的也是同一份父链快照。
    #[test]
    fn provided_value_reaches_for_each_rows() {
        use crate::{component, label, rsx};
        use std::cell::RefCell;
        use wy_signal::create_signal;

        let seen: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
        let data = create_signal(vec![String::from("a"), String::from("b")]);

        #[component]
        fn rows(ctx: &mut NodeContext, sink: Rc<RefCell<Vec<String>>>) {
            let source = ctx
                .consume::<wy_signal::Signal<Vec<String>>>()
                .copied()
                .expect("应能 consume 到数据源信号");
            ctx.render_for_each(
                move |emit| {
                    for s in source.get() {
                        emit(s.clone(), s);
                    }
                },
                move |_k, item, cx| {
                    let v = cx.consume::<String>().cloned().unwrap_or_default();
                    sink.borrow_mut().push(format!("{v}:{}", item.borrow()));
                    cx.child(label(String::new));
                },
            );
        }

        let sink2 = Rc::clone(&seen);
        let node = render_root(move |cx| {
            cx.provide(String::from("tag"));
            cx.provide(data);
            cx.child(rsx! {
                vbox {
                    rows { sink: Rc::clone(&sink2) }
                }
            });
        })
        .nodes();
        let _ = children_nodes(&node[0]);
        assert_eq!(
            *seen.borrow(),
            vec![String::from("tag:a"), String::from("tag:b")]
        );
    }

    /// 面板 provide 的值要能到达**它自己 push 的普通容器**里的 `for_each` 项。
    ///
    /// 与上一个测试的区别：这里组件注入的是一个**普通 vbox**（不是子组件），
    /// vbox 的子节点是在绘制期物化的——`provided` 必须靠节点上的快照往下带。
    #[test]
    fn provided_reaches_for_each_inside_pushed_container() {
        use crate::{component, for_each, label, rsx, vbox};
        use std::cell::RefCell;

        let seen: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));

        #[component]
        fn panel(ctx: &mut NodeContext, sink: Rc<RefCell<Vec<String>>>) {
            ctx.provide(String::from("panel"));
            let rows = for_each(
                move |emit| {
                    for s in ["a", "b"] {
                        emit(s, s.to_string());
                    }
                },
                move |_k, _v, cx| {
                    let got = cx.consume::<String>().cloned().unwrap_or_default();
                    sink.borrow_mut().push(got);
                    cx.child(label(String::new));
                },
            );
            // 把动态区域塞进一个普通容器再注入——finance 的 panel_with 就是这么做的
            ctx.child(vbox([rows]));
        }

        let sink2 = Rc::clone(&seen);
        let root = render_root(move |cx| {
            cx.child(rsx! { vbox { panel { sink: Rc::clone(&sink2) } } });
        });
        let top = root.nodes();
        let l1 = children_nodes(&top[0]);
        eprintln!("DBG l1={}", l1.len());
        for n in &l1 {
            eprintln!("DBG   kid -> {}", children_nodes(n).len());
        }
        assert_eq!(
            *seen.borrow(),
            vec![String::from("panel"), String::from("panel")],
            "面板 provide 的值应到达它 push 的容器里的 for_each 项"
        );
    }

    /// 项内 `provide` 只覆盖自己这一支，不污染兄弟。
    #[test]
    fn provided_in_one_scope_does_not_leak_to_sibling() {
        use crate::{component, label, rsx};
        use std::cell::RefCell;

        #[derive(Clone, PartialEq)]
        struct Cfg(String);

        // 读 Cfg：读不到就记 "-"
        #[component]
        fn reader(ctx: &mut NodeContext, sink: Rc<RefCell<Vec<String>>>) {
            let v = ctx
                .consume::<Cfg>()
                .map(|c| c.0.clone())
                .unwrap_or_else(|| "-".into());
            sink.borrow_mut().push(v);
            ctx.child(label(String::new));
        }

        // 覆盖 Cfg 后再放一个 reader
        #[component]
        fn overrider(ctx: &mut NodeContext, sink: Rc<RefCell<Vec<String>>>) {
            ctx.provide(Cfg("inner".into()));
            ctx.child(rsx! { reader { sink: sink } });
        }

        let seen: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
        let (s1, s2, s3) = (Rc::clone(&seen), Rc::clone(&seen), Rc::clone(&seen));
        let node = render_root(move |cx| {
            cx.provide(Cfg("outer".into()));
            cx.child(
                rsx! {
                    vbox {
                        reader { sink: Rc::clone(&s1) }
                        overrider { sink: Rc::clone(&s2) }
                        reader { sink: Rc::clone(&s3) }
                    }
                }
                .into_node(),
            );
        })
        .nodes();
        let _ = children_nodes(&node[0]);
        assert_eq!(
            *seen.borrow(),
            vec![
                String::from("outer"),
                String::from("inner"),
                String::from("outer")
            ],
            "项内 provide 只覆盖自己"
        );
    }

    #[test]
    fn rsx_vbox_with_text_and_button() {
        use crate::rsx;
        let slot = rsx! {
            vbox {
                label("head")
                btn { label: "go", on_click: || {} }
            }
        };
        let node = slot.into_node();
        let kids = collect([node]);
        assert_eq!(children_nodes(&kids[0]).len(), 2);
        assert!(
            children_nodes(&kids[0])[1].on_click_fn.is_some(),
            "btn 应有 on_click"
        );
    }

    #[test]
    fn rsx_dynamic_label_wraps_closure() {
        use crate::rsx;
        use std::cell::Cell;
        use std::rc::Rc;
        let n = Rc::new(Cell::new(7));
        let slot = rsx! {
            label(n.get())
        };
        let kids = collect([slot]);
        assert_eq!(kids.len(), 1);
    }

    /// `when(cond) { A } else { B }`：条件信号翻转 → 分支重建（反应式，非构建期 if）。
    #[test]
    fn rsx_when_swaps_branch_on_signal() {
        use crate::rsx;
        use wy_signal::{create_signal, SetValue};
        let show = create_signal(false);
        let slot = rsx! {
            vbox {
                when(show.get()) {
                    label("A")
                    label("A2")
                } else {
                    label("B")
                }
            }
        };
        let node = slot.into_node();
        let kids = collect([node]);
        assert_eq!(children_nodes(&kids[0]).len(), 1, "初始 else 分支 1 个");
        show.set(true);
        assert_eq!(children_nodes(&kids[0]).len(), 2, "翻转后 then 分支 2 个");
        show.set(false);
        assert_eq!(children_nodes(&kids[0]).len(), 1, "再翻转回 else");
    }

    /// 元素块内混入 `let`：按出现顺序求值（先克隆后绑进按钮闭包），不产出节点。
    #[test]
    fn rsx_let_between_children_keeps_order() {
        use crate::node::PointerEvent;
        use crate::rsx;
        use std::cell::Cell;
        use std::rc::Rc;
        let shared = Rc::new(Cell::new(0));
        let slot = rsx! {
            vbox {
                let c = Rc::clone(&shared);
                btn { label: "a", on_click: move || c.set(c.get() + 1) }
                let c = Rc::clone(&shared);
                btn { label: "b", on_click: move || c.set(c.get() + 10) }
            }
        };
        let node = slot.into_node();
        let kids = collect([node]);
        let children = children_nodes(&kids[0]);
        assert_eq!(children.len(), 2, "let 语句不产出节点");
        children[0].run_on_click(&mut PointerEvent::new(0.0, 0.0));
        assert_eq!(shared.get(), 1, "第一个按钮闭包持有自己的克隆");
        children[1].run_on_click(&mut PointerEvent::new(0.0, 0.0));
        assert_eq!(shared.get(), 11, "第二个按钮闭包独立于第一个");
    }

    /// 根级 `let`：rsx! 顶层混入语句，先求值再产出根节点。
    #[test]
    fn rsx_root_level_let() {
        use crate::rsx;
        use std::cell::Cell;
        use std::rc::Rc;
        let shared = Rc::new(Cell::new(5));
        let slot = rsx! {
            let c = Rc::clone(&shared);
            let n = c.get() * 2;
            label(format!("n={n}"))
        };
        let kids = collect([slot]);
        assert_eq!(kids.len(), 1, "根级 let 不产出节点");
    }

    /// 组件块内 `let`：不误判成容器，let 先求值再绑进组件闭包。
    #[test]
    fn rsx_let_inside_component_block() {
        use crate::rsx;
        use std::cell::Cell;
        use std::rc::Rc;
        let shared = Rc::new(Cell::new(0));
        let slot = rsx! {
            vbox {
                btn {
                    let c = Rc::clone(&shared);
                    label: "a",
                    on_click: move || c.set(c.get() + 1),
                }
            }
        };
        let node = slot.into_node();
        let kids = collect([node]);
        let children = children_nodes(&kids[0]);
        assert_eq!(children.len(), 1, "组件块内 let 不产出额外节点");
        assert!(children[0].on_click_fn.is_some(), "btn 仍是组件而非容器");
    }

    /// 组件记录式参数：text_field 必填 + opts 覆盖，其余走 Default。
    #[test]
    fn rsx_component_record_opts() {
        use crate::rsx;
        use crate::text_field::TextFieldOpts;
        use std::cell::RefCell;
        use std::rc::Rc;
        let value = Rc::new(RefCell::new(String::from("hi")));
        let value2 = Rc::clone(&value);
        let slot = rsx! {
            text_field {
                value: move || value2.borrow().clone(),
                on_change: { let v = Rc::clone(&value); move |t| { *v.borrow_mut() = t; } },
                width: 220.0,
                password: true,
            }
        };
        let node = slot.into_node();
        // 记录式参数编译通过即证明：width/password 进 opts，其余 Default
        assert_eq!(TextFieldOpts::default().height, 34.0, "未覆盖字段走默认");
        assert_eq!(TextFieldOpts::default().font_size, 14.0);
        let _ = node;
    }

    /// 组件缺必填参数：编译错误（此处仅验证正常路径，错误路径由宏在编译期报）。
    #[test]
    fn rsx_component_required_and_default() {
        use crate::rsx;
        let empty = String::new();
        let slot = rsx! {
            rich_text {
                value: move || empty.clone(),
                on_change: |_| {},
            }
        };
        let _ = slot.into_node();
    }

    /// `#[component]`：普通函数包装为组件，rsx! 以 `name { field: .. }` 调用。
    #[test]
    fn rsx_component_attribute_macro() {
        use crate::{component, rsx};

        #[component]
        fn stat_card(title: String, value: f32, children: Vec<Slot>) -> Node {
            let _ = title;
            let mut n = vbox(children);
            n.gap = value;
            n
        }

        let slot = rsx! {
            stat_card {
                title: "总访问".to_string(),
                value: 8.0,
                children: Vec::<Slot>::new(),
            }
        };
        let node = slot.into_node();
        assert_eq!(node.gap(), 8.0, "组件参数 value 传入并生效");
        let kids = collect([node]);
        assert!(children_nodes(&kids[0]).is_empty(), "children 为空");
    }

    /// `#[component]` 的缺省参数与 `impl Fn` 参数（setter 泛型收闭包）。
    #[test]
    fn rsx_component_defaults_and_closure_param() {
        use crate::{component, rsx};

        #[component]
        fn chip(text: impl Fn() -> String, #[props(default)] variant: u8) -> Node {
            let _ = text;
            let mut n = vbox(Vec::<Slot>::new());
            n.gap = variant as f32;
            n
        }

        // 省略 variant → 走默认值 0；text 为必填闭包。
        let slot = rsx! {
            chip { text: || "hi".to_string() }
        };
        let node = slot.into_node();
        assert_eq!(node.gap(), 0.0, "variant 缺省为 0");

        // 显式提供 variant。
        let slot = rsx! {
            chip { text: || "hi".to_string(), variant: 5 }
        };
        assert_eq!(slot.into_node().gap(), 5.0, "variant 显式传入");
    }

    /// `#[props(default = expr)]` 显式默认值。
    #[test]
    fn rsx_component_explicit_default() {
        use crate::{component, rsx};

        #[component]
        fn box_gap(#[props(default = 7i32)] gap: i32) -> Node {
            let mut n = vbox(Vec::<Slot>::new());
            n.gap = gap as f32;
            n
        }

        let slot = rsx! {
            box_gap {}
        };
        assert_eq!(slot.into_node().gap(), 7.0, "显式默认值生效");
    }

    /// `children` 插槽：块内**裸子节点**经 `.children(vec![..])` 路由。
    #[test]
    fn rsx_component_children_slot_routes_bare_nodes() {
        use crate::{component, rsx};

        #[component]
        fn wrap(children: Vec<Slot>) -> Node {
            vbox(children)
        }

        let slot = rsx! {
            wrap {
                label("a")
                label("b")
            }
        };
        let node = slot.into_node();
        let kids = collect([node]);
        assert_eq!(
            children_nodes(&kids[0]).len(),
            2,
            "子节点路由到 children 插槽"
        );
    }

    #[test]
    #[allow(unused_braces)]
    fn rsx_brace_expr_child() {
        use crate::rsx;

        let inner = vbox([label(|| "a".into())]);
        let slot = rsx! {
            vbox {
                { inner }
            }
        };
        let node = slot.into_node();
        let kids = collect([node]);
        assert_eq!(
            children_nodes(&kids[0]).len(),
            1,
            "`{{ expr }}` 块表达式子节点"
        );
    }

    #[test]
    #[allow(unused_braces)]
    fn rsx_brace_expr_child_in_component() {
        use crate::{component, rsx};

        #[component]
        fn wrap2(children: Vec<Slot>) -> Node {
            vbox(children)
        }

        let inner = vbox([label(|| "a".into())]);
        let slot = rsx! {
            wrap2 {
                { inner }
            }
        };
        let node = slot.into_node();
        let kids = collect([node]);
        assert_eq!(
            children_nodes(&kids[0]).len(),
            1,
            "组件内 `{{ expr }}` 子节点"
        );
    }

    /// 缺省字段缺省、必填字段缺失 → `build()` panic（附字段名）。
    #[test]
    #[should_panic(expected = "缺少必填 prop")]
    fn rsx_component_missing_required_panics() {
        use crate::{component, rsx};

        #[component]
        fn need(x: i32) -> Node {
            let _ = x;
            vbox(Vec::<Slot>::new())
        }

        let _ = rsx! {
            need {}
        };
    }

    #[test]
    fn rsx_static_attrs_assign_fields() {
        use crate::rsx;
        let slot = rsx! {
            vbox {
                gap: 12.0,
                hide: false,
                hbox {
                    width: 300.0,
                    label("x")
                }
            }
        };
        let n = slot.into_node();
        assert_eq!(n.gap(), 12.0, "f32 字面量直赋静态字段");
        assert_eq!(n.gap, 12.0);
        assert!(!n.is_hidden());
        let kids = collect([n]);
        let hbox = &children_nodes(&kids[0])[0];
        assert_eq!(hbox.width, 300.0);
    }

    #[test]
    fn rsx_dynamic_attr_binds_signal() {
        use crate::rsx;
        use wy_signal::{create_signal, GetValue, SetValue};
        let gap = create_signal(4.0f32);
        let vis = create_signal(true);
        let slot = rsx! {
            vbox {
                gap: gap.get(),
                hide: !vis.get(),
                label("x")
            }
        };
        let n = slot.into_node();
        assert_eq!(n.gap(), 4.0);
        assert!(!n.is_hidden());
        assert!(n.gap_fn.is_some(), "非字面量应包成 gap_fn");
        assert!(n.hide_fn.is_some(), "非字面量应包成 hide_fn");
        gap.set(32.0);
        vis.set(false);
        assert_eq!(n.gap(), 32.0, "信号变化后动态 gap 生效");
        assert!(n.is_hidden(), "信号变化后动态 hide 生效");
    }

    #[test]
    fn rsx_nested_containers() {
        use crate::rsx;
        let slot = rsx! {
            vbox {
                hbox {
                    label("a")
                    label("b")
                }
                label("c")
            }
        };
        let node = slot.into_node();
        let kids = collect([node]);
        let inner = children_nodes(&kids[0]);
        assert_eq!(inner.len(), 2, "hbox + label");
        assert_eq!(children_nodes(&inner[0]).len(), 2, "hbox 内 2 项");
    }

    #[test]
    fn rsx_conditional_none_skipped() {
        use crate::rsx;
        let show = false;
        let slot = rsx! {
            vbox {
                label("always")
                if show {
                    label("hidden")
                }
            }
        };
        let node = slot.into_node();
        let kids = collect([node]);
        assert_eq!(children_nodes(&kids[0]).len(), 1, "false 分支不产出节点");
    }

    #[test]
    fn rsx_conditional_true_present() {
        use crate::rsx;
        let show = true;
        let slot = rsx! {
            vbox {
                label("always")
                if show {
                    label("shown")
                }
            }
        };
        let node = slot.into_node();
        let kids = collect([node]);
        assert_eq!(children_nodes(&kids[0]).len(), 2);
    }

    #[test]
    fn rsx_for_each_injects_into_parent() {
        use crate::rsx;
        let list = Signal::new(vec![1i32, 2, 3]);
        let slot = rsx! {
            vbox {
                for_each(list, |i: &i32| i.to_string()) { |item, _cx| {
                    label(item.borrow().to_string())
                }}
            }
        };
        let node = slot.into_node();
        let kids = collect([node]);
        let inner = children_nodes(&kids[0]);
        assert_eq!(inner.len(), 3, "3 个列表项直接进 vbox");
    }

    #[test]
    fn rsx_for_each_height_accounts_items() {
        use crate::rsx;
        let list = Signal::new(vec![1i32, 2, 3]);
        let slot = rsx! {
            vbox {
                // 显式 8.0：Node 默认 gap 是 0（对齐 Kotlin 上游）
                gap: 8.0,
                for_each(list, |i: &i32| i.to_string()) { |_item, _cx| {
                    btn { label: "row", on_click: || {} }
                }}
            }
        };
        let node = slot.into_node();
        let kids = collect([node]);
        // btn 固定 90x32，3 项 + 2 个 8 gap
        let (_, h) = crate::node::node_size(&kids[0]);
        assert_eq!(h, 3.0 * 32.0 + 2.0 * 8.0, "父容器高度含全部列表项");
    }

    #[test]
    fn rsx_focus_flags() {
        use crate::rsx;
        let a = rsx! { focusable { label("x") } };
        let b = rsx! { focus_trap { label("y") } };
        let kids = collect([a, b]);
        assert!(kids[0].focusable);
        assert!(kids[1].focus_trap);
    }

    #[test]
    fn vbox_children_materialize() {
        let n = vbox([text(|| "a".into()), text(|| "b".into())]);
        let kids = children_nodes(&n);
        assert_eq!(kids.len(), 2);
        assert!(kids.iter().all(|k| !k.hidden));
    }

    #[test]
    fn hbox_nested_vbox() {
        let n = hbox([vbox([text(|| "a".into())]), text(|| "b".into())]);
        let kids = children_nodes(&n);
        assert_eq!(kids.len(), 2);
        assert_eq!(children_nodes(&kids[0]).len(), 1);
    }

    #[test]
    fn vec_and_array_both_accepted() {
        let a = vbox(vec![text(|| "a".into()), text(|| "b".into())]);
        let b = vbox([text(|| "a".into()), text(|| "b".into())]);
        assert_eq!(children_nodes(&a).len(), children_nodes(&b).len());
    }

    #[test]
    fn nested_containers_flatten_via_collect() {
        // group 摊平：子节点直接进父级
        let n = group([text(|| "a".into()), text(|| "b".into())]);
        assert_eq!(children_nodes(&n).len(), 2);
        assert!(n.layout.is_none(), "group 无布局");
    }

    #[test]
    fn dsl_matches_closure_form() {
        // 关键等价性：DSL 写法 == 闭包写法（结构一致）
        let dsl = vbox([text(|| "标题".into()), button(|| "确定".into(), || {})]);

        let closure = crate::components::column(|cx| {
            cx.child(text(|| "标题".into()));
            cx.child(button(|| "确定".into(), || {}));
        });

        let a = children_nodes(&dsl);
        let b = children_nodes(&closure);
        assert_eq!(a.len(), b.len());
        assert_eq!(dsl.layout, closure.layout);
    }

    #[test]
    fn child_arg_expands_to_nodes() {
        // ChildArg 产出的多节点直接摊平进父容器（无额外包装层）
        let n = vbox([ChildArg::new(|cx| {
            cx.child(text(|| "x".into()));
            cx.child(text(|| "y".into()));
        })]);
        let kids = children_nodes(&n);
        assert_eq!(kids.len(), 2);
    }

    #[test]
    fn child_arg_single_node_is_transparent() {
        // ChildArg 只产出 1 个节点时也不多包一层
        let n = vbox([ChildArg::new(|cx| {
            cx.child(text(|| "only".into()));
        })]);
        let kids = children_nodes(&n);
        assert_eq!(kids.len(), 1);
        // 直接就是文本节点本身（无额外包装）
        assert!(kids[0].on_click_fn.is_none());
    }

    #[test]
    fn for_each_expands_list_into_parent() {
        // 回归：`for_each` 必须把 GetNodes 直接注入父容器，
        // 否则 flatten 不递归进中间节点，列表项脱离父容器布局（重叠在一行）。
        let n = vbox([
            label(|| "head".into()),
            for_each(
                |f| (0..3).for_each(|i| f(i.to_string(), i)),
                |k, _v, cx| {
                    let k2 = k.clone();
                    cx.child(text(move || k2.clone()))
                },
            ),
        ]);
        let kids = children_nodes(&n);
        // head + 3 个列表项，全部作为 vbox 的直接子节点参与布局
        assert_eq!(kids.len(), 4);
        assert!(kids[0].on_click_fn.is_none(), "第 1 项是 head");
    }

    #[test]
    fn for_each_standalone_collects() {
        let list: Vec<Slot> = vec![for_each(
            |f| (0..2).for_each(|i| f(i.to_string(), i)),
            |k, _v, cx| {
                let k2 = k.clone();
                cx.child(text(move || k2.clone()))
            },
        )];
        assert_eq!(collect(list).len(), 2);
    }

    #[test]
    fn for_each_layout_height_accounts_items() {
        // 关键回归：vbox 的自然高度必须包含 for_each 的所有列表项。
        // 修复前 for_each 是中间 Node，高度塌成 0 → 所有项 y 重叠。
        // node_size 递归 children_nodes，3 项各 32 高 + 2 个 8 gap = 112
        // （vbox_gap 显式给间距：Node 默认 gap 是 0，对齐 Kotlin 上游）
        let n = vbox_gap(
            8.0,
            [for_each(
                |f| (0..3).for_each(|i| f(i.to_string(), i)),
                |_k, _v, cx| {
                    cx.child(Node {
                        width: 90.0,
                        height: 32.0,
                        ..Node::default()
                    })
                },
            )],
        );
        // node_size 递归 children_nodes，3 项各 32 高 + 2 个 8 gap = 112
        let (_, h) = crate::node::node_size(&n);
        assert_eq!(h, 3.0 * 32.0 + 2.0 * 8.0);
    }

    #[test]
    fn focusable_and_trap_flags_set() {
        assert!(focusable([text(|| "a".into())]).focusable);
        assert!(focus_trap([text(|| "a".into())]).focus_trap);
        assert!(hidden(text(|| "a".into())).hidden);
    }

    #[test]
    fn render_root_with_dsl_produces_flat_nodes() {
        let root = render_root(|cx| {
            cx.child(vbox([text(|| "a".into()), text(|| "b".into())]));
        });
        // render_root 只扁平化顶层，不下钻容器
        assert_eq!(root.nodes().len(), 1);
    }

    #[test]
    fn option_child_skipped_when_none() {
        // Option 作为数组元素：None 跳过
        let some: Option<Slot> = Some(label(|| "a".into()));
        let none: Option<Slot> = None;
        let slots: Vec<Slot> = [some, none].into_iter().flatten().collect();
        let n = vbox(slots);
        assert_eq!(children_nodes(&n).len(), 1);
    }

    #[test]
    fn gap_variants() {
        let v = vbox_gap(0.0, [text(|| "a".into())]);
        assert_eq!(v.layout, Some(Layout::Column));
        assert_eq!(v.gap(), 0.0);
        let h = hbox_gap(16.0, [text(|| "a".into())]);
        assert_eq!(h.layout, Some(Layout::Row));
        assert_eq!(h.gap(), 16.0);
    }

    #[test]
    fn nested_focus_trap_grouping() {
        let n = focus_trap([focusable([text(|| "a".into())])]);
        let kids = children_nodes(&n);
        assert_eq!(kids.len(), 1);
        assert!(kids[0].focusable);
    }

    #[test]
    fn materialize_idempotent() {
        let n = vbox([text(|| "a".into())]);
        materialize(&n);
        let first = children_nodes(&n).len();
        materialize(&n);
        assert_eq!(children_nodes(&n).len(), first);
    }

    /// `draw:` 属性：宏内完成 downcast，闭包直接拿到 `&mut Scene`。
    ///
    /// 这是「组件化而不退回绘制」的关键一环——自绘组件不必再手写
    /// `Node { draw_fn: Rc::new(|any| { let Some(s) = any.downcast_mut() .. }) }`。
    #[test]
    fn rsx_draw_attr_gets_scene_directly() {
        use crate::rsx;
        use std::cell::Cell;
        use std::rc::Rc;
        use wy_render::{Color, Rect, Scene};

        let hits = Rc::new(Cell::new(0));
        let h = Rc::clone(&hits);
        let slot = rsx! {
            vbox {
                width: 40.0,
                height: 10.0,
                draw: move |s: &mut Scene| {
                    h.set(h.get() + 1);
                    s.fill_rect(Rect::new(0.0, 0.0, 5.0, 5.0), Color::from_u32(0xFF0000FF));
                }
            }
        };
        let node = slot.into_node();
        let mut scene = Scene::new();
        node.run_draw(&mut scene);
        assert_eq!(hits.get(), 1, "draw 闭包应被调用");
        assert_eq!(scene.len(), 1, "应记录一个图元");
    }

    /// `..items`：把动态子节点列表展开进 rsx 的容器。
    #[test]
    fn rsx_spread_expands_slot_list() {
        use crate::rsx;

        let extras: Vec<Slot> = vec![
            Slot::One(crate::text(|| String::from("a"))),
            Slot::One(crate::text(|| String::from("b"))),
        ];
        let slot = rsx! {
            vbox {
                label("head")
                ..extras
                label("tail")
            }
        };
        let n = slot.into_node();
        assert_eq!(children_nodes(&n).len(), 4, "1 + 2 + 1");
    }

    /// `node { .. }`：不带子节点时仍是「自绘叶子」——尺寸取声明值、`draw` 照常跑；
    /// 只是它现在默认带 `Column` layout 以便随时容纳子组件（纯叶子形态不变）。
    #[test]
    fn rsx_node_leaf_still_self_draws() {
        use crate::rsx;
        use wy_render::{Color, Rect, Scene};

        let slot = rsx! {
            hbox {
                node {
                    width: 6.0,
                    height: 16.0,
                    draw: |s: &mut Scene| {
                        s.fill_rect(Rect::new(0.0, 0.0, 1.0, 16.0), Color::WHITE);
                    },
                }
            }
        };
        let n = slot.into_node();
        let kids = children_nodes(&n);
        assert_eq!(kids.len(), 1);
        assert_eq!(
            kids[0].layout,
            Some(crate::node::Layout::Column),
            "node 默认带 Column layout（可装子节点）"
        );
        assert!((kids[0].width - 6.0).abs() < 1e-6);
        let mut scene = Scene::new();
        kids[0].run_draw(&mut scene);
        assert_eq!(scene.len(), 1, "纯叶子 draw 仍被调用");
    }

    /// `node { .. }` 带子节点时成为「自绘容器」：子组件按 Column 排布在自身盒内，
    /// 且 `draw` 画背景、子节点叠在上层——对应 Kotlin `Box { drawBg(); content() }` 的匿名类写法，
    /// `#[component]` / `for_each` 等都能平滑嵌套进来。
    #[test]
    fn rsx_node_nests_children_as_container() {
        use crate::rsx;
        use wy_render::{Color, Rect, Scene};

        let slot = rsx! {
            hbox {
                node {
                    width: 40.0,
                    height: 60.0,
                    draw: |s: &mut Scene| {
                        let b = crate::current_box();
                        s.fill_rect(
                            Rect::new(0.0, 0.0, b.width, b.height),
                            Color::from_u32(0x223344FF),
                        );
                    },
                    label("a")
                    label("b")
                }
            }
        };
        let n = slot.into_node();
        let kids = children_nodes(&n);
        assert_eq!(kids.len(), 1, "hbox 里是一个 node 容器");
        let container = &kids[0];
        assert_eq!(container.layout, Some(crate::node::Layout::Column));
        // 子节点被排进 node 内部
        let inner = children_nodes(container);
        assert_eq!(inner.len(), 2, "node 容器容纳了两个 label");
        // 容器自身的 draw 先画背景
        let mut scene = Scene::new();
        container.run_draw(&mut scene);
        assert_eq!(scene.len(), 1, "node 自身的 draw 画了背景");
    }

    /// 组件是「布局节点」：`grow` / `width` 这类通用属性直接作用在组件根节点上，
    /// 不必为每个组件单独开 `w`/`h` prop。
    #[test]
    fn rsx_component_accepts_layout_attrs() {
        use crate::component;
        use crate::rsx;

        #[component]
        fn hint_box(label: String) -> Node {
            crate::text(move || label.clone())
        }

        let slot = rsx! {
            vbox {
                hint_box { label: "x".to_string(), grow: 1.0, width: 30.0 }
            }
        };
        let n = slot.into_node();
        let kids = children_nodes(&n);
        assert_eq!(kids.len(), 1);
        assert!((kids[0].grow - 1.0).abs() < 1e-6);
        assert!((kids[0].width - 30.0).abs() < 1e-6);
    }

    /// `on_down` / `on_up` / `on_drag_move`：拖拽手势三件套。
    #[test]
    fn rsx_drag_handlers_wire_gesture() {
        use crate::node::PointerEvent;
        use crate::rsx;
        use std::rc::Rc;

        let log: Rc<std::cell::RefCell<Vec<&'static str>>> =
            Rc::new(std::cell::RefCell::new(Vec::new()));
        let (d, u, m) = (Rc::clone(&log), Rc::clone(&log), Rc::clone(&log));
        let slot = rsx! {
            vbox {
                width: 6.0,
                on_down: move |_ev: &mut PointerEvent| {
                    d.borrow_mut().push("down");
                },
                on_up: move |_ev: &mut PointerEvent| {
                    u.borrow_mut().push("up");
                },
                on_drag_move: move |_ev: &mut PointerEvent| {
                    m.borrow_mut().push("move");
                },
            }
        };
        let node = slot.into_node();
        node.run_on_down(&mut PointerEvent::new(1.0, 2.0));
        node.run_on_move(&mut PointerEvent::new(3.0, 4.0));
        node.run_on_up(&mut PointerEvent::new(3.0, 4.0));
        assert_eq!(*log.borrow(), vec!["down", "move", "up"]);
    }
}
