//! wy-mve 核心行为测试：构建一次模型 + renderForEach holder 复用。
//!
//! 对齐 Kotlin MVE 语义：`render_root` 构建一次不重建；
//! 动态列表由 `renderForEach` memo 区域按 key 复用 holder，不重复构建。

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use crate::{
    button, children_nodes, column, layout_offsets, node_size, render_root, row, text, Layout,
    Node, NodeContext,
};
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
    let mut cx = NodeContext::new();
    cx.add_node(Node::default());
    cx.add_node(Node::default());
    assert_eq!(cx.nodes().len(), 2);
}

#[test]
fn node_arg_children_builds_subtree() {
    let mut cx = NodeContext::new();
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
    let mut child_cx = NodeContext::new();
    if let crate::ChildSlot::Node(n) = node {
        n.run_arg_children(&mut child_cx);
    }
    assert_eq!(child_cx.nodes().len(), 3);
}

#[test]
fn render_root_creates_root() {
    let root = render_root(|cx| {
        cx.add_node(Node::default());
        cx.add_node(Node::default());
    });
    assert_eq!(root.nodes().len(), 2);
}

/// 构建一次模型：信号变化不重建根子树，callback 只执行一次。
#[test]
fn render_root_builds_once_not_per_draw() {
    let calls = Rc::new(Cell::new(0));
    let root = render_root({
        let calls = calls.clone();
        move |cx| {
            calls.set(calls.get() + 1);
            cx.add_node(Node::default());
        }
    });
    assert_eq!(root.nodes().len(), 1);
    assert_eq!(calls.get(), 1);

    root.nodes();
    assert_eq!(calls.get(), 1, "build-once：重复读取不重建");
}

/// 根节点目标 memo：nodes() 与 target() 等价。
#[test]
fn root_target_matches_nodes() {
    let root = render_root(|cx| {
        cx.add_node(Node::default());
    });
    assert_eq!(root.target().get().len(), root.nodes().len());
}

/// arg_children 惰性构建：首次访问 children 时执行一次，之后复用。
#[test]
fn node_children_materialize_once() {
    let runs = Rc::new(Cell::new(0));
    let node = Node {
        arg_children_fn: Rc::new({
            let runs = runs.clone();
            move |cx| {
                runs.set(runs.get() + 1);
                cx.add_node(Node::default());
                cx.add_node(Node::default());
            }
        }),
        ..Node::default()
    };
    let children = children_nodes(&node);
    assert_eq!(children.len(), 2);
    let children2 = children_nodes(&node);
    assert_eq!(children2.len(), 2);
    assert_eq!(runs.get(), 1, "arg_children 只执行一次（构建一次）");
}

/// Node 相等 = 实例 identity（复刻 Kotlin 引用相等）。
#[test]
fn node_equality_by_identity() {
    let n = Node::default();
    assert_eq!(n, n.clone());
    assert_ne!(n, Node::default());
}

/// row / column 布局容器：渲染期实时计算尺寸与偏移。
#[test]
fn layout_offsets_row_and_column() {
    let row_node = row(|cx| {
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
    assert_eq!(node_size(&row_node), (118.0, 20.0));
    assert_eq!(
        layout_offsets(Layout::Row { gap: 8.0 }, &children_nodes(&row_node)),
        vec![(0.0, 0.0), (58.0, 0.0)]
    );

    let col_node = column(|cx| {
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
    assert_eq!(node_size(&col_node), (50.0, 58.0));
    assert_eq!(
        layout_offsets(Layout::Column { gap: 8.0 }, &children_nodes(&col_node)),
        vec![(0.0, 0.0), (0.0, 28.0)]
    );
}

/// renderForEach：增删列表 → 区域节点数实时变化（复刻 Kotlin DemoList）。
#[test]
fn render_for_each_tracks_signal_changes() {
    let items = create_signal(vec![1, 2, 3]);
    let root = render_root({
        let items = items.clone();
        move |cx| {
            let items = items.clone();
            cx.render_for_each(
                move |emit| {
                    for v in items.get() {
                        emit(v, v);
                    }
                },
                |_k, value, atomic_cx| {
                    let v = *value.borrow();
                    atomic_cx.child(text(move || format!("item {}", v)));
                },
            );
        }
    });

    assert_eq!(root.nodes().len(), 3);

    items.set(vec![1, 2]);
    assert_eq!(root.nodes().len(), 2);

    items.set(vec![5]);
    assert_eq!(root.nodes().len(), 1);
}

/// renderForEach：按 key 复用 holder，creater 只对"新 key"执行；
/// 复用的节点 identity 不变（memo 短路 → 局部重绘的基础）。
#[test]
fn render_for_each_reuses_holders_by_key() {
    let items = create_signal(vec![1, 2, 3]);
    let built = Rc::new(Cell::new(0));
    let root = render_root({
        let items = items.clone();
        let built = built.clone();
        move |cx| {
            let items = items.clone();
            let built = built.clone();
            cx.render_for_each(
                move |emit| {
                    for v in items.get() {
                        emit(v, v);
                    }
                },
                {
                    let built = built.clone();
                    move |_k, value, atomic_cx| {
                        built.set(built.get() + 1);
                        let v = *value.borrow();
                        atomic_cx.child(text(move || format!("item {}", v)));
                    }
                },
            );
        }
    });

    assert_eq!(root.nodes().len(), 3);
    assert_eq!(built.get(), 3, "初建：3 个 key 各构建一次");

    // set 前快照：key=2 的节点
    let before = root.nodes();

    // 换掉 key=2（删除），新增 key=99：只有 99 需要构建，2 被销毁
    items.set(vec![1, 99, 3]);
    let nodes = root.nodes();
    assert_eq!(nodes.len(), 3);
    assert_eq!(built.get(), 4, "仅新 key 99 触发构建");

    // 复用 key 的节点保持 identity 相等（1 和 3 复用；新节点 99 与旧节点 2 不等）
    let after = root.nodes();
    drop(nodes);
    assert_eq!(after[0], before[0], "key=1 复用");
    assert_ne!(after[1], before[1], "key=2 已被销毁，key=99 新建");
    assert_eq!(after[2], before[2], "key=3 复用");
}

/// provide/consume：类型化上下文读取。
#[test]
fn node_provide_consume_context() {
    let mut cx = NodeContext::new();
    cx.provide(42u64);
    cx.provide("hello".to_string());
    assert_eq!(cx.consume::<u64>(), Some(&42));
    assert_eq!(cx.consume::<String>(), Some(&"hello".to_string()));
    assert_eq!(cx.consume::<f32>(), None);
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
fn button_and_text_nodes() {
    let btn = button(|| "hi".into(), || {});
    assert!(btn.focusable);
    assert!(btn.on_click_fn.is_some());
    assert_eq!(btn.width, 90.0);

    let t = text(|| "hi".into());
    assert!(!t.focusable);
    assert!(t.on_click_fn.is_none());
}
