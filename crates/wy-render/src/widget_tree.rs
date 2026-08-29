//! WidgetTree：组件树管理 + 命中测试 + 事件分发。
//!
//! 组件树在构建时递归调用 `Widget::children()` 建立，之后支持：
//! - **命中测试**：给定屏幕坐标，递归找到最深层的命中组件
//! - **事件分发**：capture 阶段（root→leaf）+ bubble 阶段（leaf→root）
//!
//! # 示例
//!
//! ```ignore
//! use wy_render::widget_tree::WidgetTree;
//! use wy_render::{Rect, Color};
//!
//! struct Button;
//! impl Widget for Button {
//!     fn draw(&self, scene: &mut Scene, cx: &mut DrawContext) {
//!         scene.fill_rect(cx.outer_rect(), Color::BLUE);
//!     }
//! }
//!
//! let mut tree = WidgetTree::new(Button);
//! tree.set_layout(0, Rect::new(10.0, 10.0, 200.0, 50.0));
//!
//! // 命中测试
//! if let Some(path) = tree.hit_test(50.0, 30.0) {
//!     // path = [0] 表示根节点被命中
//! }
//!
//! // 事件分发
//! tree.dispatch_pointer_down(50.0, 30.0);
//! ```

use crate::draw_context::DrawContext;
use crate::event::{PointerEvent, PointerType};
use crate::widget::ChildBuilder;
use crate::{Point, Rect, Size};

/// 树节点：存储 widget 和布局信息。
struct TreeNode {
    widget: Box<dyn crate::widget::Widget>,
    /// 组件外框（由布局系统填充，命中测试使用）。
    layout: Rect,
    /// 子节点索引。
    children: Vec<usize>,
    /// 父节点索引（None = 根节点）。
    #[allow(dead_code)]
    parent: Option<usize>,
}

/// 组件树：管理 widget 树结构，提供命中测试和事件分发。
pub struct WidgetTree {
    nodes: Vec<TreeNode>,
    /// 根节点索引。
    root: usize,
    /// 当前获得焦点的节点索引。
    focused: Option<usize>,
}

impl WidgetTree {
    /// 从根 widget 构建组件树。
    ///
    /// 递归调用 `Widget::children()` 建立完整的树结构。
    /// 布局信息初始为零，需要通过 `set_layout()` 填充。
    pub fn new(root: impl crate::widget::Widget) -> Self {
        let mut tree = WidgetTree {
            nodes: Vec::new(),
            root: 0,
            focused: None,
        };

        // 创建根节点
        let root_idx = tree.alloc_node(root, None);

        // 递归构建子树
        tree.build_children(root_idx);

        tree
    }

    /// 递归构建子节点。
    fn build_children(&mut self, parent_idx: usize) {
        // 调用 widget.children() 获取子节点列表
        let mut builder = ChildBuilder::new();
        self.nodes[parent_idx].widget.children(&mut builder);
        let children_widgets = builder.into_children();
        let count = children_widgets.len();

        // 先预留空间
        self.nodes[parent_idx].children.reserve(count);

        // 创建所有子节点（不持有 parent 的借用）
        for child_widget in children_widgets {
            let child_idx = self.alloc_node_boxed(child_widget, Some(parent_idx));
            self.nodes[parent_idx].children.push(child_idx);
        }

        // 递归构建孙节点
        let children_clone: Vec<usize> = self.nodes[parent_idx].children.clone();
        for child_idx in children_clone {
            self.build_children(child_idx);
        }
    }

    /// 分配一个新节点，返回索引。
    fn alloc_node(&mut self, widget: impl crate::widget::Widget, parent: Option<usize>) -> usize {
        let idx = self.nodes.len();
        self.nodes.push(TreeNode {
            widget: Box::new(widget),
            layout: Rect::zero(),
            children: Vec::new(),
            parent,
        });
        idx
    }

    /// 分配一个 Box<dyn Widget> 节点。
    fn alloc_node_boxed(
        &mut self,
        widget: Box<dyn crate::widget::Widget>,
        parent: Option<usize>,
    ) -> usize {
        let idx = self.nodes.len();
        self.nodes.push(TreeNode {
            widget,
            layout: Rect::zero(),
            children: Vec::new(),
            parent,
        });
        idx
    }

