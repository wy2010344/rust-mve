//! 币安公开行情数据源：对应 ooc 版 `dashboard/market.ts` 的会话层。
//!
//! # 为什么是「后台线程 + 通道 + 定时泵」
//!
//! ooc 跑在 JS 世界里：`fetch` / `WebSocket` 的回调天然回到同一个事件循环，
//! 拿到数据直接 `signal $ value` 就行。
//!
//! Rust 原生的 MVE 不行——信号状态全在 `thread_local`（`Rc<RefCell<_>>`，
//! 非 `Send`/`Sync`），**只有 UI 线程能写**。所以这里的分工是：
//!
//! ```text
//! 后台线程（本模块）              UI 线程（main.rs 的 on_wake）
//!   ureq 轮询 ──Msg──▶ mpsc ──▶ 排空通道 ──▶ batch(|| 写信号)
//! ```
//!
//! 与 ooc 的 `createMarketSession` / `createWatchlistSession` 一一对应：
//! 同样的 handler 语义（snapshot / ticker / kline / depth / deepDepth / trades /
//! status / error），同样的「失败降级 + 退避」，只是回调搬到了另一条线程。
//!
//! 只用公开市场数据（`data-api.binance.vision`，无 API Key、不碰账户接口）。

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::market::{Candle, Depth, Level, Status, Ticker, Trade};

/// 本机伪行情服务（`C://github//mock-api`，端口 8787）。
///
/// 路径、字段、错误码与币安**完全一致**，所以切数据源只换基址，解析代码一行都不用动。
pub const MOCK_REST_BASE: &str = "http://localhost:8787/api/v3";
/// 币安公开行情（无 API Key）。
pub const LIVE_REST_BASE: &str = "https://data-api.binance.vision/api/v3";

/// 数据源。运行期可切（顶栏那个 SIM / BINANCE / MOCK 胶囊）。
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub enum Source {
    /// **内置合成行情**：纯本地计算，零网络零依赖，clone 下来就能跑。
    #[default]
    Sim,
    /// 本机伪接口：随便刷，不怕限流（需另起 `mock-api` 服务）。
    Mock,
    /// 真实币安：同一 IP 限流敏感（418/429），由 `Rhythm` 做全局退避。
    Binance,
    /// `WY_MARKET_API` 指定的自建接口（路径字段仍要与币安一致）。
    Custom(String),
}

impl Source {
    /// 是否走内置合成行情（不发起任何网络请求）。
    pub fn is_sim(&self) -> bool {
        matches!(self, Source::Sim)
    }

    /// HTTP 基址（合成源不联网，此处返回值仅用于错误提示兜底）。
    pub fn base_url(&self) -> &str {
        match self {
            Source::Sim => "",
            Source::Mock => MOCK_REST_BASE,
            Source::Binance => LIVE_REST_BASE,
            Source::Custom(url) => url,
        }
    }

    /// 顶栏徽标文案。
    pub fn label(&self) -> &'static str {
        match self {
            Source::Sim => "SIM",
            Source::Mock => "MOCK",
            Source::Binance => "BINANCE",
            Source::Custom(_) => "CUSTOM",
        }
    }

    /// 一键切换的对方（自定义源先回到合成源）。
    pub fn toggled(&self) -> Self {
        match self {
            Source::Binance => Source::Mock,
            _ => Source::Binance,
        }
    }

    /// 启动时的默认值：**离线合成**（保证 clone 下来立刻有画面）；
    /// `WY_MARKET_API` 指到哪就从哪开场。
    pub fn initial() -> Self {
        let Ok(url) = std::env::var("WY_MARKET_API") else {
            return Source::Sim;
        };
        let url = url.trim().trim_end_matches('/');
        if url.is_empty() {
            Source::Sim
        } else if url == MOCK_REST_BASE {
            Source::Mock
        } else if url == LIVE_REST_BASE {
            Source::Binance
        } else {
            Source::Custom(url.to_string())
        }
    }
}

/// 各端点的轮询节奏（毫秒）。合起来约 1.7 请求/秒，远低于币安权重上限。
const TICKER_EVERY: Duration = Duration::from_millis(1_500);
const KLINE_EVERY: Duration = Duration::from_millis(4_000);
const DEPTH_EVERY: Duration = Duration::from_millis(1_200);
const TRADE_EVERY: Duration = Duration::from_millis(1_200);
/// 深度图用的深盘口：单独低频拉取，失败退避。
const DEEP_EVERY: Duration = Duration::from_millis(5_000);
/// 深盘口档数。ooc 用 5000（覆盖 ±0.5%），这里 1000 档足够且省带宽。
const DEEP_LIMIT: u32 = 1_000;
/// 自选列表轮询节奏。
const WATCH_EVERY: Duration = Duration::from_millis(3_000);

