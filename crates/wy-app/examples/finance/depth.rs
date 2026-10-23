//! 累计深度图几何：移植 ooc 版 `dashboard/depth.ts` + `depth-group.ts`。
//!
//! # 与 ooc 的关系
//!
//! ooc 的深度图分两层：
//! - `depth-group.ts`：把原始价位按**等宽价格桶**聚合，再累加成阶梯函数；
//! - `depth.ts`：以中间价为中心、固定展示 ±0.5%（整幅 1%），缩放只改跨度；
//!   刻度用 `niceStep` 取「好看」的步长。
//!
//! 这里逐函数照搬，唯一区别是输出从 SVG 路径字符串改成归一化坐标
//! （`0..1`），因为 Rust 侧 Scene 只有矩形/文本图元——阶梯面积正好可以
//! 用「每桶一根柱」精确画出来（桶内累计量恒定，本来就是阶梯）。

use crate::market::Depth;

/// 整幅展示 1%（中间价 ±0.5%），与 ooc / 币安默认档一致。
pub const RANGE_PCT: f64 = 1.0;
/// 每侧价格桶数量：真实盘口按桶聚合后累计曲线才均匀。
pub const BUCKETS: usize = 80;
pub const MIN_ZOOM: f64 = 1.0;
pub const MAX_ZOOM: f64 = 24.0;
const X_TICK_COUNT: usize = 5;

/// 单侧聚合结果。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Side {
    /// `totals[i]` = 从中间价到第 i 个桶边界的累计量（长度 = buckets + 1）。
    pub totals: Vec<f64>,
    /// 价格桶宽度。
    pub step: f64,
    /// 该侧累计总量。
    pub max: f64,
}

/// 深度图几何（对应 ooc `DepthGeometry`）。
#[derive(Clone, Debug, PartialEq)]
pub struct Geometry {
    pub has_data: bool,
    /// 中间价。
    pub mid: f64,
    /// 价格轴左/右端。
    pub min: f64,
    pub max: f64,
    pub half_span: f64,
    /// 当前跨度占中间价的百分比（±）。
    pub range_pct: f64,
    pub bid: Side,
    pub ask: Side,
    /// 累计量轴顶（取整到刻度步长）。
    pub axis_max: f64,
    /// 累计量轴步长。
    pub y_step: f64,
}

impl Default for Geometry {
    fn default() -> Self {
        Self {
            has_data: false,
            mid: 0.0,
            min: 0.0,
            max: 0.0,
            half_span: 0.0,
            range_pct: 0.0,
            bid: Side::default(),
            ask: Side::default(),
            axis_max: 0.0,
            y_step: 0.0,
        }
    }
}

impl Geometry {
    /// 价格 → 水平归一化坐标（0 = 左端，1 = 右端）。
    pub fn x_norm(&self, price: f64) -> f32 {
        let span = self.max - self.min;
        if span <= 0.0 {
            return 0.5;
        }
        (((price - self.min) / span) as f32).clamp(0.0, 1.0)
    }

    /// 累计量 → 垂直归一化坐标（0 = 底部，1 = 轴顶）。
    pub fn y_norm(&self, total: f64) -> f32 {
        if self.axis_max <= 0.0 {
            return 0.0;
        }
        (total / self.axis_max).clamp(0.0, 1.0) as f32
    }

    /// 价格轴刻度：`(价格, 0..1 归一化位置)`。
    pub fn x_ticks(&self) -> Vec<(f64, f32)> {
        if !self.has_data {
            return Vec::new();
        }
        (0..X_TICK_COUNT)
            .map(|i| {
                let t = i as f64 / (X_TICK_COUNT - 1) as f64;
                (self.min + (self.max - self.min) * t, t as f32)
            })
            .collect()
    }

    /// 累计量轴刻度：`(累计量, 0..1 归一化位置)`；含 0 与轴顶。
    pub fn y_ticks(&self) -> Vec<(f64, f32)> {
        if !self.has_data || self.y_step <= 0.0 {
            return Vec::new();
        }
        let mut out = Vec::new();
        let mut value = 0.0;
        while value <= self.axis_max + self.y_step * 0.001 {
            out.push((value, self.y_norm(value)));
            value += self.y_step;
        }
        out
    }
}

/// 缩放夹取到 `[1, 24]`。
pub fn clamp_zoom(zoom: f64) -> f64 {
    let z = if zoom.is_finite() { zoom } else { 1.0 };
    z.clamp(MIN_ZOOM, MAX_ZOOM)
}

