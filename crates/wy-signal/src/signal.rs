//! 叶子信号：存储值 + 订阅者集合。
//!
//! - `get()`：读取值，自动向当前观察者注册依赖。
//! - `set()`：比对旧值，仅有实际变化才把依赖它的观察者推入批次并递增全局版本号。

use std::cell::RefCell;
use std::rc::Rc;

use crate::context::{bump_global_version, register_dep, with_global};
use crate::get_::{GetValue, NodeId, SetValue};

/// 信号内部状态。
struct SignalInner<T> {
    /// 当前存储值。
    value: RefCell<T>,
    /// 监听者（观察者节点 ID），依赖 PN 方向：本信号 → 观察者。
    listeners: RefCell<Vec<NodeId>>,
    /// 本信号在注册表中的唯一 ID。
    id: NodeId,
}

/// 叶子信号：持有值 + 订阅集合。
///
/// `Signal<T>` 是 `Rc` 包装的共享句柄，可被克隆并作为 Widget 字段持有。
/// 泛型 `T` 需实现 `Clone`（`get()` 返回克隆值）即可，无需 `PartialEq`
/// 之外约束——默认用 `PartialEq` 做变化比对。
pub struct Signal<T> {
    inner: Rc<SignalInner<T>>,
}

impl<T> Clone for Signal<T> {
    fn clone(&self) -> Self {
        Self {
            inner: Rc::clone(&self.inner),
        }
    }
}

impl<T> Signal<T> {
    /// 创建一个带初始值的信号。
    ///
    /// ```ignore
    /// let count = Signal::new(0);
    /// ```
    pub fn new(initial: T) -> Self {
        let id = with_global(|g| g.alloc_id());
        Self {
            inner: Rc::new(SignalInner {
                value: RefCell::new(initial),
                listeners: RefCell::new(Vec::new()),
                id,
            }),
        }
    }

    /// 读取当前值。
    ///
    /// 若在 memo / track 闭包内调用，会自动把当前观察者注册为依赖。
    pub fn get_clone(&self) -> T
    where
        T: Clone + PartialEq + 'static,
    {
        let snapshot = self.inner.value.borrow().clone();
        let dep_id = self.inner.id;

        // 再读取器：捕获信号自身，供 memo 重查快照用。
        let inner_c = Rc::clone(&self.inner);
        let reget: crate::get_::ReGet = Rc::new(move || Box::new(inner_c.value.borrow().clone()));

        let listeners = &self.inner.listeners;
        register_dep(dep_id, Box::new(snapshot.clone()), reget, listeners);
        snapshot
    }

    /// 写入新值（通过 [`SetValue`] trait 实现）。
    fn set_impl(&self, value: T)
    where
        T: PartialEq,
    {
        let changed = {
            let mut cell = self.inner.value.borrow_mut();
            if *cell == value {
                false
            } else {
                *cell = value;
                true
            }
        };

        if !changed {
            return;
        }

        // 禁止在 memo 计算期间写入（会破坏依赖一致性）。
        let mut should_flush = false;
        with_global(|g| {
            assert!(!g.computing, "信号系统：不允许在 memo 计算期间写入信号");

            // 递增全局版本号，供 memo 短路。
            bump_global_version();

            // 把依赖本信号的观察者推入批次队列。
            let listeners = self.inner.listeners.borrow();
            for &id in listeners.iter() {
                if !g.batch.contains(&id) {
                    g.batch.push(id);
                }
            }
            drop(listeners);

            // 批次深度为 0（未在显式 batch 内）时，本批完成后立即 flush。
            if g.batch_depth == 0 && !g.batch.is_empty() {
                should_flush = true;
            }
        });

        if should_flush {
            crate::batch::flush();
        }
    }
}

impl<T: Clone + PartialEq + 'static> GetValue<T> for Signal<T> {
    fn get(&self) -> T {
        self.get_clone()
    }
}

impl<T: PartialEq + 'static> SetValue<T> for Signal<T> {
    fn set(&self, value: T) {
        self.set_impl(value);
    }
}

impl<T> Signal<T> {
    /// 本信号在注册表中的节点 ID（主要用于调试）。
    pub fn id(&self) -> NodeId {
        self.inner.id
    }
}

/// 便捷构造：等价于 `Signal::new`。
pub fn create_signal<T>(value: T) -> Signal<T> {
    Signal::new(value)
}
