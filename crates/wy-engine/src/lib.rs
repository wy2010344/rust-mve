//! 引擎整合：事件系统、焦点管理、无障碍。
//!
//! 将信号系统、布局引擎、渲染管线、文本排版整合为完整的 UI 引擎。

mod event;
mod focus;
mod accessibility;

pub use event::Event;
pub use focus::FocusManager;
pub use accessibility::AccessibilityBridge;
