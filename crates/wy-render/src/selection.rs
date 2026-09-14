//! 文本选择管理：跨节点选择协调。
//!
//! 对齐 Kotlin `skia-engine/SelectionManager`。
//!
//! 核心设计：
//! - `Selectable` trait — 可选择文本节点的接口
//! - `SelPoint` / `SelPair` — 选择端点
//! - `SelectionManager` — 协调跨节点文本选择（纯信号驱动）

use std::cell::RefCell;
use std::rc::Rc;

// ===== 可选择节点接口 =====

/// 可选择文本节点的接口。
///
/// 对齐 Kotlin `Selectable` trait。
pub trait Selectable {
    /// 节点 ID。
    fn id(&self) -> usize;

    /// 选择区域的边界框（用于弹出层定位）。
    fn selection_rect(&self) -> Option<SelectionRect>;

    /// 文本总长度。
    fn text_length(&self) -> usize;

    /// 全局坐标 → 文本偏移。
    fn position_for_point(&self, global_x: f32, global_y: f32) -> usize;

    /// 读取指定范围的文本。
    fn text_in_range(&self, start: usize, end: usize) -> String;

    /// 偏移处的词边界。
    fn word_range_at(&self, offset: usize) -> Option<(usize, usize)>;

    /// 偏移处的段落边界。
    fn paragraph_range_at(&self, offset: usize) -> Option<(usize, usize)>;
}

/// 选择区域的边界框。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SelectionRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

// ===== 选择端点 =====

/// 选择端点：节点 + 偏移。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SelPoint {
    /// 节点 ID。
    pub node_id: usize,
    /// 文本偏移。
    pub offset: usize,
}

/// 选择对：锚点 + 焦点。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SelPair {
    /// 锚点（选择起始点，不随拖动变化）。
    pub anchor: SelPoint,
    /// 焦点（选择终点，随拖动变化）。
    pub focus: SelPoint,
}

impl SelPair {
    /// 创建选择对。
    pub fn new(anchor: SelPoint, focus: SelPoint) -> Self {
        Self { anchor, focus }
    }

    /// 是否为折叠选择（光标）。
    pub fn is_collapsed(&self) -> bool {
        self.anchor == self.focus
    }

    /// 按文档序排列端点。
    pub fn ordered(&self) -> (SelPoint, SelPoint) {
        if self.anchor.node_id < self.focus.node_id
            || (self.anchor.node_id == self.focus.node_id
                && self.anchor.offset <= self.focus.offset)
        {
            (self.anchor, self.focus)
        } else {
            (self.focus, self.anchor)
        }
    }
}

// ===== 选择管理器 =====

/// 选择管理器：协调跨节点文本选择。
///
/// 对齐 Kotlin `SelectionManager`：
/// - 纯信号驱动，无命令式 `setSelection` API
/// - 选择范围从底层信号派生
/// - 支持指针拖拽、键盘选择、程序化选择
pub struct SelectionManager {
    /// 已注册的可选择节点。
    selectables: RefCell<Vec<Rc<dyn Selectable>>>,
    /// 当前选择对（锚点 + 焦点）。
    current_pair: RefCell<Option<SelPair>>,
    /// 程序化选择会话（Cmd+A / select API）。
    programmatic: RefCell<Option<SelPair>>,
}

impl SelectionManager {
    /// 创建空的选择管理器。
    pub fn new() -> Self {
        Self {
            selectables: RefCell::new(Vec::new()),
            current_pair: RefCell::new(None),
            programmatic: RefCell::new(None),
        }
    }

    /// 注册可选择节点。
    pub fn register(&self, node: Rc<dyn Selectable>) {
        let id = node.id();
        self.selectables.borrow_mut().retain(|n| n.id() != id);
        self.selectables.borrow_mut().push(node);
    }

    /// 注销节点。
    pub fn unregister(&self, id: usize) {
        self.selectables.borrow_mut().retain(|n| n.id() != id);
    }

