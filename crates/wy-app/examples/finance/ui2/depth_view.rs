//! 累计深度图面板：缩放、悬停读数、双轴刻度、展开。
//!
//! 对应 ooc `depth-chart.ooc`：滚轮/按钮缩放、悬停 tooltip（价格 + 累计量 +
//! BID/ASK 侧）、x/y 轴刻度、中间价固定、空状态。
//!
//! 「FULL」不叠浮层，而是让根容器切换成"深度图独占视口"——
//! 避免为一个全屏视图引入 z-index 与绝对定位层，布局仍由 flex 决定。
//!
//! 与 v1 的差别：状态通过参数显式传入（不再 `ctx.consume`），按钮文案走
//! `text_label!` 子节点；图表本体仍是 `panels::depth_chart` 的整块自绘
//! （它是数据可视化，不是排版结构，不强拆成节点）。

use std::cell::Cell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use wy_mve::canvas_render2::node2::Ext;
use wy_mve::canvas_render2::protocol::MouseEvent2;
use wy_mve::{
    flex, grow, node2, AlignItem, CursorType, DirectionJustify, Fc, Layout, Node2, Scene,
};
use wy_render::Rect;
use wy_signal::{GetValue, SetValue, Signal};

// `text_label!` 由 `#[macro_use] pub mod widgets;` 提供，**不要**再 `use` 它
// ——那样会和同名 macro_rules 撞成 E0659 ambiguous；宏位置直接用即可。
use super::widgets::{chip_bg, icon_btn, PAD};
use crate::depth::{self, clamp_zoom};
use crate::{panels, theme, Model};

const DOUBLE_CLICK: Duration = Duration::from_millis(320);

/// 深度图面板的局部状态：缩放倍率 + 悬停位置 + 是否独占视口。
///
/// 前两个只有这块绘制区自己用（缩放不进 Model、悬停不进 Model）。
/// 对照 [`crate::ui2::BookState`]：**有第二个读者才提升**。
///
/// `full` 是这里的例外：装配层（`ui2::mod`）要读它来决定切哪棵子树，
/// 所以它从外面传进来——但传的是**一个信号**，不是把整块 Model 抬进来，
/// 也不是把它塞进 `Model` 变成全应用状态。
pub struct DepthState {
    /// 跨面板共享的部分（行情数据）。
    pub model: Rc<Model>,
    /// 是否独占视口（FULL）：由装配层持有并传进来。
    pub full: Signal<bool>,
    /// 缩放倍数（1..=24）。
    pub zoom: Signal<f64>,
    /// 悬停位置（绘图区局部像素）。
    pub hover: Signal<Option<(f32, f32)>>,
}

impl DepthState {
    fn new(model: Rc<Model>, full: Signal<bool>) -> Self {
        Self {
            model,
            full,
            zoom: Signal::new(1.0),
            hover: Signal::new(None),
        }
    }
}

/// 深度图面板。
pub fn depth_panel(model: Rc<Model>, full: Signal<bool>, exts: Vec<Ext>) -> Fc {
    let d = Rc::new(DepthState::new(model, full));

    let actions = vec![
        zoom_btn(Rc::clone(&d), "−", 22.0, |z| z / 1.5),
        zoom_btn(Rc::clone(&d), "1x", 24.0, |_| 1.0),
        zoom_btn(Rc::clone(&d), "+", 22.0, |z| z * 1.5),
        full_btn(Rc::clone(&d)),
    ];

    let m_badge = Rc::clone(&d);
    let badge: Rc<dyn Fn() -> String> = Rc::new(move || {
        let g = geometry(&m_badge);
        if g.has_data {
            format!("±{:.2}%", g.range_pct)
        } else {
            String::from("--")
        }
    });

    panel! {
        debug_name: "panel-depth".to_string(),
        exts: exts,
        title: Rc::new(|| "DEPTH".to_string()),
        badge: badge,
        actions: actions,
        children: vec![ body(Rc::clone(&d)) ],
    }
}

/// 工具栏按钮：按倍率作用于当前缩放。
fn zoom_btn(
    d: Rc<DepthState>,
    label: &'static str,
    w: f32,
    factor: impl Fn(f64) -> f64 + 'static,
) -> Fc {
    icon_btn(label, w, move || zoom_to(&d, factor(d.zoom.get())))
}

/// 深度数据源：优先用深盘口（只喂深度图），否则退回 20 档。
fn geometry(d: &DepthState) -> depth::Geometry {
    let model = &d.model;
    let deep = model.deep.get();
    let shallow = if deep.is_none() {
        model.depth.get()
    } else {
        None
    };
    let src = deep.or(shallow);
    depth::geometry(src.as_ref(), d.zoom.get())
}

