//! MVE 集成：将 `wy-mve` 的 Node 树连接到渲染管线。
//!
//! 流程：render_root → effect 监听信号 → MveWidget → WidgetTree → Scene → Vello
//!
//! 核心模式（复刻 Kotlin MVE）：
//! - `MveApp::new(callback)` 创建 effect 监听 callback 中读取的信号
//! - 信号变化时 effect 自动重建 MVE Node 树，标记 WidgetTree 为脏
//! - `widget_tree()` 返回缓存的 WidgetTree，runner 用于渲染和事件分发
//! - 每帧不重建 Widget 树，只在信号变化时重建

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use wy_layout::{
    DirectionJustify, FlexChildConvert, FlexObject, Layout, LayoutInsideObject,
};
use wy_mve::{ChildrenCache, Node, NodeContext, PointerEvent as MvePointerEvent};
use wy_render::widget::ChildBuilder;
use wy_render::widget_tree::WidgetTree;
use wy_render::{DrawContext, Rect, Scene, Widget};

/// 将 MVE Node 树转为 Widget 树的适配器。
///
/// 每个 MveWidget 包装一组 MVE Node（从 ChildrenCache 获取）。
/// `children()` 在 `WidgetTree::new()` 时调用一次（不是每帧），
/// 递归展开所有 arg_children 形成完整的 Widget 树。
struct MveWidget {
    nodes: Vec<Node>,
}

impl Widget for MveWidget {
    fn draw(&self, scene: &mut Scene, cx: &mut DrawContext) {
        let pos = cx.inner_origin();
        if pos.x != 0.0 || pos.y != 0.0 {
            scene.push_transform(pos);
        }
        for node in &self.nodes {
            if !node.skip_draw {
                node.run_draw(scene);
            }
        }
        if pos.x != 0.0 || pos.y != 0.0 {
            scene.pop_transform();
        }
    }

    fn children(&self, cx: &mut ChildBuilder) {
        // 在 WidgetTree::new() 时调用一次（不是每帧）
        // 递归展开所有 arg_children 形成 Widget 树
        for node in &self.nodes {
            let mut child_cx = NodeContext::new(0);
            node.run_arg_children(&mut child_cx);
            let child_nodes = child_cx.into_nodes();
            if !child_nodes.is_empty() {
                cx.add_child(MveWidget { nodes: child_nodes });
            }
        }
    }

    fn hit_test(&self, x: f32, y: f32, cx: &DrawContext) -> bool {
        let rect = cx.outer_rect();
        x >= rect.x && x < rect.x + rect.width && y >= rect.y && y < rect.y + rect.height
    }

    fn on_pointer_down(&mut self, event: &mut wy_render::event::PointerEvent, _cx: &DrawContext) {
        let mut mve_event = MvePointerEvent::new(event.x, event.y);
        for node in &self.nodes {
            node.run_on_down(&mut mve_event);
            if mve_event.stopped {
                event.stop_propagation();
                return;
            }
        }
    }

    fn on_pointer_up(&mut self, event: &mut wy_render::event::PointerEvent, _cx: &DrawContext) {
        let mut mve_event = MvePointerEvent::new(event.x, event.y);
        for node in &self.nodes {
            node.run_on_up(&mut mve_event);
            if mve_event.stopped {
                event.stop_propagation();
                return;
            }
        }
    }

    fn on_click(&mut self, _cx: &DrawContext) {
        let mut event = MvePointerEvent::new(0.0, 0.0);
        for node in &self.nodes {
            node.run_on_click(&mut event);
            if event.stopped {
                return;
            }
        }
    }

    fn focusable(&self) -> bool {
        self.nodes.iter().any(|n| n.is_focusable())
    }
}

/// Memo 化的布局计算缓存。
struct LayoutMemo {
    last_width: f32,
    last_height: f32,
    last_tree_shape: u64,
}

impl LayoutMemo {
    fn new() -> Self {
        Self {
            last_width: 0.0,
            last_height: 0.0,
            last_tree_shape: 0,
        }
    }

