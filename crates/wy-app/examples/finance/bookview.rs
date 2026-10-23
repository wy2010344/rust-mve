//! 盘口行：移植 ooc 版 `dashboard/orderbook-rows.ts`。
//!
//! 口径对齐币安现货：
//! - 三列 Price / Amount / Total，Total 是「该档挂单的名义金额」= Σ(price × qty)；
//! - 精度选择器按最小步长 ×10 递进，选中后同价桶合并（卖价上取整、买价下取整）；
//! - 深度条两侧都贴右生长，宽度按两侧共用的分母算，超长的墙截到 100%。
//!
//! 样式不跟币安走，这里只负责数据口径。

use crate::market::Depth;

/// 深度条口径：单档名义金额 / 从最优价外推的累计金额。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BarMode {
    /// 单档名义金额（币安 Amount）。
    Amount,
    /// 从最优价外推的累计金额（币安 Cumulative）。
    Cumulative,
}

/// 盘口显示模式：双侧 / 只看买 / 只看卖。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BookMode {
    Both,
    Bids,
    Asks,
}

impl BookMode {
    pub fn shows_bids(self) -> bool {
        !matches!(self, BookMode::Asks)
    }

    pub fn shows_asks(self) -> bool {
        !matches!(self, BookMode::Bids)
    }
}

/// 一行盘口。
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BookRow {
    pub price: f64,
    pub qty: f64,
    /// 该档名义金额（币安 Total 列）。
    pub total: f64,
    /// 深度条宽度，0~100。
    pub percent: f32,
    /// `true` = 买盘。
    pub bid: bool,
}

/// 盘口视图。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BookView {
    /// 卖盘（**远价在前**，最优卖价紧贴中间行）。
    pub asks: Vec<BookRow>,
    /// 买盘（最优价在前）。
    pub bids: Vec<BookRow>,
    /// 买盘占两侧挂单金额的比例，0..1。
    pub buy_ratio: f64,
    /// 实际生效的聚合步长。
    pub step: f64,
}

impl BookView {
    pub fn is_empty(&self) -> bool {
        self.asks.is_empty() && self.bids.is_empty()
    }
}

/// 一侧的汇总：均价与合计名义金额（「Display Avg.&Sum」用）。
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SideSummary {
    pub avg: f64,
    pub sum: f64,
}

pub fn side_summary(rows: &[BookRow]) -> SideSummary {
    let mut qty = 0.0;
    let mut sum = 0.0;
    for row in rows {
        qty += row.qty;
        sum += row.total;
    }
    SideSummary {
        avg: if qty > 0.0 { sum / qty } else { 0.0 },
        sum,
    }
}

/// 最小步长由价格量级决定：BTC 0.01、XRP 0.0001 这个档。
fn base_step(price: f64) -> f64 {
    if !price.is_finite() || price <= 0.0 {
        return 0.01;
    }
    if price >= 1.0 {
        0.01
    } else if price >= 0.01 {
        0.0001
    } else {
        0.000_001
    }
}

/// 聚合精度档位：最小步长往上乘 10。
pub fn steps(price: f64) -> Vec<f64> {
    let base = base_step(price);
    [1.0, 10.0, 100.0, 1000.0]
        .iter()
        .map(|tier| round_to(base * tier, 8))
        .collect()
}

fn round_to(value: f64, digits: u32) -> f64 {
    let scale = 10f64.powi(digits as i32);
    (value * scale).round() / scale
}

/// 步长的小数位，用来消掉 `key * step` 的浮点噪声。
fn decimals_of(step: f64) -> i32 {
    let text = format!("{step}");
    match text.find('.') {
        None => 0,
        Some(dot) => (text.len() - dot - 1) as i32,
    }
}

/// 最优价，缺一边就用另一边兜底。
pub fn best_price(depth: Option<&Depth>) -> f64 {
    let Some(d) = depth else { return 0.0 };
    let bid = d.bids.first().map(|l| l.price).unwrap_or(0.0);
    let ask = d.asks.first().map(|l| l.price).unwrap_or(0.0);
    if bid > 0.0 {
        bid
    } else {
        ask
    }
}

