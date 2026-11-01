//! 自动重绘追踪器：帧级绘制缓存 + 信号驱动重绘。
//!
//! 复刻 Kotlin `Renderer.signal.collect { didDraw().draw(canvas) }` 模型：
//! - 用 [`RecordMemo`] 缓存每一帧的 `Scene`（等价 Kotlin `didDraw` memo）；
//! - 绘制闭包只在**依赖信号变化**时重新执行，其余帧直接复用缓存 Scene，
//!   不再每次 redraw 都跑 `draw()` / Parley shaping（WS：管道精确更新）；
//! - 依赖变化且值确实改变时触发 `on_change` → `request_redraw()`，
//!   由下一帧 `record()` 消费并重挂叶子边。

use std::rc::Rc;

use wy_render::Scene;
use wy_signal::RecordMemo;

/// 帧绘制追踪器。
///
/// 等价于 Kotlin 的 `Renderer.signal` + `didDraw` memo 的组合：
/// `record()` 只在信号变化时重跑绘制闭包，其余帧复用缓存 Scene。
pub struct RedrawTracker {
    /// 绘制缓存 memo：依赖信号变化 → 重录 Scene。
    memo: RecordMemo<Scene>,
}

impl RedrawTracker {
    /// 创建追踪器。
    ///
    /// `request_redraw` 会在依赖信号变化时被调用，触发下一帧绘制。
    pub fn new(request_redraw: Rc<dyn Fn()>) -> Self {
        let memo = RecordMemo::new();
        memo.on_change(request_redraw);
        Self { memo }
    }

    /// 记录一帧：在追踪上下文中执行绘制闭包，缓存/复用 Scene。
    ///
    /// 闭包 `f` 内的所有 `Signal.get()` 自动注册为依赖。
    /// 依赖变化时 effect 重跑 → `request_redraw()` → 下一帧重绘。
    pub fn draw(&self, f: impl FnOnce(&mut Scene)) {
        self.memo.record(f);
    }

    /// 读取缓存的 Scene 引用（供 Vello 翻译/提交）。
    pub fn scene(&self) -> std::cell::Ref<'_, Scene> {
        self.memo.borrow()
    }

    /// 让缓存失效：下一帧必定重跑绘制闭包。
    ///
    /// **帧环境（视口尺寸 / 缩放比）不是信号**，追踪器看不见它。宿主在窗口
    /// resize、DPI 变化后必须调这个，否则会复用上一帧按旧尺寸录的 Scene——
    /// 现象就是"窗口拉大，内容不动；点一下（触发别的信号）才跟上"。
    pub fn invalidate(&self) {
        self.memo.invalidate();
    }
}

impl Clone for RedrawTracker {
    fn clone(&self) -> Self {
        Self {
            memo: self.memo.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn cache_is_reused_until_invalidated() {
        let calls = Cell::new(0);
        let tracker = RedrawTracker::new(Rc::new(|| {}));
        let draw = || {
            tracker.draw(|_s: &mut Scene| calls.set(calls.get() + 1));
        };

        draw();
        draw();
        assert_eq!(calls.get(), 1, "无信号变化时复用缓存帧");

        // 视口 / DPI 变化：环境不是信号，必须由宿主显式置脏。
        tracker.invalidate();
        draw();
        assert_eq!(calls.get(), 2, "invalidate 后必须重跑绘制闭包");

        draw();
        assert_eq!(calls.get(), 2, "置脏是一次性的");
    }
}
