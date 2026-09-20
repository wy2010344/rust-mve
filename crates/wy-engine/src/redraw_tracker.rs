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
}

impl Clone for RedrawTracker {
    fn clone(&self) -> Self {
        Self {
            memo: self.memo.clone(),
        }
    }
}
