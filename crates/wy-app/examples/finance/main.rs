//! 币安实时行情终端：MVE 信号驱动的组件化看板 + 累计深度图。
//!
//! 对照 ooc 版的 `pages/finance-dashboard.ooc`：同一套数据语义、同一套分区，
//! 同一个「信号驱动、树只建一次」的模型。
//!
//! ```text
//! 后台线程（binance.rs） ──Msg──▶ mpsc ──▶ on_wake（UI 线程）
//!                                          ├─ batch(|| 写信号)
//!                                          └─ 信号变 → RedrawTracker 重录 Scene
//! ```
//!
//! UI 侧全部是组件（`ui/`）：面板、标签、自选行、盘口档位、成交条目各自成节点，
//! 位置尺寸由 flex 排布决定，事件由引擎沿节点树路由。
//!
//! 数据源：默认连本机伪行情服务 `http://localhost:8787/api/v3`
//! （见 `C:\github\mock-api`，路径与字段与币安一致）。要连真实币安：
//!
//! ```text
//! WY_MARKET_API=https://data-api.binance.vision/api/v3 cargo run -p wy-app --example finance
//! ```
//!
//! 运行：`cargo run -p wy-app --example finance`

mod binance;
mod bookview;
mod chartview;
mod depth;
mod market;
mod panels;
mod synth;
mod theme;
mod ui;

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::{Duration, Instant};

use wy_engine::mve_integration::MveApp;
use wy_engine::runner::WyApp;
use wy_mve::{rsx, NodeContext};
use wy_render::Scene;
use wy_signal::{batch, GetValue, SetValue, Signal};

use binance::{Feed, Msg, Session, Source};
use market::{
    merge_trades, replace_klines, upsert_ticker, Candle, Depth, Status, Ticker, Trade,
    WATCH_SYMBOLS,
};
// `dashboard` 是 ctx 组件，走 `rsx!` 标签调用（`dashboard { model: .., ctl: .. }`）
use ui::dashboard;

/// 数据泵节拍：40ms ≈ 25Hz，足以覆盖 1.2s 一次的数据到达。
const WAKE_EVERY: Duration = Duration::from_millis(40);

// ---------------------------------------------------------------------------
// 模型：全部状态都是信号（对应 ooc 页面里的 `bindSignal`）
// ---------------------------------------------------------------------------

pub struct Model {
    pub symbol: Signal<String>,
    pub interval: Signal<String>,
    pub ticker: Signal<Option<Ticker>>,
    pub klines: Signal<Vec<Candle>>,
    /// 20 档盘口。
    pub depth: Signal<Option<Depth>>,
    /// 深盘口（只喂深度图）。
    pub deep: Signal<Option<Depth>>,
    pub trades: Signal<Vec<Trade>>,
    pub watch: Signal<Vec<Ticker>>,
    pub status: Signal<Status>,
    pub error: Signal<String>,
    /// 当前数据源（顶栏胶囊一点即切）。
    pub source: Signal<Source>,
    // ---- 以下是"真跨面板"的部分；视图自己的状态一律留在面板内部 ----
    // K 线视口（visible / offset）是 chart 面板私有的，见 `ui::chart::ChartState`；
    // 深度图是否独占视口由 `ui::mod::dashboard` 持有；
    // 盘口点价只在盘口面板内用，见 `ui::book::BookState::picked`。
}

impl Model {
    fn new() -> Self {
        Self {
            symbol: Signal::new("BTCUSDT".to_string()),
            interval: Signal::new("15m".to_string()),
            ticker: Signal::new(None),
            klines: Signal::new(Vec::new()),
            depth: Signal::new(None),
            deep: Signal::new(None),
            trades: Signal::new(Vec::new()),
            watch: Signal::new(Vec::new()),
            status: Signal::new(Status::Connecting),
            error: Signal::new(String::new()),
            source: Signal::new(Source::initial()),
        }
    }
}

/// 灌一份**合成行情**到模型。
///
/// 出图与测试都用它：内容由代码决定，不依赖网络，两次跑出来的图完全一样。
fn fill_synthetic(model: &Rc<Model>) {
    let now = 1_760_000_000_000i64;
    let symbol = "BTCUSDT";
    let interval = model.interval.get();

    let ticker = synth::ticker(symbol, now);
    model.ticker.set(Some(ticker.clone()));
    model.klines.set(synth::klines(
        symbol,
        synth::interval_ms(&interval),
        now,
        120,
    ));
    model.depth.set(Some(synth::book_depth(symbol, now)));
    model.deep.set(Some(synth::deep_depth(symbol, now)));
    model.trades.set(synth::trades(symbol, now, 40));
    // 自选列：WATCH_SYMBOLS 全列各造一份（过去塞同一个 ticker 两遍，
    // 既重复又只有一行——定高行 + 滚动列表下这个瑕疵会被放大）
    model.watch.set(
        WATCH_SYMBOLS
            .iter()
            .map(|s| synth::ticker(s, now))
            .collect(),
    );
    model.status.set(Status::Live);
}

// ---------------------------------------------------------------------------
// 控制器：开/关行情会话（对应 ooc 的 `openMarket` / `closeMarket` 闭包）
// ---------------------------------------------------------------------------

pub struct Ctl {
    pub feed: Feed,
    pub model: Rc<Model>,
    market: RefCell<Option<Session>>,
    /// 自选会话由 Ctl 持有：切数据源时整体换掉，避免列表混两个接口的报价。
    watch: RefCell<Option<Session>>,
    market_gen: Cell<u64>,
}

