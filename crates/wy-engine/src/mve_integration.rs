//! MVE 集成：将 `wy-mve` 的 Node 树连接到渲染管线。
//!
//! 流程：render_root → ChildrenCache → expand_tree → draw (用节点自身的 x/y/width/height)
//!
//! 核心模式（复刻 Kotlin Renderer.signal.collect { draw() }）：
//! - `MveApp::draw()` 中调用 `root_builder`，`RedrawTracker` 自动追踪信号
//! - 信号变化时 `RedrawTracker` 自动触发重绘
//! - `draw()` 重新调用 `root_builder` → Node 树重建 → 展开树重建
//! - 零 `create_effect`，框架层全自动

use std::cell::UnsafeCell;
use std::rc::Rc;

use wy_mve::{ChildrenCache, Node, NodeContext, PointerEvent as MvePointerEvent};
use wy_render::{Point, Scene};

/// 展开后的节点：保留节点自身的 x/y/width/height + 递归子节点。
///
/// 与 WidgetTree 的区别：WidgetTree 忽略节点位置，重新计算布局。
/// ExpandedNode 保留组件设置的位置，draw 时直接使用。
struct ExpandedNode {
    node: Node,
    children: Vec<ExpandedNode>,
}

/// 从节点列表展开为树，保留 arg_children 设置的 x/y/width/height。
fn expand_tree(nodes: &[Node]) -> Vec<ExpandedNode> {
    nodes
        .iter()
        .map(|node| {
            let mut child_cx = NodeContext::new(0);
            node.run_arg_children(&mut child_cx);
            let child_nodes = child_cx.into_nodes();
            ExpandedNode {
                node: node.clone(),
                children: expand_tree(&child_nodes),
            }
        })
        .collect()
}

/// 递归绘制展开树。
///
/// 每个节点的 draw_fn 在节点自身坐标系中执行（0,0 是节点左上角）。
/// 通过 scene.push_transform(x, y) 将子节点坐标系映射到父节点坐标系。
fn draw_tree(nodes: &[ExpandedNode], scene: &mut Scene) {
    let mut i = 0;
    while i < nodes.len() {
        let node = &nodes[i].node;
        if !node.hidden {
            if node.x != 0.0 || node.y != 0.0 {
                scene.push_transform(Point::new(node.x, node.y));
            }
            if !node.skip_draw {
                node.run_draw(scene);
            }
            draw_tree(&nodes[i].children, scene);
            if node.x != 0.0 || node.y != 0.0 {
                scene.pop_transform();
            }
        }
        i += 1;
    }
}

/// 命中测试：从上到下（后绘制的在上面），子节点优先于父节点。
fn hit_test_tree(nodes: &[ExpandedNode], x: f32, y: f32) -> bool {
    let mut i = nodes.len();
    while i > 0 {
        i -= 1;
        let child = &nodes[i];
        let nx = x - child.node.x;
        let ny = y - child.node.y;
        // 先递归子节点（子节点优先于父节点）
        if hit_test_tree(&child.children, nx, ny) {
            return true;
        }
        // 再检查当前节点（必须有 handler 才算命中）
        if (child.node.on_click_fn.is_some() || child.node.on_down_fn.is_some())
            && child.node.run_hit_test(nx, ny)
        {
            return true;
        }
        // 无 handler 的容器节点：继续检查兄弟节点
    }
    false
}

/// 递归分发点击事件（子节点优先于父节点）。
fn dispatch_click_tree(nodes: &[ExpandedNode], x: f32, y: f32, event: &mut MvePointerEvent) {
    let mut i = nodes.len();
    while i > 0 {
        i -= 1;
        let child = &nodes[i];
        let nx = x - child.node.x;
        let ny = y - child.node.y;
        if child.node.run_hit_test(nx, ny) {
            // 先分发到子节点（子节点优先）
            dispatch_click_tree(&child.children, nx, ny, event);
            if event.stopped {
                return;
            }
            // 再分发到当前节点
            child.node.run_on_click(event);
            if event.stopped {
                return;
            }
        }
    }
}

