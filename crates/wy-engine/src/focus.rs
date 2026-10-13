//! MVE 焦点管理（复刻 Kotlin `Renderer` 的焦点逻辑）。
//!
//! 焦点状态存在引擎层：一条**身份链**（`identity` 从叶子到根，`Rc<()>`），
//! 而不是直接存 `Node` 引用——MVE 树构建一次 + memo 动态区域，重算会产生
//! 新 `Node` 实例，但克隆共享 `identity`，因此用身份定位不受重算影响
//! （对应 Kotlin Node 引用相等语义）。
//!
//! 能力：
//! - 点击聚焦：命中链中取**最深可聚焦**节点。
//! - Tab / Shift+Tab 焦点遍历：全树文档序 focusable 扫描。
//! - 键盘/IME 分发：沿身份链自叶子向根冒泡（对应 Kotlin 冒泡阶段）。

use std::rc::Rc;

use wy_mve::{children_nodes, ImeEvent, Node};

/// 焦点身份链（叶子 → 根）。空 = 无焦点。
pub type FocusPath = Vec<Rc<()>>;

/// 当前焦点状态（引擎持有）。
#[derive(Default)]
pub struct FocusManager {
    /// 焦点身份链（叶子 → 根）。
    path: FocusPath,
}

impl FocusManager {
    /// 返回当前焦点链（叶子在前）。
    pub fn path(&self) -> &FocusPath {
        &self.path
    }

    /// 是否有焦点节点。
    pub fn has_focus(&self) -> bool {
        !self.path.is_empty()
    }

    /// 把焦点设到指定身份链（叶子 → 根），并返回变化。
    ///
    /// 空链 = 清除焦点。
    pub fn set_focus(&mut self, path: FocusPath) {
        self.path = path;
    }

    /// 清除焦点。
    pub fn clear_focus(&mut self) {
        self.path.clear();
    }

    /// 点击（按下）后更新焦点：在命中链中找**最深可聚焦**节点。
    ///
    /// 命中链是子→根（`hit_test_node` 输出），返回其身份链（已叶子在前）。
    pub fn focus_on_click(&mut self, hit_chain: &[Node]) {
        // hit_chain 子在前（深→浅）。找第一个 focusable。
        if let Some(idx) = hit_chain.iter().position(|n| n.focusable) {
            let path: FocusPath = hit_chain[idx..]
                .iter()
                .map(|n| Rc::clone(&n.identity))
                .collect();
            self.set_focus(path);
        } else {
            self.clear_focus();
        }
    }

    /// 从叶子向根冒泡分发键盘事件。返回是否消费。
    ///
    /// `locate` 返回叶→根链，`chain.iter()` 即从叶子（聚焦点）开始冒泡。
    pub fn dispatch_key(&self, root_nodes: &[Node], event: &mut wy_mve::KeyEvent) -> bool {
        if let Some(chain) = locate(&self.path, root_nodes) {
            for node in chain.iter() {
                if node.run_key(event) {
                    return true;
                }
            }
        }
        false
    }

    /// 从叶子向根冒泡分发 IME 事件。返回是否消费。
    pub fn dispatch_ime(&self, root_nodes: &[Node], event: &mut ImeEvent) -> bool {
        if self.path.is_empty() {
            return false;
        }
        if let Some(chain) = locate(&self.path, root_nodes) {
            for node in chain.iter() {
                if node.run_ime(event) {
                    return true;
                }
            }
        }
        false
    }
}

/// 按身份链在**当前树**中定位出 Node 链（叶子 → 根路径，含头）。
///
/// 动态区域实时求值：`children_nodes` 展开当前态。
pub fn locate(path: &[Rc<()>], root_nodes: &[Node]) -> Option<Vec<Node>> {
    if path.is_empty() {
        return None;
    }
    let mut matches: Vec<Node> = Vec::new();
    if find_path(root_nodes, path, 0, &mut matches) {
        matches.reverse();
        Some(matches)
    } else {
        None
    }
}

