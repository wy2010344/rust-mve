//! winit 平台帧源：基于 `RedrawRequested` 事件驱动动画帧。
//!
//! 对齐 Kotlin `AnimationFrame.kt` 的 `loopFrameSource()`：
//! - 每次 `RedrawRequested` 触发所有活跃订阅的回调
//! - 回调返回 `true` = 动画自然结束 → 移除订阅 + `on_finish(true)`
//! - `cancel()` = 外部取消 → `on_finish(false)`
//! - 帧间隔通过 `Instant::now()` 计算 diff

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Instant;

use wy_signal::{FrameSource, FrameSubscription};

/// 帧订阅回调：`(success, on_finish)`。
type FinishCallback = (bool, Option<Box<dyn FnOnce(bool)>>);

/// winit 帧源：绑定到窗口的 `RedrawRequested` 事件。
///
/// 使用方式：
/// ```ignore
/// let frame_source = WinitFrameSource::new();
/// // 在 redraw 回调中调用 frame_source.tick()
/// // 在动画 API 中传入 frame_source.clone()
/// ```
#[derive(Clone)]
pub struct WinitFrameSource {
    inner: Rc<RefCell<WinitFrameInner>>,
}

struct WinitFrameInner {
    /// 活跃订阅。
    subs: Vec<Subscription>,
    /// 上一帧的时间戳。
    last_tick: Option<Instant>,
}

struct Subscription {
    callback: Box<dyn FnMut(f32) -> bool>,
    on_finish: Option<Box<dyn FnOnce(bool)>>,
    canceled: bool,
    finished: bool,
    /// 订阅时刻的起始时间。
    start: Instant,
}

impl WinitFrameSource {
    /// 创建帧源。
    pub fn new() -> Self {
        Self {
            inner: Rc::new(RefCell::new(WinitFrameInner {
                subs: Vec::new(),
                last_tick: None,
            })),
        }
    }

    /// 在每次 `RedrawRequested` 时调用，驱动所有活跃动画帧。
    ///
    /// `diff_ms` 为自上一次 tick 以来的毫秒数（首次为 0）。
    /// 此方法会清除已完成/已取消的订阅。
    pub fn tick(&self) {
        let mut inner = self.inner.borrow_mut();
        let now = Instant::now();
        let diff_ms = inner
            .last_tick
            .map(|t| t.elapsed().as_secs_f32() * 1000.0)
            .unwrap_or(0.0);
        inner.last_tick = Some(now);

        if diff_ms <= 0.0 || inner.subs.is_empty() {
            return;
        }

        // 逐个回调，收集需要移除的索引
        let mut to_remove: Vec<usize> = Vec::new();
        for (i, sub) in inner.subs.iter_mut().enumerate() {
            if sub.canceled || sub.finished {
                to_remove.push(i);
                continue;
            }

            let elapsed = sub.start.elapsed().as_secs_f32() * 1000.0;
            let stop = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                (sub.callback)(elapsed)
            })) {
                Ok(stop) => stop,
                Err(_) => {
                    // 帧回调异常视为打断
                    sub.finished = true;
                    to_remove.push(i);
                    if let Some(on_finish) = sub.on_finish.take() {
                        drop(inner); // 释放 borrow 后回调
                        on_finish(false);
                        return;
                    }
                    continue;
                }
            };

            if stop {
                sub.finished = true;
                to_remove.push(i);
            }
        }

        // 移除已完成的订阅，并回调 on_finish
        // 注意：需要先收集 on_finish 回调，再释放 borrow
        let mut callbacks: Vec<FinishCallback> = Vec::new();
        for &i in to_remove.iter().rev() {
            if let Some(sub) = inner.subs.get_mut(i) {
                if sub.finished {
                    let success = !sub.canceled;
                    callbacks.push((success, sub.on_finish.take()));
                }
            }
        }
        inner.subs.retain(|s| !s.canceled && !s.finished);

        // 释放 borrow 后执行回调
        drop(inner);
        for (success, cb) in callbacks {
            if let Some(on_finish) = cb {
                on_finish(success);
            }
        }
    }

    /// 是否有活跃的动画订阅。
    pub fn has_active_subs(&self) -> bool {
        self.inner
            .borrow()
            .subs
            .iter()
            .any(|s| !s.canceled && !s.finished)
    }

    /// 活跃订阅数量。
    pub fn active_count(&self) -> usize {
        self.inner
            .borrow()
            .subs
            .iter()
            .filter(|s| !s.canceled && !s.finished)
            .count()
    }
}