/// 单侧聚合：把原始价位按等宽桶累加成阶梯函数。
///
/// bid 侧 offset = mid - price（离中间价越远 offset 越大），
/// ask 侧 offset = price - mid；两侧都只保留落在 `half_span` 内的档位。
pub fn group_side(levels: &[crate::market::Level], bid: bool, mid: f64, half_span: f64) -> Side {
    if levels.is_empty() || !half_span.is_finite() || half_span <= 0.0 || !mid.is_finite() {
        return Side::default();
    }
    let count = BUCKETS;
    let step = half_span / count as f64;
    let mut quantities = vec![0.0f64; count];

    for level in levels {
        let offset = if bid {
            mid - level.price
        } else {
            level.price - mid
        };
        if !offset.is_finite() || offset < 0.0 || offset > half_span {
            continue;
        }
        let index = ((offset / step).floor() as isize).clamp(0, count as isize - 1) as usize;
        quantities[index] += level.qty;
    }

    let mut totals = Vec::with_capacity(count + 1);
    totals.push(0.0);
    for q in &quantities {
        totals.push(totals[totals.len() - 1] + q);
    }
    let max = *totals.last().unwrap_or(&0.0);
    Side { totals, step, max }
}

/// 构建深度图几何（对应 ooc `depthGeometry`）。
pub fn geometry(depth: Option<&Depth>, zoom: f64) -> Geometry {
    let Some(depth) = depth else {
        return Geometry::default();
    };
    let Some(mid) = depth.mid() else {
        return Geometry::default();
    };
    let z = clamp_zoom(zoom);
    // 固定展示整幅 1%，缩放后跨度变窄，但中心价始终在 50%。
    let half_span = (mid * (RANGE_PCT / 200.0)).max(1e-8) / z;
    let bid = group_side(&depth.bids, true, mid, half_span);
    let ask = group_side(&depth.asks, false, mid, half_span);
    let (axis_max, y_step) = axis_scale(bid.max.max(ask.max).max(1e-8));
    Geometry {
        has_data: bid.max > 0.0 || ask.max > 0.0,
        mid,
        min: mid - half_span,
        max: mid + half_span,
        half_span,
        range_pct: half_span / mid * 100.0,
        bid,
        ask,
        axis_max,
        y_step,
    }
}

/// 悬停读数：光标横向比例 → 该价位与所在侧的累计量。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HoverInfo {
    pub price: f64,
    /// 从中间价累计到该价位的挂单量。
    pub total: f64,
    /// `true` = 买盘侧。
    pub bid: bool,
    /// 光标的横向比例（0..1），绘制层换算成像素。
    pub ratio_x: f32,
}

/// 光标横向比例 → 单侧悬停读数（中间价左侧算买盘，右侧算卖盘）。
///
/// 深度图面板用的是**对称读数对**（[`hover_pair`]）；这个单侧版本
/// 供只需要"光标所在那一侧读数"的场合使用。
#[allow(dead_code)]
pub fn hover(g: &Geometry, ratio_x: f32) -> Option<HoverInfo> {
    if !g.has_data || g.half_span <= 0.0 {
        return None;
    }
    let ratio = ratio_x.clamp(0.0, 1.0);
    let price = g.min + (g.max - g.min) * ratio as f64;
    let (bid, side) = if price <= g.mid {
        (true, &g.bid)
    } else {
        (false, &g.ask)
    };
    let offset = if bid { g.mid - price } else { price - g.mid };
    let index = if side.step > 0.0 {
        (offset / side.step).floor() as usize
    } else {
        0
    };
    // totals[i] 是「第 i 个桶之前」的累计量，落在桶 i 里的价位应取 totals[i + 1]
    let total = side
        .totals
        .get((index + 1).min(side.totals.len().saturating_sub(1)))
        .copied()
        .unwrap_or(0.0);
    Some(HoverInfo {
        price,
        total,
        bid,
        ratio_x: ratio,
    })
}

/// 对称悬停读数对（对应 ooc `depthHoverPair`）。
///
/// 深度图的**对称区**：光标在中间价左侧时，左侧读数取买盘侧、右侧读数
/// 取**镜像位置**的卖盘侧（反之亦然）。这样两条读数线关于中间价对称，
/// 中间那条亮带始终落在对称区中央——一眼能看出「这一带两侧深度差多少」。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HoverPair {
    /// 左侧读数（买盘侧）。
    pub left: HoverInfo,
    /// 右侧读数（卖盘侧）。
    pub right: HoverInfo,
    /// 两条读数线之间是否很近（中间亮带很窄）。
    pub wide: bool,
}

/// 光标横向比例 → 对称读数对。
///
/// `ratio` 是光标在绘图区里的归一化位置（0 = 左端，1 = 右端）。
///
/// **对称区**的定义：两条读数线关于**中间价**等距——
/// `d = |ratio - mid_x|`，左读数在 `mid_x - d`、右读数在 `mid_x + d`。
/// 这样无论光标在哪一侧，中间那条亮带都以中间价为轴，
/// 一眼能看出「这一带两侧深度差多少」。
pub fn hover_pair(g: &Geometry, ratio: f32) -> Option<HoverPair> {
    if !g.has_data || g.half_span <= 0.0 {
        return None;
    }
    let ratio = ratio.clamp(0.0, 1.0);
    let mid_x = g.x_norm(g.mid);
    // 光标到中间价的距离（像素比例）
    let d = (ratio - mid_x).abs();
    let left = hover_at(g, mid_x - d, true)?;
    let right = hover_at(g, mid_x + d, false)?;
    Some(HoverPair {
        left,
        right,
        wide: d < 0.15,
    })
}

