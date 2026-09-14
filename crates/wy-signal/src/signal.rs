//! 叶子信号：存储值 + 订阅者集合。
//!
//! - `get()`：读取值，自动向当前观察者注册依赖。
//! - `set()`：比对旧值，仅有实际变化才把依赖它的观察者推入批次并递增全局版本号。

use std::cell::{Cell, RefCell};
use std::fmt;
use std::rc::Rc;

use crate::context::{bump_global_version, register_dep, with_global};
use crate::get_::{GetValue, NodeId, SetValue};

/// `should_change` 回调：`(old, new) -> bool`，返回 true 表示值变化。
type ShouldChange<T> = Box<dyn Fn(&T, &T) -> bool>;

/// 信号内部状态。
struct SignalInner<T> {
    /// 当前存储值。
    value: RefCell<T>,
    /// 监听者（观察者节点 ID），依赖 PN 方向：本信号 → 观察者。
    listeners: RefCell<Vec<NodeId>>,
    /// 本信号在注册表中的唯一 ID。
    id: NodeId,
    /// 自定义变化判定（`false` 表示值相同不触发通知）。
    should_change: ShouldChange<T>,
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

impl<T: fmt::Debug> fmt::Debug for Signal<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Signal")
            .field("value", &self.inner.value.borrow())
            .finish()
    }
}

impl<T: PartialEq> Signal<T> {
    /// 创建一个带初始值的信号，使用默认比较（`PartialEq`）。
    ///
    /// ```ignore
    /// let count = Signal::new(0);
    /// ```
    pub fn new(initial: T) -> Self {
        Self::new_with_comparator(initial, |a, b| a != b)
    }
}

impl<T> Signal<T> {
    /// 创建带自定义变化判定的信号。
    ///
    /// `should_change(old, new)` 返回 `true` 表示值变化，需要通知观察者；
    /// 返回 `false` 表示值"相同"，不触发通知。
    ///
    /// 用法：
    /// - `|_, _| true` — 始终视为变化（强制通知）
    /// - `|_, _| false` — 永不变化（阻止所有通知）
    /// - `|old, new| old != new` — 等价于默认 `PartialEq`
    pub fn new_with_comparator(
        initial: T,
        should_change: impl Fn(&T, &T) -> bool + 'static,
    ) -> Self {
        let id = with_global(|g| g.alloc_id());
        Self {
            inner: Rc::new(SignalInner {
                value: RefCell::new(initial),
                listeners: RefCell::new(Vec::new()),
                id,
                should_change: Box::new(should_change),
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
    fn set_impl(&self, value: T) {
        let changed = {
            let mut cell = self.inner.value.borrow_mut();
            if (self.inner.should_change)(&cell, &value) {
                *cell = value;
                true
            } else {
                false
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
            // flush 期间推入 next_batch，避免修改正在迭代的 batch。
            let listeners = self.inner.listeners.borrow();
            let target = if g.flushing {
                &mut g.next_batch
            } else {
                &mut g.batch
            };
            for &id in listeners.iter() {
                if !target.contains(&id) {
                    target.push(id);
                }
            }
            drop(listeners);

            // 批次深度为 0（未在显式 batch 内）时，本批完成后立即 flush。
            if g.batch_depth == 0 && !g.batch.is_empty() && !g.flushing {
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

impl<T: Clone + 'static> SetValue<T> for Signal<T> {
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
pub fn create_signal<T: PartialEq>(value: T) -> Signal<T> {
    Signal::new(value)
}

/// 便捷构造：等价于 `Signal::new_with_comparator`。
pub fn create_signal_with_comparator<T>(
    value: T,
    should_change: impl Fn(&T, &T) -> bool + 'static,
) -> Signal<T> {
    Signal::new_with_comparator(value, should_change)
}

// ═══════════════════════════════════════════════
// LateSignal：写一次句柄
// ═══════════════════════════════════════════════

/// 写句柄：唯一可写入 [`LateSignal`] 的入口。
///
/// 通过 [`LateSignal::get_only_set`] 获取，只能获取一次。
/// 后续调用返回 `None`，确保写权限唯一。
pub struct WriteHandle<T> {
    signal: Signal<T>,
}

impl<T: Clone + 'static> WriteHandle<T> {
    /// 写入新值。
    pub fn set(&self, value: T) {
        self.signal.set_impl(value);
    }
}

impl<T: Clone + fmt::Debug + 'static> fmt::Debug for WriteHandle<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WriteHandle")
            .field("signal", &self.signal)
            .finish()
    }
}

/// 延迟初始化信号：写句柄只能获取一次。
///
/// 对齐 Kotlin `createLateSignal`：创建时持有初始值，通过 `get_only_set()`
/// 获取唯一的写句柄。适合将写权限转移给特定所有者的场景。
///
/// ```ignore
/// let late = LateSignal::new(0);
/// let writer = late.get_only_set().expect("第一次获取");
/// assert!(late.get_only_set().is_none(), "第二次获取失败");
/// writer.set(42);
/// assert_eq!(late.get(), 42);
/// ```
pub struct LateSignal<T> {
    signal: Signal<T>,
    /// 写句柄是否已被获取。
    taken: Cell<bool>,
}

impl<T> Clone for LateSignal<T> {
    fn clone(&self) -> Self {
        Self {
            signal: self.signal.clone(),
            taken: Cell::new(self.taken.get()),
        }
    }
}

impl<T: PartialEq> LateSignal<T> {
    /// 创建延迟信号，初始值为 `initial`。
    pub fn new(initial: T) -> Self {
        Self {
            signal: Signal::new(initial),
            taken: Cell::new(false),
        }
    }
}

impl<T> LateSignal<T> {
    /// 创建带自定义比较器的延迟信号。
    pub fn new_with_comparator(
        initial: T,
        should_change: impl Fn(&T, &T) -> bool + 'static,
    ) -> Self {
        Self {
            signal: Signal::new_with_comparator(initial, should_change),
            taken: Cell::new(false),
        }
    }

    /// 获取唯一的写句柄。首次调用返回 `Some(WriteHandle)`，后续返回 `None`。
    pub fn get_only_set(&self) -> Option<WriteHandle<T>> {
        if self.taken.get() {
            None
        } else {
            self.taken.set(true);
            Some(WriteHandle {
                signal: self.signal.clone(),
            })
        }
    }
}

impl<T: Clone + PartialEq + 'static> GetValue<T> for LateSignal<T> {
    fn get(&self) -> T {
        self.signal.get_clone()
    }
}

impl<T: Clone + fmt::Debug + 'static> fmt::Debug for LateSignal<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LateSignal")
            .field("signal", &self.signal)
            .finish()
    }
}

/// 便捷构造延迟信号。
pub fn create_late_signal<T: PartialEq>(initial: T) -> LateSignal<T> {
    LateSignal::new(initial)
}
