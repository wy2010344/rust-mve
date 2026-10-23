//! 可拖拽分割条（flex 版）。
//!
//! 这里**不接管任何位置**：
//! 分割条就是 flex 流里的一个 6px 节点，拖动改的是相邻面板的
//! `width_fn` / `grow_fn`，布局仍旧由容器分配。
//!
//! 两种模式：
//! - 列宽：像素值（对应 ooc 的 `leftWidth` / `rightWidth`）；
//! - 行高：0..1 的比例（对应 ooc 的 `leftBottom` 等，但用比例避免溢出）。
//!
//! 容器尺寸由容器自己的 `draw_fn` 写进 [`SizeProbe`]——这是 `ResizeObserver`
//! 的等价物（ooc 侧是 `measure.ts`）。它只用于把指针位移换算成比例、
//! 以及夹取边界，不参与布局计算。

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{Duration, Instant};

use wy_mve::{current_box, rsx, AlignItem, Node, PointerEvent};
use wy_render::{Rect, Scene};
use wy_signal::{GetValue, SetValue, Signal};

use crate::theme;

/// 分割条粗细。
pub const GUTTER: f32 = 6.0;
/// 双击判定窗口。
const DOUBLE_CLICK: Duration = Duration::from_millis(320);

/// 容器尺寸探针（布局期之外可读的实测值）。
pub type SizeProbe = Rc<Cell<(f32, f32)>>;

/// 新建一个探针。
pub fn probe() -> SizeProbe {
    Rc::new(Cell::new((0.0, 0.0)))
}

// 容器尺寸探针由调用方在 rsx 的 `draw:` 里记录（见 `ui/mod.rs` 的 `stacked`）：
// 只写 `Cell`（不是 `Signal`），所以不会触发重绘，也不会在 draw 期引起环。

/// 拖动轴。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    /// 左右拖动（改列宽）。
    X,
    /// 上下拖动（改行高）。
    Y,
}

/// 一次拖动的行为配置。
#[derive(Clone)]
pub struct DragSpec {
    pub axis: Axis,
    /// 指针位移 → 值变化的符号（右列往回拖时是 -1）。
    pub sign: f32,
    pub min: f32,
    pub max: f32,
    /// 双击复位到此值。
    pub initial: f32,
    /// `true` = 值域 0..1 的比例；`false` = 像素。
    pub fraction: bool,
    /// 像素模式下预留给他人的空间（另一侧列 + 中间最小宽度）。
    /// 做成闭包是因为它依赖其它列的**当前**宽度。
    pub reserve: Rc<dyn Fn() -> f32>,
}

impl DragSpec {
    /// 列宽（像素）。
    pub fn width(
        sign: f32,
        min: f32,
        max: f32,
        initial: f32,
        reserve: impl Fn() -> f32 + 'static,
    ) -> Self {
        Self {
            axis: Axis::X,
            sign,
            min,
            max,
            initial,
            fraction: false,
            reserve: Rc::new(reserve),
        }
    }

    /// 行高（比例）。
    pub fn height(initial: f32, min: f32, max: f32) -> Self {
        Self {
            axis: Axis::Y,
            sign: 1.0,
            min,
            max,
            initial,
            fraction: true,
            reserve: Rc::new(|| 0.0),
        }
    }
}

#[derive(Clone, Copy)]
struct Drag {
    pointer: f32,
    start: f32,
}