    fn needs_recompute(&self, width: f32, height: f32, tree_shape: u64) -> bool {
        (self.last_width - width).abs() > f32::EPSILON
            || (self.last_height - height).abs() > f32::EPSILON
            || self.last_tree_shape != tree_shape
    }

    fn record(&mut self, width: f32, height: f32, tree_shape: u64) {
        self.last_width = width;
        self.last_height = height;
        self.last_tree_shape = tree_shape;
    }
}

/// 列表行高度（demo 固定值）。
const ROW_HEIGHT: f32 = 50.0;
/// 按钮宽度（demo 固定值）。
const BUTTON_WIDTH: f32 = 80.0;

/// 参与 `FlexLayout` 布局的一个子节点：索引 + 主轴外部尺寸 + grow 权重。
struct FlexChild {
    idx: usize,
    size: f32,
    grow: f32,
}

impl FlexChild {
    fn new(idx: usize, size: f32) -> Self {
        Self {
            idx,
            size,
            grow: 0.0,
        }
    }
}

impl FlexChildConvert<FlexChild> for FlexRow {
    fn index(&self, c: &FlexChild) -> usize {
        c.idx
    }
    fn grow(&self, c: &FlexChild) -> f32 {
        c.grow
    }
    fn outer_size(&self, c: &FlexChild) -> f32 {
        c.size
    }
    fn ignore(&self, _c: &FlexChild) -> bool {
        false
    }
}

impl FlexObject<FlexChild> for FlexRow {
    fn gap(&self) -> f32 {
        self.gap
    }
    fn direction_justify(&self) -> DirectionJustify {
        self.direction
    }
}

/// 一维 Flex 布局容器配置：主轴方向（start/center/end/...）+ 间距 + 可用空间。
struct FlexRow {
    direction: DirectionJustify,
    gap: f32,
    inner_size: f32,
}

impl FlexRow {
    /// 用 `wy-layout` 的 `FlexLayout` 计算子节点位置与尺寸。
    fn build(&self, children: &[FlexChild]) -> wy_layout::FlexLayout {
        let inside = LayoutInsideObject::new(children, self.inner_size);
        self.to_layout(&inside)
    }
}

/// MVE 应用：连接 MVE Node 树与渲染引擎。
///
/// 核心模式（复刻 Kotlin MVE）：
/// - `new(callback)` 创建 effect 监听 callback 中读取的信号
/// - 信号变化时 effect 自动重建 MVE Node 树，标记 WidgetTree 为脏
/// - `widget_tree()` 返回缓存的 WidgetTree，runner 用于渲染和事件分发
/// - 每帧不重建 Widget 树，只在信号变化时重建
pub struct MveApp {
    root_builder: Rc<dyn Fn(&mut NodeContext)>,
    cached_nodes: Rc<RefCell<Vec<Node>>>,
    cached_tree: Option<WidgetTree>,
    tree_dirty: Rc<Cell<bool>>,
    layout_memo: LayoutMemo,
    request_redraw: Rc<dyn Fn()>,
}

impl MveApp {
    /// 从 ChildrenCache 创建（推荐方式）。
    pub fn from_cache(cache: ChildrenCache) -> Self {
        let cache_ref = cache.clone();
        Self {
            root_builder: Rc::new(move |cx| {
                for node in cache_ref.get() {
                    cx.add_node(node);
                }
            }),
            cached_nodes: Rc::new(RefCell::new(Vec::new())),
            cached_tree: None,
            tree_dirty: Rc::new(Cell::new(true)),
            layout_memo: LayoutMemo::new(),
            request_redraw: Rc::new(|| {}),
        }
    }

    /// 创建 MVE 应用（effect 模式）。
    ///
    /// `callback` 中读取的信号会被 effect 自动追踪。
    /// 信号变化时 effect 重建 MVE Node 树，标记 WidgetTree 为脏。
    /// 每帧 `widget_tree()` 只读取缓存的 WidgetTree，不重建。
    pub fn new(callback: impl Fn(&mut NodeContext) + 'static) -> Self {
        let cache = wy_mve::render_root(callback);
        Self::from_cache(cache)
    }

