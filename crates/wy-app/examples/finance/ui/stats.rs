//! 24h 统计面板：若干"标签 —— 值"行。
//!
//! 行组件自己撑满宽度，值与配色在绘制期读信号求值。整页只有一份 `model`，
//! 两个取值助手把「读 ticker → 取字段 → 缺省空串」这段样板收掉，
//! 于是下面每个 `kv_row` 只声明**取哪一列**。

use std::rc::Rc;

use wy_mve::{component, rsx, scroll, AlignItem};
use wy_render::Color;
use wy_signal::GetValue;

use super::widgets::{kv_row, panel};
use crate::market::{format_compact, format_percent, format_price, Ticker};
use crate::{theme, Model};

/// 取数助手：把 `fn(&Ticker) -> String` 变成绘制期求值的 `Fn() -> String`。
fn cell(model: Rc<Model>, f: fn(&Ticker) -> String) -> impl Fn() -> String {
    move || model.ticker.get().map(|t| f(&t)).unwrap_or_default()
}

/// 配色助手：同 [`cell`]，缺省为 `MUTED`。
fn tone_of(model: Rc<Model>, f: fn(&Ticker) -> Color) -> impl Fn() -> Color {
    move || model.ticker.get().map(|t| f(&t)).unwrap_or(theme::MUTED)
}

#[component]
pub fn stats_panel(ctx: &mut NodeContext) {
    // 只读行情：从上下文取，不需要调用方把 model 一路传下来
    let model = ctx
        .consume::<Rc<Model>>()
        .cloned()
        .expect("dashboard 应 provide 了 Rc<Model>");
    let up = |t: &Ticker| theme::tone(t.up());
    let spread = |t: &Ticker| format_price((t.ask - t.bid).max(0.0));
    let volume = |t: &Ticker| {
        // 只写 "76" 看不出是什么单位，补上基础币种
        format!(
            "{} {}",
            format_compact(t.volume),
            t.symbol.trim_end_matches("USDT")
        )
    };

    ctx.child(rsx! {
        panel {
            title: "MARKET STATS",
            badge: "24H",
            // 行定高、整体进滚动容器：统计行不再随面板 grow 均分（均分会把
            // 矮面板里的行压扁、高面板里的行拉出夸张行距）。kv_row 的 h 有
            // 默认值，这里不重复写。
            scroll {
                gap: 0.0,
                padding: 0.0,
                grow: 1.0,
                align_items: AlignItem::Stretch,
                kv_row { label: "HIGH",      value: cell(model.clone(), |t: &Ticker| format_price(t.high)),  tone: theme::TEXT }
                kv_row { label: "LOW",       value: cell(model.clone(), |t: &Ticker| format_price(t.low)),   tone: theme::TEXT }
                kv_row { label: "OPEN",      value: cell(model.clone(), |t: &Ticker| format_price(t.open)),  tone: theme::MUTED }
                kv_row { label: "CHANGE",    value: cell(model.clone(), |t: &Ticker| format_price(t.change)), tone: tone_of(model.clone(), up) }
                kv_row { label: "CHANGE %",  value: cell(model.clone(), |t: &Ticker| format_percent(t.change_percent)), tone: tone_of(model.clone(), up) }
                kv_row { label: "VOLUME",    value: cell(model.clone(), volume),     tone: theme::MUTED }
                kv_row { label: "QUOTE VOL", value: cell(model.clone(), |t: &Ticker| format_compact(t.quote_volume)), tone: theme::MUTED }
                kv_row { label: "BEST BID",  value: cell(model.clone(), |t: &Ticker| format_price(t.bid)),   tone: theme::UP }
                kv_row { label: "BEST ASK",  value: cell(model.clone(), |t: &Ticker| format_price(t.ask)),   tone: theme::DOWN }
                kv_row { label: "SPREAD",    value: cell(model.clone(), spread),    tone: theme::FAINT }
            }
        }
    });
}
