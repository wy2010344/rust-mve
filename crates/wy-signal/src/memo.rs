//! Memo：派生节点，惰性求值 + relay map 快照比对。
//!
//! Memo 是"既是信号又是观察者"的节点：
//! - 作为**信号**：可被读取（`get()`），维护自己的监听者集合与缓存值；
//! - 作为**观察者**：实现 [`TrackDyn`]，在依赖变化时被推入批次并重新求值。
//!
//! 求值采用 relay map：计算时记录每个依赖的 snapshot + 再读取器，
//! 校验时逐一重新读取，仅当依赖快照整体相等时返回缓存，否则重算。
//! 配合全局 `state_version` 做 O(1) 短路。

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use crate::context::{bump_global_version, global_version, register_dep, with_global};
use crate::get_::{GetValue, NodeId, ReGet, TrackDyn, ValBox};

struct MemoInner<T> {
    id: NodeId,
    /// 自身强引用（需在构造完成后填充，供 `current` 收集依赖）。
    self_cell: RefCell<Option<Rc<MemoInner<T>>>>,
    /// 求值闭包（计算可能读取多个信号）。
    compute: Box<dyn Fn() -> T>,
    /// 依赖再读取器：dep_id -> reget。
    relays: RefCell<std::collections::HashMap<NodeId, ReGet>>,
    /// 最近一次计算时的依赖快照。
    last_snap: RefCell<std::collections::HashMap<NodeId, Box<dyn ValBox>>>,
    /// 监听者（依赖本 memo 的观察者）。
    listeners: RefCell<Vec<NodeId>>,
    /// 最近一次计算时的全局版本号（用于短路）。
    version: Cell<u64>,
    /// 最近一次缓存值。
    value: RefCell<Option<T>>,
    /// 是否已初始化。
    inited: Cell<bool>,
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
            listeners: RefCell::new(Vec::new()),
            version: Cell::new(0),
            value: RefCell::new(None),
            inited: Cell::new(false),
        });
        *inner.self_cell.borrow_mut() = Some(Rc::clone(&inner));
        with_global(|g| {
            g.registry.insert(id, Rc::clone(&inner) as Rc<dyn TrackDyn>);
        });
        Self { inner }
    }

    /// 读取缓存值；若全局版本已变，先校验 relay 决定是否重算。
    pub fn get_cached(&self) -> T {
        // 1. 全局版本自上次计算未变 → 缓存必然有效。
        let version = global_version();
        if self.inner.version.get() != version {
            self.validate(version);
        }

        // 2. 注册当前观察者为本 memo 的监听者（若存在）。
        let dep_id = self.inner.id;
        let inner_c = Rc::clone(&self.inner);
        let reget: ReGet = Rc::new(move || {
            // memo 重新读取：调用自身的缓存逻辑（作为依赖来源）
            Box::new(inner_c.value.borrow().clone().expect("uninit memo"))
        });
        register_dep(dep_id, self.peek_snapshot(), reget, &self.inner.listeners);

        // 3. 返回缓存值。
        self.inner.value.borrow().clone().expect("uninit memo")
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
                // 通知依赖本 memo 的观察者。
                self.notify_listeners();
            }
        } else {
            self.inner.version.set(new_version);
        }
    }

    /// 执行求值闭包，并收集依赖到 relay map。
    fn evaluate(&self) -> T {
        self.inner.relays.borrow_mut().clear();
        self.inner.last_snap.borrow_mut().clear();

        let self_rc = self
            .inner
            .self_cell
            .borrow()
            .clone()
            .expect("memo self_cell");
        with_global(|g| {
            g.current = Some(self_rc as Rc<dyn TrackDyn>);
            g.computing = true;
            let v = (self.inner.compute)();
            g.current = None;
            g.computing = false;
            v
        })
    }

    /// 依次重新读取依赖，比对快照，返回是否有任一变化。
    fn relay_changed(&self) -> bool {
        let relays: Vec<(NodeId, ReGet)> = self
            .inner
            .relays
            .borrow()
            .iter()
            .map(|(k, v)| (*k, Rc::clone(v)))
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

    /// 当前缓存快照（作为依赖来源被外层 memo 收集）。
    fn peek_snapshot(&self) -> Box<dyn ValBox> {
        Box::new(self.inner.value.borrow().clone().expect("uninit memo"))
    }

    /// 通知依赖本 memo 的观察者进入批次。
    fn notify_listeners(&self) {
        let ids: Vec<NodeId> = self.inner.listeners.borrow().clone();
        with_global(|g| {
            bump_global_version();
            for id in ids {
                if !g.batch.contains(&id) {
                    g.batch.push(id);
                }
            }
        });
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

    fn collect(&self, dep_id: NodeId, snapshot: Box<dyn ValBox>, reget: ReGet) {
        self.relays.borrow_mut().insert(dep_id, reget);
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
