//! MVE 集成：将 `wy-mve` 的 Node 树连接到渲染管线。
//!
//! # 构建一次模型（复刻 Kotlin `Renderer.signal.collect { draw() }`）
//!
//! - `MveApp::new(callback)` 用 [`render_root`] **一次性**构建树结构：
//!   callback 只执行一次，顶层快照 Slot 固化，动态列表封成 memo 区域。
//! - 每帧 `draw()` 读取 `Root.target`（memo）得到当前扁平节点列表，绘制期
//!   按容器的 `Layout` 角色实时计算偏移——**不再每帧重建 Node 树**。
//! - draw / 事件处理遍历节点时读取信号（被 `RedrawTracker` 自动追踪），
//!   信号变化只触发受影响区域重算 → 局部重绘，与 Kotlin 行为一致。

use std::cell::UnsafeCell;

use wy_mve::{children_nodes, has_handler, layout_offsets, render_root, Node, NodeContext};
use wy_render::{Point, Scene};

/// 顶层节点遍历（origin 为节点坐标系原点在父坐标系中的位置）。
fn draw_nodes(nodes: &[Node], scene: &mut Scene) {
    for node in nodes {
        draw_node(node, (0.0, 0.0), scene);
    }
}

/// 递归绘制：自我绘制在最底层，子节点按文档序叠在其上（对应 Kotlin Node.draw）。
fn draw_node(node: &Node, origin: (f32, f32), scene: &mut Scene) {
    if node.hidden {
        return;
    }
    let pos = (origin.0 + node.x, origin.1 + node.y);
    if pos != (0.0, 0.0) {
        scene.push_transform(Point::new(pos.0, pos.1));
    }
    if !node.skip_draw {
        node.run_draw(scene);
    }
    let children = children_nodes(node);
    match node.layout {
        Some(layout) => {
            let offsets = layout_offsets(layout, &children);
            for (child, ofs) in children.iter().zip(offsets.iter()) {
                draw_node(child, *ofs, scene);
            }
        }
        None => {
            for child in children.iter() {
                draw_node(child, (0.0, 0.0), scene);
            }
        }
    }
    if pos != (0.0, 0.0) {
        scene.pop_transform();
    }
}

/// 命中测试：子节点优先（后绘制的在上，倒序），对应 Kotlin `Node.hitTest`。
fn hit_test_nodes(nodes: &[Node], x: f32, y: f32) -> bool {
    nodes
        .iter()
        .any(|node| hit_test_node(node, (0.0, 0.0), x, y).is_some())
}

/// 返回命中节点的 ID 链（子→根顺序，对应 Kotlin hitTest 的 [NodeWithPosition] 链）。
fn hit_test_node(node: &Node, origin: (f32, f32), x: f32, y: f32) -> Option<Vec<Node>> {
    if node.hidden {
        return None;
    }
    let pos = (origin.0 + node.x, origin.1 + node.y);
    let children = children_nodes(node);
    let offsets = match node.layout {
        Some(layout) => layout_offsets(layout, &children),
        None => vec![(0.0, 0.0); children.len()],
    };
    // 子节点优先：倒序（后绘制的在上面）
    for (child, ofs) in children.iter().zip(offsets.iter()).rev() {
        let child_origin = (pos.0 + ofs.0, pos.1 + ofs.1);
        if let Some(mut chain) = hit_test_node(child, child_origin, x, y) {
            chain.push(node.clone());
            return Some(chain);
        }
    }
    // 自身：必须有 handler 且命中才算
    if has_handler(node) && node.run_hit_test(x - pos.0, y - pos.1) {
        return Some(vec![node.clone()]);
    }
    None
}

/// 沿命中链冒泡分发点击（子→根，可 stopPropagation，对应 Kotlin 冒泡阶段）。
fn dispatch_click_nodes(nodes: &[Node], x: f32, y: f32, event: &mut wy_mve::PointerEvent) {
    if let Some(chain) = nodes
        .iter()
        .find_map(|node| hit_test_node(node, (0.0, 0.0), x, y))
    {
        for node in chain {
            node.run_on_click(event);
            if event.stopped {
                return;
            }
        }
    }
}

