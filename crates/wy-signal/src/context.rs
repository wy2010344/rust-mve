//! 全局线程局部状态（等价 Kotlin 版 `G` object）。
//!
//! 集中管理：节点注册表、当前执行中的观察者、批次队列、全局版本号。
//! 由于信号系统在单一 UI 线程内运行，这里不做 `Send`/`Sync`。

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use crate::get_::{NodeId, TrackDyn, ValBox};

thread_local! {
    static GLOBAL_VERSION: Cell<u64> = const { Cell::new(0) };

    /// 当前线程的信号系统全局状态。
    static G: RefCell<Global> = RefCell::new(Global::empty());
}

/// 读取全局版本号。
pub(crate) fn global_version() -> u64 {
    GLOBAL_VERSION.with(|v| v.get())
}

/// 记录一次"值实际变化"——递增全局版本号，供 memo 短路。
pub(crate) fn bump_global_version() {
    GLOBAL_VERSION.with(|v| v.set(v.get() + 1));
}

/// 线程局部全局状态。
pub(crate) struct Global {
    /// 当前正在执行 `add_fun`/memo 计算的观察者。
    pub current: Option<Rc<dyn TrackDyn>>,
    /// 批次队列（写阶段累积的待评估观察者）。
    pub batch: Vec<NodeId>,
    /// 是否处于 memo 计算中（禁止写入信号）。
    pub computing: bool,
    /// 观察者注册表：`NodeId -> Rc<dyn TrackDyn>`。
    pub registry: HashMap<NodeId, Rc<dyn TrackDyn>>,
    /// 下一个可用观察者节点 ID。
    pub next_id: usize,
    /// 嵌套批次深度（`batch()` 计数）。
    pub batch_depth: usize,
}

impl Global {
    fn empty() -> Self {
        Self {
            current: None,
            batch: Vec::new(),
            computing: false,
            registry: HashMap::new(),
            next_id: 0,
            batch_depth: 0,
        }
    }

    /// 分配一个新的观察者节点 ID。
    pub fn alloc_id(&mut self) -> NodeId {
        let id = NodeId(self.next_id);
        self.next_id += 1;
        id
    }
}

/// 在闭包中访问全局状态。
pub(crate) fn with_global<R>(f: impl FnOnce(&mut Global) -> R) -> R {
    G.with(|g| {
        let mut guard = g.borrow_mut();
        f(&mut guard)
    })
}

/// 依赖注册：信号读取时向当前观察者注册依赖。
///
/// 若存在当前观察者，则：
/// 1. 把观察者加入该信号的监听者集合（写入时据此通知）；
/// 2. 把最新值快照与再读取器交给观察者的 `collect`（memo 借此维护 relay map）。
pub(crate) fn register_dep(
    dep_id: NodeId,
    snapshot: Box<dyn ValBox>,
    reget: crate::get_::ReGet,
    listeners: &RefCell<Vec<NodeId>>,
) {
    with_global(|g| {
        if let Some(cur) = g.current.as_ref() {
            let cur_id = cur.node_id();
            let mut ls = listeners.borrow_mut();
            if !ls.contains(&cur_id) {
                ls.push(cur_id);
            }
            cur.collect(dep_id, snapshot, reget);
        }
    });
}
