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
        layout_offsets(Layout::Row, row_node.gap(), &children_nodes(&row_node)),
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
        layout_offsets(Layout::Column, col_node.gap(), &children_nodes(&col_node)),
        vec![(0.0, 0.0), (0.0, 28.0)]
    );
}

/// 隐藏子节点不占布局空间：容器尺寸排除它，后续兄弟不上移游标。
/// 对齐 Kotlin `hide` 子节点不参与 flex 排布——这是动态条件展示的基础。
#[test]
fn hidden_child_does_not_occupy_layout() {
    let vis = wy_signal::Signal::new(false);
    let v = vis;
    let hidden = Node {
        width: 100.0,
        height: 40.0,
        hide_fn: Some(Rc::new(move || !v.get())),
        ..Node::default()
    };
    let shown = Node {
        width: 50.0,
        height: 10.0,
        ..Node::default()
    };
    let col = column({
        let (hidden, shown) = (hidden, shown);
        move |cx| {
            cx.add_node(hidden.clone());
            cx.add_node(shown.clone());
        }
    });

    // 隐藏项不撑高度：仅 shown 的 10
    assert_eq!(node_size(&col).1, 10.0, "隐藏子节点不占高度");

    let kids = children_nodes(&col);
    let offs = layout_offsets(Layout::Column, col.gap(), &kids);
    // 隐藏项拿到当前游标 (0,0)，但不推进；shown 也在 (0,0)
    assert_eq!(offs[0].1, 0.0);
    assert_eq!(offs[1].1, 0.0, "隐藏项不推进主轴游标");
}

/// renderWhen：条件翻转 → 分支 holder 销毁/重建，区域节点数实时变化。
#[test]
fn render_when_tracks_condition() {
    let show = create_signal(false);
    let root = render_root(move |cx| {
        cx.render_when(
            move || show.get(),
            |cond, b| {
                if cond {
                    b.child(text(|| "A".into()));
                    b.child(text(|| "A2".into()));
                } else {
                    b.child(text(|| "B".into()));
                }
            },
        );
    });

    assert_eq!(root.nodes().len(), 1, "false → else 分支 1 个");

    show.set(true);
    assert_eq!(root.nodes().len(), 2, "true → then 分支 2 个");

    show.set(false);
    assert_eq!(root.nodes().len(), 1, "翻转回 else");
}

