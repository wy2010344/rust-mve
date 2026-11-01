//! 浮层滚动条（对标 TS `canvasRender2/OverlayScrollBar.ts`）。
//!
//! 与 [`scroll_bar`](crate::canvas_render2::scroll_bar)（传统占位式）的区别，
//! 逐条对齐 TS 注释：
//!
//! - **不占布局空间**：两轴尺寸恒为 0，浮在内容之上绘制，内容区拿满可用宽度；
//!   传统滚动条要吃掉 10px 布局宽度，右侧内容被挤压；
//! - **自动隐藏**：静止时完全透明，滚动时淡入，停手 `idle_delay_ms` 后淡出；
//!   指针移到贴边感应带时也浮现（悬停态）；
//! - **无轨道**：只有一枚半透明胶囊；
//! - **静止时不拦指针**：`accept_hit` 只在可见时成立，静止时点击穿透到内容；
//! - **拖拽 / 点击翻页**：拖滑块按 `max_scroll / max_offset` 换算，点空白整页滚。
//!
//! # 悬停判定为什么走全局指针
//!
//! 静止时本节点根本不在命中链上（`accept_hit` 为假），靠命中链**永远**唤不
//! 出来。所以悬停用 [`register_move`] 读全局指针位置——与 TS 消费
//! `globalMouse2Context` 同一条路。
//!
//! # 与 TS 的两处结构差异（都是 Rust 侧不得不做的形状调整）
//!
//! 1. **动画**：TS 用 `requestAnimationFrame` 逐帧写信号，Rust 侧没有帧回调，
//!    改用 [`Fade`] 的"读时按墙钟求值" + [`FrameTick`] 向宿主登记唤醒
//!    （见 [`crate::canvas_render2::animation`] 模块注释）；
//! 2. **注销**：TS 在 `ctx.addDestroy` 里摘监听器，Rust 的组件没有析构钩子，
//!    所以全局回调只捕获 [`Node2Weak`]：节点消失后第一次被调用就地注销自己
//!    （见 `Node2Weak` 的文档）。

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use wy_render::{Color, Rect, Scene};

use crate::canvas_render2::animation::Fade;
use crate::canvas_render2::fc::{Fc, Ref2};
use crate::canvas_render2::node2::{always, Axis, Get, LayoutSize, Node2, Node2Weak, StartEnd};
use crate::canvas_render2::protocol::MouseEvent2;
use crate::canvas_render2::renderer::{register_move, register_up};
use crate::canvas_render2::scroll::{content_size, max_scroll, scroll_wheel, Scroll2};
use crate::{rec, Node2Props};

/// 浮层滚动条观感（对标 TS `ScrollBarStyle2`）。
///
/// 默认值即 macOS 系统浮层滚动条那组数值：无轨道、圆角胶囊、静止半透明、
/// 悬停加粗、停止滚动后自动淡出。
#[derive(Clone, Copy, Debug)]
pub struct ScrollBarStyle2 {
    /// 静止时滑块粗细。
    pub width: f32,
    /// 悬停 / 拖拽时的滑块粗细。
    pub width_hover: f32,
    /// 贴边可交互感应带宽度（比滑块宽，便于命中）。
    pub hit_width: f32,
    /// 滑块与容器边缘的留白。
    pub edge_gap: f32,
    /// 轨道两端留白，滑块不会顶到容器圆角上。
    pub track_inset: f32,
    /// 滑块最小长度（避免内容极长时滑块细成一条线）。
    pub min_thumb: f32,
    /// 滑块基色（alpha 由下面几档单独控制，故这里不带 alpha）。
    pub thumb: Color,
    /// 静止态不透明度。
    pub idle_alpha: f32,
    /// 悬停态不透明度。
    pub hover_alpha: f32,
    /// 拖拽态不透明度。
    pub drag_alpha: f32,
    /// 停止滚动后多久开始淡出（毫秒）。
    pub idle_delay_ms: f32,
    /// 淡入时长（毫秒）。
    pub fade_in_ms: f32,
    /// 淡出时长（毫秒）。
    pub fade_out_ms: f32,
}

impl ScrollBarStyle2 {
    /// 默认样式（TS `overlayScrollBarStyle`，即 macOS 观感那一组数值）。
    #[must_use]
    pub fn macos() -> Self {
        Self {
            width: 6.0,
            width_hover: 10.0,
            hit_width: 12.0,
            edge_gap: 3.0,
            track_inset: 4.0,
            min_thumb: 28.0,
            // TS `rgba(15, 23, 42)`：alpha 由三档另行给出
            thumb: Color::rgb(15, 23, 42),
            idle_alpha: 0.3,
            hover_alpha: 0.45,
            drag_alpha: 0.62,
            idle_delay_ms: 850.0,
            fade_in_ms: 110.0,
            fade_out_ms: 320.0,
        }
    }
}

