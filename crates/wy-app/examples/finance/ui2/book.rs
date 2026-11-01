//! 盘口面板：卖盘 / 中间价 / 买盘 + 底部买卖比例条。
//!
//! 对照 ooc `orderbook.ooc` 的功能面：
//! - 三列 Price / Amount / Total（Total = 名义金额）；
//! - 表头左侧三个模式图标（Both / Bids only / Asks only）；
//! - 标题右侧三点按钮展开设置菜单：Avg&Sum、B/S Ratio、Rounding、
//!   Depth Viz（Amount / Cumulative）、Animations；
//! - 深度条贴右生长、点价、精度聚合（badge 显示当前步长）。
//!
//! # 与 v1 的两处结构性差别
//!
//! 1. **文字全是子节点**。v1 的中间价行 / 均价行 / 比例条是 `theme::text` 直接
//!    往 `Scene` 上画；v2 一律改成 `text_label!` + 布局，由引擎撑尺寸。
//! 2. **设置菜单由装配层挂**。v2 `dropdown` 的浮层根尺寸取**父节点 inner**
//!    （TS `innerSize(view, d)`），塞进盘口面板里可视区就只有那一列宽。所以
//!    [`BookState`] 从面板里提出来、由 `ui2::mod` 创建，菜单经 [`overlays`]
//!    交给装配层挂到画布根下——这也是它比 v1 多一个 `pub` 的原因。
//!
//! 每一档都是独立节点：命中、悬停、点击由引擎按树路由；行高**固定**
//! （对齐 ooc `bookRowHeight = 18`），面板矮于内容时整体进滚动容器，
//! 中间价行钉在面板中间不随滚动跑。

use std::cell::RefCell;
use std::rc::Rc;

use wy_mve::canvas_render2::node2::{Axis, Ext};
use wy_mve::canvas_render2::protocol::MouseEvent2;
use wy_mve::canvas_render2::{dropdown, for_each_node, Placement, PlacementRect};
use wy_mve::{
    flex, grow, grow_dyn, node2, AlignItem, CursorType, DirectionJustify, Fc, Layout, Node2, Scene,
};
use wy_render::Rect;
use wy_signal::{GetValue, SetValue, Signal};

// `text_label!` / `panel!` / `col_header!` 由 `#[macro_use] pub mod widgets;`
// 提供，**不要**再 `use` 同名宏——那会撞成 E0659 ambiguous。
use super::widgets::{
    chip_on, cols_row, dots_btn_ev, scroll_view, toggle_chip, ColCell, COLS_3, PAD,
};
use crate::bookview::{self, BarMode, BookMode, BookRow};
use crate::market::{format_price, format_qty};
use crate::{theme, Model};

/// 每侧行数：行高固定 [`ROW_H`]，面板矮于内容时由滚动容器兜底
/// （对齐 ooc `bookRowHeight = 18`）。
pub const ROWS: usize = 10;
/// 盘口行高（定高，不随面板 grow 均分）。
pub const ROW_H: f32 = 18.0;
/// 中间价行高。
const MID_H: f32 = 24.0;
/// 比例条高度。
const RATIO_H: f32 = 20.0;
/// 横轴定向的弹性空白（只吃主轴的剩余宽度，不吃高度）。
fn h_spacer() -> Fc {
    node2! {
        exts: vec![grow_dyn(|d| if d == Axis::X { 1.0 } else { 0.0 })],
        skip_draw: true,
    }
}

