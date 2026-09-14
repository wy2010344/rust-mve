//! MVE（Model-View-Engine）框架：信号驱动的 UI 树构建。
//!
//! 复刻 Kotlin wy-helper 的 MVE 模式（构建一次 + memo 动态区域 + per-item 信号驱动重绘）。
//!
//! 核心类型：
//! - [`Node`] — UI 元素的具象类型（声明式构造，用闭包代替继承）
//! - [`ChildSlot`] — 静态节点或动态区域
//! - [`Layout`] — 一维容器排布（Row / Column）
//! - [`NodeContext`] — `arg_children()` 的执行上下文
//! - [`Root`] — 由 [`render_root`] 返回的根 memo 产物
//!
//! 组件函数（声明式 UI 组合）：
//! - [`components::text`] / [`components::text_signal`] — 文本
//! - [`components::button`] — 按钮
//! - [`components::row`] / [`components::column`] — 布局容器

pub mod components;
mod context;
mod foreach;
mod node;

pub use components::{
    button, column, column_at, row, row_at, spacer, text, text_signal, text_styled,
};
pub use context::{render_root, NodeContext, Root};
pub use foreach::render_for_each;
pub use node::{
    children_nodes, flatten, has_handler, layout_offsets, materialize, node_size, ChildSlot, Key,
    KeyEvent, Layout, Node, PointerEvent,
};

#[cfg(test)]
mod tests;
