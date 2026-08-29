//! Widget trait：用户实现的 UI 组件。

use std::any::Any;

use crate::draw_context::DrawContext;
use crate::event::PointerEvent;
use crate::scene::Scene;

/// Widget trait：用户实现的 UI 组件。
///
/// ## 生命周期
///
/// - [`Widget::children`] **只执行一次**，用于声明子节点结构，把子节点通过
///   [`ChildBuilder`] 注册进组件树。
/// - [`Widget::draw`] 在信号变化时重新执行，向 [`Scene`] 添加高层图元；
///   绘制过程中读取的信号会被自动追踪（作为依赖）。
///
/// ## 事件
///
/// - [`Widget::hit_test`] 判断点是否在组件范围内，默认检查外框矩形。
/// - [`Widget::on_pointer_down`] 等事件处理器在命中测试通过后被调用，
///   支持 capture→bubble 两阶段传播。
pub trait Widget: 'static + Any {
    /// 子节点声明，只执行一次。
    ///
    /// 实现应调用 `cx.add_child(...)` 注册子节点。默认实现不注册任何子节点。
    fn children(&self, cx: &mut ChildBuilder) {
        let _ = cx;
    }

    /// 绘制，信号变化时重新执行。
    fn draw(&self, scene: &mut Scene, cx: &mut DrawContext);

    /// 命中测试：判断 `(x, y)` 是否落在组件范围内。
    ///
    /// 坐标是相对于组件外框左上角的局部坐标。默认实现检查是否在外框矩形内。
    /// 自定义组件可重写此方法实现非矩形命中区域（如圆形、透明区域排除等）。
    fn hit_test(&self, x: f32, y: f32, cx: &DrawContext) -> bool {
        cx.outer_rect().contains(crate::Point::new(x, y))
    }

    /// 指针按下事件（capture 阶段和 bubble 阶段都会调用）。
    ///
    /// 默认实现不消费事件（继续传播）。
    fn on_pointer_down(&mut self, _event: &mut PointerEvent, _cx: &DrawContext) {}

    /// 指针释放事件。
    fn on_pointer_up(&mut self, _event: &mut PointerEvent, _cx: &DrawContext) {}

    /// 指针移动事件。
    fn on_pointer_move(&mut self, _event: &mut PointerEvent, _cx: &DrawContext) {}

    /// 点击事件（按下 + 释放在同一组件上）。
    fn on_click(&mut self, _cx: &DrawContext) {}

    /// 是否可获得键盘焦点。
    ///
    /// 返回 `true` 的组件会参与 Tab 遍历和点击聚焦。默认 `false`。
    fn focusable(&self) -> bool {
        false
    }

    /// 无障碍节点描述（角色、名称等）。
    ///
    /// 返回 `Some((role, name))` 表示该组件应出现在无障碍树中。
    /// `role` 和 `name` 是简化表示，完整映射由 `wy-engine` 的 `AccessibilityBridge` 完成。
    /// 默认返回 `None`（不出现在无障碍树中）。
    fn accessibility(&self) -> Option<(&str, Option<&str>)> {
        None
    }
}

impl dyn Widget {
    /// 尝试把组件向下转型为具体类型 `T`。
    ///
    /// 用于需要"拿到某个已知子组件做专门操作"的场合（如焦点、命令）。
    pub fn downcast_ref<T: Any>(&self) -> Option<&T> {
        let any: &dyn Any = self;
        any.downcast_ref::<T>()
    }

    /// 尝试把组件向下转型为具体类型 `T`（可变引用）。
    pub fn downcast_mut<T: Any>(&mut self) -> Option<&mut T> {
        let any: &mut dyn Any = self;
        any.downcast_mut::<T>()
    }
}

/// 子节点构建器：把 Widget 的子组件注册进组件树。
///
/// 由框架创建并传入 [`Widget::children`]，组件在构建期调用
/// [`ChildBuilder::add_child`] 逐个注册子节点。注册即建立父子关系，后续
/// 布局（`wy-layout`）与命中测试按注册顺序遍历子节点。
#[derive(Default)]
pub struct ChildBuilder {
    children: Vec<Box<dyn Widget>>,
}

impl ChildBuilder {
    /// 创建空的构建器。
    pub fn new() -> Self {
        Self {
            children: Vec::new(),
        }
    }

    /// 注册一个子节点，返回子节点的顺序索引。
    pub fn add_child(&mut self, widget: impl Widget) -> usize {
        self.children.push(Box::new(widget));
        self.children.len() - 1
    }

    /// 已注册的子节点数量。
    pub fn len(&self) -> usize {
        self.children.len()
    }

    /// 是否尚未注册任何子节点。
    pub fn is_empty(&self) -> bool {
        self.children.is_empty()
    }