/// 后台线程送给 UI 线程的消息（== ooc 的 handler 回调）。
#[derive(Clone, Debug)]
pub enum Msg {
    /// 首个全量快照（对应 ooc `onSnapshot`）。
    Snapshot {
        gen: u64,
        ticker: Ticker,
        klines: Vec<Candle>,
        depth: Depth,
        trades: Vec<Trade>,
    },
    Ticker {
        gen: u64,
        ticker: Ticker,
    },
    Klines {
        gen: u64,
        klines: Vec<Candle>,
    },
    /// 20 档盘口（对应 ooc `onDepth`）。
    Depth {
        gen: u64,
        depth: Depth,
    },
    /// 深盘口，只喂深度图（对应 ooc `onDepthDeep`）。
    DeepDepth {
        gen: u64,
        depth: Depth,
    },
    Trades {
        gen: u64,
        trades: Vec<Trade>,
    },
    /// 自选列表整批更新（对应 ooc `onTickers`）。
    Watch(Vec<Ticker>),
    Status(Status),
    Error(String),
}

/// 数据源：持有发送端、代号分配器与**当前基址**，可克隆给多个会话。
///
/// 基址放在 `Arc<Mutex<Source>>` 里：后台线程每个请求前读一次，
/// 所以顶栏一切换，下一轮轮询就打到新接口上（不需要重建线程）。
#[derive(Clone)]
pub struct Feed {
    tx: mpsc::Sender<Msg>,
    gen: Arc<AtomicU64>,
    source: Arc<Mutex<Source>>,
}

impl Feed {
    /// 创建数据源，返回可被 UI 线程排空的接收端。
    pub fn new() -> (Self, mpsc::Receiver<Msg>) {
        Self::with_source(Source::initial())
    }

    /// 指定初始数据源（测试与 `WY_MARKET_API` 用）。
    pub fn with_source(source: Source) -> (Self, mpsc::Receiver<Msg>) {
        let (tx, rx) = mpsc::channel();
        (
            Self {
                tx,
                gen: Arc::new(AtomicU64::new(1)),
                source: Arc::new(Mutex::new(source)),
            },
            rx,
        )
    }

    /// 当前数据源。
    pub fn source(&self) -> Source {
        self.source.lock().map(|s| s.clone()).unwrap_or_default()
    }

    /// 切数据源。已在跑的会话会在下一轮请求自动跟上。
    pub fn set_source(&self, source: Source) {
        *self.source.lock().unwrap_or_else(|e| e.into_inner()) = source;
    }

    /// 开一个主行情会话（对应 ooc `createMarketSession`）。
    ///
    /// 换标的 / 换数据源都是「关旧开新」：旧会话的 `gen` 落后，UI 线程据此丢弃
    /// 迟到的数据，避免上一只票（或上一个接口）的盘口串进来。
    pub fn market(&self, symbol: &str, interval: &str) -> Session {
        let gen = self.gen.fetch_add(1, Ordering::SeqCst);
        let symbol = symbol.to_string();
        let interval = interval.to_string();
        let tx = self.tx.clone();
        let source = Arc::clone(&self.source);
        let mut session = self.spawn("binance-market", move |stop| {
            run_market(&tx, stop, gen, &symbol, &interval, &source)
        });
        session.gen = gen;
        session
    }

    /// 开一个自选列表会话（对应 ooc `createWatchlistSession`）。
    pub fn watch(&self, symbols: Vec<String>) -> Session {
        let tx = self.tx.clone();
        let source = Arc::clone(&self.source);
        let mut session = self.spawn("binance-watch", move |stop| {
            run_watch(&tx, stop, &symbols, &source)
        });
        session.gen = 0;
        session
    }

    fn spawn(&self, name: &str, body: impl FnOnce(Arc<AtomicBool>) + Send + 'static) -> Session {
        let stop = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&stop);
        let handle = thread::Builder::new()
            .name(name.into())
            .spawn(move || body(flag))
            .ok();
        Session {
            stop,
            handle,
            gen: 0,
        }
    }
}

