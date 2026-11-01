//! canvasRender2 顶层工具（对标 TS `Renderer.ts` 的注册表 / 助手部分）。
//!
//! TS `Renderer.ts` 把"全局鼠标注册表 + 拖拽助手 + 连击计数"放在 `renderCanvas`
//! 闭包里，组件经 `ctx.consume(globalMouse2Context)` 拿到 `GlobalMouse2`。
//! Rust 的 `Node2` 组件只拿到 `node`、没有 `ctx`，所以这里用**模块级注册表**
//! （线程局部）等价表达：组件调用 [`drag_mouse`] 注册全局 move/up，引擎在事件
//! 循环里消费 [`dispatch_global_mouse_down`] / [`dispatch_global_mouse_move`] /
//! [`dispatch_global_mouse_up`]，语义与 TS 逐字一致——指针移出节点后仍持续
//! 收到事件（拖拽场景）。
//!
//! 这些注册表默认是空的，引擎消费时是无操作，不改变任何既有行为；只有组件
//! 显式调用 [`drag_mouse`] 才会生效（对应 TS 里 `dragMouse` 的 `downCallbacks` /
//! `moveCallbacks` / `upCallbacks` / `wheelCallbacks` 之一种）。

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Instant;

use crate::canvas_render2::node2::Node2;

// ============================================================================
// 全局鼠标注册表（宿主默认的那一套）
// ============================================================================
//
// `down` / `move` / `up` 三条直接复用 [`crate::canvas_render2::tree::global_mouse`]
// （TS `tree/globalMouse.ts`）——那里是唯一实现，本文件不再复制一份。
//
// 滚轮**不在**这里：它沿命中链派发 `mouseWheel` 消息（由 `scroll_content` /
// `scroll_bar` 挂载 `scroll_wheel`），这样才能"内层先滚、滚到底才交给外层"并
// 被 `stopPropagation` 拦住——与 TS 的 `tree/globalMouse.ts` 约定完全一致。

/// 宿主默认注册表：按下 / 移动 / 抬起三条鼠标通道。
struct HostGlobalMouse {
    mouse: crate::canvas_render2::tree::global_mouse::GlobalMouseRegistry2,
}

impl HostGlobalMouse {
    fn new() -> Self {
        Self {
            mouse: crate::canvas_render2::tree::global_mouse::create_global_mouse2(),
        }
    }
}

thread_local! {
    /// 当前线程的默认注册表（引擎与组件同线程，单线程渲染）。
    static GLOBAL_MOUSE: RefCell<HostGlobalMouse> = RefCell::new(HostGlobalMouse::new());
}

/// 引擎消费：全局按下（坐标相对画布，不依赖命中测试）。
pub fn dispatch_global_mouse_down(x: f32, y: f32) {
    GLOBAL_MOUSE.with(|m| m.borrow().mouse.emit_down(x, y));
}

/// 引擎消费：全局移动（坐标相对画布，不依赖命中测试）。
pub fn dispatch_global_mouse_move(x: f32, y: f32) {
    GLOBAL_MOUSE.with(|m| m.borrow().mouse.emit_move(x, y));
}

/// 引擎消费：全局抬起（无坐标）。
pub fn dispatch_global_mouse_up() {
    GLOBAL_MOUSE.with(|m| m.borrow().mouse.emit_up());
}

// ============================================================================
// 注册助记（TS `GlobalMouse2.registerMouseDown/Move/Up`）
// ============================================================================

/// 注册全局按下，返回注销函数。
pub fn register_down(f: Rc<dyn Fn(f32, f32)>) -> impl Fn() {
    GLOBAL_MOUSE.with(|m| m.borrow().mouse.api.register_mouse_down(f))
}

/// 注册全局移动，返回注销函数。
pub fn register_move(f: Rc<dyn Fn(f32, f32)>) -> impl Fn() {
    GLOBAL_MOUSE.with(|m| m.borrow().mouse.api.register_mouse_move(f))
}

/// 注册全局抬起，返回注销函数。
pub fn register_up(f: Rc<dyn Fn()>) -> impl Fn() {
    GLOBAL_MOUSE.with(|m| m.borrow().mouse.api.register_mouse_up(f))
}

// ============================================================================
// 拖拽助手（对标 TS `dragMouse`）
// ============================================================================

/// 通用拖拽助手（对标 TS `dragMouse`）。
///
/// 按下后接管全局 move，松手时以最后坐标再回调一次并自动摘除注册，避免每次
/// 拖拽在注册表里残留闭包。返回一个提前终止拖拽的函数。
///
/// TS 的 `dragMouse(ctx, change)` 要从 context 里 consume 出 `GlobalMouse2`；
/// Rust 这里走**宿主默认注册表**（[`crate::canvas_render2::tree::global_mouse::drag_mouse_on`]），
/// 因为 `Node2` 组件目前只拿到 `node`、没有 `ctx`。
pub fn drag_mouse(change: impl Fn(f32, f32) + 'static) -> impl Fn() {
    GLOBAL_MOUSE.with(|m| {
        crate::canvas_render2::tree::global_mouse::drag_mouse_on(&m.borrow().mouse.api, change)
    })
}