impl Ctl {
    fn new(feed: Feed, model: Rc<Model>) -> Self {
        Self {
            feed,
            model,
            market: RefCell::new(None),
            watch: RefCell::new(None),
            market_gen: Cell::new(0),
        }
    }

    /// 开（或重开）自选会话。
    fn start_watch(&self) {
        self.watch.borrow_mut().take();
        let symbols: Vec<String> = WATCH_SYMBOLS.iter().map(|s| s.to_string()).collect();
        *self.watch.borrow_mut() = Some(self.feed.watch(symbols));
    }

    /// 关旧会话、开新会话；清掉上一个标的数据，避免串图。
    fn open_market(&self, symbol: &str, interval: &str) {
        // 先 Drop 旧会话（置停止位并 join，后台线程 ≤1 个轮询间隔即退出）
        self.market.borrow_mut().take();
        self.model.klines.set(Vec::new());
        self.model.depth.set(None);
        self.model.deep.set(None);
        self.model.trades.set(Vec::new());
        self.model.ticker.set(None);
        let session = self.feed.market(symbol, interval);
        self.market_gen.set(session.gen());
        *self.market.borrow_mut() = Some(session);
    }

    fn select_symbol(&self, symbol: &str) {
        if self.model.symbol.get() == symbol {
            return;
        }
        self.model.symbol.set(symbol.to_string());
        self.model.error.set(String::new());
        // 换标的：K 线视口不用在这里重置——它在 chart 面板里带"标的/周期"的 key，
        // key 一变自动回到默认视口（见 `ui::chart::ChartState::view`）。
        self.open_market(symbol, &self.model.interval.get());
    }

    fn select_interval(&self, interval: &str) {
        if self.model.interval.get() == interval {
            return;
        }
        self.model.interval.set(interval.to_string());
        self.model.error.set(String::new());
        self.open_market(&self.model.symbol.get(), interval);
    }

    /// 切到指定数据源（顶栏分段开关的两段各自调它）。
    ///
    /// 只做两件事：改 `Feed` 里的基址、重开会话。旧会话的 `gen` 落到后面，
    /// 迟到的报文会被 `apply` 丢掉，所以切完不会看到两边的数据混在一张图上。
    fn set_source(&self, next: Source) {
        self.feed.set_source(next.clone());
        self.model.source.set(next);
        self.model.error.set(String::new());
        self.model.klines.set(Vec::new());
        self.model.depth.set(None);
        self.model.deep.set(None);
        self.model.trades.set(Vec::new());
        self.model.ticker.set(None);
        self.model.status.set(Status::Connecting);
        self.open_market(&self.model.symbol.get(), &self.model.interval.get());
        // 自选也要换：否则左侧列表还留在旧接口的报价上
        self.start_watch();
    }
}

// ---------------------------------------------------------------------------
// 应用：MveApp + 数据泵
// ---------------------------------------------------------------------------

struct Dashboard {
    inner: MveApp,
    model: Rc<Model>,
    ctl: Rc<Ctl>,
    rx: Receiver<Msg>,
}

impl Dashboard {
    /// 把通道里堆积的消息一次性搬进信号（**一个 batch**）。
    ///
    /// 一次唤醒可能同时带来 ticker + klines + depth + trades 四条消息，
    /// 用 `batch` 包住，只 flush 一次 —— 这是本框架相对 ooc 版最显式的差别：
    /// ooc 每个 handler 各自写信号、各自触发；这里由调用方显式圈定批次。
    fn pump(&mut self) {
        let mut msgs = Vec::new();
        // 单帧上限 512 条：后台刷得太快时先让出一帧，避免一次 batch 占满 UI 线程
        while msgs.len() < 512 {
            match self.rx.try_recv() {
                Ok(msg) => msgs.push(msg),
                Err(TryRecvError::Empty) | Err(TryRecvError::Disconnected) => break,
            }
        }
        if msgs.is_empty() {
            return;
        }
        let gen = self.ctl.market_gen.get();
        let model = Rc::clone(&self.model);
        batch(|| {
            for msg in msgs {
                apply(&model, gen, msg);
            }
        });
    }
}

/// 单条消息 → 信号写入（对应 ooc 里那些 `handler` 回调体）。
fn apply(model: &Model, gen: u64, msg: Msg) {
    match msg {
        Msg::Snapshot {
            gen: g,
            ticker,
            klines,
            depth,
            trades,
        } => {
            if g != gen {
                return;
            }
            model.ticker.set(Some(ticker));
            let mut next = model.klines.get();
            replace_klines(&mut next, klines);
            model.klines.set(next);
            model.depth.set(Some(depth));
            model.trades.set(trades);
        }
        Msg::Ticker { gen: g, ticker } => {
            if g != gen {
                return;
            }
            // 主标的最新价 → 顺带刷新自选里的同一行
            let mut watch = model.watch.get();
            upsert_ticker(&mut watch, ticker.clone());
            model.watch.set(watch);
            model.ticker.set(Some(ticker));
        }
        Msg::Klines { gen: g, klines } => {
            if g != gen {
                return;
            }
            let mut next = model.klines.get();
            replace_klines(&mut next, klines);
            model.klines.set(next);
        }
        Msg::Depth { gen: g, depth } => {
            if g != gen {
                return;
            }
            model.depth.set(Some(depth));
        }
        Msg::DeepDepth { gen: g, depth } => {
            if g != gen {
                return;
            }
            model.deep.set(Some(depth));
        }
        Msg::Trades { gen: g, trades } => {
            if g != gen {
                return;
            }
            let mut next = model.trades.get();
            merge_trades(&mut next, trades);
            model.trades.set(next);
        }
        Msg::Watch(list) => {
            let mut ordered = model.watch.get();
            for ticker in list {
                upsert_ticker(&mut ordered, ticker);
            }
            model.watch.set(ordered);
        }
        Msg::Status(status) => model.status.set(status),
        Msg::Error(message) => {
            if model.error.get() != message {
                model.error.set(message);
            }
        }
    }
}