    /// 计算树结构的哈希值（用于布局缓存判断）。
    fn tree_shape_hash(nodes: &[Node]) -> u64 {
        let mut hash = 0u64;
        for node in nodes {
            hash = hash
                .wrapping_mul(31)
                .wrapping_add(if node.hidden { 1 } else { 0 });
            hash = hash
                .wrapping_mul(31)
                .wrapping_add(if node.skip_draw { 2 } else { 0 });
            hash = hash
                .wrapping_mul(31)
                .wrapping_add(if node.focusable { 4 } else { 0 });
        }
        hash
    }

    /// 自定义布局：使用 `wy-layout` 的 `FlexLayout` 进行布局。
    ///
    /// - root 子节点（列表行）沿垂直主轴用 `FlexLayout` 排列；
    /// - 每个 list item 内的按钮沿水平主轴用 `FlexLayout` 排列。
    ///
    /// 布局是一维的：`FlexLayout` 只给出主轴上的位置与尺寸标量，
    /// 交叉轴尺寸在这里取父节点全宽（工具栏/行内按钮交叉轴为行高）。
    fn layout_tree(tree: &mut WidgetTree, width: f32, height: f32) {
        tree.set_layout(0, Rect::new(0.0, 0.0, width, height));

        // root：垂直排列子节点，gap=10
        let root_children = tree.children_of(0);
        let rows: Vec<FlexChild> = root_children
            .iter()
            .enumerate()
            .map(|(i, _)| FlexChild::new(i, ROW_HEIGHT))
            .collect();
        let flex_arg = FlexRow {
            direction: DirectionJustify::Grow,
            gap: 10.0,
            inner_size: height,
        };
        let vlayout = flex_arg.build(&rows);

        for (i, &child_idx) in root_children.iter().enumerate() {
            // 行在垂直主轴上的位置 y 由 vlayout 给出，行高取主轴尺寸（=ROW_HEIGHT），
            // 交叉轴（水平）填满父节点全宽。
            let y = vlayout.child_position(i).unwrap_or(0.0);
            let h = vlayout.child_size(i).unwrap_or(0.0);
            tree.set_layout(child_idx, Rect::new(0.0, y, width, h));

            // list item 内：水平排列按钮，gap=5
            let button_children = tree.children_of(child_idx);
            let buttons: Vec<FlexChild> = button_children
                .iter()
                .enumerate()
                .map(|(i, _)| FlexChild::new(i, BUTTON_WIDTH))
                .collect();
            let btn_arg = FlexRow {
                direction: DirectionJustify::Grow,
                gap: 5.0,
                inner_size: width,
            };
            let hlayout = btn_arg.build(&buttons);

            for (bi, &btn_idx) in button_children.iter().enumerate() {
                let x = hlayout.child_position(bi).unwrap_or(0.0);
                let w = hlayout.child_size(bi).unwrap_or(0.0);
                tree.set_layout(btn_idx, Rect::new(x, 0.0, w, h));
            }
        }
    }

    /// 从缓存的 MVE Node 重建 WidgetTree。
    fn rebuild_tree(&mut self) {
        let nodes = self.cached_nodes.borrow().clone();
        let root = MveWidget { nodes };
        self.cached_tree = Some(WidgetTree::new(root));
    }
}

impl crate::runner::WyApp for MveApp {
    fn setup(&mut self, request_redraw: Rc<dyn Fn()>) {
        // 保存 request_redraw
        self.request_redraw = request_redraw.clone();

        // 创建 effect 监听 root_builder 中读取的信号
        // 信号变化时自动重建 MVE Node 树，标记 WidgetTree 为脏
        let root_builder = self.root_builder.clone();
        let cached_nodes = self.cached_nodes.clone();
        let tree_dirty = self.tree_dirty.clone();
        let request_redraw_ref = request_redraw.clone();

        wy_signal::create_effect(move || {
            let mut cx = NodeContext::new(0);
            root_builder(&mut cx);
            let new_nodes = cx.into_nodes();
            *cached_nodes.borrow_mut() = new_nodes;
            // 标记 WidgetTree 为脏（需要重建）
            tree_dirty.set(true);
            // 触发重绘
            request_redraw_ref();
        });
    }

