//! 成交面板：每条成交是一个行节点。
//!
//! 列表用 `for_each_node` 按 id 复用，行内绘制只读自身数据。
//! 行高**固定**（对齐 wy-helper 成交行 20px），整体进滚动容器——
//! 不再截断到固定条数：新成交从顶部涌入，旧的往下滚。
//!
//! 与 v1 的差别：列文字由 [`cols_row`] 产出**真实文本子节点**（不再用
//! `Scene::draw_text` 手绘），行骨架仍 [`list_row`]。

use std::cell::RefCell;
use std::rc::Rc;

use wy_mve::canvas_render2::for_each_node;
use wy_mve::canvas_render2::node2::Ext;
use wy_mve::{flex, grow, node2, AlignItem, Fc, Layout, Node2};
use wy_render::{Rect, Scene};
use wy_signal::GetValue;

use super::widgets::{cols_row, list_row, scroll_view, ColCell, COLS_3};
use crate::market::{format_price, format_qty, format_time, Trade};
use crate::{theme, Model};

/// 成交行高（定高，不随面板 grow 均分）。
pub const ROW_H: f32 = 20.0;

/// 成交面板。
pub fn trades_panel(model: Rc<Model>, exts: Vec<Ext>) -> Fc {
    let m = Rc::clone(&model);

    let rows = for_each_node(
        move |emit| {
            for t in m.trades.get().iter() {
                emit(t.id, t.clone());
            }
        },
        move |_id, trade: Rc<RefCell<Trade>>, cx| {
            cx.child(trade_row(trade));
        },
    );

    panel! {
        debug_name: "panel-trades".to_string(),
        exts: exts,
        title: Rc::new(|| "TIME & SALES".to_string()),
        badge: Rc::new(|| "LIVE".to_string()),
        actions: vec![],
        children: vec![
            col_header! {
                cells: vec![
                    ("Price", COLS_3.0),
                    ("Amount", COLS_3.1),
                    ("Time", COLS_3.2),
                ],
            },
            scroll_view(0.0, vec![rows]),
        ],
    }
}

/// 一条成交：价格 / 数量 / 时间三列 + 行底细线。
///
/// 买方是 maker = 卖方主动成交 → 下跌色（`up = !buyer_maker`）。
fn trade_row(trade: Rc<RefCell<Trade>>) -> Fc {
    // 三个 getter 各是一枚 `move` 闭包，会搬走捕获到的 `Rc`：价格列要读两次
    // （文本 + 涨色），所以**一处一份克隆**。
    let price_t = Rc::clone(&trade);
    let price_up = Rc::clone(&trade);
    let qty_t = Rc::clone(&trade);
    let time_t = Rc::clone(&trade);

    // `cols_row` 产出的是横向排列的一组列子节点，必须再包一层 Row 容器把它们
    // 排到同一行；这层容器顺带承担行底细线。
    let cells = node2! {
        layout: flex! { direction: Layout::Row, align_item: AlignItem::Center, align_fix: true },
        exts: vec![grow(1.0)],
        draw: move |s: &mut Scene, n: &Node2| {
            // 行底细线（成交行不画斑马纹，靠细分隔更接近终端）。
            // 横向 PAD 已由 `list_row` 的内层容器给过，这里按自身满宽画即可。
            s.fill_rect(
                Rect::new(0.0, n.height() - 1.0, n.width().max(0.0), 1.0),
                theme::GRID,
            );
        },
        children: cols_row(&[
            ColCell::left(
                move || format_price(price_t.borrow().price),
                9.5,
                move || theme::tone(!price_up.borrow().buyer_maker),
            ),
            ColCell::right(COLS_3.1, move || format_qty(qty_t.borrow().qty), 9.5, || theme::MUTED),
            ColCell::right(COLS_3.2, move || format_time(time_t.borrow().time), 9.0, || theme::FAINT),
        ]),
    };

    list_row(ROW_H, |_| {}, || {}, vec![cells], "trade-row", || false)
}
