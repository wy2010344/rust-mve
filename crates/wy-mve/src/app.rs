//! MveApp trait：MVE 应用的入口点。

use std::rc::Rc;

use crate::context::NodeContext;

/// MveApp trait：MVE 应用的入口点。
///
/// 对应 Kotlin 的 `SkiaApp`。
pub trait MveApp {
    /// 应用启动时调用。
    fn setup(&mut self, _request_redraw: Rc<dyn Fn()>) {}

    /// 绘制 UI。
    fn draw(&mut self, _width: f32, _height: f32) {}

    /// 处理窗口事件（由平台层调用）。
    fn handle_event(&mut self, _event: &WindowEvent) -> bool {
        false
    }
}

/// 窗口事件（平台无关表示）。
pub enum WindowEvent {
    CursorMoved {
        x: f64,
        y: f64,
    },
    MouseInput {
        pressed: bool,
        button: MouseButton,
    },
    KeyboardInput {
        key: crate::node::Key,
        pressed: bool,
        ctrl: bool,
        shift: bool,
        alt: bool,
    },
    Resize {
        width: f32,
        height: f32,
    },
    Close,
    Redraw,
}

/// 鼠标按钮。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
}

/// 启动 MVE 应用（简化版）。
pub fn run_mve_app(tree_builder: impl Fn(&mut NodeContext) + 'static) {
    let mut cx = NodeContext::new(0);
    tree_builder(&mut cx);
    // 完整版应启动 winit 事件循环
    // 暂时只构建树
}
