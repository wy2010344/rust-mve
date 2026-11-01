//! 滚动容器 —— **纯 Node2 运行时**（`Node2App`）。
//!
//! 要验证的还是最初那个问题：**列表不写死高度，缩小窗口就能滚**。
//! 滚动视口写 `exts: [grow(1.0)]`，吃掉父容器剩余高度；窗口一变矮，
//! 视口收缩、内容溢出，滚轮/拖滚动条立即生效。
//!
//! 声明式语法与其它 demo 完全一致：`node2!` / `flex!` / 自定义 `#[rec]` 组件
//! 的 `!` 宏，声明层没有 `XxxArg { .. }.of()`。
//!
//! `scroll_content!` / `scroll_bar!` 也直接产 `Fc`：节点句柄走普通参数
//! `node_ref`（对齐 TS `arg.nodeRef`），不传就内部新建，因此不需要 `.of()`。
//!
//! 运行：`cargo run -p wy-app --example node2-scroll`
//! 出图：`cargo run -p wy-app --example node2-scroll -- --snapshot docs/shot-node2-scroll.png`

mod theme;

use std::cell::Cell;
use std::rc::Rc;

use theme::*;
use wy_engine::node2_app::Node2App;
use wy_engine::runner::run;
use wy_mve::canvas_render2::node2::Node2Props;
use wy_mve::canvas_render2::{build_root_node2, create_scroll, for_each_node};
use wy_mve::wrapped_text_node;
use wy_mve::{
    flex, grow, node2, node_of, rec, AlignItem, Axis, Color, CursorType, Fc, Layout, Node2, Ref2,
    Scene,
};
use wy_mve::{scroll_bar, scroll_content};
use wy_render::Rect;
use wy_signal::{GetValue, SetValue, Signal};

/// 行高（定高：滚动内容里每行不随视口伸缩）。
const ROW_H: f32 = 32.0;
/// 初始行数。
const INITIAL_ROWS: u64 = 40;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let count = Signal::new(INITIAL_ROWS);

    if let Some((path, w, h)) = wy_app::snapshot::parse_snapshot_args() {
        let app = make_app(count);
        wy_app::snapshot::save_snapshot(&app, &path, w, h).map_err(std::io::Error::other)?;
        return Ok(());
    }
    run(make_app(count))?;
    Ok(())
}

fn make_app(count: Signal<u64>) -> Node2App {
    Node2App::new(move |viewport| {
        let page_vp = viewport.clone();
        build_root_node2(theme::root_arg(viewport), move |cx| {
            cx.child(page! { count, viewport: page_vp });
        })
    })
    .with_title("wy-ui · node2 · scroll")
}

// ============================================================================
// 页面
// ============================================================================

/// 整页：标题 + 滚动列表（`grow`）+ 底栏。
///
/// 关键就是列表这一层**不声明高度**：`exts: [grow(1.0)]` 让视口在父 `Column`
/// 的主轴上吃掉剩余空间，窗口高度变化直接传导到视口。
#[rec]
fn page(count: Signal<u64>, viewport: Rc<Cell<(f32, f32)>>) -> Fc {
    // 页面铺满画布（同 counter / todo）。
    let (vw, vh) = (viewport.clone(), viewport);
    let count_rows = count;
    let rows = for_each_node(
        move |emit| {
            for i in 0..count_rows.get() {
                emit(i, ());
            }
        },
        move |i, _v, cx| {
            cx.child(row! { index: i });
        },
    );

    let (c_footer, c_add) = (count, count);

    node2! {
        layout: flex! {
            direction: Layout::Column,
            align_item: AlignItem::Stretch,
            align_fix: true,
            gap: 10.0,
        },
        width: vw.get().0,
        height: vh.get().1,
        padding: 14.0,
        draw: |s: &mut Scene, n: &Node2| {
            s.fill_rect(Rect::new(0.0, 0.0, n.width(), n.height()), BG);
        },
        children: vec![
            header! {},
            // 列表区底板：圆角面板 + 内嵌滚动视口。
            // 面板 `grow(1.0)` 吃掉整页剩余高度 → 滚动视口随之伸缩 → 窗口变矮即可滚。
            node2! {
                exts: vec![grow(1.0)],
                layout: flex! { align_item: AlignItem::Stretch, align_fix: true },
                draw: |s: &mut Scene, n: &Node2| {
                    s.fill_round_rect(Rect::new(0.0, 0.0, n.width(), n.height()), 8.0, PANEL);
                },
                children: vec![
                    scroll_view! { gap: 4.0, pad: 8.0, children: vec![rows] },
                ],
            },
            footer! {
                count: c_footer,
                on_add: move || c_add.set(c_add.get() + 20),
            },
        ],
    }
}

/// 标题行：主标题 + 右侧提示。
#[rec]
fn header() -> Fc {
    node2! {
        layout: flex! {
            direction: Layout::Row,
            align_item: AlignItem::Center,
            gap: 10.0,
            // 定高行必须配 `align_fix`：否则 Center 的基准是最高子项（文字行框），
            // 内容会贴在行顶、比行中线高几个像素。
            align_fix: true,
        },
        height: 24.0,
        children: vec![
            label! { text: "滚动容器 · 纯 Node2", size: 15.0 },
            node2! { exts: vec![grow(1.0)] },
            label! { text: "缩小窗口 → 列表立刻可滚", size: 12.0, color: FAINT },
        ],
    }
}

