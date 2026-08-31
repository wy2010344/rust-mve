//! 组合式应用入口：纯函数定义 UI，无需定义结构体。
//!
//! 类似 Kotlin 的 `SkiaApp` + `argChildren()` 模式——
//! `run_composition` 直接接受闭包，闭包内用局部信号 + FnWidget 构建 UI 树。

use std::cell::RefCell;
use std::rc::Rc;

use wy_render::composition::FnWidget;
use wy_render::widget::ChildBuilder;
use wy_render::widget_tree::WidgetTree;
use wy_render::{Rect, Scene};

/// 组合式应用：用闭包定义 UI，无需定义结构体。
///
/// 每次 `draw()` 时重建 UI 树（保证信号值最新），
/// 同时存储到 `current_tree`，供 `handle_event()` 分发事件。
pub struct Composition {
    tree_builder: Rc<dyn Fn(&mut ChildBuilder)>,
    current_tree: RefCell<Option<WidgetTree>>,
    mouse_x: f64,
    mouse_y: f64,
}

impl Composition {
    pub fn new(tree_builder: impl Fn(&mut ChildBuilder) + 'static) -> Self {
        Self {
            tree_builder: Rc::new(tree_builder),
            current_tree: RefCell::new(None),
            mouse_x: 0.0,
            mouse_y: 0.0,
        }
    }
}

impl crate::runner::WyApp for Composition {
    fn setup(&mut self, _request_redraw: Rc<dyn Fn()>) {}

    fn draw(&mut self, scene: &mut Scene, width: f32, height: f32) {
        // 重建 UI 树——闭包内读取的信号值是最新的
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
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => {
                let key_event = match &event.logical_key {
                    winit::keyboard::Key::Character(s) if s.as_str() == "\r" => Some(
                        wy_render::event::KeyEvent::new(wy_render::event::Key::Enter, true),
                    ),
                    winit::keyboard::Key::Named(winit::keyboard::NamedKey::Backspace) => Some(
                        wy_render::event::KeyEvent::new(wy_render::event::Key::Backspace, true),
                    ),
                    winit::keyboard::Key::Character(s) => {
                        let ch = s.chars().next().unwrap_or('\0');
                        Some(wy_render::event::KeyEvent::new(
                            wy_render::event::Key::Char(ch),
                            true,
                        ))
                    }
                    _ => None,
                };
                if let Some(key_event) = key_event {
                    if let Some(tree) = self.current_tree.borrow_mut().as_mut() {
                        tree.dispatch_key_event(&key_event)
                    } else {
                        false
                    }
                } else {
                    false
                }
            }
            _ => false,
        }
    }
}

/// 启动组合式应用——纯函数，无需定义结构体。
pub fn run_composition(tree_builder: impl Fn(&mut ChildBuilder) + 'static) {
    let _ = crate::runner::run(Composition::new(tree_builder));
}
