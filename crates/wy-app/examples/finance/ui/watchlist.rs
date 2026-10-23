//! 自选列表：每行是一个独立节点（自己处理点击与悬停）。
//!
//! 行不是"面板统一绘制 + 透明热区"——那样点击要靠外部算 y 偏移。
//! 这里用 `for_each` 让每行成为真正的节点：命中、悬停、点击全部由引擎按树路由。

use std::cell::RefCell;
use std::rc::Rc;

use wy_mve::{component, current_box, for_each, rsx, Node};
use wy_render::{Rect, Scene};
use wy_signal::{GetValue, SetValue, Signal};

use super::widgets::{col_header, panel, PAD};
use crate::market::{format_compact, format_percent, format_price, Ticker};
use crate::{theme, Ctl, Model};

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
                { rows }
            }
        }
        .into_node(),
    );
}

/// 一行自选：上行「符号 + 最新价」，下行「成交额 + 涨跌幅」。
///
/// 双行只在行高够的时候出现（拖动分割条把面板压矮时自动退回单行），
/// 所以行距既能填满面板，又不会在窄高下挤成一团。
fn watch_row(ticker: Rc<RefCell<Ticker>>, ctl: Rc<Ctl>) -> Node {
    let hover = Signal::new(false);
    let ctl_click = ctl.clone();
    let ctl_draw = ctl.clone();
    let ticker_click = Rc::clone(&ticker);

    rsx! {
        node {
            grow: 1.0,
            on_hover: move |entered| hover.set(entered),
            on_click: move || {
                let sym = ctl_click.model.symbol.get();
                let this = ticker_click.borrow().symbol.clone();
                if sym != this {
                    ctl_click.select_symbol(&this);
                }
            },
            draw: move |s: &mut Scene| {
                let b = current_box();
                if b.height <= 0.0 {
                    return;
                }
                let t = ticker.borrow();
                let active = ctl_draw.model.symbol.get() == t.symbol;
                if active || hover.get() {
                    // 内缩 2px + 圆角：不硬贴面板边框，也不扎穿 8px 的面板圆角
                    s.fill_round_rect(
                        Rect::new(2.0, 1.0, (b.width - 4.0).max(0.0), (b.height - 2.0).max(0.0)),
                        5.0,
                        if active { theme::ACCENT_SOFT } else { theme::ROW_ALT },
                    );
                }
                let right = b.width - PAD;
                let tone = theme::tone(t.up());
                let price = format_price(t.last);
                let change = format_percent(t.change_percent);
                let sym_color = if active { theme::TEXT } else { theme::MUTED };

                if b.height >= 34.0 {
                    // 双行：上行主信息，下行补充量能。
                    // 两行合起来约 24px，整体居中才不会"上紧下松"。
                    let y1 = (b.height - 24.0) / 2.0;
                    let y2 = y1 + 13.5;
                    theme::text(s, PAD, y1, &t.symbol, 10.5, sym_color);
                    theme::text_right(s, right, y1, &price, 10.5, theme::TEXT);
                    theme::text(
                        s,
                        PAD,
                        y2,
                        &format!("VOL {}", format_compact(t.quote_volume)),
                        8.5,
                        theme::FAINT,
                    );
                    theme::text_right(s, right, y2, &change, 9.5, tone);
                } else {
                    // 单行：面板被压矮时的降级排版
                    let y = (b.height - 10.5) / 2.0;
                    theme::text(s, PAD, y, &t.symbol, 10.5, sym_color);
                    theme::text_right(s, PAD + (b.width - 2.0 * PAD) * 0.62, y, &price, 10.5, theme::TEXT);
                    theme::text_right(s, right, y, &change, 10.5, tone);
                }
            },
        }
    }
    .into_node()
}
