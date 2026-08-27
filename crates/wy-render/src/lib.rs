//! 渲染管线：Widget trait + Scene 中间层 + Vello 集成。
//!
//! 核心类型：
//! - [`Widget`] — 用户实现的 UI 组件 trait
//! - [`Scene`] — 平台无关的绘制命令记录器
//! - [`DrawContext`] — draw 调用的上下文，提供布局信息

mod draw_context;
mod scene;
mod widget;

pub use draw_context::DrawContext;
pub use scene::{Primitive, Scene};
pub use widget::Widget;
