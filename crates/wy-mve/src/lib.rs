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
//! - [`when`] — 按条件信号切换分支的动态区域
//! - [`ChildArg`] — 需要 `cx` 的子节点写法
//! - [`collect`] — 摊平子节点参数
//! - [`text`] / [`button`] — 同名于 `components`，此处语义等价

// 让 `#[component]` / `rsx!` 生成的 `::wy_mve::...` 路径在本 crate 内部也能解析
// （框架自带组件 scroll/dialog/dropdown 就是用同一套宏写的）。
extern crate self as wy_mve;

pub mod clipboard;
pub mod components;
mod cond;
mod context;
pub mod fc;
mod foreach;
pub mod getter;
pub mod icons;
mod node;
pub mod overlay;
mod rich_text;
pub mod scroll;
mod text_field;
pub mod text_util;

pub use components::{
    button, column, column_at, row, row_at, spacer, text, text_signal, text_styled,
};
pub use cond::render_when;
pub use context::{render_root, NodeContext, Root};
pub use fc::{
    btn, collect, focus_trap, focusable, for_each, group, hbox, hbox_at, hbox_gap, hidden, label,
    vbox, vbox_at, vbox_gap, when, ChildArg, Children, Slot,
};
pub use foreach::render_for_each;
/// 「裸值 / 闭包 → 取值函数」分发 trait，由 `#[getter]` 标注的 prop 使用。
pub use getter::IntoGetter;
/// 矢量图标组件（自绘路径，不依赖字体字形——字符图标在缺字形的机器上会变方块）。
pub use icons::{icon, Icon};
pub use node::{
    children_nodes, current_box, flatten, has_handler, layout_boxes, layout_offsets, materialize,
    node_size, with_box, ChildSlot, ImeEvent, Key, KeyEvent, Layout, LayoutBox, Node, PointerEvent,
};
/// 滚动容器 / 浮层组件（复刻 Kotlin SimpleScrollNode + Overlay/Dialog/Dropdown）。
pub use overlay::{dialog, dropdown};
pub use rich_text::{rich_editable_opts, rich_text, rich_text_opts, RichTextOpts};
pub use scroll::scroll;
pub use text_field::{text_field, text_field_opts, TextFieldColors, TextFieldOpts};
pub use wy_layout::{AlignItem, DirectionJustify};
/// 颜色类型重新导出：getter prop（如 `text_label` 的 `color`）按值传入时
/// 需要它能作为 `IntoGetter<Color>` 的值分支。
pub use wy_render::Color;
/// 绘制目标重新导出：让 `rsx! { draw: |s| .. }` 生成的 downcast 不必让调用方
/// 额外声明 `wy-render` 依赖（宏只认 `wy_mve` 一条路径）。
pub use wy_render::Scene;

/// `#[component]` 属性宏：把函数包装为 `rsx!` 可用的组件（见 `wy-mve-macros`）。
pub use wy_mve_macros::component;
/// `rsx!` 声明式 UI 宏（见 `wy-mve-macros`）。
pub use wy_mve_macros::rsx;

#[cfg(test)]
mod tests;