    /// 当前有效选择对（5 级优先级链）。
    ///
    /// 1. 程序化会话（Cmd+A / select API）
    /// 2. 当前指针拖拽（锚点固定，焦点跟随）
    /// 3. 当前键盘选择（非折叠光标）
    /// 4. 冻结的指针选择（已释放）
    /// 5. 无选择
    pub fn current_pair(&self) -> Option<SelPair> {
        // 优先级 1：程序化会话
        if let Some(pair) = *self.programmatic.borrow() {
            return Some(pair);
        }
        // 优先级 2-4：当前/冻结的指针或键盘选择
        *self.current_pair.borrow()
    }

    /// 设置当前选择对（指针拖拽/键盘选择）。
    pub fn set_current_pair(&self, pair: Option<SelPair>) {
        *self.current_pair.borrow_mut() = pair;
    }

    /// 全选。
    pub fn select_all(&self) -> Option<SelPair> {
        let selectables = self.selectables.borrow();
        if selectables.is_empty() {
            return None;
        }

        // 按文档序排列
        let mut sorted: Vec<&Rc<dyn Selectable>> = selectables.iter().collect();
        sorted.sort_by_key(|n| n.id());

        let first = sorted.first()?;
        let last = sorted.last()?;

        let anchor = SelPoint {
            node_id: first.id(),
            offset: 0,
        };
        let focus = SelPoint {
            node_id: last.id(),
            offset: last.text_length(),
        };

        let pair = SelPair::new(anchor, focus);
        *self.programmatic.borrow_mut() = Some(pair);
        Some(pair)
    }

    /// 清除选择。
    pub fn clear(&self) {
        *self.programmatic.borrow_mut() = None;
        *self.current_pair.borrow_mut() = None;
    }

    /// 程序化选择指定范围。
    pub fn select(&self, anchor: SelPoint, focus: SelPoint) -> Option<SelPair> {
        let pair = SelPair::new(anchor, focus);
        *self.programmatic.borrow_mut() = Some(pair);
        Some(pair)
    }

    /// 选中文本（聚合所有选中节点的文本）。
    pub fn selected_text(&self) -> String {
        let pair = match self.current_pair() {
            Some(p) => p,
            None => return String::new(),
        };

        let (start, end) = pair.ordered();
        let selectables = self.selectables.borrow();

        // 收集所有在选择范围内的节点
        let mut in_range: Vec<&Rc<dyn Selectable>> = selectables
            .iter()
            .filter(|n| {
                let id = n.id();
                if start.node_id == end.node_id {
                    id == start.node_id
                } else {
                    id == start.node_id
                        || id == end.node_id
                        || (id > start.node_id && id < end.node_id)
                }
            })
            .collect();

        // 按文档序排序
        in_range.sort_by_key(|n| n.id());

        let mut result = String::new();
        for node in in_range {
            let (s, e) = if node.id() == start.node_id && node.id() == end.node_id {
                (start.offset, end.offset)
            } else if node.id() == start.node_id {
                (start.offset, node.text_length())
            } else if node.id() == end.node_id {
                (0, end.offset)
            } else {
                (0, node.text_length())
            };

            if s < e && s < node.text_length() {
                let e = e.min(node.text_length());
                result.push_str(&node.text_in_range(s, e));
            }
        }

        result
    }

    /// 选择区域的边界框（用于弹出层定位）。
    pub fn selected_rect(&self) -> Option<SelectionRect> {
        let pair = self.current_pair()?;
        let (start, end) = pair.ordered();
        let selectables = self.selectables.borrow();

        let mut min_x = f32::MAX;
        let mut min_y = f32::MAX;
        let mut max_x = f32::MIN;
        let mut max_y = f32::MIN;
        let mut found = false;

        for node in selectables.iter() {
            let id = node.id();
            let in_range = if start.node_id == end.node_id {
                id == start.node_id
            } else {
                id >= start.node_id && id <= end.node_id
            };

            if in_range {
                if let Some(rect) = node.selection_rect() {
                    min_x = min_x.min(rect.x);
                    min_y = min_y.min(rect.y);
                    max_x = max_x.max(rect.x + rect.width);
                    max_y = max_y.max(rect.y + rect.height);
                    found = true;
                }
            }
        }

        if found {
            Some(SelectionRect {
                x: min_x,
                y: min_y,
                width: max_x - min_x,
                height: max_y - min_y,
            })
        } else {
            None
        }
    }