/// renderForEach：增删列表 → 区域节点数实时变化（复刻 Kotlin DemoList）。
#[test]
fn render_for_each_tracks_signal_changes() {
    let items = create_signal(vec![1, 2, 3]);
    let root = render_root({
        move |cx| {
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
        let built = built.clone();
        move |cx| {
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

// ============================================================================
// 动态属性（复刻 Kotlin getter 语义）
// ============================================================================

#[test]
fn dynamic_gap_via_fn() {
    let gap = create_signal(4.0f32);
    let n = Node {
        layout: Some(Layout::Column),
        gap_fn: Some(Rc::new(move || gap.get())),
        ..Node::default()
    };
    assert_eq!(n.gap(), 4.0);
    gap.set(20.0);
    assert_eq!(n.gap(), 20.0, "gap_fn 每次求值，信号变化即生效");
}

#[test]
fn dynamic_hide_via_fn() {
    let vis = create_signal(true);
    let n = Node {
        hide_fn: Some(Rc::new(move || !vis.get())),
        ..Node::default()
    };
    assert!(!n.is_hidden());
    vis.set(false);
    assert!(n.is_hidden());
}

#[test]
fn dynamic_position_and_size_via_fn() {
    let x = create_signal(10.0f32);
    let w = create_signal(100.0f32);
    let n = Node {
        x_fn: Some(Rc::new(move || x.get())),
        width_fn: Some(Rc::new(move || w.get())),
        ..Node::default()
    };
    assert_eq!(n.pos_x(), 10.0);
    assert_eq!(n.effective_width(), 100.0);
    x.set(50.0);
    w.set(200.0);
    assert_eq!(n.pos_x(), 50.0);
    assert_eq!(n.effective_width(), 200.0);
    // node_size 走动态值
    assert_eq!(node_size(&n), (200.0, 0.0));
}

#[test]
fn size_fn_overrides_layout_derivation() {
    let n = Node {
        layout: Some(Layout::Column),
        size_fn: Some(Rc::new(|| (300.0, 40.0))),
        ..Node::default()
    };
    assert_eq!(node_size(&n), (300.0, 40.0), "size_fn 优先于子节点累加");
}

#[test]
fn dynamic_gap_drives_layout_offsets() {
    let gap = create_signal(8.0f32);
    let container = Node {
        layout: Some(Layout::Row),
        gap_fn: Some(Rc::new(move || gap.get())),
        arg_children_fn: Rc::new(move |cx| {
            cx.add_node(Node {
                width: 10.0,
                height: 10.0,
                ..Node::default()
            });
            cx.add_node(Node {
                width: 10.0,
                height: 10.0,
                ..Node::default()
            });
        }),
        ..Node::default()
    };
    let children = children_nodes(&container);
    let offsets = layout_offsets(Layout::Row, container.gap(), &children);
    assert_eq!(offsets[1].0, 18.0);
    gap.set(0.0);
    let offsets = layout_offsets(Layout::Row, container.gap(), &children);
    assert_eq!(offsets[1].0, 10.0, "gap 变化后布局偏移立即更新");
}

// ===== flex 排布（grow / justify / align / padding）=====

use crate::{layout_boxes, AlignItem, LayoutBox};

/// 造一个固定尺寸的普通节点。
fn fixed(w: f32, h: f32) -> Node {
    Node {
        width: w,
        height: h,
        ..Node::default()
    }
}

#[test]
fn grow_takes_remaining_main_axis_space() {
    let a = fixed(100.0, 20.0);
    let mut b = fixed(0.0, 20.0);
    b.grow = 1.0;
    let row = crate::hbox_gap(10.0, vec![a.clone(), b.clone()]);
    let boxes = layout_boxes(&row, (400.0, 100.0), &[a, b]);
    assert_eq!(boxes[0].width, 100.0);
    assert_eq!(boxes[0].x, 0.0);
    // 400 - 100 - gap(10) = 290
    assert_eq!(boxes[1].width, 290.0);
    assert_eq!(boxes[1].x, 110.0);
}

#[test]
fn grow_split_by_weight() {
    let mut a = fixed(0.0, 10.0);
    let mut b = fixed(0.0, 10.0);
    a.grow = 1.0;
    b.grow = 3.0;
    let row = crate::hbox_gap(0.0, vec![a.clone(), b.clone()]);
    let boxes = layout_boxes(&row, (400.0, 10.0), &[a, b]);
    assert_eq!(boxes[0].width, 100.0);
    assert_eq!(boxes[1].width, 300.0);
}

#[test]
fn padding_insets_children() {
    let a = fixed(50.0, 20.0);
    let mut row = crate::hbox_gap(0.0, vec![a.clone()]);
    row.padding = 8.0;
    row.align_items = AlignItem::Start;
    let boxes = layout_boxes(&row, (200.0, 100.0), &[a]);
    assert_eq!(boxes[0].x, 8.0);
    assert_eq!(boxes[0].y, 8.0);
}

#[test]
fn align_stretch_fills_cross_axis() {
    let a = fixed(50.0, 20.0);
    let mut row = crate::hbox_gap(0.0, vec![a.clone()]);
    row.align_items = AlignItem::Stretch;
    let boxes = layout_boxes(&row, (200.0, 100.0), &[a]);
    assert_eq!(boxes[0].height, 100.0);
    assert_eq!(boxes[0].y, 0.0);
}

#[test]
fn align_center_is_default_and_explicit_position_wins() {
    // 无显式 y：交叉轴居中
    let a = fixed(50.0, 20.0);
    let row = crate::hbox_gap(0.0, vec![a.clone()]);
    let boxes = layout_boxes(&row, (200.0, 100.0), &[a]);
    assert_eq!(boxes[0].y, 40.0);

    // 有显式 y：自己定位，不参与自动对齐
    let mut b = fixed(50.0, 20.0);
    b.y = 5.0;
    let row = crate::hbox_gap(0.0, vec![b.clone()]);
    let boxes = layout_boxes(&row, (200.0, 100.0), &[b]);
    assert_eq!(boxes[0].y, 0.0);
}

#[test]
fn current_box_is_visible_inside_with_box() {
    let seen = std::cell::Cell::new(LayoutBox::ZERO);
    crate::with_box(LayoutBox::new(0.0, 0.0, 320.0, 240.0), || {
        seen.set(crate::current_box());
    });
    assert_eq!(seen.get().width, 320.0);
    assert_eq!(seen.get().height, 240.0);
    // 作用域外恢复
    assert_eq!(crate::current_box(), LayoutBox::ZERO);
}

#[test]
fn rsx_supports_flex_attrs_and_generic_events() {
    use crate::{rsx, text};
    let slot = rsx! {
        hbox {
            gap: 8.0,
            grow: 1.0,
            padding: 4.0,
            align_items: AlignItem::Stretch,
            on_click: move || {},
            on_hover: move |_entered: bool| {},
            text(|| String::from("x"))
        }
    };
    let node = slot.into_node();
    assert_eq!(node.grow, 1.0);
    assert_eq!(node.padding, 4.0);
    assert_eq!(node.align_items, AlignItem::Stretch);
    assert!(node.on_click_fn.is_some(), "容器应能直接绑点击");
    assert!(node.on_hover_fn.is_some(), "容器应能直接绑悬停");
}
