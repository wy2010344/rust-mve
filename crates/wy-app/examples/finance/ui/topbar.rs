//! 顶栏：品牌、标的/周期切换、价格、数据源开关。
//!
//! 视觉：HEADER 底色 + 底部 1px 分隔，品牌位是「方块 + 双行字」，
//! 右侧数据源是分段开关（点哪边切哪边），不是一块光板。
//!
//! 每个 tab / 开关都是独立节点，点击回调直接绑在自己身上——没有"点击坐标落在
//! 哪个矩形里"的判断，命中测试由引擎沿节点树做（子优先 + 冒泡）。

use std::rc::Rc;

use wy_mve::{component, current_box, rsx, AlignItem, DirectionJustify, Node, Slot};
use wy_render::{Rect, Scene};
use wy_signal::{GetValue, SetValue, Signal};

use super::widgets::{flex_spacer, tab_btn, tab_of, text_label, vsep};
use crate::binance::Source;
use crate::market::{format_percent, format_price, TAB_INTERVALS, TAB_SYMBOLS};
use crate::{theme, Ctl, Model};

/// 顶栏高度（含内边距）。
pub const TOPBAR_H: f32 = 48.0;

#[component]
pub fn topbar(ctx: &mut NodeContext) {
    let model = ctx
        .consume::<Rc<Model>>()
        .cloned()
        .expect("dashboard 应 provide 了 Rc<Model>");
    let ctl = ctx
        .consume::<Rc<Ctl>>()
        .cloned()
        .expect("dashboard 应 provide 了 Rc<Ctl>");

    let symbol_tabs: Vec<Slot> = TAB_SYMBOLS
        .iter()
        .map(|sym| {
            let (m, c, sym) = (model.clone(), ctl.clone(), *sym);
            Slot::One(tab_btn(
                sym,
                move || m.symbol.get() == sym,
                move || c.select_symbol(sym),
            ))
        })
        .collect();

    let interval_tabs: Vec<Slot> = TAB_INTERVALS
        .iter()
        .map(|itv| {
            let (m, c, itv) = (model.clone(), ctl.clone(), *itv);
            Slot::One(tab_btn(
                itv,
                move || m.interval.get() == itv,
                move || c.select_interval(itv),
            ))
        })
        .collect();

    let m_err = model.clone();
    let m_err_hide = model.clone();

    ctx.child(
        rsx! {
            hbox {
                gap: 6.0,
                height: TOPBAR_H,
                padding: 10.0,
                align_items: AlignItem::Center,
                draw: |s: &mut Scene| {
                    let b = current_box();
                    s.fill_rect(Rect::new(0.0, 0.0, b.width, b.height), theme::HEADER);
                    s.fill_rect(Rect::new(0.0, b.height - 1.0, b.width, 1.0), theme::BORDER);
                },

                { brand(model.clone()) }
                vsep {}
                ..symbol_tabs
                // 自选里点了固定 tab 之外的标的时补一格「当前标的」，否则顶栏一个都不高亮
                { current_tab(model.clone()) }
                vsep {}
                ..interval_tabs
                vsep {}
                { price(model.clone()) }
                { change(model.clone()) }
                // 剩余空间吃掉，数据源开关贴右
                flex_spacer {}
                { source_switch(model.clone(), ctl.clone()) }
                // 错误提示（有错误时才出现，`hide` 走信号）
                text_label {
                    content: m_err.error.get(),
                    font_size: 9.0,
                    color: theme::DOWN,
                    hide: m_err_hide.error.get().is_empty(),
                }
            }
        }
        .into_node(),
    );
}

/// 品牌块：方块标记 + 主标题 + 「数据源 · 连接状态」副标题。
///
/// 整个块是**一个**自绘节点：三行字紧挨在一起，用 flex 拆成三个节点反而
/// 会因为行高各自取整而对不齐基线。
fn brand(model: Rc<Model>) -> Node {
    rsx! {
        node {
            width: 152.0,
            height: 26.0,
            draw: move |s: &mut Scene| {
                // 22px 圆角方块：主色 + 内圈亮边，避免"光板矩形"。
                // 不用"上半块提亮"那种假渐变——会在方块中线留一条硬缝，
                // 放大看像两块拼起来的。
                let mark = Rect::new(0.0, 2.0, 22.0, 22.0);
                s.fill_round_rect(mark, 6.0, theme::ACCENT);
                s.stroke_round_rect(Rect::new(1.5, 3.5, 19.0, 19.0), 5.0, theme::ACCENT_HI, 1.0);
                s.stroke_round_rect(Rect::new(0.5, 2.5, 21.0, 21.0), 6.0, theme::ACCENT_EDGE, 1.0);
                // y 用 6.8 而不是 (22-11)/2：字号是 em 高度，字形实际只占
                // 中间偏下的一段，按 em 居中会低 1~2px（实测校准）。
                theme::text_center(s, 11.0, 6.8, "W", 11.0, theme::BG);

                // 双行字：主标题 12.5 / 副标题 8.5，中间留 2px
                // （两行直接压在一起会显得"糊"，是这个位置最容易翻车的地方）
                theme::text(s, 30.0, 1.5, "WY MARKETS", 12.5, theme::TEXT);
                let source = model.source.get();
                let status = model.status.get();
                let dot = if status.is_live() {
                    theme::UP
                } else {
                    theme::FAINT
                };
                // 副标题左边一个小圆点，颜色表示连接状态
                s.fill_round_rect(Rect::new(30.0, 18.5, 4.0, 4.0), 2.0, dot);
                theme::text(
                    s,
                    37.0,
                    16.0,
                    &format!("{} · {}", source.label(), status.label()),
                    8.5,
                    theme::FAINT,
                );
            },
        }
    }
    .into_node()
}