    fn draw(&mut self, _scene: &mut Scene, _width: f32, _height: f32) {
        // 使用 widget_tree() 而不是 draw()
        // 此方法不应被调用（runner 优先使用 widget_tree()）
    }

    fn widget_tree(&mut self) -> Option<&mut WidgetTree> {
        // 如果 WidgetTree 为脏（信号变化导致），重建
        if self.tree_dirty.get() {
            self.rebuild_tree();
            self.tree_dirty.set(false);
        }

        // 计算布局（只在窗口尺寸或树结构变化时重新计算）
        let tree = self.cached_tree.as_mut()?;
        let tree_shape = Self::tree_shape_hash(&self.cached_nodes.borrow());
        let width = self.layout_memo.last_width;
        let height = self.layout_memo.last_height;

        if width > 0.0
            && height > 0.0
            && self.layout_memo.needs_recompute(width, height, tree_shape)
        {
            Self::layout_tree(tree, width, height);
            self.layout_memo.record(width, height, tree_shape);
        }

        Some(tree)
    }

    fn on_resize(&mut self, width: f32, height: f32) {
        // 窗口尺寸变化时，强制重新计算布局
        self.layout_memo.last_width = 0.0;
        self.layout_memo.last_height = 0.0;

        // 如果已有缓存的 tree，立即计算布局
        if let Some(tree) = self.cached_tree.as_mut() {
            let tree_shape = Self::tree_shape_hash(&self.cached_nodes.borrow());
            if self.layout_memo.needs_recompute(width, height, tree_shape) {
                Self::layout_tree(tree, width, height);
                self.layout_memo.record(width, height, tree_shape);
            }
        }
    }

    fn handle_event(&mut self, _event: &winit::event::WindowEvent) -> bool {
        // 事件由 runner 通过 widget_tree() 自动分发
        false
    }
}

/// 启动 MVE 应用。
pub fn run_mve(builder: impl Fn() -> ChildrenCache + 'static) {
    let cache = builder();
    let app = MveApp::from_cache(cache);
    let _ = crate::runner::run(app);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `FlexRow::build` 以 `Grow` 模式沿主轴排列子节点（含 gap，去除末尾多余 gap）。
    #[test]
    fn flex_row_grow_stacks_with_gap() {
        let rows = vec![
            FlexChild::new(0, ROW_HEIGHT),
            FlexChild::new(1, ROW_HEIGHT),
        ];
        let arg = FlexRow {
            direction: DirectionJustify::Grow,
            gap: 10.0,
            inner_size: 200.0,
        };
        let layout = arg.build(&rows);

        assert_eq!(layout.child_position(0).unwrap(), 0.0);
        assert_eq!(layout.child_size(0).unwrap(), ROW_HEIGHT);
        // 第二个子节点偏移 = 50 + 10
        assert_eq!(layout.child_position(1).unwrap(), 60.0);
        assert_eq!(layout.child_size(1).unwrap(), ROW_HEIGHT);
        // 总长 = 50*2 + 10 = 110（去掉末尾 gap）
        assert_eq!(layout.size_from_children().unwrap(), 110.0);
    }

    /// `DirectionJustify::Center` 模式：子节点在可用空间内居中。
    #[test]
    fn flex_row_center_justifies() {
        let buttons = vec![
            FlexChild::new(0, BUTTON_WIDTH),
            FlexChild::new(1, BUTTON_WIDTH),
        ];
        let arg = FlexRow {
            direction: DirectionJustify::Center,
            gap: 0.0,
            inner_size: 200.0,
        };
        let layout = arg.build(&buttons);
        // all_remaining = 200 - 160 = 40，起始 = 20
        assert_eq!(layout.child_position(0).unwrap(), 20.0);
        assert_eq!(layout.child_position(1).unwrap(), 100.0);
    }
}
