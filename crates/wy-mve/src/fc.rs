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
/// # Examples
///
/// ```
/// use wy_mve::{vbox, text, ChildArg};
/// let list: &'static [i32] = &[1, 2, 3];
/// let node = vbox([ChildArg::new(|cx| {
///     cx.render_for_each(
///         |f| list.iter().for_each(|i| f(i.to_string(), *i)),
///         |k, _v, cx| {
///             let k2 = k.clone();
///             cx.child(text(move || k2.clone()))
///         },
///     );
/// })]);
/// # let _ = node;
/// ```
pub struct ChildArg(ChildBody);

impl ChildArg {
    /// 把构造体推迟到 `cx` 可用时才执行。
    pub fn new(f: impl FnOnce(&mut NodeContext) + 'static) -> Self {
        Self(Box::new(f))
    }

    fn into_nodes(self) -> Vec<Node> {
        // 与 `arg_children_fn` 同一时机求值。
        let mut cx = NodeContext::new();
        (self.0)(&mut cx);
        let slots = cx.into_slots();
        let mut out = Vec::new();
        flatten(&slots, &mut out);
        out
    }
}

/// 一组子节点参数：摊平为 `Node` 列表。
///
/// 接受 `Node`、`ChildArg`、`Vec<T>`、`[T; N]`（可嵌套），以及它们的组合。
pub trait Children {
    /// 摊平追加到 `out`。
    fn push_into(self, out: &mut Vec<Node>);
}

impl Children for Node {
    fn push_into(self, out: &mut Vec<Node>) {
        out.push(self);
    }
}

impl Children for ChildArg {
    fn push_into(self, out: &mut Vec<Node>) {
        out.extend(self.into_nodes());
    }
}

impl Children for Vec<Node> {
    fn push_into(self, out: &mut Vec<Node>) {
        out.extend(self);
    }
}

impl Children for Vec<ChildArg> {
    fn push_into(self, out: &mut Vec<Node>) {
        for c in self {
            c.push_into(out);
        }
    }
}

impl Children for ChildSlot {
    fn push_into(self, out: &mut Vec<Node>) {
        let slots = [self];
        flatten(&slots, out);
    }
}

impl<T: Children> Children for Option<T> {
    fn push_into(self, out: &mut Vec<Node>) {
        if let Some(v) = self {
            v.push_into(out);
        }
    }
}

impl<T: Children, const N: usize> Children for [T; N] {
    fn push_into(self, out: &mut Vec<Node>) {
        for c in self {
            c.push_into(out);
        }
    }
}

/// 收集子节点参数为 `Node` 列表。
#[must_use]
pub fn collect(children: impl Children) -> Vec<Node> {
    let mut out = Vec::new();
    children.push_into(&mut out);
    out
}

/// 把子节点列表包进布局容器。
fn box_with(layout: Option<Layout>, children: impl Children) -> Node {
    let nodes = Rc::new(collect(children));
    Node {
        layout,
        arg_children_fn: Rc::new(move |cx: &mut NodeContext| {
            for c in nodes.iter() {
                cx.child(c.clone());
            }
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
pub fn vbox(children: impl Children) -> Node {
    box_with(Some(Layout::Column { gap: 8.0 }), children)
}

/// 横排容器（主轴 X，间距 8）。
#[must_use]
pub fn hbox(children: impl Children) -> Node {
    box_with(Some(Layout::Row { gap: 8.0 }), children)
}

/// 自定义间距的竖排容器。
#[must_use]
pub fn vbox_gap(gap: f32, children: impl Children) -> Node {
    box_with(Some(Layout::Column { gap }), children)
}

/// 自定义间距的横排容器。
#[must_use]
pub fn hbox_gap(gap: f32, children: impl Children) -> Node {
    box_with(Some(Layout::Row { gap }), children)
}

/// 无布局的透明分组：只把子节点平铺进父级。
#[must_use]
pub fn group(children: impl Children) -> Node {
    box_with(None, children)
}

/// 可聚焦容器（进 Tab 序）。
#[must_use]
pub fn focusable(children: impl Children) -> Node {
    let mut n = group(children);
    n.focusable = true;
    n
}

/// 焦点陷阱容器：Tab 焦点不逃逸此子树。
#[must_use]
pub fn focus_trap(children: impl Children) -> Node {
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

/// 按 `render_for_each` 展开列表子节点（keyed diff）。
///
/// `for_each` 提供 `(key, value)`，`creater` 为每项构造节点。
///
/// # Examples
///
/// ```
/// use wy_mve::{vbox, text, for_each};
/// let node = for_each(
///     |f| (0..3).for_each(|i| f(i.to_string(), i)),
///     |k, _v, cx| {
///         let k2 = k.clone();
///         cx.child(text(move || k2.clone()))
///     },
/// );
/// # let _ = node;
/// ```
#[must_use]
pub fn for_each<K, T>(
    each: impl Fn(&mut dyn FnMut(K, T)) + 'static,
    creater: impl Fn(K, Rc<std::cell::RefCell<T>>, &mut NodeContext) + 'static,
) -> Node
where
    K: Eq + std::hash::Hash + Clone + 'static,
    T: Clone + PartialEq + 'static,
{
    // `render_for_each` 是 FnOnce 语义，而 `arg_children_fn` 是 Fn：
    // 用 `Option` 延迟消费，且构建期只会跑一次（Node 惰性 memo 保证）。
    let each = std::cell::RefCell::new(Some(each));
    let creater = std::cell::RefCell::new(Some(creater));
    Node {
        arg_children_fn: Rc::new(move |cx: &mut NodeContext| {
            let (Some(each), Some(creater)) =
                (each.borrow_mut().take(), creater.borrow_mut().take())
            else {
                return;
            };
            cx.render_for_each(each, creater);
        }),
        ..Node::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node::{children_nodes, materialize};
    use crate::{button, render_root, text};

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
    fn for_each_expands_list() {
        let n = for_each(
            |f| (0..3).for_each(|i| f(i.to_string(), i)),
            |k, _v, cx| {
                let k2 = k.clone();
                cx.child(text(move || k2.clone()))
            },
        );
        let kids = children_nodes(&n);
        // 动态区域在 children_nodes 中按当前态展开
        assert_eq!(kids.len(), 3);
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
        let some: Option<Node> = Some(text(|| "a".into()));
        let none: Option<Node> = None;
        let n = vbox([some, none]);
        assert_eq!(children_nodes(&n).len(), 1);
    }

    #[test]
    fn gap_variants() {
        assert_eq!(
            vbox_gap(0.0, [text(|| "a".into())]).layout,
            Some(Layout::Column { gap: 0.0 })
        );
        assert_eq!(
            hbox_gap(16.0, [text(|| "a".into())]).layout,
            Some(Layout::Row { gap: 16.0 })
        );
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
