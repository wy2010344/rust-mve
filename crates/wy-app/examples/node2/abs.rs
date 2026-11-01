//! 绝对坐标 —— **纯 Node2 运行时**（`Node2App`）。
//!
//! 每个盒子都在自己内部画一行读数：`abs=(x, y) size=(w×h)`。
//! 嵌套三层（根 padding 20 → 外层 padding 12 → 中层 10 → 内层 8），
//! 读数应当逐层累加：padding + 兄弟偏移 + 再一层 padding。
//!
//! 与旧 `abs2.rs` 的差别：
//!
//! - 旧：读 `n.geom()`——那是 v1 引擎在绘制前写回共享 cell 的**缓存**；
//! - 新：读 [`Node2::abs_x`] / [`Node2::abs_y`]——沿父链累加 `x()`/`y()` 的
//!   **消息式**求值（TS `absolutePosition`），不需要任何回填，也不怕顺序。
//!
//! 运行：`cargo run -p wy-app --example node2-abs`
//! 出图：`cargo run -p wy-app --example node2-abs -- --snapshot docs/shot-node2-abs.png`

mod theme;

use std::cell::Cell;
use std::rc::Rc;

use theme::*;
use wy_engine::node2_app::Node2App;
use wy_engine::runner::run;
use wy_mve::canvas_render2::build_root_node2;
use wy_mve::{flex, node2, node_of, rec, AlignItem, Fc, Layout, Node2, Ref2, Scene};
use wy_render::{Color, Point, Rect};

/// 自报坐标的盒子：画底色，并在**内容区顶部**插一行读数。
///
/// 读数节点读的是父（本盒子）的 `Node2`，所以展示的是本盒子的绝对位置。
#[rec]
fn abs_box(
    /// 自己的句柄槽：不传就内部新建一个（下层 node2 会写它，读数子节点拿得到）。
    #[props(default = Ref2::new())]
    get: Ref2<Node2>,
    title: &'static str,
    fill: Color,
    #[props(default = 12.0f32)] pad: f32,
    children: Vec<Fc>,
) -> Fc {
    let _ = &get;
    // TS `const node = ref.get();`：读数子节点要读**父**的几何，所以这里只复制槽。
    let node_read = get.clone();
    let readout = node_of(move |_rn: Ref2<Node2>| {
        node2! {
            height: 16.0,
            draw: move |s: &mut Scene, _n: &Node2| {
                let n = node_read.get();
                s.draw_text(
                    Point::new(0.0, 2.0),
                    &format!(
                        "{title}  abs=({:.0}, {:.0})  size=({:.0}×{:.0})",
                        n.abs_x(),
                        n.abs_y(),
                        n.width(),
                        n.height()
                    ),
                    12.0,
                    ACCENT,
                );
            },
        }
    });

    let mut all: Vec<Fc> = vec![readout];
    all.extend(children);
    node2! {
        // 把自己的槽交给下层工厂：读数的子节点要靠它取本盒子的最终几何。
        // 不写这句，槽就一直是空的 —— TS 里对应的正是漏了 `node2.create({..})(ref)`。
        node_ref: get.clone(),
        // `Stretch` 子节点要拿"父级分配的交叉轴尺寸"，前提是容器自己交叉轴
        // 尺寸不是由子节点撑出来的（TS `alignFix`）。不加这句，`StackLayout.size()`
        // 会退化成"取子节点最大交叉轴尺寸"，而子节点又要向容器要 → 父子互问，
        // TS 直接报成环。
        layout: flex! {
            direction: Layout::Column,
            align_item: AlignItem::Stretch,
            align_fix: true,
            gap: 8.0,
        },

        padding: pad,
        draw: move |s: &mut Scene, n: &Node2| {
            s.fill_rect(Rect::new(0.0, 0.0, n.width(), n.height()), fill);
        },
        children: all,
    }
}

/// 一行普通文字。
#[rec]
fn text_line(text: &'static str) -> Fc {
    node2! {
        height: 16.0,
        draw: move |s: &mut Scene, _n: &Node2| {
            s.draw_text(Point::new(0.0, 2.0), text, 12.0, TEXT);
        },
    }
}

/// TS `nodeOf(node => rectNode({ .. }))` 的直接对应：
/// `node_of` 把**自己**的 node 释放给回调，draw 里读自己的最终宽高与绝对位置。
#[rec]
fn self_read_box() -> Fc {
    node_of(|get: Ref2<Node2>| {
        let sink = get.clone();
        node2! {
            node_ref: sink,
            layout: flex! { align_item: AlignItem::Stretch, align_fix: true },
            padding: 10.0,
            draw: move |s: &mut Scene, _n: &Node2| {
                let n = get.get();
                let (w, h) = (n.width(), n.height());
                s.stroke_round_rect(
                    Rect::new(0.5, 0.5, (w - 1.0).max(0.0), (h - 1.0).max(0.0)),
                    8.0,
                    ACCENT,
                    1.0,
                );
                s.draw_text(
                    Point::new(10.0, 8.0),
                    &format!(
                        "nodeOf 自读  abs=({:.0}, {:.0})  size=({:.0}×{:.0})",
                        n.abs_x(),
                        n.abs_y(),
                        n.width(),
                        n.height()
                    ),
                    12.0,
                    TEXT,
                );
            },
            children: vec![
                // 给上面的读数让出 22px
                node2! { height: 22.0 },
                text_line! { text: "node_of 暴露自身 node → 读最终几何" },
            ],
        }
    })
}