/// 会话句柄：drop 即让后台线程退出（对应 ooc `close()`）。
pub struct Session {
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
    /// 本次会话的代号：UI 侧据此丢弃上一个标的迟到的消息。
    gen: u64,
}

impl Session {
    /// 会话代号（自选会话恒为 0）。
    pub fn gen(&self) -> u64 {
        self.gen
    }

    fn shutdown(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.shutdown();
    }
}

// ---------------------------------------------------------------------------
// 主行情：轮询循环
// ---------------------------------------------------------------------------

fn run_market(
    tx: &mpsc::Sender<Msg>,
    stop: Arc<AtomicBool>,
    gen: u64,
    symbol: &str,
    interval: &str,
    source: &Mutex<Source>,
) {
    // 合成源：不发起任何网络请求，纯本地算。
    // 与 HTTP 源走同一套 `Msg` 协议，所以 UI 侧完全不知道差别。
    if source.lock().map(|s| s.is_sim()).unwrap_or(false) {
        run_market_sim(tx, stop, gen, symbol, interval, source);
        return;
    }

    let agent = agent();
    let _ = tx.send(Msg::Status(Status::Connecting));

    let mut rhythm = Rhythm::new();
    // 首屏快照：四类数据到齐后合并发一次，**不额外发请求**。
    // （早先版本在首个快照失败时每 60ms 重试一轮，直接把 IP 打进 418 封禁）
    let mut boot = Boot::default();

    while !stop.load(Ordering::SeqCst) {
        // 运行期切到合成源：立刻改走本地生成（顶栏那个 SIM 段）。
        if source.lock().map(|s| s.is_sim()).unwrap_or(false) {
            run_market_sim(tx, stop, gen, symbol, interval, source);
            return;
        }

        // 被限流：整轮暂停，不碰任何端点
        if rhythm.cooling() {
            thread::sleep(Duration::from_millis(250));
            continue;
        }

        let now = Instant::now();

        if now >= rhythm.next_ticker {
            rhythm.next_ticker = now + TICKER_EVERY;
            match get(
                &agent,
                &base_of(source),
                &format!("/ticker/24hr?symbol={symbol}"),
            ) {
                Ok(v) => {
                    rhythm.ok(tx);
                    let ticker = parse_ticker(&v);
                    boot.ticker = Some(ticker.clone());
                    let _ = tx.send(Msg::Ticker { gen, ticker });
                }
                Err(e) => rhythm.err(tx, e),
            }
        }

        if now >= rhythm.next_kline {
            rhythm.next_kline = now + KLINE_EVERY;
            let path = format!("/klines?symbol={symbol}&interval={interval}&limit=120");
            match get(&agent, &base_of(source), &path) {
                Ok(v) => {
                    rhythm.ok(tx);
                    let klines = parse_klines(&v);
                    boot.klines = Some(klines.clone());
                    let _ = tx.send(Msg::Klines { gen, klines });
                }
                Err(e) => rhythm.err(tx, e),
            }
        }

        if now >= rhythm.next_depth {
            rhythm.next_depth = now + DEPTH_EVERY;
            match get(
                &agent,
                &base_of(source),
                &format!("/depth?symbol={symbol}&limit=20"),
            ) {
                Ok(v) => {
                    rhythm.ok(tx);
                    let depth = parse_depth(&v);
                    boot.depth = Some(depth.clone());
                    let _ = tx.send(Msg::Depth { gen, depth });
                }
                Err(e) => rhythm.err(tx, e),
            }
        }

        if now >= rhythm.next_trade {
            rhythm.next_trade = now + TRADE_EVERY;
            match get(
                &agent,
                &base_of(source),
                &format!("/trades?symbol={symbol}&limit=30"),
            ) {
                Ok(v) => {
                    rhythm.ok(tx);
                    let trades = parse_trades(&v);
                    boot.trades = Some(trades.clone());
                    let _ = tx.send(Msg::Trades { gen, trades });
                }
                Err(e) => rhythm.err(tx, e),
            }
        }

        if now >= rhythm.next_deep {
            match get(
                &agent,
                &base_of(source),
                &format!("/depth?symbol={symbol}&limit={DEEP_LIMIT}"),
            ) {
                Ok(v) => {
                    rhythm.deep_delay = DEEP_EVERY;
                    let _ = tx.send(Msg::DeepDepth {
                        gen,
                        depth: parse_depth(&v),
                    });
                }
                // 深盘口单独退避：被限频时逐步退到 60s，保留当前图
                Err(e) => {
                    if e.rate_limited() {
                        rhythm.err(tx, e);
                    } else {
                        rhythm.deep_delay = (rhythm.deep_delay * 2).min(Duration::from_secs(60));
                    }
                }
            }
            rhythm.next_deep = Instant::now() + rhythm.deep_delay;
        }

        if let Some(snapshot) = boot.take_snapshot(gen) {
            let _ = tx.send(snapshot);
        }

        thread::sleep(Duration::from_millis(60));
    }
}

