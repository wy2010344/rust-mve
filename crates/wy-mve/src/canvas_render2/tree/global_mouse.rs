//! 全局鼠标注册表（TS `tree/globalMouse.ts` 的 1:1 复刻）。
//!
//! 这里**没有任何宿主 API**：只是一组回调集合 + 广播入口。
//! 宿主（Renderer / ooc 的宿主）在收到真实事件时调
//! [`GlobalMouseRegistry2::emit_down`] / [`GlobalMouseRegistry2::emit_move`] /
//! [`GlobalMouseRegistry2::emit_up`]，节点侧则经 context 拿到 [`GlobalMouse2`]
//! 注册回调——典型用途是拖拽：指针移出节点甚至移出画布后仍要持续收到 move/up。
//!
//! 滚轮**不在**这里：它沿命中链派发（`mouseWheel` 消息），这样才能"内层先滚、
//! 滚到底再传给外层"并被节点 `stopPropagation` 拦住。
//!
//! ## 与 TS 的差别（仅两处，均为宿主模型差异）
//!
//! 1. TS 用 `mve-core` 的 `createContext` 注入注册表；Rust 的
//!    [`crate::context::NodeContext`] 已有 `provide` / `consume`，按同一语义使用
//!    （[`drag_mouse`] 的签名与 TS `dragMouse(ctx, change)` 一致）。
//! 2. TS 的回调集合是 `Set<T>`（按引用去重）；Rust 用 id 表 + `Vec`，语义等价。
//!    广播前先取快照再回调，避免回调内注销触发重复借用。

use std::cell::RefCell;
use std::rc::Rc;

use crate::context::NodeContext;

type MoveCb = Rc<dyn Fn(f32, f32)>;
type UpCb = Rc<dyn Fn()>;
/// 注销句柄槽：`drag_mouse_on` 的两阶段初始化用（句柄注册成功后才回填）。
type OffSlot = Rc<RefCell<Option<Box<dyn Fn()>>>>;

/// 全局鼠标能力对象（TS `GlobalMouse2`）。
///
/// 节点侧经 context 拿到它；三个注册方法都返回注销句柄。
#[derive(Clone)]
pub struct GlobalMouse2 {
    inner: Rc<RefCell<Registry>>,
}

/// 注册表本体（TS 闭包内的三个 `Set`）。
#[derive(Default)]
struct Registry {
    next_id: u64,
    down: Vec<(u64, MoveCb)>,
    mv: Vec<(u64, MoveCb)>,
    up: Vec<(u64, UpCb)>,
}

impl GlobalMouse2 {
    /// 注册全局按下（TS `registerMouseDown`）：坐标相对画布，不依赖命中测试。
    pub fn register_mouse_down(&self, callback: MoveCb) -> impl Fn() {
        let inner = Rc::clone(&self.inner);
        let id = inner.borrow_mut().add_down(callback);
        move || inner.borrow_mut().remove(id)
    }

    /// 注册全局移动（TS `registerMouseMove`）。
    pub fn register_mouse_move(&self, callback: MoveCb) -> impl Fn() {
        let inner = Rc::clone(&self.inner);
        let id = inner.borrow_mut().add_move(callback);
        move || inner.borrow_mut().remove(id)
    }

    /// 注册全局抬起（TS `registerMouseUp`）。
    pub fn register_mouse_up(&self, callback: UpCb) -> impl Fn() {
        let inner = Rc::clone(&self.inner);
        let id = inner.borrow_mut().add_up(callback);
        move || inner.borrow_mut().remove(id)
    }
}

impl Registry {
    fn add_down(&mut self, f: MoveCb) -> u64 {
        let id = self.next_id();
        self.down.push((id, f));
        id
    }
    fn add_move(&mut self, f: MoveCb) -> u64 {
        let id = self.next_id();
        self.mv.push((id, f));
        id
    }
    fn add_up(&mut self, f: UpCb) -> u64 {
        let id = self.next_id();
        self.up.push((id, f));
        id
    }
    fn next_id(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }
    fn remove(&mut self, id: u64) {
        self.down.retain(|(i, _)| *i != id);
        self.mv.retain(|(i, _)| *i != id);
        self.up.retain(|(i, _)| *i != id);
    }
    fn snapshot_down(&self) -> Vec<MoveCb> {
        self.down.iter().map(|(_, f)| Rc::clone(f)).collect()
    }
    fn snapshot_move(&self) -> Vec<MoveCb> {
        self.mv.iter().map(|(_, f)| Rc::clone(f)).collect()
    }
    fn snapshot_up(&self) -> Vec<UpCb> {
        self.up.iter().map(|(_, f)| Rc::clone(f)).collect()
    }
}

