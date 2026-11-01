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
mod ui2;

// `#[rec]` 生成的宏（panel! / text_label! / col_header! / kv_row!）是 `#[macro_export]`
// 在 crate 根，其展开体引用 `$crate::XxxArg`，故必须把 `XxxArg` 再导出到 crate 根，
// 宏才能解析。每新增一个 `#[rec]` 组件都需在此补一行。
pub use ui2::widgets::{ColHeaderArg, KvRowArg, PanelArg, TextLabelArg};
// `#[rec(extends = node2)]` 的 `panel!` 宏展开体引用 `$crate::Node2Props`，
// 必须把 `Node2Props` 也再导出到 crate 根，否则该绝对路径解析失败。
pub use wy_mve::Node2Props;

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::{Duration, Instant};

use wy_engine::node2_app::Node2App;
use wy_engine::runner::WyApp;
use wy_mve::canvas_render2::build_root_node2;
use wy_mve::canvas_render2::node2::{always, get};
use wy_render::Scene;
use wy_signal::{batch, GetValue, SetValue, Signal};

use binance::{Feed, Msg, Session, Source};
use market::{
    merge_trades, replace_klines, upsert_ticker, Candle, Depth, Status, Ticker, Trade,
    WATCH_SYMBOLS,
};
use ui2::dashboard;

/// 数据泵节拍：40ms ≈ 25Hz，足以覆盖 1.2s 一次的数据到达。
const WAKE_EVERY: Duration = Duration::from_millis(40);
/// 初始窗口尺寸（与截图同一套）。
const WINDOW: (f32, f32) = (1440.0, 900.0);

/// 画布根 + 页面 + 浮层，包成 [`Node2App`]。
///
/// 浮层（盘口设置菜单）由 `ui2::dashboard` 自己挂——它的可视区取父节点 inner，
/// 只有挂在画布根下那一层整窗盒子里才等于整窗。文档序最后 → 绘在最上层。
fn make_app(model: Rc<Model>, ctl: Rc<Ctl>, w: f32, h: f32) -> Node2App {
    Node2App::new(move |viewport| {
        let m = Rc::clone(&model);
        let c = Rc::clone(&ctl);
        // `root_arg` 吃掉 viewport（根要用它算尺寸），页面自己也要一份。
        let vp = Rc::clone(&viewport);
        build_root_node2(root_arg(viewport), move |cx| {
            cx.child(dashboard(m, c, vp));
        })
    })
    .with_size(w, h)
    .with_title("wy-ui · finance · canvasRender2")
}

/// 画布根：原点 `(0,0)`、尺寸等于视口（TS 画布根同款）。
///
/// 根没有父节点，`defaultPosition` 无从推导，所以 `x`/`y` 必须显式给 0。
fn root_arg(viewport: Rc<Cell<(f32, f32)>>) -> Node2Props {
    let (vw, vh) = (viewport.clone(), viewport);
    Node2Props {
        x: Some(always(0.0)),
        y: Some(always(0.0)),
        width: Some(get(move || vw.get().0)),
        height: Some(get(move || vh.get().1)),
        ..Node2Props::default()
    }
}

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
    // K 线视口（visible / offset）是 chart 面板私有的，见 `ui2::chart::ChartState`；
    // 深度图是否独占视口由 `ui2::dashboard` 持有；
    // 盘口点价只在盘口面板内用，见 `ui2::book::BookState::picked`。
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
        // key 一变自动回到默认视口（见 `ui2::chart::ChartState::view`）。
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
// 应用：Node2App + 数据泵
// ---------------------------------------------------------------------------

