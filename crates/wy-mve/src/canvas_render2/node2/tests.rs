//! `Node2` 的单元测试。

use std::cell::Cell;
use std::rc::Rc;

use wy_render::Scene;

use crate::canvas_render2::fc::Fc;
use crate::context::NodeContext;

use super::*;

/// `build_root_fc` 测试用的根底色（只作断言记号，不参与任何视觉约定）。
const ROOT_BG: wy_render::Color = wy_render::Color::rgba(1, 2, 3, 255);

/// `ignore_flex` 必须落到布局判据上（`not_in_layout()`）。
///
/// 两个来源（节点字段与 `exts`）合进同一个判据：`layout_children()` 与 flex
/// 排布都要看到一致的结果，否则 `ignore_flex(true)` 只影响其中一条路径。
#[test]
fn ignore_flex_reaches_layout() {
    let n = Node2::empty().build(Node2Props {
        exts: vec![ignore_flex(true)],
        ..Node2Props::default()
    });
    assert!(
        n.not_in_layout(),
        "ignore_flex(true) 必须让布局跳过该子节点"
    );

    let n = Node2::empty().build(Node2Props {
        exts: vec![ignore_flex(false)],
        ..Node2Props::default()
    });
    assert!(!n.not_in_layout());
}

/// 读取式：`ignore_flex_dyn` 每次求值，不在构造时冻结
/// （TS 的 `conv.ignore(n)` 是布局期调用 `it.ignore()`）。
#[test]
fn ignore_flex_is_dynamic() {
    let flag = Rc::new(Cell::new(false));
    let read = Rc::clone(&flag);
    let n = Node2::empty().build(Node2Props {
        exts: vec![ignore_flex_dyn(move || read.get())],
        ..Node2Props::default()
    });
    assert!(!n.not_in_layout());
    flag.set(true);
    assert!(n.not_in_layout());
}

/// `ignoreFlex` 与 `notInLayout` 是两个独立来源，任一为真都生效。
#[test]
fn ignore_flex_or_not_in_layout() {
    let n = Node2::empty().build(Node2Props {
        exts: vec![ignore_flex(true)],
        not_in_layout: always(false),
        ..Node2Props::default()
    });
    assert!(n.not_in_layout());

    let n = Node2::empty().build(Node2Props {
        exts: vec![ignore_flex(false)],
        not_in_layout: always(true),
        ..Node2Props::default()
    });
    assert!(n.not_in_layout());
}

// ---- `tree/geometry.ts` 纯消息算法（命中 / 绝对坐标 / 祖父裁剪）----

/// 固定尺寸的叶子节点（声明层数据，挂载即可用，不依赖引擎布局）。
fn leaf(w: f32, h: f32) -> Node2Props {
    Node2Props {
        width: Some(Rc::new(move || w)),
        height: Some(Rc::new(move || h)),
        ..Node2Props::default()
    }
}

/// 子节点构造体：挂载时把 `arg` 建好并挂到当前父（同 `mount` 的挂载路径）。
fn child_fc(arg: Node2Props) -> Fc {
    Fc::new(move |cx| {
        let n = Node2::empty().build(arg);
        n.mount(cx);
    })
}

/// 建一个节点（不展开子节点）。
fn build_root(arg: Node2Props) -> Node2 {
    Node2::empty().build(arg)
}

/// 把 `arg` 建成的子节点挂到 `parent` 下（设置 `cx.node2_parent` 后展开 Fc，
/// 走与引擎挂载一致的 `attach_to`）。
fn attach_child(parent: &Node2, arg: Node2Props) {
    let mut cx = NodeContext::new();
    cx.node2_parent = Some(parent.clone());
    child_fc(arg).mount(&mut cx);
}

// ---- `children()` 已过滤 hide（`canvasCore/List.ts` 的 flatten + collectIndex）----

