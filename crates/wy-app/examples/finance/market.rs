//! 行情数据模型与纯函数：对应 ooc 版 `dashboard/market.ts` 的类型 + 格式化部分。
//!
//! 这一层刻意不碰信号、不碰 UI：与 ooc 保持同样的分工——数据/几何是纯函数，
//! 组件只负责把纯函数结果画出来。信号写入全部集中在 `main.rs` 的数据泵里。

/// 24h 行情快照（对应 ooc `Ticker`）。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Ticker {
    pub symbol: String,
    /// 最新成交价。
    pub last: f64,
    /// 24h 涨跌额。
    pub change: f64,
    /// 24h 涨跌幅（百分比数值，非小数）。
    pub change_percent: f64,
    pub high: f64,
    pub low: f64,
    pub open: f64,
    pub volume: f64,
    pub quote_volume: f64,
    pub bid: f64,
    pub ask: f64,
}

impl Ticker {
    pub fn up(&self) -> bool {
        self.change_percent >= 0.0
    }
}

/// 一根 K 线（对应 ooc `Candle`）。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Candle {
    /// 开盘时间（毫秒，唯一键）。
    pub time: i64,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub volume: f64,
}

/// 盘口一档。
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Level {
    pub price: f64,
    pub qty: f64,
}

/// 盘口快照（对应 ooc `Depth`）。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Depth {
    /// 买盘：价格从高到低（最优买价在前）。
    pub bids: Vec<Level>,
    /// 卖盘：价格从低到高（最优卖价在前）。
    pub asks: Vec<Level>,
    /// 收到快照的时间（毫秒）。
    pub time: i64,
}

impl Depth {
    pub fn best_bid(&self) -> Option<f64> {
        self.bids.first().map(|l| l.price)
    }

    pub fn best_ask(&self) -> Option<f64> {
        self.asks.first().map(|l| l.price)
    }

    /// 中间价（对应 ooc `depthSeries` 里的 mid 计算）。
    pub fn mid(&self) -> Option<f64> {
        match (self.best_bid(), self.best_ask()) {
            (Some(b), Some(a)) => Some((a + b) / 2.0),
            (None, Some(a)) => Some(a),
            (Some(b), None) => Some(b),
            (None, None) => None,
        }
    }
}

/// 一笔成交（对应 ooc `Trade`）。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Trade {
    pub id: u64,
    pub price: f64,
    pub qty: f64,
    pub time: i64,
    /// true = 买方是 maker（即卖方主动成交 → 显示为下跌色）。
    pub buyer_maker: bool,
}

/// 连接状态（对应 ooc `MarketStatus`）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Connecting,
    Live,
    Reconnecting,
    Offline,
}

impl Status {
    pub fn label(self) -> &'static str {
        match self {
            Status::Connecting => "CONNECTING",
            Status::Live => "LIVE",
            Status::Reconnecting => "RETRYING",
            Status::Offline => "OFFLINE",
        }
    }

    pub fn is_live(self) -> bool {
        self == Status::Live
    }
}

/// 自选列表的默认标的（与 ooc 一致）。
pub const WATCH_SYMBOLS: [&str; 10] = [
    "BTCUSDT", "ETHUSDT", "SOLUSDT", "BNBUSDT", "XRPUSDT", "DOGEUSDT", "ADAUSDT", "AVAXUSDT",
    "LINKUSDT", "TONUSDT",
];

/// 顶栏可点的标的分组（ooc 也只放 4 个 tab）。
pub const TAB_SYMBOLS: [&str; 4] = ["BTCUSDT", "ETHUSDT", "SOLUSDT", "BNBUSDT"];
/// 顶栏可点的周期分组。
pub const TAB_INTERVALS: [&str; 4] = ["1m", "5m", "15m", "1h"];

/// K 线最多保留根数（与 ooc 的 `upsertKline(max = 120)` 一致）。
pub const MAX_CANDLES: usize = 120;
/// 成交列表最多保留笔数。
pub const MAX_TRADES: usize = 26;

