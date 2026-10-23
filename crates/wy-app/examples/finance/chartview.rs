//! K 线视口与几何：移植 ooc 版 `dashboard/chart-view.ts`。
//!
//! 视口用「可见根数 + 距最新一根的偏移」表示：
//! `offset = 0` 贴着右边缘（跟随最新），`offset > 0` 表示往回翻了几根。
//!
//! 与 ooc 的差别只在坐标系：那边输出百分比（直接写 DOM style），
//! 这边输出**绘图区局部像素**（Scene 图元要像素）。换算只发生在
//! 滚轮锚点与拖拽位移两处，其余逻辑逐一对应。

use crate::market::{format_price, Candle};

/// 一根 K 线最少 / 最多显示多少根。
pub const MIN_VISIBLE: i64 = 12;
pub const MAX_VISIBLE: i64 = 360;
pub const DEFAULT_VISIBLE: i64 = 90;

/// 价格区占绘图高度的比例，下方留给成交量柱。
pub const PRICE_AREA: f32 = 0.78;
/// 成交量柱区的顶边（比例）。
pub const VOLUME_TOP: f32 = 0.82;

const MAX_PRICE_TICKS: i32 = 8;
const MIN_PRICE_TICKS: usize = 2;

/// 可见区间。
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Frame {
    pub total: usize,
    /// 实际可见根数（数据不足时会小于配置值）。
    pub count: usize,
    /// 距最新一根的偏移。
    pub offset: usize,
    pub start: usize,
    pub end: usize,
}

/// 一根 K 线的盒（绘图区局部像素）。
#[derive(Clone, Copy, Debug)]
pub struct CandleBox {
    pub left: f32,
    pub width: f32,
    pub up: bool,
    pub body_top: f32,
    pub body_h: f32,
    pub wick_top: f32,
    pub wick_h: f32,
    pub vol_top: f32,
    pub vol_h: f32,
}

/// 价格刻度。
#[derive(Clone, Copy, Debug)]
pub struct PriceTick {
    pub value: f64,
    pub y: f32,
}

/// 时间轴标签对齐。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TickAlign {
    Start,
    Mid,
    End,
}

/// 时间刻度。
#[derive(Clone, Debug)]
pub struct TimeTick {
    pub x: f32,
    pub text: String,
    pub align: TickAlign,
}

/// 一次布局的全部产物。
///
/// 字段是给"消费方"（绘制、交互、测试）共用的完整快照，
/// 其中 `frame` / `pinned` / `plot_w` 目前只有调试与测试在读，故整体放行 dead_code。
#[allow(dead_code)]
#[derive(Clone, Debug, Default)]
pub struct ChartLayout {
    pub has_data: bool,
    pub frame: Frame,
    /// 可见区间的原始 K 线（悬停取用）。
    pub candles: Vec<Candle>,
    pub boxes: Vec<CandleBox>,
    pub price_ticks: Vec<PriceTick>,
    pub time_ticks: Vec<TimeTick>,
    /// 含上下留白的价格范围。
    pub high: f64,
    pub low: f64,
    pub last: f64,
    /// 最新价在价格区内的 y。
    pub last_y: f32,
    /// 最新一根是否在可见区间内。
    pub show_last: bool,
    /// 可见区间的时长（毫秒），用于时间轴粒度。
    pub span_ms: i64,
    pub pinned: bool,
    /// 价格区 / 绘图区高度。
    pub price_h: f32,
    pub plot_h: f32,
    /// 绘图区宽度与单根 K 线的横向步长。
    pub plot_w: f32,
    pub step: f32,
}

fn clamp_i64(value: i64, min: i64, max: i64) -> i64 {
    value.max(min).min(max)
}

/// 实际可见根数：不超过数据量、不低于下限。
pub fn visible_count(total: usize, count: i64) -> i64 {
    let max = (total as i64).clamp(1, MAX_VISIBLE);
    let min = MIN_VISIBLE.min(max);
    clamp_i64(if count > 0 { count } else { DEFAULT_VISIBLE }, min, max)
}

fn clamp_offset(total: usize, count: i64, offset: i64) -> i64 {
    clamp_i64(offset.max(0), 0, (total as i64 - count).max(0))
}

/// 计算可见区间。
pub fn chart_frame(total: usize, count: i64, offset: i64) -> Frame {
    let c = visible_count(total, count);
    let off = clamp_offset(total, c, offset);
    let end = (total as i64 - off).max(0) as usize;
    let start = (end as i64 - c).max(0) as usize;
    Frame {
        total,
        count: c as usize,
        offset: off as usize,
        start,
        end,
    }
}

