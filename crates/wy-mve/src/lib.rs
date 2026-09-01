//! MVE（Model-View-Engine）框架：信号驱动的 UI 树构建。
//!
//! 复刻 Kotlin wy-helper 的 MVE 模式。
//!
//! 核心类型：
//! - [`Node`] — UI 元素的具象类型（用闭包代替继承）
//! - [`NodeContext`] — `arg_children()` 的执行上下文
//! - [`StateHolder`] — 持有构建后的节点树

mod app;
mod context;
mod node;
mod state_holder;

pub use app::{run_mve_app, MouseButton, MveApp, WindowEvent};
pub use context::{render_root, ChildrenCache, Context, NodeContext};
pub use node::{Key, KeyEvent, Node, PointerEvent};
pub use state_holder::StateHolder;

#[cfg(test)]
mod tests;
