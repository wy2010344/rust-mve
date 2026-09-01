//! MVE 集成：将 `wy-mve` 的 Node 树连接到渲染管线。
//!
//! 流程：`render_root` → `ChildrenCache` → `MveWidget` → `WidgetTree` → `Scene` → Vello

use std::cell::RefCell;
use std::rc::Rc;

use wy_mve::{ChildrenCache, Node};
use wy_render::composition::FnWidget;
use wy_render::widget::ChildBuilder;
use wy_render::widget_tree::WidgetTree;
use wy_render::{DrawContext, Rect, Scene, Widget};

/// 将 MVE Node 树转为 Widget 树的适配器。
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

    fn on_click(&mut self, _cx: &DrawContext) {}

    fn focusable(&self) -> bool {
        self.node.is_focusable()
    }
}

/// MVE 应用：连接 MVE Node 树与渲染引擎。
///
/// 参照 `Composition` 模式：存储 WidgetTree，事件分发到已存储的树。
pub struct MveApp {
    #[expect(dead_code)]
    cache: ChildrenCache,
    tree_builder: Rc<dyn Fn(&mut ChildBuilder)>,
    current_tree: RefCell<Option<WidgetTree>>,
    mouse_x: f64,
    mouse_y: f64,
}

impl MveApp {
    /// 从 `ChildrenCache` 创建。
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
            current_tree: RefCell::new(None),
            mouse_x: 0.0,
            mouse_y: 0.0,
        }
    }
}

impl crate::runner::WyApp for MveApp {
    fn setup(&mut self, _request_redraw: Rc<dyn Fn()>) {}

    fn draw(&mut self, scene: &mut Scene, width: f32, height: f32) {
        // 重建 WidgetTree（与 Composition 同模式）
        let builder = self.tree_builder.clone();
        let root = FnWidget::new(|_, _| {}, move |cx| builder(cx));
        let mut tree = WidgetTree::new(root);
        tree.set_layout(0, Rect::new(0.0, 0.0, width, height));
        tree.draw_scene(scene);
        // 存储当前帧的树，供 handle_event 使用
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
                if let Some(tree) = self.current_tree.borrow_mut().as_mut() {
                    let x = self.mouse_x as f32;
                    let y = self.mouse_y as f32;
                    tree.dispatch_pointer_down(x, y);
                    tree.dispatch_pointer_up(x, y)
                } else {
                    false
                }
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
