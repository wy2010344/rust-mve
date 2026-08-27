//! 信号系统：push-pull 混合响应式状态管理。
//!
//! 核心原语：
//! - [`Signal`] — 叶子节点，存储值 + 订阅者集合
//! - [`Memo`] — 派生节点，惰性求值 + relay map 快照比对
//! - [`TrackSignal`] — 副作用观察者，依赖动态收集

mod signal;
mod memo;
mod track;
mod batch;

pub use signal::{Signal, SignalId};
pub use memo::Memo;
pub use track::TrackSignal;
pub use batch::batch;