/// 合成行情循环：本地生成，不发任何网络请求。
///
/// 与 HTTP 版**发完全相同的 `Msg`**——UI 侧、信号侧完全不知道差别，
/// 这正是数据源可运行时切换的前提。
///
/// 节奏刻意比真实行情**快**（价格每 200ms 走一格）：demo 要让人
/// 一眼看出「这是活的」，而不是等真实接口的 1.2s 轮询。
fn run_market_sim(
    tx: &mpsc::Sender<Msg>,
    stop: Arc<AtomicBool>,
    gen: u64,
    symbol: &str,
    interval: &str,
    source: &Mutex<Source>,
) {
    let _ = tx.send(Msg::Status(Status::Connecting));
    let mut boot = Boot::default();
    let interval_ms = crate::synth::interval_ms(interval);
    // 首帧：四类数据立刻齐活，先出一帧完整画面
    let mut first = true;
    // 已经推过的 K 线桶（K 线只在跨桶时推，省得每 200ms 重推 120 根）
    let mut sent_bucket: Option<i64> = None;
    // 已经推过的深盘口时间戳（深盘口低频刷新）
    let mut sent_deep_at: Option<i64> = None;

    while !stop.load(Ordering::SeqCst) {
        // 运行期切回 HTTP 源：交还控制权给调用方重新开一条会话
        if !source.lock().map(|s| s.is_sim()).unwrap_or(false) {
            return;
        }

        let now_ms = now_millis();
        let ticker = crate::synth::ticker(symbol, now_ms);
        boot.ticker = Some(ticker.clone());
        let _ = tx.send(Msg::Ticker { gen, ticker });

        let depth = crate::synth::book_depth(symbol, now_ms);
        boot.depth = Some(depth.clone());
        let _ = tx.send(Msg::Depth { gen, depth });

        let trades = crate::synth::trades(symbol, now_ms, 30);
        boot.trades = Some(trades.clone());
        let _ = tx.send(Msg::Trades { gen, trades });

        // K 线按周期分桶，只在**跨到新的桶**时推一帧
        let bucket = (now_ms / interval_ms) * interval_ms;
        if sent_bucket != Some(bucket) {
            let klines = crate::synth::klines(symbol, interval_ms, now_ms, 120);
            boot.klines = Some(klines.clone());
            let _ = tx.send(Msg::Klines { gen, klines });
            sent_bucket = Some(bucket);
        }

        // 深盘口（喂深度图）：低频一点，1000 档算起来不便宜
        if sent_deep_at.map(|t| now_ms - t > 2_000).unwrap_or(true) {
            let deep = crate::synth::deep_depth(symbol, now_ms);
            let _ = tx.send(Msg::DeepDepth { gen, depth: deep });
            sent_deep_at = Some(now_ms);
        }

        if first {
            if let Some(snapshot) = boot.take_snapshot(gen) {
                let _ = tx.send(snapshot);
            }
            let _ = tx.send(Msg::Status(Status::Live));
            first = false;
        }

        // 分片睡眠：关窗时能及时退出
        let deadline = Instant::now() + Duration::from_millis(200);
        while Instant::now() < deadline && !stop.load(Ordering::SeqCst) {
            thread::sleep(Duration::from_millis(40));
        }
    }
}

/// 当前 Unix 毫秒时间戳。
fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// 轮询节律 + 故障/限流状态机。
///
/// 对应 ooc 版 `createMarketSession` 里的 `failures` / `polling` / `deepDelay` 那套状态，
/// 另外多一层**全局退避**：币安返回 418/429 时暂停整轮轮询，
/// 否则每个端点各撞一次只会把封禁时间越拖越长。
struct Rhythm {
    next_ticker: Instant,
    next_kline: Instant,
    next_depth: Instant,
    next_trade: Instant,
    next_deep: Instant,
    deep_delay: Duration,
    failing: bool,
    /// 当前退避时长（指数增长，上限 5 分钟）。
    cooldown: Duration,
    /// 退避结束时刻。
    cooling_until: Instant,
}

