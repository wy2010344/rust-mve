//! 统一事件模型：从 wy-render 重新导出。
//!
//! 所有事件类型定义在 `wy-render` 中以避免循环依赖。
//! 本模块提供 `wy_engine::event::*` 的便捷访问路径。

pub use wy_render::event::{Event, Key, KeyEvent, PointerDevice, PointerEvent, PointerType};
