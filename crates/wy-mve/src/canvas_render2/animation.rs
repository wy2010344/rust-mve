//! 帧时钟与宿主唤醒登记（对标 TS `OverlayScrollBar.ts` 里那块 `requestAnimationFrame`）。
//!
//! # 为什么需要这一层
//!
//! TS 侧的浮现动画是「写一个信号 → 下一帧再写一次」的 rAF 循环：
//! ```ignore
//! function createFade(get, set) {
//!   function to(target, ms) {
//!     const start = now(); const span = target - from;
//!     const step = () => { set(from + span * t); raf = t < 1 ? rAF(step) : 0; };
//!     raf = rAF(step);
//!   }
//! }
//! ```
//! Rust 侧的信号没有这个循环：绘制只在事件到来时发生，没人推帧，动画就永远
//! 停在第 0 帧。所以这里补两件东西，语义与 rAF 一一对应：
//!
//! 1. [`Fade`]：**读时按墙钟求值**的时间线（浮现 → 保持 → 淡出）。不靠定时器
//!    写状态，所以"某一帧没画"不会让动画错位，也不会累积误差；
//! 2. [`FrameTick`]：把"我还有动画没跑完、此刻该重绘"登记给宿主。宿主
//!    （`wy-engine` 的 `Node2App`）在事件循环即将阻塞时取最近一个截止时刻，
//!    到点重绘一次——与编辑器光标闪烁
//!    （[`CARET_BLINK_MS`](crate::canvas_render2::CARET_BLINK_MS)）走的是
//!    **同一套**唤醒机制，只是截止时刻多了一路来源。
//!
//! # 为什么不直接开线程 / 定时器
//!
//! UI 树与信号都是单线程 `Rc`，跨线程定时器要么加锁、要么把回调搬回 UI 线程，
//! 两边都会让"绘制期纯读"这条约束失效。而事件循环本来就有一个"下次什么时候
//! 该醒"的钩子（`WyApp::next_wake`），复用它零成本。

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};
use std::time::{Duration, Instant};

/// 一帧的时长（≈60Hz），对齐 `requestAnimationFrame` 的节奏。
pub const FRAME_MS: f64 = 16.0;

thread_local! {
    /// 单调时钟起点。用 `Instant` 而不是墙钟：改系统时间不该让动画跳帧。
    static ORIGIN: Instant = Instant::now();
    /// 已登记的唤醒牌（弱引用：节点销毁后自动失效，无需显式注销）。
    static PENDING: RefCell<Vec<Weak<FrameTick>>> = const { RefCell::new(Vec::new()) };
}

/// 进程内的单调毫秒（对标 `performance.now()`）。
#[must_use]
pub fn now_ms() -> f64 {
    ORIGIN.with(|o| o.elapsed().as_secs_f64() * 1000.0)
}

/// 宿主唤醒牌：登记一个"此刻要重绘"的截止时刻。
///
/// 弱登记——创建即进全局表，调用方只管 `Rc::clone` 传给需要的人；最后一个
/// 强引用消失后表里的弱引用失效，下次扫描自动剔除（这就是 TS `addDestroy`
/// 里那句 `fadeTo.stop()` 的等价物，只是不需要显式调用）。
pub struct FrameTick {
    /// 绝对时刻（[`now_ms`] 坐标系）；`None` = 无待办。
    at: Cell<Option<f64>>,
}

impl FrameTick {
    /// 新建一块已登记的牌。
    #[must_use]
    pub fn new() -> Rc<Self> {
        let tick = Rc::new(Self {
            at: Cell::new(None),
        });
        PENDING.with(|p| p.borrow_mut().push(Rc::downgrade(&tick)));
        tick
    }

    /// 登记下一次唤醒时刻。
    pub fn schedule(&self, at_ms: f64) {
        self.at.set(Some(at_ms));
    }

    /// 撤销登记（动画结束后调用）。
    pub fn clear(&self) {
        self.at.set(None);
    }

    /// 当前登记的唤醒时刻。
    #[must_use]
    pub fn deadline(&self) -> Option<f64> {
        self.at.get()
    }
}

