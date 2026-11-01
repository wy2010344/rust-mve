//! 24h 统计面板（v2）：若干"标签 —— 值"行。
//!
//! 与 v1 同语义：行定高、整体进滚动容器；取值表达式直接作为 getter 传入，
//! 绘制期读信号、变动自动重绘。文本一律走 `text_label!` 子节点，不手绘。

use std::rc::Rc;

use wy_mve::canvas_render2::node2::Ext;
use wy_mve::{flex, grow, node2, AlignItem, Fc, Layout, Node2};
use wy_render::{Color, Rect, Scene};
use wy_signal::GetValue;

use super::widgets::scroll_view;
use crate::market::{format_compact, format_percent, format_price};
use crate::{theme, Ctl, Model};

/// 统计面板：标题栏 + 滚动行。
pub fn stats_panel(model: Rc<Model>, _ctl: Rc<Ctl>, exts: Vec<Ext>) -> Fc {
    // getter 值即"绘制期求值的表达式"——宏会把它包进 `move ||`，
    // 所以这里写裸表达式即可（不要自己再套一层 `move ||`）。
    // 每个 getter 由宏独立包一层 `move ||`，会捕获各自的 `m` 克隆，
    // 故每行先 `Rc::clone(&model)` 出一个独立句柄，避免"移动一次"冲突。
    // 每个 getter 会被宏独立包一层 `move ||`，各自捕获闭包里用到的句柄。
    // 所以**一处 getter 用一份克隆**：`value` 与 `tone` 共用同一个 `Rc` 会
    // 被第二个闭包抢走（移动后再次使用）。
    let rows: Vec<Fc> = vec![
        {
            let v = Rc::clone(&model);
            kv_row! {
                label: "HIGH".to_string(),
                value: v.ticker.get().map(|t| format_price(t.high)).unwrap_or_default(),
                tone: theme::TEXT,
            }
        },
        {
            let v = Rc::clone(&model);
            kv_row! {
                label: "LOW".to_string(),
                value: v.ticker.get().map(|t| format_price(t.low)).unwrap_or_default(),
                tone: theme::TEXT,
            }
        },
        {
            let v = Rc::clone(&model);
            kv_row! {
                label: "OPEN".to_string(),
                value: v.ticker.get().map(|t| format_price(t.open)).unwrap_or_default(),
                tone: theme::MUTED,
            }
        },
        // 下面两行 `value` 与 `tone` 都要读 model：各取一份克隆。
        {
            let v = Rc::clone(&model);
            let t = Rc::clone(&model);
            kv_row! {
                label: "CHANGE".to_string(),
                value: v.ticker.get().map(|t| format_price(t.change)).unwrap_or_default(),
                tone: t.ticker.get().map(|t| theme::tone(t.up())).unwrap_or(theme::MUTED),
            }
        },
        {
            let v = Rc::clone(&model);
            let t = Rc::clone(&model);
            kv_row! {
                label: "CHANGE %".to_string(),
                value: v.ticker.get().map(|t| format_percent(t.change_percent)).unwrap_or_default(),
                tone: t.ticker.get().map(|t| theme::tone(t.up())).unwrap_or(theme::MUTED),
            }
        },
        {
            let v = Rc::clone(&model);
            kv_row! {
                label: "VOLUME".to_string(),
                value: v.ticker.get()
                    .map(|t| format!("{} {}", format_compact(t.volume), t.symbol.trim_end_matches("USDT")))
                    .unwrap_or_default(),
                tone: theme::MUTED,
            }
        },
        {
            let v = Rc::clone(&model);
            kv_row! {
                label: "QUOTE VOL".to_string(),
                value: v.ticker.get().map(|t| format_compact(t.quote_volume)).unwrap_or_default(),
                tone: theme::MUTED,
            }
        },
        {
            let v = Rc::clone(&model);
            kv_row! {
                label: "BEST BID".to_string(),
                value: v.ticker.get().map(|t| format_price(t.bid)).unwrap_or_default(),
                tone: theme::UP,
            }
        },
        {
            let v = Rc::clone(&model);
            kv_row! {
                label: "BEST ASK".to_string(),
                value: v.ticker.get().map(|t| format_price(t.ask)).unwrap_or_default(),
                tone: theme::DOWN,
            }
        },
        {
            let v = Rc::clone(&model);
            kv_row! {
                label: "SPREAD".to_string(),
                value: v.ticker.get().map(|t| format_price((t.ask - t.bid).max(0.0))).unwrap_or_default(),
                tone: theme::FAINT,
            }
        },
    ];

    panel! {
        debug_name: "panel-stats".to_string(),
        exts: exts,
        title: Rc::new(|| "MARKET STATS".to_string()),
        badge: Rc::new(|| "24H".to_string()),
        actions: vec![],
        children: vec![ stats_header(Rc::clone(&model)), scroll_view(0.0, rows) ],
    }
}