#[test]
fn children_filters_hide_and_backfills_index() {
    // 三个子节点，中间那个 hide —— TS 侧 `flatten` 在构建期就把它剔掉了，
    // 于是 `children()` 只剩两个，且 `_index` 是 0/1（跳过隐藏项重新编号）。
    let root = build_root(leaf(300.0, 300.0));
    let mut hidden = leaf(10.0, 10.0);
    hidden.hide = Rc::new(|| true);
    attach_child(&root, leaf(10.0, 10.0));
    attach_child(&root, hidden);
    attach_child(&root, leaf(10.0, 10.0));

    // 原始挂载列表仍是 3 个
    assert_eq!(root.raw_children().len(), 3);
    // 消息层只有 2 个，且不含隐藏的那个
    let kids = root.children();
    assert_eq!(kids.len(), 2);
    assert!(kids.iter().all(|c| !c.hide()));
    // collectIndex 按过滤后的列表重排
    assert_eq!(kids[0].index(), 0);
    assert_eq!(kids[1].index(), 1);
}

#[test]
fn layout_children_only_filters_not_in_layout() {
    // TS `layoutChildren()` = `children().filter(n => !n.notInLayout())`：
    // 只滤 `notInLayout`，`hide` 归 `children()` 管——这里再滤一次就重复了。
    let root = build_root(leaf(300.0, 300.0));
    let mut out_of_layout = leaf(10.0, 10.0);
    out_of_layout.not_in_layout = Rc::new(|| true);
    attach_child(&root, leaf(10.0, 10.0));
    attach_child(&root, out_of_layout);
    attach_child(&root, leaf(10.0, 10.0));

    assert_eq!(root.children().len(), 3);
    let layout_kids = root.layout_children();
    assert_eq!(layout_kids.len(), 2);
    assert_eq!(layout_kids[0].layout_index(), 0);
    assert_eq!(layout_kids[1].layout_index(), 1);
}

// ---- Node2 原生渲染入口（脱离 v1 引擎的独立绘制）----

/// `render_node2` 应按 TS `nodeShell.draw` 递归：先画自身，再逐个子节点
/// `push_transform(child.x(), child.y())` 后画子节点。新模式不再依赖 v1
/// 引擎回填的 `geom`——子节点坐标直接来自实时几何。
#[test]
fn render_node2_draws_self_then_children() {
    let mut root_a = leaf(200.0, 200.0);
    root_a.x = Some(Rc::new(|| 0.0));
    root_a.y = Some(Rc::new(|| 0.0));
    let root_drew = Rc::new(Cell::new(false));
    let rr = Rc::clone(&root_drew);
    root_a.draw = Some(Rc::new(move |_s, _n| rr.set(true)));
    let root = build_root(root_a);

    let mut c1 = leaf(50.0, 50.0);
    c1.x = Some(Rc::new(|| 10.0));
    c1.y = Some(Rc::new(|| 20.0));
    let c1_drew = Rc::new(Cell::new(false));
    let c1r = Rc::clone(&c1_drew);
    c1.draw = Some(Rc::new(move |_s, _n| c1r.set(true)));
    attach_child(&root, c1);

    let mut scene = Scene::new();
    root.render_node2(&mut scene);

    assert!(root_drew.get(), "根节点自身 draw 应被调用");
    assert!(c1_drew.get(), "子节点 draw 应被递归调用（按实时几何定位）");
}

/// 纯 Node2 组装：`build_root_node2` + `expand_node2` 能造出可布局、可绘制的树。
#[test]
fn build_root_node2_smoke() {
    let root = build_root_node2(
        Node2Props {
            x: Some(always(0.0)),
            y: Some(always(0.0)),
            width: Some(always(200.0)),
            height: Some(always(100.0)),
            ..Node2Props::default()
        },
        |cx| {
            cx.child(Fc::from(Node2::empty().build(Node2Props {
                x: Some(always(0.0)),
                y: Some(always(0.0)),
                width: Some(always(50.0)),
                height: Some(always(20.0)),
                ..Node2Props::default()
            })));
        },
    );

    assert_eq!(root.children().len(), 1, "页面应挂在根下");
    let page = &root.children()[0];
    assert_eq!((page.width(), page.height()), (50.0, 20.0));

    let mut scene = Scene::new();
    root.render_node2(&mut scene);
    assert!(scene.iter().count() > 0, "纯 Node2 树应能直接出图元");
}

