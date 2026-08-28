//! 焦点管理：焦点注册、Tab 遍历、焦点陷阱。
//!
//! 对应 Kotlin `Node.focusable` / `Node.focusOrder` / `Node.focusTrap` /
//! `EngineGlobal.focused` + `moveFocus()`。
//!
//! 焦点模型：
//! - 每个节点可声明 `focusable: bool`（是否参与 Tab 遍历）
//! - `focus_order: Option<u32>` 控制 Tab 顺序（值越小越先，None 排最后按文档序）
//! - `focus_trap: bool` 声明焦点陷阱（Tab 只在子树内循环）
//! - `FocusManager` 维护可焦点节点列表和当前焦点

/// 可焦点节点的信息。
#[derive(Copy, Clone, Debug)]
pub struct FocusableNode {
    /// 节点 ID（由调用方分配的唯一标识）。
    pub id: usize,
    /// Tab 遍历顺序（值越小越先；`None` 排在所有显式顺序之后）。
    pub order: Option<u32>,
    /// 是否为焦点陷阱（Tab 只在以本节点为根的子树内循环）。
    pub trap: bool,
    /// 父节点 ID（用于焦点陷阱的子树范围判断）。
    pub parent_id: Option<usize>,
}

/// 焦点管理器：管理可焦点节点列表和当前焦点。
pub struct FocusManager {
    /// 已注册的可焦点节点（按注册顺序）。
    nodes: Vec<FocusableNode>,
    /// 当前焦点节点 ID；`None` 表示无焦点。
    focused: Option<usize>,
}

impl FocusManager {
    /// 创建空的焦点管理器。
    pub fn new() -> Self {
        Self {
            nodes: Vec::new(),
            focused: None,
        }
    }

    /// 注册一个可焦点节点。
    pub fn register(&mut self, node: FocusableNode) {
        // 避免重复注册
        self.nodes.retain(|n| n.id != node.id);
        self.nodes.push(node);
    }

    /// 注销一个节点。
    pub fn unregister(&mut self, id: usize) {
        self.nodes.retain(|n| n.id != id);
        if self.focused == Some(id) {
            self.focused = None;
        }
    }

    /// 设置焦点到指定节点。
    pub fn set_focus(&mut self, id: usize) -> bool {
        if self.nodes.iter().any(|n| n.id == id) {
            self.focused = Some(id);
            true
        } else {
            false
        }
    }

    /// 清除焦点。
    pub fn clear_focus(&mut self) {
        self.focused = None;
    }

    /// 获取当前焦点节点 ID。
    pub fn focused(&self) -> Option<usize> {
        self.focused
    }

    /// 获取当前焦点节点的引用。
    pub fn focused_node(&self) -> Option<&FocusableNode> {
        self.focused
            .and_then(|id| self.nodes.iter().find(|n| n.id == id))
    }

    /// Tab 遍历：移动焦点到下一个可焦点节点。
    ///
    /// 遍历规则：
    /// 1. 如果当前焦点有 `focus_trap`，只在陷阱子树内遍历
    /// 2. 按 `focus_order` 排序（有显式顺序的在前，无顺序的按文档序）
    /// 3. `shift` 为 true 时反向遍历（Shift+Tab）
    pub fn move_focus(&mut self, shift: bool) -> Option<usize> {
        if self.nodes.is_empty() {
            return None;
        }

        // 获取排序后的焦点节点列表
        let sorted = self.sorted_focusable_nodes();

        // 找到当前焦点在排序列表中的位置
        let current_pos = self
            .focused
            .and_then(|id| sorted.iter().position(|n| n.id == id));

        let next_pos = if let Some(pos) = current_pos {
            if shift {
                if pos == 0 {
                    sorted.len() - 1
                } else {
                    pos - 1
                }
            } else {
                (pos + 1) % sorted.len()
            }
        } else {
            // 无焦点时，从第一个开始
            0
        };

        let next_id = sorted[next_pos].id;
        self.focused = Some(next_id);
        Some(next_id)
    }

    /// 排序后的可焦点节点列表。
    ///
    /// 排序规则：有显式 `order` 的按 order 升序，无 order 的按文档序排在后面。
    fn sorted_focusable_nodes(&self) -> Vec<&FocusableNode> {
        let mut with_order: Vec<&FocusableNode> =
            self.nodes.iter().filter(|n| n.order.is_some()).collect();
        let without_order: Vec<&FocusableNode> =
            self.nodes.iter().filter(|n| n.order.is_none()).collect();

        with_order.sort_by_key(|n| n.order.unwrap());
        // without_order 保持注册顺序（文档序）

        with_order.extend(without_order);
        with_order
    }

