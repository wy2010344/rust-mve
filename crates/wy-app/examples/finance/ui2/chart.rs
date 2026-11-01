//! K 线面板：视口交互（缩放/平移/十字线）+ 工具栏。
//!
//! 对应 ooc `chart.ooc` + `chart-view.ts`：滚轮以光标为锚点缩放、
//! 按住拖拽平移、双击回到最新、悬停出 OHLC 与十字线。
//!
//! 交互全部挂在**一个叶子节点**上（本文件的 `body`），它不接管位置——
//! 位置尺寸由面板的 flex 分配，指针坐标由引擎换算成节点局部坐标后给过来。
//!
//! 与 v1 的差别只有原语：状态显式传参（不再 `ctx.consume`）、按钮文案走
//! `text_label!` 子节点；图表本体仍是 `panels::chart` 的整块自绘（它是数据
//! 可视化，不是排版结构，不强拆成节点）。

use std::cell::Cell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use wy_mve::canvas_render2::node2::Ext;
use wy_mve::canvas_render2::protocol::MouseEvent2;
use wy_mve::{grow, node2, Fc, Node2, Ref2, Scene};
use wy_render::Rect;
use wy_signal::{GetValue, SetValue, Signal};

// `panel!` 由 `#[macro_use] pub mod widgets;` 提供，不要再 `use` 同名宏。
use super::widgets::{chip_custom, icon_btn};
use crate::chartview::{self, DEFAULT_VISIBLE};
use crate::{panels, theme, Model};

/// 双击回到最新的判定窗口。
const DOUBLE_CLICK: Duration = Duration::from_millis(320);
/// 按钮缩放倍率。
const STEP_ZOOM: f64 = 1.25;

/// K 线面板的局部状态：视口 + 十字线。
///
/// **视口带 key**。存的是 `(key, visible, offset)`，`key` 是当前「标的 + 周期」；
/// 读的时候先比 key，对不上就当没设过、直接给默认视口。于是
/// 「换标的 / 换周期自动回到最新」不需要任何人在外面喊一声重置——
/// 状态自己就失效了。
///
/// 对比把 `offset` 提升进 `Model` 的老写法：那条路每多一个"应该重置"的入口
/// （换数据源、刷新、切周期…）就要在控制器里补一行 `offset.set(0)`，
/// 漏一处就留一个"停在上一只票的偏移上"的 bug。
///
/// 十字线（`hover`）同理：只有这块绘制区读写它，换数据源也不该重置它。
pub struct ChartState {
    /// 跨面板共享的行情数据（由 `dashboard` 传入）。
    pub model: Rc<Model>,
    /// `(key, 可见根数, 距最新一根的偏移)`。
    viewport: Signal<(String, i64, i64)>,
    /// 十字线位置（绘图区局部像素）。
    hover: Signal<Option<(f32, f32)>>,
}

impl ChartState {
    fn new(model: Rc<Model>) -> Self {
        Self {
            model,
            viewport: Signal::new((String::new(), DEFAULT_VISIBLE, 0)),
            hover: Signal::new(None),
        }
    }

    /// 视口归属的 key：标的 + 周期。
    fn key(&self) -> String {
        format!("{}|{}", self.model.symbol.get(), self.model.interval.get())
    }

    /// 当前视口：key 对不上就退回默认（贴右 + 默认根数）。
    fn view(&self) -> (i64, i64) {
        let (k, visible, offset) = self.viewport.get();
        if k == self.key() {
            (visible, offset)
        } else {
            (DEFAULT_VISIBLE, 0)
        }
    }

    fn set_view(&self, visible: i64, offset: i64) {
        self.viewport.set((self.key(), visible, offset));
    }

    /// 回到最新：默认根数 + 贴右。
    fn reset(&self) {
        self.set_view(DEFAULT_VISIBLE, 0);
    }
}

/// K 线面板。
pub fn chart_panel(model: Rc<Model>, exts: Vec<Ext>) -> Fc {
    let s = Rc::new(ChartState::new(model));

    let actions = vec![
        zoom_btn(Rc::clone(&s), "−", 22.0, |st| zoom_step(st, STEP_ZOOM)),
        zoom_btn(Rc::clone(&s), "1x", 24.0, |st| st.reset()),
        zoom_btn(Rc::clone(&s), "+", 22.0, |st| {
            zoom_step(st, 1.0 / STEP_ZOOM)
        }),
        latest_btn(Rc::clone(&s)),
    ];

    // `title` / `badge` 是面板的非 getter 参数（`Rc<dyn Fn() -> String>`），
    // 各自是一枚独立闭包 → 一处一份句柄。
    let s_title = Rc::clone(&s);
    let s_badge = Rc::clone(&s);

    panel! {
        debug_name: "panel-chart".to_string(),
        exts: exts,
        title: Rc::new(move || {
            format!(
                "{} · {}",
                s_title.model.symbol.get(),
                s_title.model.interval.get().to_uppercase()
            )
        }),
        badge: Rc::new(move || format!("{} bars", s_badge.view().0)),
        actions: actions,
        children: vec![ body(s) ],
    }
}

/// 工具栏按钮：把「点一下对视口做什么」压成一行。
fn zoom_btn(
    s: Rc<ChartState>,
    label: &'static str,
    w: f32,
    act: impl Fn(&ChartState) + 'static,
) -> Fc {
    icon_btn(label, w, move || act(&s))
}

