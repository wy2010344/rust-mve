//! 盘口面板：卖盘 / 中间价 / 买盘 + 底部买卖比例条。
//!
//! 对照 ooc `orderbook.ooc` 的功能面：
//! - 三列 Price / Amount / Total（Total = 名义金额）；
//! - 表头左侧三个模式图标（Both / Bids only / Asks only）；
//! - 标题右侧三点按钮展开设置条：Avg&Sum、B/S Ratio、Rounding、
//!   Depth Viz（Amount / Cumulative）、Animations；
//! - 深度条贴右生长、点价、精度聚合（badge 显示当前步长）。
//!
//! 每一档都是独立节点：命中、悬停、点击由引擎按树路由，行高由 flex 均分，
//! 面板被拖矮时行高自动收窄，不需要在面板里反算行号或行高。

use std::cell::RefCell;
use std::rc::Rc;

use wy_mve::{component, current_box, dropdown, for_each, rsx, AlignItem, Node, Slot};
use wy_render::{Rect, Scene};
use wy_signal::{GetValue, SetValue, Signal};

use super::widgets::{chip_on, col_header, panel_with, toggle_chip, PAD};
use crate::bookview::{self, BarMode, BookMode, BookRow};
use crate::market::{format_price, format_qty};
use crate::{theme, Model};

/// 每侧固定行数：行高由父容器均分（`grow: 1`），面板变矮时自动收窄。
pub const ROWS: usize = 10;
/// 中间价行高。
const MID_H: f32 = 24.0;
/// 比例条高度。
const RATIO_H: f32 = 20.0;

/// 盘口面板的状态：一份共享 + 九个局部。
///
/// **非必要不提升**。九个开关/记录只被这块面板自己读写——没有别的面板、
/// 顶栏或控制器需要知道"盘口现在是不是累计口径"。所以它们就 `Signal::new`
/// 在 [`book_panel`] 里，随组件一起创建、随组件一起被 `Rc` 持有，
/// **不进全局 `Model`**。
///
/// 放进 `Model` 的唯一后果是把「这块面板的显示偏好」变成全应用状态：
/// 换数据源要记得重置、写测试要造一个完整的 `Model`、想复用这块面板得先
/// 接受它带的十来个字段。就近持有则没有这些问题。
///
/// 反过来，`model`（行情数据、当前标的）是**真跨面板**的，这部分照样提升
/// ——提升的门槛是"有第二个读者"，不是"图方便"。它由 `dashboard` provide，
/// 面板只 `consume`。
///
/// 点价（`picked`）是这里最典型的一例：它过去挂在 `Model` 上，理由是
/// "换标的要清掉它"。改成**带 symbol 的 key** 之后，换标的天然失效，
/// 于是它既能留在面板内、又不需要控制器记得去清。
pub struct BookState {
    /// 跨面板共享的部分。
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
    /// 菜单锚点（触发按钮的窗口坐标，事件期捕获）。
    pub anchor: Signal<(f32, f32)>,
    /// 点价：`(标的, 价格)`。标的对不上就当没点过——换标的自动失效。
    picked: Signal<Option<(String, f64)>>,
}

impl BookState {
    fn new(model: Rc<Model>) -> Self {
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
            anchor: Signal::new((0.0, 0.0)),
            picked: Signal::new(None),
        }
    }

    /// 点价：记下当前标的与这一档的价格。
    fn pick(&self, price: f64) {
        self.picked.set(Some((self.model.symbol.get(), price)));
    }

    /// 读点价：标的已经换了就当没点过（省掉"换标的中枢清一次"这类耦合）。
    fn picked_price(&self) -> Option<f64> {
        let symbol = self.model.symbol.get();
        self.picked
            .get()
            .and_then(|(sym, price)| (sym == symbol).then_some(price))
    }
}