/// 从最优价往外走，把同一步长桶里的挂单并成一档，凑够 `count` 档就停。
fn aggregate(levels: &[crate::market::Level], bid: bool, step: f64, count: usize) -> Vec<BookRow> {
    let mut rows = Vec::new();
    if levels.is_empty() || count == 0 {
        return rows;
    }
    let decimals = decimals_of(step);
    // 桶下标带相对容差：`99.9 / 0.1` 在 f64 里是 998.999…，不修会让同一档
    // 被拆成两个桶（真实交易所返回的报价就带这种噪声）。
    let bucket_of = |price: f64| {
        let ratio = price / step;
        let eps = ratio.abs() * 1e-9 + 1e-12;
        if bid {
            (ratio + eps).floor()
        } else {
            (ratio - eps).ceil()
        }
    };

    let mut key = bucket_of(levels[0].price);
    let mut qty = 0.0;
    let mut total = 0.0;
    for level in levels {
        // 脏数据（0 / 负 / NaN）不参与聚合
        if !level.qty.is_finite() || level.qty <= 0.0 {
            continue;
        }
        let next = bucket_of(level.price);
        if next != key {
            rows.push(BookRow {
                price: round_to(key * step, decimals.max(0) as u32),
                qty,
                total,
                percent: 0.0,
                bid,
            });
            if rows.len() >= count {
                return rows;
            }
            key = next;
            qty = 0.0;
            total = 0.0;
        }
        qty += level.qty;
        total += level.price * level.qty;
    }
    if rows.len() < count && qty > 0.0 {
        rows.push(BookRow {
            price: round_to(key * step, decimals.max(0) as u32),
            qty,
            total,
            percent: 0.0,
            bid,
        });
    }
    rows
}

/// 生成盘口视图。
///
/// `step` 传 0（或不在当前档位里）时按价格量级自动取最小步长。
/// 返回的 `asks` 已反转（远价在前），界面直接顺序渲染即可。
pub fn view(depth: Option<&Depth>, count: usize, step: f64, bar_mode: BarMode) -> BookView {
    let Some(depth) = depth else {
        return BookView {
            buy_ratio: 0.5,
            step: base_step(0.0),
            ..BookView::default()
        };
    };
    if depth.bids.is_empty() && depth.asks.is_empty() {
        return BookView {
            buy_ratio: 0.5,
            step: base_step(0.0),
            ..BookView::default()
        };
    }
    let options = steps(best_price(Some(depth)));
    let effective = if options.iter().any(|s| (*s - step).abs() < 1e-12) {
        step
    } else {
        options[0]
    };
    let mut asks = aggregate(&depth.asks, false, effective, count);
    let mut bids = aggregate(&depth.bids, true, effective, count);

    let mut max = 0.0f64;
    let mut bid_sum = 0.0;
    let mut ask_sum = 0.0;
    for row in &asks {
        max = max.max(row.total);
        ask_sum += row.total;
    }
    for row in &bids {
        max = max.max(row.total);
        bid_sum += row.total;
    }
    // Amount 共用「单档最大名义值」；Cumulative 共用「两侧累计到最深处」的较大者。
    let scale = match bar_mode {
        BarMode::Cumulative => ask_sum.max(bid_sum),
        BarMode::Amount => max,
    };
    let denom = if scale > 0.0 { scale } else { 1.0 };
    apply_bars(&mut asks, denom, bar_mode);
    apply_bars(&mut bids, denom, bar_mode);
    let both = bid_sum + ask_sum;
    asks.reverse();
    BookView {
        asks,
        bids,
        buy_ratio: if both > 0.0 { bid_sum / both } else { 0.5 },
        step: effective,
    }
}

/// 单侧统一算条宽；Cumulative 按「最优在前」逐档累加，最深处满格。
fn apply_bars(rows: &mut [BookRow], denom: f64, mode: BarMode) {
    match mode {
        BarMode::Amount => {
            for row in rows {
                row.percent = ((row.total / denom) * 100.0).min(100.0) as f32;
            }
        }
        BarMode::Cumulative => {
            let mut running = 0.0;
            for row in rows {
                running += row.total;
                row.percent = ((running / denom) * 100.0).min(100.0) as f32;
            }
        }
    }
}

/// 总列格式化：大数给 K/M/B，小值给到 6 位小数。
///
/// `exact = true` 对应「Rounding 关闭」：完整数值 + 千分位。
pub fn format_total(value: f64, exact: bool) -> String {
    if !value.is_finite() {
        return String::from("--");
    }
    if exact {
        // 完整数值也只保留 2 位小数：一是列宽只有几十像素，二是盘口
        // 名义金额精确到分就够了（再多只会把 Total 列撑出面板）
        return thousands(&trim_zeros(&format!("{value:.2}")));
    }
    if value >= 1e9 {
        format!("{:.2}B", value / 1e9)
    } else if value >= 1e6 {
        format!("{:.2}M", value / 1e6)
    } else if value >= 1e3 {
        format!("{:.2}K", value / 1e3)
    } else if value >= 1.0 {
        format!("{value:.2}")
    } else {
        format!("{value:.6}")
    }
}

fn trim_zeros(text: &str) -> String {
    if !text.contains('.') {
        return text.to_string();
    }
    let trimmed = text.trim_end_matches('0').trim_end_matches('.');
    trimmed.to_string()
}

