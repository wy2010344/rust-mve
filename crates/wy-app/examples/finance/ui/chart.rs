//! K 线面板：视口交互（缩放/平移/十字线）+ 工具栏。
//!
//! 对应 ooc `chart.ooc` + `chart-view.ts`：滚轮以光标为锚点缩放、
//! 按住拖拽平移、双击回到最新、悬停出 OHLC 与十字线。
//!
//! 交互全部挂在**一个叶子节点**上（本文件的 `body`），它不接管位置——
//! 位置尺寸由面板的 flex 分配，指针坐标由引擎换算成节点局部坐标后给过来。

use std::cell::Cell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use wy_mve::{component, current_box, rsx, AlignItem, Node, Slot};
use wy_render::{Rect, Scene};
use wy_signal::{GetValue, SetValue, Signal};

use super::widgets::{icon_btn, panel_with};
use crate::chartview::{self, DEFAULT_VISIBLE};
use crate::{panels, Model};

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
    /// 跨面板共享的部分（行情数据、当前标的）。
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

#[component]
pub fn chart_panel(ctx: &mut NodeContext) {
    // `model` 由 `dashboard` provide：面板只声明"我要行情"，不接 prop
    let model = ctx
        .consume::<Rc<Model>>()
        .cloned()
        .expect("dashboard 应 provide 了 Rc<Model>");
    let s = Rc::new(ChartState::new(model));

    let actions = vec![
        zoom_btn(Rc::clone(&s), "−", 22.0, |s| zoom_step(s, STEP_ZOOM)),
        zoom_btn(Rc::clone(&s), "1x", 24.0, |s| s.reset()),
        zoom_btn(Rc::clone(&s), "+", 22.0, |s| zoom_step(s, 1.0 / STEP_ZOOM)),
        latest_btn(Rc::clone(&s)),
    ];

    let s_title = Rc::clone(&s);
    let s_badge = Rc::clone(&s);
    ctx.child(panel_with(
        Rc::new(move || {
            format!(
                "{} · {}",
                s_title.model.symbol.get(),
                s_title.model.interval.get().to_uppercase()
            )
        }),
        Rc::new(move || format!("{} 根", s_badge.view().0)),
        actions,
        vec![Slot::One(body(s))],
    ));
}

/// 工具栏按钮：把「点一下对视口做什么」压成一行。
fn zoom_btn(
    s: Rc<ChartState>,
    label: &'static str,
    w: f32,
    act: impl Fn(&ChartState) + 'static,
) -> Slot {
    rsx! {
        icon_btn {
            label: label,
            w: w,
            on_click: move || act(&s),
        }
    }
}

/// 以中线为锚点按倍率缩放。
fn zoom_step(s: &ChartState, factor: f64) {
    let total = s.model.klines.get().len();
    let (visible, offset) = s.view();
    let (count, next) = chartview::zoom_frame(total, visible, offset, factor, 0.5);
    s.set_view(count, next);
}

/// 「最新」按钮：往右翻过之后才出现。
fn latest_btn(s: Rc<ChartState>) -> Slot {
    let s_click = Rc::clone(&s);
    let s_hide = Rc::clone(&s);
    rsx! {
        icon_btn {
            label: "LATEST",
            w: 46.0,
            on_click: move || {
                let (visible, _) = s_click.view();
                s_click.set_view(visible, 0);
            },
            hide: s_hide.view().1 == 0,
        }
    }
}

/// 绘图区节点：唯一的交互入口。
fn body(s: Rc<ChartState>) -> Node {
    let drag: Rc<Cell<Option<(f32, i64)>>> = Rc::new(Cell::new(None));
    let last_press: Rc<Cell<Option<Instant>>> = Rc::new(Cell::new(None));

    let s_wheel = Rc::clone(&s);
    let s_down = Rc::clone(&s);
    let s_pan = Rc::clone(&s);
    let s_move = Rc::clone(&s);
    let s_leave = Rc::clone(&s);

    let drag_down = Rc::clone(&drag);
    let drag_pan = Rc::clone(&drag);

    rsx! {
        node {
            grow: 1.0,
            align_self: AlignItem::Stretch,

            // 滚轮：以光标为锚点缩放。用 `on_wheel_ev` 是因为要吃掉事件，
            // 不然滚轮会继续冒泡给外层。
            on_wheel_ev: move |ev: &mut wy_mve::PointerEvent| {
                let b = current_box();
                let plot = panels::chart_plot(Rect::new(0.0, 0.0, b.width, b.height));
                let anchor = if plot.width > 0.0 {
                    ((ev.x - plot.x) / plot.width).clamp(0.0, 1.0) as f64
                } else {
                    0.5
                };
                let (visible, offset) = s_wheel.view();
                let (count, next) = chartview::zoom_by_wheel(
                    s_wheel.model.klines.get().len(),
                    visible,
                    offset,
                    ev.wheel,
                    anchor,
                );
                s_wheel.set_view(count, next);
                ev.stop_propagation();
            },

            // 悬停移动：记录局部坐标（十字线用）
            on_move: move |x, y| s_move.hover.set(Some((x, y))),
            on_hover: move |entered| {
                if !entered {
                    s_leave.hover.set(None);
                }
            },

            // 按下：开始拖拽 / 双击回最新
            on_down: move |ev| {
                let now = Instant::now();
                if let Some(prev) = last_press.get() {
                    if now.duration_since(prev) < DOUBLE_CLICK {
                        s_down.reset();
                        last_press.set(None);
                        drag_down.set(None);
                        return;
                    }
                }
                last_press.set(Some(now));
                drag_down.set(Some((ev.root_x, s_down.view().1)));
            },

            // 拖拽：像素位移换算成根数
            on_drag_move: move |ev| {
                let Some((start_x, start_offset)) = drag_pan.get() else {
                    return;
                };
                let b = current_box();
                let plot = panels::chart_plot(Rect::new(0.0, 0.0, b.width, b.height));
                let (visible, _) = s_pan.view();
                let (count, offset) = chartview::pan_by_pixels(
                    s_pan.model.klines.get().len(),
                    visible,
                    start_offset,
                    ev.root_x - start_x,
                    plot.width,
                );
                s_pan.set_view(count, offset);
            },
            on_up: move |_ev| drag.set(None),

            draw: move |scene: &mut Scene| {
                let b = current_box();
                let body = Rect::new(0.0, 0.0, b.width, b.height);
                let plot = panels::chart_plot(body);
                let data = s.model.klines.get();
                let (visible, offset) = s.view();
                let l = chartview::layout(&data, visible, offset, plot.width, plot.height);
                let hov = match (s.hover.get(), l.has_data) {
                    (Some((x, y)), true) => chartview::hover(
                        &l,
                        (x - plot.x) / plot.width.max(1.0),
                        (y - plot.y) / plot.height.max(1.0),
                    ),
                    _ => None,
                };
                panels::chart(scene, body, &l, hov.as_ref());
            },
        }
    }
    .into_node()
}
