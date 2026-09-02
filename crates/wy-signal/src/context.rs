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
    /// 当前批次队列（flush 时逐个取出执行）。
    pub batch: Vec<NodeId>,
    /// 下一批次队列（flush 期间新触发的观察者推入此处，避免无限循环）。
    pub next_batch: Vec<NodeId>,
    /// 是否处于 memo 计算中（禁止写入信号）。
    pub computing: bool,
    /// 是否正在 flush 中（新触发的观察者推入 next_batch）。
    pub flushing: bool,
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
            next_batch: Vec::new(),
            computing: false,
            flushing: false,
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

/// 设置当前执行中的观察者，运行 `f` 并返回其返回值，退出时**恢复**原 current。
///
/// 采用栈式语义：保存进入前的 current，`f` 结束后恢复它，从而支持**嵌套**
/// 观察者（如 memo 计算中再读另一 memo）。`current` 的赋值用**短生命周期借用**
/// 完成；闭包 `f`（观察者求值体）通常在内部还会读信号（触发 `register_dep`
/// 再入全局），因此不在全局借用内执行。
pub(crate) fn with_current<R, F: FnOnce() -> R>(cur: crate::get_::TrackRef, f: F) -> R {
    let saved = with_global(|g| {
        let prev = g.current.take();
        g.current = Some(cur);
        prev
    });
    let r = f();
    with_global(|g| g.current = saved);
    r
}

/// 与 [`with_current`] 相同，但命名上强调"观察者求值"；一并保存/恢复 `computing`。
pub(crate) fn with_current_track<R, F: FnOnce() -> R>(cur: crate::get_::TrackRef, f: F) -> R {
    with_current(cur, f)
}

/// 依赖注册：信号读取时向当前观察者注册依赖。
///
/// 若存在当前观察者，则：
/// 1. 把观察者加入该信号的监听者集合（写入时据此通知）；
/// 2. 把最新值快照与再读取器交给观察者的 `collect`（memo 借此维护 relay map）。
///
/// 注意：此函数可能在观察者执行（`add_fun`/evaluate）期间被调用，而后者也会
/// 短暂借用全局状态，因此这里**只做短生命周期借用**取出 current 强引用，
/// 随后在全局 borrow 之外完成 listener 注册与 collect，避免 `RefCell` 重入。
pub(crate) fn register_dep(
    dep_id: NodeId,
    snapshot: Box<dyn ValBox>,
    reget: crate::get_::ReGet,
    listeners: &RefCell<Vec<NodeId>>,
) {
    // 短借用：仅取出当前观察者的强引用。
    let current: Option<crate::get_::TrackRef> = with_global(|g| g.current.clone());

    if let Some(cur) = current {
        let cur_id = cur.node_id();
        {
            let mut ls = listeners.borrow_mut();
            if !ls.contains(&cur_id) {
                ls.push(cur_id);
            }
        }
        // 在全局 borrow 之外调用（可能内部借用 memo 的 relays RefCell）。
        cur.collect(dep_id, snapshot, reget);
    }
}
