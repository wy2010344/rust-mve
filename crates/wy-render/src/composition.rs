//! 组合模块：提供 Kotlin 风格的嵌套组合 API。
//!
//! 核心思想：通过闭包嵌套定义 UI 树，信号驱动自动重绘。
//! 类似 Kotlin 的 `argChildren()` 模式。

use crate::draw_context::DrawContext;
use crate::scene::Scene;
use crate::widget::{ChildBuilder, Widget};
use crate::{Color, Point};

type DrawFn = Box<dyn Fn(&mut Scene, &mut DrawContext)>;
type ChildrenFn = Box<dyn Fn(&mut ChildBuilder)>;
type ClickFn = Box<dyn Fn(&DrawContext)>;

/// 闭包 Widget：用闭包实现 `draw`，用子闭包树定义子节点。
///
/// 类似 Kotlin 中 `object : Node(this) { override fun argChildren() { ... } }`。
pub struct FnWidget {
    draw_fn: DrawFn,
    children_fn: ChildrenFn,
    on_click_fn: Option<ClickFn>,
    focusable: bool,
}

impl FnWidget {
    /// 创建闭包 Widget。
    pub fn new(
        draw_fn: impl Fn(&mut Scene, &mut DrawContext) + 'static,
        children_fn: impl Fn(&mut ChildBuilder) + 'static,
    ) -> Self {
        Self {
            draw_fn: Box::new(draw_fn),
            children_fn: Box::new(children_fn),
            on_click_fn: None,
            focusable: false,
        }
    }

    /// 设置点击回调。
    pub fn on_click(mut self, f: impl Fn(&DrawContext) + 'static) -> Self {
        self.on_click_fn = Some(Box::new(f));
        self
    }

    /// 设为可聚焦。
    pub fn focusable(mut self) -> Self {
        self.focusable = true;
        self
    }
}

impl Widget for FnWidget {
    fn draw(&self, scene: &mut Scene, cx: &mut DrawContext) {
        (self.draw_fn)(scene, cx);
    }

    fn children(&self, cx: &mut ChildBuilder) {
        (self.children_fn)(cx);
    }

    fn on_click(&mut self, cx: &DrawContext) {
        if let Some(f) = &self.on_click_fn {
            f(cx);
        }
    }

    fn focusable(&self) -> bool {
        self.focusable
    }
}

/// ChildBuilder 扩展：提供便捷的构建方法。
///
/// 类似 Kotlin 的 `StateHolderWithNode` 上的子节点添加方法。
pub trait ChildBuilderExt {
    /// 添加一个纯绘制 Widget（无子节点）。
    fn leaf(&mut self, draw_fn: impl Fn(&mut Scene, &mut DrawContext) + 'static) -> usize;

    /// 添加一个带子节点的 Widget。
    fn node(
        &mut self,
        draw_fn: impl Fn(&mut Scene, &mut DrawContext) + 'static,
        children_fn: impl Fn(&mut ChildBuilder) + 'static,
    ) -> usize;

    /// 添加一段文本。
    fn text(&mut self, text: &str) -> usize;

    /// 添加一个矩形。
    fn rect(&mut self, color: Color) -> usize;

    /// 添加一个圆角矩形。
    fn round_rect(&mut self, color: Color, radius: f32) -> usize;
}

impl ChildBuilderExt for ChildBuilder {
    fn leaf(&mut self, draw_fn: impl Fn(&mut Scene, &mut DrawContext) + 'static) -> usize {
        self.add_child(FnWidget::new(draw_fn, |_| {}))
    }

    fn node(
        &mut self,
        draw_fn: impl Fn(&mut Scene, &mut DrawContext) + 'static,
        children_fn: impl Fn(&mut ChildBuilder) + 'static,
    ) -> usize {
        self.add_child(FnWidget::new(draw_fn, children_fn))
    }

    fn text(&mut self, text: &str) -> usize {
        let text = text.to_string();
        self.leaf(move |scene, cx| {
            let rect = cx.outer_rect();
            scene.draw_text(Point::new(rect.x, rect.y), &text, 14.0, Color::BLACK);
        })
    }

    fn rect(&mut self, color: Color) -> usize {
        self.leaf(move |scene, cx| {
            scene.fill_rect(cx.outer_rect(), color);
        })
    }

    fn round_rect(&mut self, color: Color, radius: f32) -> usize {
        self.leaf(move |scene, cx| {
            scene.fill_round_rect(cx.outer_rect(), radius, color);
        })
    }
}
