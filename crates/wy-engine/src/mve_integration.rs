//! MVE 集成：将 `wy-mve` 的 Node 树连接到渲染管线。
//!
//! 流程：`render_root` → `ChildrenCache` → `MveWidget` → `WidgetTree` → `Scene` → Vello

use std::cell::RefCell;
use std::rc::Rc;

use wy_mve::{ChildrenCache, Node};
use wy_render::composition::FnWidget;
use wy_render::widget::ChildBuilder;
use wy_render::DrawContext;
use wy_render::{Rect, Scene, Widget};

/// 将 MVE Node 树转为 Widget 树的适配器。
///
/// 每个 `MveWidget` 持有一个 `Node`，`draw()` 调用 `node.run_draw(scene)`。
struct MveWidget {
    node: Node,
}

impl Widget for MveWidget {
    fn draw(&self, scene: &mut Scene, _cx: &mut DrawContext) {
        self.node.run_draw(scene);
    }

    fn hit_test(&self, x: f32, y: f32, _cx: &DrawContext) -> bool {
        self.node.run_hit_test(x, y)
    }

    fn on_click(&mut self, _cx: &DrawContext) {
        // 事件类型在 wy-mve 和 wy-render 之间统一前，简化处理
        // TODO: 统一 PointerEvent 类型
    }

    fn focusable(&self) -> bool {
        self.node.is_focusable()
    }
}

/// MVE 应用：连接 MVE Node 树与渲染引擎。
///
/// 每帧从 `ChildrenCache` 读取最新节点树，转为 `WidgetTree` 渲染。
pub struct MveApp {
    cache: ChildrenCache,
    tree_builder: Rc<dyn Fn(&mut ChildBuilder)>,
    mouse_x: f64,
    mouse_y: f64,
}

impl MveApp {
    /// 从 `ChildrenCache` 创建。
    ///
    /// 直接将 `ChildrenCache` 中的每个 Node 包装为 `MveWidget`。
    pub fn from_cache(cache: ChildrenCache) -> Self {
        let nodes = cache.get();
        let nodes = Rc::new(RefCell::new(nodes));
        let nodes_ref = nodes.clone();

        Self {
            cache,
            tree_builder: Rc::new(move |cx| {
                for node in nodes_ref.borrow().iter() {
                    cx.add_child(MveWidget { node: node.clone() });
                }
            }),
            mouse_x: 0.0,
            mouse_y: 0.0,
        }
    }
}

impl crate::runner::WyApp for MveApp {
    fn setup(&mut self, _request_redraw: Rc<dyn Fn()>) {}

    fn draw(&mut self, scene: &mut Scene, width: f32, height: f32) {
        // 从 ChildrenCache 获取最新节点树
        let _fresh_nodes = self.cache.get();

        // 用 FnWidget 包装，创建 WidgetTree
        let builder = self.tree_builder.clone();
        let root = FnWidget::new(|_, _| {}, move |cx| builder(cx));
        let mut tree = wy_render::widget_tree::WidgetTree::new(root);
        tree.set_layout(0, Rect::new(0.0, 0.0, width, height));
        tree.draw_scene(scene);
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
            } => false,
            _ => false,
        }
    }
}

/// 启动 MVE 应用。
///
/// `builder` 构建 MVE 节点树，返回 `ChildrenCache`。
pub fn run_mve(builder: impl Fn() -> ChildrenCache + 'static) {
    let cache = builder();
    let app = MveApp::from_cache(cache);
    let _ = crate::runner::run(app);
}