/// 以锚点（0..1，光标横向比例）为中心缩放；锚点下的那根 K 线保持不动。
pub fn zoom_frame(total: usize, count: i64, offset: i64, factor: f64, anchor: f64) -> (i64, i64) {
    let frame = chart_frame(total, count, offset);
    let span = (frame.end as i64 - frame.start as i64).max(1);
    let ratio = if anchor.is_finite() {
        anchor.clamp(0.0, 1.0)
    } else {
        0.5
    };
    let at = frame.start as f64 + ratio * span as f64;
    let factor = if factor.is_finite() { factor } else { 1.0 };
    let next = visible_count(total, (frame.count as f64 * factor).round() as i64);
    let next_start = (at - ratio * next as f64).round() as i64;
    let off = clamp_offset(total, next, total as i64 - next - next_start);
    (next, off)
}

/// 平移：`delta > 0` 表示往更早的方向翻（内容向右移动）。
pub fn pan_frame(total: usize, count: i64, offset: i64, delta: f64) -> (i64, i64) {
    let frame = chart_frame(total, count, offset);
    let off = clamp_offset(
        total,
        frame.count as i64,
        frame.offset as i64 + delta.round() as i64,
    );
    (frame.count as i64, off)
}

/// 拖拽平移：像素位移换算成根数。
pub fn pan_by_pixels(total: usize, count: i64, offset: i64, dx: f32, plot_w: f32) -> (i64, i64) {
    let frame = chart_frame(total, count, offset);
    let per = if plot_w > 0.0 {
        plot_w / (frame.end as i64 - frame.start as i64).max(1) as f32
    } else {
        12.0
    };
    let delta = if dx.is_finite() {
        (dx / per) as f64
    } else {
        0.0
    };
    pan_frame(total, count, offset, delta)
}

/// 滚轮缩放：`delta` 为带符号滚动量（正 = 向下 = 缩小）。
pub fn zoom_by_wheel(total: usize, count: i64, offset: i64, delta: f32, anchor: f64) -> (i64, i64) {
    // `wheel` 以「行」为单位（引擎已把像素归一化），一行约 20% 缩放。
    let factor = ((delta as f64) * 0.18).exp().clamp(0.5, 2.0);
    zoom_frame(total, count, offset, factor, anchor)
}

/// 「好看」的刻度步长：1 / 2 / 2.5 / 5 / 10 × 10ⁿ（按对数距离取最近）。
fn nice_step(raw: f64) -> f64 {
    if !raw.is_finite() || raw <= 0.0 {
        return 1.0;
    }
    let power = 10f64.powf(raw.log10().floor());
    let fraction = raw / power;
    let candidates = [1.0, 2.0, 2.5, 5.0, 10.0];
    let mut best = candidates[0];
    let mut best_distance = f64::INFINITY;
    for c in candidates {
        let d = (c / fraction).ln().abs();
        if d < best_distance {
            best_distance = d;
            best = c;
        }
    }
    best * power
}

/// 价格轴刻度：目标 6 段。
fn price_ticks_of(high: f64, low: f64, y_of: impl Fn(f64) -> f32) -> Vec<PriceTick> {
    let span = high - low;
    if !span.is_finite() || span <= 0.0 {
        return Vec::new();
    }
    let step = nice_step(span / 6.0);
    let mut value = (low / step).ceil() * step;
    let mut out = Vec::new();
    for _ in 0..(MAX_PRICE_TICKS + 2) {
        if value > high {
            break;
        }
        // 去掉浮点尾差，刻度值才和格式化结果一致
        let rounded = tidy(value);
        out.push(PriceTick {
            value: rounded,
            y: y_of(rounded),
        });
        value += step;
    }
    if out.len() < MIN_PRICE_TICKS {
        return Vec::new();
    }
    out
}

/// 按 12 位有效数字收尾（消掉 0.30000000000000004 这类噪声）。
fn tidy(value: f64) -> f64 {
    if !value.is_finite() || value == 0.0 {
        return value;
    }
    let digits = 12i32 - value.abs().log10().floor() as i32 - 1;
    let scale = 10f64.powi(digits.max(0));
    (value * scale).round() / scale
}

const MS_HOUR: i64 = 3_600_000;

/// 时间轴标签：跨度越长越粗，避免一屏出现两段同样的 12:30。
pub fn format_axis_time(ms: i64, span_ms: i64) -> String {
    let (month, day, hour, minute) = local_parts(ms);
    if span_ms >= 36 * MS_HOUR {
        format!("{month:02}-{day:02}")
    } else if span_ms >= 20 * MS_HOUR {
        format!("{month:02}-{day:02} {hour:02}:{minute:02}")
    } else {
        format!("{hour:02}:{minute:02}")
    }
}

/// 毫秒 → (月, 日, 时, 分)，固定 +8 时区（与 `market::format_time` 一致）。
fn local_parts(ms: i64) -> (i64, i64, i64, i64) {
    let secs = ms / 1000 + 8 * 3600;
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    let _ = y;
    (m, d, rem / 3600, (rem % 3600) / 60)
}