    /// 设置指定节点的布局矩形。
    ///
    /// `idx` 是节点在树中的索引（根=0，子节点按 `add_child` 顺序递增）。
    pub fn set_layout(&mut self, idx: usize, rect: Rect) {
        if let Some(node) = self.nodes.get_mut(idx) {
            node.layout = rect;
        }
    }

    /// 设置所有节点的布局矩形（批量）。
    ///
    /// 按深度优先顺序，依次设置每个节点的布局。
    pub fn set_layouts(&mut self, layouts: &[Rect]) {
        for (i, rect) in layouts.iter().enumerate() {
            if let Some(node) = self.nodes.get_mut(i) {
                node.layout = *rect;
            }
        }
    }

    /// 命中测试：给定屏幕坐标，返回从根到叶子的命中路径。
    ///
    /// 返回 `Some(path)` 其中 `path[0]` 是根节点索引，`path.last()` 是最深层命中节点。
    /// 返回 `None` 表示没有命中任何组件。
    pub fn hit_test(&mut self, x: f32, y: f32) -> Option<Vec<usize>> {
        let mut path = Vec::new();
        if self.hit_test_recursive(self.root, x, y, &mut path) {
            Some(path)
        } else {
            None
        }
    }

    /// 递归命中测试。
    fn hit_test_recursive(&mut self, idx: usize, x: f32, y: f32, path: &mut Vec<usize>) -> bool {
        let layout = self.nodes[idx].layout;

        // 命中测试使用零基 DrawContext（坐标已转换为局部坐标）
        let cx = DrawContext::new(
            Rect::new(0.0, 0.0, layout.width, layout.height),
            Point::new(0.0, 0.0),
            Size::new(layout.width, layout.height),
        );

        // 先检查自身是否命中（提取结果，释放借用）
        let hit = self.nodes[idx].widget.hit_test(x, y, &cx);
        if !hit {
            return false;
        }

        path.push(idx);

        // 逆序遍历子节点（后添加的在上面，优先命中）
        // 注意：必须先克隆子节点列表，因为 hit_test_recursive 需要 &mut self
        let children: Vec<usize> = self.nodes[idx].children.clone();
        for &child_idx in children.iter().rev() {
            let child_layout = self.nodes[child_idx].layout;
            // 子节点坐标是相对于父节点的，需要转换
            let child_x = x - (child_layout.x - layout.x);
            let child_y = y - (child_layout.y - layout.y);

            if self.hit_test_recursive(child_idx, child_x, child_y, path) {
                return true;
            }
        }

        // 没有子节点命中，当前节点就是目标
        true
    }

    /// 获取当前焦点节点索引。
    pub fn focused(&self) -> Option<usize> {
        self.focused
    }

    /// 设置焦点到指定节点（如果该节点可聚焦）。
    pub fn set_focus(&mut self, idx: usize) -> bool {
        if idx < self.nodes.len() && self.nodes[idx].widget.focusable() {
            self.focused = Some(idx);
            true
        } else {
            false
        }
    }

    /// 清除焦点。
    pub fn clear_focus(&mut self) {
        self.focused = None;
    }

    /// 分发指针按下事件（capture→bubble 两阶段）。
    ///
    /// 先从根到叶子（capture），再从叶子到根（bubble）。
    /// 如果命中路径中有可聚焦组件，自动设置焦点。
    /// 返回是否有人消费了事件。
    pub fn dispatch_pointer_down(&mut self, x: f32, y: f32) -> bool {
        if let Some(path) = self.hit_test(x, y) {
            // 点击聚焦：从叶子到根找到最深层的 focusable 组件
            for &idx in path.iter().rev() {
                if self.nodes[idx].widget.focusable() {
                    self.focused = Some(idx);
                    break;
                }
            }

            let mut event = PointerEvent::new(PointerType::Down, x, y);

            // Capture 阶段：root → leaf
            for &idx in &path {
                let cx = self.make_draw_context(idx);
                self.nodes[idx].widget.on_pointer_down(&mut event, &cx);
                if event.is_propagation_stopped() {
                    return true;
                }
            }

            // Bubble 阶段：leaf → root
            let bubble_path: Vec<usize> = path.iter().rev().copied().skip(1).collect();
            for idx in bubble_path {
                let cx = self.make_draw_context(idx);
                self.nodes[idx].widget.on_pointer_down(&mut event, &cx);
                if event.is_propagation_stopped() {
                    return true;
                }
            }

            // 点击检测：如果按下和释放都在同一组件上
            // （简化：仅在 down 时记录，up 时检查）
            true
        } else {
            false
        }
    }

