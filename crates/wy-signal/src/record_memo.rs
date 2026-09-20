//! 记录型 memo：把"绘制命令"缓存为一份可复用的记录值。
//!
//! 对应 Kotlin `Renderer.didDraw = memo { recordPicture { draw(it) } }`：
//! 绘制闭包只在依赖信号变化时才重新执行，其余帧直接复用缓存值。
//!
//! 与 [`Memo`](crate::Memo) 的区别：
//! - 记录值 `R` 只需 `Default`（由 `compute` 写入），不要求 `Clone + PartialEq`
//!   （`Scene` 只是记录器，无法比对）；
//! - 变化判定依托依赖信号 relay 快照：**依赖变了才重录**，作为"变更"依据；
//! - 提供 `on_change` 钩子（如触发 `request_redraw()`），在依赖变化时执行。
//!
//! 关键一致性保证（配合全局 listener 清空模型）：
//! - 每次 `record()` 后把本 memo 重新登记到所有叶子依赖的监听者集合
//!   （复刻 Kotlin 短路路径的 `for ((k,_) in relays) k()`），
//!   否则叶子信号 `set` 清空监听者后，本 memo 会从批次链路中掉线；
//! - `add_fun`（依赖变化推入批次时）只负责报告"有变化"，真正的重录
//!   延后到下一帧 `record(&mut R)`（此时 runner 才有 `&mut app` 可传）。

use std::cell::{Cell, Ref, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use crate::context::global_version;
use crate::get_::{Dep, NodeId, ReGet, TrackDyn, ValBox};

struct RecordMemoInner<R> {
    id: NodeId,
    /// 自身强引用（需在构造完成后填充，供 `current` 收集依赖）。
    self_cell: RefCell<Option<Rc<RecordMemoInner<R>>>>,
    /// 依赖：dep_id -> 再读取器 + 依赖源的监听者集合。
    relays: RefCell<HashMap<NodeId, Dep>>,
    /// 最近一次录制的依赖快照（relay 比对用）。
    last_snap: RefCell<HashMap<NodeId, Box<dyn ValBox>>>,
    /// 最近一次校验的全局版本号。
    version: Cell<u64>,
    /// 是否已录制过。
    inited: Cell<bool>,
    /// 缓存的记录值（由 `record()` 的 compute 写入）。
    value: RefCell<R>,
    /// 变化钩子：依赖变化（add_fun 或 record 内部确认）时执行。
    on_change: RefCell<Vec<Rc<dyn Fn()>>>,
}

/// 记录型 memo：缓存一份由闭包写入的记录值，依赖不变时复用。
pub struct RecordMemo<R> {
    inner: Rc<RecordMemoInner<R>>,
}

impl<R: Default> Clone for RecordMemo<R> {
    fn clone(&self) -> Self {
        Self {
            inner: Rc::clone(&self.inner),
        }
    }
}

impl<R: Default + 'static> RecordMemo<R> {
    /// 创建记录型 memo。
    pub fn new() -> Self {
        let id = crate::context::with_global(|g| g.alloc_id());
        let inner = Rc::new(RecordMemoInner {
            id,
            self_cell: RefCell::new(None),
            relays: RefCell::new(HashMap::new()),
            last_snap: RefCell::new(HashMap::new()),
            version: Cell::new(0),
            inited: Cell::new(false),
            value: RefCell::new(R::default()),
            on_change: RefCell::new(Vec::new()),
        });
        *inner.self_cell.borrow_mut() = Some(Rc::clone(&inner));
        crate::context::with_global(|g| {
            g.registry.insert(id, Rc::clone(&inner) as Rc<dyn TrackDyn>);
        });
        Self { inner }
    }

    /// 注册变化钩子：由依赖变化而被推入批次（`add_fun`）时执行。
    ///
    /// 框架用它把 `request_redraw()` 接到信号变化上：绘制依赖一有变化，
    /// 立即安排下一帧，不必等待下一次 `record()` 调用。
    pub fn on_change(&self, f: Rc<dyn Fn()>) {
        self.inner.on_change.borrow_mut().push(f);
    }

    /// 记录一帧：仅在"未初始化"或"依赖已变化"时执行 `compute`，否则复用缓存值。
    ///
    /// `compute` 在追踪上下文（current = 本 memo）中执行，内部读取的信号
    /// 会被收集为依赖；漏跑时不会改写缓存值。
    ///
    /// 无论是否重录，返回前都会把本 memo 重新挂到叶子依赖上（保持批次链路）。
    pub fn record(&self, compute: impl FnOnce(&mut R)) {
        let version = global_version();
        let should_run = if !self.inner.inited.get() {
            true
        } else if self.inner.version.get() != version {
            self.relay_changed()
        } else {
            false
        };

        if should_run {
            self.evaluate(compute, version);
        } else {
            self.inner.version.set(version);
        }

        self.rehang_edges();
    }

    /// 访问缓存的记录值（不触发校验）。
    pub fn borrow(&self) -> Ref<'_, R> {
        self.inner.value.borrow()
    }

    /// 清空并重录（供创建 memo 后首次收集依赖）。
    fn evaluate(&self, compute: impl FnOnce(&mut R), version: u64) {
        self.inner.relays.borrow_mut().clear();
        self.inner.last_snap.borrow_mut().clear();

        let self_rc = self
            .inner
            .self_cell
            .borrow()
            .clone()
            .expect("record memo self_cell");
        let mut new_val = R::default();
        crate::context::with_current_track(self_rc as Rc<dyn TrackDyn>, || compute(&mut new_val));
        *self.inner.value.borrow_mut() = new_val;
        self.inner.inited.set(true);
        self.inner.version.set(version);
    }

    /// relay 快照比对：任一依赖当前值与上次录制时不同 → 需要重录。
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
                self.inner.last_snap.borrow_mut().insert(id, cur);
                return true;
            }
        }
        false
    }

    /// 把本 memo 重新登记到所有叶子依赖的监听者集合。
    ///
    /// 叶子信号 `set` 时会清空自身监听者；若本 memo 不在这里及时补挂，
    /// 下一次同信号变化将不会再把本 memo 推入批次（响应链断裂）。
    fn rehang_edges(&self) {
        let deps: Vec<Dep> = self.inner.relays.borrow().values().cloned().collect();
        for dep in deps {
            let mut ls = dep.listeners.borrow_mut();
            if !ls.contains(&self.inner.id) {
                ls.push(self.inner.id);
            }
        }
    }
}

impl<R: Default + 'static> Default for RecordMemo<R> {
    fn default() -> Self {
        Self::new()
    }
}

impl<R: Default + 'static> TrackDyn for RecordMemoInner<R> {
    fn node_id(&self) -> NodeId {
        self.id
    }

    fn collect(&self, dep: Dep, snapshot: Box<dyn ValBox>) {
        let dep_id = dep.dep_id;
        self.relays.borrow_mut().insert(dep_id, dep);
        self.last_snap.borrow_mut().insert(dep_id, snapshot);
    }

    fn add_fun(&self) {
        // 只报告"有变化"，重录延后到下一帧 record()（届时才有 &mut app）。
        //
        // 注意**不能**在这里更新 last_snap 或 version：本 memo 只会因自身
        // 依赖叶子变化而被推入批次，此时立即改 snapshot 会让下一帧 record()
        // 的 relay 比对误判为"未变化" → 复用陈旧场景。
        if !self.inited.get() {
            return;
        }
        let version = global_version();
        if self.version.get() == version {
            return;
        }
        let hooks: Vec<Rc<dyn Fn()>> = self.on_change.borrow().clone();
        for hook in hooks {
            hook();
        }
    }
}
