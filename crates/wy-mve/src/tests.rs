use std::cell::{Cell, RefCell};
use std::rc::Rc;

use crate::{render_root, Node, NodeContext};
use wy_signal::{create_signal, GetValue, SetValue};

#[test]
fn node_new_defaults() {
    let node = Node::default();
    assert!(!node.is_focusable());
    assert!(!node.is_hidden());
}

#[test]
fn node_focusable_and_hide() {
    let node = Node {
        focusable: true,
        hidden: true,
        ..Node::default()
    };
    assert!(node.is_focusable());
    assert!(node.is_hidden());
}

#[test]
fn node_clone_shares_rc() {
    let node = Node {
        draw_fn: Rc::new(|_| {}),
        ..Node::default()
    };
    let cloned = node.clone();
    assert!(std::ptr::eq(
        &*node.draw_fn as *const _,
        &*cloned.draw_fn as *const _,
    ));
}

#[test]
fn node_add_node_to_context() {
    let mut cx = NodeContext::new(0);
    cx.add_node(Node::default());
    cx.add_node(Node::default());
    assert_eq!(cx.nodes().len(), 2);
}

#[test]
fn node_arg_children_builds_subtree() {
    let mut cx = NodeContext::new(0);
    cx.add_node(Node {
        arg_children_fn: Rc::new(|child_cx| {
            child_cx.add_node(Node::default());
            child_cx.add_node(Node::default());
            child_cx.add_node(Node::default());
        }),
        ..Node::default()
    });
    assert_eq!(cx.nodes().len(), 1);

    let node = &cx.nodes()[0];
    let mut child_cx = NodeContext::new(0);
    node.run_arg_children(&mut child_cx);
    assert_eq!(child_cx.nodes().len(), 3);
}

#[test]
fn render_root_creates_children_cache() {
    let cache = render_root(|cx| {
        cx.add_node(Node::default());
        cx.add_node(Node::default());
    });
    let nodes = cache.get();
    assert_eq!(nodes.len(), 2);
}

#[test]
fn render_root_tracks_signals() {
    let counter = create_signal(0u32);

    let cache = render_root({
        let counter = counter.clone();
        move |cx| {
            let c = counter.get();
            for _ in 0..c {
                cx.add_node(Node::default());
            }
        }
    });

    assert_eq!(cache.get().len(), 0);

    counter.set(3);
    assert_eq!(cache.get().len(), 3);

    counter.set(1);
    assert_eq!(cache.get().len(), 1);
}

#[test]
fn node_provide_consume_context() {
    let mut cx = NodeContext::new(0);
    cx.provide(42, "hello".to_string());
    assert_eq!(cx.consume::<String>(42), Some(&"hello".to_string()));
    assert_eq!(cx.consume::<String>(99), None);
}

#[test]
fn node_event_types() {
    let clicked = Rc::new(RefCell::new(false));
    let clicked_ref = clicked.clone();

    let node = Node {
        on_click_fn: Some(Rc::new(move |_| {
            *clicked_ref.borrow_mut() = true;
        })),
        ..Node::default()
    };

    let mut event = crate::PointerEvent::new(0.0, 0.0);
    node.run_on_click(&mut event);
    assert!(*clicked.borrow());
}

#[test]
fn node_key_event() {
    let node = Node {
        key_fn: Some(Rc::new(|event| event.key == crate::Key::Enter)),
        ..Node::default()
    };

    let mut event = crate::KeyEvent {
        key: crate::Key::Enter,
        ctrl: false,
        shift: false,
        alt: false,
        meta: false,
    };
    assert!(node.run_key(&mut event));

    let mut event = crate::KeyEvent {
        key: crate::Key::Escape,
        ctrl: false,
        shift: false,
        alt: false,
        meta: false,
    };
    assert!(!node.run_key(&mut event));
}

#[test]
fn children_cache_clone_is_independent() {
    let cache = render_root(|cx| {
        cx.add_node(Node::default());
    });
    let cache2 = cache.clone();
    assert_eq!(cache.get().len(), cache2.get().len());
}