struct Dashboard {
    inner: Node2App,
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
        // 灌一份合成行情，保证出图内容稳定可复现
        fill_synthetic(&model);
        let app = make_app(model, ctl, w as f32, h as f32);
        wy_app::snapshot::save_snapshot(&app, &path, w, h).map_err(std::io::Error::other)?;
        return Ok(());
    }

    let model = Rc::new(Model::new());
    let (feed, rx) = Feed::new();
    let ctl = Rc::new(Ctl::new(feed, model.clone()));

    // 自选会话常驻；主行情先在默认标的上开一条
    ctl.start_watch();
    ctl.open_market("BTCUSDT", "15m");

    let inner = make_app(model.clone(), ctl.clone(), WINDOW.0, WINDOW.1);
    let app = Dashboard {
        inner,
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
    use wy_mve::canvas_render2::protocol::{create_mouse_event2, MouseEventInit2, MouseType2};
    // 拖拽的 move/up 走全局广播（与真实宿主 `Node2Deps::dispatch` 同一路径）。
    use wy_mve::canvas_render2::renderer::{dispatch_global_mouse_move, dispatch_global_mouse_up};
    use wy_mve::canvas_render2::tree::engine_global::probe_regions;
    use wy_mve::canvas_render2::tree::geometry::clear_layout_memo;
    use wy_mve::Node2;

    const VW: f32 = 1440.0;
    const VH: f32 = 900.0;

    // ========================================================================
    // 建树 / 遍历 / 派发
    // ========================================================================
    //
    // v1 那套 `layout_boxes` + `children_nodes` + `last_box` + `on_click_fn`
    // 随 `ui/` 一起退役。这里换成 v2 的等价手段：
    // `Node2::width()/height()/abs_x()/abs_y()`（pull 式实时几何）、
    // `raw_children()`（不过滤 hide 的遍历）、`Node2::mouse_click()` 等
    // （节点消息，不经命中链）。

    /// 建整棵树并跑一帧（填布局 memo 与容器尺寸探针）。
    fn build_at(w: f32, h: f32) -> (Rc<Model>, Node2) {
        let model = synth_model();
        let (feed, _rx) = Feed::new();
        let ctl = Rc::new(Ctl::new(feed, Rc::clone(&model)));
        let viewport = Rc::new(Cell::new((w, h)));
        let vp = Rc::clone(&viewport);
        let m = Rc::clone(&model);
        let root = build_root_node2(root_arg(viewport), move |cx| {
            cx.child(dashboard(m, ctl, vp));
        });
        frame(&root);
        (model, root)
    }

    fn build() -> (Rc<Model>, Node2) {
        build_at(VW, VH)
    }

    /// 跑一帧：失效布局 memo、重算动态区域、画一遍（同时填探针）。
    fn frame(root: &Node2) {
        clear_layout_memo();
        probe_regions();
        let mut scene = Scene::new();
        root.render_node2(&mut scene);
    }

    /// 按 `debug_name` 找节点（用 `raw_children`，隐藏的浮层也能找到）。
    fn by_name(node: &Node2, name: &str) -> Option<Node2> {
        if node.debug_name().as_deref() == Some(name) {
            return Some(node.clone());
        }
        node.raw_children()
            .into_iter()
            .find_map(|c| by_name(&c, name))
    }

    fn named(root: &Node2, name: &str) -> Node2 {
        by_name(root, name).unwrap_or_else(|| panic!("未找到节点 {name}"))
    }

    /// 收集所有同名节点（文档序）。
    fn all(root: &Node2, name: &str, out: &mut Vec<Node2>) {
        if root.debug_name().as_deref() == Some(name) {
            out.push(root.clone());
        }
        for c in root.raw_children() {
            all(&c, name, out);
        }
    }

    fn event(type_: MouseType2, gx: f32, gy: f32) -> wy_mve::canvas_render2::protocol::MouseEvent2 {
        create_mouse_event2(MouseEventInit2 {
            type_,
            x: 0.0,
            y: 0.0,
            global_x: gx,
            global_y: gy,
            device: None,
            pointer_id: None,
            button: None,
            buttons: None,
            wheel_delta_x: None,
            wheel_delta_y: None,
            pressure: None,
        })
    }

    fn click(node: &Node2) {
        let (cx, cy) = (
            node.abs_x() + node.width() / 2.0,
            node.abs_y() + node.height() / 2.0,
        );
        node.mouse_click(&mut event(MouseType2::Click, cx, cy));
    }

    /// 造一份合成行情：ticker / K 线 / 20 档盘口 / 1000 档深盘口 / 自选。
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
            (0..40)
                .map(|i| Trade {
                    id: i,
                    price: 67_000.0 + (i as f64).sin() * 12.0,
                    qty: 0.05 + i as f64 * 0.01,
                    time: 1_700_000_000_000 + i as i64 * 1_000,
                    buyer_maker: i % 2 == 0,
                })
                .collect(),
        );
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

    // ========================================================================
    // 绘制链路
    // ========================================================================

    /// 整块看板能画完，不 panic、图元数够多。
    #[test]
    fn draws_full_dashboard_without_panicking() {
        let model = synth_model();
        let (feed, _rx) = Feed::new();
        let ctl = Rc::new(Ctl::new(feed, Rc::clone(&model)));
        let app = make_app(model, ctl, VW, VH);
        let mut scene = Scene::new();
        app.draw(&mut scene, VW, VH);
        assert!(scene.len() > 300, "场景图元太少：{}", scene.len());
    }

    /// **离线可跑**保证：默认数据源是内置合成源，不依赖任何网络/外部服务。
    #[test]
    fn default_source_is_offline_synthetic() {
        assert_eq!(Source::initial(), Source::Sim, "默认必须是离线合成源");
        assert!(Source::Sim.is_sim());
        assert!(!Source::Binance.is_sim());
    }

    /// 合成源能产出**完整的一帧看板**（ticker / K 线 / 盘口 / 深盘口 / 成交齐全）。
    #[test]
    fn synthetic_source_produces_a_complete_frame() {
        let now = 1_760_000_000_000i64;

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

        let model = Rc::new(Model::new());
        model.ticker.set(Some(ticker));
        model.klines.set(klines);
        model.depth.set(Some(depth));
        model.deep.set(Some(deep));
        model.trades.set(trades);
        model.watch.set(vec![model.ticker.get().unwrap()]);
        model.status.set(Status::Live);

        let (feed, _rx) = Feed::new();
        let ctl = Rc::new(Ctl::new(feed, Rc::clone(&model)));
        let app = make_app(model, ctl, VW, VH);
        let mut scene = Scene::new();
        app.draw(&mut scene, VW, VH);
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
        for pair in g.bid.totals.windows(2) {
            assert!(pair[1] >= pair[0], "累计曲线必须单调不减");
        }
    }

    // ========================================================================
    // 结构与几何
    // ========================================================================

    /// 画布根铺满视口（浮层的可视区靠它）。
    #[test]
    fn root_fills_the_viewport() {
        let (_, root) = build();
        let page = named(&root, "finance-root");
        assert_eq!((page.width(), page.height()), (VW, VH), "根应铺满视口");
    }

    /// 顶栏在上、三列在下：顶栏底边不越过任何一列的顶边。
    #[test]
    fn topbar_sits_above_the_three_columns() {
        let (_, root) = build();
        let bar = named(&root, "topbar");
        assert!(bar.height() > 0.0, "顶栏应有高度");
        let bar_bottom = bar.abs_y() + bar.height();
        for tag in ["col-left", "col-center", "col-right"] {
            let col = named(&root, tag);
            assert!(col.height() > 0.0, "{tag} 高度应大于 0");
            assert!(
                col.abs_y() >= bar_bottom - 0.5,
                "{tag} 应排在顶栏下方：{} < {bar_bottom}",
                col.abs_y()
            );
        }
    }

    /// 三列横向依次排开、互不重叠，且横向铺满视口。
    #[test]
    fn three_columns_are_ordered_and_disjoint() {
        let (_, root) = build();
        let l = named(&root, "col-left");
        let c = named(&root, "col-center");
        let r = named(&root, "col-right");
        assert!(l.abs_x() + l.width() <= c.abs_x() + 0.5, "左列与中列重叠");
        assert!(c.abs_x() + c.width() <= r.abs_x() + 0.5, "中列与右列重叠");
        assert!(
            r.abs_x() + r.width() <= VW + 0.5,
            "右列越出视口右边：{}",
            r.abs_x() + r.width()
        );
        assert!(c.width() > l.width(), "中列应比左列宽（grow 1）");
    }

    /// 窗口变宽时三列**按同一比例**等比放大。
    ///
    /// 列宽不是像素常量，而是 `grow` 权重（占比）：flex 给 grow 子项分配的是
    /// `remaining * 权重 / 权重和`，所以换窗口大小只缩放、不改比例。
    #[test]
    fn columns_scale_proportionally_with_window() {
        let (_, narrow) = build_at(1200.0, 800.0);
        let (_, wide) = build_at(1800.0, 900.0);

        // 三列拿到的可用宽度（行宽减去两根 6px 分割条）。
        let cols = |root: &Node2| {
            named(root, "col-left").width()
                + named(root, "col-center").width()
                + named(root, "col-right").width()
        };
        let frac = |root: &Node2, name: &str| named(root, name).width() / cols(root);

        for name in ["col-left", "col-center", "col-right"] {
            let (a, b) = (frac(&narrow, name), frac(&wide, name));
            assert!((a - b).abs() < 0.01, "{name} 的宽度占比应恒定：{a} → {b}");
        }

        // 三列 + 两根分割条正好铺满行宽（无绝对宽度、也无留白）。
        let body = 1200.0 - 16.0;
        assert!(
            (cols(&narrow) + 12.0 - body).abs() < 0.5,
            "三列应铺满行宽：{} + 12 ≠ {body}",
            cols(&narrow)
        );
    }

    /// 深度优先遍历（含隐藏节点与浮层）。
    fn walk(n: &Node2, f: &mut impl FnMut(&Node2)) {
        f(n);
        for c in n.raw_children() {
            walk(&c, f);
        }
    }

    /// 只遍历**可见**子树：隐藏节点上不能再问几何（`width()` 会 panic）。
    fn walk_visible(n: &Node2, f: &mut impl FnMut(&Node2)) {
        if n.hide() {
            return;
        }
        f(n);
        for c in n.raw_children() {
            walk_visible(&c, f);
        }
    }

    /// 收集可见子树里的同名节点。
    fn all_visible(root: &Node2, name: &str, out: &mut Vec<Node2>) {
        walk_visible(root, &mut |n| {
            if n.debug_name().as_deref() == Some(name) {
                out.push(n.clone());
            }
        });
    }

    /// `skip_draw` 在引擎里是「整棵子树一起跳过」（`render_node2` 里 `continue`
    /// 在递归之前），所以**带子节点的节点绝不能声明它**——那批子节点会凭空消失。
    ///
    /// 踩过的坑：`panel` 的标题栏为了"自己不画"写了 `skip_draw: true`，结果面板
    /// 标题、徽标、标题栏按钮全部不显示，而检查树的几何一切正常（节点在、尺寸在、
    /// 也没 `hide`），只有出图时才看得出来。
    #[test]
    fn no_node_with_children_skips_drawing() {
        let (_, root) = build();
        let mut bad: Vec<String> = Vec::new();
        walk(&root, &mut |n| {
            if n.skip_draw() && !n.raw_children().is_empty() {
                bad.push(n.debug_name().unwrap_or_else(|| String::from("(无名)")));
            }
        });
        assert!(
            bad.is_empty(),
            "这些节点有子节点却 skip_draw，子内容会被整棵跳过：{bad:?}"
        );
    }

    /// 每个面板的标题都占到一个真盒子（宽高非 0、落在标题栏高度内）。
    ///
    /// 标题是文本子节点，靠外层 wrapper 的 `Row` 布局量尺寸；wrapper 一旦漏了
    /// `layout`（默认绝对定位），高就是 0，标题会贴在标题栏最上沿而不是居中。
    #[test]
    fn panel_titles_have_a_box() {
        let (_, root) = build();
        let mut titles = Vec::new();
        all(&root, "panel-title", &mut titles);
        assert!(titles.len() >= 4, "至少四个面板应有标题：{}", titles.len());
        for t in &titles {
            assert!(t.width() > 0.0, "面板标题宽度应为正：{}", t.width());
            assert!(t.height() > 0.0, "面板标题高度应为正：{}", t.height());
        }
    }

    /// 列标题行（`col_header`）自身有尺寸：没有 `layout` 的外层会塌成 0×0，
    /// 里面的列会全部叠到同一点。
    #[test]
    fn col_header_rows_have_size() {
        let (_, root) = build();
        let mut headers = Vec::new();
        all(&root, "col-header", &mut headers);
        assert!(
            headers.len() >= 3,
            "至少三个面板有列表头：{}",
            headers.len()
        );
        for h in &headers {
            assert!(h.width() > 50.0, "表头行宽度应撑满面板：{}", h.width());
            assert!(h.height() > 0.0, "表头行高度应为正：{}", h.height());
        }
    }

    /// 自选行是**双层**排版：两行文字各自有高、纵向不重叠。
    ///
    /// 两行都只写 `align_item: Center` 而不给 `height` 时，行高会走
    /// `sizeFromParent` → 环闭合取 0，两行落在同一个 y 上、文字直接叠成重影。
    #[test]
    fn watch_row_lines_are_stacked() {
        let (_, root) = build();
        let mut rows = Vec::new();
        all(&root, "watch-row", &mut rows);
        assert!(!rows.is_empty());
        for r in &rows {
            let inner = r.raw_children().into_iter().next().expect("行有内层容器");
            let lines = inner.raw_children();
            assert_eq!(lines.len(), 2, "自选行上下两排");
            for l in &lines {
                assert!(l.height() > 0.0, "每一排都应有高度：{}", l.height());
            }
            assert!(
                lines[1].abs_y() >= lines[0].abs_y() + lines[0].height() - 0.5,
                "两排不能重叠：{} + {} > {}",
                lines[0].abs_y(),
                lines[0].height(),
                lines[1].abs_y()
            );
        }
    }

    /// 顶栏里的 tab 横向依次排开、互不重叠（都要有真实宽度）。
    ///
    /// 漏了 `layout` 的包装容器会让组内 tab 全钉在 (0,0)，看起来像"按钮叠在
    /// 价格文字上"。
    #[test]
    fn topbar_tabs_are_laid_out_in_a_row() {
        let (_, root) = build();
        let mut visible = Vec::new();
        all_visible(&root, "tab", &mut visible);
        assert!(
            visible.len() >= 8,
            "标的与周期 tab 共至少 8 个：{}",
            visible.len()
        );
        for t in &visible {
            assert!(t.width() > 0.0, "tab 宽度应为正");
            assert!(t.height() > 0.0, "tab 高度应为正");
        }
        for pair in visible.windows(2) {
            assert!(
                pair[1].abs_x() >= pair[0].abs_x() + pair[0].width() - 0.5,
                "相邻 tab 重叠：{} + {} > {}",
                pair[0].abs_x(),
                pair[0].width(),
                pair[1].abs_x()
            );
        }
    }

    /// 布局自检：所有面板／列都在视口内，尺寸有限且为正。
    ///
    /// 组件化之后位置全由 flex 决定，一旦某块面板忘了 `grow` / `Stretch`，
    /// 它会以 0 宽（画不出也点不中）或越界的形式出现，这里直接兜住。
    #[test]
    fn panels_stay_inside_the_viewport() {
        for (w, h) in [(1440.0, 900.0), (1100.0, 720.0), (1920.0, 1080.0)] {
            let (_, root) = build_at(w, h);
            let mut names = vec!["finance-root", "topbar"];
            names.extend([
                "col-left",
                "col-center",
                "col-right",
                "panel-watchlist",
                "panel-stats",
                "panel-chart",
                "panel-depth",
                "panel-book",
                "panel-trades",
            ]);
            for name in names {
                let n = named(&root, name);
                let (x, y, bw, bh) = (n.abs_x(), n.abs_y(), n.width(), n.height());
                assert!(
                    x.is_finite() && y.is_finite() && bw.is_finite() && bh.is_finite(),
                    "{w}x{h} {name} 出现非有限盒：({x}, {y}, {bw}, {bh})"
                );
                assert!(bw > 0.0 && bh > 0.0, "{w}x{h} {name} 尺寸塌了：{bw}x{bh}");
                assert!(
                    x >= -0.5 && y >= -0.5,
                    "{w}x{h} {name} 越出左上：({x}, {y})"
                );
                assert!(
                    x + bw <= w + 0.5 && y + bh <= h + 0.5,
                    "{w}x{h} {name} 越出右下：({}, {})",
                    x + bw,
                    y + bh
                );
            }
        }
    }

    // ========================================================================
    // 交互
    // ========================================================================

    /// 拖过列宽之后，**行情刷新**不能把它打回默认。
    ///
    /// 整块看板挂在 `if_node` → `render_when` 的分支里，分支闭包会重跑
    /// （行情 tick 就是最常见的触发）。列宽/分格比这些拖拽信号若在闭包**内部**
    /// 新建，重跑一次就换回初始值——用户看到的就是"怎么拖都没用"。
    /// 它们必须与 `full` / `book_state` 一样造在 `dashboard`（只执行一次）。
    #[test]
    fn dragged_width_survives_a_data_refresh() {
        let (model, root) = build();
        let before = named(&root, "col-left").width();

        let gutter = named(&root, "gutter-x");
        let (gx, gy) = (
            gutter.abs_x() + gutter.width() / 2.0,
            gutter.abs_y() + gutter.height() / 2.0,
        );
        gutter.mouse_down(&mut event(MouseType2::Down, gx, gy));
        dispatch_global_mouse_move(gx + 60.0, gy);
        dispatch_global_mouse_up();
        frame(&root);

        let dragged = named(&root, "col-left").width();
        assert!(
            (dragged - before - 60.0).abs() < 1.5,
            "拖动应生效：{before} → {dragged}"
        );

        // 行情 tick：构建期读到的 `ticker` 变了 → `render_when` 分支重跑
        let mut t = model.ticker.get().expect("应有 ticker");
        t.last += 1.0;
        model.ticker.set(Some(t));
        frame(&root);

        let after = named(&root, "col-left").width();
        assert!(
            (after - dragged).abs() < 0.5,
            "数据刷新不该复位列宽：{dragged} → {after}"
        );
    }

    /// 拖列间分割条：左列宽跟着指针位移变，且不改树结构。
    #[test]
    fn dragging_column_gutter_resizes_columns() {
        let (_, root) = build();
        let before = named(&root, "col-left").width();

        // 文档序第一根竖条 = 左列与中列之间那根
        let gutter = named(&root, "gutter-x");
        let gx = gutter.abs_x() + gutter.width() / 2.0;
        let gy = gutter.abs_y() + gutter.height() / 2.0;

        // 起手在节点上（命中链派发 down），随后的 move/up 走**全局**广播——
        // 与真实宿主一致（`Node2Deps::dispatch` 里广播，对标 TS `Renderer.ts`）。
        gutter.mouse_down(&mut event(MouseType2::Down, gx, gy));
        dispatch_global_mouse_move(gx + 60.0, gy);
        dispatch_global_mouse_up();
        frame(&root);

        let after = named(&root, "col-left").width();
        assert!(
            (after - before - 60.0).abs() < 1.5,
            "左列应宽出 60px：{before} → {after}"
        );
        // 结构不变：三列仍在
        assert!(by_name(&root, "col-center").is_some(), "拖动不该改树结构");
    }

    /// 拖行间分割条：上下两块按比例分高，比例是**信号**而不是像素。
    #[test]
    fn dragging_row_gutter_changes_split() {
        let (_, root) = build();
        let col = named(&root, "col-center");
        let top_before = named(&root, "panel-chart").height();

        // 中列里的竖条：文档序里第一根
        let gutter = by_name(&col, "gutter-y").expect("中列应有行间分割条");
        let gx = gutter.abs_x() + gutter.width() / 2.0;
        let gy = gutter.abs_y() + gutter.height() / 2.0;
        let dim = col.height().max(1.0);

        gutter.mouse_down(&mut event(MouseType2::Down, gx, gy));
        dispatch_global_mouse_move(gx, gy + dim * 0.1);
        dispatch_global_mouse_up();
        frame(&root);

        let top_after = named(&root, "panel-chart").height();
        assert!(
            top_after > top_before + dim * 0.05,
            "上格应吃掉约 10% 高度：{top_before} → {top_after}（列高 {dim}）"
        );
    }

    /// 点「FULL」→ 整页切到"深度图独占"子树：盘口 / 自选等面板都不在树里了。
    #[test]
    fn full_toggle_swaps_the_whole_page() {
        let (_, root) = build();
        assert!(by_name(&root, "panel-book").is_some(), "初始应有盘口面板");

        click(&named(&root, "depth-full"));
        frame(&root);

        assert!(
            by_name(&root, "panel-book").is_none(),
            "切到 FULL 后盘口面板应整枝消失"
        );
        assert!(
            by_name(&root, "panel-watchlist").is_none(),
            "切到 FULL 后自选面板应整枝消失"
        );
        let depth = named(&root, "panel-depth");
        assert!(
            depth.height() > VH * 0.8,
            "FULL 下深度面板应占满视口：{}",
            depth.height()
        );
    }

    /// 盘口设置菜单：点三点按钮 → 浮层在按钮下方展开（挂画布根下）。
    #[test]
    fn book_menu_opens_below_its_button() {
        let (_, root) = build();
        let btn = named(&root, "book-menu");
        let (bx, by) = (btn.abs_x(), btn.abs_y());

        // 事件里的 `global - local` 就是按钮原点（真实运行时同理）
        btn.mouse_click(&mut event(MouseType2::Click, bx, by));
        frame(&root);

        let panel = named(&root, "dropdown-panel");
        assert!(!panel.hide(), "点完三点按钮，菜单应展开");
        assert_eq!(panel.width(), 120.0, "面板宽度由参数给定");
        assert!(
            panel.abs_y() >= by + 20.0,
            "菜单应在按钮下方：按钮 y={by}，菜单 y={}",
            panel.abs_y()
        );
        assert!(
            panel.abs_y() + panel.height() <= VH + 0.5,
            "菜单不该越出窗口底部"
        );
        assert!(
            (panel.abs_x() + panel.width() - (bx + 20.0)).abs() < 1.0,
            "右对齐：菜单右缘应贴住按钮右缘"
        );
    }

    /// 盘口每档都是定高行，两侧各 `ROWS` 档。
    #[test]
    fn book_rows_are_fixed_height() {
        let (_, root) = build();
        let mut rows = Vec::new();
        all(&root, "book-row", &mut rows);
        assert_eq!(
            rows.len(),
            ui2::book::ROWS * 2,
            "盘口两侧各 {} 档",
            ui2::book::ROWS
        );
        for r in &rows {
            assert!(
                (r.height() - ui2::book::ROW_H).abs() < 0.5,
                "盘口行必须定高 {}：实际 {}",
                ui2::book::ROW_H,
                r.height()
            );
        }
    }

    /// 点某一档 → 中间价行出现 LIMIT 标记（点价只在面板内，不写回 Model）。
    #[test]
    fn clicking_a_book_row_sets_the_limit_marker() {
        let (_, root) = build();
        let mut rows = Vec::new();
        all(&root, "book-row", &mut rows);
        assert!(!rows.is_empty());

        click(&rows[0]);
        frame(&root);

        // 点价标记是中间价行里唯一带 ACCENT 的文本节点，用几何认它：
        // 中间价行三段等宽，标记落在正中那一格里。
        let mid = named(&root, "book-mid");
        let marker = mid
            .raw_children()
            .into_iter()
            .nth(1)
            .expect("中间价行有三段");
        let texts = marker.raw_children();
        assert!(!texts.is_empty(), "中段应挂着标记文本");
        assert!(
            texts.iter().any(|t| !t.hide()),
            "点过一档之后，中段应至少有一行可见文字"
        );
    }
}