    /// 分发指针释放事件。
    pub fn dispatch_pointer_up(&mut self, x: f32, y: f32) -> bool {
        if let Some(path) = self.hit_test(x, y) {
            let mut event = PointerEvent::new(PointerType::Up, x, y);

            // Capture 阶段
            for &idx in &path {
                let cx = self.make_draw_context(idx);
                self.nodes[idx].widget.on_pointer_up(&mut event, &cx);
                if event.is_propagation_stopped() {
                    return true;
                }
            }

            // Bubble 阶段
            let bubble_path: Vec<usize> = path.iter().rev().copied().skip(1).collect();
            for idx in bubble_path {
                let cx = self.make_draw_context(idx);
                self.nodes[idx].widget.on_pointer_up(&mut event, &cx);
                if event.is_propagation_stopped() {
                    return true;
                }
            }

            // 点击事件：在 bubble 阶段触发 on_click
            let target = *path.last().unwrap();
            let cx = self.make_draw_context(target);
            self.nodes[target].widget.on_click(&cx);

            true
        } else {
            false
        }
    }

    /// 分发指针移动事件。
    pub fn dispatch_pointer_move(&mut self, x: f32, y: f32) -> bool {
        if let Some(path) = self.hit_test(x, y) {
            let mut event = PointerEvent::new(PointerType::Move, x, y);

            for &idx in &path {
                let cx = self.make_draw_context(idx);
                self.nodes[idx].widget.on_pointer_move(&mut event, &cx);
                if event.is_propagation_stopped() {
                    return true;
                }
            }

            let bubble_path: Vec<usize> = path.iter().rev().copied().skip(1).collect();
            for idx in bubble_path {
                let cx = self.make_draw_context(idx);
                self.nodes[idx].widget.on_pointer_move(&mut event, &cx);
                if event.is_propagation_stopped() {
                    return true;
                }
            }

            true
        } else {
            false
        }
    }

    /// 为指定节点构造 DrawContext。
    fn make_draw_context(&self, idx: usize) -> DrawContext {
        let layout = self.nodes[idx].layout;
        DrawContext::new(
            layout,
            Point::new(layout.x, layout.y),
            Size::new(layout.width, layout.height),
        )
    }

    /// 节点数量。
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// 树是否为空。
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// 获取节点的布局矩形。
    pub fn layout(&self, idx: usize) -> Rect {
        self.nodes.get(idx).map_or(Rect::zero(), |n| n.layout)
    }