/// MVE 应用：连接 MVE Node 树与渲染引擎。
///
/// 树结构只构建一次；每帧 `draw()` 读取 `Root.target` memo 得到当前节点列表，
/// 信号变化自动触发重绘（`RedrawTracker`）。复刻 Kotlin Renderer 的管线模型。
pub struct MveApp {
    root: wy_mve::Root,
    cursor: UnsafeCell<(f32, f32)>,
}

impl MveApp {
    /// 创建 MVE 应用：`callback` 构建根订阅树（只执行一次）。
    ///
    /// callback 内可用 `cx.child(...)` 添加顶层节点、`cx.render_for_each(...)`
    /// 添加动态列表区域；其中读取的信号会被 memo 自动追踪。
    pub fn new(callback: impl Fn(&mut NodeContext) + 'static) -> Self {
        let root = render_root(callback);
        Self {
            root,
            cursor: UnsafeCell::new((0.0, 0.0)),
        }
    }
}

impl crate::runner::WyApp for MveApp {
    fn draw(&self, scene: &mut Scene, _width: f32, _height: f32) {
        // 等价于 Kotlin: signal.collect { didDraw().draw(canvas, 0f, 0f) }
        // Root.target 读取 → RedrawTracker 追踪 → 信号变化触发重绘
        let nodes = self.root.nodes();
        draw_nodes(&nodes, scene);
    }

    fn widget_tree(&mut self) -> Option<&mut wy_render::widget_tree::WidgetTree> {
        // 不使用 WidgetTree：直接遍历 Node 树绘制和处理事件
        None
    }

    fn on_resize(&mut self, _width: f32, _height: f32) {}

