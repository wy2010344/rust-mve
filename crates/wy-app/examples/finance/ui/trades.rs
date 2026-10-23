//! 成交面板：每条成交是一个行节点。
//!
//! 列表用 `for_each` 按 id 复用，行内绘制只读自身数据和 `current_box()`。
//! 行高由父容器均分（`grow: 1`），面板被拖矮时自动收窄。

use std::cell::RefCell;
use std::rc::Rc;

use wy_mve::{component, current_box, for_each, rsx, Node};
use wy_render::{Rect, Scene};
use wy_signal::GetValue;

use super::widgets::{col_header, panel, PAD};
use crate::market::{format_price, format_qty, format_time, Trade};
use crate::{theme, Model};

/// 最多显示多少条（行数固定，行高自适应）。
pub const MAX_ROWS: usize = 14;

#[component]
pub fn trades_panel(ctx: &mut NodeContext) {
    let model = ctx
        .consume::<Rc<Model>>()
        .cloned()
        .expect("dashboard 应 provide 了 Rc<Model>");
    let rows = for_each(
        move |emit| {
            for t in model.trades.get().iter().take(MAX_ROWS) {
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
                col_header { cells: vec![("Price", 0.0), ("Amount", 0.60), ("Time", 1.0)] }
                // `for_each` 是惰性区域：直接作为 children 就地展开成兄弟槽位，
                // 不要包 vbox——包了之后动态区域会脱离父容器布局（高度塌成 0）。
                { rows }
            }
        }
        .into_node(),
    );
}

fn trade_row(trade: Rc<RefCell<Trade>>) -> Node {
    rsx! {
        node {
            grow: 1.0,
            draw: move |s: &mut Scene| {
                let b = current_box();
                if b.height <= 0.0 {
                    return;
                }
                let t = trade.borrow();
                // 买方是 maker = 卖方主动成交 → 下跌色
                let color = theme::tone(!t.buyer_maker);
                let y = (b.height - 9.0) / 2.0;
                theme::text(s, PAD, y, &format_price(t.price), 9.5, color);
                theme::text_right(
                    s,
                    PAD + (b.width - 2.0 * PAD) * 0.60,
                    y,
                    &format_qty(t.qty),
                    9.5,
                    theme::MUTED,
                );
                theme::text_right(
                    s,
                    b.width - PAD,
                    y,
                    &format_time(t.time),
                    9.0,
                    theme::FAINT,
                );
                // 行底细线（成交行不画斑马纹，靠细分隔更接近终端）
                s.fill_rect(
                    Rect::new(PAD, b.height - 1.0, (b.width - 2.0 * PAD).max(0.0), 1.0),
                    theme::GRID,
                );
            },
        }
    }
    .into_node()
}
