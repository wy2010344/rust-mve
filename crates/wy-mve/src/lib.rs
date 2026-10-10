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
//! - [`text_field`] / [`text_field_opts`] — 单行文本输入
//!
//! FC 风格 DSL（子节点作剩余参数，取代嵌套闭包与 `cx` 样板）：
//! - [`vbox`] / [`hbox`] — 竖排 / 横排容器
//! - [`group`] / [`focusable`] / [`focus_trap`] — 分组与焦点容器
//! - [`for_each`] — keyed 列表
//! - [`ChildArg`] — 需要 `cx` 的子节点写法
//! - [`collect`] — 摊平子节点参数
//! - [`text`] / [`button`] — 同名于 `components`，此处语义等价

pub mod components;
mod context;
mod fc;
mod foreach;
mod node;
mod rich_text;
mod text_field;

pub use components::{
    button, column, column_at, row, row_at, spacer, text, text_signal, text_styled,
};
pub use context::{render_root, NodeContext, Root};
pub use fc::{
    btn, collect, focus_trap, focusable, for_each, group, hbox, hbox_at, hbox_gap, hidden, label,
    vbox, vbox_at, vbox_gap, ChildArg, Children, Slot,
};
pub use foreach::render_for_each;
pub use node::{
    children_nodes, flatten, has_handler, layout_offsets, materialize, node_size, ChildSlot,
    ImeEvent, Key, KeyEvent, Layout, Node, PointerEvent,
};
pub use rich_text::{rich_editable_opts, rich_text, rich_text_opts, RichTextOpts};
pub use text_field::{text_field, text_field_opts, TextFieldOpts};

#[cfg(test)]
mod tests;