    fn handle_event(&mut self, event: &winit::event::WindowEvent) -> bool {
        use winit::event::{ElementState, MouseButton, WindowEvent};

        // SAFETY: handle_event 在 runner 的事件循环中调用，
        // runner 拥有 self.app 的独占访问权。
        let cursor = unsafe { &mut *self.cursor.get() };

        match event {
            WindowEvent::CursorMoved { position, .. } => {
                *cursor = (position.x as f32, position.y as f32);
                false
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } => {
                let (x, y) = *cursor;
                let nodes = self.root.nodes();
                if hit_test_nodes(&nodes, x, y) {
                    let mut mve_event = wy_mve::PointerEvent::new(x, y);
                    dispatch_click_nodes(&nodes, x, y, &mut mve_event);
                    return mve_event.stopped;
                }
                false
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner::WyApp;
    use std::rc::Rc;
    use wy_mve::{button, column, node_size, row, text, PointerEvent};
    use wy_signal::{GetValue, SetValue, Signal};

    /// 构建 counter 结构：column > (text + row(button-, button+))
    fn counter_app(count: Signal<i32>) -> MveApp {
        MveApp::new(move |cx| {
            let column_count = count.clone();
            cx.child(column(move |cx| {
                let c = column_count.clone();
                cx.child(text(move || format!("Count: {}", c.get())));

                let row_count = column_count.clone();
                cx.child(row(move |cx| {
                    cx.child(button(move || "−".into(), {
                        let s = row_count.clone();
                        move || s.set(s.get() - 1)
                    }));
                    cx.child(button(move || "+".into(), {
                        let s = row_count.clone();
                        move || s.set(s.get() + 1)
                    }));
                }));
            }));
        })
    }

    #[test]
    fn draw_builds_tree_once() {
        let count = Signal::new(0);
        let app = counter_app(count.clone());

        let mut scene = Scene::new();
        app.draw(&mut scene, 100.0, 100.0);

        // 信号变化后再次 draw：树只用 memo 重算，不重建
        count.set(10);
        app.draw(&mut scene, 100.0, 100.0);
    }

    #[test]
    fn node_equality_by_identity() {
        // 同一 Node 克隆相等；不同实例不等
        let n = Node::default();
        assert_eq!(n, n.clone());
        assert_ne!(n, Node::default());
    }

    #[test]
    fn layout_container_sizes_children() {
        let col = column(|cx| {
            cx.child(Node {
                width: 50.0,
                height: 20.0,
                hit_test_fn: Rc::new(|_, _| false),
                ..Node::default()
            });
            cx.child(Node {
                width: 50.0,
                height: 30.0,
                hit_test_fn: Rc::new(|_, _| false),
                ..Node::default()
            });
        });
        let (w, h) = node_size(&col);
        assert_eq!(w, 50.0);
        assert_eq!(h, 58.0); // 20 + 8 gap + 30

        let layout = col.layout.unwrap();
        let offsets = layout_offsets(layout, &children_nodes(&col));
        assert_eq!(offsets[0], (0.0, 0.0));
        assert_eq!(offsets[1], (0.0, 28.0));
    }

    #[test]
    fn counter_click_dispatches_to_button() {
        let count = Signal::new(0);
        let app = counter_app(count.clone());

        let mut scene = Scene::new();
        app.draw(&mut scene, 100.0, 100.0);

        // column > text + row(button-, button+)
        // 坐标由布局实时计算（文本高度来自 Parley 测量），不硬编码
        let nodes = app.root.nodes();
        assert_eq!(nodes.len(), 1); // column

        let col = &nodes[0];
        let children = children_nodes(col);
        assert_eq!(children.len(), 2); // text + row
        let (_, text_h) = node_size(&children[0]);
        let row_y = text_h + 8.0; // column gap=8

        let row = &children[1];
        let row_children = children_nodes(row);
        let (btn_w, _) = node_size(&row_children[0]);
        let plus_x = btn_w + 8.0 + btn_w / 2.0; // button- 宽 + gap + button+ 半宽

        let click = (plus_x, row_y + 16.0); // 按钮垂直中心
        let chain = hit_test_node(col, (0.0, 0.0), click.0, click.1);
        assert!(chain.is_some());
        let chain = chain.unwrap();
        assert_eq!(chain.len(), 3); // +, row, column
        assert!(chain[0].on_click_fn.is_some(), "deepest is the + button");
        assert_eq!(chain[1].layout, Some(wy_mve::Layout::Row { gap: 8.0 }));

        // 分发点击 → + 按钮 handler 执行并 stop
        let mut event = PointerEvent::new(click.0, click.1);
        dispatch_click_nodes(&nodes, click.0, click.1, &mut event);
        assert!(event.stopped);
        assert_eq!(count.get(), 1);
    }

    #[test]
    fn hit_test_misses_outside_column() {
        let count = Signal::new(0);
        let app = counter_app(count.clone());
        let _ = app;

        let mut scene = Scene::new();
        app.draw(&mut scene, 100.0, 100.0);

        let nodes = app.root.nodes();
        assert!(!hit_test_nodes(&nodes, 1000.0, 1000.0));
    }

    #[test]
    fn text_reads_signal_at_draw() {
        let count = Signal::new(0);
        let count2 = count.clone();
        let text_node = text(move || format!("Count: {}", count2.get()));
        assert!(text_node.on_click_fn.is_none());

        // draw 读信号在 tracker.draw 外不触发 memo；直接验证信号可读
        count.set(5);
        assert_eq!(count.get(), 5);
    }

    #[test]
    fn render_for_each_builds_list_once() {
        use wy_mve::NodeContext;
        let items = Signal::new(vec![1, 2, 3]);
        let items2 = items.clone();
        let app = MveApp::new(move |cx: &mut NodeContext| {
            let list = items2.clone();
            cx.render_for_each(
                move |emit| {
                    let snapshot = list.get();
                    for v in snapshot {
                        emit(v, v);
                    }
                },
                |_k, value, atomic_cx| {
                    let v = *value.borrow();
                    let _ = v;
                    atomic_cx.child(text(move || format!("item {}", v)));
                },
            );
        });

        let mut scene = Scene::new();
        app.draw(&mut scene, 100.0, 100.0);
        let nodes = app.root.nodes();
        assert_eq!(nodes.len(), 3);

        // 增删列表 → 顶层节点数变化
        items.set(vec![1, 2]);
        app.draw(&mut scene, 100.0, 100.0);
        let nodes = app.root.nodes();
        assert_eq!(nodes.len(), 2);
    }
}