    /// 获取根节点索引。
    pub fn root(&self) -> usize {
        self.root
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widget::Widget;
    use crate::{Color, Scene};

    struct Button {
        label: String,
        clicked: bool,
    }

    impl Button {
        fn new(label: &str) -> Self {
            Self {
                label: label.to_string(),
                clicked: false,
            }
        }
    }

    impl Widget for Button {
        fn draw(&self, scene: &mut Scene, cx: &mut DrawContext) {
            scene.fill_rect(cx.outer_rect(), Color::BLUE);
            scene.draw_text(
                Point::new(cx.inner_x(), cx.inner_y()),
                &self.label,
                14.0,
                Color::WHITE,
            );
        }

        fn on_click(&mut self, _cx: &DrawContext) {
            self.clicked = true;
        }
    }

    struct Panel;

    impl Widget for Panel {
        fn children(&self, cx: &mut ChildBuilder) {
            cx.add_child(Button::new("OK"));
            cx.add_child(Button::new("Cancel"));
        }

        fn draw(&self, scene: &mut Scene, cx: &mut DrawContext) {
            scene.fill_rect(cx.outer_rect(), Color::GRAY);
        }
    }

    fn build_test_tree() -> (WidgetTree, usize, usize) {
        let mut tree = WidgetTree::new(Panel);
        // 根节点(Panel): 索引 0，外框 0,0,400,300
        tree.set_layout(0, Rect::new(0.0, 0.0, 400.0, 300.0));
        // OK 按钮: 索引 1，外框 10,10,100,40
        tree.set_layout(1, Rect::new(10.0, 10.0, 100.0, 40.0));
        // Cancel 按钮: 索引 2，外框 120,10,100,40
        tree.set_layout(2, Rect::new(120.0, 10.0, 100.0, 40.0));
        (tree, 1, 2) // return ok_idx, cancel_idx
    }

    #[test]
    fn tree_builds_children() {
        let tree = WidgetTree::new(Panel);
        assert_eq!(tree.len(), 3); // Panel + 2 buttons
        assert_eq!(tree.root(), 0);
    }

    #[test]
    fn hit_test_misses_empty_tree() {
        let mut tree = WidgetTree::new(Panel);
        assert!(tree.hit_test(500.0, 500.0).is_none());
    }

    #[test]
    fn hit_test_finds_root() {
        let mut tree = WidgetTree::new(Panel);
        tree.set_layout(0, Rect::new(0.0, 0.0, 400.0, 300.0));
        // 在根范围内，但不在任何子节点上
        let path = tree.hit_test(200.0, 200.0);
        assert!(path.is_some());
        assert_eq!(path.unwrap(), vec![0]);
    }

    #[test]
    fn hit_test_finds_child() {
        let (mut tree, ok_idx, _) = build_test_tree();
        let path = tree.hit_test(50.0, 30.0);
        assert!(path.is_some());
        let path = path.unwrap();
        assert_eq!(path[0], 0); // 根
        assert_eq!(path[1], ok_idx); // OK 按钮
    }

    #[test]
    fn hit_test_finds_correct_sibling() {
        let (mut tree, _, cancel_idx) = build_test_tree();
        let path = tree.hit_test(150.0, 30.0);
        assert!(path.is_some(), "hit_test should find Cancel button");
        let path = path.unwrap();
        assert_eq!(path[1], cancel_idx);
    }

    #[test]
    fn hit_test_misses_outside() {
        let mut tree = WidgetTree::new(Panel);
        assert!(tree.hit_test(500.0, 500.0).is_none());
    }

    #[test]
    fn dispatch_click_triggers_on_click() {
        let mut tree = WidgetTree::new(Panel);
        tree.set_layout(0, Rect::new(0.0, 0.0, 400.0, 300.0));
        tree.set_layout(1, Rect::new(10.0, 10.0, 100.0, 40.0));
        tree.set_layout(2, Rect::new(120.0, 10.0, 100.0, 40.0));

        // 点击 OK 按钮
        tree.dispatch_pointer_down(50.0, 30.0);
        tree.dispatch_pointer_up(50.0, 30.0);

        // 验证 OK 按钮被点击
        let ok = tree.nodes[1].widget.downcast_ref::<Button>().unwrap();
        assert!(ok.clicked);

        // Cancel 按钮未被点击
        let cancel = tree.nodes[2].widget.downcast_ref::<Button>().unwrap();
        assert!(!cancel.clicked);
    }

    #[test]
    fn dispatch_miss_returns_false() {
        let mut tree = WidgetTree::new(Panel);
        tree.set_layout(0, Rect::new(0.0, 0.0, 400.0, 300.0));
        assert!(!tree.dispatch_pointer_down(500.0, 500.0));
    }

    #[test]
    fn layout_setters_work() {
        let mut tree = WidgetTree::new(Panel);
        tree.set_layout(0, Rect::new(0.0, 0.0, 100.0, 100.0));
        assert_eq!(tree.layout(0), Rect::new(0.0, 0.0, 100.0, 100.0));

        tree.set_layouts(&[
            Rect::new(0.0, 0.0, 200.0, 200.0),
            Rect::new(10.0, 10.0, 50.0, 50.0),
            Rect::new(70.0, 10.0, 50.0, 50.0),
        ]);
        assert_eq!(tree.layout(0), Rect::new(0.0, 0.0, 200.0, 200.0));
        assert_eq!(tree.layout(1), Rect::new(10.0, 10.0, 50.0, 50.0));
    }
}
