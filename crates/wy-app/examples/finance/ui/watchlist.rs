//! 自选列表：每行是一个独立节点（自己处理点击与悬停）。
//!
//! 行不是"面板统一绘制 + 透明热区"——那样点击要靠外部算 y 偏移。
//! 这里用 `for_each` 让每行成为真正的节点：命中、悬停、点击全部由引擎按树路由。
//! 行高**固定**（对齐 wy-helper 自选行 38px），整体进滚动容器，
//! 面板矮于内容时滚动而不是把行压扁。

use std::cell::RefCell;
use std::rc::Rc;

use wy_mve::{component, for_each, rsx, scroll, AlignItem, Node};
use wy_render::{Rect, Scene};
use wy_signal::{GetValue, SetValue, Signal};

use super::widgets::{col_header, cols_row_at, list_row, panel, ColCell};
use crate::market::{format_compact, format_percent, format_price, Ticker};
use crate::{theme, Ctl, Model};

/// 自选行高（定高：上行「符号+最新价」、下行「成交额+涨跌幅」的双行排版）。
pub const ROW_H: f32 = 38.0;

#[component]
pub fn watchlist(ctx: &mut NodeContext) {
    // model / ctl 都由 `dashboard` provide，这里只声明"我要什么"
    let model = ctx
        .consume::<Rc<Model>>()
        .cloned()
        .expect("dashboard 应 provide 了 Rc<Model>");
    let ctl = ctx
        .consume::<Rc<Ctl>>()
        .cloned()
        .expect("dashboard 应 provide 了 Rc<Ctl>");
    // 行是 `for_each` 的项，拿不到本函数的局部变量——provide 给它们
    ctx.provide(Rc::clone(&ctl));

    let rows = for_each(
        move |emit| {
            let list = model.watch.get();
            for t in list.iter() {
                emit(t.symbol.clone(), t.clone());
            }
        },
        move |_key, ticker, cx| {
            // 项内 consume：不必把 ctl 一路捕获进闭包
            let ctl = cx
                .consume::<Rc<Ctl>>()
                .cloned()
                .expect("watchlist 应 provide 了 Rc<Ctl>");
            cx.child(watch_row(ticker, ctl));
        },
    );

    ctx.child(
        rsx! {
            panel {
                title: "WATCHLIST",
                badge: "24H",
                // 双行排版下价格与涨跌幅都贴右边，表头就两列——
                // 还按 3 列写会让"LAST"压在价格上、看起来对不齐。
                col_header { cells: vec![("SYMBOL", 0.0), ("LAST / 24H", 1.0)] }
                // 滚动容器：行定高，超出视口滚动（引擎命中测试按 clip 裁剪）
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

/// 一行自选：上行「符号 + 最新价」，下行「成交额 + 涨跌幅」。
///
/// 两行合起来约 24px，整体居中才不会"上紧下松"。列文字走 [`cols_row`] /
/// [`cols_row_at`]（与表头同一套比例），行骨架走 [`list_row`]（定高 +
/// 悬停 + 点击）——不再手算列 x，也不再随面板 grow 均分。
fn watch_row(ticker: Rc<RefCell<Ticker>>, ctl: Rc<Ctl>) -> Node {
    let hover = Signal::new(false);
    let ctl_click = ctl.clone();
    let ctl_draw = ctl.clone();
    let ticker_click = Rc::clone(&ticker);
    let ticker_draw = Rc::clone(&ticker);
    // Signal 是 Copy（内部 Rc 句柄）：直接赋值即共享
    let h_write = hover;
    let h_read = hover;

    list_row(
        ROW_H,
        move |entered| h_write.set(entered),
        move || {
            let sym = ctl_click.model.symbol.get();
            let this = ticker_click.borrow().symbol.clone();
            if sym != this {
                ctl_click.select_symbol(&this);
            }
        },
        move |s: &mut Scene, w: f32, h: f32| {
            let t = ticker_draw.borrow();
            let active = ctl_draw.model.symbol.get() == t.symbol;
            if active || h_read.get() {
                // 内缩 2px + 圆角：不硬贴面板边框，也不扎穿 8px 的面板圆角
                s.fill_round_rect(
                    Rect::new(2.0, 1.0, (w - 4.0).max(0.0), (h - 2.0).max(0.0)),
                    5.0,
                    if active {
                        theme::ACCENT_SOFT
                    } else {
                        theme::ROW_ALT
                    },
                );
            }
            let (symbol, price, change, vol, up) = (
                t.symbol.clone(),
                t.last,
                t.change_percent,
                t.quote_volume,
                t.up(),
            );
            let sym_color = if active { theme::TEXT } else { theme::MUTED };
            let y1 = (h - 24.0) / 2.0;
            let y2 = y1 + 13.5;
            // 上行：符号（左）+ 最新价（右）
            cols_row_at(
                s,
                w,
                y1,
                &[
                    ColCell::left(move || symbol.clone(), 10.5, move || sym_color),
                    ColCell::right(1.0, move || format_price(price), 10.5, || theme::TEXT),
                ],
            );
            // 下行：成交额（左）+ 涨跌幅（右）
            cols_row_at(
                s,
                w,
                y2,
                &[
                    ColCell::left(
                        move || format!("VOL {}", format_compact(vol)),
                        8.5,
                        || theme::FAINT,
                    ),
                    ColCell::right(
                        1.0,
                        move || format_percent(change),
                        9.5,
                        move || theme::tone(up),
                    ),
                ],
            );
        },
        "watch-row",
    )
}