impl Rhythm {
    fn new() -> Self {
        let now = Instant::now();
        Self {
            next_ticker: now,
            next_kline: now,
            next_depth: now,
            next_trade: now,
            next_deep: now + Duration::from_millis(600),
            deep_delay: DEEP_EVERY,
            failing: false,
            cooldown: Duration::ZERO,
            cooling_until: now,
        }
    }

    fn cooling(&self) -> bool {
        Instant::now() < self.cooling_until
    }

    /// 一次成功：恢复状态，仅在从故障恢复时清空错误文案。
    fn ok(&mut self, tx: &mpsc::Sender<Msg>) {
        self.cooldown = Duration::ZERO;
        if self.failing {
            self.failing = false;
            let _ = tx.send(Msg::Error(String::new()));
        }
        let _ = tx.send(Msg::Status(Status::Live));
    }

    /// 一次失败：限流走全局退避，其它错误只提示一次。
    fn err(&mut self, tx: &mpsc::Sender<Msg>, error: FetchError) {
        let _ = tx.send(Msg::Status(Status::Reconnecting));
        if error.rate_limited() {
            self.cooldown = if self.cooldown.is_zero() {
                Duration::from_secs(30)
            } else {
                (self.cooldown * 2).min(Duration::from_secs(300))
            };
            self.cooling_until = Instant::now() + self.cooldown;
            self.failing = true;
            let _ = tx.send(Msg::Error(format!(
                "币安限流（{}），已暂停轮询 {}s",
                error,
                self.cooldown.as_secs()
            )));
            return;
        }
        if !self.failing {
            self.failing = true;
            let _ = tx.send(Msg::Error(error.to_string()));
        }
    }
}

/// 首屏快照的拼装缓冲：四类数据齐了才发。
#[derive(Default)]
struct Boot {
    ticker: Option<Ticker>,
    klines: Option<Vec<Candle>>,
    depth: Option<Depth>,
    trades: Option<Vec<Trade>>,
}

impl Boot {
    fn take_snapshot(&mut self, gen: u64) -> Option<Msg> {
        let ticker = self.ticker.clone()?;
        let klines = self.klines.clone()?;
        let depth = self.depth.clone()?;
        let trades = self.trades.clone()?;
        self.ticker = None;
        self.klines = None;
        self.depth = None;
        self.trades = None;
        Some(Msg::Snapshot {
            gen,
            ticker,
            klines,
            depth,
            trades,
        })
    }
}

// ---------------------------------------------------------------------------
// 自选列表：轮询循环
// ---------------------------------------------------------------------------

fn run_watch(
    tx: &mpsc::Sender<Msg>,
    stop: Arc<AtomicBool>,
    symbols: &[String],
    source: &Mutex<Source>,
) {
    // 合成源：本地生成自选行情，不发请求
    if source.lock().map(|s| s.is_sim()).unwrap_or(false) {
        run_watch_sim(tx, stop, symbols, source);
        return;
    }

    let agent = agent();
    let query = serde_json::to_string(symbols).unwrap_or_else(|_| "[]".into());
    let path = format!("/ticker/24hr?symbols={query}");
    let mut failing = false;

    while !stop.load(Ordering::SeqCst) {
        // 运行期切到合成源
        if source.lock().map(|s| s.is_sim()).unwrap_or(false) {
            run_watch_sim(tx, stop, symbols, source);
            return;
        }

        match get(&agent, &base_of(source), &path) {
            Ok(Value::Array(items)) => {
                failing = false;
                let tickers: Vec<Ticker> = items.iter().map(parse_ticker).collect();
                let _ = tx.send(Msg::Watch(tickers));
            }
            Ok(_) => {}
            Err(e) => {
                if !failing {
                    failing = true;
                    let _ = tx.send(Msg::Error(format!("自选行情：{e}")));
                }
            }
        }
        // 分片睡眠，关闭时能及时退出
        let deadline = Instant::now() + WATCH_EVERY;
        while Instant::now() < deadline && !stop.load(Ordering::SeqCst) {
            thread::sleep(Duration::from_millis(100));
        }
    }
}

