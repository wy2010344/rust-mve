//! `render_when`：按条件信号切换分支的动态区域。
//!
//! 复刻 Kotlin composable 里 `if (signal.value) { A } else { B }` 的**反应式**语义：
//! 条件信号变化 → 旧分支 holder 销毁、新分支重建；条件不变则不重建。
//!
//! 实现复用 [`render_for_each`](crate::foreach::render_for_each) 的 keyed holder 机制：
//! 把一个 `bool` 键的单元素列表喂进去，条件翻转即键翻转 → 旧 holder 走 `destroyed`、
//! 新 holder 触发 `branch` 重建。因此分支切换与列表项增删走**完全相同的生命周期**。

use crate::context::NodeContext;
use crate::foreach::render_for_each;
use crate::node::ChildSlot;

/// 追加一个条件分支区域到 `slots`。
///
/// - `cond` 在区域 memo 求值时执行，读取条件信号（自动追踪）。
/// - `branch(cond_value, cx)` 仅在**切换到该分支**时执行一次，向 `cx` 注入该分支子节点。
///
/// 分支的节点是动态区域（`GetNodes` slot），直接注入父容器参与一维布局，
/// 不会被包成中间节点。
pub fn render_when(
    slots: &mut Vec<ChildSlot>,
    cond: impl Fn() -> bool + 'static,
    branch: impl Fn(bool, &mut NodeContext) + 'static,
) {
    render_for_each(
        slots,
        move |emit| emit(cond(), ()),
        move |key, _value, cx| branch(key, cx),
    );
}