/// 盘口面板的状态：一份共享 + 一批局部开关。
///
/// **非必要不提升**。这些开关只被这块面板自己读写——没有别的面板、顶栏或
/// 控制器需要知道"盘口现在是不是累计口径"。
///
/// 唯一被抬到装配层的是**它本身**（而不是某个字段进全局 `Model`）：
/// v2 的 `dropdown` 必须挂在画布根下才拿得到整窗可视区，而菜单的开关与锚点
/// 必须和触发按钮共享同一份状态，所以 `ui2::mod` 造 `BookState`、面板与浮层
/// 各取一个 `Rc` 句柄。`Model` 本身仍然一份共享、不进这里的状态集合。
///
/// 点价（`picked`）用**带 symbol 的 key**：换标的天然失效，不需要控制器
/// 记得去清。
pub struct BookState {
    /// 跨面板共享的行情数据。
    pub model: Rc<Model>,
    /// 显示模式（Both / Bids only / Asks only）。
    pub mode: Signal<BookMode>,
    /// 精度聚合步长（0 = 自动）。
    pub step: Signal<f64>,
    /// 深度条口径：false = Amount，true = Cumulative。
    pub cumulative: Signal<bool>,
    /// Total 列是否收成 K/M（false = 完整数值）。
    pub rounding: Signal<bool>,
    /// 是否显示底部 B/S 比例条。
    pub ratio: Signal<bool>,
    /// 是否显示两侧均价与合计（Avg&Sum）。
    pub avg: Signal<bool>,
    /// 深度条是否走动画（保留开关，绘制层按帧插值）。
    pub anim: Signal<bool>,
    /// 设置菜单是否展开。
    pub menu: Signal<bool>,
    /// 菜单锚点：触发按钮的窗口矩形 `(x, y, w, h)`，事件期捕获。
    pub anchor: Signal<(f32, f32, f32, f32)>,
    /// 点价：`(标的, 价格)`。标的对不上就当没点过。
    picked: Signal<Option<(String, f64)>>,
}

impl BookState {
    /// 造一份面板状态。由装配层（`ui2::mod`）调用一次，面板与浮层共享。
    pub fn new(model: Rc<Model>) -> Self {
        Self {
            model,
            mode: Signal::new(BookMode::Both),
            step: Signal::new(0.0),
            cumulative: Signal::new(false),
            rounding: Signal::new(true),
            ratio: Signal::new(true),
            avg: Signal::new(false),
            anim: Signal::new(true),
            menu: Signal::new(false),
            anchor: Signal::new((0.0, 0.0, 0.0, 0.0)),
            picked: Signal::new(None),
        }
    }

    /// 点价：记下当前标的与这一档的价格。
    fn pick(&self, price: f64) {
        self.picked.set(Some((self.model.symbol.get(), price)));
    }

    /// 读点价：标的已经换了就当没点过。
    fn picked_price(&self) -> Option<f64> {
        let symbol = self.model.symbol.get();
        self.picked
            .get()
            .and_then(|(sym, price)| (sym == symbol).then_some(price))
    }
}

fn bar_mode(b: &BookState) -> BarMode {
    if b.cumulative.get() {
        BarMode::Cumulative
    } else {
        BarMode::Amount
    }
}

/// 当前盘口视图（精度 / 口径 / 行数都按状态现算）。
///
/// 先把 `Option<Depth>` 取成局部量再借用：`Signal::get` 返回的是临时值，
/// 直接 `.as_ref()` 传参会让借用活不过语句。
fn view_of(b: &BookState) -> bookview::BookView {
    let depth = b.model.depth.get();
    bookview::view(depth.as_ref(), ROWS, b.step.get(), bar_mode(b))
}

/// 盘口面板（内容部分；设置菜单见 [`overlays`]）。
pub fn book_panel(b: Rc<BookState>, exts: Vec<Ext>) -> Fc {
    let actions = vec![
        mode_btn(Rc::clone(&b), "≡", BookMode::Both),
        mode_btn(Rc::clone(&b), "▲", BookMode::Bids),
        mode_btn(Rc::clone(&b), "▼", BookMode::Asks),
        menu_btn(Rc::clone(&b)),
    ];

    let b_step = Rc::clone(&b);
    let step_label: Rc<dyn Fn() -> String> =
        Rc::new(move || bookview::format_step(view_of(&b_step).step));

    // 内容区各段直接作为面板的子节点：内容列本身已经是 `grow: 1` + Stretch 的
    // 纵向 flex，再套一层只会多出一个"自然高度"的盒子，把下面的 `grow` 行挤成 0。
    panel! {
        debug_name: "panel-book".to_string(),
        exts: exts,
        title: Rc::new(|| String::from("ORDER BOOK")),
        badge: step_label,
        actions: actions,
        children: vec![
            col_header! {
                cells: vec![
                    ("Price", COLS_3.0),
                    ("Amount", COLS_3.1),
                    ("Total", COLS_3.2),
                ],
            },
            side_list(Rc::clone(&b), false),
            mid_row(Rc::clone(&b)),
            side_list(Rc::clone(&b), true),
            avg_sum_row(Rc::clone(&b)),
            ratio_bar(Rc::clone(&b)),
        ],
    }
}

