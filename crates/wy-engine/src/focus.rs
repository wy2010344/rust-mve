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
            eprintln!("[IME] dispatch: no focus path, event dropped");
            return false;
        }
        match locate(&self.path, root_nodes) {
            Some(chain) => {
                eprintln!("[IME] dispatch: chain len={}, event={:?}", chain.len(), event);
                for (i, node) in chain.iter().enumerate() {
                    if node.ime_fn.is_some() {
                        eprintln!("[IME] dispatch: node[{}] has ime_fn, calling", i);
                    }
                    if node.run_ime(event) {
                        eprintln!("[IME] dispatch: consumed by node[{}]", i);
                        return true;
                    }
                }
                eprintln!("[IME] dispatch: not consumed by any node");
            }
            None => {
                eprintln!("[IME] dispatch: locate returned None");
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
pub fn focusable_nodes(root_nodes: &[Node]) -> Vec<Vec<Node>> {
    let mut out = Vec::new();
    let mut chain: Vec<Node> = Vec::new();
    collect_focusable(root_nodes, &mut chain, &mut out);
    out
}

fn collect_focusable(nodes: &[Node], chain: &mut Vec<Node>, out: &mut Vec<Vec<Node>>) {
    for node in nodes {
        // 自我（文档序在前）
        chain.push(node.clone());
        if node.focusable && !node.hidden {
            // 契约：焦点链为 叶→根（与 find_path/locate/hit_test_node 一致），
            // 此处收集到的 chain 是 根→叶，反转后再入列。
            out.push(chain.iter().rev().cloned().collect());
        }
        collect_focusable(&children_nodes(node), chain, out);
        chain.pop();
    }
}

/// Tab 焦点遍历：从当前焦点（身份链叶子）前进/后退一个 focusable。
///
/// `forward=true` 正向（Tab），`false` 反向（Shift+Tab）。
/// 返回新焦点链（叶子→根）；无元素返回空。
pub fn tab_navigate(
    root_nodes: &[Node],
    current: &FocusPath,
    forward: bool,
    wrap: bool,
) -> FocusPath {
    let list = focusable_nodes(root_nodes);
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

#[cfg(test)]
mod tests {
    use super::*;
    use wy_mve::{button, text, Key, KeyEvent, Node};

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
}