impl Default for WinitFrameSource {
    fn default() -> Self {
        Self::new()
    }
}

impl FrameSource for WinitFrameSource {
    fn subscribe(
        &self,
        callback: Box<dyn FnMut(f32) -> bool>,
        on_finish: Box<dyn FnOnce(bool)>,
    ) -> Box<dyn FrameSubscription> {
        let start = Instant::now();
        let sub = Subscription {
            callback,
            on_finish: Some(on_finish),
            canceled: false,
            finished: false,
            start,
        };

        let idx = self.inner.borrow().subs.len();
        self.inner.borrow_mut().subs.push(sub);

        Box::new(WinitSubHandle {
            inner: self.inner.clone(),
            idx,
        })
    }
}

struct WinitSubHandle {
    inner: Rc<RefCell<WinitFrameInner>>,
    idx: usize,
}

impl FrameSubscription for WinitSubHandle {
    fn cancel(&self) {
        let mut inner = self.inner.borrow_mut();
        if let Some(sub) = inner.subs.get_mut(self.idx) {
            if !sub.canceled && !sub.finished {
                sub.canceled = true;
                // 触发 on_finish(false)
                if let Some(on_finish) = sub.on_finish.take() {
                    drop(inner);
                    on_finish(false);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn winit_frame_source_tick_basic() {
        let fs = WinitFrameSource::new();
        let value = Rc::new(RefCell::new(0.0f32));
        let val = value.clone();

        fs.subscribe(
            Box::new(move |elapsed| {
                *val.borrow_mut() = elapsed;
                false
            }),
            Box::new(|_| {}),
        );

        // 首次 tick：diff=0，不触发回调
        fs.tick();
        assert_eq!(*value.borrow(), 0.0);

        // 模拟帧推进（通过手动设置 last_tick）
        {
            let mut inner = fs.inner.borrow_mut();
            inner.last_tick = Some(Instant::now() - std::time::Duration::from_millis(16));
        }

        fs.tick();
        assert!(*value.borrow() > 0.0, "应收到 elapsed 时间");
    }

    #[test]
    fn winit_frame_source_removes_finished() {
        let fs = WinitFrameSource::new();

        fs.subscribe(
            Box::new(|_| true), // 立即结束
            Box::new(|_| {}),
        );

        // 设置 last_tick 以产生正的 diff
        {
            let mut inner = fs.inner.borrow_mut();
            inner.last_tick = Some(Instant::now() - std::time::Duration::from_millis(16));
        }

        fs.tick();
        assert_eq!(fs.active_count(), 0, "已完成的订阅应被移除");
    }

    #[test]
    fn winit_frame_source_cancel() {
        let fs = WinitFrameSource::new();
        let finished = Rc::new(RefCell::new(false));
        let fin = finished.clone();

        let handle = fs.subscribe(
            Box::new(|_| false),
            Box::new(move |success| {
                *fin.borrow_mut() = success;
            }),
        );

        assert!(fs.has_active_subs());
        handle.cancel();
        assert!(!fs.has_active_subs());
        assert!(!*finished.borrow(), "取消应回调 false");
    }

    #[test]
    fn winit_frame_source_multiple_subs() {
        let fs = WinitFrameSource::new();
        let count = Rc::new(RefCell::new(0u32));
        let c = count.clone();

        fs.subscribe(
            Box::new(move |_| {
                *c.borrow_mut() += 1;
                false
            }),
            Box::new(|_| {}),
        );

        let c2 = count.clone();
        fs.subscribe(
            Box::new(move |_| {
                *c2.borrow_mut() += 10;
                false
            }),
            Box::new(|_| {}),
        );

        {
            let mut inner = fs.inner.borrow_mut();
            inner.last_tick = Some(Instant::now() - std::time::Duration::from_millis(16));
        }

        fs.tick();
        assert_eq!(*count.borrow(), 11, "两个订阅都应被回调");
    }
}