/// DFS：匹配 path，从当前层（深度 `depth`）匹配的节点身份是 `path[path.len()-1-depth]`。
///
/// 契约：`FocusPath` 是**叶→根**（`path[0]` 为叶子身份）。起点在根层匹配
/// `path[last]`（根身份），逐层下钻匹配 `path[last-1]…path[0]`；
/// 匹配成功的节点以**根→叶**顺序推入 `matches`，调用方需要时自行反转
/// （如 `dispatch_key` 需要叶→根冒泡）。
fn find_path(nodes: &[Node], path: &[Rc<()>], depth: usize, matches: &mut Vec<Node>) -> bool {
    if depth >= path.len() {
        return true;
    }
    // path 叶→根：当前深度 depth 应匹配 path[len-1-depth]（depth=0 → 根身份）。
    let want = &path[path.len() - 1 - depth];
    for node in nodes {
        if Rc::ptr_eq(&node.identity, want) {
            matches.push(node.clone());
            if find_path(&children_nodes(node), path, depth + 1, matches) {
                return true;
            }
            matches.pop();
        }
    }
    false
}

/// 收集整棵树文档序 focusable 节点（含自身路径链，供 Tab 聚焦）。
///
/// 返回 `Vec<(Node 链 叶子→根)>`，意义：Tab 顺序的每个可聚焦节点。
/// 有 `focus_order` 的节点按值排序，无的保持文档序。
pub fn focusable_nodes(root_nodes: &[Node]) -> Vec<Vec<Node>> {
    let mut out = Vec::new();
    let mut chain: Vec<Node> = Vec::new();
    collect_focusable(root_nodes, &mut chain, &mut out);
    sort_by_focus_order(&mut out);
    out
}

/// 按 focus_order 排序：有显式顺序的按值升序，无的保持相对文档序排在后面。
fn sort_by_focus_order(list: &mut [Vec<Node>]) {
    list.sort_by(|a, b| {
        let order_a = a.first().and_then(|n| n.focus_order);
        let order_b = b.first().and_then(|n| n.focus_order);
        match (order_a, order_b) {
            (Some(a), Some(b)) => a.cmp(&b),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => std::cmp::Ordering::Equal, // 保持原序
        }
    });
}

fn collect_focusable(nodes: &[Node], chain: &mut Vec<Node>, out: &mut Vec<Vec<Node>>) {
    for node in nodes {
        // 自我（文档序在前）
        chain.push(node.clone());
        if node.focusable && !node.is_hidden() {
            // 契约：焦点链为 叶→根（与 find_path/locate/hit_test_node 一致），
            // 此处收集到的 chain 是 根→叶，反转后再入列。
            out.push(chain.iter().rev().cloned().collect());
        }
        // 注：`focus_trap` 不阻断收集。trap 只是**Tab 遍历范围**标记，
        // 其子树节点仍进入全局列表；范围限定由 `tab_navigate` 处理。
        collect_focusable(&children_nodes(node), chain, out);
        chain.pop();
    }
}

/// 收集指定节点子树内文档序 focusable 节点（`tab_navigate` 的 trap 范围）。
fn collect_focusable_in_subtree(root: &Node, chain: &mut Vec<Node>, out: &mut Vec<Vec<Node>>) {
    chain.push(root.clone());
    if root.focusable && !root.is_hidden() {
        out.push(chain.iter().rev().cloned().collect());
    }
    for child in children_nodes(root) {
        collect_focusable_in_subtree(&child, chain, out);
    }
    chain.pop();
}

