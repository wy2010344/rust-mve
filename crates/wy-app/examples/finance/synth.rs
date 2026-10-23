//! 合成行情源：确定性的伪随机时间函数，离线生成盘口 / K 线 / 成交。
//!
//! # 为什么需要它
//!
//! 一个能被面试官点开的 demo，必须满足「clone 下来就能跑」：
//! 不能依赖外网（面试官的公司网可能直接封了币安），也不能依赖本机
//! 另起的服务（`mock-api` 是另一个仓库，别人没有）。
//!
//! 所以这里的行情**完全由时间函数生成**：
//! `price(symbol, t)` 只由 `(symbol, t)` 决定 →
//! - 任何线程、任何时刻读到的都是同一个值，不需要共享可变状态；
//! - 两个面板（主行情 / 自选列表）不会各走各的、价格对不上；
//! - 完全离线，重启后曲线形态一致（同一 `t` 同一结果）。
//!
//! # 形态
//!
//! - **价格**：基准价 × (1 + 振幅 × fbm(t))。`fbm` 是多倍频值噪声
//!   （value noise 的和），看起来就是一条连续的随机游走；
//! - **K 线**：按周期分桶，桶内取样若干点求 open/high/low/close；
//! - **盘口**：以中间价为中心向两侧铺档，挂单量随距离衰减（近厚远薄）；
//! - **成交**：最近一秒内的若干笔，`id` 随时间单调递增（供 `merge_trades` 去重）。
//!
//! 这一层刻意不碰信号、不碰 UI——与 [`crate::market`] 一样是纯数据，
//! 由 UI 线程的数据泵把结果写进信号。

use crate::market::{Candle, Depth, Level, Ticker, Trade};

/// 默认展示的行情周期（毫秒）。
pub const DEFAULT_INTERVAL_MS: i64 = 15 * 60 * 1000;

/// 周期字符串 → 毫秒（`1m` / `5m` / `15m` / `1h` / `4h` / `1d`）。
///
/// 与币安的周期命名一致；不认识的按 `15m` 兜底（合成源不报错，
/// 顶栏那几个 tab 都是已知的）。
pub fn interval_ms(interval: &str) -> i64 {
    match interval {
        "1m" => 60_000,
        "5m" => 5 * 60_000,
        "15m" => DEFAULT_INTERVAL_MS,
        "30m" => 30 * 60_000,
        "1h" => 60 * 60_000,
        "4h" => 4 * 60 * 60_000,
        "1d" => 24 * 60 * 60_000,
        _ => DEFAULT_INTERVAL_MS,
    }
}

/// 自选列表 / 顶栏可点的标的的基准价。
///
/// 价格量级不同，盘口步长与数量级都跟着变（否则 BTC 的挂单量会
/// 小到看不见，而 DOGE 的会大到溢出）。
pub fn base_price(symbol: &str) -> f64 {
    match symbol {
        "BTCUSDT" => 67_000.0,
        "ETHUSDT" => 3_200.0,
        "SOLUSDT" => 152.0,
        "BNBUSDT" => 585.0,
        "XRPUSDT" => 0.62,
        "DOGEUSDT" => 0.135,
        "ADAUSDT" => 0.45,
        "AVAXUSDT" => 28.5,
        "LINKUSDT" => 14.2,
        "TONUSDT" => 5.4,
        _ => 100.0,
    }
}

/// 标的 → 随机种子（FNV-1a）。
fn symbol_seed(symbol: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in symbol.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h | 1
}

/// 整数 → [0, 1) 的确定性伪随机（splitmix64 的输出混合）。
fn hash01(seed: u64, n: u64) -> f64 {
    let mut x = seed
        .wrapping_add(n.wrapping_mul(0x9E37_79B9_7F4A_7C15))
        .wrapping_add(0x2545_F491_4F6C_DD1D);
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^= x >> 31;
    (x >> 11) as f64 / ((1u64 << 53) as f64)
}