/// 合成自选行情循环（不联网）。
fn run_watch_sim(
    tx: &mpsc::Sender<Msg>,
    stop: Arc<AtomicBool>,
    symbols: &[String],
    source: &Mutex<Source>,
) {
    while !stop.load(Ordering::SeqCst) {
        if !source.lock().map(|s| s.is_sim()).unwrap_or(false) {
            return;
        }
        let now_ms = now_millis();
        let tickers: Vec<Ticker> = symbols
            .iter()
            .map(|s| crate::synth::ticker(s, now_ms))
            .collect();
        let _ = tx.send(Msg::Watch(tickers));

        let deadline = Instant::now() + Duration::from_millis(900);
        while Instant::now() < deadline && !stop.load(Ordering::SeqCst) {
            thread::sleep(Duration::from_millis(50));
        }
    }
}

// ---------------------------------------------------------------------------
// HTTP + 解析
// ---------------------------------------------------------------------------

/// 读一次当前基址。
///
/// 每个请求前调用，所以顶栏点一下切换，下一轮轮询就换接口了；
/// `Mutex` 只在这几行里短暂持有，不会拖慢轮询。
fn base_of(source: &Mutex<Source>) -> String {
    source
        .lock()
        .map(|s| s.base_url().to_string())
        .unwrap_or_else(|_| MOCK_REST_BASE.to_string())
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(5))
        .timeout_read(Duration::from_secs(10))
        .build()
}

/// 请求失败的分类：限流要全局退避，其它错误只需提示。
#[derive(Debug)]
enum FetchError {
    /// HTTP 状态码（418/429 = 限流/封禁）。
    Status(u16),
    /// 传输/解析错误。
    Other(String),
}

impl FetchError {
    fn rate_limited(&self) -> bool {
        matches!(self, FetchError::Status(418) | FetchError::Status(429))
    }
}

impl std::fmt::Display for FetchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FetchError::Status(code) => write!(f, "HTTP {code}"),
            FetchError::Other(msg) => f.write_str(msg),
        }
    }
}

fn get(agent: &ureq::Agent, base: &str, path: &str) -> Result<Value, FetchError> {
    let url = format!("{base}{path}");
    let body = agent
        .get(&url)
        .call()
        .map_err(|e| match e {
            ureq::Error::Status(code, _) => FetchError::Status(code),
            other => FetchError::Other(other.to_string()),
        })?
        .into_string()
        .map_err(|e| FetchError::Other(e.to_string()))?;
    serde_json::from_str(&body).map_err(|e| FetchError::Other(format!("JSON 解析失败：{e}")))
}

// ---------------------------------------------------------------------------
// 报文解析（字段与币安一致，数值都是字符串）
// ---------------------------------------------------------------------------

fn num(v: &Value, key: &str) -> f64 {
    v.get(key)
        .and_then(|x| x.as_str())
        .and_then(|s| s.parse().ok())
        .unwrap_or(0.0)
}

fn parse_ticker(v: &Value) -> Ticker {
    Ticker {
        symbol: v
            .get("symbol")
            .and_then(|x| x.as_str())
            .unwrap_or_default()
            .to_string(),
        last: num(v, "lastPrice"),
        change: num(v, "priceChange"),
        change_percent: num(v, "priceChangePercent"),
        high: num(v, "highPrice"),
        low: num(v, "lowPrice"),
        open: num(v, "openPrice"),
        volume: num(v, "volume"),
        quote_volume: num(v, "quoteVolume"),
        bid: num(v, "bidPrice"),
        ask: num(v, "askPrice"),
    }
}

fn parse_klines(v: &Value) -> Vec<Candle> {
    let Some(rows) = v.as_array() else {
        return Vec::new();
    };
    rows.iter()
        .filter_map(|row| {
            let slot = row.as_array()?;
            let f = |i: usize| {
                slot.get(i)
                    .and_then(|x| x.as_str())
                    .and_then(|s| s.parse::<f64>().ok())
                    .unwrap_or(0.0)
            };
            let time = slot.first()?.as_i64()?;
            Some(Candle {
                time,
                open: f(1),
                high: f(2),
                low: f(3),
                close: f(4),
                volume: f(5),
            })
        })
        .collect()
}