fn zoom_to(d: &DepthState, value: f64) {
    d.zoom.set(clamp_zoom(value));
}

/// 「FULL / CLOSE」按钮：切换独占视口。
///
/// 文案随状态变，所以不能复用静态字形的 [`chip_custom`]；这里按同样的几何
/// 与配色重建节点，并把文案交给 `text_label!` 子节点。
fn full_btn(d: Rc<DepthState>) -> Fc {
    const W: f32 = 40.0;
    const H: f32 = 20.0;
    let hover = Signal::new(false);

    // getter 闭包会把捕获到的东西整体搬走，所以要用的信号先克隆出来。
    let f_active = d.full.clone();
    let f_click = d.full.clone();

    let active_active = Rc::new(move || f_active.get()) as Rc<dyn Fn() -> bool>;
    // 四个 getter / 回调各自一份，避免闭包互相抢。
    let active_draw = Rc::clone(&active_active);
    let active_text = Rc::clone(&active_active);
    let active_color = Rc::clone(&active_active);
    let hover_draw = hover.clone();

    node2! {
        debug_name: "depth-full".to_string(),
        width: W,
        height: H,
        cursor: CursorType::Pointer,
        on_hover: move |on| hover.set(on),
        mouse_click: move |_e: &mut MouseEvent2| {
            let v = f_click.get();
            f_click.set(!v);
        },
        // 定高 chip：`align_fix` 让文案以自身高为居中基准（同 `widgets::chip_custom`）。
        layout: flex! { direction: Layout::Row, align_item: AlignItem::Center, direction_justify: DirectionJustify::Center, align_fix: true },
        draw: move |s: &mut Scene, n: &Node2| {
            let on = active_draw();
            let (bg, edge) = chip_bg(hover_draw.get(), on);
            s.fill_round_rect(Rect::new(0.0, 0.0, n.width(), H), 4.0, bg);
            s.stroke_round_rect(Rect::new(0.5, 0.5, n.width() - 1.0, H - 1.0), 4.0, edge, 1.0);
        },
        children: vec![ text_label! {
            content: if active_text() { "CLOSE".to_string() } else { "FULL".to_string() },
            font_size: 9.0,
            color: if active_color() { theme::TEXT } else { theme::MUTED },
        } ],
    }
}

/// 绘图区节点：交互入口。
fn body(d: Rc<DepthState>) -> Fc {
    let last_press: Rc<Cell<Option<Instant>>> = Rc::new(Cell::new(None));

    let m_wheel = Rc::clone(&d);
    let m_move = Rc::clone(&d);
    let m_leave = Rc::clone(&d);
    let m_down = Rc::clone(&d);
    let m_draw = Rc::clone(&d);
    let press = Rc::clone(&last_press);

    let _ = PAD;

    node2! {
        exts: vec![grow(1.0)],

        // 向上滚（负）放大、向下滚缩小；吃掉事件避免外层跟着动
        mouse_wheel: move |e: &mut MouseEvent2| {
            let factor = (-(e.wheel_delta_y as f64) * 0.22).exp();
            zoom_to(&m_wheel, m_wheel.zoom.get() * factor);
            e.stop_propagation();
        },
        mouse_move: move |e: &mut MouseEvent2| {
            m_move.hover.set(Some((e.x, e.y)));
        },
        on_hover: move |entered| {
            if !entered {
                m_leave.hover.set(None);
            }
        },
        // 双击复位缩放：自己计时（320ms），不依赖引擎的双击判定
        mouse_down: move |_e: &mut MouseEvent2| {
            let now = Instant::now();
            if let Some(prev) = press.get() {
                if now.duration_since(prev) < DOUBLE_CLICK {
                    zoom_to(&m_down, 1.0);
                    press.set(None);
                    return;
                }
            }
            press.set(Some(now));
        },

        draw: move |s: &mut Scene, n: &Node2| {
            let b = Rect::new(0.0, 0.0, n.width(), n.height());
            let g = geometry(&m_draw);
            let plot = panels::depth_plot(b);
            // 对称区读数对：左右两条读数线关于中间价对称（ooc depthHoverPair）
            let hov = match (m_draw.hover.get(), g.has_data) {
                (Some((x, _)), true) => {
                    depth::hover_pair(&g, (x - plot.x) / plot.width.max(1.0))
                }
                _ => None,
            };
            panels::depth_chart(s, b, &g, hov.as_ref());
        },
    }
}
