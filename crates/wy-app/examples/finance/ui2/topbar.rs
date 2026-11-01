//! 顶栏：品牌、标的/周期切换、价格、数据源开关。
//!
//! 每个 tab / 开关都是独立节点，点击回调直接绑在自己身上，命中测试由引擎沿节点
//! 树做（子优先 + 冒泡）。文本全部走 [`text_label!`] 子节点。

use std::rc::Rc;

use wy_mve::canvas_render2::protocol::MouseEvent2;
use wy_mve::{flex, node2, AlignItem, CursorType, DirectionJustify, Fc, Layout, Node2, Scene};
use wy_render::Rect;
use wy_signal::{GetValue, SetValue, Signal};

use super::widgets::{flex_spacer, tab_btn, tab_of, vsep};
use crate::binance::Source;
use crate::market::{format_percent, format_price, TAB_INTERVALS, TAB_SYMBOLS};
use crate::{theme, Ctl, Model};

/// 顶栏高度（含内边距）。
pub const TOPBAR_H: f32 = 48.0;

/// 顶栏：hbox，固定高，画底色 + 底部分隔线。
pub fn topbar(model: Rc<Model>, ctl: Rc<Ctl>) -> Fc {
    let m = Rc::clone(&model);

    let symbol_tabs: Vec<Fc> = TAB_SYMBOLS
        .iter()
        .map(|sym| {
            let (mm, cc, s) = (Rc::clone(&m), Rc::clone(&ctl), *sym);
            tab_btn(s, move || mm.symbol.get() == s, move || cc.select_symbol(s))
        })
        .collect();

    let interval_tabs: Vec<Fc> = TAB_INTERVALS
        .iter()
        .map(|itv| {
            let (mm, cc, s) = (Rc::clone(&m), Rc::clone(&ctl), *itv);
            tab_btn(
                s,
                move || mm.interval.get() == s,
                move || cc.select_interval(s),
            )
        })
        .collect();

    let m_err = Rc::clone(&m);
    let m_err_hide = Rc::clone(&m);

    node2! {
        debug_name: "topbar".to_string(),
        // 定高条：`align_fix` 让所有子项以**条高**为基准居中；缺了它基准退化成
        // 最高子项（品牌块 26），整行会贴顶约 1px。
        layout: flex! { direction: Layout::Row, align_item: AlignItem::Center, gap: 6.0, align_fix: true },
        height: TOPBAR_H,
        padding: 10.0,
        draw: move |s: &mut Scene, n: &Node2| {
            s.fill_rect(Rect::new(0.0, 0.0, n.width(), n.height()), theme::HEADER);
            s.fill_rect(Rect::new(0.0, n.height() - 1.0, n.width(), 1.0), theme::BORDER);
        },
        children: vec![
            brand(Rc::clone(&m)),
            vsep(),
            tab_group(symbol_tabs),
            current_tab(Rc::clone(&m)),
            vsep(),
            tab_group(interval_tabs),
            vsep(),
            price(Rc::clone(&m)),
            change(Rc::clone(&m)),
            flex_spacer(),
            source_switch(Rc::clone(&m), Rc::clone(&ctl)),
            text_label! {
                content: m_err.error.get(),
                font_size: 9.0,
                color: theme::DOWN,
                hide: m_err_hide.error.get().is_empty(),
            },
        ],
    }
}

/// 一组 tab 的横向容器。
///
/// **必须显式给 `layout`**：`node2!` 不写 `layout` 就是绝对定位，组内每个 tab 都
/// 会被钉在 (0,0)（顶栏里表现为所有周期按钮叠在价格文字上），而容器自身宽度也
/// 取不到（0），整组不占 flex 空间。
fn tab_group(tabs: Vec<Fc>) -> Fc {
    node2! {
        layout: flex! { direction: Layout::Row, align_item: AlignItem::Center, gap: 4.0 },
        children: tabs,
    }
}

/// 品牌块：方块标记 + 主标题 + 「数据源 · 连接状态」副标题（文本走子节点）。
fn brand(model: Rc<Model>) -> Fc {
    let m = Rc::clone(&model);
    let m2 = Rc::clone(&model);
    node2! {
        width: 152.0,
        height: 26.0,
        // 仅画 mark 外框 + 状态点（都是形状，不是文字）
        draw: move |s: &mut Scene, n: &Node2| {
            let _ = n;
            let mark = Rect::new(0.0, 2.0, 22.0, 22.0);
            s.fill_round_rect(mark, 6.0, theme::ACCENT);
            s.stroke_round_rect(Rect::new(1.5, 3.5, 19.0, 19.0), 5.0, theme::ACCENT_HI, 1.0);
            s.stroke_round_rect(Rect::new(0.5, 2.5, 21.0, 21.0), 6.0, theme::ACCENT_EDGE, 1.0);
        },
        children: vec![
            // mark 内的 "W"：文字子节点，居中于 mark 方块
            // mark 内的 "W"：文字子节点，居中于 mark 方块
            node2! {
                x: 0.0, y: 2.0, width: 22.0, height: 22.0,
                // 定高 22 方块：`align_fix` 让 "W" 以方块高为基准居中（否则贴顶偏高）。
                layout: flex! { direction: Layout::Row, align_item: AlignItem::Center, direction_justify: DirectionJustify::Center, align_fix: true },
                children: vec![ text_label! { content: "W".to_string(), font_size: 11.0, color: theme::BG } ],
            },
            node2! {
                x: 30.0, y: 1.5,
                layout: flex! { direction: Layout::Row, align_item: AlignItem::Start },
                children: vec![ text_label! { content: "WY MARKETS".to_string(), font_size: 11.0, color: theme::TEXT } ] },
            node2! {
                x: 30.0, y: 18.5, width: 4.0, height: 4.0,
                draw: move |s: &mut Scene, n: &Node2| {
                    let _ = n;
                    let dot = if m2.status.get().is_live() { theme::UP } else { theme::FAINT };
                    s.fill_round_rect(Rect::new(0.0, 0.0, 4.0, 4.0), 2.0, dot);
                },
            },
            node2! {
                x: 37.0, y: 16.0,
                layout: flex! { direction: Layout::Row, align_item: AlignItem::Start },
                children: vec![ text_label! { content: format!("{} · {}", m.source.get().label(), m.status.get().label()), font_size: 8.5, color: theme::FAINT } ] },
        ],
    }
}

