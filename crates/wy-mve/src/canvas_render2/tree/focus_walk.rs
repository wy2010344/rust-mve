//! 焦点遍历（TS `tree/focusWalk.ts`）。
//!
//! 全是"问节点"的操作：`hide()` / `children()` / `focusable()` / `focusOrder()` /
//! `focusTrap()` / `parent`。副作用（改全局焦点信号、同步宿主焦点）留给调用方，
//! 所以这些函数可以原样单测。

use crate::canvas_render2::node2::Node2;
use crate::canvas_render2::tree::geometry::NodeHit2;

/// 文档序收集全部可聚焦节点（跳过隐藏子树，TS `focusableList`）。
///
/// `children()` 本身已过滤 `hide()`，这里的 `hide()` 判断只作用于**根**——
/// 与 TS 一致（`walk(root)` 一进来先问根的 `hide()`）。
#[must_use]
pub fn focusable_list(root: &Node2) -> Vec<Node2> {
    let mut list = Vec::new();
    walk(root, &mut list);
    list
}

fn walk(n: &Node2, list: &mut Vec<Node2>) {
    if n.hide() {
        return;
    }
    if n.focusable() {
        list.push(n.clone());
    }
    for c in n.children() {
        walk(&c, list);
    }
}

/// 最近的外层焦点圈禁容器（自身算起；无则 `None`，TS `closestTrap`）。
#[must_use]
pub fn closest_trap(from: &Node2) -> Option<Node2> {
    let mut cur = Some(from.clone());
    while let Some(n) = cur {
        if n.focus_trap() {
            return Some(n);
        }
        cur = n.parent();
    }
    None
}

/// `n` 是否在 `ancestor` 的子树里（TS `isDescendantOf`）。
#[must_use]
pub fn is_descendant_of(n: &Node2, ancestor: &Node2) -> bool {
    let mut cur = n.parent();
    while let Some(p) = cur {
        if &p == ancestor {
            return true;
        }
        cur = p.parent();
    }
    false
}

/// 下一个该聚焦的节点（Tab / Shift+Tab，对标 Kotlin `moveFocus`，TS `nextFocusNode`）。
///
/// 焦点在 `focusTrap` 子树内时只在圈内循环，否则全局循环；
/// `focusOrder` 显式排序优先（未指定者保持文档序，读取式、每项只求值一次）。
/// 返回 `None` 表示没有可聚焦节点（调用方决定是否 preventDefault）。
#[must_use]
pub fn next_focus_node(root: &Node2, current: Option<&Node2>, forward: bool) -> Option<Node2> {
    let all = focusable_list(root);
    if all.is_empty() {
        return None;
    }
    let mut scope = all.clone();
    let trap = current.and_then(closest_trap);
    if let Some(trap) = trap {
        let in_trap: Vec<Node2> = all
            .into_iter()
            .filter(|n| n == &trap || is_descendant_of(n, &trap))
            .collect();
        if !in_trap.is_empty() {
            scope = in_trap;
        }
    }
    // focusOrder 显式排序优先（未指定者保持文档序）
    // 每项只求值一次：先物化成 (index, order) 再排，不在比较器里反复发消息。
    let ordered: Vec<(usize, Option<i32>)> = scope
        .iter()
        .enumerate()
        .map(|(i, n)| (i, n.focus_order()))
        .collect();
    if ordered.iter().any(|(_, o)| o.is_some()) {
        let mut idx: Vec<usize> = (0..scope.len()).collect();
        idx.sort_by_key(|&i| {
            let (_, o) = ordered[i];
            (o.unwrap_or(i32::MAX), i)
        });
        scope = idx.into_iter().map(|i| scope[i].clone()).collect();
    }
    let idx = match current {
        Some(c) => scope.iter().position(|n| n == c),
        None => None,
    };
    match idx {
        None => {
            if forward {
                scope.first().cloned()
            } else {
                scope.last().cloned()
            }
        }
        Some(i) => {
            let len = scope.len();
            Some(scope[(i + len + if forward { 1 } else { len - 1 }) % len].clone())
        }
    }
}