/// 最近一次需要重绘的剩余时长；`None` = 没有任何动画在跑。
///
/// 宿主把它与自己的其它唤醒源（光标闪烁等）取最小值。已到期的返回
/// `Some(Duration::ZERO)`——**不等**于忙循环：落到 0 表示"该立刻画一帧"，
/// 画完 [`take_due_frame`] 会把牌子摘掉。
#[must_use]
pub fn time_until_next_frame() -> Option<Duration> {
    let now = now_ms();
    PENDING.with(|p| {
        let mut list = p.borrow_mut();
        list.retain(|w| w.strong_count() > 0);
        list.iter()
            .filter_map(Weak::upgrade)
            .filter_map(|t| t.deadline())
            .map(|at| {
                if at <= now {
                    Duration::ZERO
                } else {
                    Duration::from_secs_f64((at - now) / 1000.0)
                }
            })
            .min()
    })
}

/// 摘掉所有**已到期**的唤醒牌，返回"确实有到点的"（宿主据此重绘一帧）。
///
/// 与 [`time_until_next_frame`] 成对使用：宿主先 `on_wake` 再 `next_wake`，
/// 前者决定要不要画，后者决定下一次几点醒。少了这层"摘牌"，到期的牌子会
/// 一直留在表里，`WaitUntil(now)` 就变成满速空转。
pub fn take_due_frame() -> bool {
    let now = now_ms();
    PENDING.with(|p| {
        let mut list = p.borrow_mut();
        list.retain(|w| w.strong_count() > 0);
        let mut due = false;
        for w in list.iter() {
            if let Some(t) = w.upgrade() {
                if t.deadline().is_some_and(|at| at <= now) {
                    t.clear();
                    due = true;
                }
            }
        }
        due
    })
}

/// 「浮现 → 保持 → 淡出」时间线（对标 TS `createFade` + `reveal` + `scheduleHide`）。
///
/// TS 用三段定时器 + rAF 插值拼出这个效果：`reveal()` 起一段 `fadeInMs` 的淡入、
/// 同时排一个 `idleDelayMs` 后触发的 `fadeOutMs` 淡出，每次活动都把计时推后
/// （"停手才开始倒数"）。
///
/// 这里把同一件事写成**一条纯函数**：只存"最近一次活动时刻"与"那次活动的起点
/// 透明度"，当前值按墙钟落在哪一段直接算出来。好处是重复调用幂等、跳过帧不错位，
/// 而且不需要任何定时器——只需要在途中让宿主多画几帧（[`FrameTick`]）。
pub struct Fade {
    tick: Rc<FrameTick>,
    /// 最近一次活动时刻。
    last: Cell<f64>,
    /// 该次浮现的起点透明度（淡出中途被打断时，从这里继续而不是跳回 0）。
    from: Cell<f32>,
    /// 是否被触发过（没触发过恒为 0）。
    started: Cell<bool>,
    /// 淡入时长（毫秒）。
    fade_in: f64,
    /// 停手后多久开始淡出（毫秒）。
    idle_delay: f64,
    /// 淡出时长（毫秒）。
    fade_out: f64,
}

impl Fade {
    /// 三段时长（毫秒），对应 TS `ScrollBarStyle2` 的
    /// `fadeInMs` / `idleDelayMs` / `fadeOutMs`。
    #[must_use]
    pub fn new(fade_in: f64, idle_delay: f64, fade_out: f64) -> Self {
        Self {
            tick: FrameTick::new(),
            last: Cell::new(0.0),
            from: Cell::new(0.0),
            started: Cell::new(false),
            fade_in: fade_in.max(0.0),
            idle_delay: idle_delay.max(0.0),
            fade_out: fade_out.max(0.0),
        }
    }

    /// 触发一次浮现（TS `reveal()`）：从**当前**透明度起步，重新开始倒数。
    pub fn trigger(&self) {
        let cur = self.value();
        self.from.set(cur);
        self.last.set(now_ms());
        self.started.set(true);
        self.tick.schedule(now_ms() + FRAME_MS);
    }