/// 当前标的：大型价格。
fn price(model: Rc<Model>) -> Node {
    let m_val = model.clone();
    let m_tone = model.clone();
    rsx! {
        text_label {
            content: move || {
                m_val.ticker.get().map(|t| format_price(t.last)).unwrap_or_else(|| String::from("--"))
            },
            font_size: 15.0,
            color: move || m_tone.ticker.get().map(|t| theme::tone(t.up())).unwrap_or(theme::MUTED),
        }
    }
    .into_node()
}

/// 当前标的：24h 涨跌幅。
fn change(model: Rc<Model>) -> Node {
    let m_val = model.clone();
    let m_tone = model.clone();
    rsx! {
        text_label {
            content: move || {
                m_val.ticker.get().map(|t| format_percent(t.change_percent)).unwrap_or_default()
            },
            font_size: 10.0,
            color: move || m_tone.ticker.get().map(|t| theme::tone(t.up())).unwrap_or(theme::MUTED),
        }
    }
    .into_node()
}

/// 「当前标的」补位 tab：只在该标的不在固定 tab 组里时出现。
///
/// 它是自选区选出来的，所以永远处于选中态、点击不需要做事。
fn current_tab(model: Rc<Model>) -> Node {
    let m_label = model.clone();
    let m_active = model.clone();
    let m_hide = model.clone();
    let mut node = tab_of(
        Rc::new(move || m_label.symbol.get()),
        Rc::new(move || {
            let sym = m_active.symbol.get();
            !TAB_SYMBOLS.contains(&sym.as_str())
        }),
        Rc::new(|| {}),
    );
    node.hide_fn = Some(Rc::new(move || {
        let sym = m_hide.symbol.get();
        TAB_SYMBOLS.contains(&sym.as_str())
    }));
    node
}

/// 数据源分段开关：`[ SIM | BINANCE | MOCK ]`，点哪边切哪边。
///
/// 外框由容器画、每段各自是节点，不需要算"点在左半边还是右半边"。
///
/// `SIM` 是默认项——**内置合成行情**，不联网也能跑（clone 下来立刻有画面）。
fn source_switch(model: Rc<Model>, ctl: Rc<Ctl>) -> Node {
    rsx! {
        hbox {
            gap: 4.0,
            width: 192.0,
            height: 22.0,
            padding: 2.0,
            align_items: AlignItem::Center,
            justify: DirectionJustify::Center,
            draw: |s: &mut Scene| {
                s.fill_round_rect(Rect::new(0.0, 0.0, 192.0, 22.0), 4.0, theme::ROW_ALT);
                s.stroke_round_rect(Rect::new(0.5, 0.5, 191.0, 21.0), 4.0, theme::BORDER, 1.0);
            },
            { source_seg(model.clone(), ctl.clone(), Source::Sim) }
            { source_seg(model.clone(), ctl.clone(), Source::Binance) }
            { source_seg(model.clone(), ctl.clone(), Source::Mock) }
        }
    }
    .into_node()
}

/// 分段开关里的一段。
fn source_seg(model: Rc<Model>, ctl: Rc<Ctl>, seg: Source) -> Node {
    const SEG_W: f32 = 60.0;
    const SEG_H: f32 = 18.0;
    let hover = Signal::new(false);

    let m_click = model.clone();
    let seg_click = seg.clone();

    rsx! {
        node {
            width: SEG_W,
            height: SEG_H,
            on_hover: move |on| hover.set(on),
            on_click: move || {
                if m_click.source.get() != seg_click {
                    ctl.set_source(seg_click.clone());
                }
            },
            draw: move |s: &mut Scene| {
                let on = model.source.get() == seg;
                if on {
                    s.fill_round_rect(Rect::new(0.0, 0.0, SEG_W, SEG_H), 3.0, theme::ACCENT_SOFT);
                } else if hover.get() {
                    s.fill_round_rect(Rect::new(0.0, 0.0, SEG_W, SEG_H), 3.0, theme::ROW_ALT);
                }
                let color = if on {
                    theme::TEXT
                } else if hover.get() {
                    theme::MUTED
                } else {
                    theme::FAINT
                };
                theme::text_center(s, SEG_W / 2.0, 5.0, seg.label(), 9.0, color);
            },
        }
    }
    .into_node()
}