/// 递归分发 pointer down 事件。
#[allow(dead_code)]
fn dispatch_down_tree(nodes: &[ExpandedNode], x: f32, y: f32, event: &mut MvePointerEvent) {
    let mut i = nodes.len();
    while i > 0 {
        i -= 1;
        let child = &nodes[i];
        let nx = x - child.node.x;
        let ny = y - child.node.y;
        if child.node.run_hit_test(nx, ny) {
            dispatch_down_tree(&child.children, nx, ny, event);
            if event.stopped {
                return;
            }
            child.node.run_on_down(event);
            if event.stopped {
                return;
            }
        }
    }
}

/// 递归分发 pointer up 事件。
#[allow(dead_code)]
fn dispatch_up_tree(nodes: &[ExpandedNode], x: f32, y: f32, event: &mut MvePointerEvent) {
    let mut i = nodes.len();
    while i > 0 {
        i -= 1;
        let child = &nodes[i];
        let nx = x - child.node.x;
        let ny = y - child.node.y;
        if child.node.run_hit_test(nx, ny) {
            dispatch_up_tree(&child.children, nx, ny, event);
            if event.stopped {
                return;
            }
            child.node.run_on_up(event);
            if event.stopped {
                return;
            }
        }
    }
}

/// MVE 应用：连接 MVE Node 树与渲染引擎。
///
/// 核心模式（复刻 Kotlin Renderer.signal.collect { draw() }）：
/// - `draw()` 中调用 `root_builder` 构建 Node 树 → `RedrawTracker` 自动追踪信号
/// - 信号变化时 `RedrawTracker` 自动触发重绘
/// - `draw()` 重新调用 `root_builder` → Node 树重建 → 展开树重建
/// - 零 `create_effect`，框架层全自动
pub struct MveApp {
    root_builder: Rc<dyn Fn(&mut NodeContext)>,
    expanded_tree: UnsafeCell<Option<Vec<ExpandedNode>>>,
    cursor: UnsafeCell<(f32, f32)>,
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
            expanded_tree: UnsafeCell::new(None),
            cursor: UnsafeCell::new((0.0, 0.0)),
        }
    }

    /// 创建 MVE 应用。
    ///
    /// `callback` 中读取的信号会被 `RedrawTracker` 自动追踪。
    /// 信号变化时自动触发重绘，`draw()` 重新调用 `callback` 重建 Node 树。
    pub fn new(callback: impl Fn(&mut NodeContext) + 'static) -> Self {
        Self {
            root_builder: Rc::new(callback),
            expanded_tree: UnsafeCell::new(None),
            cursor: UnsafeCell::new((0.0, 0.0)),
        }
    }
}

impl crate::runner::WyApp for MveApp {
    fn draw(&self, scene: &mut Scene, _width: f32, _height: f32) {
        // 等价于 Kotlin: signal.collect { draw() }
        // root_builder 中读取的信号被 RedrawTracker 自动追踪
        //
        // SAFETY: draw() 仅在 runner 的 render() 中通过 tracker.draw() 调用，
        // runner 拥有 self.app 的独占访问权，且 &self 是 tracker 创建的临时引用，
        // 不与其他代码并发访问 expanded_tree/cursor。
        let expanded_tree = unsafe { &mut *self.expanded_tree.get() };

        let mut cx = NodeContext::new(0);
        (self.root_builder)(&mut cx);
        let nodes = cx.into_nodes();

        // 展开节点树（保留 arg_children 设置的 x/y/width/height）
        *expanded_tree = Some(expand_tree(&nodes));

        // 从展开树绘制
        if let Some(tree) = expanded_tree.as_ref() {
            draw_tree(tree, scene);
        }
    }

    fn widget_tree(&mut self) -> Option<&mut wy_render::widget_tree::WidgetTree> {
        // 不使用 WidgetTree：我们直接从展开的节点树绘制和处理事件
        None
    }

    fn on_resize(&mut self, _width: f32, _height: f32) {
        // 组件设置自己的尺寸，窗口变化时不需要重新布局
        // 但需要重绘（信号变化会自动触发）
    }