/// 统计面板顶部：24h 涨跌幅大字号 + 区间位置渐变条（对齐 ooc `stats.ooc`）。
///
/// 这条带是 OOC 行情终端的招牌元素：一根 rose→amber→emerald 的渐变条，填充到
/// 「最新价落在 [low, high] 的百分比」，一眼看出当前价在区间的哪一端。
fn stats_header(model: Rc<Model>) -> Fc {
    let m_pct = Rc::clone(&model);
    let m_pct_tone = Rc::clone(&model);
    let m_bar = Rc::clone(&model);
    let m_bar_label = Rc::clone(&model);

    node2! {
        height: 80.0,
        padding_inline_start: theme::PAD,
        padding_inline_end: theme::PAD,
        padding_block_start: 12.0,
        padding_block_end: 12.0,
        layout: flex! { direction: Layout::Column, align_item: AlignItem::Stretch, align_fix: true, gap: 8.0 },
        draw: move |s: &mut Scene, n: &Node2| {
            s.fill_rect(
                Rect::new(theme::PAD, n.height() - 1.0, (n.width() - 2.0 * theme::PAD).max(0.0), 1.0),
                theme::GRID,
            );
        },
        children: vec![
            // 第一行：标签 + 大字号涨跌幅
            node2! {
                height: 22.0,
                layout: flex! { direction: Layout::Row, align_item: AlignItem::Center, align_fix: true },
                children: vec![
                    text_label! { content: "24H CHANGE".to_string(), font_size: 10.0, color: theme::FAINT },
                    node2! { exts: vec![grow(1.0)], skip_draw: true },
                    text_label! {
                        content: { m_pct.ticker.get().map(|t| format_percent(t.change_percent)).unwrap_or_default() },
                        font_size: 14.0,
                        color: { m_pct_tone.ticker.get().map(|t| theme::tone(t.up())).unwrap_or(theme::MUTED) },
                    },
                ],
            },
            // 第二行：区间位置渐变条
            node2! {
                height: 4.0,
                draw: move |s: &mut Scene, n: &Node2| {
                    let w = n.width();
                    let h = n.height();
                    s.fill_round_rect(Rect::new(0.0, 0.0, w, h), h / 2.0, theme::GRID);
                    if let Some(t) = m_bar.ticker.get() {
                        let span = t.high - t.low;
                        let frac = if span > 0.0 {
                            (((t.last - t.low) / span) * 100.0).clamp(0.0, 100.0)
                        } else {
                            50.0
                        } / 100.0;
                        draw_range_gradient(s, 0.0, 0.0, (w * frac as f32).max(0.0), h);
                    }
                },
            },
            // 第三行：Low / 百分比 / High
            node2! {
                height: 12.0,
                layout: flex! { direction: Layout::Row, align_item: AlignItem::Center, align_fix: true },
                children: vec![
                    text_label! { content: "LOW".to_string(), font_size: 9.0, color: theme::FAINT },
                    node2! { exts: vec![grow(1.0)], skip_draw: true },
                    text_label! {
                        content: {
                            m_bar_label.ticker.get().map(|t| {
                                let span = t.high - t.low;
                                let p = if span > 0.0 { ((t.last - t.low) / span) * 100.0 } else { 50.0 };
                                format!("{:.1}%", p)
                            }).unwrap_or_else(|| "50.0%".to_string())
                        },
                        font_size: 9.0,
                        color: theme::MUTED,
                    },
                    node2! { exts: vec![grow(1.0)], skip_draw: true },
                    text_label! { content: "HIGH".to_string(), font_size: 9.0, color: theme::FAINT },
                ],
            },
        ],
    }
}

/// 区间位置渐变条：rose(0)→amber(0.5)→emerald(1) 三色插值，按 `w` 宽平铺。
///
/// 调用方只填 `w` 到「当前价在区间的百分比」，于是右端颜色落在渐变对应处
/// （对齐 ooc `from-rose-400 via-amber-300 to-emerald-400` 的填充段）。
fn draw_range_gradient(s: &mut Scene, x: f32, y: f32, w: f32, h: f32) {
    if w <= 0.0 {
        return;
    }
    let rose = (0xF4u32, 0x3Fu32, 0x5Eu32);
    let amber = (0xFCu32, 0xD3u32, 0x4Du32);
    let emerald = (0x34u32, 0xD3u32, 0x99u32);
    let steps = 48u32;
    let seg = w / steps as f32;
    for i in 0..steps {
        let t = (i as f32 + 0.5) / steps as f32;
        let (r, g, b) = if t < 0.5 {
            lerp3(rose, amber, t / 0.5)
        } else {
            lerp3(amber, emerald, (t - 0.5) / 0.5)
        };
        let c = Color::from_u32((r << 24) | (g << 16) | (b << 8) | 0xFF);
        // seg+1 消除相邻条带间的亚像素接缝
        s.fill_rect(Rect::new(x + t * w, y, seg + 1.0, h), c);
    }
}

/// 三段式线性插值，各通道都是 0..255 的整数。
fn lerp3(a: (u32, u32, u32), b: (u32, u32, u32), t: f32) -> (u32, u32, u32) {
    let k = t.clamp(0.0, 1.0);
    (
        (a.0 as f32 + (b.0 as f32 - a.0 as f32) * k) as u32,
        (a.1 as f32 + (b.1 as f32 - a.1 as f32) * k) as u32,
        (a.2 as f32 + (b.2 as f32 - a.2 as f32) * k) as u32,
    )
}