/// 一维值噪声：整数格点上取伪随机，格点之间用 smoothstep 插值。
///
/// 这是「连续的伪随机」——`t` 连续变化时输出连续，
/// 于是价格曲线不会一跳一跳。
fn value_noise(seed: u64, t: f64) -> f64 {
    let i = t.floor();
    let f = t - i;
    let a = hash01(seed, i as i64 as u64);
    let b = hash01(seed, (i as i64 + 1) as u64);
    let u = f * f * (3.0 - 2.0 * f);
    a + (b - a) * u
}

/// 多倍频噪声（fBm）：4 个倍频相加，归一化到 [0, 1]。
fn fbm(seed: u64, t: f64) -> f64 {
    let mut sum = 0.0;
    let mut amp = 1.0;
    let mut freq = 1.0;
    let mut norm = 0.0;
    for i in 0..4u64 {
        sum += amp * value_noise(seed.wrapping_add(i.wrapping_mul(7919)), t * freq);
        norm += amp;
        amp *= 0.5;
        freq *= 2.0;
    }
    sum / norm
}

/// 某一时刻的价格。
///
/// `t` 是**秒**（Unix 时间戳的小数部分）。同一个 `(symbol, t)`
/// 永远得到同一个价——这是整套合成行情的地基。
pub fn price_at(symbol: &str, t: f64) -> f64 {
    let base = base_price(symbol);
    let seed = symbol_seed(symbol);
    // 慢趋势（分钟级）+ 快波动（秒级）：两段叠加更像真实行情
    let slow = fbm(seed, t / 45.0) * 2.0 - 1.0;
    let fast = (fbm(seed ^ 0xA5A5_A5A5, t / 3.0) * 2.0 - 1.0) * 0.35;
    base * (1.0 + 0.012 * slow + 0.004 * fast)
}

/// 当前时刻的价格（毫秒时间戳 → 秒）。
pub fn price_now(symbol: &str, now_ms: i64) -> f64 {
    price_at(symbol, now_ms as f64 / 1000.0)
}

/// 24h 行情快照。
pub fn ticker(symbol: &str, now_ms: i64) -> Ticker {
    let t = now_ms as f64 / 1000.0;
    let last = price_at(symbol, t);
    let open = price_at(symbol, t - 86_400.0);
    // 高低点：24 小时内取 24 个采样点的极值
    let mut high = last;
    let mut low = last;
    for i in 0..24 {
        let p = price_at(symbol, t - 86_400.0 * (i as f64 + 1.0) / 24.0);
        if p > high {
            high = p;
        }
        if p < low {
            low = p;
        }
    }
    let seed = symbol_seed(symbol);
    let vol_noise = 0.6 + hash01(seed ^ 0x1234, (now_ms / 60_000) as u64) * 0.8;
    let volume = 18_000.0 * vol_noise * (base_price(symbol).sqrt() / 10.0).max(0.4);
    let tick = price_tick(symbol);
    Ticker {
        symbol: symbol.to_string(),
        last,
        change: last - open,
        change_percent: if open > 0.0 {
            (last - open) / open * 100.0
        } else {
            0.0
        },
        high,
        low,
        open,
        volume,
        quote_volume: volume * last,
        bid: last - tick / 2.0,
        ask: last + tick / 2.0,
    }
}

/// 盘口步长（价格最小变动单位）。
///
/// 深盘口用**细步长**（`price × 1e-5`）：1000 档正好铺满 ±0.5%，
/// 与深度图的展示跨度对齐；20 档的可见盘口用**粗步长**（`price × 5e-5`），
/// 否则 20 档挤在一根 K 线宽的范围内，价差看不出来。
pub fn price_tick(symbol: &str) -> f64 {
    base_price(symbol) * 1e-5
}

