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
    /// 遍历规则（对齐 Kotlin `moveFocus()`）：
    /// 1. 从当前焦点沿 parent 链上溯，找到最近的 `focus_trap` 祖先
    /// 2. 若找到 trap，只在该 trap 子树内遍历；否则全局遍历
    /// 3. 按 `focus_order` 排序（有显式顺序的在前，无顺序的按文档序）
    /// 4. `shift` 为 true 时反向遍历（Shift+Tab）
    pub fn move_focus(&mut self, shift: bool) -> Option<usize> {
        if self.nodes.is_empty() {
            return None;
        }

        // 确定遍历范围：trap 子树 or 全局
        let trap_id = self.find_nearest_trap_ancestor();
        let sorted = self.sorted_focusable_nodes_in_scope(trap_id);

        if sorted.is_empty() {
            return None;
        }

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

    /// 从当前焦点沿 parent 链上溯，返回最近的 `focus_trap` 节点 ID。
    ///
    /// 对齐 Kotlin `findFocusTrap()`：无状态，天然支持嵌套 trap。
    fn find_nearest_trap_ancestor(&self) -> Option<usize> {
        let focused_id = self.focused?;
        let mut current = self.nodes.iter().find(|n| n.id == focused_id);
        while let Some(node) = current {
            if node.trap {
                return Some(node.id);
            }
            current = node
                .parent_id
                .and_then(|pid| self.nodes.iter().find(|n| n.id == pid));
        }
        None
    }

    /// 在指定 scope 内排序后的可焦点节点列表。
    ///
    /// `trap_id = None` 时返回全局列表；`Some(id)` 时只返回该 trap 子树内的节点
    /// （不包含 trap 节点本身，对齐 Kotlin `collectFocusable(trap)` 中 trap 默认不可聚焦）。
    fn sorted_focusable_nodes_in_scope(&self, trap_id: Option<usize>) -> Vec<&FocusableNode> {
        let candidates: Vec<&FocusableNode> = if let Some(tid) = trap_id {
            // 只收集 trap 子树内的后代节点（不包含 trap 节点本身）
            self.nodes
                .iter()
                .filter(|n| self.is_descendant_of(n.id, tid))
                .collect()
        } else {
            self.nodes.iter().collect()
        };

        let mut with_order: Vec<&FocusableNode> = candidates
            .iter()
            .copied()
            .filter(|n| n.order.is_some())
            .collect();
        let without_order: Vec<&FocusableNode> = candidates
            .iter()
            .copied()
            .filter(|n| n.order.is_none())
            .collect();

        with_order.sort_by_key(|n| n.order.unwrap());
        with_order.extend(without_order);
        with_order
    }

    /// 判断 `id` 是否是 `ancestor_id` 的后代（沿 parent 链上溯可达）。
    fn is_descendant_of(&self, id: usize, ancestor_id: usize) -> bool {
        let mut current = self.nodes.iter().find(|n| n.id == id);
        while let Some(node) = current {
            if node.parent_id == Some(ancestor_id) {
                return true;
            }
            current = node
                .parent_id
                .and_then(|pid| self.nodes.iter().find(|n| n.id == pid));
        }
        false
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

    // ===== 对齐 Kotlin FocusScopeTest：trap 内循环 =====

    #[test]
    fn tab_stays_inside_trap() {
        // Kotlin FocusScopeTest#tabStaysInsideOuterTrap
        // trap(A, [A1, A2]) — Tab 在 A1→A2→A1 间循环，不跳出到外部节点
        let mut fm = FocusManager::new();
        fm.register(FocusableNode {
            id: 10,
            order: None,
            trap: true,
            parent_id: None,
        });
        fm.register(FocusableNode {
            id: 11,
            order: None,
            trap: false,
            parent_id: Some(10),
        });
        fm.register(FocusableNode {
            id: 12,
            order: None,
            trap: false,
            parent_id: Some(10),
        });
        fm.register(FocusableNode {
            id: 20,
            order: None,
            trap: false,
            parent_id: None,
        }); // trap 外
        fm.set_focus(11);

        // Tab 应在 trap 内循环：11→12→11
        assert_eq!(fm.move_focus(false), Some(12));
        assert_eq!(fm.move_focus(false), Some(11));
        assert_eq!(fm.move_focus(false), Some(12));
        // 始终在 trap 内
        assert!(fm.is_focused_in_trap(10));
    }

    #[test]
    fn shift_tab_cycles_backwards_inside_trap() {
        // Kotlin FocusScopeTest#shiftTabCyclesBackwardsInsideTrap
        let mut fm = FocusManager::new();
        fm.register(FocusableNode {
            id: 10,
            order: None,
            trap: true,
            parent_id: None,
        });
        fm.register(FocusableNode {
            id: 11,
            order: None,
            trap: false,
            parent_id: Some(10),
        });
        fm.register(FocusableNode {
            id: 12,
            order: None,
            trap: false,
            parent_id: Some(10),
        });
        fm.register(FocusableNode {
            id: 20,
            order: None,
            trap: false,
            parent_id: None,
        });
        fm.set_focus(11);

        // Shift+Tab 应反向循环：11→12→11
        assert_eq!(fm.move_focus(true), Some(12));
        assert_eq!(fm.move_focus(true), Some(11));
    }

    #[test]
    fn focus_outside_trap_uses_global_order() {
        // Kotlin FocusScopeTest#focusOutsideTrapUsesGlobalOrder
        // 无 trap 时，全局 Tab 遍历
        let mut fm = FocusManager::new();
        fm.register(FocusableNode {
            id: 1,
            order: None,
            trap: false,
            parent_id: None,
        });
        fm.register(FocusableNode {
            id: 2,
            order: None,
            trap: false,
            parent_id: None,
        });
        fm.register(FocusableNode {
            id: 3,
            order: None,
            trap: false,
            parent_id: None,
        });
        fm.set_focus(1);

        assert_eq!(fm.move_focus(false), Some(2));
        assert_eq!(fm.move_focus(false), Some(3));
        assert_eq!(fm.move_focus(false), Some(1)); // 全局循环
    }

    #[test]
    fn nested_trap_scopes_to_innermost() {
        // Kotlin FocusScopeTest#tabStaysInsideInnermostTrap
        // outer(trap) → inner(trap) → [I1, I2]
        // 焦点在 inner 子树内时，只在 inner 的子节点间循环
        let mut fm = FocusManager::new();
        fm.register(FocusableNode {
            id: 100,
            order: None,
            trap: true,
            parent_id: None,
        }); // outer
        fm.register(FocusableNode {
            id: 110,
            order: None,
            trap: true,
            parent_id: Some(100),
        }); // inner
        fm.register(FocusableNode {
            id: 111,
            order: None,
            trap: false,
            parent_id: Some(110),
        }); // I1
        fm.register(FocusableNode {
            id: 112,
            order: None,
            trap: false,
            parent_id: Some(110),
        }); // I2
        fm.register(FocusableNode {
            id: 101,
            order: None,
            trap: false,
            parent_id: Some(100),
        }); // outer child (not inner)
        fm.register(FocusableNode {
            id: 20,
            order: None,
            trap: false,
            parent_id: None,
        }); // global
        fm.set_focus(111);

        // 最近 trap 祖先是 inner(110)，只在 I1→I2 间循环
        assert_eq!(fm.move_focus(false), Some(112));
        assert_eq!(fm.move_focus(false), Some(111));
        // 不会跳出到 outer child(101) 或 global(20)
    }

    #[test]
    fn trap_excludes_hidden_and_non_focusable() {
        // Kotlin FocusScopeTest#trapExcludesHiddenAndNonFocusable
        // 注册的节点都是 focusable 的，trap 内只遍历已注册节点
        let mut fm = FocusManager::new();
        fm.register(FocusableNode {
            id: 10,
            order: None,
            trap: true,
            parent_id: None,
        });
        fm.register(FocusableNode {
            id: 11,
            order: None,
            trap: false,
            parent_id: Some(10),
        });
        // id=12 未注册（模拟 hidden/non-focusable）
        fm.register(FocusableNode {
            id: 13,
            order: None,
            trap: false,
            parent_id: Some(10),
        });
        fm.register(FocusableNode {
            id: 20,
            order: None,
            trap: false,
            parent_id: None,
        });
        fm.set_focus(11);

        // 只在 trap 内已注册的节点间循环：11→13→11
        assert_eq!(fm.move_focus(false), Some(13));
        assert_eq!(fm.move_focus(false), Some(11));
    }

    #[test]
    fn register_replaces_existing_node() {
        // 同一 ID 重复注册只保留最新
        let mut fm = FocusManager::new();
        fm.register(FocusableNode {
            id: 1,
            order: None,
            trap: false,
            parent_id: None,
        });
        fm.register(FocusableNode {
            id: 1,
            order: Some(5),
            trap: true,
            parent_id: None,
        });
        assert_eq!(fm.nodes.len(), 1);
        let n = fm.nodes.iter().find(|n| n.id == 1).unwrap();
        assert_eq!(n.order, Some(5));
        assert!(n.trap);
    }

    #[test]
    fn move_focus_single_node_wraps() {
        let mut fm = FocusManager::new();
        fm.register(node(1));
        fm.set_focus(1);
        assert_eq!(fm.move_focus(false), Some(1)); // 唯一节点，循环到自身
    }

    #[test]
    fn focused_node_returns_info() {
        let mut fm = FocusManager::new();
        fm.register(ordered_node(5, 10));
        fm.set_focus(5);
        let info = fm.focused_node().unwrap();
        assert_eq!(info.id, 5);
        assert_eq!(info.order, Some(10));
    }

    #[test]
    fn unregister_nonexistent_is_noop() {
        let mut fm = FocusManager::new();
        fm.register(node(1));
        fm.unregister(99);
        assert_eq!(fm.nodes.len(), 1);
    }
}