impl WyApp for Dashboard {
    fn draw(&self, scene: &mut Scene, width: f32, height: f32) {
        self.inner.draw(scene, width, height);
    }

    fn widget_tree(&mut self) -> Option<&mut wy_render::widget_tree::WidgetTree> {
        None
    }

    fn handle_event(&mut self, event: &winit::event::WindowEvent) -> bool {
        self.inner.handle_event(event)
    }

    fn handle_key_event(&mut self, event: &wy_engine::KeyEvent) {
        self.inner.handle_key_event(event);
    }

    fn handle_ime_event(&mut self, event: &wy_mve::ImeEvent) {
        self.inner.handle_ime_event(event);
    }

    fn ime_allowed(&self) -> Option<bool> {
        self.inner.ime_allowed()
    }

    fn ime_cursor_area(&self) -> Option<(f32, f32, f32, f32)> {
        self.inner.ime_cursor_area()
    }

    fn window_size(&self) -> (f32, f32) {
        (1440.0, 900.0)
    }

    fn window_title(&self) -> &str {
        "WY Markets · 行情终端"
    }

    fn next_wake(&self) -> Option<Instant> {
        Some(Instant::now() + WAKE_EVERY)
    }

    fn on_wake(&mut self) {
        self.pump();
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();

    // `--snapshot <路径>`：离屏出图后退出（README 配图用，不开窗口）
    if let Some((path, w, h)) = wy_app::snapshot::parse_snapshot_args() {
        let model = Rc::new(Model::new());
        let (feed, _rx) = Feed::new();
        let ctl = Rc::new(Ctl::new(feed, model.clone()));
        let ui_model = model.clone();
        let ui_ctl = ctl.clone();
        let app = MveApp::new(move |cx: &mut NodeContext| {
            let (m, c) = (Rc::clone(&ui_model), Rc::clone(&ui_ctl));
            cx.child(rsx! { dashboard { model: m, ctl: c } });
        });
        // 灌一份合成行情，保证出图内容稳定可复现
        fill_synthetic(&model);
        wy_app::snapshot::save_snapshot(&app, &path, w, h).map_err(std::io::Error::other)?;
        return Ok(());
    }

    let model = Rc::new(Model::new());
    let (feed, rx) = Feed::new();
    let ctl = Rc::new(Ctl::new(feed, model.clone()));

    // 自选会话常驻；主行情先在默认标的上开一条
    ctl.start_watch();
    ctl.open_market("BTCUSDT", "15m");

    let ui_model = model.clone();
    let ui_ctl = ctl.clone();
    let app = Dashboard {
        inner: MveApp::new(move |cx: &mut NodeContext| {
            let (m, c) = (Rc::clone(&ui_model), Rc::clone(&ui_ctl));
            // `dashboard` 是 ctx 组件：给它 model / ctl，它自己 provide 给整棵子树
            cx.child(rsx! { dashboard { model: m, ctl: c } });
        }),
        model,
        ctl,
        rx,
    };

    let result = wy_engine::runner::run(app);
    // 窗口关闭：Ctl 随 app 一起 drop，两个会话的线程自动停下
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use market::Level;

    /// 造一份合成行情，跑完整绘制路径（布局 + 组件 + 深度几何 + 文本测量）。
    fn synth_model() -> Rc<Model> {
        let model = Rc::new(Model::new());
        model.ticker.set(Some(Ticker {
            symbol: "BTCUSDT".into(),
            last: 67_000.0,
            change: 120.0,
            change_percent: 0.18,
            high: 67_500.0,
            low: 66_100.0,
            open: 66_880.0,
            volume: 12_345.0,
            quote_volume: 820_000_000.0,
            bid: 66_999.0,
            ask: 67_001.0,
        }));
        let klines: Vec<Candle> = (0..120)
            .map(|i| {
                let base = 66_000.0 + (i as f64 * 8.0).sin() * 400.0;
                Candle {
                    time: 1_700_000_000_000 + i as i64 * 900_000,
                    open: base,
                    high: base + 60.0,
                    low: base - 60.0,
                    close: base + 20.0,
                    volume: 10.0 + (i as f64).sin().abs() * 40.0,
                }
            })
            .collect();
        model.klines.set(klines);
        model.depth.set(Some(Depth {
            bids: (0..20)
                .map(|i| Level {
                    price: 66_999.0 - i as f64,
                    qty: 0.5 + i as f64 * 0.1,
                })
                .collect(),
            asks: (0..20)
                .map(|i| Level {
                    price: 67_001.0 + i as f64,
                    qty: 0.4 + i as f64 * 0.1,
                })
                .collect(),
            time: 0,
        }));
        model.deep.set(Some(Depth {
            bids: (0..1000)
                .map(|i| Level {
                    price: 66_999.0 - i as f64 * 0.3,
                    qty: 0.2 + (i as f64 * 0.01).sin().abs(),
                })
                .collect(),
            asks: (0..1000)
                .map(|i| Level {
                    price: 67_001.0 + i as f64 * 0.3,
                    qty: 0.2 + (i as f64 * 0.02).cos().abs(),
                })
                .collect(),
            time: 0,
        }));
        model.trades.set(
            (0..20)
                .map(|i| Trade {
                    id: 100 + i,
                    price: 67_000.0 + i as f64 * 0.5,
                    qty: 0.01 * i as f64,
                    time: 1_700_000_000_000,
                    buyer_maker: i % 3 == 0,
                })
                .collect(),
        );
        // 自选列：WATCH_SYMBOLS 全列各造一份（同一个 ticker 塞两遍既重复
        // 又只有一行，定高行 + 滚动列表下会被放大成"列表只剩一行"）
        model.watch.set(
            WATCH_SYMBOLS
                .iter()
                .map(|s| Ticker {
                    symbol: (*s).to_string(),
                    last: 67_000.0,
                    change: 120.0,
                    change_percent: 0.18,
                    high: 67_500.0,
                    low: 66_100.0,
                    open: 66_880.0,
                    volume: 12_345.0,
                    quote_volume: 820_000_000.0,
                    bid: 66_999.0,
                    ask: 67_001.0,
                })
                .collect(),
        );
        model.status.set(Status::Live);
        model
    }

    fn build_app(model: Rc<Model>) -> MveApp {
        let (feed, _rx) = Feed::new();
        let ctl = Rc::new(Ctl::new(feed, model.clone()));
        MveApp::new(move |cx: &mut NodeContext| {
            let (m, c) = (model.clone(), ctl.clone());
            cx.child(rsx! { dashboard { model: m, ctl: c } });
        })
    }

    #[test]
    fn draws_full_dashboard_without_panicking() {
        let model = synth_model();
        let app = build_app(model.clone());
        let mut scene = Scene::new();
        app.draw(&mut scene, 1440.0, 900.0);
        assert!(scene.len() > 300, "场景图元太少：{}", scene.len());
    }

    /// **离线可跑**保证：默认数据源是内置合成源，不依赖任何网络/外部服务。
    ///
    /// 这条测试守的是「clone 下来就能跑」这条底线——之前默认源是
    /// `localhost:8787` 的本机伪接口，别人 clone 下来打开是一片空白。
    #[test]
    fn default_source_is_offline_synthetic() {
        assert_eq!(Source::initial(), Source::Sim, "默认必须是离线合成源");
        assert!(Source::Sim.is_sim());
        assert!(!Source::Binance.is_sim());
    }

    /// 合成源能产出**完整的一帧看板**（ticker / K 线 / 盘口 / 深盘口 / 成交齐全）。
    ///
    /// 这条链路等价于「打开 demo 的第一秒看到什么」：
    /// 数据齐 → 写进信号 → 布局 → 绘制 → 图元数够多。
    #[test]
    fn synthetic_source_produces_a_complete_frame() {
        let now = 1_760_000_000_000i64;

        // 数据侧：合成源造出四类数据
        let ticker = synth::ticker("BTCUSDT", now);
        assert!(ticker.last > 0.0, "合成 ticker 应有价格");
        let klines = synth::klines("BTCUSDT", synth::interval_ms("15m"), now, 120);
        assert_eq!(klines.len(), 120);
        let depth = synth::book_depth("BTCUSDT", now);
        assert_eq!(depth.bids.len(), 20);
        let deep = synth::deep_depth("BTCUSDT", now);
        assert_eq!(deep.bids.len(), 1000);
        let trades = synth::trades("BTCUSDT", now, 30);
        assert_eq!(trades.len(), 30);

        // UI 侧：把它们写进模型，跑完整绘制路径
        let model = Rc::new(Model::new());
        model.ticker.set(Some(ticker));
        model.klines.set(klines);
        model.depth.set(Some(depth));
        model.deep.set(Some(deep));
        model.trades.set(trades);
        model.watch.set(vec![model.ticker.get().unwrap()]);
        model.status.set(Status::Live);

        let app = build_app(model);
        let mut scene = Scene::new();
        app.draw(&mut scene, 1440.0, 900.0);
        assert!(
            scene.len() > 300,
            "合成数据下的场景图元太少：{}",
            scene.len()
        );
    }

    /// 深度图在合成数据下有可画的累计曲线（不是空白图）。
    #[test]
    fn synthetic_depth_has_drawable_curve() {
        let now = 1_760_000_000_000i64;
        let deep = synth::deep_depth("BTCUSDT", now);
        let g = depth::geometry(Some(&deep), 1.0);
        assert!(g.has_data, "合成深盘口应产生累计曲线");
        assert!(g.bid.max > 0.0, "买盘侧应有累计量");
        assert!(g.ask.max > 0.0, "卖盘侧应有累计量");
        // 曲线必须单调不减（否则画出来的面积会自相矛盾）
        for pair in g.bid.totals.windows(2) {
            assert!(pair[1] >= pair[0]);
        }
    }

    /// 沿 `body → 中列 → 深度面板 → 标题栏` 取到动作区的一排按钮。
    ///
    /// 标题栏 = [标题, 徽标, −, 1x, +, FULL]，所以 `kids[3]` 是 `+`、
    /// `kids[5]` 是 `FULL`。返回的同时带上中列的盒，派发点击要用绝对坐标。
    fn depth_header(app: &MveApp) -> (Vec<wy_mve::Node>, wy_mve::LayoutBox) {
        let (_, rows, avail) = body_row(app, 1440.0, 900.0);
        let boxes = wy_mve::layout_boxes(&rows[0].clone(), avail, &rows);
        // 中列 = [K 线面板, 横条, 深度面板]
        let center = &rows[2];
        let center_box = boxes[2];
        let center_kids = wy_mve::children_nodes(center);
        let depth = &center_kids[2];
        // 深度面板 = [标题栏, 内容区]
        let panel_kids = wy_mve::children_nodes(depth);
        (wy_mve::children_nodes(&panel_kids[0]), center_box)
    }

    /// 点「FULL」→ 整页切到"深度图独占"子树。
    ///
    /// `full` 是**装配层**持有的信号（`ui::mod` 用 `when` 切两棵子树），
    /// 深度面板只读写它。这条测试盯的是那条链路：切完之后页面根下只剩
    /// 深度面板，顶栏与三列都不在树里了。
    ///
    /// 顺带也是一个回归：FULL 子树是 ctx 组件的惰性 slot，一旦有人图省事
    /// 写成 `.into_node()`，它会在**空白上下文**里执行、`consume` 取不到
    /// model——这里会以 panic 的形式暴露出来。
    #[test]
    fn full_toggle_swaps_the_whole_page() {
        let model = synth_model();
        let app = build_app(model.clone());
        let mut scene = Scene::new();
        app.draw(&mut scene, 1440.0, 900.0);

        // 切换前：页面根下是「顶栏 + 三列」
        let page = app.root().nodes()[0].clone();
        let normal = wy_mve::children_nodes(&page);
        assert_eq!(normal.len(), 1, "when 是透明节点：页面根下只有当前子树");
        let normal_kids = wy_mve::children_nodes(&normal[0]);
        assert_eq!(normal_kids.len(), 2, "正常看板 = [顶栏, body]");
        assert_eq!(
            wy_mve::children_nodes(&normal_kids[1]).len(),
            5,
            "body 是三列 + 两根分割条"
        );

        let (header_kids, center_box) = depth_header(&app);
        let full_btn = &header_kids[5];
        assert!(full_btn.on_click_fn.is_some(), "动作区第 6 个应是 FULL");
        full_btn.run_on_click(&mut wy_mve::PointerEvent::new(
            center_box.x + 4.0,
            center_box.y + 4.0,
        ));

        // 切换后：页面根下只有深度面板（[标题栏, 内容区]），内容区是叶子
        let after = wy_mve::children_nodes(&page);
        assert_eq!(after.len(), 1, "切到 FULL 后仍只有一棵子树");
        let kids = wy_mve::children_nodes(&after[0]);
        assert_eq!(kids.len(), 2, "深度面板 = [标题栏, 内容区]");
        assert_eq!(
            wy_mve::children_nodes(&kids[1]).len(),
            1,
            "内容区只剩绘图叶子，不再是三列"
        );
    }

    /// 缩放只改深度图几何，不动树结构。
    ///
    /// 驱动方式是**真点按钮**：沿 `body → 中列 → 深度面板 → 标题栏` 找到动作区
    /// 第三个按钮（`+`）并派发点击。缩放倍率现在是深度面板的私有信号
    /// （见 `ui::depth_view::DepthState`），不再有全局入口——这正是
    /// 「非必要不提升」之后测试该有的样子：通过 UI 驱动，而不是去改一个
    /// 集中的模型字段。
    #[test]
    fn zoom_changes_only_geometry() {
        let model = synth_model();
        let app = build_app(model.clone());
        let mut scene = Scene::new();
        app.draw(&mut scene, 1440.0, 900.0);
        let before = scene.len();

        let (header_kids, center_box) = depth_header(&app);
        // 标题栏 = [标题, 徽标, −, 1x, +, FULL]
        let plus = &header_kids[4];
        assert!(plus.on_click_fn.is_some(), "动作区第 3 个应是缩放按钮");
        plus.run_on_click(&mut wy_mve::PointerEvent::new(
            center_box.x + 4.0,
            center_box.y + 4.0,
        ));

        let mut scene2 = Scene::new();
        app.draw(&mut scene2, 1440.0, 900.0);
        // 树结构不变，只有深度图的桶宽/刻度位置变化，图元总数应基本一致
        let delta = (before as i32 - scene2.len() as i32).abs();
        assert!(delta <= 8, "缩放只应改变深度图形状，图元数差 {delta}");
    }

    /// 三列布局随窗口宽度重排：换尺寸仍应产出完整图元。
    #[test]
    fn columns_follow_window_width() {
        let model = synth_model();
        let app = build_app(model.clone());
        let mut a = Scene::new();
        app.draw(&mut a, 1200.0, 800.0);
        let mut b = Scene::new();
        app.draw(&mut b, 1800.0, 900.0);
        assert!(!a.is_empty() && !b.is_empty(), "两种尺寸都应产出图元");
    }

    /// 一个可见组件在屏幕上的绝对盒（`getBoundingClientRect` 的等价物）。
    #[derive(Clone, Copy, Debug)]
    struct AbsBox {
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        /// 有子节点的容器（面板 / 列 / 行），这类盒子绝不能塌。
        container: bool,
        /// 装配层打的语义锚点（`col-*` 等），测试按它认对象。
        tag: Option<&'static str>,
    }

    /// 走一遍引擎的布局路径，把每个可见节点的绝对盒摊平。
    ///
    /// 与 `draw_node` 保持一致：容器子节点用 `layout_boxes` 定位，无布局的
    /// group 子节点用自然尺寸；隐藏节点整枝跳过。
    ///
    /// **裁剪感知**：`clip` 容器（滚动容器视口）外的子树本来就被 Scene 的
    /// 裁剪栈挡着，这里同样把盒与裁剪区求交——完全在外的丢弃、骑在边上
    /// 的裁齐。否则"滚动内容比视口高"这种设计意图会被误报成溢出
    /// （引擎的命中测试也走同一语义，见 `hit_test_node` 的 clip 拦截）。
    fn collect_boxes(
        node: &wy_mve::Node,
        parent: (f32, f32),
        avail: (f32, f32),
        clip: Option<(f32, f32, f32, f32)>,
        out: &mut Vec<AbsBox>,
    ) {
        if node.is_hidden() {
            return;
        }
        let abs = (parent.0 + node.pos_x(), parent.1 + node.pos_y());
        let children = wy_mve::children_nodes(node);
        let mut b = AbsBox {
            x: abs.0,
            y: abs.1,
            width: avail.0,
            height: avail.1,
            container: !children.is_empty(),
            tag: node.tag,
        };
        if let Some((cx, cy, cw, ch)) = clip {
            // 与裁剪区求交：完全在外 → 这一枝不可见，整枝跳过
            let x0 = b.x.max(cx);
            let y0 = b.y.max(cy);
            let x1 = (b.x + b.width).min(cx + cw);
            let y1 = (b.y + b.height).min(cy + ch);
            if x1 - x0 <= 0.0 || y1 - y0 <= 0.0 {
                return;
            }
            b.x = x0;
            b.y = y0;
            b.width = x1 - x0;
            b.height = y1 - y0;
        }
        out.push(b);
        // 本节点是裁剪容器：子树限制在自身盒内
        let clip = if node.clip {
            Some((b.x, b.y, b.width, b.height))
        } else {
            clip
        };
        if node.layout.is_some() {
            let boxes = wy_mve::layout_boxes(node, avail, &children);
            for (child, b) in children.iter().zip(boxes.iter()) {
                collect_boxes(
                    child,
                    (abs.0 + b.x, abs.1 + b.y),
                    (b.width, b.height),
                    clip,
                    out,
                );
            }
        } else {
            for child in children.iter() {
                let own = wy_mve::node_size(child);
                collect_boxes(child, abs, own, clip, out);
            }
        }
    }

    fn boxes_at(app: &MveApp, w: f32, h: f32) -> Vec<AbsBox> {
        let mut scene = Scene::new();
        app.draw(&mut scene, w, h);
        let mut out = Vec::new();
        for node in app.root().nodes() {
            collect_boxes(&node, (0.0, 0.0), (w, h), None, &mut out);
        }
        out
    }

    /// 布局自检：所有容器都有正尺寸、都在视口内、兄弟盒不重叠。
    ///
    /// 这条测试代替"肉眼看截图"——组件化之后位置全由 flex 决定，
    /// 一旦某个面板忘了 `grow`/`Stretch`，它会以 0 宽（画不出也点不中）
    /// 或者溢出的形式出现，这里直接兜住。
    #[test]
    fn layout_audit_keeps_panels_on_screen() {
        for (w, h) in [(1440.0, 900.0), (1100.0, 720.0), (1920.0, 1080.0)] {
            let model = synth_model();
            let app = build_app(model.clone());
            let boxes = boxes_at(&app, w, h);
            assert!(
                boxes.len() > 30,
                "{w}x{h} 只找到 {} 个可见节点",
                boxes.len()
            );

            for b in &boxes {
                assert!(
                    b.width.is_finite()
                        && b.height.is_finite()
                        && b.x.is_finite()
                        && b.y.is_finite(),
                    "{w}x{h} 出现非有限盒：{b:?}"
                );
                assert!(b.x >= -0.5 && b.y >= -0.5, "{w}x{h} 盒越出左上边界：{b:?}");
                assert!(
                    b.x + b.width <= w + 0.5 && b.y + b.height <= h + 0.5,
                    "{w}x{h} 盒越出右下边界：{b:?}"
                );
                if b.container {
                    assert!(
                        b.width > 0.5 && b.height > 0.5,
                        "{w}x{h} 容器塌了（忘了 grow / Stretch？）：{b:?}"
                    );
                }
            }
        }
    }

    /// 三列顺序不乱：左列完全在中间列左边，中间列完全在右列左边。
    ///
    /// 按 `tag` 认列（`ui::mod::stacked` 给三列打了 `col-*`）：几何特征
    /// （"又高又不满屏"）在列表进了滚动容器之后不再唯一——裁剪后的滚动
    /// 内容盒也符合这一特征，按特征挑会把面板里的内容盒误当成"列"。
    /// tag 是装配层的语义锚点，列表内部怎么改都不影响这条断言。
    #[test]
    fn columns_do_not_overlap() {
        let model = synth_model();
        let app = build_app(model.clone());
        let (w, h) = (1440.0, 900.0);
        let boxes = boxes_at(&app, w, h);

        let mut columns: Vec<AbsBox> = boxes
            .iter()
            .filter(|b| matches!(b.tag, Some("col-left" | "col-center" | "col-right")))
            .copied()
            .collect();
        assert_eq!(columns.len(), 3, "应能按 tag 认出三列：{columns:?}");
        columns.sort_by(|a, b| a.x.partial_cmp(&b.x).unwrap());
        for pair in columns.windows(2) {
            let (left, right) = (pair[0], pair[1]);
            assert!(
                left.x + left.width <= right.x + 0.5,
                "列重叠：{:?} 与 {:?}",
                left,
                right
            );
        }
    }

    /// 取页面根下的 body 行（顶栏下面那一行）及其布局结果。
    ///
    /// 树是 `app 根(1 个孩子) → dashboard → [顶栏, body] → [左列, 竖条, 中列, 竖条, 右列]`。
    /// 注意 `when` 是**透明节点**：它挂上去的孩子会直接出现在父容器的子节点里，
    /// 所以 dashboard 的孩子看起来就是 `[顶栏, body]`，中间没有额外一层。
    /// 测试要直接拿到 `body` 的子节点，才能对分割条派发拖拽。
    fn body_row(app: &MveApp, w: f32, h: f32) -> (wy_mve::Node, Vec<wy_mve::Node>, (f32, f32)) {
        let nodes = app.root().nodes();
        let page = wy_mve::children_nodes(&nodes[0]);
        let page = page[0].clone();
        let page_children = wy_mve::children_nodes(&page);
        let page_boxes = wy_mve::layout_boxes(&page, (w, h), &page_children);
        let body = page_children.last().unwrap().clone();
        let b = *page_boxes.last().unwrap();
        let rows = wy_mve::children_nodes(&body);
        (body, rows, (b.width, b.height))
    }

    /// 拖左边的竖向分割条 → 左列真的变宽（信号 → `grow_fn` → 重排 全链路）。
    ///
    /// 这条测试盯的是"拖动看起来没反应"：分割条只有 6px，如果它的盒是 0、
    /// 或者 `SizeProbe` 没被绘制期写入，拖动就会被夹到最小值而看不出变化。
    #[test]
    fn dragging_column_gutter_resizes_columns() {
        const W: f32 = 1440.0;
        const H: f32 = 900.0;
        let model = synth_model();
        let app = build_app(model.clone());
        // 先绘制一遍：容器探针（SizeProbe）是在 draw 期写入的
        let _ = boxes_at(&app, W, H);

        let (body, kids, avail) = body_row(&app, W, H);
        let boxes = wy_mve::layout_boxes(&body, avail, &kids);

        // 左列右边那根 6px 的分割条
        let (gi, gbox) = kids
            .iter()
            .zip(boxes.iter())
            .enumerate()
            .find(|(_, (n, b))| b.width <= 8.0 && b.height > 200.0 && n.on_down_fn.is_some())
            .map(|(i, (_, b))| (i, *b))
            .expect("应能找到竖向分割条");
        let left_w = boxes[gi - 1].width;
        assert!(left_w > 100.0, "左列初始宽度异常：{left_w}");

        let gutter = &kids[gi];
        let x = gbox.x + gbox.width / 2.0;
        let y = gbox.y + gbox.height / 2.0;
        gutter.run_on_down(&mut wy_mve::PointerEvent::new(x, y));
        gutter.run_on_move(&mut wy_mve::PointerEvent::new(x + 60.0, y));
        gutter.run_on_up(&mut wy_mve::PointerEvent::new(x + 60.0, y));

        let after = wy_mve::layout_boxes(&body, avail, &kids);
        let new_left = after[gi - 1].width;
        assert!(
            (new_left - (left_w + 60.0)).abs() < 1.0,
            "拖 60px 后左列应为 {}，实际 {new_left}",
            left_w + 60.0
        );
        assert!(
            after[gi + 1].width < boxes[gi + 1].width - 59.0,
            "中间列应让出 60px"
        );
    }

    /// 列表行高固定、且整体在滚动容器里（对齐参考实现；行高不再随
    /// 面板 `grow` 均分——均分会把矮面板的行压扁、高面板的行拉伸出
    /// 夸张行距）。
    ///
    /// 行按 `tag` 认（`list_row` 的语义标签），不按几何特征或索引——
    /// ctx 组件挂的孩子数不定（book 挂 2 个：面板 + 设置菜单，索引法会错位）。
    /// 行高直接求 `height_fn`（`rsx!` 的 `height:` 生成的就是它）。
    /// 顺带钉死：自选列出 `WATCH_SYMBOLS` 全部标的（合成数据曾把同一个
    /// ticker 塞两遍，定高行下这个瑕疵会被放大成"列表只剩一行"）。
    #[test]
    fn list_rows_are_fixed_height_inside_scroll() {
        let model = synth_model();
        let app = build_app(model.clone());
        let _ = boxes_at(&app, 1440.0, 900.0);

        let page = app.root().nodes()[0].clone();

        // 三个列表都在滚动容器里：watchlist / stats 各 1 个，book 两侧 2 个，
        // trades 1 个——clip 视口共 5 个
        let clips = count_clip(&page);
        assert_eq!(clips, 5, "应有 5 个滚动视口（watch/stats/book×2/trades）");

        let watch_rows = find_all_by_tag(&page, "watch-row");
        assert_eq!(
            watch_rows.len(),
            WATCH_SYMBOLS.len(),
            "自选应列出全部标的（合成数据按 WATCH_SYMBOLS 生成）"
        );
        for r in &watch_rows {
            assert_row_height(r, ui::watchlist::ROW_H, "自选");
        }

        let book_rows = find_all_by_tag(&page, "book-row");
        assert_eq!(book_rows.len(), ui::book::ROWS * 2, "盘口两侧各 ROWS 档");
        for r in &book_rows {
            assert_row_height(r, ui::book::ROW_H, "盘口");
        }

        let trade_rows = find_all_by_tag(&page, "trade-row");
        assert!(!trade_rows.is_empty(), "成交应有行");
        for r in &trade_rows {
            assert_row_height(r, ui::trades::ROW_H, "成交");
        }
    }

    /// 行高直接求 `height_fn`（`rsx!` 的 `height:` 生成的是布局期闭包，
    /// 字段 `node.height` 仍是 0，读它什么都断言不到）。
    fn assert_row_height(row: &wy_mve::Node, want: f32, what: &str) {
        let h = row
            .height_fn
            .as_ref()
            .unwrap_or_else(|| panic!("{what}行应有 height_fn（定高行）"))();
        assert_eq!(h, want, "{what}行应定高 {want}");
    }

    /// 数子树里的 clip 视口数（滚动容器）。
    fn count_clip(node: &wy_mve::Node) -> usize {
        let kids = wy_mve::children_nodes(node);
        node.clip as usize + kids.iter().map(count_clip).sum::<usize>()
    }

    /// 收集子树里所有带某 tag 的节点。
    fn find_all_by_tag(node: &wy_mve::Node, tag: &'static str) -> Vec<wy_mve::Node> {
        let mut out = Vec::new();
        if node.tag == Some(tag) {
            out.push(node.clone());
        }
        for c in wy_mve::children_nodes(node) {
            out.extend(find_all_by_tag(&c, tag));
        }
        out
    }

    /// 按 tag 在树里找节点（测试的语义锚点，见 `ui::mod::stacked`）。
    fn find_by_tag(node: &wy_mve::Node, tag: &'static str) -> Option<wy_mve::Node> {
        if node.tag == Some(tag) {
            return Some(node.clone());
        }
        wy_mve::children_nodes(node)
            .iter()
            .find_map(|c| find_by_tag(c, tag))
    }

    /// 拖行分割条 → 上下比例真的变（比例模式走 `probe` 的高度）。
    #[test]
    fn dragging_row_gutter_changes_split() {
        const W: f32 = 1440.0;
        const H: f32 = 900.0;
        let model = synth_model();
        let app = build_app(model.clone());
        let _ = boxes_at(&app, W, H);

        let (body, rows, avail) = body_row(&app, W, H);
        let boxes = wy_mve::layout_boxes(&body, avail, &rows);
        let col = &rows[0];
        let col_box = boxes[0];
        let col_kids = wy_mve::children_nodes(col);
        let col_boxes = wy_mve::layout_boxes(col, (col_box.width, col_box.height), &col_kids);

        let (gi, gbox) = col_kids
            .iter()
            .zip(col_boxes.iter())
            .enumerate()
            .find(|(_, (n, b))| b.height <= 8.0 && b.width > 100.0 && n.on_down_fn.is_some())
            .map(|(i, (_, b))| (i, *b))
            .expect("应能找到横向分割条");
        let top_h = col_boxes[gi - 1].height;

        let gutter = &col_kids[gi];
        let x = gbox.x + gbox.width / 2.0;
        let y = gbox.y + gbox.height / 2.0;
        eprintln!("DBG col_box={:?} gbox={:?} top_h={}", col_box, gbox, top_h);
        gutter.run_on_down(&mut wy_mve::PointerEvent::new(x, y));
        gutter.run_on_move(&mut wy_mve::PointerEvent::new(x, y - 40.0));
        eprintln!("DBG after move, kids={}", col_kids.len());
        gutter.run_on_up(&mut wy_mve::PointerEvent::new(x, y - 40.0));

        let after = wy_mve::layout_boxes(col, (col_box.width, col_box.height), &col_kids);
        let new_top = after[gi - 1].height;
        assert!(
            (new_top - (top_h - 40.0)).abs() < 2.0,
            "上移 40px 后上方面板应为 {}，实际 {new_top}",
            top_h - 40.0
        );
    }

    /// 临时：把可见节点的盒按树打出来
    /// （`cargo test --example finance -- --ignored dump_boxes --nocapture`）。
    #[test]
    #[ignore]
    fn dump_boxes() {
        fn go(n: &wy_mve::Node, parent: (f32, f32), avail: (f32, f32), depth: usize) {
            if n.is_hidden() {
                return;
            }
            let abs = (parent.0 + n.pos_x(), parent.1 + n.pos_y());
            println!(
                "{:indent$}x={:7.1} y={:7.1} w={:7.1} h={:6.1} grow={:4.1} layout={}",
                "",
                abs.0,
                abs.1,
                avail.0,
                avail.1,
                n.effective_grow(),
                if n.layout.is_some() { "Y" } else { "-" },
                indent = depth * 2
            );
            let children = wy_mve::children_nodes(n);
            if n.layout.is_some() {
                let boxes = wy_mve::layout_boxes(n, avail, &children);
                for (c, b) in children.iter().zip(boxes.iter()) {
                    go(
                        c,
                        (abs.0 + b.x, abs.1 + b.y),
                        (b.width, b.height),
                        depth + 1,
                    );
                }
            } else {
                for c in children.iter() {
                    let own = wy_mve::node_size(c);
                    go(c, abs, own, depth + 1);
                }
            }
        }
        let model = synth_model();
        let app = build_app(model.clone());
        let mut scene = Scene::new();
        app.draw(&mut scene, 1440.0, 900.0);
        for node in app.root().nodes() {
            go(&node, (0.0, 0.0), (1440.0, 900.0), 0);
        }
    }
}
