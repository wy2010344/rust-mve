//! 引擎整合：事件系统、焦点管理、无障碍。
//!
//! 将信号系统、布局引擎、渲染管线、文本排版整合为完整的 UI 引擎。
//!
//! 核心类型：
//! - [`Event`] / [`PointerEvent`] / [`KeyEvent`] — 统一事件模型
//! - [`FocusManager`] — 焦点注册、Tab 遍历、焦点陷阱
//! - [`AccessibilityBridge`] — Widget 树到 AccessKit 的映射
//! - [`runner`] — winit + wgpu + Vello 应用运行器

pub use accesskit;

mod accessibility;
pub mod composition;
mod event;
mod focus;
pub mod mve_integration;
pub mod runner;
pub mod winit_translate;

pub use accessibility::{AccessNode, AccessRole, AccessibilityBridge};
pub use event::{Event, Key, KeyEvent, PointerDevice, PointerEvent, PointerType};
pub use focus::{FocusManager, FocusableNode};