/// 天数 → 公历年月日（Howard Hinnant 的 civil_from_days 算法）。
fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// 价格轴格式：比行情页少一位视觉噪声，尽量塞进 62px 的轴列。
pub fn format_axis_price(value: f64) -> String {
    format_price(value)
}

fn time_ticks_of(candles: &[Candle], span_ms: i64, plot_w: f32, step: f32) -> Vec<TimeTick> {
    let n = candles.len();
    if n == 0 {
        return Vec::new();
    }
    // 每个标签按 96px 估算：宽面板多给几个，窄面板只留首尾。
    let want = ((plot_w / 96.0).floor() as i64).clamp(2, 7);
    let count = (want as usize).min(n);
    let mut out: Vec<TimeTick> = Vec::new();
    let mut previous = String::new();
    for i in 0..count {
        let index = if count == 1 {
            0
        } else {
            (i * (n - 1) / (count - 1)).min(n - 1)
        };
        let text = format_axis_time(candles[index].time, span_ms);
        // 跨度大时标签只到「日」，相邻两个可能撞成同一个字：跳过重复的。
        if text == previous {
            continue;
        }
        previous = text;
        out.push(TimeTick {
            x: step * (index as f32 + 0.5),
            text: previous.clone(),
            align: if i == 0 {
                TickAlign::Start
            } else if i + 1 == count {
                TickAlign::End
            } else {
                TickAlign::Mid
            },
        });
    }
    out
}

/// 构建完整布局（对应 ooc `buildLayout`）。
pub fn layout(all: &[Candle], count: i64, offset: i64, plot_w: f32, plot_h: f32) -> ChartLayout {
    let frame = chart_frame(all.len(), count, offset);
    let price_h = plot_h * PRICE_AREA;
    let visible = &all[frame.start..frame.end];
    let mut out = ChartLayout {
        frame,
        price_h,
        plot_h,
        plot_w,
        pinned: frame.offset == 0,
        ..ChartLayout::default()
    };
    if visible.is_empty() {
        return out;
    }

    let mut raw_high = f64::MIN;
    let mut raw_low = f64::MAX;
    let mut vol_max = 0.0f64;
    for c in visible {
        raw_high = raw_high.max(c.high);
        raw_low = raw_low.min(c.low);
        vol_max = vol_max.max(c.volume);
    }
    // 上下各留 8% 呼吸位，避免影线贴边、最新价标签压到刻度上。
    let pad = (raw_high - raw_low) * 0.08;
    let pad = if pad > 0.0 {
        pad
    } else {
        (raw_high.abs() * 0.0006).max(1e-9)
    };
    let high = raw_high + pad;
    let low = raw_low - pad;
    let span = if high - low > 0.0 { high - low } else { 1.0 };
    let y_of = |v: f64| ((high - v) / span) as f32 * price_h;

    let n = visible.len();
    let step = plot_w / n as f32;
    let body_w = (step * 0.62).max(1.0);
    let vol_range = vol_max.max(1e-12);
    let vol_h_area = plot_h - plot_h * VOLUME_TOP;
    let boxes: Vec<CandleBox> = visible
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let open_y = y_of(c.open);
            let close_y = y_of(c.close);
            let vh = (c.volume / vol_range) as f32 * vol_h_area;
            CandleBox {
                left: i as f32 * step + step / 2.0 - body_w / 2.0,
                width: body_w,
                up: c.close >= c.open,
                body_top: open_y.min(close_y),
                body_h: (open_y - close_y).abs().max(0.8),
                wick_top: y_of(c.high),
                wick_h: (y_of(c.low) - y_of(c.high)).max(0.8),
                vol_top: plot_h - vh,
                vol_h: vh,
            }
        })
        .collect();

    let last_visible = visible.last().unwrap();
    let newest = all.last();
    let last = newest.map(|c| c.close).unwrap_or(last_visible.close);
    out.last_y = y_of(last);
    out.show_last = frame.offset == 0;
    out.span_ms = if n > 1 {
        last_visible.time - visible[0].time
    } else {
        0
    };

    // 最新价标签是实心的，压住附近的刻度更干净（和币安一样让位）
    out.price_ticks = price_ticks_of(high, low, y_of)
        .into_iter()
        .filter(|t| (t.y - out.last_y).abs() > 3.5)
        .collect();
    out.time_ticks = time_ticks_of(visible, out.span_ms, plot_w, step);
    out.candles = visible.to_vec();
    out.boxes = boxes;
    out.has_data = true;
    out.high = high;
    out.low = low;
    out.last = last;
    out.step = step;
    out
}