    /// 检查节点是否可选择。
    pub fn is_selectable(&self, id: usize) -> bool {
        self.selectables.borrow().iter().any(|n| n.id() == id)
    }

    /// 已注册的可选择节点数量。
    pub fn selectable_count(&self) -> usize {
        self.selectables.borrow().len()
    }

    /// 指针按下：开始新的选择会话。
    pub fn on_pointer_down(&self, node_id: usize, offset: usize) {
        let point = SelPoint { node_id, offset };
        let pair = SelPair::new(point, point);
        *self.programmatic.borrow_mut() = None;
        *self.current_pair.borrow_mut() = Some(pair);
    }

    /// 指针拖拽：更新焦点。
    pub fn on_pointer_move(&self, node_id: usize, offset: usize) {
        let mut pair = self.current_pair.borrow_mut();
        if let Some(ref mut p) = *pair {
            p.focus = SelPoint { node_id, offset };
        }
    }

    /// 指针释放：冻结选择。
    pub fn on_pointer_up(&self) {
        // 冻结：current_pair 保持不变
    }

    /// Shift+点击：从上次锚点扩展选择。
    pub fn on_shift_click(&self, node_id: usize, offset: usize) {
        let focus = SelPoint { node_id, offset };
        // 先读取锚点，释放借用后再写入
        let anchor = self
            .programmatic
            .borrow()
            .map(|p| p.anchor)
            .or_else(|| self.current_pair.borrow().map(|p| p.anchor));
        if let Some(anchor) = anchor {
            let new_pair = SelPair::new(anchor, focus);
            *self.programmatic.borrow_mut() = Some(new_pair);
        }
    }

    /// 双击：选中单词。
    pub fn on_double_click(&self, node_id: usize, offset: usize) {
        let selectables = self.selectables.borrow();
        if let Some(node) = selectables.iter().find(|n| n.id() == node_id) {
            if let Some((word_start, word_end)) = node.word_range_at(offset) {
                let anchor = SelPoint {
                    node_id,
                    offset: word_start,
                };
                let focus = SelPoint {
                    node_id,
                    offset: word_end,
                };
                let pair = SelPair::new(anchor, focus);
                *self.programmatic.borrow_mut() = Some(pair);
                *self.current_pair.borrow_mut() = Some(pair);
            }
        }
    }

    /// 三击：选中段落。
    pub fn on_triple_click(&self, node_id: usize, offset: usize) {
        let selectables = self.selectables.borrow();
        if let Some(node) = selectables.iter().find(|n| n.id() == node_id) {
            if let Some((para_start, para_end)) = node.paragraph_range_at(offset) {
                let anchor = SelPoint {
                    node_id,
                    offset: para_start,
                };
                let focus = SelPoint {
                    node_id,
                    offset: para_end,
                };
                let pair = SelPair::new(anchor, focus);
                *self.programmatic.borrow_mut() = Some(pair);
                *self.current_pair.borrow_mut() = Some(pair);
            }
        }
    }
}

impl Default for SelectionManager {
    fn default() -> Self {
        Self::new()
    }
}

// ===== 辅助函数 =====