/// 设置菜单（浮层）：由装配层挂到**画布根下**，不占面板布局空间。
///
/// 五个开关的差别只有「读写哪个信号」，所以收敛成 [`toggle_of`]：
/// 新增一个开关是一行，不是一段。
pub fn overlays(b: &Rc<BookState>) -> Vec<Fc> {
    let items = vec![
        toggle_of(Rc::clone(b), "AVG&SUM", |s| s.avg),
        toggle_of(Rc::clone(b), "B/S", |s| s.ratio),
        toggle_of(Rc::clone(b), "ROUND", |s| s.rounding),
        toggle_of(Rc::clone(b), "CUM", |s| s.cumulative),
        toggle_of(Rc::clone(b), "ANIM", |s| s.anim),
    ];

    let b_open = Rc::clone(b);
    let b_anchor = Rc::clone(b);
    let b_close = Rc::clone(b);

    vec![dropdown(
        move || b_open.menu.get(),
        move || {
            let (x, y, w, h) = b_anchor.anchor.get();
            PlacementRect::new(x, y, w, h)
        },
        // 按钮在标题栏最右：向下、右对齐弹，右侧不会被窗口切掉。
        || Placement::BottomEnd,
        6.0,
        120.0,
        move || b_close.menu.set(false),
        || theme::PANEL,
        || theme::BORDER,
        items,
    )]
}

/// 一个开关：`get` 指向面板状态里的布尔信号，点击即取反。
fn toggle_of(
    b: Rc<BookState>,
    label: &'static str,
    get: impl Fn(&BookState) -> Signal<bool> + 'static,
) -> Fc {
    // `get` 要给「读」和「写」两处用，而两处各自是独立闭包：先包一层 `Rc`
    // 再分两份，`impl Fn` 本身不可克隆。
    let get_read = Rc::new(get);
    let get_write = Rc::clone(&get_read);
    let b_read = Rc::clone(&b);
    let b_write = Rc::clone(&b);

    toggle_chip(
        label,
        move || get_read(&b_read).get(),
        move || {
            let sig = get_write(&b_write);
            let on = sig.get();
            sig.set(!on);
        },
    )
}

/// 模式图标：Both / Bids only / Asks only（与其它标题栏按钮同一套外观）。
fn mode_btn(b: Rc<BookState>, label: &'static str, mode: BookMode) -> Fc {
    const W: f32 = 20.0;
    let b_active = Rc::clone(&b);
    let b_click = Rc::clone(&b);
    chip_on(
        label,
        W,
        theme::MUTED,
        move || b_active.mode.get() == mode,
        move || b_click.mode.set(mode),
    )
}

/// 更多操作开关：打开设置菜单（三点自绘，不依赖字体里的 `⋯`）。
///
/// 锚点必须在这里捕获：只有事件期才拿得到按钮的窗口原点
/// （`global_x - x`），宽高是常量，一并存进 `anchor`。
fn menu_btn(b: Rc<BookState>) -> Fc {
    const W: f32 = 20.0;
    let b_active = Rc::clone(&b);
    let b_click = Rc::clone(&b);

    dots_btn_ev(
        "book-menu",
        W,
        move || b_active.menu.get(),
        move |e: &mut MouseEvent2| {
            let x = e.global_x - e.x;
            let y = e.global_y - e.y;
            b_click.anchor.set((x, y, W, 20.0));
            let open = b_click.menu.get();
            b_click.menu.set(!open);
        },
    )
}

