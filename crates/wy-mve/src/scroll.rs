//! 滚动容器（复刻 Kotlin `SimpleScrollNode` + `Scroll` + `ScrollContent`）。
//!
//! Kotlin 侧的三个角色在 Rust 里合成一个 `#[component]`：
//! - `Scroll`（滚动状态 + 边界钳制）→ 内部 `offset: Signal<f32>`；
//! - `ScrollContent`（随 offset 反向偏移 + 裁剪）→ 内容包装节点的 `y_fn = -offset`
//!   加上容器 `clip: true`。容器布局方向取滚动方向的反向（纵向滚动 → Row 容器），
//!   纵向偏移因此走**交叉轴**的显式定位——与 Kotlin `scrollDirection.opposite` 同款；
//! - `OverlayScrollBar`（浮动滚动条）→ `not_in_layout` 滑块，不占布局、贴右缘，
//!   几何在布局/绘制期按内容实测高度实时计算，无溢出时自动隐藏。
//!
//! 嵌套滚动：内层滚动量已达边界时不再 `stop_propagation`，滚轮沿命中链冒泡给
//! 外层容器（对应 Kotlin `Scroll.scroll` 的 `parentScroll` 余量放行）。
//!
//! # 视口尺寸
//!
//! `w`/`h` 只是**名义尺寸**（参与父容器 flex 的自然尺寸）：
//! - 显式声明（如 `h: 70.0`）→ 固定视口；
//! - 调用处再给 `grow: 1.0`（组件标签上的布局属性直接作用于根节点）→
//!   名义值作自然尺寸，flex 把剩余空间分给它——**填满父容器剩余高度**。
//!
//! 所有几何（滚动上限、滑块轨道）都按引擎每帧分配的 [`Node::last_box`]
//! 实时计算，不依赖构建期的静态值。

use std::cell::RefCell;
use std::rc::Rc;

use crate::{component, AlignItem, Layout, Node, NodeContext, PointerEvent};
use wy_render::{Color, Rect, Scene};
use wy_signal::{GetValue, SetValue, Signal};

/// 滚轮一格（LineDelta 1.0）对应的滚动像素。
const LINE_STEP: f32 = 40.0;
/// 滑块最短长度（太短难看清也难点中，Kotlin 同款下限 20，这里稍放宽）。
const MIN_THUMB: f32 = 24.0;
/// 滑块宽 / 距右缘留白。
const THUMB_W: f32 = 4.0;
const THUMB_MARGIN: f32 = 8.0;