/// 命中链中最深的可聚焦节点（叶子优先，与 Kotlin focusable 查找一致，
/// TS `focusableLeafOf`）。
#[must_use]
pub fn focusable_leaf_of(path: &[impl NodeHit2]) -> Option<Node2> {
    for it in path.iter().rev() {
        let n = it.hit_node();
        if n.focusable() && !n.hide() {
            return Some(n.clone());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canvas_render2::node2::{Node2, Node2Props};
    use crate::canvas_render2::tree::geometry::hit_path;
    use crate::{Fc, NodeContext};
    use std::rc::Rc;

    fn leaf(w: f32, h: f32) -> Node2Props {
        Node2Props {
            width: Some(Rc::new(move || w)),
            height: Some(Rc::new(move || h)),
            ..Node2Props::default()
        }
    }

    fn attach_child(parent: &Node2, arg: Node2Props) {
        let mut cx = NodeContext::new();
        cx.node2_parent = Some(parent.clone());
        Fc::new(move |cx| {
            Node2::empty().build(arg).mount(cx);
        })
        .mount(&mut cx);
    }

    fn focusable() -> Node2Props {
        let mut a = leaf(20.0, 20.0);
        a.focusable = Rc::new(|| true);
        a
    }

    #[test]
    fn focusable_list_skips_hidden_subtrees() {
        let root = Node2::empty().build(leaf(200.0, 200.0));
        // 可见分支：a / a1
        let mut a = leaf(50.0, 50.0);
        a.focusable = Rc::new(|| true);
        attach_child(&root, a);
        let a_node = root.children()[0].clone();
        attach_child(&a_node, focusable());
        // 隐藏分支：b（可聚焦）+ b1（可聚焦）——整棵子树都不该出现
        let mut b = leaf(50.0, 50.0);
        b.hide = Rc::new(|| true);
        b.focusable = Rc::new(|| true);
        attach_child(&root, b);
        let b_node = root.raw_children()[1].clone();
        attach_child(&b_node, focusable());

        let list = focusable_list(&root);
        assert_eq!(list.len(), 2, "隐藏子树里的可聚焦节点不算数");
    }

    #[test]
    fn next_focus_node_cycles_in_document_order() {
        let root = Node2::empty().build(leaf(200.0, 200.0));
        attach_child(&root, focusable());
        attach_child(&root, focusable());
        attach_child(&root, focusable());
        let kids = root.children();
        let (a, b, c) = (kids[0].clone(), kids[1].clone(), kids[2].clone());

        assert_eq!(next_focus_node(&root, None, true), Some(a.clone()));
        assert_eq!(next_focus_node(&root, None, false), Some(c.clone()));
        assert_eq!(next_focus_node(&root, Some(&a), true), Some(b.clone()));
        assert_eq!(next_focus_node(&root, Some(&a), false), Some(c.clone()));
        assert_eq!(next_focus_node(&root, Some(&c), true), Some(a.clone()));
    }

    #[test]
    fn next_focus_node_honors_focus_order() {
        let root = Node2::empty().build(leaf(200.0, 200.0));
        // 文档序 0/1/2，显式 order 让第 3 个排到最前
        let mut x = focusable();
        x.focus_order = Rc::new(|| Some(2));
        let mut y = focusable();
        y.focus_order = Rc::new(|| Some(1));
        let mut z = focusable();
        z.focus_order = Rc::new(|| Some(0));
        attach_child(&root, x);
        attach_child(&root, y);
        attach_child(&root, z);
        let kids = root.children();
        let (a, _b, c) = (kids[0].clone(), kids[1].clone(), kids[2].clone());

        // 有显式 order 时按 order 排：z(0) → y(1) → x(2)
        assert_eq!(
            next_focus_node(&root, None, true),
            Some(c.clone()),
            "order 0 排最前"
        );
        assert_eq!(
            next_focus_node(&root, Some(&a), true),
            Some(c.clone()),
            "末位后回到首位"
        );
    }

    #[test]
    fn focus_trap_cycles_only_inside_itself() {
        // root → trap(可聚焦 + focusTrap) → inner(可聚焦)；root → outside(可聚焦)
        let root = Node2::empty().build(leaf(200.0, 200.0));
        let mut t = focusable();
        t.focus_trap = Rc::new(|| true);
        attach_child(&root, t);
        let trap = root.children()[0].clone();
        attach_child(&trap, focusable());
        attach_child(&root, focusable());

        assert_eq!(closest_trap(&trap), Some(trap.clone()));
        // 圈内只有 trap 与 inner，从 inner 前进应回到 trap，而不是跑到 outside
        let inner = trap.children()[0].clone();
        assert_eq!(
            next_focus_node(&root, Some(&inner), true),
            Some(trap.clone())
        );
    }

    #[test]
    fn focusable_leaf_of_picks_deepest_focusable() {
        let mut root_a = leaf(100.0, 100.0);
        root_a.focusable = Rc::new(|| true);
        root_a.x = Some(Rc::new(|| 0.0));
        root_a.y = Some(Rc::new(|| 0.0));
        let root = Node2::empty().build(root_a);
        let mut child_a = leaf(50.0, 50.0);
        child_a.x = Some(Rc::new(|| 0.0));
        child_a.y = Some(Rc::new(|| 0.0));
        child_a.focusable = Rc::new(|| true);
        attach_child(&root, child_a);

        let path = hit_path(&root, 10.0, 10.0);
        let leaf_node = focusable_leaf_of(&path).expect("命中链上应当有可聚焦节点");
        assert_eq!(leaf_node, root.children()[0], "取最深的那个");
    }
}