/// 展开双击选择：从锚点单词向两侧扩展。
///
/// 对齐 Kotlin `expandWordSelection`。
pub fn expand_word_selection(
    selectables: &[&dyn Selectable],
    anchor_node_id: usize,
    anchor_offset: usize,
    focus_node_id: usize,
    focus_offset: usize,
    expand_right: bool,
) -> Option<SelPair> {
    let anchor_node = selectables.iter().find(|n| n.id() == anchor_node_id)?;
    let (word_start, word_end) = anchor_node.word_range_at(anchor_offset)?;

    if expand_right {
        // 焦点在锚点右侧，焦点扩展到焦点节点的词尾
        let focus_node = selectables.iter().find(|n| n.id() == focus_node_id)?;
        if let Some((_, f_end)) = focus_node.word_range_at(focus_offset) {
            Some(SelPair::new(
                SelPoint {
                    node_id: anchor_node_id,
                    offset: word_start,
                },
                SelPoint {
                    node_id: focus_node_id,
                    offset: f_end,
                },
            ))
        } else {
            None
        }
    } else {
        // 焦点在锚点左侧，焦点扩展到焦点节点的词头
        let focus_node = selectables.iter().find(|n| n.id() == focus_node_id)?;
        if let Some((f_start, _)) = focus_node.word_range_at(focus_offset) {
            Some(SelPair::new(
                SelPoint {
                    node_id: focus_node_id,
                    offset: f_start,
                },
                SelPoint {
                    node_id: anchor_node_id,
                    offset: word_end,
                },
            ))
        } else {
            None
        }
    }
}