/// 一侧列表：`for_each_node` 按档位下标复用行，**行定高、整体进滚动容器**。
///
/// 中间价行固定在面板中间（不滚动），两侧各自滚动——对齐 ooc 的做法：
/// 盘口滚来滚去时锚点不能跑。
fn side_list(b: Rc<BookState>, bid: bool) -> Fc {
    let b_each = Rc::clone(&b);
    let b_row = Rc::clone(&b);

    let rows = for_each_node(
        move |emit| {
            let mode = b_each.mode.get();
            if bid && !mode.shows_bids() {
                return;
            }
            if !bid && !mode.shows_asks() {
                return;
            }
            let v = view_of(&b_each);
            let side = if bid { &v.bids } else { &v.asks };
            for (i, row) in side.iter().enumerate() {
                emit((bid, i), *row);
            }
        },
        move |_k, row: Rc<RefCell<BookRow>>, cx| {
            // 行是在动态区域里构建的，拿不到 `side_list` 的局部量——所以
            // `Rc<BookState>` 显式捕获进创建闭包（v2 不做 `ctx.consume`）。
            cx.child(book_row(Rc::clone(&b_row), row));
        },
    );

    // 模式隐藏这一侧时整段不进布局（`hide` 同时退出布局与命中），
    // 另一侧随即占满剩余高度。
    let b_hide = Rc::clone(&b);
    node2! {
        debug_name: if bid { "book-bids" } else { "book-asks" }.to_string(),
        exts: vec![grow(1.0)],
        layout: flex! { direction: Layout::Column, align_item: AlignItem::Stretch, align_fix: true },
        hide: {
            let mode = b_hide.mode.get();
            if bid { !mode.shows_bids() } else { !mode.shows_asks() }
        },
        children: vec![ scroll_view(0.0, vec![rows]) ],
    }
}

/// 一档：深度条 + 价格 + 数量 + 名义金额。
///
/// 列文字走 [`cols_row`]（与表头 `col_header` 同一套比例），行骨架自己搭
/// （要比 [`list_row`] 多一条"贴右生长"的深度条）。
fn book_row(b: Rc<BookState>, data: Rc<RefCell<BookRow>>) -> Fc {
    let hover = Signal::new(false);
    let hov_draw = hover;

    let b_click = Rc::clone(&b);
    let b_total = Rc::clone(&b);
    let row_click = Rc::clone(&data);
    let row_draw = Rc::clone(&data);
    let row_price = Rc::clone(&data);
    let row_tone = Rc::clone(&data);
    let row_qty = Rc::clone(&data);
    let row_total = Rc::clone(&data);

    node2! {
        debug_name: "book-row".to_string(),
        height: ROW_H,
        cursor: CursorType::Pointer,
        on_hover: move |on| hover.set(on),
        // 点价：拿到本档价格（只在盘口面板内用，不写回共享 Model）
        mouse_click: move |_e: &mut MouseEvent2| {
            let p = row_click.borrow().price;
            if p > 0.0 {
                b_click.pick(p);
            }
        },
        layout: flex! { direction: Layout::Row, align_item: AlignItem::Center, align_fix: true },
        draw: move |s: &mut Scene, n: &Node2| {
            let (w, h) = (n.width(), n.height());
            let row = *row_draw.borrow();
            if hov_draw.get() {
                s.fill_rect(Rect::new(0.0, 0.0, w, h), theme::ROW_ALT);
            }
            // 深度条：贴右生长
            let frac = (row.percent / 100.0).clamp(0.0, 1.0);
            let bar_w = (w - 2.0 * PAD) * frac;
            if bar_w > 0.0 {
                let soft = if row.bid {
                    theme::UP_SOFT
                } else {
                    theme::DOWN_SOFT
                };
                s.fill_rect(
                    Rect::new(w - PAD - bar_w, 1.0, bar_w, (h - 2.0).max(1.0)),
                    soft,
                );
            }
        },
        children: vec![ node2! {
            exts: vec![grow(1.0)],
            padding_inline_start: PAD,
            padding_inline_end: PAD,
            layout: flex! { direction: Layout::Row, align_item: AlignItem::Center, align_fix: true },
            children: cols_row(&[
                ColCell::left(
                    move || format_price(row_price.borrow().price),
                    9.5,
                    move || theme::tone(row_tone.borrow().bid),
                ),
                ColCell::right(COLS_3.1, move || format_qty(row_qty.borrow().qty), 9.5, || theme::MUTED),
                ColCell::right(
                    COLS_3.2,
                    move || bookview::format_total(row_total.borrow().total, !b_total.rounding.get()),
                    9.5,
                    || theme::MUTED,
                ),
            ]),
        } ],
    }
}

