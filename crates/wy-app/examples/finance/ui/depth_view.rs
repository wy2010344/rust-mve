//! 累计深度图面板：缩放、悬停读数、双轴刻度、展开。
//!
//! 对应 ooc `depth-chart.ooc`：滚轮/按钮缩放、悬停 tooltip（价格 + 累计量 +
//! BID/ASK 侧）、x/y 轴刻度、中间价固定、空状态。
//!
//! 「FULL」不叠浮层，而是让根容器切换成"深度图独占视口"——
//! 避免为一个全屏视图引入 z-index 与绝对定位层，布局仍由 flex 决定。

use std::cell::Cell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use wy_mve::{component, current_box, rsx, AlignItem, Node, Slot};
use wy_render::{Rect, Scene};
use wy_signal::{GetValue, SetValue, Signal};

use super::widgets::{chip_custom, icon_btn, panel_with};
use crate::depth::{self, clamp_zoom};
use crate::{panels, theme, Model};

const DOUBLE_CLICK: Duration = Duration::from_millis(320);

/// 深度图面板的局部状态：缩放倍率 + 悬停位置 + 是否独占视口。
///
/// 前两个只有这块绘制区自己用（缩放不进 Model、悬停不进 Model）。
/// 对照 [`crate::ui::book::BookState`]：**有第二个读者才提升**。
///
/// `full` 是这里的例外：装配层（`ui::mod`）要读它来决定切哪棵子树，
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

#[component]
pub fn depth_panel(ctx: &mut NodeContext, full: Signal<bool>) {
    let model = ctx
        .consume::<Rc<Model>>()
        .cloned()
        .expect("dashboard 应 provide 了 Rc<Model>");
    let d = Rc::new(DepthState::new(model, full));

    let actions = vec![
        zoom_btn(Rc::clone(&d), "−", 22.0, |z| z / 1.5),
        zoom_btn(Rc::clone(&d), "1x", 24.0, |_| 1.0),
        zoom_btn(Rc::clone(&d), "+", 22.0, |z| z * 1.5),
        Slot::One(full_btn(Rc::clone(&d))),
    ];

    let m_badge = Rc::clone(&d);
    let badge = Rc::new(move || {
        let g = geometry(&m_badge);
        if g.has_data {
            format!("±{:.2}%", g.range_pct)
        } else {
            String::from("--")
        }
    });

    ctx.child(panel_with(
        Rc::new(|| String::from("DEPTH")),
        badge,
        actions,
        vec![Slot::One(body(Rc::clone(&d)))],
    ));
}

/// 工具栏按钮：按倍率作用于当前缩放。
fn zoom_btn(
    d: Rc<DepthState>,
    label: &'static str,
    w: f32,
    factor: impl Fn(f64) -> f64 + 'static,
) -> Slot {
    rsx! {
        icon_btn {
            label: label,
            w: w,
            on_click: move || zoom_to(&d, factor(d.zoom.get())),
        }
    }
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
/// 文案随状态变，但外观仍走 [`chip_custom`]——展开时和其它选中态的
/// 小按钮一致（描边高亮），不会冒出一个"独立风格"的按钮。
fn full_btn(d: Rc<DepthState>) -> Node {
    const W: f32 = 40.0;
    let m_active = Rc::clone(&d);
    let m_click = Rc::clone(&d);
    let m_label = Rc::clone(&d);
    chip_custom(
        W,
        theme::MUTED,
        // `full` 由装配层持有：这里只读写同一个信号，不进 Model
        Rc::new(move || m_active.full.get()),
        Rc::new(move || {
            let v = m_click.full.get();
            m_click.full.set(!v);
        }),
        Rc::new(move |s, w, h, color| {
            let open = m_label.full.get();
            theme::text_center(
                s,
                w / 2.0,
                (h - 9.0) / 2.0,
                if open { "CLOSE" } else { "FULL" },
                9.0,
                color,
            );
        }),
    )
}

/// 绘图区节点：交互入口。
fn body(d: Rc<DepthState>) -> Node {
    let last_press: Rc<Cell<Option<Instant>>> = Rc::new(Cell::new(None));

    let m_wheel = Rc::clone(&d);
    let m_move = Rc::clone(&d);
    let m_leave = Rc::clone(&d);
    let m_down = Rc::clone(&d);
    let press = Rc::clone(&last_press);

    rsx! {
        node {
            grow: 1.0,
            align_self: AlignItem::Stretch,

            // 向上滚（负）放大、向下滚缩小；吃掉事件避免外层跟着动
            on_wheel_ev: move |ev: &mut wy_mve::PointerEvent| {
                let factor = (-(ev.wheel as f64) * 0.22).exp();
                zoom_to(&m_wheel, m_wheel.zoom.get() * factor);
                ev.stop_propagation();
            },
            on_move: move |x, y| m_move.hover.set(Some((x, y))),
            on_hover: move |entered| {
                if !entered {
                    m_leave.hover.set(None);
                }
            },
            on_down: move |_ev| {
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

            draw: move |s: &mut Scene| {
                let b = current_box();
                let body = Rect::new(0.0, 0.0, b.width, b.height);
                let g = geometry(&d);
                let plot = panels::depth_plot(body);
                // 对称区读数对：左右两条读数线关于中间价对称（ooc depthHoverPair）
                let hov = match (d.hover.get(), g.has_data) {
                    (Some((x, _)), true) => {
                        depth::hover_pair(&g, (x - plot.x) / plot.width.max(1.0))
                    }
                    _ => None,
                };
                panels::depth_chart(s, body, &g, hov.as_ref());
            },
        }
    }
    .into_node()
}