#[component]
pub fn book_panel(ctx: &mut NodeContext) {
    // `model` 由 `dashboard` provide；面板自己造自己的局部状态
    let model = ctx
        .consume::<Rc<Model>>()
        .cloned()
        .expect("dashboard 应 provide 了 Rc<Model>");
    let b = Rc::new(BookState::new(model));
    // 行是 `for_each` 的项，拿不到上面的局部变量——provide 给它们
    ctx.provide(Rc::clone(&b));

    let actions = vec![
        Slot::One(mode_btn("≡", Rc::clone(&b), BookMode::Both)),
        Slot::One(mode_btn("▲", Rc::clone(&b), BookMode::Bids)),
        Slot::One(mode_btn("▼", Rc::clone(&b), BookMode::Asks)),
        Slot::One(menu_btn(Rc::clone(&b))),
    ];

    let step_label = {
        let b = Rc::clone(&b);
        move || {
            let v = bookview::view(
                b.model.depth.get().as_ref(),
                ROWS,
                b.step.get(),
                bar_mode(&b),
            );
            bookview::format_step(v.step)
        }
    };

    // 内容区各段直接作为面板的子节点：内容列本身已经是 `grow: 1` + Stretch 的
    // 纵向 flex，再套一层 vbox 只会多出一个"自然高度"的盒子，把下面的
    // `grow` 行全部挤成 0（布局自检会直接报出来）。
    let node = panel_with(
        Rc::new(|| String::from("ORDER BOOK")),
        Rc::new(step_label),
        actions,
        vec![
            rsx! {
                col_header {
                    cells: vec![("Price", 0.0), ("Amount", 0.60), ("Total", 1.0)],
                }
            },
            Slot::One(side_list(Rc::clone(&b), false)),
            Slot::One(mid_row(Rc::clone(&b))),
            Slot::One(side_list(Rc::clone(&b), true)),
            Slot::One(avg_sum_row(Rc::clone(&b))),
            Slot::One(ratio_bar(Rc::clone(&b))),
        ],
    );
    ctx.child(node);
    // 设置菜单是**浮层**：挂在面板外面（文档序最后 → 绘制在最上层），
    // 不占面板布局空间，面板多矮都看得见。
    ctx.child(settings_menu(Rc::clone(&b)));
}

fn bar_mode(b: &BookState) -> BarMode {
    if b.cumulative.get() {
        BarMode::Cumulative
    } else {
        BarMode::Amount
    }
}

/// 模式图标：Both / Bids only / Asks only。
///
/// 和其它面板标题栏上的按钮共用 [`chip_on`] 的外观——过去这里是
/// "裸字形 + 选中下划线"，跟右边的带框三点按钮挨在一起就显参差，
/// 下划线还会被误读成杂线。
fn mode_btn(label: &'static str, b: Rc<BookState>, mode: BookMode) -> Node {
    const W: f32 = 20.0;
    let b_active = Rc::clone(&b);
    let b_click = Rc::clone(&b);
    rsx! {
        chip_on {
            label: label,
            w: W,
            active: move || b_active.mode.get() == mode,
            on_click: move || b_click.mode.set(mode),
        }
    }
    .into_node()
}

/// 更多操作开关：打开**弹出菜单**（三点自绘，不依赖字体里的 `⋯`）。
///
/// 菜单是浮层（`dropdown`）而非内嵌条——面板很矮时内嵌条会把内容挤掉，
/// 浮层则按可视区自动翻转，永远看得见。
fn menu_btn(b: Rc<BookState>) -> Node {
    let b_active = Rc::clone(&b);
    let b_click = Rc::clone(&b);
    let b_anchor = Rc::clone(&b);
    rsx! {
        node {
            width: 20.0,
            height: 17.0,
            focusable: true,
            on_click_ev: move |ev: &mut wy_mve::PointerEvent| {
                // 捕获按钮的窗口坐标作为菜单锚点（事件期）
                b_anchor.anchor.set((ev.root_x - ev.x, ev.root_y - ev.y));
                let open = b_click.menu.get();
                b_click.menu.set(!open);
            },
            draw: move |s: &mut wy_render::Scene| {
                let b = wy_mve::current_box();
                let color = if b_active.menu.get() {
                    theme::TEXT
                } else {
                    theme::MUTED
                };
                // 三点（自绘）
                let (cx, cy) = (b.width / 2.0, b.height / 2.0);
                for k in -1..=1 {
                    s.fill_round_rect(
                        wy_render::Rect::new(cx + k as f32 * 4.5 - 1.0, cy - 1.0, 2.0, 2.0),
                        1.0,
                        color,
                    );
                }
            },
        }
    }
    .into_node()
}

/// 设置菜单：**浮层下拉**（ooc 的 orderbook 用弹出菜单，这里对齐）。
///
/// 五个开关的差别只有「读写哪个信号」，所以收敛成 [`toggle_of`]：
/// 新增一个开关是一行，不是一段。
///
/// 菜单是浮层（`dropdown`）：盘口面板往往很矮，内嵌条会把档位挤没；
/// 浮层按可视区自动翻转（下方没空间就向上弹），永远看得见。
fn settings_menu(b: Rc<BookState>) -> Node {
    let items = vec![
        toggle_of(Rc::clone(&b), "AVG&SUM", |b| b.avg),
        toggle_of(Rc::clone(&b), "B/S", |b| b.ratio),
        toggle_of(Rc::clone(&b), "ROUND", |b| b.rounding),
        toggle_of(Rc::clone(&b), "CUM", |b| b.cumulative),
        toggle_of(Rc::clone(&b), "ANIM", |b| b.anim),
    ];

    let b_open = Rc::clone(&b);
    let b_anchor = Rc::clone(&b);
    let b_anchor2 = Rc::clone(&b);
    let b_close = Rc::clone(&b);
    rsx! {
        dropdown {
            open: move || b_open.menu.get(),
            anchor_x: move || b_anchor.anchor.get().0,
            anchor_y: move || b_anchor2.anchor.get().1 + 20.0,
            panel_width: 120.0,
            on_close: move || b_close.menu.set(false),
            surface: move || theme::PANEL,
            border: move || theme::BORDER,
            ..items
        }
    }
    .into_node()
}

