//! 看板装配（v2）：位置与尺寸全部交给 flex，拖拽改权重而不是改坐标。
//!
//! 结构同 v1：顶栏 + 三列（左：自选/统计；中：图表/深度；右：盘口/成交），
//! 列间两根可拖拽分割条；深度「FULL」时整页切到深度独占子树（不叠浮层）。
//!
//! 本章装配已迁移的面板（topbar / watchlist / stats），chart / book / trades /
//! depth 暂以占位面板呈现，待各自模块迁移后替换。文本一律走 `text_label!` /
//! `wrapped_text_node!` 子节点。

// 基础组件必须排在最前并以 `#[macro_use]` 声明：其 `#[rec]` 生成的 `!` 宏
// （panel! / text_label! / col_header! / kv_row!）才能对后续子模块（stats /
// splitter / topbar / watchlist / 本文件）可见，无需逐文件 `use crate::xxx;`。
#[macro_use]
pub mod widgets;

pub mod book;
pub mod chart;
pub mod depth_view;
pub mod splitter;
pub mod stats;
pub mod theme;
pub mod topbar;
pub mod trades;
pub mod watchlist;

use std::cell::Cell;
use std::rc::Rc;

use wy_mve::canvas_render2::if_node;
use wy_mve::{flex, grow, node2, AlignItem, Ext, Fc, Layout, Node2, NodeContext, Scene};
use wy_render::Rect;
use wy_signal::{GetValue, Signal};

use self::splitter::{DragSpec, SizeProbe};
use crate::{Ctl, Model};

use self::book::{book_panel, BookState};
use self::chart::chart_panel;
use self::depth_view::depth_panel;
use self::stats::stats_panel;
use self::topbar::topbar;
use self::trades::trades_panel;
use self::watchlist::watchlist;
use self::widgets::{grow_all, grow_x, grow_y};

/// 外边距。
const PAD: f32 = 8.0;
/// 三列在行宽里的**占比**（不是像素）：布局侧当 `grow` 权重用，
/// 窗口怎么缩放都保持这个比例，也不存任何绝对宽度。
const LEFT_FRAC: f32 = 0.19;
const RIGHT_FRAC: f32 = 0.21;
/// 三列各自的上下分格比（上半块占比），也是双击分割条的复位值。
///
/// 左列给 0.55：自选列表是主内容（10 个符号一次看全），统计是附属信息。
const LEFT_SPLIT: f32 = 0.55;
const CENTER_SPLIT: f32 = 0.64;
const RIGHT_SPLIT: f32 = 0.62;

/// 三列的宽度与分格比例（可拖拽的量）。
///
/// # 为什么必须在 [`dashboard`] 里造，而不是 [`normal_layout`] 里
///
/// 整块看板挂在 `if_node` → `render_when` 的条件下（FULL 视图切换）。`render_when`
/// 会**跟踪构建期读到的每个信号**，一旦其中之一变化就重跑整个分支闭包——而闭包
/// 里新建的 `Signal` 每次都是全新的一份初始值。于是行情 tick 一进来（或任何被
/// 跟踪的信号变了），拖好的列宽/格高就被抹回默认，表现为"怎么拖都没用"。
///
/// 抬到这一层（`dashboard` 只执行一次）就与 `full` / `book_state` 同命：句柄
/// 只有一份，分支重建时被复制进去、值仍是拖过之后的。`Signal` 是 `Copy`，
/// 所以多个分支闭包各拿一份互不影响。
#[derive(Clone, Copy)]
struct Splits {
    left_frac: Signal<f32>,
    right_frac: Signal<f32>,
    left_split: Signal<f32>,
    center_split: Signal<f32>,
    right_split: Signal<f32>,
}