/// 中间价分隔行：LAST + 点价标记。
///
/// 三段等宽（各 `grow` 1）分别是「左贴边 / 居中 / 右贴边」，所以
/// LEFT 贴在 PAD、LIMIT 落在整行正中、价格贴在 `w - PAD`，
/// 与 v1 的 `text` / `text_center` / `text_right` 三处锚点等价。
fn mid_row(b: Rc<BookState>) -> Fc {
    // 每个 getter 都是独立的 `move ||`，同一份 `Rc` 被第二个 getter 捕获就
    // `E0382`——一处一份句柄。
    let b_empty_l = Rc::clone(&b);
    let b_empty_c = Rc::clone(&b);
    let b_empty_r = Rc::clone(&b);
    let b_last = Rc::clone(&b);
    let b_last_tone = Rc::clone(&b);
    let b_lim_text = Rc::clone(&b);
    let b_lim_hide = Rc::clone(&b);

    node2! {
        debug_name: "book-mid".to_string(),
        height: MID_H,
        padding_inline_start: PAD,
        padding_inline_end: PAD,
        layout: flex! { direction: Layout::Row, align_item: AlignItem::Center, align_fix: true },
        draw: move |s: &mut Scene, n: &Node2| {
            let (w, h) = (n.width(), n.height());
            s.fill_rect(Rect::new(0.0, 0.0, w, h), theme::ROW_ALT);
            s.fill_rect(Rect::new(PAD, 0.0, (w - 2.0 * PAD).max(0.0), 1.0), theme::BORDER);
            s.fill_rect(Rect::new(PAD, h - 1.0, (w - 2.0 * PAD).max(0.0), 1.0), theme::BORDER);
        },
        children: vec![
            node2! {
                exts: vec![grow_dyn(|d| if d == Axis::X { 1.0 } else { 0.0 })],
                layout: flex! { direction: Layout::Row, align_item: AlignItem::Center, direction_justify: DirectionJustify::Start },
                children: vec![ text_label! {
                    content: "LAST".to_string(),
                    font_size: 9.0,
                    color: theme::FAINT,
                    hide: view_of(&b_empty_l).is_empty(),
                } ],
            },
            node2! {
                exts: vec![grow_dyn(|d| if d == Axis::X { 1.0 } else { 0.0 })],
                layout: flex! { direction: Layout::Row, align_item: AlignItem::Center, direction_justify: DirectionJustify::Center },
                children: vec![
                    // 盘口还没到时不要显示 0.00，占位提示更诚实
                    text_label! {
                        content: "WAITING FOR BOOK".to_string(),
                        font_size: 9.0,
                        color: theme::FAINT,
                        hide: !view_of(&b_empty_c).is_empty(),
                    },
                    // 点价标记（点过某一档后显示，供下单面板参考）
                    text_label! {
                        content: {
                            b_lim_text
                                .picked_price()
                                .map(|p| format!("LIMIT {}", format_price(p)))
                                .unwrap_or_default()
                        },
                        font_size: 9.0,
                        color: theme::ACCENT,
                        hide: b_lim_hide.picked_price().is_none(),
                    },
                ],
            },
            node2! {
                exts: vec![grow_dyn(|d| if d == Axis::X { 1.0 } else { 0.0 })],
                layout: flex! { direction: Layout::Row, align_item: AlignItem::Center, direction_justify: DirectionJustify::End },
                children: vec![ text_label! {
                    content: {
                        let t = b_last.model.ticker.get();
                        t.map(|x| format_price(x.last)).unwrap_or_default()
                    },
                    font_size: 13.0,
                    color: {
                        let t = b_last_tone.model.ticker.get();
                        theme::tone(t.map(|x| x.up()).unwrap_or(true))
                    },
                    hide: view_of(&b_empty_r).is_empty(),
                } ],
            },
        ],
    }
}

