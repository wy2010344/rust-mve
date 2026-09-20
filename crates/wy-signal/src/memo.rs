//! Memo：派生节点，惰性求值 + relay map 快照比对。
//!
//! Memo 是"既是信号又是观察者"的节点：
//! - 作为**信号**：可被读取（`get()`），维护自己的监听者集合与缓存值；
//! - 作为**观察者**：实现 [`TrackDyn`]，在依赖变化时被推入批次并重新求值。
//!
//! 求值采用 relay map：计算时记录每个依赖的 snapshot + 再读取器，
//! 校验时逐一重新读取，仅当依赖快照整体相等时返回缓存，否则重算。
//! 配合全局 `state_version` 做 O(1) 短路。
//!
//! 短路路径（复刻 Kotlin `Memo.invoke()`）：当有**新的**观察者读取本 memo 时，
//! 把该观察者重新登记到所有叶子依赖的监听者集合上（Kotlin：`for ((k,_) in relays) k()`），
//! 保证叶子信号的 `listeners` 始终反映真实依赖，不积累 stale 边。

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use crate::context::{
    bump_global_version, check_memo_cycle, global_version, pop_eval_stack, push_eval_stack,
    with_current_track, with_global,
};
use crate::get_::{Dep, GetValue, NodeId, ReGet, TrackDyn, ValBox};

/// 值变化回调类型。
type AfterCallback<T> = Box<dyn Fn(&T)>;

struct MemoInner<T> {
    id: NodeId,
    /// 自身强引用（需在构造完成后填充，供 `current` 收集依赖）。
    self_cell: RefCell<Option<Rc<MemoInner<T>>>>,
    /// 求值闭包（计算可能读取多个信号）。
    compute: Box<dyn Fn() -> T>,
    /// 依赖：dep_id -> 再读取器 + 依赖源的监听者集合。
    relays: RefCell<std::collections::HashMap<NodeId, Dep>>,
    /// 最近一次计算时的依赖快照。
    last_snap: RefCell<std::collections::HashMap<NodeId, Box<dyn ValBox>>>,
    /// 监听者（依赖本 memo 的观察者）。
    listeners: Rc<RefCell<Vec<NodeId>>>,
    /// 最近一次计算时的全局版本号（用于短路）。
    version: Cell<u64>,
    /// 最近一次缓存值。
    value: RefCell<Option<T>>,
    /// 是否已初始化。
    inited: Cell<bool>,
    /// 值变化回调：Memo 值变化时触发（含首次计算）。
    afters: RefCell<Vec<AfterCallback<T>>>,
}

/// 派生值节点。
pub struct Memo<T> {
    inner: Rc<MemoInner<T>>,
}

impl<T> Clone for Memo<T> {
    fn clone(&self) -> Self {
        Self {
            inner: Rc::clone(&self.inner),
        }
    }
}