/// 注册表本体 + 宿主侧广播入口（TS `GlobalMouseRegistry2`）。
pub struct GlobalMouseRegistry2 {
    /// 提供给节点树的能力对象（经 context 注入）。
    pub api: GlobalMouse2,
    inner: Rc<RefCell<Registry>>,
}

impl GlobalMouseRegistry2 {
    /// 宿主广播：全局按下。
    pub fn emit_down(&self, x: f32, y: f32) {
        // 先取快照再回调：`for f in self.inner.borrow().snapshot()` 会把 `Ref`
        // 临时量活到整个循环结束，回调里注销就会撞上重复借用。
        let cbs = self.inner.borrow().snapshot_down();
        for f in cbs {
            f(x, y);
        }
    }

    /// 宿主广播：全局移动。
    pub fn emit_move(&self, x: f32, y: f32) {
        let cbs = self.inner.borrow().snapshot_move();
        for f in cbs {
            f(x, y);
        }
    }

    /// 宿主广播：全局抬起。
    pub fn emit_up(&self) {
        let cbs = self.inner.borrow().snapshot_up();
        for f in cbs {
            f();
        }
    }

    /// 销毁时清空全部注册（避免残留闭包）。
    pub fn clear(&self) {
        let mut r = self.inner.borrow_mut();
        r.down.clear();
        r.mv.clear();
        r.up.clear();
    }
}

/// 建一个全局鼠标注册表（TS `createGlobalMouse2()`）。
#[must_use]
pub fn create_global_mouse2() -> GlobalMouseRegistry2 {
    let inner = Rc::new(RefCell::new(Registry::new()));
    GlobalMouseRegistry2 {
        api: GlobalMouse2 {
            inner: Rc::clone(&inner),
        },
        inner,
    }
}

impl Registry {
    fn new() -> Self {
        Self {
            next_id: 0,
            ..Self::default()
        }
    }
}

/// 通用拖拽助手（TS `dragMouse`）。
///
/// 起手后接管全局 move，松手时以最后坐标再回调一次并自动摘除注册，避免每次拖拽
/// 在注册表里残留闭包。返回一个提前终止拖拽的函数。
///
/// 与 TS 同签名：从 context 里 consume 出 [`GlobalMouse2`]；取不到（宿主没注入）
/// 时返回空函数，与 TS `if (!g) return emptyFun` 一致。
#[must_use]
pub fn drag_mouse(ctx: &NodeContext, change: impl Fn(f32, f32) + 'static) -> Box<dyn Fn()> {
    match ctx.consume::<GlobalMouse2>() {
        Some(g) => drag_mouse_on(g, change),
        None => Box::new(|| {}),
    }
}