/// 一个开关：`get` 指向面板局部状态里的布尔信号，点击即取反。
fn toggle_of(
    b: Rc<BookState>,
    label: &'static str,
    get: impl Fn(&BookState) -> Signal<bool> + 'static,
) -> Slot {
    let get_read = Rc::new(get);
    let get_write = Rc::clone(&get_read);
    let b_read = Rc::clone(&b);
    let b_write = Rc::clone(&b);
    rsx! {
        toggle_chip {
            label: label,
            active: move || get_read(&b_read).get(),
            on_click: move || {
                let sig = get_write(&b_write);
                let on = sig.get();
                sig.set(!on);
            },
        }
    }
}

/// 一侧列表：`for_each` 按档位下标复用行，行高由 `grow` 均分。
///
/// 行是在动态区域里构建的，拿不到本函数的局部变量，所以走 `consume`：
/// `book_panel` 把 `Rc<BookState>` provide 了下来，这里直接取。
fn side_list(b: Rc<BookState>, bid: bool) -> Node {
    let m_emit = Rc::clone(&b);
    let rows = for_each(
        move |emit| {
            let mode = m_emit.mode.get();
            if bid && !mode.shows_bids() {
                return;
            }
            if !bid && !mode.shows_asks() {
                return;
            }
            let view = bookview::view(
                m_emit.model.depth.get().as_ref(),
                ROWS,
                m_emit.step.get(),
                bar_mode(&m_emit),
            );
            let side = if bid { &view.bids } else { &view.asks };
            for (i, row) in side.iter().enumerate() {
                emit((bid, i), *row);
            }
        },
        move |_key, row, cx| {
            // 项内 consume：不必把 BookState 一路捕获进闭包
            let b = cx
                .consume::<Rc<BookState>>()
                .cloned()
                .expect("book_panel 应 provide 了 Rc<BookState>");
            cx.child(book_row(b, row));
        },
    );

    let m_hide = Rc::clone(&b);
    rsx! {
        vbox {
            gap: 0.0,
            padding: 0.0,
            // 行撑满面板宽度（否则行宽为 0，绘制和命中都会塌掉）
            align_items: AlignItem::Stretch,
            grow: 1.0,
            hide: {
                let mode = m_hide.mode.get();
                if bid { !mode.shows_bids() } else { !mode.shows_asks() }
            },
            ..[rows]
        }
    }
    .into_node()
}

/// 一档：数量条 + 价格 + 数量 + 名义金额。
fn book_row(b: Rc<BookState>, data: Rc<RefCell<BookRow>>) -> Node {
    let hover = Signal::new(false);
    let m_click = Rc::clone(&b);
    let m_draw = Rc::clone(&b);
    let row_click = Rc::clone(&data);
    rsx! {
        node {
            height: 0.0,
            grow: 1.0,
            on_hover: move |on| hover.set(on),
            // 点价：拿到本档价格（只在盘口面板内用，不写回共享 Model）
            on_click: move || {
                let p = row_click.borrow().price;
                if p > 0.0 {
                    m_click.pick(p);
                }
            },
            draw: move |s: &mut Scene| {
                let b = current_box();
                if b.height <= 0.0 {
                    return;
                }
                let row = *data.borrow();
                if hover.get() {
                    s.fill_rect(Rect::new(0.0, 0.0, b.width, b.height), theme::ROW_ALT);
                }
                // 深度条：贴右生长
                let frac = (row.percent / 100.0).clamp(0.0, 1.0);
                let bar_w = (b.width - 2.0 * PAD) * frac;
                let soft = if row.bid {
                    theme::UP_SOFT
                } else {
                    theme::DOWN_SOFT
                };
                if bar_w > 0.0 {
                    s.fill_rect(
                        Rect::new(b.width - PAD - bar_w, 1.0, bar_w, (b.height - 2.0).max(1.0)),
                        soft,
                    );
                }
                let y = (b.height - 9.0) / 2.0;
                theme::text(s, PAD, y, &format_price(row.price), 9.5, theme::tone(row.bid));
                theme::text_right(
                    s,
                    PAD + (b.width - 2.0 * PAD) * 0.60,
                    y,
                    &format_qty(row.qty),
                    9.5,
                    theme::MUTED,
                );
                theme::text_right(
                    s,
                    b.width - PAD,
                    y,
                    &bookview::format_total(row.total, !m_draw.rounding.get()),
                    9.5,
                    theme::MUTED,
                );
            },
        }
    }
    .into_node()
}