/// 声明式根：`build_root_fc` 把 `node2!` 声明的一棵树直接变成画布根。
///
/// 三条断言分别锁住"根"的三个定义性事实：
/// 1. 返回的节点**没有父**（探针父已经摘掉）——否则 `absolute_position` 会多算
///    一层偏移，命中测试也会从探针开始走；
/// 2. 子节点已经展开（`expand_node2`），不是待展开的 `child_fcs`；
/// 3. 能直接出图元。
#[test]
fn build_root_fc_makes_a_parentless_expanded_root() {
    let root = build_root_fc(node2! {
        x: 0.0,
        y: 0.0,
        width: 200.0,
        height: 100.0,
        draw: |s: &mut Scene, n: &Node2| {
            s.fill_rect(wy_render::Rect::new(0.0, 0.0, n.width(), n.height()), ROOT_BG);
        },
        children: vec![node2! {
            x: 0.0,
            y: 0.0,
            width: 50.0,
            height: 20.0,
        }],
    });

    assert!(
        root.parent().is_none(),
        "根不能留着探针父（会多算一层偏移）"
    );
    assert_eq!(root.children().len(), 1, "子节点应已展开成活树");
    assert_eq!((root.width(), root.height()), (200.0, 100.0));

    let mut scene = Scene::new();
    root.render_node2(&mut scene);
    assert!(
        scene
            .iter()
            .any(|p| matches!(p, wy_render::Primitive::Rect { color, .. } if *color == ROOT_BG)),
        "根自己的 draw 应画出图元"
    );
}

/// **认父时序回归**：`children` 表达式里用 `.of()` 提前造出来的子树，
/// 必须挂到**直接父节点**上。
///
/// TS 的 `node2` 工厂在拿到 `ctx` 那一刻（挂载期）才 `node.parent = ctx.getParent()`；
/// 若在 `build` 里就认父，`.of()` 那一刻父节点自己还没进 `ParentScope`，
/// 取到的是**祖父**——三层嵌套时最内层会被提级挂到根上。
#[test]
fn build_root_node2_nested_children_attach_to_immediate_parent() {
    fn boxed(w: f32, h: f32, kids: Vec<Fc>) -> Node2 {
        Node2::empty().build(Node2Props {
            width: Some(always(w)),
            height: Some(always(h)),
            children: kids,
            ..Node2Props::default()
        })
    }

    let leaf = boxed(10.0, 10.0, vec![]);
    let mid = boxed(100.0, 40.0, vec![Fc::from(leaf.clone())]);
    let outer = boxed(180.0, 80.0, vec![Fc::from(mid.clone())]);

    let outer_in_tree = outer.clone();
    let root = build_root_node2(
        Node2Props {
            x: Some(always(0.0)),
            y: Some(always(0.0)),
            width: Some(always(200.0)),
            height: Some(always(100.0)),
            ..Node2Props::default()
        },
        move |cx| cx.child(Fc::from(outer_in_tree)),
    );

    assert_eq!(root.children().len(), 1, "根只应有一个孩子（外层）");
    assert_eq!(root.children()[0].id(), outer.id());
    assert_eq!(outer.children().len(), 1, "外层：`mid` 不许被提级到根");
    assert_eq!(outer.children()[0].id(), mid.id());
    assert_eq!(mid.children().len(), 1, "中层：`leaf` 不许被提级");
    assert_eq!(mid.children()[0].id(), leaf.id());

    // 父子链也要对（`abs_x` / `hit_path` / `layout_index` 全靠它）
    assert_eq!(outer.parent().expect("外层应有父").id(), root.id());
    assert_eq!(mid.parent().expect("中层应有父").id(), outer.id());
    assert_eq!(leaf.parent().expect("内层应有父").id(), mid.id());
    assert_eq!(mid.layout_index(), 0);
    assert_eq!(leaf.layout_index(), 0);
}

/// 布局是**一条策略句柄**（Kotlin `LayoutDirection`），不是二选一的枚举。
///
/// 缺省就地是绝对定位：`dir()` 给中性主轴、`gap()` 为 0、没有 flex 配置——
/// 既不藏在 `Option` 里，也不需要调用方先判空再分支。
#[test]
fn layout_dir_default_is_absolute() {
    let d = LayoutDir2::default();
    assert_eq!(d.dir(), Layout::Column);
    assert_eq!(d.gap(), 0.0);
    assert!(d.flex().is_none(), "绝对定位没有 flex 配置");
    assert_eq!(format!("{d:?}"), "AbsoluteLayoutDir");
}