    /// 当前透明度 0~1（读时求值）。
    ///
    /// 途中顺手登记下一帧唤醒——这就是 TS `step()` 里
    /// `raf = t < 1 ? requestAnimationFrame(step) : 0` 的等价物。
    #[must_use]
    pub fn value(&self) -> f32 {
        if !self.started.get() {
            return 0.0;
        }
        let e = (now_ms() - self.last.get()).max(0.0);
        let from = self.from.get();
        let (alpha, next_wake) = if e < self.fade_in {
            // 淡入：from → 1
            let t = if self.fade_in > 0.0 {
                (e / self.fade_in) as f32
            } else {
                1.0
            };
            (from + (1.0 - from) * t, Some(now_ms() + FRAME_MS))
        } else if e < self.idle_delay {
            // 保持：不透明，等到 idel_delay 那一刻再醒（否则永远醒不来）
            (1.0, Some(self.last.get() + self.idle_delay))
        } else {
            let t = if self.fade_out > 0.0 {
                ((e - self.idle_delay) / self.fade_out) as f32
            } else {
                1.0
            };
            if t >= 1.0 {
                (0.0, None)
            } else {
                (1.0 - t, Some(now_ms() + FRAME_MS))
            }
        };
        match next_wake {
            Some(at) => self.tick.schedule(at),
            None => self.tick.clear(),
        }
        alpha.clamp(0.0, 1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 时间线上取一点：把 `last` 回拨 `e` 毫秒，等价于"已经过去 e 毫秒"。
    fn at(f: &Fade, e: f64) -> f32 {
        f.last.set(now_ms() - e);
        f.value()
    }

    fn fade() -> Fade {
        let f = Fade::new(110.0, 850.0, 320.0);
        f.trigger();
        f
    }

    #[test]
    fn fade_is_invisible_before_first_trigger() {
        let f = Fade::new(110.0, 850.0, 320.0);
        assert_eq!(f.value(), 0.0, "没触发过就是完全透明（不占视觉）");
    }

    #[test]
    fn fade_rises_then_holds_then_falls() {
        let f = fade();
        assert!(at(&f, 0.0) < 0.05, "起点接近 0");
        let mid = at(&f, 55.0);
        assert!((mid - 0.5).abs() < 0.1, "淡入中途约一半，实际 {mid}");
        assert!((at(&f, 200.0) - 1.0).abs() < 1e-5, "淡入完成后保持 1");
        let falling = at(&f, 850.0 + 160.0);
        assert!(
            (falling - 0.5).abs() < 0.1,
            "淡出中途约一半，实际 {falling}"
        );
        assert_eq!(at(&f, 850.0 + 400.0), 0.0, "淡出结束归零");
    }

    #[test]
    fn trigger_during_fade_out_continues_from_current_alpha() {
        let f = fade();
        let cur = at(&f, 850.0 + 160.0); // 淡出中途
        assert!(cur > 0.2 && cur < 0.8);
        f.trigger(); // 重新活动
        let just = f.value();
        assert!(
            (just - cur).abs() < 0.05,
            "重新触发时不该跳回 0（TS 里 createFade 从当前值起步）"
        );
        assert!((at(&f, 200.0) - 1.0).abs() < 1e-5, "随后回到不透明");
    }

    #[test]
    fn fade_keeps_scheduling_frames_until_settled() {
        let f = fade();
        let _ = at(&f, 0.0);
        assert!(f.tick.deadline().is_some(), "途中要排下一帧");
        let _ = at(&f, 850.0 + 400.0);
        assert!(f.tick.deadline().is_none(), "静止后必须摘牌");
    }

    #[test]
    fn hold_phase_wakes_exactly_when_fade_out_begins() {
        let f = fade();
        f.last.set(now_ms() - 200.0); // 处于保持段
        let _ = f.value();
        let at = f.tick.deadline().expect("保持段也要排唤醒");
        let remain = at - now_ms();
        assert!(
            (remain - 650.0).abs() < 20.0,
            "应在 idleDelay 结束点醒来（剩余 {remain}）"
        );
    }

    #[test]
    fn take_due_frame_reports_and_clears() {
        let tick = FrameTick::new();
        tick.schedule(now_ms() - 1.0);
        assert!(take_due_frame(), "已过期的牌子算到点");
        assert!(tick.deadline().is_none(), "到点后必须摘牌");
        assert!(!take_due_frame(), "摘过就没有第二个到点的了");
    }

    #[test]
    fn no_frame_requested_without_tickers() {
        let tick = FrameTick::new();
        tick.schedule(now_ms() + 10_000.0);
        assert!(time_until_next_frame().is_some());
        tick.clear();
        drop(tick);
        assert!(
            time_until_next_frame().is_none(),
            "牌子全没了就该回到纯 Wait（不强求宿主推帧）"
        );
    }
}