/// 构建一根分割条。
pub fn gutter(value: Signal<f32>, spec: DragSpec, p: SizeProbe) -> Node {
    let active = Signal::new(false);
    let hover = Signal::new(false);
    let drag: Rc<RefCell<Option<Drag>>> = Rc::new(RefCell::new(None));
    let last_press: Rc<Cell<Option<Instant>>> = Rc::new(Cell::new(None));

    let axis = spec.axis;
    let pointer_of = move |ev: &PointerEvent| match axis {
        Axis::X => ev.root_x,
        Axis::Y => ev.root_y,
    };
    let probe_of = move |size: (f32, f32)| match axis {
        Axis::X => size.0,
        Axis::Y => size.1,
    };

    let on_down = {
        let drag = Rc::clone(&drag);
        let last_press = Rc::clone(&last_press);
        let spec = spec.clone();
        move |ev: &mut PointerEvent| {
            // 双击 → 复位该分割
            let now = Instant::now();
            if let Some(prev) = last_press.get() {
                if now.duration_since(prev) < DOUBLE_CLICK {
                    value.set(spec.initial);
                    last_press.set(None);
                    return;
                }
            }
            last_press.set(Some(now));
            *drag.borrow_mut() = Some(Drag {
                pointer: pointer_of(ev),
                start: value.get(),
            });
            active.set(true);
        }
    };

    let on_move = {
        let drag = Rc::clone(&drag);
        let spec = spec.clone();
        move |ev: &mut PointerEvent| {
            let Some(d) = *drag.borrow() else { return };
            let dp = pointer_of(ev) - d.pointer;
            let dim = probe_of(p.get()).max(1.0);
            let (next, max) = if spec.fraction {
                // 比例模式：位移 / 容器尺寸
                (d.start + dp * spec.sign / dim, spec.max)
            } else {
                // 像素模式：留出 reserve，避免把中间列压没了
                let reserve = (spec.reserve)();
                let max = spec.max.min((dim - reserve).max(spec.min));
                (d.start + dp * spec.sign, max)
            };
            value.set(next.clamp(spec.min, max));
        }
    };

    let on_up = {
        let drag = Rc::clone(&drag);
        move |_ev: &mut PointerEvent| {
            *drag.borrow_mut() = None;
            active.set(false);
        }
    };

    rsx! {
        node {
            on_down: on_down,
            on_drag_move: on_move,
            on_up: on_up,
            on_hover: move |entered: bool| hover.set(entered),
            width: if axis == Axis::X { GUTTER } else { 0.0 },
            height: if axis == Axis::Y { GUTTER } else { 0.0 },
            align_self: AlignItem::Stretch,
            draw: move |s: &mut Scene| {
                let b = current_box();
                if b.width <= 0.0 || b.height <= 0.0 {
                    return;
                }
                let on = active.get();
                let hot = on || hover.get();
                if on {
                    s.fill_round_rect(Rect::new(0.0, 0.0, b.width, b.height), 3.0, theme::ACCENT_SOFT);
                } else if hover.get() {
                    s.fill_round_rect(Rect::new(0.0, 0.0, b.width, b.height), 3.0, theme::ROW_ACTIVE);
                }
                // 三个小点常驻显示：不悬停也得看得出"这儿能拖"。
                // 颜色只在不悬停时压暗到 HANDLE——比底色亮一档，又不会在满屏
                // 面板里抢注意力。
                let color = if on {
                    theme::ACCENT
                } else if hover.get() {
                    theme::TEXT
                } else {
                    theme::HANDLE
                };
                let (cx, cy) = (b.width / 2.0, b.height / 2.0);
                for k in -1..=1 {
                    let off = k as f32 * 10.0;
                    let (dx, dy) = match axis {
                        Axis::X => (0.0, off),
                        Axis::Y => (off, 0.0),
                    };
                    s.fill_round_rect(Rect::new(cx + dx - 2.0, cy + dy - 2.0, 4.0, 4.0), 2.0, color);
                }
                // 悬停/拖动时再补一条贯穿的 1px 轴线，能看清"缝"在哪、拖到了哪
                if hot {
                    let seam = if on { theme::ACCENT_DIM } else { theme::HANDLE };
                    match axis {
                        Axis::X => s.fill_rect(Rect::new(cx - 0.5, 0.0, 1.0, b.height), seam),
                        Axis::Y => s.fill_rect(Rect::new(0.0, cy - 0.5, b.width, 1.0), seam),
                    }
                }
            },
        }
    }
    .into_node()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fraction_drag_maps_pixels_through_measured_size() {
        let v = Signal::new(0.5);
        let p = probe();
        p.set((0.0, 400.0)); // 容器高 400
        let node = gutter(v, DragSpec::height(0.5, 0.1, 0.9), p);

        node.run_on_down(&mut PointerEvent::new(0.0, 100.0));
        node.run_on_move(&mut PointerEvent::new(0.0, 180.0)); // 下移 80px = 0.2
        assert!((v.get() - 0.7).abs() < 1e-5, "实际 {}", v.get());
        node.run_on_up(&mut PointerEvent::new(0.0, 180.0));
    }

    #[test]
    fn pixel_drag_counts_reserve_in_max() {
        let v = Signal::new(286.0);
        let p = probe();
        p.set((900.0, 0.0));
        let spec = DragSpec::width(1.0, 176.0, 420.0, 286.0, || 700.0);
        let node = gutter(v, spec, p);

        node.run_on_down(&mut PointerEvent::new(300.0, 10.0));
        node.run_on_move(&mut PointerEvent::new(800.0, 10.0));
        assert!(
            (v.get() - 200.0).abs() < 1e-5,
            "应被 reserve 夹住，实际 {}",
            v.get()
        );
        node.run_on_up(&mut PointerEvent::new(800.0, 10.0));
    }

    #[test]
    fn double_click_restores_initial() {
        let v = Signal::new(0.3);
        let p = probe();
        p.set((0.0, 400.0));
        let node = gutter(v, DragSpec::height(0.5, 0.1, 0.9), p);
        node.run_on_down(&mut PointerEvent::new(0.0, 10.0));
        node.run_on_up(&mut PointerEvent::new(0.0, 10.0));
        node.run_on_down(&mut PointerEvent::new(0.0, 10.0));
        assert!((v.get() - 0.5).abs() < 1e-6);
    }
}
