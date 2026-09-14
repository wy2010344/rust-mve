//! 声明式组件辅助函数：text / button / row / column。
//!
//! 用户通过组合这些函数构建 UI，不需要手动写 draw/hit_test/event。
//!
//! 布局在**渲染期**由容器节点的 `Layout` 角色完成（对应 Kotlin Flex 一维布局），
//! 组件构造时不计算任何位置。

use std::rc::Rc;

use crate::context::NodeContext;
use crate::node::{Layout, Node};
use wy_render::{Color, Point, Rect, Scene};

/// 文本组件：闭包返回文本内容。
///
/// 闭包在 draw 时执行，内部读取信号会自动追踪依赖。
///
/// ```ignore
/// let count = Signal::new(0);
/// cx.child(text(move || format!("Count: {}", count.get())));
/// ```
pub fn text(content: impl Fn() -> String + 'static) -> Node {
    Node {
        draw_fn: Rc::new(move |scene| {
            if let Some(scene) = scene.downcast_mut::<Scene>() {
                let t = content();
                if !t.is_empty() {
                    scene.draw_text(Point::new(0.0, 0.0), &t, 14.0, Color::BLACK);
                }
            }
        }),
        ..Node::default()
    }
}

/// 文本组件（带配置）：闭包返回文本内容，指定字号和颜色。
pub fn text_styled(content: impl Fn() -> String + 'static, font_size: f32, color: Color) -> Node {
    Node {
        draw_fn: Rc::new(move |scene| {
            if let Some(scene) = scene.downcast_mut::<Scene>() {
                let t = content();
                if !t.is_empty() {
                    scene.draw_text(Point::new(0.0, 0.0), &t, font_size, color);
                }
            }
        }),
        ..Node::default()
    }
}

/// 信号文本：draw 期读取信号并 Display 格式化，信号变化自动触发重绘。
pub fn text_signal(value: impl Fn() -> Box<dyn std::fmt::Display> + 'static) -> Node {
    Node {
        draw_fn: Rc::new(move |scene| {
            if let Some(scene) = scene.downcast_mut::<Scene>() {
                let t = format!("{}", value());
                if !t.is_empty() {
                    scene.draw_text(Point::new(0.0, 0.0), &t, 14.0, Color::BLACK);
                }
            }
        }),
        ..Node::default()
    }
}

/// 按钮组件：标签文本 + 点击回调。
///
/// 内置：pressed 视觉反馈、焦点支持、点击后停止事件冒泡。
///
/// ```ignore
/// let count = Signal::new(0);
/// cx.child(button(
///     move || format!("Count: {}", count.get()),
///     {
///         let c = count.clone();
///         move || c.set(c.get() + 1)
///     },
/// ));
/// ```
pub fn button(label: impl Fn() -> String + 'static, on_click: impl Fn() + 'static) -> Node {
    let label = Rc::new(label);
    let on_click = Rc::new(on_click);
    let label2 = label.clone();
    let on_click2 = on_click.clone();

    Node {
        focusable: true,
        width: 90.0,
        height: 32.0,
        hit_test_fn: Rc::new(|x, y| (0.0..90.0).contains(&x) && (0.0..32.0).contains(&y)),
        draw_fn: Rc::new(move |scene| {
            if let Some(scene) = scene.downcast_mut::<Scene>() {
                let bg = Color::from_u32(0xFF_DCDCDC);
                let border = Color::from_u32(0xFF_B0B0B0);
                scene.fill_round_rect(Rect::new(0.0, 0.0, 90.0, 32.0), 6.0, bg);
                scene.stroke_round_rect(Rect::new(0.0, 0.0, 90.0, 32.0), 6.0, border, 1.5);
                let t = label2();
                scene.draw_text(Point::new(10.0, 8.0), &t, 14.0, Color::BLACK);
            }
        }),
        on_click_fn: Some(Rc::new(move |event| {
            on_click2();
            event.stop_propagation();
        })),
        ..Node::default()
    }
}

/// 水平排列容器（主轴 = X，子节点依次右排，间距 8）。
pub fn row(children: impl Fn(&mut NodeContext) + 'static) -> Node {
    Node {
        layout: Some(Layout::Row { gap: 8.0 }),
        arg_children_fn: Rc::new(move |cx| children(cx)),
        ..Node::default()
    }
}

/// 水平排列容器（带绝对位置）。
pub fn row_at(x: f32, y: f32, children: impl Fn(&mut NodeContext) + 'static) -> Node {
    Node {
        x,
        y,
        layout: Some(Layout::Row { gap: 8.0 }),
        arg_children_fn: Rc::new(move |cx| children(cx)),
        ..Node::default()
    }
}

/// 垂直排列容器（主轴 = Y，子节点依次下排，间距 8）。
pub fn column(children: impl Fn(&mut NodeContext) + 'static) -> Node {
    Node {
        layout: Some(Layout::Column { gap: 8.0 }),
        arg_children_fn: Rc::new(move |cx| children(cx)),
        ..Node::default()
    }
}

/// 垂直排列容器（带绝对位置）。
pub fn column_at(x: f32, y: f32, children: impl Fn(&mut NodeContext) + 'static) -> Node {
    Node {
        x,
        y,
        layout: Some(Layout::Column { gap: 8.0 }),
        arg_children_fn: Rc::new(move |cx| children(cx)),
        ..Node::default()
    }
}

/// 间距组件。
pub fn spacer(width: f32, height: f32) -> Node {
    Node {
        width,
        height,
        ..Node::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node::children_nodes;

    #[test]
    fn button_creates_node() {
        let btn = button(|| "Click me".into(), || {});
        assert!(btn.focusable);
        assert_eq!(btn.width, 90.0);
        assert_eq!(btn.height, 32.0);
        assert!(btn.on_click_fn.is_some());
    }

    #[test]
    fn text_creates_node() {
        let t = text(|| "hello".into());
        assert!(!t.focusable);
        assert!(t.on_click_fn.is_none());
    }

    #[test]
    fn row_sets_layout() {
        let r = row(|_| {});
        assert_eq!(r.layout, Some(Layout::Row { gap: 8.0 }));
    }

    #[test]
    fn column_sets_layout() {
        let c = column(|_| {});
        assert_eq!(c.layout, Some(Layout::Column { gap: 8.0 }));
    }

    #[test]
    fn row_offsets_children() {
        let r = row(|cx| {
            cx.add_node(Node {
                width: 50.0,
                height: 20.0,
                ..Node::default()
            });
            cx.add_node(Node {
                width: 60.0,
                height: 20.0,
                ..Node::default()
            });
        });
        let children = children_nodes(&r);
        let offsets = crate::node::layout_offsets(Layout::Row { gap: 8.0 }, &children);
        assert_eq!(offsets[0], (0.0, 0.0));
        assert_eq!(offsets[1], (58.0, 0.0));
    }
}