/// 指定侧、指定横向位置（归一化）的读数。
fn hover_at(g: &Geometry, ratio_x: f32, bid: bool) -> Option<HoverInfo> {
    let side = if bid { &g.bid } else { &g.ask };
    let ratio_x = ratio_x.clamp(0.0, 1.0);
    let price = g.min + (g.max - g.min) * ratio_x as f64;
    let offset = if bid { g.mid - price } else { price - g.mid };
    let index = if side.step > 0.0 {
        (offset / side.step).floor() as usize
    } else {
        0
    };
    let total = side
        .totals
        .get((index + 1).min(side.totals.len().saturating_sub(1)))
        .copied()
        .unwrap_or(0.0);
    Some(HoverInfo {
        price,
        total,
        bid,
        ratio_x,
    })
}

/// 「好看」的刻度步长：1 / 1.25 / 1.5 / 2 / 2.5 / 3 / 4 / 5 / 10 × 10^n。
fn nice_step(value: f64) -> f64 {
    if !value.is_finite() || value <= 0.0 {
        return 1.0;
    }
    let power = 10f64.powf(value.log10().floor());
    let fraction = value / power;
    let candidates = [1.0, 1.25, 1.5, 2.0, 2.5, 3.0, 4.0, 5.0, 10.0];
    let nice = candidates
        .iter()
        .copied()
        .find(|c| fraction <= *c)
        .unwrap_or(10.0);
    nice * power
}

/// 累计量轴：目标 4 段，轴顶取整到刻度步长的整数倍。
fn axis_scale(raw_max: f64) -> (f64, f64) {
    let value = raw_max.max(1e-8);
    let step = nice_step(value / 4.0);
    let max = (value / step).ceil() * step;
    (max.max(step), step)
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
    fn mid_is_average_of_best_bid_ask() {
        let d = depth(&[(99.0, 1.0)], &[(101.0, 1.0)]);
        assert_eq!(geometry(Some(&d), 1.0).mid, 100.0);
    }

    #[test]
    fn totals_are_monotonic_and_match_sum() {
        let d = depth(
            &[(99.9, 2.0), (99.8, 3.0), (99.6, 5.0)],
            &[(100.1, 1.0), (100.2, 4.0)],
        );
        let g = geometry(Some(&d), 1.0);
        assert!(g.has_data);
        // 桶内累计单调不减
        for pair in g.bid.totals.windows(2) {
            assert!(pair[1] >= pair[0]);
        }
        // 落在 ±0.5% 内的档位全部被统计（这里全部在内）
        assert!((g.bid.max - 10.0).abs() < 1e-9);
        assert!((g.ask.max - 5.0).abs() < 1e-9);
    }

    #[test]
    fn out_of_range_levels_are_ignored() {
        // ±0.5% @ 100 → [99.5, 100.5]
        let d = depth(&[(90.0, 100.0), (99.9, 1.0)], &[(110.0, 100.0)]);
        let g = geometry(Some(&d), 1.0);
        assert!((g.bid.max - 1.0).abs() < 1e-9, "远价档位应被丢弃");
        assert!((g.ask.max - 0.0).abs() < 1e-9);
    }

    #[test]
    fn zoom_narrows_span_around_mid() {
        let d = depth(&[(99.9, 1.0)], &[(100.1, 1.0)]);
        let wide = geometry(Some(&d), 1.0);
        let tight = geometry(Some(&d), 4.0);
        assert!((tight.half_span * 4.0 - wide.half_span).abs() < 1e-9);
        assert_eq!(wide.range_pct / tight.range_pct, 4.0);
        // 中间价始终居中
        assert!((wide.x_norm(wide.mid) - 0.5).abs() < 1e-6);
        assert!((tight.x_norm(tight.mid) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn zoom_is_clamped() {
        assert_eq!(clamp_zoom(0.1), MIN_ZOOM);
        assert_eq!(clamp_zoom(999.0), MAX_ZOOM);
        assert_eq!(clamp_zoom(f64::NAN), 1.0);
    }

    #[test]
    fn axis_uses_nice_steps() {
        let (max, step) = axis_scale(10.0);
        assert!(step > 0.0);
        assert!(max >= 10.0);
        assert!((max / step).fract() < 1e-9, "轴顶应是步长的整数倍");
    }

    #[test]
    fn empty_depth_has_no_data() {
        let g = geometry(None, 1.0);
        assert!(!g.has_data);
        assert!(g.x_ticks().is_empty());
        assert!(g.y_ticks().is_empty());
    }
}
