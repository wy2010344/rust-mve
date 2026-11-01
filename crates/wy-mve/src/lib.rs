//! MVE（Model-View-Engine）框架：信号驱动的声明式 UI 树。
//!
//! **只有一套写法**：`canvasRender2`（对标 TS `canvasRender2`）。入口是
//! [`Node2App`](../../wy_engine/node2_app/struct.Node2App.html) +
//! [`canvas_render2::build_root_node2`] + [`node2!`] + [`macro@rec`] + [`Fc`]。
//!
//! ```ignore
//! use wy_mve::canvas_render2::build_root_node2;
//! use wy_mve::canvas_render2::node2::grow;
//! use wy_mve::{node2, rec, text_label, Fc, AlignItem};
//!
//! fn page() -> Fc {
//!     node2! {
//!         layout: flex! { direction: Layout::Column, align_item: AlignItem::Stretch, align_fix: true },
//!         exts: vec![grow(1.0)],
//!         children: vec![ text_label! { content: "Hello".to_string() } ],
//!     }
//! }
//! ```
//!
//! # 模块划分
//!
//! - [`canvas_render2`] — 全部节点模型、基础组件、布局策略、事件协议、选择/焦点。
//! - [`primitives`] — 宿主与框架共用的纯数据类型（光标、布局轴、键位、IME）。
//! - [`icons`] — 矢量图标（自绘路径，不依赖字体字形）。
//! - `context` — 构建上下文 [`NodeContext`]（父指针 + `provide` / `consume`）。
//! - `foreach` — 动态区域（`render_for_each` / `render_when`）。
//! - `node_ref` — 提前释放的几何句柄 [`NodeRef`] / [`NodeGeom`]。
//!
//! # 已删除的旧声明层
//!
//! `#[component]`、`rsx!`、`Node`（渲染 IR）、`Slot` / `ChildArg`、`components`
//! 组件库、`scroll` / `rich_text` / `text_field` / `overlay` 的 v1 版本、
//! `MveApp` 运行时——全部随迁移完成删除。仓库里没有任何 v1 消费者。

// 让 `#[rec]` 生成的 `::wy_mve::...` 路径在本 crate 内部也能解析
// （框架自带组件 `node2` / `scroll_content` / `editable_text_node` 就用同一套宏写）。
extern crate self as wy_mve;

pub mod canvas_render2;
mod context;
mod foreach;
pub mod icons;
mod node_ref;
pub mod primitives;

pub use context::NodeContext;
pub use icons::{draw_icon, Icon};
pub use node_ref::{node_of, NodeGeom, NodeRef};
pub use primitives::{CursorType, ImeEvent, Key, KeyEvent, Layout, LayoutBox, PointerButton};

/// 新体系入口：基础元素 `node2`、`Fc`、`Node2` 声明模型、`scroll` 等，
/// 全部经 [`canvas_render2`] 一个模块导出（这里只是便利重导出）。
pub use canvas_render2::element::node2;
/// `#[rec(extends = base)]` 的宏展开中间件：展开产物要按**绝对路径**引用它们
/// （不能引用 `#[macro_export]` 宏），因此必须 `pub`；但不属于对外契约，
/// 用 `#[doc(hidden)]` 隐去。
#[doc(hidden)]
pub use canvas_render2::element::{Node2Props, Node2PropsBuilder};
/// canvasRender2 的节点模型（对标 TS `Node2` / `Node2Arg` / `exts` / `flex`）。
///
/// 节点参数是 [`Node2Props`]（来自 `node2` 组件的签名），没有额外的 IR 层。
pub use canvas_render2::node2::{
    absolute, align, align_xy, always, axis_of, find_ext, get, grow, grow_dyn, grow_xy,
    ignore_flex, ignore_flex_dyn, AbsoluteLayoutDir, AlignChild, Axis, Ext, Flex, FlexArg,
    GrowChild, IgnoreFlexChild, ImeExt, LayoutDir, LayoutDir2, LayoutSize, Node2, StartEnd,
};
/// canvasRender2 的滚动原语（TS `Scroll.ts` / `ScrollBar.ts`）：`extends = node2`
/// 的声明式组件，`scroll_content!` / `scroll_bar!` 与 `node2!` 同构（宏与函数同名，
/// 一并导出：本行同时带出函数与 `#[macro_export]` 宏）。
pub use canvas_render2::{
    editable_text_node, image_node, overlay_scroll_bar, rect_node, rich_text_node, scroll_bar,
    scroll_content, EditableTextNodeArg, ImageNodeArg, OverlayScrollBarArg, RectNodeArg,
    RichTextNodeArg, ScrollBarArg, ScrollBarStyle2, ScrollContentArg,
};
/// canvasRender2 自带的文本节点（TS `WrappedTextNode.ts` / `RichTextNode.ts`）。
pub use canvas_render2::{wrapped_text_node, WrappedTextNodeArg};
/// canvasRender2 的组件产物类型：`Fc = (ctx) => void`，以及节点句柄的槽
/// `Ref2<M>`（TS `StoreRef<M>`）。组件是普通函数、直接返回 `Fc`，没有柯里化。
pub use canvas_render2::{Fc, NodeApply, Ref2};
pub use wy_layout::{AlignItem, DirectionFixBetweenWhenOne, DirectionJustify};
/// 唯一的声明式组件宏：**只做「命名参数 + 默认值」**，外加一个同名 `!` 宏。
///
/// - 给函数生成 `XxxArg` + builder；`XxxArg::build()` 的返回类型就是
///   **函数签名上写的那个类型**（`-> Flex` 就产出 `Flex`，`-> Fc` 就产出 `Fc`）。
/// - 生成同名 `!` 宏，调用点写 `flex!{..}` / `todo_app!{..}`，支持字段简写。
/// - **不做任何隐式注入**：没有 ctx 注入、没有返回类型改写、没有自动 mount。
///   要句柄就声明一个 `Ref2<Node2>` 参数并给默认值，要挂载就自己 `Fc::new(..)`。
pub use wy_mve_macros::rec;
/// 颜色类型重新导出：`text_label` 等组件的 `color` 字段按值传入时要用到它。
pub use wy_render::Color;
/// 绘制目标重新导出：`draw: |s: &mut Scene, n: &Node2| { .. }` 的签名要用到它。
pub use wy_render::Scene;