/// 一行：序号 + 文本，悬停高亮 + 底部分隔线。
#[rec]
fn row(index: u64) -> Fc {
    let hover = Signal::new(false);
    node2! {
        layout: flex! {
            direction: Layout::Row,
            align_item: AlignItem::Center,
            gap: 10.0,
            align_fix: true,
        },
        height: ROW_H,
        padding_inline_start: 10.0,
        padding_inline_end: 10.0,
        on_hover: move |entered: bool| hover.set(entered),
        draw: move |s: &mut Scene, n: &Node2| {
            let (w, h) = (n.width(), n.height());
            if hover.get() {
                s.fill_rect(Rect::new(0.0, 0.0, w, h), ROW_ALT);
            }
            s.fill_rect(Rect::new(0.0, h - 1.0, w, 1.0), LINE);
        },
        children: vec![
            label! { text: format!("{:02}", index + 1), size: 12.0, color: ACCENT },
            label! {
                text: format!("第 {} 行内容——拖动滚动条或滚轮滚动", index + 1),
            },
        ],
    }
}

/// 底栏：总行数 + 「增加 20 行」按钮（证明内容尺寸变化后滚动上限实时跟随）。
#[rec]
fn footer(count: Signal<u64>, #[props(required)] on_add: impl Fn() + 'static) -> Fc {
    let count_text = count;
    node2! {
        layout: flex! {
            direction: Layout::Row,
            align_item: AlignItem::Center,
            gap: 10.0,
            align_fix: true,
        },
        height: 30.0,
        children: vec![
            label! { text: format!("共 {} 行", count_text.get()), size: 12.0, color: MUTED },
            node2! { exts: vec![grow(1.0)] },
            btn! { text: "增加 20 行", on_click: move || on_add() },
        ],
    }
}

// ============================================================================
// 通用小件
// ============================================================================

/// 文本标签：`text` / `size` / `color` 全是 getter（裸值或活闭包都收）。
#[rec]
fn label(
    #[getter]
    #[props(default = String::new())]
    text: String,
    #[getter]
    #[props(default = 13.0f32)]
    size: f32,
    #[getter]
    #[props(default = TEXT)]
    color: Color,
) -> Fc {
    wrapped_text_node! {
        text: text(),
        font_size: size(),
        color: color(),
    }
}

/// 按钮：accent 底板自绘 + 文本子节点撑尺寸。
#[rec]
fn btn(
    #[getter]
    #[props(default = String::new())]
    text: String,
    #[props(required)] on_click: impl Fn() + 'static,
) -> Fc {
    node2! {
        layout: flex! {
            direction: Layout::Row,
            align_item: AlignItem::Center,
        },
        padding_inline_start: 12.0,
        padding_inline_end: 12.0,
        padding_block_start: 6.0,
        padding_block_end: 6.0,
        cursor: CursorType::Pointer,
        mouse_click: move |_e| on_click(),
        draw: |s: &mut Scene, n: &Node2| {
            s.fill_round_rect(Rect::new(0.0, 0.0, n.width(), n.height()), 6.0, ACCENT);
        },
        children: vec![label! { text: text(), size: 12.5, color: BG }],
    }
}

/// 滚动视口：容器 + 内容 + 滚动条，三段都用 canvasRender2 的原语拼。
///
/// TS 侧这段也是调用方手写的（`createScroll` → `scrollContent` + `scrollBar`），
/// 库不提供"一行搞定"的封装——容器的方向/内边距/间距属于**业务声明**，
/// 不该有隐藏默认值。所以这里把它收成一个本地组件，参数仍然全在明面上。
///
/// 容器需要**先有身份**（`createScroll(container, ..)` 拿着它），再往里挂
/// 内容/滚动条，所以走 `node_of` 而不是 `node2!`：`node_of` 把一个 `Ref2`
/// 交给回调，回调用它建出节点、最后写回槽。
#[rec]
fn scroll_view(
    #[props(default = 4.0f32)] gap: f32,
    #[props(default = 8.0f32)] pad: f32,
    children: Vec<Fc>,
) -> Fc {
    node_of(move |get: Ref2<Node2>| {
        let node = Node2::empty();
        let scroll = create_scroll(&node, Axis::Y);
        let content = scroll_content! {
            scroll: scroll.clone(),
            // 内容：纵向排布 + 交叉轴撑满 + 主轴伸展（= 视口宽）。
            layout: flex! { align_item: AlignItem::Stretch, align_fix: true, gap: gap },
            exts: vec![grow(1.0)],
            // 内边距放在**内容**上：容器的 `y` 被 `scrollContent` 写成
            // `-scrollValue()`（显式位置，不经 padding 补偿），容器自己的
            // padding 会被这段位移吃掉，行首被裁掉一截。
            padding_inline_start: pad,
            padding_inline_end: pad,
            padding_block_start: pad,
            padding_block_end: pad,
            children: children,
        };
        // `scroll` 在 content 处已 clone，这里 move 进 bar（二者各持一份句柄）。
        let bar = scroll_bar! {
            scroll: scroll,
        };
        // 滚轮：滚动内容节点已通过 `scroll_content` 挂 `mouseWheel`（TS `scrollWheel`），
        // 沿命中链到达内容区时由本节点消费并拦住外层（嵌套滚动"内层优先"）。无需额外注册。

        // 容器：纵向滚动 → 布局取 Row（滚动轴落在交叉轴，见
        // `canvas_render2::scroll` 模块注释），`clip` 把子节点溢出裁到 inner 区。
        let node = node.build(Node2Props {
            layout: flex! {
                direction: Layout::Row,
                align_item: AlignItem::Stretch,
                align_fix: true,
            },
            exts: vec![grow(1.0)],
            clip: true,
            children: vec![content, bar],
            ..Node2Props::default()
        });
        get.set(node.clone());
        Fc::from(node)
    })
}
