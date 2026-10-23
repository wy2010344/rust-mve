//! 成交面板：每条成交是一个行节点。
//!
//! 列表用 `for_each` 按 id 复用，行内绘制只读自身数据和 `current_box()`。
//! 行高**固定**（对齐 wy-helper 成交行 20px），整体进滚动容器——
//! 不再截断到固定条数：新成交从顶部涌入，旧的往下滚。

use std::cell::RefCell;
use std::rc::Rc;

use wy_mve::{component, for_each, rsx, scroll, AlignItem, Node};
use wy_render::{Rect, Scene};
use wy_signal::GetValue;

use super::widgets::{col_header, cols_row, list_row, panel, ColCell, COLS_3, PAD};
use crate::market::{format_price, format_qty, format_time, Trade};
use crate::{theme, Model};

/// 成交行高（定高，不随面板 grow 均分）。
pub const ROW_H: f32 = 20.0;

#[component]
pub fn trades_panel(ctx: &mut NodeContext) {
    let model = ctx
        .consume::<Rc<Model>>()
        .cloned()
        .expect("dashboard 应 provide 了 Rc<Model>");
    let rows = for_each(
        move |emit| {
            for t in model.trades.get().iter() {
                emit(t.id, t.clone());
            }
        },
        move |_id, trade, cx| {
            cx.child(trade_row(trade));
        },
    );

    ctx.child(
        rsx! {
            panel {
                title: "TIME & SALES",
                badge: "LIVE",
                col_header {
                    cells: vec![
                        ("Price", COLS_3.0),
                        ("Amount", COLS_3.1),
                        ("Time", COLS_3.2),
                    ],
                }
                // 滚动容器：`for_each` 是惰性区域，直接作为 children 就地展开成
                // 兄弟槽位；行定高，超出视口滚动（引擎命中测试按 clip 裁剪，
                // 视口外的行看不见也点不中）。
                scroll {
                    gap: 0.0,
                    padding: 0.0,
                    grow: 1.0,
                    align_items: AlignItem::Stretch,
                    { rows }
                }
            }
        }
        .into_node(),
    );
}

/// 一条成交：价格 / 数量 / 时间三列 + 行底细线。
///
/// 列文字走 [`cols_row`]（与表头同一套比例），行骨架走 [`list_row`]。
fn trade_row(trade: Rc<RefCell<Trade>>) -> Node {
    list_row(
        ROW_H,
        |_on| {},
        || {},
        move |s: &mut Scene, w: f32, h: f32| {
            let t = trade.borrow();
            // 买方是 maker = 卖方主动成交 → 下跌色
            let (price, qty, time, up) = (t.price, t.qty, t.time, !t.buyer_maker);
            cols_row(
                s,
                w,
                h,
                &[
                    ColCell::left(move || format_price(price), 9.5, move || theme::tone(up)),
                    ColCell::right(COLS_3.1, move || format_qty(qty), 9.5, || theme::MUTED),
                    ColCell::right(COLS_3.2, move || format_time(time), 9.0, || theme::FAINT),
                ],
            );
            // 行底细线（成交行不画斑马纹，靠细分隔更接近终端）
            s.fill_rect(
                Rect::new(PAD, h - 1.0, (w - 2.0 * PAD).max(0.0), 1.0),
                theme::GRID,
            );
        },
        "trade-row",
    )
}
