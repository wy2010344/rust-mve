//! 信号系统：push-pull 混合响应式状态管理。
//!
//! 核心原语：
//! - [`Signal`] — 叶子节点，存储值 + 订阅者集合
//! - [`Memo`] — 派生节点，惰性求值 + relay map 快照比对
//! - [`TrackEffect`] / [`Track`] — 副作用观察者，依赖动态收集
//!
//! # 模型
//!
//! 采用 **push-notify + pull-evaluate** 混合模型，与 Kotlin 版 `wy-helper`
//! 的 `signal/` 模块保持一致：
//!
//! - 信号**写入**（`.set()`）时，做 old/new 比对，仅有变化才把依赖它的
//!   观察者加入批次队列（push 阶段）。
//! - 批次在闭包结束（`batch()` 出栈）后统一 **flush**：逐个调用观察者的
//!   `add_fun()`，由观察者对其依赖做 **pull** 求值 / 比对。
//! - `Memo` 在 pull 阶段使用 **relay map** 快照比对：记录每个依赖在计算时的
//!   snapshot，下次比对时逐一重新读取，整个依赖快照相等则返回缓存值，否则
//!   重算。配合全局 `state_version` 做 O(1) 短路。
//!
//! # 线程模型
//!
//! 全部状态（registry、当前观察者、批次队列）位于 `thread_local!`，
//! 信号与 memo 以 `Rc<RefCell<_>>` 组织，非 `Send`/`Sync`，仅在单一
//! UI 线程内使用。批量更新通过 [`batch`] 闭包包裹。

mod batch;
mod context;
mod get_;
mod memo;
mod signal;
mod track;

pub use batch::{batch, flush};
pub use get_::{GetValue, NodeId, SetValue};
pub use memo::{create_memo, Memo};
pub use signal::{create_signal, Signal};
pub use track::{create_effect, track, Disposed, Track, TrackEffect};