fn thousands(text: &str) -> String {
    let (head, tail) = match text.find('.') {
        Some(dot) => (&text[..dot], &text[dot..]),
        None => (text, ""),
    };
    let mut out = String::new();
    for (i, ch) in head.chars().enumerate() {
        if i > 0 && (head.len() - i) % 3 == 0 && ch.is_ascii_digit() {
            out.push(',');
        }
        out.push(ch);
    }
    out.push_str(tail);
    out
}

/// 精度标签：0.01 / 0.1 / 1 / 10 …（去掉多余的 0）。
pub fn format_step(step: f64) -> String {
    if !step.is_finite() || step <= 0.0 {
        return String::from("--");
    }
    trim_zeros(&format!("{step}"))
}

/// 占比标签：0~1 的比例 → 一位小数的百分数（不带 % 号）。
pub fn format_share(ratio: f64) -> String {
    let v = if ratio.is_finite() {
        ratio.clamp(0.0, 1.0)
    } else {
        0.5
    };
    format!("{:.1}", v * 100.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::market::Level;

    fn depth(bids: &[(f64, f64)], asks: &[(f64, f64)]) -> Depth {
        Depth {
            bids: bids
                .iter()
                .map(|(p, q)| Level { price: *p, qty: *q })
                .collect(),
            asks: asks
                .iter()
                .map(|(p, q)| Level { price: *p, qty: *q })
                .collect(),
            time: 0,
        }
    }

    #[test]
    fn aggregates_same_bucket_and_keeps_best_first() {
        // 步长 0.1：99.99 / 99.98 落进同一个买盘桶 99.9，99.90 自己一桶
        let d = depth(
            &[(99.99, 1.0), (99.98, 2.0), (99.85, 3.0)],
            &[(100.01, 1.0)],
        );
        let v = view(Some(&d), 10, 0.1, BarMode::Amount);
        assert_eq!(v.step, 0.1);
        assert_eq!(v.bids.len(), 2, "两档应合并成一档");
        assert!((v.bids[0].qty - 3.0).abs() < 1e-9);
        assert!((v.bids[0].price - 99.9).abs() < 1e-9, "桶价应为 99.9");
        // 卖盘已反转：远价在前
        assert!(v.asks.is_empty() || v.asks[0].price >= 100.0);
    }

    /// 边界档位不能因为浮点噪声被拆开（真实交易所报价就长这样）。
    #[test]
    fn bucket_edges_survive_float_noise() {
        // 99.9 恰好落在 0.1 的档位上：bid 应归入 99.9（而不是掉到 99.8）
        let d = depth(&[(99.9, 1.0)], &[(100.1, 1.0)]);
        let v = view(Some(&d), 10, 0.1, BarMode::Amount);
        assert!(
            (v.bids[0].price - 99.9).abs() < 1e-9,
            "买到 {}",
            v.bids[0].price
        );
        assert!(
            (v.asks[0].price - 100.1).abs() < 1e-9,
            "卖到 {}",
            v.asks[0].price
        );
    }

    #[test]
    fn total_is_notional_and_ratio_sums_to_one() {
        let d = depth(&[(99.0, 2.0)], &[(101.0, 3.0)]);
        let v = view(Some(&d), 10, 0.01, BarMode::Amount);
        assert!((v.bids[0].total - 198.0).abs() < 1e-9);
        assert!((v.asks[0].total - 303.0).abs() < 1e-9);
        assert!((v.buy_ratio - 198.0 / 501.0).abs() < 1e-9);
    }

    #[test]
    fn cumulative_bars_grow_to_full_depth() {
        let d = depth(&[(99.0, 1.0), (98.0, 1.0), (97.0, 1.0)], &[(101.0, 1.0)]);
        let v = view(Some(&d), 10, 0.01, BarMode::Cumulative);
        let last = v.bids.last().unwrap();
        assert!((last.percent - 100.0).abs() < 0.01, "最深处应满格");
        assert!(v.bids[0].percent < last.percent);
    }

    #[test]
    fn step_options_scale_with_price_magnitude() {
        let btc = steps(60_000.0);
        assert_eq!(btc[0], 0.01);
        assert_eq!(btc[3], 10.0);
        let xrp = steps(0.52);
        assert_eq!(xrp[0], 0.0001);
    }

    #[test]
    fn total_formatting_switches_units() {
        assert_eq!(format_total(1_500_000.0, false), "1.50M");
        assert_eq!(format_total(2_508.908_813, true), "2,508.91");
        assert_eq!(format_total(f64::NAN, false), "--");
    }

    #[test]
    fn empty_depth_is_empty_view() {
        let v = view(None, 10, 0.0, BarMode::Amount);
        assert!(v.is_empty());
        assert!((v.buy_ratio - 0.5).abs() < 1e-9);
    }
}
