use std::cell::{Cell, RefCell};
use std::rc::Rc;

/// 全局线程局部状态，用于信号依赖收集和批量更新。
thread_local! {
    static G: ThreadLocalState = ThreadLocalState::new();
}

struct ThreadLocalState {
    /// 当前正在执行的 TrackSignal
    current_track: Cell<Option<SignalId>>,
    /// 全局版本号，memo 用于快速短路
    state_version: Cell<u64>,
    /// 批次队列
    batch: RefCell<BatchState>,
}

struct BatchState {
    dirty: std::collections::HashSet<SignalId>,
    scheduled: bool,
}

/// 信号 ID，用于标识订阅者。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SignalId(usize);

/// 信号：存储值 + 订阅者集合。
///
/// 写入时将所有订阅者推入批次队列，读取时自动注册依赖。
pub struct Signal<T> {
    value: RefCell<T>,
    listeners: RefCell<Vec<SignalId>>,
    version: Cell<u64>,
}