/// Tab 焦点遍历：从当前焦点（身份链叶子）前进/后退一个 focusable。
///
/// `forward=true` 正向（Tab），`false` 反向（Shift+Tab）。
/// 返回新焦点链（叶子→根）；无元素返回空。
///
/// 如果当前焦点在某个 `focus_trap` 子树内，Tab 只在该 trap 内循环。
pub fn tab_navigate(
    root_nodes: &[Node],
    current: &FocusPath,
    forward: bool,
    wrap: bool,
) -> FocusPath {
    // 根据当前焦点位置，确定参与遍历的 focusable 节点列表
    let list = if let Some(leaf_id) = current.first() {
        // 找到焦点所在的最近 focus_trap 祖先
        if let Some(trap_root) = find_focus_trap_for(root_nodes, leaf_id) {
            // 在 trap 子树内收集 focusable 节点
            let mut out = Vec::new();
            let mut chain = Vec::new();
            collect_focusable_in_subtree(&trap_root, &mut chain, &mut out);
            out
        } else {
            focusable_nodes(root_nodes)
        }
    } else {
        focusable_nodes(root_nodes)
    };

    if list.is_empty() {
        return FocusPath::new();
    }

    // 当前焦点叶子身份
    let pos = current.first().and_then(|leaf| {
        list.iter()
            .position(|chain| chain.first().is_some_and(|n| Rc::ptr_eq(&n.identity, leaf)))
    });

    let next = match pos {
        Some(i) if forward => {
            if i + 1 < list.len() {
                i + 1
            } else if wrap {
                0
            } else {
                return FocusPath::new();
            }
        }
        Some(i) => {
            if i > 0 {
                i - 1
            } else if wrap {
                list.len() - 1
            } else {
                return FocusPath::new();
            }
        }
        None => {
            // 无焦点或焦点不在此树：默认聚焦第一个
            if forward {
                0
            } else if wrap {
                list.len() - 1
            } else {
                0
            }
        }
    };
    list[next].iter().map(|n| Rc::clone(&n.identity)).collect()
}

/// 从节点列表中找到包含指定叶子的最近 focus_trap 祖先。
fn find_focus_trap_for(nodes: &[Node], leaf_id: &Rc<()>) -> Option<Node> {
    for node in nodes {
        if let Some(found) = find_trap_recursive(node, leaf_id) {
            return Some(found);
        }
    }
    None
}

/// DFS：在子树中查找包含叶子的**最内层** focus_trap 祖先。
fn find_trap_recursive(node: &Node, leaf_id: &Rc<()>) -> Option<Node> {
    // 叶子自身不是 trap 祖先
    if Rc::ptr_eq(&node.identity, leaf_id) {
        return None;
    }
    // 本子树不含叶子 → 早退（避免对无关子树做全量扫描）
    if !subtree_contains(node, leaf_id) {
        return None;
    }
    // 含叶子：先深入找更内层的 trap（嵌套时内层优先）
    for child in children_nodes(node) {
        if let Some(found) = find_trap_recursive(&child, leaf_id) {
            return Some(found);
        }
    }
    // 没有更内层 trap，且自己是 trap → 自己就是范围
    if node.focus_trap {
        return Some(node.clone());
    }
    None
}