/// 当前标的：大型价格。
fn price(model: Rc<Model>) -> Fc {
    let m_val = Rc::clone(&model);
    let m_tone = Rc::clone(&model);
    text_label! {
        content: m_val.ticker.get().map(|t| format_price(t.last)).unwrap_or_else(|| String::from("--")),
        font_size: 15.0,
        color: m_tone.ticker.get().map(|t| theme::tone(t.up())).unwrap_or(theme::MUTED),
    }
}

/// 当前标的：24h 涨跌幅。
fn change(model: Rc<Model>) -> Fc {
    let m_val = Rc::clone(&model);
    let m_tone = Rc::clone(&model);
    text_label! {
        content: m_val.ticker.get().map(|t| format_percent(t.change_percent)).unwrap_or_default(),
        font_size: 10.0,
        color: m_tone.ticker.get().map(|t| theme::tone(t.up())).unwrap_or(theme::MUTED),
    }
}

/// 「当前标的」补位 tab：只在该标的不在固定 tab 组里时出现。
fn current_tab(model: Rc<Model>) -> Fc {
    let m_label = Rc::clone(&model);
    let m_active = Rc::clone(&model);
    let m_hide = Rc::clone(&model);
    node2! {
        hide: TAB_SYMBOLS.contains(&m_hide.symbol.get().as_str()),
        // 同 `tab_group`：包一层就必须给 layout，否则 tab 会被钉在 (0,0)。
        layout: flex! { direction: Layout::Row, align_item: AlignItem::Center },
        children: vec![tab_of(
            Rc::new(move || m_label.symbol.get()),
            Rc::new(move || !TAB_SYMBOLS.contains(&m_active.symbol.get().as_str())),
            Rc::new(|| {}),
        )],
    }
}

/// 数据源分段开关：`[ SIM | BINANCE | MOCK ]`。
fn source_switch(model: Rc<Model>, ctl: Rc<Ctl>) -> Fc {
    node2! {
        layout: flex! { direction: Layout::Row, align_item: AlignItem::Center, gap: 4.0, direction_justify: DirectionJustify::Center, align_fix: true },
        width: 192.0,
        height: 22.0,
        padding: 2.0,
        draw: move |s: &mut Scene, n: &Node2| {
            s.fill_round_rect(Rect::new(0.0, 0.0, n.width(), n.height()), 4.0, theme::ROW_ALT);
            s.stroke_round_rect(Rect::new(0.5, 0.5, n.width() - 1.0, n.height() - 1.0), 4.0, theme::BORDER, 1.0);
        },
        children: vec![
            source_seg(Rc::clone(&model), Rc::clone(&ctl), Source::Sim),
            source_seg(Rc::clone(&model), Rc::clone(&ctl), Source::Binance),
            source_seg(Rc::clone(&model), Rc::clone(&ctl), Source::Mock),
        ],
    }
}

/// 分段开关里的一段。
fn source_seg(model: Rc<Model>, ctl: Rc<Ctl>, seg: Source) -> Fc {
    const SEG_W: f32 = 60.0;
    const SEG_H: f32 = 18.0;
    let hover = Signal::new(false);
    let seg_click = seg.clone();
    let seg_draw = seg.clone();
    let seg_color = seg.clone();
    node2! {
        width: SEG_W,
        height: SEG_H,
        cursor: CursorType::Pointer,
        on_hover: {
            let hover = hover.clone();
            move |on| hover.set(on)
        },
        mouse_click: {
            let m_click = Rc::clone(&model);
            let seg_click = seg_click.clone();
            let ctl = Rc::clone(&ctl);
            move |_e: &mut MouseEvent2| {
                if m_click.source.get() != seg_click {
                    ctl.set_source(seg_click.clone());
                }
            }
        },
        // 定高 18 的分段按钮：`align_fix` 让文字以**本段高度**为基准居中
        // （缺了它基准退化成文字自身行框 ~14px，整体贴顶偏高约 2px）。
        layout: flex! { direction: Layout::Row, align_item: AlignItem::Center, direction_justify: DirectionJustify::Center, align_fix: true },
        draw: {
            let m_draw = Rc::clone(&model);
            let hover = hover.clone();
            move |s: &mut Scene, n: &Node2| {
                let _ = n;
                let on = m_draw.source.get() == seg_draw;
                if on {
                    s.fill_round_rect(Rect::new(0.0, 0.0, SEG_W, SEG_H), 3.0, theme::ACCENT_SOFT);
                } else if hover.get() {
                    s.fill_round_rect(Rect::new(0.0, 0.0, SEG_W, SEG_H), 3.0, theme::ROW_ALT);
                }
            }
        },
        children: vec![ text_label! {
            content: seg.label().to_string(),
            font_size: 9.0,
            color: {
                let m_color = Rc::clone(&model);
                let hover = hover.clone();
                {
                    let on = m_color.source.get() == seg_color;
                    if on { theme::TEXT } else if hover.get() { theme::MUTED } else { theme::FAINT }
                }
            },
        } ],
    }
}
