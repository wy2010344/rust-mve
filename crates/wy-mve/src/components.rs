//! 声明式组件辅助函数：text / button / row / column。
//!
//! 用户通过组合这些函数构建 UI，不需要手动写 draw/hit_test/event。

use std::rc::Rc;

use crate::context::NodeContext;
use crate::node::Node;
use wy_render::{Color, Point, Rect, Scene};

/// 文本组件：闭包返回文本内容。
///
/// 闭包在 draw 时执行，内部读取信号会自动追踪。
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

/// 信号文本：闭包返回值自动 Display。
///
/// 信号读取在 tree 构造阶段执行（被 tracker.collect 追踪），
/// draw 阶段从缓存读取，不重新读信号。
pub fn text_signal(value: impl Fn() -> Box<dyn std::fmt::Display> + 'static) -> Node {
    // 构造时读取信号值（在 tracker.collect 阶段，被追踪为依赖）
    let text = format!("{}", value());
    let cache_key = crate::signal_cache::next_key();
    crate::signal_cache::cache_signal_value(cache_key, text);

    Node {
        draw_fn: Rc::new(move |scene| {
            if let Some(scene) = scene.downcast_mut::<Scene>() {
                if let Some(t) = crate::signal_cache::get_cached_signal::<String>(cache_key) {
                    if !t.is_empty() {
                        scene.draw_text(Point::new(0.0, 0.0), &t, 14.0, Color::BLACK);
                    }
                }
            }
        }),
        ..Node::default()
    }
}

/// 按钮组件：标签文本 + 点击回调。
///
/// 内置：hover/pressed 视觉反馈、焦点支持、Enter/Space 键盘触发。
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
        on_click_fn: Some(Rc::new(move |_| {
            on_click2();
        })),
        ..Node::default()
    }
}

/// 水平排列容器。
pub fn row(children: impl Fn(&mut NodeContext) + 'static) -> Node {
    Node {
        arg_children_fn: Rc::new(move |cx| {
            children(cx);
            let mut offset_x = 0.0;
            for node in cx.nodes_mut().iter_mut() {
                node.x = offset_x;
                offset_x += node.width + 8.0;
            }
        }),
        ..Node::default()
    }
}

/// 垂直排列容器。
pub fn column(children: impl Fn(&mut NodeContext) + 'static) -> Node {
    Node {
        arg_children_fn: Rc::new(move |cx| {
            children(cx);
            let mut offset_y = 0.0;
            for node in cx.nodes_mut().iter_mut() {
                node.y = offset_y;
                offset_y += node.height + 8.0;
            }
        }),
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

    #[test]
    fn node_has_layout_fields() {
        let mut node = Node::default();
        assert_eq!(node.x, 0.0);
        assert_eq!(node.width, 0.0);
        node.x = 10.0;
        node.width = 100.0;
        assert_eq!(node.x, 10.0);
        assert_eq!(node.width, 100.0);
    }

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
    fn row_arranges_children() {
        let mut cx = NodeContext::new(0);
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
        r.run_arg_children(&mut cx);
        let nodes = cx.nodes();
        assert_eq!(nodes[0].x, 0.0);
        assert_eq!(nodes[1].x, 58.0);
    }

    #[test]
    fn column_arranges_children() {
        let mut cx = NodeContext::new(0);
        let c = column(|cx| {
            cx.add_node(Node {
                width: 50.0,
                height: 20.0,
                ..Node::default()
            });
            cx.add_node(Node {
                width: 50.0,
                height: 30.0,
                ..Node::default()
            });
        });
        c.run_arg_children(&mut cx);
        let nodes = cx.nodes();
        assert_eq!(nodes[0].y, 0.0);
        assert_eq!(nodes[1].y, 28.0);
    }
}
