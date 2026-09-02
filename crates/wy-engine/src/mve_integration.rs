//! MVE 集成：将 `wy-mve` 的 Node 树连接到渲染管线。
//!
//! 流程：callback → NodeContext → MveWidget → WidgetTree → Scene → Vello
//!
//! 每帧直接调用 callback 构建 Node 树（实时读取信号），不使用缓存。

use std::cell::RefCell;
use std::rc::Rc;

use wy_mve::{Node, NodeContext, PointerEvent as MvePointerEvent};
use wy_render::composition::FnWidget;
use wy_render::widget::ChildBuilder;
use wy_render::widget_tree::WidgetTree;
use wy_render::{DrawContext, Rect, Scene, Widget};

/// 将 MVE Node 树转为 Widget 树的适配器。
///
/// 每个 MveWidget 包装一个 MVE Node，并将其 `arg_children` 暴露为 Widget 子节点。
struct MveWidget {
    node: Node,
}

impl Widget for MveWidget {
    fn draw(&self, scene: &mut Scene, cx: &mut DrawContext) {
        let pos = cx.inner_origin();
        if pos.x != 0.0 || pos.y != 0.0 {
            scene.push_transform(pos);
        }
        self.node.run_draw(scene);
        if pos.x != 0.0 || pos.y != 0.0 {
            scene.pop_transform();
        }
    }

    fn children(&self, cx: &mut ChildBuilder) {
        let mut child_cx = NodeContext::new(0);
        self.node.run_arg_children(&mut child_cx);
        for child in child_cx.into_nodes() {
            cx.add_child(MveWidget { node: child });
        }
    }

    fn hit_test(&self, x: f32, y: f32, cx: &DrawContext) -> bool {
        let rect = cx.outer_rect();
        x >= rect.x && x < rect.x + rect.width && y >= rect.y && y < rect.y + rect.height
    }

    fn on_click(&mut self, _cx: &DrawContext) {
        let mut event = MvePointerEvent::new(0.0, 0.0);
        self.node.run_on_click(&mut event);
    }

    fn focusable(&self) -> bool {
        self.node.is_focusable()
    }
}

/// MVE 应用：连接 MVE Node 树与渲染引擎。
///
/// 每帧调用 callback 构建 Node 树（实时读取信号），然后通过 WidgetTree 渲染和事件分发。
pub struct MveApp {
    callback: Rc<dyn Fn(&mut NodeContext)>,
    current_tree: RefCell<Option<WidgetTree>>,
    request_redraw: RefCell<Option<Rc<dyn Fn()>>>,
    mouse_x: f64,
    mouse_y: f64,
}

impl MveApp {
    /// 创建 MVE 应用。
    ///
    /// `callback` 在每帧 draw 时被调用，读取信号并构建 Node 树。
    /// 这是真正的 MVE 模式：信号在每次绘制时实时读取，无缓存。
    pub fn new(callback: impl Fn(&mut NodeContext) + 'static) -> Self {
        Self {
            callback: Rc::new(callback),
            current_tree: RefCell::new(None),
            request_redraw: RefCell::new(None),
            mouse_x: 0.0,
            mouse_y: 0.0,
        }
    }

    fn redraw(&self) {
        if let Some(cb) = self.request_redraw.borrow().as_ref() {
            cb();
        }
    }

    /// 自定义布局：root 垂直排列子节点，list item 内水平排列按钮。
    fn layout_tree(tree: &mut WidgetTree, width: f32, height: f32) {
        // root 获得全窗口
        tree.set_layout(0, Rect::new(0.0, 0.0, width, height));

        let root_children = tree.children_of(0);
        let mut y_offset = 0.0f32;
        for &child_idx in &root_children {
            // 每个顶层子节点高度50px
            let child_rect = Rect::new(0.0, y_offset, width, 50.0);
            tree.set_layout(child_idx, child_rect);

            // 子节点的子节点（按钮）水平排列
            let button_children = tree.children_of(child_idx);
            let mut x_offset = 0.0f32;
            for &btn_idx in &button_children {
                let btn_rect = Rect::new(x_offset, 0.0, 80.0, 40.0);
                tree.set_layout(btn_idx, btn_rect);
                x_offset += 85.0;
            }

            y_offset += 50.0;
        }
    }
}

impl crate::runner::WyApp for MveApp {
    fn setup(&mut self, request_redraw: Rc<dyn Fn()>) {
        *self.request_redraw.borrow_mut() = Some(request_redraw);
    }

    fn draw(&mut self, scene: &mut Scene, width: f32, height: f32) {
        // 实时调用 callback 构建 Node 树（读取信号）
        let mut cx = NodeContext::new(0);
        (self.callback)(&mut cx);
        let nodes = std::rc::Rc::new(std::cell::RefCell::new(cx.into_nodes()));

        // 构建 WidgetTree
        let nodes_ref = nodes.clone();
        let root = FnWidget::new(
            |_, _| {},
            move |builder| {
                for node in nodes_ref.borrow().iter() {
                    builder.add_child(MveWidget { node: node.clone() });
                }
            },
        );
        let mut tree = WidgetTree::new(root);

        // 自定义布局
        Self::layout_tree(&mut tree, width, height);

        tree.draw_scene(scene);
        *self.current_tree.borrow_mut() = Some(tree);
    }

    fn handle_event(&mut self, event: &winit::event::WindowEvent) -> bool {
        use winit::event::{ElementState, MouseButton, WindowEvent};

        match event {
            WindowEvent::CursorMoved { position, .. } => {
                self.mouse_x = position.x;
                self.mouse_y = position.y;
                false
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } => {
                let handled = if let Some(tree) = self.current_tree.borrow_mut().as_mut() {
                    let x = self.mouse_x as f32;
                    let y = self.mouse_y as f32;
                    tree.dispatch_pointer_down(x, y);
                    tree.dispatch_pointer_up(x, y)
                } else {
                    false
                };
                self.redraw();
                handled
            }
            WindowEvent::MouseInput {
                state: ElementState::Released,
                button: MouseButton::Left,
                ..
            } => {
                // Released 也触发 redraw（确保状态同步）
                self.redraw();
                false
            }
            _ => false,
        }
    }
}

/// 启动 MVE 应用（兼容旧 API）。
///
/// `builder` 返回的 `ChildrenCache` 会在每帧被读取。
/// 推荐直接使用 `MveApp::new(callback)` + `runner::run(app)`。
pub fn run_mve(builder: impl Fn() -> wy_mve::ChildrenCache + 'static) {
    let cache = builder();
    let cache_ref = cache.clone();
    let app = MveApp::new(move |cx| {
        for node in cache_ref.get() {
            cx.add_node(node);
        }
    });
    let _ = crate::runner::run(app);
}