/// 比较文档顺序。
///
/// 对齐 Kotlin `compareDocumentOrder`。
pub fn compare_document_order(x_id: usize, y_id: usize) -> std::cmp::Ordering {
    x_id.cmp(&y_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 测试用的 Mock 可选择节点。
    struct MockSelectable {
        id: usize,
        text: String,
    }

    impl Selectable for MockSelectable {
        fn id(&self) -> usize {
            self.id
        }

        fn selection_rect(&self) -> Option<SelectionRect> {
            Some(SelectionRect {
                x: 0.0,
                y: self.id as f32 * 20.0,
                width: 100.0,
                height: 20.0,
            })
        }

        fn text_length(&self) -> usize {
            self.text.len()
        }

        fn position_for_point(&self, _global_x: f32, global_y: f32) -> usize {
            ((global_y - self.id as f32 * 20.0) / 20.0 * self.text.len() as f32)
                .round()
                .max(0.0)
                .min(self.text.len() as f32) as usize
        }

        fn text_in_range(&self, start: usize, end: usize) -> String {
            let s = start.min(self.text.len());
            let e = end.min(self.text.len());
            self.text[s..e].to_string()
        }

        fn word_range_at(&self, offset: usize) -> Option<(usize, usize)> {
            if offset >= self.text.len() {
                return None;
            }
            // 简单的空格分词
            let bytes = self.text.as_bytes();
            let mut start = offset;
            while start > 0 && bytes[start - 1] != b' ' {
                start -= 1;
            }
            let mut end = offset;
            while end < self.text.len() && bytes[end] != b' ' {
                end += 1;
            }
            Some((start, end))
        }

        fn paragraph_range_at(&self, offset: usize) -> Option<(usize, usize)> {
            if offset >= self.text.len() {
                return None;
            }
            // 简单的换行分段
            let bytes = self.text.as_bytes();
            let mut start = offset;
            while start > 0 && bytes[start - 1] != b'\n' {
                start -= 1;
            }
            let mut end = offset;
            while end < self.text.len() && bytes[end] != b'\n' {
                end += 1;
            }
            Some((start, end))
        }
    }

    fn mock(id: usize, text: &str) -> Rc<dyn Selectable> {
        Rc::new(MockSelectable {
            id,
            text: text.to_string(),
        })
    }

    // ===== 基础测试 =====

    #[test]
    fn register_and_count() {
        let sm = SelectionManager::new();
        sm.register(mock(1, "hello"));
        sm.register(mock(2, "world"));
        assert_eq!(sm.selectable_count(), 2);
    }

    #[test]
    fn unregister() {
        let sm = SelectionManager::new();
        sm.register(mock(1, "hello"));
        sm.register(mock(2, "world"));
        sm.unregister(1);
        assert_eq!(sm.selectable_count(), 1);
        assert!(!sm.is_selectable(1));
        assert!(sm.is_selectable(2));
    }

    #[test]
    fn select_all() {
        let sm = SelectionManager::new();
        sm.register(mock(1, "hello"));
        sm.register(mock(2, "world"));
        let pair = sm.select_all().unwrap();
        assert_eq!(pair.anchor.node_id, 1);
        assert_eq!(pair.anchor.offset, 0);
        assert_eq!(pair.focus.node_id, 2);
        assert_eq!(pair.focus.offset, 5); // "world".len()
    }

    #[test]
    fn select_all_empty() {
        let sm = SelectionManager::new();
        assert!(sm.select_all().is_none());
    }

    #[test]
    fn clear_selection() {
        let sm = SelectionManager::new();
        sm.register(mock(1, "hello"));
        sm.select_all();
        assert!(sm.current_pair().is_some());
        sm.clear();
        assert!(sm.current_pair().is_none());
    }

    #[test]
    fn selected_text_single_node() {
        let sm = SelectionManager::new();
        sm.register(mock(1, "hello world"));
        sm.select(
            SelPoint {
                node_id: 1,
                offset: 0,
            },
            SelPoint {
                node_id: 1,
                offset: 5,
            },
        );
        assert_eq!(sm.selected_text(), "hello");
    }

    #[test]
    fn selected_text_cross_node() {
        let sm = SelectionManager::new();
        sm.register(mock(1, "hello"));
        sm.register(mock(2, " "));
        sm.register(mock(3, "world"));
        sm.select(
            SelPoint {
                node_id: 1,
                offset: 2,
            },
            SelPoint {
                node_id: 3,
                offset: 3,
            },
        );
        assert_eq!(sm.selected_text(), "llo wor");
    }

    #[test]
    fn selected_text_backward() {
        let sm = SelectionManager::new();
        sm.register(mock(1, "hello"));
        sm.register(mock(2, "world"));
        // 反向选择：焦点在锚点之前
        sm.select(
            SelPoint {
                node_id: 2,
                offset: 3,
            },
            SelPoint {
                node_id: 1,
                offset: 2,
            },
        );
        assert_eq!(sm.selected_text(), "llowor");
    }

    #[test]
    fn selected_text_empty_when_no_selection() {
        let sm = SelectionManager::new();
        sm.register(mock(1, "hello"));
        assert_eq!(sm.selected_text(), "");
    }

    // ===== 指针交互测试 =====

    #[test]
    fn pointer_down_starts_selection() {
        let sm = SelectionManager::new();
        sm.register(mock(1, "hello"));
        sm.on_pointer_down(1, 2);
        let pair = sm.current_pair().unwrap();
        assert_eq!(pair.anchor, pair.focus);
        assert_eq!(pair.anchor.offset, 2);
    }

    #[test]
    fn pointer_move_updates_focus() {
        let sm = SelectionManager::new();
        sm.register(mock(1, "hello"));
        sm.on_pointer_down(1, 0);
        sm.on_pointer_move(1, 5);
        let pair = sm.current_pair().unwrap();
        assert_eq!(pair.focus.offset, 5);
        assert_eq!(pair.anchor.offset, 0); // 锚点不变
    }

    #[test]
    fn pointer_down_clears_programmatic() {
        let sm = SelectionManager::new();
        sm.register(mock(1, "hello"));
        sm.select_all();
        assert!(sm.current_pair().is_some());
        sm.on_pointer_down(1, 2);
        // 程序化选择应被清除
        let pair = sm.current_pair().unwrap();
        assert_eq!(pair.anchor, pair.focus); // 折叠光标
    }

    #[test]
    fn shift_click_extends_from_anchor() {
        let sm = SelectionManager::new();
        sm.register(mock(1, "hello"));
        sm.register(mock(2, "world"));
        sm.on_pointer_down(1, 0);
        sm.on_shift_click(2, 3);
        let pair = sm.current_pair().unwrap();
        assert_eq!(pair.anchor.node_id, 1);
        assert_eq!(pair.anchor.offset, 0);
        assert_eq!(pair.focus.node_id, 2);
        assert_eq!(pair.focus.offset, 3);
    }

    // ===== 双击/三击测试 =====

    #[test]
    fn double_click_selects_word() {
        let sm = SelectionManager::new();
        sm.register(mock(1, "hello world"));
        sm.on_double_click(1, 2); // 点在 "hello" 中间
        let text = sm.selected_text();
        assert_eq!(text, "hello");
    }

    #[test]
    fn triple_click_selects_paragraph() {
        let sm = SelectionManager::new();
        sm.register(mock(1, "line one\nline two"));
        sm.on_triple_click(1, 5); // 点在第一行
        let text = sm.selected_text();
        assert_eq!(text, "line one");
    }

    // ===== selected_rect 测试 =====

    #[test]
    fn selected_rect_covers_all_selected() {
        let sm = SelectionManager::new();
        sm.register(mock(1, "aaa"));
        sm.register(mock(2, "bbb"));
        sm.register(mock(3, "ccc"));
        sm.select_all();
        let rect = sm.selected_rect().unwrap();
        // mock id*20: node 1 y=20, node 3 y=60
        assert_eq!(rect.y, 20.0); // node 1
        assert_eq!(rect.height, 60.0); // node 3 y=60, height=20, so max=80, min=20, height=60
    }

    #[test]
    fn selected_rect_none_when_no_selection() {
        let sm = SelectionManager::new();
        sm.register(mock(1, "hello"));
        assert!(sm.selected_rect().is_none());
    }

    // ===== SelPair 测试 =====

    #[test]
    fn sel_pair_collapsed() {
        let pair = SelPair::new(
            SelPoint {
                node_id: 1,
                offset: 5,
            },
            SelPoint {
                node_id: 1,
                offset: 5,
            },
        );
        assert!(pair.is_collapsed());
    }

    #[test]
    fn sel_pair_not_collapsed() {
        let pair = SelPair::new(
            SelPoint {
                node_id: 1,
                offset: 0,
            },
            SelPoint {
                node_id: 1,
                offset: 5,
            },
        );
        assert!(!pair.is_collapsed());
    }

    #[test]
    fn sel_pair_ordered_same_node() {
        let pair = SelPair::new(
            SelPoint {
                node_id: 1,
                offset: 5,
            },
            SelPoint {
                node_id: 1,
                offset: 0,
            },
        );
        let (s, e) = pair.ordered();
        assert_eq!(s.offset, 0);
        assert_eq!(e.offset, 5);
    }

    #[test]
    fn sel_pair_ordered_cross_node() {
        let pair = SelPair::new(
            SelPoint {
                node_id: 2,
                offset: 0,
            },
            SelPoint {
                node_id: 1,
                offset: 5,
            },
        );
        let (s, e) = pair.ordered();
        assert_eq!(s.node_id, 1);
        assert_eq!(e.node_id, 2);
    }

    // ===== 辅助函数测试 =====

    #[test]
    fn compare_document_order_test() {
        assert_eq!(compare_document_order(1, 2), std::cmp::Ordering::Less);
        assert_eq!(compare_document_order(2, 1), std::cmp::Ordering::Greater);
        assert_eq!(compare_document_order(1, 1), std::cmp::Ordering::Equal);
    }

    #[test]
    fn expand_word_selection_right() {
        let n1 = MockSelectable {
            id: 1,
            text: "hello world".into(),
        };
        let selectables: Vec<&dyn Selectable> = vec![&n1];
        let pair = expand_word_selection(&selectables, 1, 2, 1, 8, true).unwrap();
        assert_eq!(pair.anchor.offset, 0);
        assert_eq!(pair.focus.offset, 11);
    }

    #[test]
    fn expand_word_selection_left() {
        let n1 = MockSelectable {
            id: 1,
            text: "hello world".into(),
        };
        let selectables: Vec<&dyn Selectable> = vec![&n1];
        let pair = expand_word_selection(&selectables, 1, 8, 1, 2, false).unwrap();
        assert_eq!(pair.anchor.offset, 0);
        assert_eq!(pair.focus.offset, 11);
    }
}
