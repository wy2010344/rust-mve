//! 渲染管线：Widget trait + Scene 中间层 + Vello 集成。
//!
//! 核心类型：
//! - [`Widget`] — 用户实现的 UI 组件 trait
//! - [`Scene`] — 平台无关的绘制命令记录器
//! - [`DrawContext`] — draw 调用的上下文，提供布局信息
//! - [`Color`] / [`Rect`] — 绘制所需的类型安全颜色与几何基础

mod color;
mod draw_context;
pub mod event;
mod math;
mod scene;
pub mod vello_executor;
pub mod widget;
pub mod widget_tree;
pub mod widgets;

pub use color::Color;
pub use draw_context::DrawContext;
pub use math::{Point, Rect, Size};
pub use scene::{Primitive, Scene};
pub use widget::{ChildBuilder, Widget};