    /// 只读访问子节点列表。
    pub fn iter(&self) -> impl Iterator<Item = &dyn Widget> {
        self.children.iter().map(|b| b.as_ref())
    }

    /// 消费构建器，返回子节点列表。
    pub fn into_children(self) -> Vec<Box<dyn Widget>> {
        self.children
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Color, Rect};

    struct DummyWidget {
        rects: usize,
    }

    impl Widget for DummyWidget {
        fn draw(&self, scene: &mut Scene, _cx: &mut DrawContext) {
            for i in 0..self.rects {
                scene.fill_rect(Rect::new(i as f32, 0.0, 10.0, 10.0), Color::BLACK);
            }
        }
    }

    struct Parent;

    impl Widget for Parent {
        fn children(&self, cx: &mut ChildBuilder) {
            cx.add_child(DummyWidget { rects: 2 });
            cx.add_child(DummyWidget { rects: 3 });
        }

        fn draw(&self, scene: &mut Scene, _cx: &mut DrawContext) {
            scene.fill_rect(Rect::new(0.0, 0.0, 100.0, 100.0), Color::RED);
        }
    }

    /// 支持点击计数的测试组件。
    struct ClickCounter {
        count: usize,
    }

    impl Widget for ClickCounter {
        fn draw(&self, scene: &mut Scene, cx: &mut DrawContext) {
            scene.fill_rect(cx.outer_rect(), Color::BLUE);
        }

        fn on_click(&mut self, _cx: &DrawContext) {
            self.count += 1;
        }
    }

    #[test]
    fn child_builder_registers_children_in_order() {
        let mut cx = ChildBuilder::new();
        let parent = Parent;
        parent.children(&mut cx);

        assert_eq!(cx.len(), 2);
        let first = cx.iter().next().unwrap();
        let second = cx.iter().nth(1).unwrap();
        assert!(first.downcast_ref::<DummyWidget>().is_some());
        assert!(second.downcast_ref::<DummyWidget>().is_some());
        assert_eq!(first.downcast_ref::<DummyWidget>().unwrap().rects, 2);
        assert_eq!(second.downcast_ref::<DummyWidget>().unwrap().rects, 3);
    }

    #[test]
    fn child_builder_empty_when_new() {
        assert!(ChildBuilder::new().is_empty());
    }

    #[test]
    fn child_builder_add_child_returns_index() {
        let mut cx = ChildBuilder::new();
        let i0 = cx.add_child(DummyWidget { rects: 1 });
        let i1 = cx.add_child(DummyWidget { rects: 1 });
        assert_eq!(i0, 0);
        assert_eq!(i1, 1);
    }

    #[test]
    fn widget_draw_produces_primitives() {
        let w = DummyWidget { rects: 2 };
        let mut scene = Scene::new();
        let mut cx = DrawContext::new(
            Rect::new(0.0, 0.0, 10.0, 10.0),
            crate::Point::new(0.0, 0.0),
            crate::Size::new(10.0, 10.0),
        );
        w.draw(&mut scene, &mut cx);
        assert_eq!(scene.len(), 2);
    }

    #[test]
    fn hit_test_default_checks_outer_rect() {
        let w = DummyWidget { rects: 1 };
        let cx = DrawContext::new(
            Rect::new(10.0, 20.0, 100.0, 50.0),
            crate::Point::new(10.0, 20.0),
            crate::Size::new(100.0, 50.0),
        );
        // 在外框内
        assert!(Widget::hit_test(&w, 50.0, 30.0, &cx));
        // 在外框外
        assert!(!Widget::hit_test(&w, 5.0, 5.0, &cx));
        // 边界上
        assert!(Widget::hit_test(&w, 10.0, 20.0, &cx));
        // 右下角外
        assert!(!Widget::hit_test(&w, 111.0, 71.0, &cx));
    }

    #[test]
    fn on_click_increments_counter() {
        let mut w = ClickCounter { count: 0 };
        let cx = DrawContext::default();
        assert_eq!(w.count, 0);
        Widget::on_click(&mut w, &cx);
        assert_eq!(w.count, 1);
        Widget::on_click(&mut w, &cx);
        assert_eq!(w.count, 2);
    }

    #[test]
    fn downcast_mut_works() {
        let mut w = ClickCounter { count: 0 };
        let widget: &mut dyn Widget = &mut w;
        let counter = widget.downcast_mut::<ClickCounter>().unwrap();
        counter.count = 42;
        assert_eq!(w.count, 42);
    }

    #[test]
    fn child_builder_into_children() {
        let mut cx = ChildBuilder::new();
        cx.add_child(DummyWidget { rects: 1 });
        cx.add_child(DummyWidget { rects: 2 });
        let children = cx.into_children();
        assert_eq!(children.len(), 2);
    }
}