/// [`drag_mouse`] 的本体：直接喂一个 [`GlobalMouse2`]。
///
/// TS 没有这一条（它只能经 context 取），Rust 侧需要它是因为宿主目前把默认
/// 注册表挂在线程上（见 `renderer.rs`），拿不到 context 的场景（滚动条拖拽）要走它。
#[must_use]
pub fn drag_mouse_on(g: &GlobalMouse2, change: impl Fn(f32, f32) + 'static) -> Box<dyn Fn()> {
    let change = Rc::new(change);
    // TS 用 `let lastX = NaN` 判"一次 move 都没收到过"
    let last = Rc::new(RefCell::new((f32::NAN, f32::NAN)));

    // 两阶段初始化：move/up 的回调要互相摘除、回调自身又要被返回值摘除，而句柄
    // 在注册成功之后才存在——TS 闭包里直接引用 `offMove` / `offUp` 变量即可，
    // Rust 得用 slot 回填。
    let off_move: OffSlot = Rc::new(RefCell::new(None));
    let off_up: OffSlot = Rc::new(RefCell::new(None));

    let last_mv = Rc::clone(&last);
    let change_mv = Rc::clone(&change);
    let mv = g.register_mouse_move(Rc::new(move |x, y| {
        *last_mv.borrow_mut() = (x, y);
        change_mv(x, y);
    }));
    *off_move.borrow_mut() = Some(Box::new(mv));

    let mv_for_up = Rc::clone(&off_move);
    let up_for_up = Rc::clone(&off_up);
    let change_up = Rc::clone(&change);
    let up = g.register_mouse_up(Rc::new(move || {
        // 松手时同时摘掉两个注册（TS `offMove(); offUp();`），再以最后坐标回调一次
        if let Some(off) = mv_for_up.borrow().as_ref() {
            off();
        }
        if let Some(off) = up_for_up.borrow().as_ref() {
            off();
        }
        let (lx, ly) = *last.borrow();
        if !lx.is_nan() {
            change_up(lx, ly);
        }
    }));
    *off_up.borrow_mut() = Some(Box::new(up));

    // TS `return () => { offMove(); offUp(); }`
    Box::new(move || {
        if let Some(off) = off_move.borrow().as_ref() {
            off();
        }
        if let Some(off) = off_up.borrow().as_ref() {
            off();
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx_with(g: GlobalMouse2) -> NodeContext {
        let mut ctx = NodeContext::new();
        ctx.provide(g);
        ctx
    }

    #[test]
    fn registry_broadcasts_and_unregisters() {
        let r = create_global_mouse2();
        let seen = Rc::new(RefCell::new(Vec::new()));
        let s = Rc::clone(&seen);
        let off = r
            .api
            .register_mouse_down(Rc::new(move |x, y| s.borrow_mut().push((x, y))));

        r.emit_down(1.0, 2.0);
        off();
        r.emit_down(3.0, 4.0);
        assert_eq!(*seen.borrow(), vec![(1.0, 2.0)]);
    }

    #[test]
    fn clear_drops_everything() {
        let r = create_global_mouse2();
        let n = Rc::new(RefCell::new(0usize));
        let a = Rc::clone(&n);
        let _off = r
            .api
            .register_mouse_up(Rc::new(move || *a.borrow_mut() += 1));
        r.emit_up();
        assert_eq!(*n.borrow(), 1);
        r.clear();
        r.emit_up();
        assert_eq!(*n.borrow(), 1);
    }

    #[test]
    fn drag_mouse_replays_last_position_on_up() {
        let r = create_global_mouse2();
        let ctx = ctx_with(r.api.clone());
        let seen = Rc::new(RefCell::new(Vec::new()));
        let s = Rc::clone(&seen);
        let stop = drag_mouse(&ctx, move |x, y| s.borrow_mut().push((x, y)));

        r.emit_move(10.0, 20.0);
        r.emit_move(30.0, 40.0);
        r.emit_up();
        // 两次 move + 松手时以最后坐标再回调一次
        assert_eq!(
            *seen.borrow(),
            vec![(10.0, 20.0), (30.0, 40.0), (30.0, 40.0)]
        );

        // 松手后已自动摘除：后续 move 不再回调
        r.emit_move(50.0, 60.0);
        assert_eq!(seen.borrow().len(), 3);
        stop();
    }

    #[test]
    fn drag_mouse_without_move_does_not_replay() {
        let r = create_global_mouse2();
        let ctx = ctx_with(r.api.clone());
        let seen = Rc::new(RefCell::new(Vec::new()));
        let s = Rc::clone(&seen);
        let _stop = drag_mouse(&ctx, move |x, y| s.borrow_mut().push((x, y)));
        r.emit_up();
        // TS：`lastX` 仍是 NaN，不回调
        assert!(seen.borrow().is_empty());
    }

    #[test]
    fn drag_mouse_without_context_is_noop() {
        let ctx = NodeContext::new();
        let seen = Rc::new(RefCell::new(0usize));
        let s = Rc::clone(&seen);
        let _stop = drag_mouse(&ctx, move |_, _| *s.borrow_mut() += 1);
        let r = create_global_mouse2();
        r.emit_move(1.0, 1.0);
        r.emit_up();
        assert_eq!(*seen.borrow(), 0);
    }

    #[test]
    fn early_stop_unregisters_both() {
        let r = create_global_mouse2();
        let ctx = ctx_with(r.api.clone());
        let seen = Rc::new(RefCell::new(0usize));
        let s = Rc::clone(&seen);
        let stop = drag_mouse(&ctx, move |_, _| *s.borrow_mut() += 1);
        stop();
        r.emit_move(1.0, 1.0);
        r.emit_up();
        assert_eq!(*seen.borrow(), 0);
    }
}
