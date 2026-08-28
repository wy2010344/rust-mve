//! 观察者节点：TrackEffect（副作用）与 Track（惰性值）。
//!
//! 两者实现 [`TrackDyn`]，作为 signal 的监听者存在：当被依赖的信号写入时，
//! 观察者被推入批次，并在批量 flush 阶段调用 [`TrackDyn::add_fun`] 重新评估。

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use crate::context::with_global;
use crate::get_::{GetValue, NodeId, ReGet, TrackDyn, ValBox};

// ═══════════════════════════════════════════════
// TrackEffect：副作用观察者
// ═══════════════════════════════════════════════

/// 副作用观察者内部状态（实现 [`TrackDyn`]）。
struct TrackEffectInner {
    id: NodeId,
    /// 自身强引用（构造完成后填充）。
    self_cell: RefCell<Option<Rc<TrackEffectInner>>>,
    /// 副作用闭包。运行时把自身设为当前观察者，从而收集依赖。
    effect: Box<dyn Fn()>,
    disposed: Cell<bool>,
}

/// 副作用观察者：每次依赖变化时重新执行 `effect`。
pub struct TrackEffect {
    inner: Rc<TrackEffectInner>,
}

impl TrackEffect {
    /// 创建副作用观察者。
    ///
    /// 在 `effect` 闭包内读取的信号会被自动追踪：任一信号变化都会触发
    /// `effect` 重新执行。首次创建时立即执行一次以收集初始依赖。
    pub fn new(effect: impl Fn() + 'static) -> Self {
        let id = with_global(|g| g.alloc_id());
        let inner = Rc::new(TrackEffectInner {
            id,
            self_cell: RefCell::new(None),
            effect: Box::new(effect),
            disposed: Cell::new(false),
        });
        *inner.self_cell.borrow_mut() = Some(Rc::clone(&inner));
        with_global(|g| {
            g.registry.insert(id, Rc::clone(&inner) as Rc<dyn TrackDyn>);
        });
        let eff = Self { inner };
        // 首次执行收集依赖。
        eff.add_fun_now();
        eff
    }

    /// 停用观察者：不再响应依赖变化。
    pub fn dispose(&self) {
        self.inner.disposed.set(true);
        with_global(|g| {
            g.registry.remove(&self.inner.id);
        });
    }

    fn add_fun_now(&self) {
        if self.inner.disposed.get() {
            return;
        }
        let this = self
            .inner
            .self_cell
            .borrow()
            .clone()
            .expect("effect self_cell");
        crate::context::with_current(this as Rc<dyn TrackDyn>, || {
            (self.inner.effect)();
        });
    }
}

impl TrackDyn for TrackEffectInner {
    fn node_id(&self) -> NodeId {
        self.id
    }

    fn collect(&self, _dep_id: NodeId, _snapshot: Box<dyn ValBox>, _reget: ReGet) {
        // TrackEffect 通过"成为信号监听者"来响应，无需 relay map。
    }

    fn add_fun(&self) {
        if self.disposed.get() {
            return;
        }
        let this = self.self_cell.borrow().clone().expect("effect self_cell");
        crate::context::with_current(this as Rc<dyn TrackDyn>, || {
            (self.effect)();
        });
    }
}

/// 便捷构造副作用观察者。
pub fn create_effect(effect: impl Fn() + 'static) -> TrackEffect {
    TrackEffect::new(effect)
}

// ═══════════════════════════════════════════════
// Track<T>：惰性值观察者
// ═══════════════════════════════════════════════

struct TrackInner<T> {
    id: NodeId,
    /// 自身强引用（构造完成后填充）。
    self_cell: RefCell<Option<Rc<TrackInner<T>>>>,
    /// 惰性求值闭包。
    compute: Box<dyn Fn() -> T>,
    /// 值变化时回调（可为空）。
    on_change: RefCell<Option<OnChange<T>>>,
    /// 最近一次缓存值。
    last: RefCell<Option<T>>,
    inited: Cell<bool>,
    disposed: Cell<bool>,
}

/// `on_change` 回调的类型别名，降低类型复杂度。
type OnChange<T> = Box<dyn FnMut(&T)>;

/// 惰性值观察者：重算 `compute`，当结果变化时调用回调，并可作为值节点读取。
pub struct Track<T> {
    inner: Rc<TrackInner<T>>,
}

impl<T: Clone + PartialEq + 'static> Track<T> {
    /// 创建惰性值观察者。
    pub fn new(compute: impl Fn() -> T + 'static) -> Self {
        let id = with_global(|g| g.alloc_id());
        let inner = Rc::new(TrackInner {
            id,
            self_cell: RefCell::new(None),
            compute: Box::new(compute),
            on_change: RefCell::new(None),
            last: RefCell::new(None),
            inited: Cell::new(false),
            disposed: Cell::new(false),
        });
        *inner.self_cell.borrow_mut() = Some(Rc::clone(&inner));
        with_global(|g| {
            g.registry.insert(id, Rc::clone(&inner) as Rc<dyn TrackDyn>);
        });
        let t = Self { inner };
        t.recompute(true);
        t
    }

    /// 设置值变化回调。
    pub fn on_change(&self, cb: impl FnMut(&T) + 'static) -> &Self {
        *self.inner.on_change.borrow_mut() = Some(Box::new(cb));
        self
    }

    fn recompute(&self, force_first: bool) {
        if self.inner.disposed.get() {
            return;
        }
        let this = self
            .inner
            .self_cell
            .borrow()
            .clone()
            .expect("track self_cell");
        let v =
            crate::context::with_current_track(this as Rc<dyn TrackDyn>, || (self.inner.compute)());

        let changed = if self.inner.inited.get() {
            self.inner.last.borrow().as_ref() != Some(&v)
        } else {
            force_first
        };
        if changed {
            if let Some(f) = self.inner.on_change.borrow_mut().as_mut() {
                f(&v);
            }
            *self.inner.last.borrow_mut() = Some(v);
            self.inner.inited.set(true);
        }
    }
}

impl<T: Clone + 'static> Track<T> {
    /// 读取当前（最近一次重算的）值。
    pub fn get_value(&self) -> T {
        self.inner.last.borrow().clone().expect("uninit track")
    }
}

impl<T: Clone + PartialEq + 'static> TrackDyn for TrackInner<T> {
    fn node_id(&self) -> NodeId {
        self.id
    }

    fn collect(&self, _dep_id: NodeId, _snapshot: Box<dyn ValBox>, _reget: ReGet) {}

    fn add_fun(&self) {
        if self.disposed.get() {
            return;
        }
        let this = self.self_cell.borrow().clone().expect("track self_cell");
        let v = crate::context::with_current_track(this as Rc<dyn TrackDyn>, || (self.compute)());

        let changed = if self.inited.get() {
            self.last.borrow().as_ref() != Some(&v)
        } else {
            true
        };
        if changed {
            if let Some(f) = self.on_change.borrow_mut().as_mut() {
                f(&v);
            }
            *self.last.borrow_mut() = Some(v);
            self.inited.set(true);
        }
    }
}

impl<T: Clone + 'static> GetValue<T> for Track<T> {
    fn get(&self) -> T {
        self.get_value()
    }
}

/// 便捷构造惰性值观察者。
pub fn track<T: Clone + PartialEq + 'static>(compute: impl Fn() -> T + 'static) -> Track<T> {
    Track::new(compute)
}

/// 停用节点（track 等）的占位标记，用于 API 一致性。
pub struct Disposed;
