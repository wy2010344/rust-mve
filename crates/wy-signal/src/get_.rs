//! 类型擦除的读 / 写原语与节点抽象。
//!
//! 对应 Kotlin 层的 `GetValue<T>` / `SetValue<T>`。为了让信号、memo、观察者
//! 能被注册表与监听机制统一处理，这里提供了一套小型的类型擦除抽象。

use std::any::Any;
use std::rc::Rc;

/// 观察者节点 ID（唯一标识一个 TrackEffect / Memo）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NodeId(pub(crate) usize);

/// 只读值来源：`Signal<T>` 与 `Memo<T>` 共有的读取能力。
///
/// `get()` 必须在 memo 或 track 闭包内调用以确保依赖被当前观察者收集；
/// 否则返回值依然正确，只是不会建立响应式连接。
pub trait GetValue<T>: 'static {
    /// 读取当前值（自动向当前观察者注册依赖）。
    fn get(&self) -> T;
}

/// 可写值目标（仅为 `Signal<T>` 提供）。
pub trait SetValue<T> {
    /// 写入新值，若与旧值不同则触发依赖更新。
    fn set(&self, value: T);
}

/// 用于 relay map 快照比对的类型擦除值。
///
/// 任意 `T: Clone + PartialEq + 'static` 都可被装箱为 [`ValBox`]，
/// 从而让 memo 在不了解具体依赖类型的情况下做 `eq` 比较。
pub trait ValBox: Any {
    /// 与另一个擦除值比较是否相等（类型不符视为不相等）。
    fn eq(&self, other: &dyn ValBox) -> bool;
    /// 类型向下转换辅助。
    fn as_any(&self) -> &dyn Any;
}

impl<T: Clone + PartialEq + 'static> ValBox for T {
    fn eq(&self, other: &dyn ValBox) -> bool {
        other
            .as_any()
            .downcast_ref::<T>()
            .map(|o| self == o)
            .unwrap_or(false)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// 依赖再读取器：memo 用它重新读取某个依赖的当前值做快照比对。
///
/// 闭包捕获依赖自身，读取时返回类型擦除的值。
///
/// `Rc<dyn Fn>` 可克隆（仅递增引用计数），便于在 `collect` 与 relay map 中复用。
pub(crate) type ReGet = Rc<dyn Fn() -> Box<dyn ValBox>>;

/// 观察者节点的强引用句柄。
pub(crate) type TrackRef = Rc<dyn TrackDyn>;

/// 一个依赖的完整描述：再读取器 + 依赖源的监听者集合。
///
/// 观察者（memo / draw memo）短路时可以拿着 `listeners` 直接重挂叶子边，
/// 无需重新执行依赖的 `get()`（复刻 Kotlin memo 短路路径的 `for ((k,_) in relays) k()`）。
#[derive(Clone)]
pub struct Dep {
    /// 依赖源节点 ID。
    pub dep_id: NodeId,
    /// 值再读取器：重新读取依赖当前值做快照比对。
    pub reget: ReGet,
    /// 依赖源的监听者集合：重挂叶子边时往里面登记当前观察者
    /// （由 wy-engine 的 draw memo 在每帧短路时消费）。
    pub listeners: Rc<std::cell::RefCell<Vec<NodeId>>>,
}

/// 观察者节点（TrackEffect / Memo 共同实现）。
///
/// 信号被写入时，会查询注册表并通过此 trait 把观察者推入批次；
/// memo 计算时还会通过 [`TrackDyn::collect`] 收集依赖快照。
pub trait TrackDyn: 'static {
    /// 返回观察者自身的节点 ID。
    fn node_id(&self) -> NodeId;

    /// 收集一个依赖（memo 用它记录 relay map；普通跟踪忽略）。
    fn collect(&self, dep: Dep, snapshot: Box<dyn ValBox>);

    /// 是否收集依赖快照（memo / draw memo 需要；effect 之类的纯监听者不需要）。
    ///
    /// 返回 `false` 时，[`register_dep`](crate::context::register_dep) 只登记监听，
    /// 不构建快照与再读取器，节省热路径分配。
    fn wants_dep_snapshot(&self) -> bool {
        true
    }

    /// 重新评估观察者（批量 flush 时由批次调度器调用）。
    fn add_fun(&self);
}