    /// 当前焦点是否在指定陷阱节点的子树内。
    pub fn is_focused_in_trap(&self, trap_id: usize) -> bool {
        if let Some(focused_id) = self.focused {
            // 检查焦点节点的父链是否包含 trap_id
            let mut current = self.nodes.iter().find(|n| n.id == focused_id);
            while let Some(node) = current {
                if node.id == trap_id {
                    return true;
                }
                current = node
                    .parent_id
                    .and_then(|pid| self.nodes.iter().find(|n| n.id == pid));
            }
        }
        false
    }
}

impl Default for FocusManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(id: usize) -> FocusableNode {
        FocusableNode {
            id,
            order: None,
            trap: false,
            parent_id: None,
        }
    }

    fn ordered_node(id: usize, order: u32) -> FocusableNode {
        FocusableNode {
            id,
            order: Some(order),
            trap: false,
            parent_id: None,
        }
    }

    #[test]
    fn register_and_focused() {
        let mut fm = FocusManager::new();
        fm.register(node(1));
        fm.register(node(2));
        assert!(fm.set_focus(1));
        assert_eq!(fm.focused(), Some(1));
        assert!(!fm.set_focus(99)); // 未注册的节点
    }

    #[test]
    fn clear_focus() {
        let mut fm = FocusManager::new();
        fm.register(node(1));
        fm.set_focus(1);
        fm.clear_focus();
        assert_eq!(fm.focused(), None);
    }

    #[test]
    fn unregister_removes_focus() {
        let mut fm = FocusManager::new();
        fm.register(node(1));
        fm.set_focus(1);
        fm.unregister(1);
        assert_eq!(fm.focused(), None);
        assert!(fm.focused_node().is_none());
    }

    #[test]
    fn move_focus_forward() {
        let mut fm = FocusManager::new();
        fm.register(node(1));
        fm.register(node(2));
        fm.register(node(3));
        // 无焦点时，move_focus 从第一个开始
        assert_eq!(fm.move_focus(false), Some(1));
        assert_eq!(fm.move_focus(false), Some(2));
        assert_eq!(fm.move_focus(false), Some(3));
        // 循环回第一个
        assert_eq!(fm.move_focus(false), Some(1));
    }

    #[test]
    fn move_focus_backward() {
        let mut fm = FocusManager::new();
        fm.register(node(1));
        fm.register(node(2));
        fm.register(node(3));
        fm.set_focus(2);
        assert_eq!(fm.move_focus(true), Some(1));
        assert_eq!(fm.move_focus(true), Some(3)); // 循环到最后
    }

    #[test]
    fn focus_order_sorting() {
        let mut fm = FocusManager::new();
        fm.register(ordered_node(10, 30)); // order=30
        fm.register(ordered_node(20, 10)); // order=10
        fm.register(ordered_node(30, 20)); // order=20
        fm.register(node(40)); // 无 order

        // 按 order 排序：20(order=10) → 30(order=20) → 10(order=30) → 40(无order)
        assert_eq!(fm.move_focus(false), Some(20));
        assert_eq!(fm.move_focus(false), Some(30));
        assert_eq!(fm.move_focus(false), Some(10));
        assert_eq!(fm.move_focus(false), Some(40));
    }

    #[test]
    fn empty_manager_no_focus() {
        let mut fm = FocusManager::new();
        assert_eq!(fm.move_focus(false), None);
        assert_eq!(fm.focused(), None);
    }

    #[test]
    fn is_focused_in_trap() {
        let mut fm = FocusManager::new();
        fm.register(FocusableNode {
            id: 1,
            order: None,
            trap: true,
            parent_id: None,
        });
        fm.register(FocusableNode {
            id: 2,
            order: None,
            trap: false,
            parent_id: Some(1),
        });
        fm.register(FocusableNode {
            id: 3,
            order: None,
            trap: false,
            parent_id: None,
        });

        fm.set_focus(2);
        assert!(fm.is_focused_in_trap(1)); // 焦点在 trap 1 的子树内
        assert!(!fm.is_focused_in_trap(99)); // 不存在的 trap

        fm.set_focus(3);
        assert!(!fm.is_focused_in_trap(1)); // 焦点不在 trap 1 的子树内
    }
}