    fn handle_event(&mut self, event: &winit::event::WindowEvent) -> bool {
        use winit::event::{ElementState, MouseButton, WindowEvent};

        // SAFETY: handle_event 在 runner 的事件循环中调用，
        // runner 拥有 self.app 的独占访问权。
        let cursor = unsafe { &mut *self.cursor.get() };
        let expanded_tree = unsafe { &mut *self.expanded_tree.get() };

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
                if let Some(tree) = expanded_tree.as_ref() {
                    if hit_test_tree(tree, x, y) {
                        let mut mve_event = MvePointerEvent::new(x, y);
                        dispatch_click_tree(tree, x, y, &mut mve_event);
                        return mve_event.stopped;
                    }
                }
                false
            }
            _ => false,
        }
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
    use crate::runner::WyApp;
    use wy_signal::{GetValue, SetValue, Signal};

    #[test]
    fn expand_tree_preserves_positions() {
        let text_node = Node {
            x: 10.0,
            y: 20.0,
            width: 100.0,
            height: 30.0,
            ..Node::default()
        };
        let btn_node = Node {
            x: 50.0,
            y: 60.0,
            width: 80.0,
            height: 40.0,
            ..Node::default()
        };

        let nodes = vec![text_node, btn_node];
        let expanded = expand_tree(&nodes);

        assert_eq!(expanded.len(), 2);
        assert_eq!(expanded[0].node.x, 10.0);
        assert_eq!(expanded[0].node.y, 20.0);
        assert_eq!(expanded[1].node.x, 50.0);
        assert_eq!(expanded[1].node.y, 60.0);
    }

    #[test]
    fn expand_tree_preserves_children() {
        let parent = Node {
            arg_children_fn: Rc::new(move |cx| {
                cx.add_node(Node {
                    x: 5.0,
                    y: 10.0,
                    width: 20.0,
                    height: 15.0,
                    ..Node::default()
                });
            }),
            ..Node::default()
        };

        let expanded = expand_tree(&[parent]);
        assert_eq!(expanded.len(), 1);
        assert_eq!(expanded[0].children.len(), 1);
        assert_eq!(expanded[0].children[0].node.x, 5.0);
        assert_eq!(expanded[0].children[0].node.y, 10.0);
    }

    #[test]
    fn expand_tree_with_row_component() {
        use wy_mve::components::row;

        let row_node = row(|cx| {
            cx.add_node(Node {
                width: 50.0,
                height: 20.0,
                ..Node::default()
            });
            cx.add_node(Node {
                width: 60.0,
                height: 20.0,
                ..Node::default()
            });
        });

        let expanded = expand_tree(&[row_node]);
        assert_eq!(expanded.len(), 1);
        assert_eq!(expanded[0].children.len(), 2);
        // row 设置的 x 位置
        assert_eq!(expanded[0].children[0].node.x, 0.0);
        assert_eq!(expanded[0].children[1].node.x, 58.0); // 50 + 8 gap
    }

    #[test]
    fn hit_test_tree_finds_node() {
        let node = Node {
            width: 100.0,
            height: 50.0,
            hit_test_fn: Rc::new(|x, y| (0.0..100.0).contains(&x) && (0.0..50.0).contains(&y)),
            on_click_fn: Some(Rc::new(|_| {})),
            ..Node::default()
        };
        let expanded = expand_tree(&[node]);

        assert!(hit_test_tree(&expanded, 50.0, 25.0));
        assert!(!hit_test_tree(&expanded, 150.0, 25.0));
    }

    #[test]
    fn hit_test_tree_with_children() {
        let parent = Node {
            width: 200.0,
            height: 100.0,
            hit_test_fn: Rc::new(|x, y| (0.0..200.0).contains(&x) && (0.0..100.0).contains(&y)),
            arg_children_fn: Rc::new(|cx| {
                cx.add_node(Node {
                    x: 10.0,
                    y: 10.0,
                    width: 80.0,
                    height: 40.0,
                    hit_test_fn: Rc::new(|x, y| {
                        (0.0..80.0).contains(&x) && (0.0..40.0).contains(&y)
                    }),
                    on_click_fn: Some(Rc::new(|_| {})),
                    ..Node::default()
                });
            }),
            ..Node::default()
        };
        let expanded = expand_tree(&[parent]);

        // 点击子节点区域内
        assert!(hit_test_tree(&expanded, 50.0, 30.0));
        // 点击父节点但不在子节点内（在子节点右侧）
        assert!(!hit_test_tree(&expanded, 150.0, 30.0));
    }

    #[test]
    fn signal_rebuild_works() {
        let count = Signal::new(0);
        let count_clone = count.clone();

        let app = MveApp::new(move |cx| {
            let val = count_clone.get();
            cx.add_node(Node {
                width: val as f32 * 10.0,
                height: 20.0,
                ..Node::default()
            });
        });

        // 第一次 draw
        let mut scene = Scene::new();
        app.draw(&mut scene, 100.0, 100.0);

        let tree = unsafe { &*app.expanded_tree.get() };
        assert_eq!(tree.as_ref().unwrap()[0].node.width, 0.0);

        // 改变信号
        count.set(5);

        // 第二次 draw
        app.draw(&mut scene, 100.0, 100.0);

        let tree = unsafe { &*app.expanded_tree.get() };
        assert_eq!(tree.as_ref().unwrap()[0].node.width, 50.0);
    }

    /// 模拟 counter 完整结构：column_at > text + row > button-, button+
    /// 验证点击按钮能命中并触发 on_click_fn
    #[test]
    fn counter_click_dispatches_to_button() {
        use wy_mve::components::{button, column_at, row, text_signal};

        let count = Signal::new(0);

        // 构建和 counter demo 相同的树结构
        let mut root_cx = NodeContext::new(0);
        let text_count = count.clone();
        let col_count = count.clone();
        let col_node = column_at(350.0, 250.0, move |cx| {
            let c = text_count.clone();
            cx.child(text_signal(move || Box::new(format!("Count: {}", c.get()))));

            let row_count = col_count.clone();
            cx.child(row(move |cx| {
                cx.child(button(|| "−".into(), {
                    let s = row_count.clone();
                    move || s.set(s.get() - 1)
                }));
                cx.child(button(|| "+".into(), {
                    let s = row_count.clone();
                    move || s.set(s.get() + 1)
                }));
            }));
        });
        root_cx.add_node(col_node);
        let nodes = root_cx.into_nodes();

        // 展开树
        let expanded = expand_tree(&nodes);

        // 验证树结构
        assert_eq!(expanded.len(), 1); // column_at
        let col = &expanded[0];
        assert_eq!(col.node.x, 350.0);
        assert_eq!(col.node.y, 250.0);
        assert_eq!(col.children.len(), 2); // text + row

        let row = &col.children[1];
        assert_eq!(row.children.len(), 2); // button- and button+
        let btn_minus = &row.children[0];
        let btn_plus = &row.children[1];
        assert_eq!(btn_minus.node.x, 0.0); // row arranges: first at x=0
        assert_eq!(btn_plus.node.x, 98.0); // 90 + 8 gap
        assert_eq!(btn_minus.node.width, 90.0);
        assert_eq!(btn_plus.node.width, 90.0);
        assert!(btn_minus.node.on_click_fn.is_some());
        assert!(btn_plus.node.on_click_fn.is_some());

        // 点击 "+" 按钮中心（屏幕坐标）
        let click_x = 350.0 + 98.0 + 45.0;
        let click_y = 250.0 + 8.0 + 16.0;

        assert!(hit_test_tree(&expanded, click_x, click_y));

        // 分发点击
        let mut event = MvePointerEvent::new(click_x, click_y);
        dispatch_click_tree(&expanded, click_x, click_y, &mut event);
        assert!(event.stopped, "event should be stopped by button handler");

        // 验证信号已更新
        assert_eq!(count.get(), 1);
    }

    /// 直接测试坐标变换链：screen → column → row → button
    #[test]
    fn coordinate_chain_debug() {
        let click_x = 350.0 + 98.0 + 45.0; // = 493
        let click_y = 250.0 + 8.0 + 16.0; // = 274

        // column_at(350, 250) → nx = 493-350 = 143, ny = 274-250 = 24
        let col_nx = click_x - 350.0;
        let col_ny = click_y - 250.0;
        assert_eq!(col_nx, 143.0);
        assert_eq!(col_ny, 24.0);

        // row 在 column 中的位置：text height=0 + gap=8 → row.y = 8
        let row_ny = col_ny - 8.0;
        assert_eq!(row_ny, 16.0);

        // button+ 在 row 中的位置：x = 90 + 8 = 98
        let btn_nx = col_nx - 98.0;
        let btn_ny = row_ny;
        assert_eq!(btn_nx, 45.0); // button center x
        assert_eq!(btn_ny, 16.0); // button center y

        // button hit_test: 45 < 90 AND 16 < 32 → should be true
        let btn = Node {
            width: 90.0,
            height: 32.0,
            ..Node::default()
        };
        assert!(btn.run_hit_test(btn_nx, btn_ny));
    }
}