/// 光标 → 十字线与 OHLC；x 吸附到最近的 K 线，y 只决定价格。
#[allow(dead_code)]
#[derive(Clone, Debug)]
pub struct Hover {
    pub candle: Candle,
    /// 光标纵坐标对应的价格。
    pub price: f64,
    /// 吸附到 K 线中心的横向像素位置。
    pub x: f32,
    pub y: f32,
    pub index: usize,
    /// 相对上一根的涨跌幅（第一根为 None）。
    pub change_pct: Option<f64>,
    pub time_text: String,
}

/// 光标比例（0..1）→ 悬停信息。
pub fn hover(layout: &ChartLayout, ratio_x: f32, ratio_y: f32) -> Option<Hover> {
    let n = layout.candles.len();
    if n == 0 || !ratio_x.is_finite() || !ratio_y.is_finite() {
        return None;
    }
    let index = ((ratio_x.clamp(0.0, 0.9999) * n as f32).floor() as usize).min(n - 1);
    let candle = layout.candles[index].clone();
    let x = layout.step * (index as f32 + 0.5);
    let y = (ratio_y.clamp(0.0, 1.0) * layout.plot_h).min(layout.price_h);
    let span = layout.high - layout.low;
    let price = layout.high - (y / layout.price_h) as f64 * span;
    let prev = if index > 0 {
        Some(layout.candles[index - 1].close)
    } else {
        None
    };
    let change_pct = prev.and_then(|p| {
        if p > 0.0 {
            Some((candle.close - p) / p * 100.0)
        } else {
            None
        }
    });
    let time_text = format_axis_time(candle.time, layout.span_ms);
    Some(Hover {
        candle,
        price,
        x,
        y,
        index,
        change_pct,
        time_text,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candles(n: usize) -> Vec<Candle> {
        (0..n)
            .map(|i| Candle {
                time: 1_700_000_000_000 + i as i64 * 900_000,
                open: 100.0 + i as f64,
                high: 102.0 + i as f64,
                low: 99.0 + i as f64,
                close: 101.0 + i as f64,
                volume: 1.0 + i as f64,
            })
            .collect()
    }

    #[test]
    fn visible_count_respects_bounds() {
        assert_eq!(visible_count(1000, 5), MIN_VISIBLE);
        assert_eq!(visible_count(1000, 9_999), MAX_VISIBLE);
        assert_eq!(visible_count(30, 90), 30);
        assert_eq!(visible_count(0, 90), 1, "无数据时也至少 1 根");
    }

    #[test]
    fn frame_pins_to_latest_when_offset_zero() {
        let f = chart_frame(500, 90, 0);
        assert_eq!(f.end, 500);
        assert_eq!(f.start, 410);
    }

    #[test]
    fn zoom_keeps_anchor_candle_under_cursor() {
        let total = 500;
        let (count, offset) = zoom_frame(total, 90, 0, 0.5, 0.5);
        assert_eq!(count, 45);
        // 锚点在中间：可见区间中点应保持在同一位置（允许 1 根误差）
        let before = chart_frame(total, 90, 0);
        let after = chart_frame(total, count, offset);
        let mid_before = (before.start + before.end) as f64 / 2.0;
        let mid_after = (after.start + after.end) as f64 / 2.0;
        assert!(
            (mid_before - mid_after).abs() <= 1.0,
            "{mid_before} vs {mid_after}"
        );
    }

    #[test]
    fn pan_is_clamped_to_available_history() {
        let (count, offset) = pan_frame(500, 90, 0, 10_000.0);
        assert_eq!(count, 90);
        assert_eq!(offset, 410);
        let (_, back) = pan_frame(500, 90, offset, -10_000.0);
        assert_eq!(back, 0);
    }

    #[test]
    fn layout_has_ticks_and_boxes() {
        let data = candles(120);
        let l = layout(&data, 90, 0, 800.0, 300.0);
        assert!(l.has_data);
        assert_eq!(l.boxes.len(), 90);
        assert!(!l.price_ticks.is_empty());
        assert!(!l.time_ticks.is_empty());
        assert!(l.show_last);
        // 最新价落在价格区内
        assert!(l.last_y >= 0.0 && l.last_y <= l.price_h + 0.5);
    }

    #[test]
    fn hover_snaps_to_candle_center() {
        let data = candles(90);
        let l = layout(&data, 90, 0, 800.0, 300.0);
        let h = hover(&l, 0.0, 0.5).expect("有悬停");
        assert_eq!(h.index, 0);
        assert!((h.x - l.step * 0.5).abs() < 0.01);
    }

    #[test]
    fn axis_time_switches_granularity() {
        let t = 1_700_000_000_000;
        assert_eq!(
            format_axis_time(t, 900_000 * 30).len(),
            5,
            "短跨度只给 HH:MM"
        );
        assert!(
            format_axis_time(t, MS_HOUR * 24).contains('-'),
            "长跨度带日期"
        );
    }
}
