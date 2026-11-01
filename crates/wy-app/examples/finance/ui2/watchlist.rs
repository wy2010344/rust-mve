//! 自选列表：每行是一个独立节点（自己处理点击与悬停）。
//!
//! 行是 `for_each_node` 的项，命中/悬停/点击全部由引擎按树路由。行高固定、整体进
//! 滚动容器，面板矮于内容时滚动而不是把行压扁。文本全部走 `wrapped_text_node!`
//! 子节点（经 [`cols_row`]）。

use std::cell::RefCell;
use std::rc::Rc;

use wy_mve::canvas_render2::for_each_node;
use wy_mve::canvas_render2::node2::Ext;
use wy_mve::{flex, node2, AlignItem, Fc, Layout};
use wy_signal::GetValue;

use super::widgets::{cols_row, list_row, scroll_view, ColCell};
use crate::market::{format_compact, format_percent, format_price, Ticker};
use crate::theme;
use crate::{Ctl, Model};

/// 自选行高（定高：上行「符号+最新价」、下行「成交额+涨跌幅」的双行排版）。
pub const ROW_H: f32 = 38.0;

/// 自选面板。
pub fn watchlist(model: Rc<Model>, ctl: Rc<Ctl>, exts: Vec<Ext>) -> Fc {
    let m = Rc::clone(&model);
    let c = Rc::clone(&ctl);

    let rows = for_each_node(
        move |emit| {
            for t in m.watch.get() {
                emit(t.symbol.clone(), t.clone());
            }
        },
        move |_key, ticker: Rc<RefCell<Ticker>>, cx| {
            let c2 = Rc::clone(&c);
            cx.child(watch_row(ticker, c2));
        },
    );

    panel! {
        debug_name: "panel-watchlist".to_string(),
        exts: exts,
        title: Rc::new(|| String::from("WATCHLIST")),
        badge: Rc::new(|| String::from("24H")),
        actions: vec![],
        children: vec![
            col_header! { cells: vec![("SYMBOL", 0.0), ("LAST", 0.66), ("24H", 1.0)] },
            scroll_view(0.0, vec![rows]),
        ],
    }
}

/// 一行自选：上行「符号 + 最新价」，下行「成交额 + 涨跌幅」。
fn watch_row(ticker: Rc<RefCell<Ticker>>, ctl: Rc<Ctl>) -> Fc {
    // 每个 getter / 回调都是 `move` 闭包，会把捕获到的 `Rc` 搬走；同一份 `Rc` 被
    // 第二个闭包用一次就报"移动后再次使用"。所以**一处一份克隆**，各拿各的句柄。
    let row1_symbol = {
        let t = Rc::clone(&ticker);
        move || t.borrow().symbol.clone()
    };
    let row1_symbol_tone = {
        let t = Rc::clone(&ticker);
        let c = Rc::clone(&ctl);
        move || {
            if c.model.symbol.get() == t.borrow().symbol {
                theme::TEXT
            } else {
                theme::MUTED
            }
        }
    };
    let row1_last = {
        let t = Rc::clone(&ticker);
        move || format_price(t.borrow().last)
    };
    let row1_change = {
        let t = Rc::clone(&ticker);
        move || format_percent(t.borrow().change_percent)
    };
    let row1_tone = {
        let t = Rc::clone(&ticker);
        move || {
            if t.borrow().change_percent >= 0.0 {
                theme::UP
            } else {
                theme::DOWN
            }
        }
    };
    // 两行都是「按内容定高」的容器吗？不是——这里**显式定高**。
    // 只给 `align_item: Center` 而不给 `height` 时，行高会退化成
    // `default_size` → 向父级要素尺寸 → 父级又按子节点外包尺寸算 → 环闭合取 0：
    // 两行的 box 都成了 0 高、落在同一个 y 上，文字直接叠在一起。
    // v1 是「同一个 draw 里画两行文字，y1 = (h-24)/2、y2 = y1+13.5」，
    // 这里用「两个 13.5 高的行节点上下相接」表达同一个行距。
    const LINE_H: f32 = 13.5;
    // 第一行：Symbol | Last | 24h（对齐 ooc 的三列，24h 涨跌幅从第二行挪到此处）。
    let row1 = node2! {
        layout: flex! { direction: Layout::Row, align_item: AlignItem::Center, align_fix: true },
        height: LINE_H,
        children: cols_row(&[
            ColCell::left(row1_symbol, 10.5, row1_symbol_tone),
            ColCell::right(0.66, row1_last, 10.5, || theme::TEXT),
            ColCell::right(1.0, row1_change, 9.5, row1_tone),
        ]),
    };

    let row2_volume = {
        let t = Rc::clone(&ticker);
        move || format!("VOL {}", format_compact(t.borrow().quote_volume))
    };
    let row2 = node2! {
        layout: flex! { direction: Layout::Row, align_item: AlignItem::Center, align_fix: true },
        height: LINE_H,
        children: cols_row(&[
            ColCell::left(row2_volume, 8.5, || theme::FAINT),
        ]),
    };

    // 点击回调与选中高亮各用一份自己的克隆。
    let click_ctl = Rc::clone(&ctl);
    let click_t = Rc::clone(&ticker);
    let hl_ctl = Rc::clone(&ctl);
    let hl_t = Rc::clone(&ticker);

    list_row(
        ROW_H,
        |_| {},
        move || {
            let sym = click_ctl.model.symbol.get();
            let this = click_t.borrow().symbol.clone();
            if sym != this {
                click_ctl.select_symbol(&this);
            }
        },
        vec![row1, row2],
        "watch-row",
        move || hl_ctl.model.symbol.get() == hl_t.borrow().symbol,
    )
}
