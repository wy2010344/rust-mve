use crate::draw_context::DrawContext;
use crate::scene::Scene;

/// Widget trait：用户实现的 UI 组件。
///
/// `children()` 只执行一次，用于声明子节点结构。
/// `draw()` 在信号变化时重新执行，用于绘制内容。
pub trait Widget: 'static {
    /// 子节点声明，只执行一次。
    fn children(&self, cx: &mut ChildBuilder);

    /// 绘制，信号变化时重新执行。
    fn draw(&self, scene: &mut Scene, cx: &mut DrawContext);
}

/// 子节点构建器。
pub struct ChildBuilder {
    // TODO: 实现
}