#[test]
fn state_holder_basic() {
    let mut holder = crate::StateHolder::new(|cx| {
        cx.add_node(Node::default());
    });
    assert_eq!(holder.children().len(), 1);

    holder.rebuild(|cx| {
        cx.add_node(Node::default());
        cx.add_node(Node::default());
    });
    assert_eq!(holder.children().len(), 2);
}

#[test]
fn nested_node_tree() {
    let cache = render_root(|cx| {
        cx.add_node(Node {
            arg_children_fn: Rc::new(|child_cx| {
                child_cx.add_node(Node::default());
                child_cx.add_node(Node {
                    arg_children_fn: Rc::new(|grandchild_cx| {
                        grandchild_cx.add_node(Node::default());
                    }),
                    ..Node::default()
                });
            }),
            ..Node::default()
        });
    });

    let nodes = cache.get();
    assert_eq!(nodes.len(), 1);

    let mut child_cx = NodeContext::new(0);
    nodes[0].run_arg_children(&mut child_cx);
    assert_eq!(child_cx.nodes().len(), 2);

    let mut grandchild_cx = NodeContext::new(0);
    child_cx.nodes()[1].run_arg_children(&mut grandchild_cx);
    assert_eq!(grandchild_cx.nodes().len(), 1);
}

// ===== 对齐 Kotlin NodeLazyBuildTest：argChildren 惰性构建 =====

/// arg_children_fn 在 Node 构造时不执行，仅在显式调用 run_arg_children 时执行。
/// 对齐 Kotlin NodeLazyBuildTest.argChildrenNotCalledDuringNodeConstruction。
#[test]
fn arg_children_not_called_during_construction() {
    let call_count = Rc::new(Cell::new(0));
    let node = Node {
        arg_children_fn: Rc::new({
            let call_count = call_count.clone();
            move |_| {
                call_count.set(call_count.get() + 1);
            }
        }),
        ..Node::default()
    };
    assert_eq!(call_count.get(), 0, "构造期间不应调用 arg_children");

    let mut cx = NodeContext::new(0);
    node.run_arg_children(&mut cx);
    assert_eq!(call_count.get(), 1, "显式调用后执行一次");
}

/// render_root 中的 arg_children_fn 仅在 effect 执行时调用。
#[test]
fn render_root_defers_arg_children_to_effect() {
    let call_count = Rc::new(Cell::new(0));
    let cache = render_root({
        let call_count = call_count.clone();
        move |cx| {
            call_count.set(call_count.get() + 1);
            cx.add_node(Node::default());
        }
    });
    assert_eq!(call_count.get(), 1, "render_root effect 立即执行一次");
    assert_eq!(cache.get().len(), 1);
}

/// 信号变化触发 render_root 重建子节点树。
#[test]
fn signal_change_triggers_render_root_rebuild() {
    let count = create_signal(2u32);
    let build_count = Rc::new(Cell::new(0));

    let cache = render_root({
        let count = count.clone();
        let build_count = build_count.clone();
        move |cx| {
            build_count.set(build_count.get() + 1);
            let n = count.get();
            for _ in 0..n {
                cx.add_node(Node::default());
            }
        }
    });

    assert_eq!(cache.get().len(), 2);
    assert_eq!(build_count.get(), 1);

    count.set(4);
    assert_eq!(cache.get().len(), 4);
    assert_eq!(build_count.get(), 2, "信号变化应触发重建");
}

/// StateHolder 支持 rebuild：重建后子节点更新。
#[test]
fn state_holder_rebuild_with_signal() {
    let items = create_signal(vec![1, 2]);
    let mut holder = crate::StateHolder::new({
        let items = items.clone();
        move |cx| {
            for _ in items.get().iter() {
                cx.add_node(Node::default());
            }
        }
    });
    assert_eq!(holder.children().len(), 2);

    items.set(vec![10, 20, 30]);
    holder.rebuild({
        let items = items.clone();
        move |cx| {
            for _ in items.get().iter() {
                cx.add_node(Node::default());
            }
        }
    });
    assert_eq!(holder.children().len(), 3);
}