/// 组件**内部**想拿自己的 node，**不必**在签名里声明 `get: Ref2<Node2>`——
/// body 里套一层 `node_of` 即可。
#[rec]
fn self_aware_panel() -> Fc {
    node_of(|get: Ref2<Node2>| {
        let sink = get.clone();
        node2! {
            node_ref: sink,
            layout:
                flex! {
                    direction: Layout::Column,
                    align_item: AlignItem::Stretch,
                    align_fix: true,
                    gap: 6.0,
                },

            padding: 10.0,
            draw: move |s: &mut Scene, _n: &Node2| {
                let n = get.get();
                let (w, h) = (n.width(), n.height());
                let green = Color::from_u32(0x7BD88FFF);
                s.stroke_round_rect(
                    Rect::new(0.5, 0.5, (w - 1.0).max(0.0), (h - 1.0).max(0.0)),
                    8.0,
                    green,
                    1.0,
                );
                s.draw_text(
                    Point::new(10.0, 8.0),
                    &format!(
                        "body nodeOf  abs=({:.0}, {:.0})  size=({:.0}×{:.0})",
                        n.abs_x(),
                        n.abs_y(),
                        n.width(),
                        n.height()
                    ),
                    12.0,
                    green,
                );
            },
            children: vec![
                node2! { height: 22.0 },
                text_line! { text: "组件体内 node_of → 拿到组件自己的 node" },
                text_line! { text: "（源码里没有声明 get: Ref2<Node2> 参数）" },
            ],
        }
    })
}

/// 整页：三层嵌套，每层自报绝对坐标。
#[rec]
fn abs_page(viewport: Rc<Cell<(f32, f32)>>) -> Fc {
    // 页面铺满画布：根节点是**默认的绝对布局**（TS `arg.layout ?? absoluteLayoutDirection`），
    // 它不会替子节点分配尺寸（`absoluteLayoutObject.childSize()` 直接抛错），
    // 所以页面必须自己从视口拿 `width`/`height`。
    let (vw, vh) = (viewport.clone(), viewport);
    node2! {
        layout:
            flex! {
                direction: Layout::Column,
                align_item: AlignItem::Stretch,
                align_fix: true,
                gap: 12.0,
            },

        width: vw.get().0,
        height: vh.get().1,
        padding: 20.0,
        draw: move |s: &mut Scene, n: &Node2| {
            s.fill_rect(Rect::new(0.0, 0.0, n.width(), n.height()), BG);
        },
        children: vec![
            struct_rows! {},
            abs_box! {
                title: "外层",
                fill: L1,
                pad: 12.0,
                children: vec![
                    text_line! { text: "外层内容：根 padding 20 → abs x 应为 20" },
                    abs_box! {
                        title: "中层",
                        fill: L2,
                        pad: 10.0,
                        children: vec![
                            text_line! { text: "中层内容：再叠一层 padding 12" },
                            abs_box! {
                                title: "内层",
                                fill: L3,
                                pad: 8.0,
                                children: vec![text_line! { text: "内层内容：再叠一层 padding 10" }],
                            },
                        ],
                    },
                ],
            },
            self_read_box! {},
            self_aware_panel! {},
        ],
    }
}

/// 结构组件演示：`for_each_node` **不产生节点**，直接把 N 行插进父容器的
/// children 里（对应 TS `forEachNode` 里那句 `ctx.renderForEach(..)`）。
#[rec]
fn struct_rows() -> Fc {
    wy_mve::canvas_render2::for_each_node(
        |emit| {
            for i in 0..3u32 {
                emit(i, i);
            }
        },
        |key, _v, cx| {
            let label = format!("结构节点 forEach 行 #{key}（自身没有 Node）");
            cx.child(node2! {
                height: 16.0,
                draw: move |s: &mut Scene, _n: &Node2| {
                    s.draw_text(Point::new(0.0, 2.0), &label, 12.0, TEXT);
                },
            });
        },
    )
}

fn make_app() -> Node2App {
    Node2App::new(move |viewport| {
        let page_vp = viewport.clone();
        build_root_node2(theme::root_arg(viewport), move |cx| {
            cx.child(abs_page! { viewport: page_vp });
        })
    })
    .with_title("wy-ui · node2 · abs")
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    if let Some((path, w, h)) = wy_app::snapshot::parse_snapshot_args() {
        let app = make_app();
        wy_app::snapshot::save_snapshot(&app, &path, w, h).map_err(std::io::Error::other)?;
        return Ok(());
    }

    run(make_app())?;
    Ok(())
}