/// 中间价分隔行：LAST + 点价标记。
fn mid_row(b: Rc<BookState>) -> Node {
    rsx! {
        node {
            height: MID_H,
            draw: move |s: &mut Scene| {
                let bx = current_box();
                s.fill_rect(Rect::new(0.0, 0.0, bx.width, MID_H), theme::ROW_ALT);
                s.fill_rect(
                    Rect::new(PAD, 0.0, (bx.width - 2.0 * PAD).max(0.0), 1.0),
                    theme::BORDER,
                );
                s.fill_rect(
                    Rect::new(PAD, MID_H - 1.0, (bx.width - 2.0 * PAD).max(0.0), 1.0),
                    theme::BORDER,
                );
                // 盘口还没到时不要显示 0.00，占位提示更诚实
                let v = bookview::view(
                    b.model.depth.get().as_ref(),
                    ROWS,
                    b.step.get(),
                    bar_mode(&b),
                );
                if v.is_empty() {
                    theme::text(s, PAD, 7.0, "WAITING FOR BOOK", 9.0, theme::FAINT);
                    return;
                }
                let last = b.model.ticker.get().map(|t| t.last).unwrap_or(0.0);
                let up = b.model.ticker.get().map(|t| t.up()).unwrap_or(true);
                theme::text(s, PAD, 7.0, "LAST", 9.0, theme::FAINT);
                theme::text_right(s, bx.width - PAD, 5.0, &format_price(last), 13.0, theme::tone(up));
                // 点价标记（点过某一档后显示，供下单面板参考）
                if let Some(p) = b.picked_price() {
                    theme::text_center(
                        s,
                        bx.width / 2.0,
                        8.0,
                        &format!("LIMIT {}", format_price(p)),
                        9.0,
                        theme::ACCENT,
                    );
                }
            },
        }
    }
    .into_node()
}

/// 两侧均价与合计（对应 ooc 的 `Display Avg.&Sum`）。
fn avg_sum_row(b: Rc<BookState>) -> Node {
    const H: f32 = 18.0;
    let b_hide = Rc::clone(&b);
    rsx! {
        node {
            height: H,
            hide: !b_hide.avg.get(),
            draw: move |s: &mut Scene| {
                let bx = current_box();
                let v = bookview::view(
                    b.model.depth.get().as_ref(),
                    ROWS,
                    b.step.get(),
                    bar_mode(&b),
                );
                let bids = bookview::side_summary(&v.bids);
                let asks = bookview::side_summary(&v.asks);
                let y = (H - 9.0) / 2.0;
                theme::text(
                    s,
                    PAD,
                    y,
                    &format!(
                        "B {} / {}",
                        format_price(bids.avg),
                        bookview::format_total(bids.sum, true)
                    ),
                    8.5,
                    theme::UP,
                );
                theme::text_right(
                    s,
                    bx.width - PAD,
                    y,
                    &format!(
                        "A {} / {}",
                        format_price(asks.avg),
                        bookview::format_total(asks.sum, true)
                    ),
                    8.5,
                    theme::DOWN,
                );
            },
        }
    }
    .into_node()
}

/// 底部买卖挂单比例条（对应 ooc 的 B/S ratio）。
fn ratio_bar(b: Rc<BookState>) -> Node {
    let b_hide = Rc::clone(&b);
    rsx! {
        node {
            height: RATIO_H,
            hide: !b_hide.ratio.get(),
            draw: move |s: &mut Scene| {
                let bx = current_box();
                let v = bookview::view(
                    b.model.depth.get().as_ref(),
                    ROWS,
                    b.step.get(),
                    bar_mode(&b),
                );
                let bid_w = (bx.width * v.buy_ratio.clamp(0.0, 1.0) as f32).max(0.0);
                let y = 6.0;
                let h = 3.0;
                theme::bar(s, 0.0, y, bid_w, h, theme::UP);
                theme::bar(s, bid_w, y, bx.width - bid_w, h, theme::DOWN);
                theme::text(s, PAD, 10.0, "B", 9.0, theme::UP);
                theme::text(
                    s,
                    PAD + 10.0,
                    10.0,
                    &bookview::format_share(v.buy_ratio),
                    9.0,
                    theme::MUTED,
                );
                theme::text_right(
                    s,
                    bx.width - PAD - 10.0,
                    10.0,
                    &bookview::format_share(1.0 - v.buy_ratio),
                    9.0,
                    theme::MUTED,
                );
                theme::text_right(s, bx.width - PAD, 10.0, "S", 9.0, theme::DOWN);
            },
        }
    }
    .into_node()
}
