//! 组合模块：提供 Kotlin 风格的嵌套组合 API。
//!
//! 核心思想：通过闭包嵌套定义 UI 树，信号驱动自动重绘。
//! 类似 Kotlin 的 `argChildren()` 模式——没有结构体，纯函数组合。
//!
//! ```ignore
//! run_composition(|cx| {
//!     let count = Signal::new(0);
//!     let c = count.clone();
//!     cx.add_child(FnWidget::new(
//!         move |scene, _| { scene.draw_text(...); },
//!         |_| {},
//!     ).on_click(move |_| { count.set(count.get() + 1); }));
//! });
//! ```

use crate::draw_context::DrawContext;
use crate::scene::Scene;
use crate::widget::{ChildBuilder, Widget};

type DrawFn = Box<dyn Fn(&mut Scene, &mut DrawContext)>;
type ChildrenFn = Box<dyn Fn(&mut ChildBuilder)>;
type ClickFn = Box<dyn Fn(&DrawContext)>;

/// 闭包 Widget：用闭包实现 `draw`，用子闭包树定义子节点。
pub struct FnWidget {
    draw_fn: DrawFn,
    children_fn: ChildrenFn,
    on_click_fn: Option<ClickFn>,
    focusable: bool,
}

impl FnWidget {
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

    pub fn on_click(mut self, f: impl Fn(&DrawContext) + 'static) -> Self {
        self.on_click_fn = Some(Box::new(f));
        self
    }

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

    fn hit_test(&self, _x: f32, _y: f32, _cx: &DrawContext) -> bool {
        true
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