/// 盘口快照。
///
/// `tick_ratio` 是步长相对价格的比（深盘口 1e-5，可见盘口 5e-5）。
pub fn depth(symbol: &str, now_ms: i64, levels: usize, tick_ratio: f64) -> Depth {
    let mid = price_now(symbol, now_ms);
    let step = mid * tick_ratio;
    let seed = symbol_seed(symbol);
    let epoch = (now_ms / 1_000) as u64;
    // 挂单量基准：与价格成反比，保证「价值」量级接近
    let base_qty = 800_000.0 / base_price(symbol).max(0.01);

    let mut bids = Vec::with_capacity(levels);
    let mut asks = Vec::with_capacity(levels);
    for i in 0..levels {
        let noise = hash01(seed ^ 0x77, epoch.wrapping_mul(131).wrapping_add(i as u64));
        // 近厚远薄：距离越远，挂单量越小（叠加噪声避免过于规则）
        let shape = 1.0 / (1.0 + i as f64 * 0.055);
        let qty = base_qty * shape * (0.35 + noise * 1.35);
        // 买盘价格从中间价往下、卖盘往上，都以半步居中 → best_bid/ask 的中点正好是 mid
        bids.push(Level {
            price: mid - (i as f64 + 0.5) * step,
            qty,
        });
        asks.push(Level {
            price: mid + (i as f64 + 0.5) * step,
            qty,
        });
    }
    Depth {
        bids,
        asks,
        time: now_ms,
    }
}

/// 可见盘口（20 档，粗步长）。
pub fn book_depth(symbol: &str, now_ms: i64) -> Depth {
    depth(symbol, now_ms, 20, 5e-5)
}

/// 深盘口（1000 档，细步长，喂深度图）。
///
/// 步长取 `price × 5e-6`：1000 档正好铺满深度图的 ±0.5% 展示跨度
/// （`depth::RANGE_PCT`），**一档不浪费**。步长再粗一点，两端的档位
/// 会被 `group_side` 当成超出跨度丢掉；再细一点，1000 档也铺不满，
/// 曲线两端会缺一截。
pub fn deep_depth(symbol: &str, now_ms: i64) -> Depth {
    depth(symbol, now_ms, 1000, 5e-6)
}

/// K 线序列（按周期分桶，取最近 `count` 根）。
///
/// 每根桶内取 8 个采样点求 high/low——只取两端的话，
/// 随机游走会经常"开=收"，蜡烛没有实体，图表看着像一条线。
pub fn klines(symbol: &str, interval_ms: i64, now_ms: i64, count: usize) -> Vec<Candle> {
    let seed = symbol_seed(symbol);
    let bucket = (now_ms / interval_ms) * interval_ms;
    let mut out = Vec::with_capacity(count);
    for k in 0..count {
        let t0 = bucket - k as i64 * interval_ms;
        let t1 = t0 + interval_ms;
        let open = price_at(symbol, t0 as f64 / 1000.0);
        let close = price_at(symbol, t1 as f64 / 1000.0);
        let mut high = open.max(close);
        let mut low = open.min(close);
        for s in 1..8 {
            let ts = t0 + interval_ms * s / 8;
            let p = price_at(symbol, ts as f64 / 1000.0);
            if p > high {
                high = p;
            }
            if p < low {
                low = p;
            }
        }
        // 成交量：区间内的波动幅度 × 随机因子
        let vol_noise = 0.5 + hash01(seed ^ 0xABCD, (t0 / interval_ms) as u64) * 1.0;
        let volume = (high - low) / base_price(symbol).max(1e-9) * 12_000.0 * vol_noise;
        out.push(Candle {
            time: t0,
            open,
            high,
            low,
            close,
            volume: volume.max(0.0),
        });
    }
    out
}

