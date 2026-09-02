use std::cell::RefCell;
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