/// `flex!` 产出的同样是一条策略句柄：主轴方向 / 间距 / flex 配置都问得出来。
#[test]
fn flex_dir_is_a_policy_handle() {
    let d = flex! {
        direction: Layout::Row,
        gap: 6.0,
    };
    assert_eq!(d.dir(), Layout::Row);
    assert_eq!(d.gap(), 6.0);
    let f = d.flex().expect("flex 布局应能取到配置");
    assert_eq!((f.direction)(), Layout::Row);
}

/// **裁剪顺序回归**：`clip` 只夹子节点，**不夹自己**。
///
/// TS 的裁剪来自子节点的 `drawAtParent`（`scrollContent` 在父的绘制循环里、
/// `translate` 之前 `ctx.clip()`），容器的背景/边框在自己的 `draw` 里画、发生
/// 在那之前 —— 所以卡片永远完整。早期实现把 `push_clip` 放在父循环里包住整个
/// `render_node2`，于是带 `clip` 的卡片被自己的内盒裁掉：画面上就是"卡片不见了、
/// 条目直接浮在页面上"（`canvas2-todo` 快照里就是这么发现的）。
///
/// 断言用**图元顺序**而不是像素：卡片底色必须出现在 `ClipPush` 之前。
#[test]
fn clip_bounds_children_but_not_self() {
    use wy_render::{Color, Primitive, Rect};

    const CARD: Color = Color::rgba(255, 255, 255, 255);
    const ROW: Color = Color::rgba(10, 20, 30, 255);

    // 溢出内盒的行走节点：内容应被裁（图元仍在，但位于 ClipPush 之后）
    let row = Node2::empty().build(Node2Props {
        width: Some(always(400.0)),
        height: Some(always(400.0)),
        draw: Some(Rc::new(|s: &mut Scene, n: &Node2| {
            s.fill_rect(Rect::new(0.0, 0.0, n.width(), n.height()), ROW);
        })),
        ..Node2Props::default()
    });

    let card = Node2::empty().build(Node2Props {
        width: Some(always(200.0)),
        height: Some(always(100.0)),
        padding: Some(always(10.0)),
        clip: true,
        draw: Some(Rc::new(|s: &mut Scene, n: &Node2| {
            s.fill_rect(Rect::new(0.0, 0.0, n.width(), n.height()), CARD);
        })),
        children: vec![Fc::from(row)],
        ..Node2Props::default()
    });

    // `build` 只是把 `children` 记成待展开的 `Fc`；进树时才摊开（TS 的
    // `expandNode2`）。不展开的话子节点压根没挂上，绘制期自然一个图元都没有。
    expand_node2(&card);

    let mut scene = Scene::new();
    card.render_node2(&mut scene);

    let idx = |pred: &dyn Fn(&Primitive) -> bool| scene.iter().position(|p| pred(p));
    let card_at = idx(&|p| matches!(p, Primitive::Rect { color, .. } if *color == CARD))
        .expect("卡片自身底色必须画出来（被自己裁掉过就是这条挂了）");
    let clip_at = idx(&|p| matches!(p, Primitive::ClipPush { .. })).expect("应压入子节点裁剪");
    let row_at = idx(&|p| matches!(p, Primitive::Rect { color, .. } if *color == ROW))
        .expect("行节点底色应画出来");

    assert!(
        card_at < clip_at,
        "卡片底色必须在裁剪之前（clip 只夹子节点）"
    );
    assert!(clip_at < row_at, "子节点图元必须在裁剪之内");

    // 裁剪盒 = 内盒（扣掉四向 padding），且出栈次数与入栈一致
    let inner = scene
        .iter()
        .find_map(|p| match p {
            Primitive::ClipPush { rect } => Some(*rect),
            _ => None,
        })
        .expect("有 ClipPush");
    assert_eq!(
        (inner.x, inner.y, inner.width, inner.height),
        (10.0, 10.0, 180.0, 80.0)
    );

    let pushes = scene
        .iter()
        .filter(|p| matches!(p, Primitive::ClipPush { .. }))
        .count();
    let pops = scene
        .iter()
        .filter(|p| matches!(p, Primitive::ClipPop))
        .count();
    assert_eq!(pushes, pops, "裁剪栈必须配平");
}