impl Default for ScrollBarStyle2 {
    fn default() -> Self {
        Self::macos()
    }
}

/// 全局监听的注销句柄槽：句柄存在这里，回调只持它的**弱引用**——
/// 闭包自己被注销函数间接持有就会成环，用槽破掉。
type UnregisterSlot = Rc<RefCell<Option<Rc<dyn Fn()>>>>;

/// 滑块几何（TS `ThumbGeo2` / Kotlin `OverlayScrollBar.ThumbGeo`）。
#[derive(Clone, Copy, Debug)]
struct ThumbGeo {
    /// 沿滚动方向的偏移（`track_inset` 之外的部分）。
    offset: f32,
    /// 沿滚动方向的长度。
    length: f32,
    /// 最大偏移（轨道长 − 滑块长）。
    max_offset: f32,
    /// 最大滚动距离。
    max_scroll: f32,
}

/// 指针是否落在贴边感应带内（入参是画布坐标，TS `inBand`）。
fn in_band(node: &Node2, d: Axis, st: &ScrollBarStyle2, px: f32, py: f32) -> bool {
    let Some(c) = node.parent() else {
        return false;
    };
    let len = c.inner_size(d);
    if len <= 0.0 {
        return false;
    }
    let (bx, by) = (node.abs_x(), node.abs_y());
    if d == Axis::Y {
        px >= bx && px <= bx + st.hit_width && py >= by && py <= by + len
    } else {
        py >= by && py <= by + st.hit_width && px >= bx && px <= bx + len
    }
}