/// 以中线为锚点按倍率缩放。
fn zoom_step(s: &ChartState, factor: f64) {
    let total = s.model.klines.get().len();
    let (visible, offset) = s.view();
    let (count, next) = chartview::zoom_frame(total, visible, offset, factor, 0.5);
    s.set_view(count, next);
}

/// 「最新」按钮：往右翻过之后才出现。
fn latest_btn(s: Rc<ChartState>) -> Fc {
    let s_click = Rc::clone(&s);
    let s_hide = Rc::clone(&s);
    let hidden = Rc::new(move || s_hide.view().1 == 0) as Rc<dyn Fn() -> bool>;

    chip_custom(
        46.0,
        theme::MUTED,
        Rc::new(|| false),
        Rc::new(move || {
            let (visible, _) = s_click.view();
            s_click.set_view(visible, 0);
        }),
        hidden,
        "LATEST",
    )
}

/// 绘图区节点：唯一的交互入口。
///
/// 事件里要按**自己的实测尺寸**算绘图区（滚轮锚点、拖拽换算），而事件回调
/// 拿不到 `&Node2`——所以给节点留一个 `Ref2` 句柄，回调里 `get()` 读几何。
fn body(s: Rc<ChartState>) -> Fc {
    let drag: Rc<Cell<Option<(f32, i64)>>> = Rc::new(Cell::new(None));
    let last_press: Rc<Cell<Option<Instant>>> = Rc::new(Cell::new(None));
    let node_ref = Ref2::<Node2>::new();

    // 每个事件回调都是独立的 `move` 闭包：一处一份句柄。
    let wheel_state = Rc::clone(&s);
    let wheel_box = node_ref.clone();
    let down_state = Rc::clone(&s);
    let down_press = Rc::clone(&last_press);
    let down_drag = Rc::clone(&drag);
    let move_state = Rc::clone(&s);
    let move_drag = Rc::clone(&drag);
    let move_box = node_ref.clone();
    let leave_state = Rc::clone(&s);
    let up_drag = Rc::clone(&drag);
    let draw_state = Rc::clone(&s);

    node2! {
        node_ref: node_ref,
        exts: vec![grow(1.0)],
        debug_name: "chart-body".to_string(),

        // 滚轮：以光标为锚点缩放。吃掉事件，不然会继续冒泡给外层。
        mouse_wheel: move |e: &mut MouseEvent2| {
            let me = wheel_box.get();
            let plot = panels::chart_plot(Rect::new(0.0, 0.0, me.width(), me.height()));
            let anchor = if plot.width > 0.0 {
                ((e.x - plot.x) / plot.width).clamp(0.0, 1.0) as f64
            } else {
                0.5
            };
            let (visible, offset) = wheel_state.view();
            let (count, next) = chartview::zoom_by_wheel(
                wheel_state.model.klines.get().len(),
                visible,
                offset,
                e.wheel_delta_y,
                anchor,
            );
            wheel_state.set_view(count, next);
            e.stop_propagation();
        },

        // 移动：先记十字线位置（每组坐标都要），再按需平移。
        // v1 把「悬停」与「拖拽」拆成两个回调，v2 的 `mouse_move` 一名一职，
        // 于是合并成一处、用 `drag` 是否为空来分流——语义等价。
        mouse_move: move |e: &mut MouseEvent2| {
            move_state.hover.set(Some((e.x, e.y)));
            let Some((start_x, start_offset)) = move_drag.get() else {
                return;
            };
            let me = move_box.get();
            let plot = panels::chart_plot(Rect::new(0.0, 0.0, me.width(), me.height()));
            let (visible, _) = move_state.view();
            let (count, offset) = chartview::pan_by_pixels(
                move_state.model.klines.get().len(),
                visible,
                start_offset,
                e.global_x - start_x,
                plot.width,
            );
            move_state.set_view(count, offset);
        },
        on_hover: move |entered: bool| {
            if !entered {
                leave_state.hover.set(None);
            }
        },

        // 按下：开始拖拽 / 双击回最新
        mouse_down: move |e: &mut MouseEvent2| {
            let now = Instant::now();
            if let Some(prev) = down_press.get() {
                if now.duration_since(prev) < DOUBLE_CLICK {
                    down_state.reset();
                    down_press.set(None);
                    down_drag.set(None);
                    return;
                }
            }
            down_press.set(Some(now));
            down_drag.set(Some((e.global_x, down_state.view().1)));
        },
        mouse_up: move |_e: &mut MouseEvent2| up_drag.set(None),

        draw: move |scene: &mut Scene, n: &Node2| {
            let b = Rect::new(0.0, 0.0, n.width(), n.height());
            let plot = panels::chart_plot(b);
            let data = draw_state.model.klines.get();
            let (visible, offset) = draw_state.view();
            let l = chartview::layout(&data, visible, offset, plot.width, plot.height);
            let hov = match (draw_state.hover.get(), l.has_data) {
                (Some((x, y)), true) => chartview::hover(
                    &l,
                    (x - plot.x) / plot.width.max(1.0),
                    (y - plot.y) / plot.height.max(1.0),
                ),
                _ => None,
            };
            panels::chart(scene, b, &l, hov.as_ref());
        },
    }
}