/// **`clip_to_parent` 回归**：子节点要求「裁到**父**内盒」时，裁剪发生在
/// `translate` 之前、用父的坐标，且只影响这一支（后面的兄弟不受影响）。
///
/// 这是 TS `ScrollContent.drawAtParent` 的落点（`assemble.ts` 的
/// `save → drawAtParent → translate → draw → restore`）。写成「自己的内盒裁后代」
/// 会差一个坐标系：滚动内容自己的高度是 600、视口只有 300，拿自己的框裁等于没裁。
#[test]
fn clip_to_parent_clips_this_branch_to_parent_inner() {
    use wy_render::{Color, Primitive, Rect};

    const CARD: Color = Color::rgba(9, 9, 9, 255);
    const INSIDE: Color = Color::rgba(1, 2, 3, 255);
    const AFTER: Color = Color::rgba(4, 5, 6, 255);

    let paint = |color: Color, w: f32, h: f32| {
        Rc::new(move |s: &mut Scene, _n: &Node2| {
            s.fill_rect(Rect::new(0.0, 0.0, w, h), color);
        })
    };

    // 溢出视口的滚动内容：自身 400×400，要求按父内盒裁剪
    let content = Node2::empty().build(Node2Props {
        width: Some(always(400.0)),
        height: Some(always(400.0)),
        clip_to_parent: true,
        draw: Some(paint(INSIDE, 400.0, 400.0)),
        ..Node2Props::default()
    });
    // 兄弟节点：不该被上面那次裁剪影响
    let sibling = Node2::empty().build(Node2Props {
        width: Some(always(400.0)),
        height: Some(always(400.0)),
        draw: Some(paint(AFTER, 400.0, 400.0)),
        ..Node2Props::default()
    });
    let card = Node2::empty().build(Node2Props {
        width: Some(always(200.0)),
        height: Some(always(100.0)),
        padding: Some(always(10.0)),
        draw: Some(paint(CARD, 200.0, 100.0)),
        children: vec![Fc::from(content), Fc::from(sibling)],
        ..Node2Props::default()
    });
    expand_node2(&card);

    let mut scene = Scene::new();
    card.render_node2(&mut scene);

    let idx = |pred: &dyn Fn(&Primitive) -> bool| scene.iter().position(|p| pred(p));
    let card_at =
        idx(&|p| matches!(p, Primitive::Rect { color, .. } if *color == CARD)).expect("卡片底色");
    let clip_at = idx(&|p| matches!(p, Primitive::ClipPush { .. })).expect("应压入父盒裁剪");
    let inside_at =
        idx(&|p| matches!(p, Primitive::Rect { color, .. } if *color == INSIDE)).expect("内容");
    let pop_at = idx(&|p| matches!(p, Primitive::ClipPop)).expect("应弹出裁剪");
    let after_at =
        idx(&|p| matches!(p, Primitive::Rect { color, .. } if *color == AFTER)).expect("兄弟");

    assert!(card_at < clip_at, "父自身绘制不受这次裁剪影响");
    assert!(clip_at < inside_at, "内容图元必须在裁剪之内");
    assert!(inside_at < pop_at, "内容画完才出栈");
    assert!(pop_at < after_at, "裁剪只夹这一支：后面的兄弟在栈外");

    // 裁剪盒用的是**父**内盒（padding 10 → 200-20 × 100-20）
    let inner = scene
        .iter()
        .find_map(|p| match p {
            Primitive::ClipPush { rect } => Some(*rect),
            _ => None,
        })
        .expect("有 ClipPush");
    assert_eq!(
        (inner.x, inner.y, inner.width, inner.height),
        (10.0, 10.0, 180.0, 80.0),
        "clip_to_parent 必须用父的内盒，不是自己的盒"
    );

    let pushes = scene
        .iter()
        .filter(|p| matches!(p, Primitive::ClipPush { .. }))
        .count();
    let pops = scene
        .iter()
        .filter(|p| matches!(p, Primitive::ClipPop))
        .count();
    assert_eq!(pushes, pops, "裁剪栈必须配平");
}