impl<T: Clone + PartialEq + 'static> Memo<T> {
    /// 创建 memo：`compute` 在首次读取或依赖变化时执行。
    ///
    /// `compute` 内可读取任意 [`Signal`](crate::Signal)，依赖会被自动收集。
    pub fn new(compute: impl Fn() -> T + 'static) -> Self {
        let id = with_global(|g| g.alloc_id());
        let inner = Rc::new(MemoInner {
            id,
            self_cell: RefCell::new(None),
            compute: Box::new(compute),
            relays: RefCell::new(std::collections::HashMap::new()),
            last_snap: RefCell::new(std::collections::HashMap::new()),
            listeners: Rc::new(RefCell::new(Vec::new())),
            version: Cell::new(0),
            value: RefCell::new(None),
            inited: Cell::new(false),
            afters: RefCell::new(Vec::new()),
        });
        *inner.self_cell.borrow_mut() = Some(Rc::clone(&inner));
        with_global(|g| {
            g.registry.insert(id, Rc::clone(&inner) as Rc<dyn TrackDyn>);
        });
        Self { inner }
    }

    /// 读取缓存值；若全局版本已变，先校验 relay 决定是否重算。
    pub fn get_cached(&self) -> T {
        // 1. 若尚未初始化，或全局版本自上次计算已变，则需校验/重算。
        //    首次读取（inited=false）时**不能**因版本号相同而短路，必须求值。
        let version = global_version();
        if !self.inner.inited.get() || self.inner.version.get() != version {
            self.validate(version);
        }

        // 2. 注册当前观察者为本 memo 的监听者（作为依赖来源）。
        //    叶子边的重挂无需在这里做：本 memo 在**重算**时会重新读取叶子
        //    并把自己登记回叶子的监听集合；观察者则在此处登记为本 memo 的
        //    监听者，叶子变化 → memo 重算 → 通知观察者，链路即可延续。
        let dep_id = self.inner.id;
        let inner_c = Rc::clone(&self.inner);
        let listeners = Rc::clone(&self.inner.listeners);
        crate::context::register_dep(dep_id, listeners, move || {
            let reget_inner = Rc::clone(&inner_c);
            let reget: ReGet = Rc::new(move || {
                // 重新读取时先保证值新鲜（复刻 Kotlin relay `get()` = 完整 invoke）。
                let v = global_version();
                if !reget_inner.inited.get() || reget_inner.version.get() != v {
                    Memo {
                        inner: Rc::clone(&reget_inner),
                    }
                    .validate(v);
                }
                Box::new(reget_inner.value.borrow().clone().expect("uninit memo"))
            });
            (
                Box::new(inner_c.value.borrow().clone().expect("uninit memo")),
                reget,
            )
        });

        // 4. 返回缓存值。
        self.inner.value.borrow().clone().expect("uninit memo")
    }

    /// 注册值变化回调：memo 值变化时（含首次计算）调用 `f(new_value)`。
    ///
    /// 回调在 memo 重算完成后、通知下游观察者之前执行。
    pub fn after(&self, f: impl Fn(&T) + 'static) {
        self.inner.afters.borrow_mut().push(Box::new(f));
    }

    /// 校验并可能重算：relay 快照比对。
    fn validate(&self, new_version: u64) {
        let inited = self.inner.inited.get();
        let should_recompute = if !inited { true } else { self.relay_changed() };

        if should_recompute {
            let v = self.evaluate();
            let changed = if inited {
                self.inner.value.borrow().as_ref() != Some(&v)
            } else {
                true
            };
            *self.inner.value.borrow_mut() = Some(v);
            self.inner.inited.set(true);
            self.inner.version.set(new_version);

            if changed {
                // 执行 afters 回调（在通知下游观察者之前）。
                let v_ref = self.inner.value.borrow();
                let v = v_ref.as_ref().unwrap();
                for after in self.inner.afters.borrow().iter() {
                    after(v);
                }
                drop(v_ref);

                // 通知依赖本 memo 的观察者。
                self.notify_listeners();
            }
        } else {
            self.inner.version.set(new_version);
        }
    }

    /// 执行求值闭包，并收集依赖到 relay map。
    fn evaluate(&self) -> T {
        // 循环检测：若本 memo 已在求值栈中，说明存在循环依赖。
        if check_memo_cycle(self.inner.id) {
            panic!(
                "信号系统：检测到循环 memo 依赖（memo id={}）",
                self.inner.id.0
            );
        }

        self.inner.relays.borrow_mut().clear();
        self.inner.last_snap.borrow_mut().clear();

        let self_rc = self
            .inner
            .self_cell
            .borrow()
            .clone()
            .expect("memo self_cell");
        // 标记"计算中"（禁止写入信号），以栈式保存/恢复 computing。
        let prev_computing = with_global(|g| {
            let prev = g.computing;
            g.computing = true;
            prev
        });
        push_eval_stack(self.inner.id);
        let v = with_current_track(self_rc as Rc<dyn TrackDyn>, || (self.inner.compute)());
        pop_eval_stack();
        with_global(|g| g.computing = prev_computing);
        v
    }

    /// 依次重新读取依赖，比对快照，返回是否有任一变化。
    fn relay_changed(&self) -> bool {
        let relays: Vec<(NodeId, ReGet)> = self
            .inner
            .relays
            .borrow()
            .iter()
            .map(|(id, dep)| (*id, dep.reget.clone()))
            .collect();
        for (id, reget) in relays {
            let cur = reget();
            let changed = match self.inner.last_snap.borrow().get(&id) {
                Some(old) => !cur.eq(old.as_ref()),
                None => true,
            };
            if changed {
                // 更新为最新快照，避免重复触发。
                self.inner.last_snap.borrow_mut().insert(id, cur);
                return true;
            }
        }
        false
    }

    /// 通知依赖本 memo 的观察者进入批次，推送后清空监听者集合。
    ///
    /// 依赖关系由每次读取重新登记，避免幽灵通知。
    fn notify_listeners(&self) {
        let ids: Vec<NodeId> = self.inner.listeners.borrow().clone();
        with_global(|g| {
            bump_global_version();
            let target = if g.flushing {
                &mut g.next_batch
            } else {
                &mut g.batch
            };
            for id in ids {
                if !target.contains(&id) {
                    target.push(id);
                }
            }
        });
        self.inner.listeners.borrow_mut().clear();
    }
}

impl<T: Clone + PartialEq + 'static> GetValue<T> for Memo<T> {
    fn get(&self) -> T {
        self.get_cached()
    }
}

impl<T: Clone + PartialEq + 'static> TrackDyn for MemoInner<T> {
    fn node_id(&self) -> NodeId {
        self.id
    }

    fn collect(&self, dep: Dep, snapshot: Box<dyn ValBox>) {
        let dep_id = dep.dep_id;
        self.relays.borrow_mut().insert(dep_id, dep);
        self.last_snap.borrow_mut().insert(dep_id, snapshot);
    }

    fn add_fun(&self) {
        // 通过 self_cell 取得 Memo 包装，复用其一致性逻辑。
        let memo = Memo {
            inner: self.self_cell.borrow().clone().expect("memo self_cell"),
        };
        let version = global_version();
        if memo.inner.version.get() == version {
            return;
        }
        memo.validate(version);
    }
}

/// 便捷构造：等价于 `Memo::new`。
pub fn create_memo<T: Clone + PartialEq + 'static>(compute: impl Fn() -> T + 'static) -> Memo<T> {
    Memo::new(compute)
}
