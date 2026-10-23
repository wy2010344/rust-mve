//! 看板装配：位置与尺寸全部交给 flex，拖拽改权重而不是改坐标。
//!
//! 布局结构（`gutter` 为 6px 可拖拽分割条，`grow` 为权重）：
//!
//! ```text
//! root (Column, padding 8)
//! ├── topbar                                 固定 48
//! └── body (Row, grow 1)                     实测尺寸写进 probe，供分割条换算
//!     ├── left   (width = left_w)   Column: watchlist(grow a) | gutter | stats(grow 1-a)
//!     ├── gutter X                  ← 拖动改 left_w
//!     ├── center (grow 1)           Column: chart(grow b) | gutter | depth(grow 1-b)
//!     ├── gutter X                  ← 拖动改 right_w（sign = -1）
//!     └── right  (width = right_w)  Column: book(grow c) | gutter | trades(grow 1-c)
//! ```
//!
//! 这一层就是**纯粹的组装**（对应 React 的 render / ooc 的 `div { ... }`）：
//! 只写「谁是谁的孩子、各自占多少权重」，不写任何坐标。
//! 每个面板都是一个 `#[component]`，在 `rsx!` 里当标签用，可以直接给它挂
//! `grow` / `width` / `draw` 这类通用属性——组件和容器一样是布局节点。
//!
//! 深度图「FULL」时切到另一棵子树（深度图独占视口），不叠浮层：
//! 这样既不需要 z-index，也不用把窗口尺寸塞进组件。

pub mod book;
pub mod chart;
pub mod depth_view;
pub mod splitter;
pub mod stats;
pub mod topbar;
pub mod trades;
pub mod watchlist;
pub mod widgets;

use std::rc::Rc;

use wy_mve::{component, rsx, AlignItem, Node, Slot};
use wy_render::{Rect, Scene};
use wy_signal::{GetValue, Signal};

use crate::ui::{
    book::book_panel, chart::chart_panel, depth_view::depth_panel, stats::stats_panel,
    trades::trades_panel, watchlist::watchlist,
};
use crate::{theme, Ctl, Model};
use widgets::LayoutAttr;
// 模块叫 topbar，函数也叫 topbar——导入时起个别名，`rsx!` 标签里才不歧义
use topbar::topbar as topbar_bar;

/// 外边距。
const PAD: f32 = 8.0;
/// 左右列初始宽度。
const SIDE_W: f32 = 286.0;
const RIGHT_W: f32 = 300.0;

/// 组装整块看板：**ctx 组件**形态——拿到 `ctx` 后先把共享状态 provide 下去。
///
/// `model`（行情数据）与 `ctl`（控制器）是真跨面板的：六个面板都要读行情、
/// 顶栏和自选还要发命令。提供一次，子树里任何深度的组件 `ctx.consume()` 即可，
/// 不必逐层当 prop 传——这就是"必要时用 consume 透传"的那一处必要。
///
/// 反过来，只属于某个面板的状态（K 线视口、盘口点价、深度缩放）**不在这里**，
/// 也**不进 Model**：它们在各自的面板里就近持有。
#[component]
pub fn dashboard(ctx: &mut NodeContext, model: Rc<Model>, ctl: Rc<Ctl>) {
    ctx.provide(Rc::clone(&model));
    ctx.provide(Rc::clone(&ctl));

    // 深度图是否独占视口：只有"切哪棵子树"和深度面板自己读它，
    // 所以就放在装配层，不进 Model。
    let full = Signal::new(false);

    let normal = normal_layout(full);

    let f1 = full;
    let f2 = full;
    // 两棵子树二选一：正常看板（顶栏 + 三列）/ 深度图独占。
    // 用 `when` 而不是叠浮层——不需要 z-index，也不用把窗口尺寸塞进组件。
    //
    // FULL 子树在分支里**现建**：`Slot::Many`（ctx 组件）不可克隆，而分支是
    // `Fn`、每次翻转都会调。代价只是来回切时深度图的缩放回到 1x——
    // 那本来就是面板私有状态。
    let page = rsx! {
        vbox {
            gap: 0.0,
            padding: PAD,
            align_items: AlignItem::Stretch,
            draw: |s: &mut Scene| {
                let b = wy_mve::current_box();
                s.fill_rect(Rect::new(0.0, 0.0, b.width, b.height), theme::BG);
            },

            { wy_mve::when(
                move || !f1.get(),
                move |on, cx| { if on { cx.child(fill(normal.clone())); } },
            ) }
            { wy_mve::when(
                move || f2.get(),
                move |on, cx| { if on { cx.child(full_depth(full)); } },
            ) }
        }
    };
    ctx.child(page);
}

