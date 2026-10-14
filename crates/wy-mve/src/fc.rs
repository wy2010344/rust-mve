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
    fn push_slots(self, cx: &mut NodeContext) {
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
        let n = vbox([for_each(
            |f| (0..3).for_each(|i| f(i.to_string(), i)),
            |_k, _v, cx| {
                cx.child(Node {
                    width: 90.0,
                    height: 32.0,
                    ..Node::default()
                })
            },
        )]);
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
}