/// macOS 风格浮层滚动条（对标 TS `overlayScrollBar`）。
///
/// 用法（容器内，与 [`scroll_content`](crate::canvas_render2::scroll_content) 并列）：
/// ```ignore
/// let scroll = create_scroll(&container, Axis::Y);
/// rect_node! {
///     children: vec![
///         scroll_content! { scroll: scroll.clone(), exts: vec![grow(1.0)] },
///         overlay_scroll_bar! { scroll: scroll.clone() },
///     ],
/// }
/// ```
///
/// 几何全部由容器的 `inner_size / content_size / max_scroll` 推导，没有写死的
/// 像素位置——换尺寸、换方向都不用改这里。
#[rec(extends = node2)]
#[must_use]
pub fn overlay_scroll_bar(
    /// 滚动状态（必填）。
    scroll: Scroll2,
    /// 观感参数（默认 [`ScrollBarStyle2::macos`]）。
    style: Option<Get<ScrollBarStyle2>>,
    /// 节点句柄槽（对齐 TS `arg.nodeRef`）。
    #[props(default = Ref2::new())]
    node_ref: Ref2<Node2>,
) -> Fc {
    let style = style.unwrap_or_else(|| always(ScrollBarStyle2::macos()));
    let d = scroll.direction();
    // TS 每次读 `p.style().fadeXxx`；这里三段时长只在构造期取一次——它们不是
    // 主题色，切主题不需要变（TS 自己也只在 reveal / scheduleHide 里读）。
    let s0 = style();
    let fade = Rc::new(Fade::new(
        f64::from(s0.fade_in_ms),
        f64::from(s0.idle_delay_ms),
        f64::from(s0.fade_out_ms),
    ));

    // ---------- 可见性三态 ----------
    // TS 用信号存这三态（绘制期读取）。Rust 的绘制本来就是"输入事件驱动"，
    // 悬停/拖拽用共享 `Cell` 足够；唯一需要推帧的是淡出动画（见 `Fade`）。
    let hovering = Rc::new(Cell::new(false));
    let dragging = Rc::new(Cell::new(false));

    // 当前透明度：拖拽 / 悬停时恒为 1（与 Kotlin `currentAlpha` 一致，
    // fade 只管"滚动引起的浮现"）。
    let alpha_now = {
        let fade = Rc::clone(&fade);
        let hovering = Rc::clone(&hovering);
        let dragging = Rc::clone(&dragging);
        Rc::new(move || {
            if dragging.get() || hovering.get() {
                1.0
            } else {
                fade.value()
            }
        })
    };

    // ---------- 全局悬停监听：只持弱引用 ----------
    // 槽在第一次 `draw` 里回填（那时节点已存在）。全局回调**不能**捕获
    // `node_ref`：那会让节点永不销毁、全局注册表只增不减。
    let node_slot: Rc<RefCell<Option<Node2Weak>>> = Rc::new(RefCell::new(None));
    let unregister: UnregisterSlot = Rc::new(RefCell::new(None));
    {
        let slot = Rc::clone(&node_slot);
        let fade = Rc::clone(&fade);
        let style = Rc::clone(&style);
        let hovering = Rc::clone(&hovering);
        let unreg = Rc::downgrade(&unregister);
        let handle = register_move(Rc::new(move |px: f32, py: f32| {
            let Some(node) = slot.borrow().as_ref().and_then(Node2Weak::upgrade) else {
                // 节点已销毁：就地摘掉全局监听（等价于 TS 的 `ctx.addDestroy`）
                if let Some(s) = unreg.upgrade() {
                    if let Some(off) = s.borrow_mut().take() {
                        off();
                    }
                }
                return;
            };
            let v = in_band(&node, d, &style(), px, py);
            if v == hovering.get() {
                return;
            }
            hovering.set(v);
            if v {
                fade.trigger();
            }
        }));
        *unregister.borrow_mut() = Some(Rc::new(handle));
    }

    // 滚动活动（滚轮 / 拖滑块 / 程序写值）→ 浮现并重新倒数。
    // 句柄必须活到节点销毁：捕获进 `draw` 闭包（闭包挂在节点上，随节点一起消失）。
    let activity = {
        let fade = Rc::clone(&fade);
        scroll.add_activity_listener(Rc::new(move || fade.trigger()))
    };

    // ---------- 几何（全部从容器派生） ----------
    let geo_of = {
        let style = Rc::clone(&style);
        let node_ref = node_ref.clone();
        let scroll = scroll.clone();
        move || -> Option<ThumbGeo> {
            let c = node_ref.try_get()?.parent()?;
            let st = style();
            let full = c.inner_size(d);
            let viewport = full - st.track_inset * 2.0;
            if viewport <= 0.0 {
                return None;
            }
            let content = content_size(&c, d);
            let m = max_scroll(&c, d);
            if m <= 0.0 || content <= 0.0 {
                return None;
            }
            // TS: `Math.min(Math.max(minThumb, viewport * full / content), viewport)`
            let length = (st.min_thumb.max(viewport * full / content)).min(viewport);
            let max_offset = (viewport - length).max(0.0);
            let offset = if max_offset > 0.0 {
                (max_offset * scroll.value() / m).clamp(0.0, max_offset)
            } else {
                0.0
            };
            Some(ThumbGeo {
                offset,
                length,
                max_offset,
                max_scroll: m,
            })
        }
    };

    // 贴在容器内容盒的右缘（纵向）/ 下缘（横向）。
    let position_of = {
        let style = Rc::clone(&style);
        let node_ref = node_ref.clone();
        move |axis: Axis| -> f32 {
            let Some(c) = node_ref.try_get().and_then(|n| n.parent()) else {
                return 0.0;
            };
            let st = style();
            if d == Axis::Y {
                match axis {
                    Axis::X => {
                        c.padding(Axis::X, StartEnd::Start) + c.inner_size(Axis::X) - st.hit_width
                    }
                    Axis::Y => c.padding(Axis::Y, StartEnd::Start),
                }
            } else if axis == Axis::Y {
                c.padding(Axis::Y, StartEnd::Start) + c.inner_size(Axis::Y) - st.hit_width
            } else {
                c.padding(Axis::X, StartEnd::Start)
            }
        }
    };

    // 视口长度（点击翻页按它算一屏）。
    let viewport_length = {
        let node_ref = node_ref.clone();
        move || {
            node_ref
                .try_get()
                .and_then(|n| n.parent())
                .map_or(0.0, |c| c.inner_size(d))
        }
    };

    // ---------- 命中 ----------
    let hit_style = Rc::clone(&style);
    let hit_geo = geo_of.clone();
    let hit_alpha = Rc::clone(&alpha_now);

    // ---------- 绘制 ----------
    let draw_style = Rc::clone(&style);
    let draw_hover = Rc::clone(&hovering);
    let draw_drag = Rc::clone(&dragging);
    let draw_alpha = Rc::clone(&alpha_now);
    let draw_geo = geo_of.clone();
    let slot_draw = Rc::clone(&node_slot);

    // ---------- 按下：拖滑块 / 点空白翻页 ----------
    let down_style = Rc::clone(&style);
    let down_geo = geo_of;
    let down_drag = Rc::clone(&dragging);
    let down_fade = Rc::clone(&fade);
    let down_scroll = scroll.clone();

    let wheel = scroll_wheel(&scroll);
    node2! {
        ..rest,
        node_ref: node_ref.clone(),
        // 浮层：两轴都不占布局空间，内容区拿满可用尺寸（TS `layoutSize(0, true)`）
        size: |_d: Axis| LayoutSize::inside(0.0),
        position: position_of,
        // 指针落在滚动条上滚轮也该滚
        mouse_wheel: move |e: &mut MouseEvent2| wheel(e),
        // 滚动条不是内容：不参与焦点遍历与文本选择
        focusable: false,
        selection_enabled: false,
        accept_hit: move |x: f32, y: f32| {
            // 不可滚动时没有可命中的滑块
            if hit_geo().is_none() {
                return false;
            }
            // 只有"看得见"时才拦截指针：静止态让点击穿透到内容
            if hit_alpha() <= 0.02 {
                return false;
            }
            let st = hit_style();
            if d == Axis::Y {
                x >= 0.0 && x <= st.hit_width
            } else {
                y >= 0.0 && y <= st.hit_width
            }
        },
        mouse_down: move |e: &mut MouseEvent2| {
            let Some(c) = down_geo() else { return };
            let st = down_style();
            let vertical = d == Axis::Y;
            // 局部坐标里滑块起点是 `track_inset + offset`
            let local = if vertical { e.y } else { e.x };
            let pos = local - st.track_inset;
            if pos >= c.offset && pos <= c.offset + c.length {
                if c.max_offset <= 0.0 {
                    return;
                }
                let start_value = down_scroll.value();
                let start_pointer = if vertical { e.global_y } else { e.global_x };
                down_drag.set(true);
                down_fade.trigger();
                let scroll_move = down_scroll.clone();
                let fade_move = Rc::clone(&down_fade);
                let scale = c.max_scroll / c.max_offset;
                let off_move = Rc::new(register_move(Rc::new(move |x: f32, y: f32| {
                    let pointer = if vertical { y } else { x };
                    scroll_move.set_value(start_value + (pointer - start_pointer) * scale);
                    fade_move.trigger();
                })));
                // 抬起：先摘 move，再摘自己（弱槽持有句柄，避免闭包自引用成环）
                let up_slot: UnregisterSlot = Rc::new(RefCell::new(None));
                let up_slot_weak = Rc::downgrade(&up_slot);
                let off_move_owned = Rc::clone(&off_move);
                let drag_up = Rc::clone(&down_drag);
                let fade_up = Rc::clone(&down_fade);
                let handle = register_up(Rc::new(move || {
                    off_move_owned();
                    if let Some(s) = up_slot_weak.upgrade() {
                        if let Some(off) = s.borrow_mut().take() {
                            off();
                        }
                    }
                    drag_up.set(false);
                    fade_up.trigger();
                }));
                *up_slot.borrow_mut() = Some(Rc::new(handle));
            } else {
                // 点轨道空白处：整页滚动（至少一屏的九成，别小到看不见）
                let dir = if pos < c.offset { -1.0 } else { 1.0 };
                down_scroll
                    .scroll(dir * (viewport_length() - st.track_inset * 2.0).max(40.0) * 0.9);
            }
            e.stop_propagation();
        },
        draw: move |scene: &mut Scene, n: &Node2| {
            // `activity` 只为了活到节点销毁；这里按住它（drop 时自动退订）。
            let _ = &activity;
            // 第一次绘制时回填弱引用（全局悬停监听从这里拿到"我是谁"）
            if slot_draw.borrow().is_none() {
                *slot_draw.borrow_mut() = Some(n.downgrade());
            }
            let Some(c) = draw_geo() else { return };
            let alpha = draw_alpha();
            if alpha <= 0.004 {
                return;
            }
            let st = draw_style();
            let vertical = d == Axis::Y;
            let expanded = draw_drag.get() || draw_hover.get();
            let w = if expanded { st.width_hover } else { st.width };
            let base = if draw_drag.get() {
                st.drag_alpha
            } else if draw_hover.get() {
                st.hover_alpha
            } else {
                st.idle_alpha
            };
            let a = (base * alpha).clamp(0.0, 1.0);
            let color = st.thumb.with_alpha((a * 255.0).round() as u8);
            // 胶囊：贴边留白 `edge_gap`，两端留白 `track_inset`
            let along = st.track_inset + c.offset;
            let across = st.hit_width - st.edge_gap - w;
            let rect = if vertical {
                Rect::new(across, along, w, c.length)
            } else {
                Rect::new(along, across, c.length, w)
            };
            scene.fill_round_rect(rect, w / 2.0, color);
        },
    }
}