/// 同 key 覆盖式 upsert（对应 ooc `upsertKline`）。
///
/// 当前示例走整批替换（[`replace_klines`]），这个函数留给将来接 WebSocket
/// 逐根推送时用——保留它就是为了对齐 ooc 的两条更新路径。
#[allow(dead_code)]
///
/// 同一根未收盘的 K 线会被流式更新，这里按 `time` 原地替换，否则追加并截断。
pub fn upsert_kline(list: &mut Vec<Candle>, candle: Candle) {
    if let Some(last) = list.last_mut() {
        if last.time == candle.time {
            *last = candle;
            return;
        }
    }
    list.push(candle);
    if list.len() > MAX_CANDLES {
        let drop = list.len() - MAX_CANDLES;
        list.drain(0..drop);
    }
}

/// 整批替换 K 线（轮询版用；仍保持 `MAX_CANDLES` 上限）。
pub fn replace_klines(list: &mut Vec<Candle>, mut next: Vec<Candle>) {
    if next.len() > MAX_CANDLES {
        let drop = next.len() - MAX_CANDLES;
        next.drain(0..drop);
    }
    *list = next;
}

/// 成交并入列表（对应 ooc `pushTrade`：新成交在头部，截断到 `MAX_TRADES`）。
pub fn merge_trades(list: &mut Vec<Trade>, mut incoming: Vec<Trade>) {
    if incoming.is_empty() {
        return;
    }
    incoming.sort_by_key(|t| std::cmp::Reverse(t.id));
    let newest = incoming.first().map(|t| t.id).unwrap_or(0);
    let oldest_incoming = incoming.last().map(|t| t.id).unwrap_or(0);
    // 只并入比现有更新的（轮询会重复返回同一批）
    list.retain(|t| t.id < oldest_incoming || t.id > newest);
    let mut merged = incoming;
    merged.append(list);
    merged.sort_by_key(|t| std::cmp::Reverse(t.id));
    merged.truncate(MAX_TRADES);
    *list = merged;
}

/// 自选列表 upsert（对应 ooc `upsertTicker`）。
pub fn upsert_ticker(list: &mut Vec<Ticker>, ticker: Ticker) {
    if let Some(slot) = list.iter_mut().find(|t| t.symbol == ticker.symbol) {
        *slot = ticker;
        return;
    }
    list.push(ticker);
    // 保持 WATCH_SYMBOLS 的固定顺序，避免列表跳动
    list.sort_by_key(|t| {
        WATCH_SYMBOLS
            .iter()
            .position(|s| *s == t.symbol)
            .unwrap_or(usize::MAX)
    });
}

/// 价格格式化（对应 ooc `formatPrice`：按量级选小数位 + 千分位）。
pub fn format_price(value: f64) -> String {
    if !value.is_finite() || value == 0.0 {
        return "--".into();
    }
    let abs = value.abs();
    let digits = if abs >= 1000.0 || abs >= 1.0 {
        2
    } else if abs >= 0.01 {
        4
    } else {
        6
    };
    thousands(value, digits)
}

/// 数量格式化（对应 ooc `formatQty`）。
pub fn format_qty(value: f64) -> String {
    if !value.is_finite() {
        return "--".into();
    }
    let abs = value.abs();
    let digits = if abs >= 1000.0 {
        0
    } else if abs >= 1.0 {
        3
    } else {
        5
    };
    thousands(value, digits)
}

/// 紧凑数量（对应 ooc `formatCompact`：1.2K / 3.4M / 5.6B）。
pub fn format_compact(value: f64) -> String {
    if !value.is_finite() {
        return "--".into();
    }
    let abs = value.abs();
    let (scaled, suffix) = if abs >= 1e9 {
        (value / 1e9, "B")
    } else if abs >= 1e6 {
        (value / 1e6, "M")
    } else if abs >= 1e3 {
        (value / 1e3, "K")
    } else {
        (value, "")
    };
    format!(
        "{}{}",
        thousands(scaled, if abs >= 1e3 { 2 } else { 0 }),
        suffix
    )
}

/// 百分比（对应 ooc `formatPercent`，带符号）。
pub fn format_percent(value: f64) -> String {
    if !value.is_finite() {
        return "--".into();
    }
    format!("{}{:.2}%", if value >= 0.0 { "+" } else { "" }, value)
}