/// 组装整块看板（v2 入口）：返回一个 `Fc`，由 [`build_root_node2`] 挂到画布根下。
///
/// # 为什么顶层分两层
///
/// `dropdown` / `dialog` 的浮层根尺寸取**父节点 inner**（TS `innerSize(view, d)`）。
/// 页面节点带 `PAD` 内边距，把浮层塞进去可视区就少一圈、遮罩也盖不住整窗。
/// 所以这里有一层**无内边距的整窗盒子**：`[页面(grow), 浮层…]`。
/// 浮层排文档序最后 → 绘在最上层、命中最先命中。
pub fn dashboard(model: Rc<Model>, ctl: Rc<Ctl>, viewport: Rc<Cell<(f32, f32)>>) -> Fc {
    // 深度图是否独占视口：只有装配层持有这个信号，深度面板只读写它。
    let full = Signal::new(false);
    // 可拖拽的列宽 / 分格比同样只造一次（见 [`Splits`] 的文档）。
    let splits = Splits {
        left_frac: Signal::new(LEFT_FRAC),
        right_frac: Signal::new(RIGHT_FRAC),
        left_split: Signal::new(LEFT_SPLIT),
        center_split: Signal::new(CENTER_SPLIT),
        right_split: Signal::new(RIGHT_SPLIT),
    };
    // 盘口状态在这里造：面板与设置菜单必须共享同一份，而菜单要挂到上面那层整窗
    // 盒子里（见函数文档）。这是它被抬出面板的唯一理由。
    let book_state = Rc::new(BookState::new(Rc::clone(&model)));

    let m = Rc::clone(&model);
    let c = Rc::clone(&ctl);

    let f_cond = full;
    let f_a = full;
    let f_b = full;
    let m_a = Rc::clone(&m);
    let m_b = Rc::clone(&m);
    let c_b = Rc::clone(&c);
    let b_page = Rc::clone(&book_state);

    let page = node2! {
        layout: flex! { direction: Layout::Column, align_item: AlignItem::Stretch, align_fix: true },
        padding: PAD,
        debug_name: "dashboard".to_string(),
        exts: vec![grow(1.0)],
        draw: move |s: &mut Scene, n: &Node2| {
            s.fill_rect(Rect::new(0.0, 0.0, n.width(), n.height()), theme::BG);
        },
        children: vec![
            // 两棵子树二选一：正常看板 / 深度图独占（FULL）。
            // if_node 是结构组件，自身不产生节点，只把当前分支挂进 children。
            if_node(
                move || f_cond.get(),
                move |cx| cx.child(full_depth(Rc::clone(&m_a), f_a)),
                Some(move |cx: &mut NodeContext| {
                    // 分支闭包会被**多次**调用（FULL 来回切时重建子树），
                    // 所以捕获的 `Rc` 一律在体内克隆，绝不能把句柄移出去。
                    cx.child(normal_layout(
                        Rc::clone(&m_b),
                        Rc::clone(&c_b),
                        f_b,
                        Rc::clone(&b_page),
                        splits,
                    ))
                }),
            ),
        ],
    };

    let mut children = vec![page];
    children.extend(book::overlays(&book_state));

    let (vw, vh) = (viewport.clone(), viewport);
    node2! {
        debug_name: "finance-root".to_string(),
        // 整窗盒子：**不设内边距**，浮层的可视区才是全窗口。
        // 宽高显式取视口（不靠"未声明就撑满"的特例），与 TS 画布根同款。
        width: vw.get().0,
        height: vh.get().1,
        layout: flex! { direction: Layout::Column, align_item: AlignItem::Stretch, align_fix: true },
        children: children,
    }
}

