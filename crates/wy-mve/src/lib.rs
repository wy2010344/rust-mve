//! MVE（Model-View-Engine）框架：信号驱动的 UI 树构建。
//!
//! 复刻 Kotlin wy-helper 的 MVE 模式。
//!
//! 核心类型：
//! - [`Node`] — UI 元素的具象类型（声明式构造，用闭包代替继承）
//! - [`NodeContext`] — `arg_children()` 的执行上下文
//! - [`StateHolder`] — 持有构建后的节点树
//!
//! 组件函数（声明式 UI 组合）：
//! - [`components::text`] / [`components::text_signal`] — 文本
//! - [`components::button`] — 按钮
//! - [`components::row`] / [`components::column`] — 布局容器

mod app;
pub mod components;
mod context;
mod node;
mod signal_cache;
mod state_holder;

pub use app::{run_mve_app, MouseButton, MveApp, WindowEvent};
pub use components::{
    button, column, column_at, row, row_at, spacer, text, text_signal, text_styled,
};
pub use context::{render_root, ChildrenCache, Context, NodeContext};
pub use node::{Key, KeyEvent, Node, PointerEvent};
pub use state_holder::StateHolder;

#[cfg(test)]
mod tests;