/// 纵向滚动容器：视口裁剪 + 滚轮滚动 + 边界放行 + 浮动滚动条。
///
/// ```ignore
/// scroll {
///     h: 70.0,
///     gap: 6.0,
///     // 子节点即滚动内容（纵向排布，交叉轴拉伸）
///     row_one()
///     row_two()
/// }
/// // 填满父容器剩余高度：名义 h 作自然尺寸，grow 吃掉剩余空间
/// scroll { h: 70.0, grow: 1.0, list() }
/// ```
#[component]
pub fn scroll(
    /// 名义视口宽度（交叉轴通常被父容器 `Stretch` 覆盖；0 = 全交父容器）。
    #[props(default = 0.0f32)]
    w: f32,
    /// 名义视口高度（主轴自然尺寸；配合 `grow: 1.0` 填满剩余空间）。
    #[props(default = 0.0f32)]
    h: f32,
    /// 内容行间距。
    #[props(default = 0.0f32)]
    gap: f32,
    /// 视口内边距（内容与滚动条都内缩）。
    #[props(default = 0.0f32)]
    padding: f32,
    children: Vec<Slot>,
) -> Node {
    let offset = Signal::new(0.0f32);

    // --- 视口骨架：先建节点，几何闭包通过句柄 `vp` 读每帧实际分配的盒 ---
    let mut viewport = Node {
        layout: Some(Layout::Row),
        width: w,
        height: h,
        padding,
        clip: true,
        ..Node::default()
    };
    let vp = viewport.clone();

    // 实际视口内高/内宽（上一帧 flex 分配值；首帧绘制前是名义值兜底）。
    // Rc 共享：多个几何闭包各自捕获同一份。
    let view_h: Rc<dyn Fn() -> f32> = Rc::new(move || {
        let b = vp.last_box();
        let bh = if b.height > 0.0 { b.height } else { h };
        (bh - padding * 2.0).max(0.0)
    });
    let vp2 = viewport.clone();
    let view_w: Rc<dyn Fn() -> f32> = Rc::new(move || {
        let b = vp2.last_box();
        let bw = if b.width > 0.0 { b.width } else { w };
        (bw - padding * 2.0).max(0.0)
    });

    // --- 内容节点：纵向 vbox，grow 吃满视口主轴（宽），随 offset 反向偏移 ---
    let content_children = Rc::new(RefCell::new(Some(children)));
    let mut content = Node {
        layout: Some(Layout::Column),
        gap,
        align_items: AlignItem::Stretch,
        grow: 1.0,
        arg_children_fn: Rc::new(move |cx: &mut NodeContext| {
            if let Some(c) = content_children.borrow_mut().take() {
                for s in c {
                    cx.child(s);
                }
            }
        }),
        ..Node::default()
    };
    content.y_fn = Some(Rc::new(move || -offset.get()));

    // 滚动上限在事件/绘制期实时测量（内容高度随 for_each 动态变化）
    let meas_wheel = content.clone();
    let view_h_wheel = Rc::clone(&view_h);
    let on_wheel_fn = Rc::new(move |ev: &mut PointerEvent| {
        let max = (crate::node_size(&meas_wheel).1 - view_h_wheel()).max(0.0);
        if max <= 0.0 {
            return; // 无溢出：不消费，放行给外层容器
        }
        let cur = offset.get();
        let next = (cur + ev.wheel * LINE_STEP).clamp(0.0, max);
        if (next - cur).abs() > f32::EPSILON {
            offset.set(next);
            // 消费了才拦截；到边界后剩余滚动冒泡给外层（嵌套滚动放行）
            ev.stop_propagation();
        }
    });
    viewport.on_wheel_fn = Some(on_wheel_fn);

    // --- 浮动滚动条：not_in_layout 贴右缘，几何实时算，无溢出自动隐藏 ---
    let bar = {
        let meas_thumb_h = content.clone();
        let meas_thumb_y = content.clone();
        let meas_hide = content.clone();
        let track: Rc<dyn Fn() -> f32> = {
            let view_h = Rc::clone(&view_h);
            Rc::new(move || (view_h() - THUMB_MARGIN * 2.0).max(0.0))
        };
        let track_h = Rc::clone(&track);
        let track_y = Rc::clone(&track);
        let view_h_bar = Rc::clone(&view_h);
        let view_h_bar2 = Rc::clone(&view_h);
        Node {
            not_in_layout: true,
            x_fn: Some(Rc::new(move || view_w() + padding - THUMB_MARGIN)),
            width: THUMB_W,
            height_fn: Some(Rc::new(move || {
                let content_h = crate::node_size(&meas_thumb_h).1;
                let max = content_h - view_h_bar();
                if max <= 0.0 {
                    track_h()
                } else {
                    (track_h() * view_h_bar() / content_h).max(MIN_THUMB)
                }
            })),
            y_fn: Some(Rc::new(move || {
                let content_h = crate::node_size(&meas_thumb_y).1;
                let max = content_h - view_h_bar2();
                if max <= 0.0 {
                    return padding + THUMB_MARGIN;
                }
                let thumb_h = (track_y() * view_h_bar2() / content_h).max(MIN_THUMB);
                padding + THUMB_MARGIN + (track_y() - thumb_h) * offset.get() / max
            })),
            hide_fn: Some(Rc::new(move || crate::node_size(&meas_hide).1 <= view_h())),
            draw_fn: Rc::new(|any: &mut dyn std::any::Any| {
                let Some(s) = any.downcast_mut::<Scene>() else {
                    return;
                };
                let b = crate::current_box();
                s.fill_round_rect(
                    Rect::new(0.0, 0.0, b.width, b.height),
                    THUMB_W / 2.0,
                    Color::rgba(0, 0, 0, 70),
                );
            }),
            ..Node::default()
        }
    };

    // --- 视口子节点：内容包装 + 滚动条（骨架建好后回填） ---
    viewport.arg_children_fn = Rc::new(move |cx: &mut NodeContext| {
        cx.add_node(content.clone());
        cx.add_node(bar.clone());
    });
    viewport
}