/// 正常看板：顶栏 + 三列（列间两根分割条）。
fn normal_layout(
    model: Rc<Model>,
    ctl: Rc<Ctl>,
    full: Signal<bool>,
    book: Rc<BookState>,
    sp: Splits,
) -> Fc {
    let (left_frac, left_split) = (sp.left_frac, sp.left_split);
    let (center_split, right_split) = (sp.center_split, sp.right_split);
    let right_frac = sp.right_frac;

    // 两个 `move` 闭包各自会搬走捕获到的 `Rc`，所以一列一份句柄。
    let m_watch = Rc::clone(&model);
    let c_watch = Rc::clone(&ctl);
    let m_stats = Rc::clone(&model);
    let c_stats = Rc::clone(&ctl);
    // 两格的 `grow` 是同一比例的 `a` / `1-a`（`grow_y` 里现算），所以窗口
    // 怎么缩放都不会溢出；比例是信号，拖动分割条时高度自然重排。
    let left = stacked(
        move |cx| {
            cx.child(watchlist(
                Rc::clone(&m_watch),
                Rc::clone(&c_watch),
                grow_y(left_split, true),
            ))
        },
        move |cx| {
            cx.child(stats_panel(
                Rc::clone(&m_stats),
                Rc::clone(&c_stats),
                grow_y(left_split, false),
            ))
        },
        left_split,
        LEFT_SPLIT,
        "col-left",
        // 列宽只给**占比权重**：flex 把 `remaining * 权重 / 权重和` 分给 grow 子项，
        // 三列权重恰好和为 1，于是宽度直接就是「行宽 × 占比」——不存绝对宽度。
        grow_x(move || left_frac.get()),
    );

    // 中列两块面板各要一份 `Rc<Model>`（两个 `move` 闭包，不能共用一份）。
    let m_chart = Rc::clone(&model);
    let m_depth = Rc::clone(&model);
    let f2 = full;
    let center = stacked(
        move |cx| cx.child(chart_panel(Rc::clone(&m_chart), grow_y(center_split, true))),
        move |cx| {
            cx.child(depth_panel(
                Rc::clone(&m_depth),
                f2,
                grow_y(center_split, false),
            ))
        },
        center_split,
        CENTER_SPLIT,
        "col-center",
        // 中间列吃掉其余占比（左右拖走后剩下的都归它）。
        grow_x(move || (1.0 - left_frac.get() - right_frac.get()).max(0.0)),
    );

    let m3 = Rc::clone(&model);
    let b_right = Rc::clone(&book);
    let right = stacked(
        move |cx| cx.child(book_panel(Rc::clone(&b_right), grow_y(right_split, true))),
        move |cx| cx.child(trades_panel(Rc::clone(&m3), grow_y(right_split, false))),
        right_split,
        RIGHT_SPLIT,
        "col-right",
        grow_x(move || right_frac.get()),
    );

    let row_probe: SizeProbe = splitter::probe();
    let rp = Rc::clone(&row_probe);
    // 两个分割条各改一侧占比；上界每次现算（其余列 + 中间列最小占比）。
    let g1 = splitter::gutter(
        left_frac,
        DragSpec::column(1.0, LEFT_FRAC, move || right_frac.get()),
        Rc::clone(&row_probe),
    );
    let g2 = splitter::gutter(
        right_frac,
        DragSpec::column(-1.0, RIGHT_FRAC, move || left_frac.get()),
        Rc::clone(&row_probe),
    );

    // 行级实测尺寸写进 row_probe：分割条把指针位移除以它得到占比增量。
    let body = node2! {
        layout: flex! { direction: Layout::Row, align_item: AlignItem::Stretch, align_fix: true },
        exts: vec![grow(1.0)],
        draw: move |_s: &mut Scene, n: &Node2| {
            rp.set((n.width(), n.height()));
        },
        children: vec![left, g1, center, g2, right],
    };

    // 顶栏在上、三列在下：v1 是同构的两层（`vbox { bar, body }`）。
    node2! {
        layout: flex! { direction: Layout::Column, align_item: AlignItem::Stretch, align_fix: true },
        exts: vec![grow(1.0)],
        children: vec![topbar(model, ctl), body],
    }
}

/// 竖向堆叠两块面板，中间夹一根可拖拽分割条。
///
/// `top` / `bottom` 是「产生一个子节点」的闭包（`cx.child(...)` 调一次）；
/// 两块的 `grow` 是同一比例的 `a` / `1-a`，所以窗口怎么缩放都不会溢出。
/// `col` 是这一列在行里的**宽度权重**（`grow_x`），列宽由 flex 分配而非写死。
/// 容器实测尺寸写进探针，供分割条把指针位移换算成占比增量。
fn stacked(
    top: impl Fn(&mut NodeContext) + 'static,
    bottom: impl Fn(&mut NodeContext) + 'static,
    split: Signal<f32>,
    default: f32,
    tag: &'static str,
    col: Vec<Ext>,
) -> Fc {
    let p = splitter::probe();
    let pp = Rc::clone(&p);
    // 双击复位值用**常量默认**传进来，绝不能在构建期 `split.get()`：
    // 那样会给 `render_when` 的分支闭包挂上一条对拖动信号的依赖，
    // 拖动一次就重跑分支、把刚拖出来的值连同整棵子树一起换掉。
    let g = splitter::gutter(split, DragSpec::split(default), Rc::clone(&p));

    let draw = move |_s: &mut Scene, n: &Node2| {
        pp.set((n.width(), n.height()));
    };
    let children = vec![Fc::new(top), g, Fc::new(bottom)];

    node2! {
        debug_name: tag.to_string(),
        layout: flex! { direction: Layout::Column, align_item: AlignItem::Stretch, align_fix: true },
        exts: col,
        draw: draw,
        children: children,
    }
}

/// 深度图独占视口（「FULL」）：深度面板自己带 FULL/CLOSE 按钮切回来。
///
/// 不再是占位面板——面板本体就是 [`depth_panel`]，`full` 信号由装配层持有，
/// 面板只读写（切换时整棵子树重建，面板私有状态随之复位，与 v1 同）。
fn full_depth(model: Rc<Model>, full: Signal<bool>) -> Fc {
    // 独占视口：整格吃满高度（`grow_all`），深度面板自己带 FULL/CLOSE 按钮切回来。
    Fc::new(move |cx| cx.child(depth_panel(Rc::clone(&model), full, grow_all())))
}