/// 最近若干笔成交（新的在前）。
///
/// `id` 随时间单调递增：`merge_trades` 按 id 去重与排序，
/// id 乱序会导致列表抖动或丢新成交。
pub fn trades(symbol: &str, now_ms: i64, count: usize) -> Vec<Trade> {
    let seed = symbol_seed(symbol);
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let noise = hash01(seed ^ 0x5EED, (now_ms / 100) as u64);
        // 价格在中间价附近小幅偏移
        let price = price_now(symbol, now_ms) * (1.0 + (noise - 0.5) * 0.0008);
        let qty_noise = hash01(seed ^ 0xBEEF, (now_ms / 100) as u64 ^ i as u64);
        let qty = 0.02 + qty_noise * 0.6;
        // id 越大越新：高位放最新的一笔
        let id = (now_ms as u64) << 12 | (count - i) as u64;
        out.push(Trade {
            id,
            price,
            qty,
            time: now_ms - i as i64 * 80,
            buyer_maker: qty_noise > 0.5,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const T0: i64 = 1_760_000_000_000;

    #[test]
    fn price_is_deterministic_for_same_time() {
        let a = price_at("BTCUSDT", 100.0);
        let b = price_at("BTCUSDT", 100.0);
        assert_eq!(a, b, "同一时刻同一标的价格必须一致");
    }

    #[test]
    fn price_moves_over_time() {
        let a = price_at("BTCUSDT", 100.0);
        let b = price_at("BTCUSDT", 160.0);
        assert_ne!(a, b, "价格应随时间变化");
        // 振幅控制在合理范围（±3%）
        assert!((a / base_price("BTCUSDT") - 1.0).abs() < 0.03);
        assert!((b / base_price("BTCUSDT") - 1.0).abs() < 0.03);
    }

    #[test]
    fn price_is_continuous() {
        // 相邻 1ms 的价格差不应出现跳变（连续噪声的必然结果）
        let a = price_at("ETHUSDT", 1000.0);
        let b = price_at("ETHUSDT", 1000.001);
        assert!((a - b).abs() / a < 0.001, "相邻时刻价格应连续");
    }

    #[test]
    fn book_mid_equals_mid_price() {
        let d = book_depth("BTCUSDT", T0);
        let mid = d.mid().expect("盘口应有中间价");
        let expect = price_now("BTCUSDT", T0);
        assert!((mid - expect).abs() < 1e-6, "中间价应等于合成价格");
    }

    #[test]
    fn book_sides_are_ordered() {
        let d = book_depth("BTCUSDT", T0);
        // 买盘价格从高到低
        for w in d.bids.windows(2) {
            assert!(w[0].price > w[1].price, "买盘应按价格降序");
        }
        // 卖盘价格从低到高
        for w in d.asks.windows(2) {
            assert!(w[0].price < w[1].price, "卖盘应按价格升序");
        }
        assert_eq!(d.bids.len(), 20);
        assert_eq!(d.asks.len(), 20);
    }

    #[test]
    fn deep_book_covers_the_chart_span() {
        // 1000 档 × 5e-6 ≈ ±0.5%，与深度图展示跨度一致（不浪费档位）
        let d = deep_depth("BTCUSDT", T0);
        let mid = d.mid().unwrap();
        let span = (d.asks.last().unwrap().price - mid) / mid;
        assert!((span - 0.005).abs() < 0.0001, "深盘口应覆盖 ±0.5%");
    }

    /// 深盘口的档位**全部落在深度图的展示跨度内**（不会被 `group_side` 丢弃）。
    #[test]
    fn deep_book_levels_all_land_inside_the_chart() {
        let d = deep_depth("BTCUSDT", T0);
        let mid = d.mid().unwrap();
        let half = mid * (crate::depth::RANGE_PCT / 200.0);
        assert!(d.bids.last().unwrap().price > mid - half);
        assert!(d.asks.last().unwrap().price < mid + half);
    }

    #[test]
    fn klines_are_ordered_and_well_formed() {
        let ks = klines("BTCUSDT", DEFAULT_INTERVAL_MS, T0, 30);
        assert_eq!(ks.len(), 30);
        for w in ks.windows(2) {
            assert!(w[0].time > w[1].time, "K 线按时间倒序（新的在前）");
        }
        for k in &ks {
            assert!(k.high >= k.low, "最高价不低于最低价");
            assert!(k.high >= k.open && k.high >= k.close);
            assert!(k.low <= k.open && k.low <= k.close);
        }
    }

    #[test]
    fn trades_ids_increase_and_are_newest_first() {
        let ts = trades("BTCUSDT", T0, 30);
        assert_eq!(ts.len(), 30);
        for w in ts.windows(2) {
            assert!(w[0].id > w[1].id, "成交 id 应递减（新的在前）");
        }
    }

    #[test]
    fn ticker_stats_are_consistent() {
        let t = ticker("BTCUSDT", T0);
        assert_eq!(t.symbol, "BTCUSDT");
        assert!(t.high >= t.low);
        assert!(t.bid < t.ask);
        assert!(t.quote_volume > 0.0);
    }
}