// ============================================================================
// 连击计数（对标 TS `createClickCounter`）
// ============================================================================

/// 上一次点击的记忆：`(节点, x, y, 时间, 已累计的连击数)`。
///
/// 抽成别名只为读得顺——`Rc<RefCell<..>>` 写全了会把函数签名淹掉。
type LastClick = Rc<RefCell<Option<(Node2, f32, f32, Instant, usize)>>>;

/// 连击判定：同一叶子节点、400ms 内、位移 < 5px 则计数递增，否则重置
/// （对标 Kotlin `ClickTracker` / TS `createClickCounter`）。
///
/// 返回 `Fn(leaf, x, y) -> count`：`count >= 3` 三击选段、`== 2` 双击选词、
/// `== 1` 单击。引擎的鼠标按下派发（`onMouseDown`）在 P5 接入选区会话时消费它。
///
/// 不写 `#[must_use]`：返回的 `impl Fn` 本身就是 `#[must_use]`，重复标注会
/// 触发 `double_must_use` 之类的告警。
pub fn create_click_counter() -> impl Fn(Option<Node2>, f32, f32) -> usize + 'static {
    let last: LastClick = Rc::new(RefCell::new(None));
    move |leaf: Option<Node2>, x: f32, y: f32| -> usize {
        let now = Instant::now();
        let count = {
            let prev = last.borrow();
            match &*prev {
                Some((pnode, px, py, pt, pc))
                    if leaf.as_ref().is_some_and(|l| l == pnode)
                        && (x - px).abs() < 5.0
                        && (y - py).abs() < 5.0
                        && now.duration_since(*pt).as_millis() < 400 =>
                {
                    *pc + 1
                }
                _ => 1,
            }
        };
        if let Some(l) = leaf {
            *last.borrow_mut() = Some((l, x, y, now, count));
        }
        count
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    /// 拖拽：按下期间全局 move 持续回调（含移出节点），抬起以最后坐标再调一次后
    /// 自动摘除（对标 TS `dragMouse` 的 `offMove` / `offUp` 组合）。
    #[test]
    fn drag_mouse_receives_global_move_and_final_up() {
        let log: Rc<RefCell<Vec<(f32, f32)>>> = Rc::new(RefCell::new(Vec::new()));
        let stop = drag_mouse({
            let log = Rc::clone(&log);
            move |x, y| log.borrow_mut().push((x, y))
        });
        // 按下期间全局 move（即使指针移出节点也持续收到）
        dispatch_global_mouse_move(10.0, 20.0);
        dispatch_global_mouse_move(30.0, 40.0);
        assert_eq!(*log.borrow(), vec![(10.0, 20.0), (30.0, 40.0)]);
        // 抬起：以最后坐标再回调一次，随后自动摘除
        dispatch_global_mouse_up();
        assert_eq!(
            *log.borrow(),
            vec![(10.0, 20.0), (30.0, 40.0), (30.0, 40.0)]
        );
        // 摘除后不再收到
        dispatch_global_mouse_move(50.0, 60.0);
        assert_eq!(
            *log.borrow(),
            vec![(10.0, 20.0), (30.0, 40.0), (30.0, 40.0)]
        );
        let _ = stop;
    }

    /// 提前 `stop` 立即摘除两个注册。
    #[test]
    fn drag_mouse_stop_cancels_registration() {
        let log: Rc<RefCell<Vec<(f32, f32)>>> = Rc::new(RefCell::new(Vec::new()));
        let stop = drag_mouse({
            let log = Rc::clone(&log);
            move |x, y| log.borrow_mut().push((x, y))
        });
        stop();
        dispatch_global_mouse_move(1.0, 2.0);
        assert!(log.borrow().is_empty(), "stop 后应不再收到 move");
    }

    /// 连击计数：同节点近距递增，位移过远或换节点重置为 1（对标 TS `createClickCounter`）。
    #[test]
    fn click_counter_increments_then_resets() {
        let record = create_click_counter();
        let a = Node2::empty();
        assert_eq!(record(Some(a.clone()), 10.0, 10.0), 1);
        assert_eq!(record(Some(a.clone()), 11.0, 11.0), 2, "同节点近距 → 双击");
        assert_eq!(record(Some(a.clone()), 12.0, 12.0), 3, "同节点近距 → 三击");
        // 位移过远 → 重置为 1
        assert_eq!(record(Some(a.clone()), 100.0, 100.0), 1, "位移过远 → 重置");
        // 不同节点 → 重置为 1
        let b = Node2::empty();
        assert_eq!(record(Some(b), 10.0, 10.0), 1, "不同节点 → 重置");
    }
}
