//! 自动重绘追踪器：框架层自动关联信号 → 重绘。
//!
//! 复刻 Kotlin `Renderer.signal.collect { didDraw().draw(canvas) }` 模式：
//! - `draw(f)` 将闭包 `f` 包裹在 effect 上下文中执行
//! - `f` 中每个 `Signal.get()` 自动注册此 effect 为依赖
//! - 信号变化 → effect 重跑 → `request_redraw()` → 触发重绘
//! - 业务代码零 `create_effect`

use std::rc::Rc;

use wy_signal::TrackEffect;

/// 自动重绘追踪器。
///
/// 等价于 Kotlin 的 `Renderer.signal`：一个根 effect 包裹整个 draw，
/// 所有信号读取自动追踪，信号变化自动触发重绘。
#[derive(Clone)]
pub struct RedrawTracker {
    /// 根 effect：body 只调用 `request_redraw()`，
    /// 依赖由 `draw()` 中的 `collect` 在 draw 期间注册。
    effect: TrackEffect,
}

impl RedrawTracker {
    /// 创建追踪器。
    ///
    /// `request_redraw` 会在信号变化时被调用，触发下一帧绘制。
    pub fn new(request_redraw: Rc<dyn Fn()>) -> Self {
        let effect = TrackEffect::new(move || {
            request_redraw();
        });
        Self { effect }
    }

    /// 在追踪上下文中执行闭包（等价于 Kotlin 的 `signal.collect { draw() }`）。
    ///
    /// 闭包 `f` 内的所有 `Signal.get()` 自动注册为根 effect 的依赖。
    /// 信号变化时 effect 重跑 → `request_redraw()` → 下一帧重绘。
    pub fn draw<R>(&self, f: impl FnOnce() -> R) -> R {
        self.effect.collect(f)
    }
}