/// 撑满父容器（主轴 grow + 交叉轴 stretch）。
fn fill(mut node: Node) -> Node {
    node.grow = 1.0;
    node.align_self = Some(AlignItem::Stretch);
    node
}

/// 竖向堆叠两块面板，中间夹一根可拖拽分割条。
///
/// 两块都用 `grow` 跟着同一个比例变（`a` 与 `1-a`），
/// 所以窗口怎么缩放都不会溢出。比例由**调用方**在 `rsx!` 标签上写：
///
/// ```ignore
/// stacked(
///     rsx! { watchlist { model: m, grow: split.get() } },
///     rsx! { stats_panel { model: m, grow: (1.0 - split.get()).max(0.0) } },
///     split,
/// )
/// ```
fn stacked(top: Slot, bottom: Slot, split: Signal<f32>) -> Node {
    let probe = splitter::probe();
    let g = splitter::gutter(
        split,
        splitter::DragSpec::height(0.55, 0.18, 0.85),
        Rc::clone(&probe),
    );
    splitter::with_probe(
        rsx! {
            vbox {
                gap: 0.0,
                align_items: AlignItem::Stretch,
                { top }
                { g }
                { bottom }
            }
        }
        .into_node(),
        probe,
    )
}

fn normal_layout(full: Signal<bool>) -> Node {
    let left_w = Signal::new(SIDE_W);
    let right_w = Signal::new(RIGHT_W);
    // 每列两块面板的行距比例：按两者的"内容需求"分，一屏刚好铺满、不留死白。
    let left_split = Signal::new(0.40);
    let center_split = Signal::new(0.64);
    let right_split = Signal::new(0.62);

    // ---- 左列：自选 + 统计 ----
    // 面板不再接 `model` / `ctl`：它们从 `dashboard` provide 的上下文里 consume。
    let left = stacked(
        rsx! {
            watchlist {
                grow: left_split.get(),
            }
        },
        rsx! {
            stats_panel {
                grow: (1.0 - left_split.get()).max(0.0),
            }
        },
        left_split,
    )
    .with_width(left_w);

    // ---- 中列：K 线 + 深度 ----
    let center = fill(stacked(
        rsx! {
            chart_panel {
                grow: center_split.get(),
            }
        },
        rsx! {
            depth_panel {
                // 「FULL」是否打开：装配层持有，深度面板只管读写
                full: full,
                grow: (1.0 - center_split.get()).max(0.0),
            }
        },
        center_split,
    ));

    // ---- 右列：盘口 + 成交 ----
    let right = stacked(
        rsx! {
            book_panel {
                grow: right_split.get(),
            }
        },
        rsx! {
            trades_panel {
                grow: (1.0 - right_split.get()).max(0.0),
            }
        },
        right_split,
    )
    .with_width(right_w);

    // ---- 分割条：拖动改相邻列宽 ----
    let row_probe = splitter::probe();
    let g1 = splitter::gutter(
        left_w,
        splitter::DragSpec::width(1.0, 176.0, 420.0, SIDE_W, move || right_w.get() + 420.0),
        Rc::clone(&row_probe),
    );
    let g2 = splitter::gutter(
        right_w,
        splitter::DragSpec::width(-1.0, 224.0, 520.0, RIGHT_W, move || left_w.get() + 420.0),
        Rc::clone(&row_probe),
    );

    let body = rsx! {
        hbox {
            gap: 0.0,
            align_items: AlignItem::Stretch,
            { left }
            { g1 }
            { center }
            { g2 }
            { right }
        }
    }
    .into_node();
    let body = fill(splitter::with_probe(body, row_probe));

    // ---- 顶栏 ----
    let bar = rsx! {
        topbar_bar {}
    };

    rsx! {
        vbox {
            gap: 0.0,
            padding: 0.0,
            align_items: AlignItem::Stretch,
            { bar }
            { body }
        }
    }
    .into_node()
}

/// 深度图独占视口（「FULL」）。
///
/// 返回 `Slot` 而不是 `Node`：ctx 组件是惰性 slot，`into_node()` 会让它在一个
/// **空白上下文**里立刻执行，`consume` 就取不到 model 了。撑满父容器用标签上的
/// `grow` / `align_self`——它们对 ctx 组件同样生效。
fn full_depth(full: Signal<bool>) -> Slot {
    rsx! {
        depth_panel {
            full: full,
            grow: 1.0,
            align_self: AlignItem::Stretch,
        }
    }
}