/// 两侧均价与合计（对应 ooc 的 `Display Avg.&Sum`）。
fn avg_sum_row(b: Rc<BookState>) -> Fc {
    const H: f32 = 18.0;
    let b_hide = Rc::clone(&b);
    let b_bids = Rc::clone(&b);
    let b_asks = Rc::clone(&b);

    node2! {
        height: H,
        padding_inline_start: PAD,
        padding_inline_end: PAD,
        hide: !b_hide.avg.get(),
        layout: flex! { direction: Layout::Row, align_item: AlignItem::Center, align_fix: true },
        children: vec![
            text_label! {
                content: {
                    let s = bookview::side_summary(&view_of(&b_bids).bids);
                    format!("B {} / {}", format_price(s.avg), bookview::format_total(s.sum, true))
                },
                font_size: 8.5,
                color: theme::UP,
            },
            h_spacer(),
            text_label! {
                content: {
                    let s = bookview::side_summary(&view_of(&b_asks).asks);
                    format!("A {} / {}", format_price(s.avg), bookview::format_total(s.sum, true))
                },
                font_size: 8.5,
                color: theme::DOWN,
            },
        ],
    }
}

/// 底部买卖挂单比例条（对应 ooc 的 B/S ratio）。
///
/// 底色条留在节点的 `draw` 里（它是比例条本体，不是文字），文字仍是子节点；
/// 父节点先画、子节点后画，所以标签自然压在条上。
fn ratio_bar(b: Rc<BookState>) -> Fc {
    let b_hide = Rc::clone(&b);
    let b_draw = Rc::clone(&b);
    let b_left = Rc::clone(&b);
    let b_right = Rc::clone(&b);

    node2! {
        height: RATIO_H,
        padding_inline_start: PAD,
        padding_inline_end: PAD,
        // 标签落在比例条**下方**：条占 y 2..5（`draw` 里），标签从 5 起在 15 的
        // 内容盒里居中 → 约 6.7..18.3。v1 是「条 6..9 + 文字 10..19」，这里把两者
        // 都上提 4px，避开面板底部那条圆角边。
        // 不这么分的话（条 6..9 + 文字垂直居中在 20 里 = 4..16）会把条压在字上。
        padding_block_start: 5.0,
        hide: !b_hide.ratio.get(),
        layout: flex! { direction: Layout::Row, align_item: AlignItem::Center, align_fix: true },
        draw: move |s: &mut Scene, n: &Node2| {
            let w = n.width();
            let ratio = view_of(&b_draw).buy_ratio.clamp(0.0, 1.0) as f32;
            let bid_w = (w * ratio).max(0.0);
            theme::bar(s, 0.0, 2.0, bid_w, 3.0, theme::UP);
            theme::bar(s, bid_w, 2.0, (w - bid_w).max(0.0), 3.0, theme::DOWN);
        },
        children: vec![
            text_label! { content: "B".to_string(), font_size: 9.0, color: theme::UP },
            // `B` 与它的占比之间留 4px（v1 是 `PAD` 与 `PAD + 10` 两处锚点）
            node2! { width: 4.0, skip_draw: true },
            text_label! {
                content: {
                    let v = view_of(&b_left);
                    bookview::format_share(v.buy_ratio)
                },
                font_size: 9.0,
                color: theme::MUTED,
            },
            h_spacer(),
            text_label! {
                content: {
                    let v = view_of(&b_right);
                    bookview::format_share(1.0 - v.buy_ratio)
                },
                font_size: 9.0,
                color: theme::MUTED,
            },
            node2! { width: 4.0, skip_draw: true },
            text_label! { content: "S".to_string(), font_size: 9.0, color: theme::DOWN },
        ],
    }
}