fn parse_depth(v: &Value) -> Depth {
    let side = |key: &str| -> Vec<Level> {
        v.get(key)
            .and_then(|x| x.as_array())
            .map(|rows| {
                rows.iter()
                    .filter_map(|row| {
                        let s = row.as_array()?;
                        let price = s.first()?.as_str()?.parse().ok()?;
                        let qty = s.get(1)?.as_str()?.parse().ok()?;
                        Some(Level { price, qty })
                    })
                    .collect()
            })
            .unwrap_or_default()
    };
    Depth {
        bids: side("bids"),
        asks: side("asks"),
        time: 0,
    }
}

fn parse_trades(v: &Value) -> Vec<Trade> {
    let Some(rows) = v.as_array() else {
        return Vec::new();
    };
    rows.iter()
        .filter_map(|t| {
            Some(Trade {
                id: t.get("id")?.as_u64()?,
                price: t.get("price")?.as_str()?.parse().ok()?,
                qty: t.get("qty")?.as_str()?.parse().ok()?,
                time: t.get("time")?.as_i64()?,
                buyer_maker: t.get("isBuyerMaker")?.as_bool()?,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_string_numbers() {
        let v: Value = serde_json::from_str(
            r#"{"symbol":"BTCUSDT","lastPrice":"67000.10","priceChangePercent":"-1.25"}"#,
        )
        .unwrap();
        let t = parse_ticker(&v);
        assert_eq!(t.last, 67_000.10);
        assert_eq!(t.change_percent, -1.25);
    }

    #[test]
    fn parses_klines() {
        let v: Value =
            serde_json::from_str(r#"[[1700000000000,"1.0","2.0","0.5","1.5","100.0"]]"#).unwrap();
        let k = parse_klines(&v);
        assert_eq!(k.len(), 1);
        assert_eq!(k[0].high, 2.0);
        assert_eq!(k[0].volume, 100.0);
    }

    #[test]
    fn parses_depth_levels() {
        let v: Value =
            serde_json::from_str(r#"{"bids":[["100.0","2.0"]],"asks":[["101.0","3.0"]]}"#).unwrap();
        let d = parse_depth(&v);
        assert_eq!(d.bids[0].price, 100.0);
        assert_eq!(d.asks[0].qty, 3.0);
        assert_eq!(d.mid(), Some(100.5));
    }

    /// 真实网络联调：抓一次 BTCUSDT 快照 + 深盘口，确认接口与解析都通。
    ///
    /// 默认 ignore（会连外网）：`cargo test -p wy-app --example finance -- --ignored`
    #[test]
    #[ignore = "需要网络：cargo test -p wy-app --example finance -- --ignored"]
    fn live_binance_delivers_snapshot_and_deep_depth() {
        use std::time::Instant;
        let (feed, rx) = Feed::new();
        let session = feed.market("BTCUSDT", "1m");
        let watch = feed.watch(vec!["BTCUSDT".into(), "ETHUSDT".into()]);

        let deadline = Instant::now() + Duration::from_secs(30);
        let (mut snapshot, mut deep, mut watch_ok) = (false, false, false);
        while Instant::now() < deadline && !(snapshot && deep && watch_ok) {
            match rx.recv_timeout(Duration::from_millis(500)) {
                Ok(Msg::Snapshot {
                    ticker,
                    klines,
                    depth,
                    trades,
                    ..
                }) => {
                    assert_eq!(ticker.symbol, "BTCUSDT");
                    assert!(ticker.last > 0.0, "最新价应大于 0");
                    assert!(!klines.is_empty(), "应有 K 线");
                    assert!(!depth.asks.is_empty() && !depth.bids.is_empty());
                    assert!(!trades.is_empty(), "应有成交");
                    snapshot = true;
                }
                Ok(Msg::DeepDepth { depth, .. }) => {
                    assert!(depth.bids.len() > 100, "深盘口应有足够档位");
                    assert!(depth.mid().is_some());
                    deep = true;
                }
                Ok(Msg::Watch(list)) => {
                    assert_eq!(list.len(), 2);
                    watch_ok = true;
                }
                Ok(Msg::Error(e)) if !e.is_empty() => panic!("币安接口报错：{e}"),
                Ok(_) => {}
                Err(_) => {}
            }
        }
        drop(session);
        drop(watch);
        assert!(snapshot, "30 秒内应收到 Snapshot");
        assert!(deep, "30 秒内应收到深盘口");
        assert!(watch_ok, "30 秒内应收到自选列表");
    }
}