/// 毫秒时间戳 → `HH:MM:SS`。
///
/// 无 chrono 依赖：按固定时区偏移换算（默认 +8，中国标准时间）。
pub fn format_time(ms: i64) -> String {
    if ms <= 0 {
        return "--".into();
    }
    const OFFSET_SECONDS: i64 = 8 * 3600;
    let total = ms / 1000 + OFFSET_SECONDS;
    let secs_of_day = total.rem_euclid(86_400);
    format!(
        "{:02}:{:02}:{:02}",
        secs_of_day / 3600,
        (secs_of_day % 3600) / 60,
        secs_of_day % 60
    )
}

/// 千分位 + 固定小数位（减少对 locale 的依赖）。
fn thousands(value: f64, digits: usize) -> String {
    let negative = value < 0.0;
    let text = format!("{:.*}", digits, value.abs());
    let (int_part, frac_part) = match text.split_once('.') {
        Some((i, f)) => (i.to_string(), f.to_string()),
        None => (text.clone(), String::new()),
    };
    let mut grouped = String::with_capacity(int_part.len() + int_part.len() / 3);
    for (i, ch) in int_part.chars().enumerate() {
        if i > 0 && (int_part.len() - i) % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(ch);
    }
    let body = if frac_part.is_empty() {
        grouped
    } else {
        format!("{grouped}.{frac_part}")
    };
    if negative {
        format!("-{body}")
    } else {
        body
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn price_picks_digits_by_magnitude() {
        assert_eq!(format_price(67_432.1), "67,432.10");
        assert_eq!(format_price(3.14159), "3.14");
        assert_eq!(format_price(0.052341), "0.0523");
        assert_eq!(format_price(0.0001234), "0.000123");
        assert_eq!(format_price(0.0), "--");
    }

    #[test]
    fn qty_and_compact() {
        assert_eq!(format_qty(12.3456), "12.346");
        assert_eq!(format_qty(1234.6), "1,235");
        assert_eq!(format_compact(1_234_567.0), "1.23M");
    }

    #[test]
    fn kline_upsert_replaces_same_time() {
        let mut list = vec![Candle {
            time: 1,
            close: 10.0,
            ..Candle::default()
        }];
        upsert_kline(
            &mut list,
            Candle {
                time: 1,
                close: 11.0,
                ..Candle::default()
            },
        );
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].close, 11.0);
        upsert_kline(
            &mut list,
            Candle {
                time: 2,
                ..Candle::default()
            },
        );
        assert_eq!(list.len(), 2);
    }

    #[test]
    fn kline_upsert_truncates() {
        let mut list: Vec<Candle> = (0..MAX_CANDLES as i64)
            .map(|i| Candle {
                time: i,
                ..Candle::default()
            })
            .collect();
        upsert_kline(
            &mut list,
            Candle {
                time: MAX_CANDLES as i64,
                ..Candle::default()
            },
        );
        assert_eq!(list.len(), MAX_CANDLES);
        assert_eq!(list.last().unwrap().time, MAX_CANDLES as i64);
    }

    #[test]
    fn trades_merge_dedups_by_id() {
        let mut list = vec![
            Trade {
                id: 5,
                ..Trade::default()
            },
            Trade {
                id: 4,
                ..Trade::default()
            },
        ];
        merge_trades(
            &mut list,
            vec![
                Trade {
                    id: 6,
                    ..Trade::default()
                },
                Trade {
                    id: 5,
                    ..Trade::default()
                },
            ],
        );
        let ids: Vec<u64> = list.iter().map(|t| t.id).collect();
        assert_eq!(ids, vec![6, 5, 4], "应按 id 倒序且不重复");
    }

    #[test]
    fn watchlist_keeps_stable_order() {
        let mut list = Vec::new();
        upsert_ticker(
            &mut list,
            Ticker {
                symbol: "ETHUSDT".into(),
                ..Ticker::default()
            },
        );
        upsert_ticker(
            &mut list,
            Ticker {
                symbol: "BTCUSDT".into(),
                ..Ticker::default()
            },
        );
        assert_eq!(list[0].symbol, "BTCUSDT", "顺序跟随 WATCH_SYMBOLS");
    }
}