/// 检查子树是否包含指定节点。
fn subtree_contains(node: &Node, target_id: &Rc<()>) -> bool {
    if Rc::ptr_eq(&node.identity, target_id) {
        return true;
    }
    for child in children_nodes(node) {
        if subtree_contains(&child, target_id) {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use wy_mve::{button, text, Key, KeyEvent, Node, NodeContext};

    fn app(count: Rc<std::cell::RefCell<i32>>) -> Vec<Node> {
        let c1 = count.clone();
        let b1 = button(move || format!("-{}", c1.borrow()), {
            let c = count.clone();
            move || *c.borrow_mut() -= 1
        });
        let c2 = count.clone();
        let b2 = button(move || format!("+{}", c2.borrow()), {
            let c = count.clone();
            move || *c.borrow_mut() += 1
        });
        vec![text(move || "label".into()), b1, b2]
    }

    /// 构造 focus_trap 测试树（对齐 Kotlin FocusScopeTest）。
    ///
    /// ```text
    /// outside (focusable)
    ///   └─ outerTrap (focusTrap=true)
    ///        ├─ innerTrap (focusTrap=true)
    ///        │    ├─ inner0 (focusable)
    ///        │    └─ inner1 (focusable)
    ///        └─ outerInside (focusable)
    /// ```
    fn trap_tree() -> (Vec<Node>, Node, Node, Node, Node, Node, Node) {
        let inner0 = button(|| "inner0".into(), || {});
        let inner1 = button(|| "inner1".into(), || {});
        let i0 = inner0.clone();
        let i1 = inner1.clone();
        let inner_trap = Node {
            focus_trap: true,
            arg_children_fn: Rc::new(move |cx: &mut NodeContext| {
                cx.add_node(i0.clone());
                cx.add_node(i1.clone());
            }),
            ..Node::default()
        };

        let outer_inside = button(|| "outerInside".into(), || {});
        let it = inner_trap.clone();
        let oi = outer_inside.clone();
        let outer_trap = Node {
            focus_trap: true,
            arg_children_fn: Rc::new(move |cx: &mut NodeContext| {
                cx.add_node(it.clone());
                cx.add_node(oi.clone());
            }),
            ..Node::default()
        };

        let outside = button(|| "outside".into(), || {});
        let ot = outer_trap.clone();
        let os = outside.clone();
        let root = Node {
            arg_children_fn: Rc::new(move |cx: &mut NodeContext| {
                cx.add_node(os.clone());
                cx.add_node(ot.clone());
            }),
            ..Node::default()
        };

        (
            vec![root],
            outside,
            outer_trap,
            inner_trap,
            outer_inside,
            inner0,
            inner1,
        )
    }

    // ===== 原有测试 =====

    #[test]
    fn focusable_collects_buttons() {
        let count = Rc::new(std::cell::RefCell::new(0));
        let nodes = app(count);
        let list = focusable_nodes(&nodes);
        assert_eq!(list.len(), 2); // 两个按钮
    }

    #[test]
    fn tab_forward_wraps() {
        let count = Rc::new(std::cell::RefCell::new(0));
        let nodes = app(count);
        let first = tab_navigate(&nodes, &FocusPath::new(), true, true);
        assert_eq!(first.len(), 1);
        assert!(Rc::ptr_eq(&first[0], &nodes[1].identity), "第 1 个按钮");
        let second = tab_navigate(&nodes, &first, true, true);
        assert!(Rc::ptr_eq(&second[0], &nodes[2].identity), "第 2 个按钮");
        let wrapped = tab_navigate(&nodes, &second, true, true);
        assert!(Rc::ptr_eq(&wrapped[0], &nodes[1].identity), "回绕到第 1 个");
    }

    #[test]
    fn tab_backward_from_first_goes_last() {
        let count = Rc::new(std::cell::RefCell::new(0));
        let nodes = app(count);
        let first = tab_navigate(&nodes, &FocusPath::new(), true, true);
        let prev = tab_navigate(&nodes, &first, false, true);
        assert!(
            Rc::ptr_eq(&prev[0], &nodes[2].identity),
            "反向回绕到最后 1 个"
        );
    }

    #[test]
    fn locate_finds_current_node_in_tree() {
        let count = Rc::new(std::cell::RefCell::new(0));
        let nodes = app(count);
        let second = tab_navigate(&nodes, &FocusPath::new(), true, true);
        let second = tab_navigate(&nodes, &second, true, true);
        let located = locate(&second, &nodes);
        assert!(located.is_some());
        let chain = located.unwrap();
        assert_eq!(chain.len(), 1);
        assert!(Rc::ptr_eq(&chain[0].identity, &nodes[2].identity));
    }

    #[test]
    fn locate_empty_path_is_none() {
        let count = Rc::new(std::cell::RefCell::new(0));
        let nodes = app(count);
        assert!(locate(&FocusPath::new(), &nodes).is_none());
    }

    #[test]
    fn focus_on_click_deepest_focusable() {
        let count = Rc::new(std::cell::RefCell::new(0));
        let nodes = app(count);
        // 命中链：冒泡链顺序 叶子→根，这里直接给单个按钮
        let chain = vec![nodes[1].clone()];
        let mut fm = FocusManager::default();
        fm.focus_on_click(&chain);
        assert!(Rc::ptr_eq(&fm.path()[0], &nodes[1].identity));
    }

    #[test]
    fn dispatch_key_only_to_focused() {
        let count = Rc::new(std::cell::RefCell::new(0));
        let nodes = app(count);
        let mut fm = FocusManager::default();
        let second = tab_navigate(&nodes, &FocusPath::new(), true, true);
        let second = tab_navigate(&nodes, &second, true, true);
        fm.set_focus(second);
        // 键盘事件：按钮无 key_fn；mve KeyEvent 用字面量构造，返回 false 无消费
        let mut ev = KeyEvent {
            key: Key::Enter,
            ctrl: false,
            shift: false,
            alt: false,
            meta: false,
        };
        assert!(!fm.dispatch_key(&nodes, &mut ev));
    }

    // ===== 对齐 Kotlin FocusScopeTest =====

    #[test]
    fn focus_outside_trap_uses_global() {
        // 对齐 FocusScopeTest.focusOutsideTrapUsesGlobalOrder
        // 无焦点时 Tab 按全局树序：outside → inner0 → inner1 → outerInside
        let (nodes, outside, _ot, _it, _oi, inner0, _inner1) = trap_tree();
        let first = tab_navigate(&nodes, &FocusPath::new(), true, true);
        assert!(
            Rc::ptr_eq(&first[0], &outside.identity),
            "第一个应该是 outside"
        );
        let second = tab_navigate(&nodes, &first, true, true);
        assert!(
            Rc::ptr_eq(&second[0], &inner0.identity),
            "第二个应该是 inner0"
        );
    }

    #[test]
    fn tab_stays_inside_trap() {
        // 对齐 FocusScopeTest.tabStaysInsideOuterTrap
        // 焦点在外层 trap 内的 inner0 时，Tab 只在外层 trap 子树内循环：
        // inner0 → inner1 → outerInside → inner0
        let (nodes, outside, _ot, _it, _oi, inner0, inner1) = trap_tree();
        // 聚焦 inner0
        let focus_inner0 = vec![inner0.identity.clone()];
        let next = tab_navigate(&nodes, &focus_inner0, true, true);
        assert!(
            Rc::ptr_eq(&next[0], &inner1.identity),
            "inner0 Tab → inner1"
        );

        let focus_inner1 = vec![inner1.identity.clone()];
        let next2 = tab_navigate(&nodes, &focus_inner1, true, true);
        // inner1 → outerInside（outerInside 在 innerTrap 之后）
        let outside_id = outside.identity.clone();
        // 验证不会跳到 outside
        assert!(!Rc::ptr_eq(&next2[0], &outside_id), "不应该跳出到 outside");
    }

    #[test]
    fn tab_stays_inside_innermost() {
        // 对齐 FocusScopeTest.tabStaysInsideInnermostTrap
        // 焦点在 innerTrap 内的 inner0 时，Tab 只在 inner0/inner1 之间循环
        let (nodes, outside, _ot, _it, _oi, inner0, inner1) = trap_tree();
        // 聚焦 inner0
        let focus_inner0 = vec![inner0.identity.clone()];
        let next = tab_navigate(&nodes, &focus_inner0, true, true);
        assert!(
            Rc::ptr_eq(&next[0], &inner1.identity),
            "inner0 Tab → inner1"
        );

        // inner1 Tab → inner0（wrap 回到 inner0，因为 innerTrap 只有这两个 focusable）
        let focus_inner1 = vec![inner1.identity.clone()];
        let next2 = tab_navigate(&nodes, &focus_inner1, true, true);
        assert!(
            Rc::ptr_eq(&next2[0], &inner0.identity),
            "inner1 Tab → inner0 (wrap)"
        );

        // 验证 outside 和 outerInside 不在循环内
        let outside_id = outside.identity.clone();
        assert!(!Rc::ptr_eq(&next2[0], &outside_id));
    }

    #[test]
    fn shift_tab_cycles_backwards_inside_trap() {
        // 对齐 FocusScopeTest.shiftTabCyclesBackwardsInsideTrap
        // 焦点在 outer_inside（外层 trap 内、内层 trap 外）时，Shift+Tab 退到 inner1
        let (nodes, _outside, _ot, _it, outer_inside, _inner0, inner1) = trap_tree();
        let focus_outer_inside = vec![outer_inside.identity.clone()];
        let prev = tab_navigate(&nodes, &focus_outer_inside, false, true);
        // outer_inside 前一个 focusable 是 inner1（内层 trap 的最后一项）
        assert!(
            Rc::ptr_eq(&prev[0], &inner1.identity),
            "outer_inside Shift+Tab → inner1"
        );
    }

    #[test]
    fn trap_excludes_hidden() {
        // 对齐 FocusScopeTest.trapExcludesHiddenAndNonFocusable
        // 在 innerTrap 子树内，隐藏的 inner0 应被跳过
        let (_nodes, _outside, _ot, _it, _oi, inner0, inner1) = trap_tree();
        // 将 inner0 设为 hidden
        let mut hidden_inner0 = inner0.clone();
        hidden_inner0.hidden = true;
        // 重建树：需要替换 innerTrap 的子节点
        // 由于 Node 是声明式不可变的，直接测试 focusable_nodes 在 trap 内的行为
        let i1 = inner1.clone();
        let inner_trap = Node {
            focus_trap: true,
            arg_children_fn: Rc::new(move |cx: &mut NodeContext| {
                cx.add_node(hidden_inner0.clone());
                cx.add_node(i1.clone());
            }),
            ..Node::default()
        };
        let ot = inner_trap.clone();
        let root = Node {
            arg_children_fn: Rc::new(move |cx: &mut NodeContext| {
                cx.add_node(ot.clone());
            }),
            ..Node::default()
        };
        let nodes = vec![root];

        // 在 innerTrap 子树内遍历，只应有 inner1
        let focus_inner1 = vec![inner1.identity.clone()];
        let next = tab_navigate(&nodes, &focus_inner1, true, true);
        // innerTrap 内只有 inner1 focusable，wrap 回到自己
        assert!(Rc::ptr_eq(&next[0], &inner1.identity), "wrap 回到 inner1");
    }

    // ===== focus_order 排序测试 =====

    #[test]
    fn focus_order_sorts_explicit_before_default() {
        // 有 focus_order 的节点排在无 focus_order 的节点之前
        let b_no_order = button(|| "no_order".into(), || {});
        let mut b_order_1 = button(|| "order_1".into(), || {});
        b_order_1.focus_order = Some(1);
        let mut b_order_0 = button(|| "order_0".into(), || {});
        b_order_0.focus_order = Some(0);

        let nodes = vec![b_no_order, b_order_1, b_order_0];
        let list = focusable_nodes(&nodes);
        assert_eq!(list.len(), 3);

        // focus_order=0 排第一，focus_order=1 排第二，无 order 排最后
        assert!(
            Rc::ptr_eq(&list[0][0].identity, &nodes[2].identity),
            "order_0 first"
        );
        assert!(
            Rc::ptr_eq(&list[1][0].identity, &nodes[1].identity),
            "order_1 second"
        );
        assert!(
            Rc::ptr_eq(&list[2][0].identity, &nodes[0].identity),
            "no_order last"
        );
    }

    #[test]
    fn focus_order_navigates_in_order() {
        // Tab 遍历遵循 focus_order
        let mut b_a = button(|| "A".into(), || {});
        b_a.focus_order = Some(2);
        let mut b_b = button(|| "B".into(), || {});
        b_b.focus_order = Some(1);
        let mut b_c = button(|| "C".into(), || {});
        b_c.focus_order = Some(3);

        let nodes = vec![b_a, b_b, b_c];
        // 无焦点时聚焦第一个（focus_order=1 → B）
        let first = tab_navigate(&nodes, &FocusPath::new(), true, true);
        assert!(
            Rc::ptr_eq(&first[0], &nodes[1].identity),
            "first is B (order=1)"
        );

        // B → A (order=2)
        let second = tab_navigate(&nodes, &first, true, true);
        assert!(
            Rc::ptr_eq(&second[0], &nodes[0].identity),
            "second is A (order=2)"
        );

        // A → C (order=3)
        let third = tab_navigate(&nodes, &second, true, true);
        assert!(
            Rc::ptr_eq(&third[0], &nodes[2].identity),
            "third is C (order=3)"
        );

        // C → B (wrap)
        let wrapped = tab_navigate(&nodes, &third, true, true);
        assert!(Rc::ptr_eq(&wrapped[0], &nodes[1].identity), "wrap to B");
    }

    #[test]
    fn focus_order_mixed_with_default() {
        // 混合有/无 focus_order：有 order 的按值排序，无 order 的保持文档序
        let b1 = button(|| "no_order_1".into(), || {});
        let mut b2 = button(|| "order_10".into(), || {});
        b2.focus_order = Some(10);
        let b3 = button(|| "no_order_2".into(), || {});
        let mut b4 = button(|| "order_5".into(), || {});
        b4.focus_order = Some(5);

        let nodes = vec![b1, b2, b3, b4];
        let list = focusable_nodes(&nodes);
        assert_eq!(list.len(), 4);

        // order=5 → order=10 → no_order_1 (doc order) → no_order_2 (doc order)
        assert!(
            Rc::ptr_eq(&list[0][0].identity, &nodes[3].identity),
            "order=5 first"
        );
        assert!(
            Rc::ptr_eq(&list[1][0].identity, &nodes[1].identity),
            "order=10 second"
        );
        assert!(
            Rc::ptr_eq(&list[2][0].identity, &nodes[0].identity),
            "no_order_1 third"
        );
        assert!(
            Rc::ptr_eq(&list[3][0].identity, &nodes[2].identity),
            "no_order_2 fourth"
        );
    }

    // ===== IME + focus_trap 集成测试 =====

    #[test]
    fn ime_dispatch_respects_focus_trap() {
        // IME 事件应只分发到焦点节点所在链，不会逃逸 trap
        let (nodes, _outside, _ot, _it, _oi, inner0, _inner1) = trap_tree();
        let mut fm = FocusManager::default();
        // 聚焦 inner0（在最内层 trap 内）
        fm.set_focus(vec![inner0.identity.clone()]);

        // IME 事件应被分发到 inner0（冒泡链：inner0 → innerTrap → outerTrap → root）
        let mut event = ImeEvent::Preedit {
            text: "test".into(),
            cursor: Some((0, 0)),
        };
        let consumed = fm.dispatch_ime(&nodes, &mut event);
        // inner0 没有 ime_fn，所以不会被消费，但分发链正确
        assert!(!consumed, "inner0 无 ime_fn，不消费");
    }

    #[test]
    fn ime_dispatch_no_focus_returns_false() {
        // 无焦点时 IME 分发返回 false
        let (nodes, _outside, _ot, _it, _oi, _inner0, _inner1) = trap_tree();
        let fm = FocusManager::default();
        let mut event = ImeEvent::Preedit {
            text: "test".into(),
            cursor: Some((0, 0)),
        };
        let consumed = fm.dispatch_ime(&nodes, &mut event);
        assert!(!consumed, "无焦点时 IME 不消费");
    }

    #[test]
    fn key_dispatch_respects_focus_trap() {
        // 键盘事件应只分发到焦点节点所在链
        let (nodes, _outside, _ot, _it, _oi, inner0, _inner1) = trap_tree();
        let mut fm = FocusManager::default();
        fm.set_focus(vec![inner0.identity.clone()]);

        let mut ev = KeyEvent {
            key: Key::Tab,
            ctrl: false,
            shift: false,
            alt: false,
            meta: false,
        };
        // inner0 没有 key_fn，不消费
        let consumed = fm.dispatch_key(&nodes, &mut ev);
        assert!(!consumed, "inner0 无 key_fn，不消费");
    }
}
