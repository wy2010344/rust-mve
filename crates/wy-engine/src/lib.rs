//! 引擎整合：事件系统、焦点管理、无障碍。
//!
//! 将信号系统、布局引擎、渲染管线、文本排版整合为完整的 UI 引擎。
//!
//! 核心类型：
//! - [`Event`] / [`PointerEvent`] / [`KeyEvent`] — 统一事件模型
//! - [`AccessibilityBridge`] — Widget 树到 AccessKit 的映射
//! - [`runner`] — winit + wgpu + Vello 应用运行器

pub use accesskit;

mod accessibility;
pub mod composition;
mod event;
pub mod frame_source;
pub mod ime_guard;
pub mod node2_app;
pub mod redraw_tracker;
pub mod runner;
pub mod winit_translate;

pub use accessibility::{AccessNode, AccessRole, AccessibilityBridge};
pub use event::{Event, Key, KeyEvent, PointerDevice, PointerEvent, PointerType};
