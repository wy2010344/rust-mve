//! 可拖拽分割条（canvasRender2 版）。
//!
//! 与 v1 同语义：分割条是 flex 流里的一个 6px 节点，拖动改的是相邻面板的
//! `width` / 比例信号，布局仍由容器分配。差异只在事件原语：v1 用宿主
//! `PointerEvent`（`.root_x`），这里用协议级 [`MouseEvent2`]（`.global_x`）。

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{Duration, Instant};

use wy_mve::canvas_render2::protocol::MouseEvent2;
// 拖拽必须走**全局**注册表：只沿命中链派发的 `mouse_move` 会在指针离开分割条
// 那 6px 后断掉（见 `drag_global` 的文档）。
use wy_mve::canvas_render2::renderer::{register_move, register_up};
use wy_mve::{align, node2, AlignItem, CursorType, Fc, Node2, Scene};
use wy_render::Rect;
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

/// 拖动轴。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    /// 左右拖动（改列宽）。
    X,
    /// 上下拖动（改行高）。
    Y,
}

/// 一次拖动的行为配置。
///
/// 值域一律是**占比**（0..1），不是像素：布局侧用 `grow` 权重消费它，
/// 窗口缩放时各列/各格自动等比，不存任何绝对尺寸。
#[derive(Clone)]
pub struct DragSpec {
    /// 拖动轴。
    pub axis: Axis,
    /// 指针位移 → 值变化的符号（右列往回拖时是 -1）。
    pub sign: f32,
    /// 占比下界。
    pub min: f32,
    /// 占比上界：**每次现算**（列宽要给"其余列 + 中间列最小占比"留够空间）。
    pub max: Rc<dyn Fn() -> f32>,
    /// 双击复位到此占比。
    pub initial: f32,
}

/// 任何一块都不许被拖到看不清。
const MIN_SPLIT: f32 = 0.18;
/// 列宽占比下界。
const MIN_COL: f32 = 0.12;
/// 中间列至少留的占比（K 线 / 深度图的刻度区）。
const MIN_CENTER: f32 = 0.34;

impl DragSpec {
    /// 竖向堆叠的上下分格比（上半块占比）。
    pub fn split(initial: f32) -> Self {
        Self {
            axis: Axis::Y,
            sign: 1.0,
            min: MIN_SPLIT,
            max: Rc::new(|| 1.0 - MIN_SPLIT),
            initial,
        }
    }

    /// 横向列宽占比。`rest` 返回"其余列当前占比之和"，据此现算上界。
    pub fn column(sign: f32, initial: f32, rest: impl Fn() -> f32 + 'static) -> Self {
        Self {
            axis: Axis::X,
            sign,
            min: MIN_COL,
            max: Rc::new(move || (1.0 - rest() - MIN_CENTER).max(MIN_COL)),
            initial,
        }
    }
}

/// 起一次**全局**拖拽：`mv` 驱动位移，松手（指针在哪都算）时调 `done` 并摘除注册。
///
/// # 为什么不能只用节点本地的 `mouse_move` / `mouse_up`
///
/// 引擎只沿**当前指针下的命中链**派发 `Move` / `Up`（`tree/pointer.rs::move_`），
/// 而分割条只有 [`GUTTER`] 宽——指针一横向拖出这条缝，本地回调就再也收不到，
/// 表现为"按住拖不动"。TS 侧同样不用节点消息，而是 `dragMouse`
/// （`tree/globalMouse.ts`）：起手时把 move/up 注册到全局表，宿主在
/// **路由 dispatch 里**按类型广播（`Renderer.ts`），与命中无关。
///
/// 这里的注册顺序与 `global_mouse::drag_mouse_on` 一致（两阶段回填句柄，
/// 避免 move/up 回调互相摘除时撞上重复借用）；差别只是松手还要复位 `active`
/// 这类界面状态。
fn drag_global(mv: impl Fn(f32, f32) + 'static, done: impl Fn() + 'static) {
    let off_move: Rc<RefCell<Option<Box<dyn Fn()>>>> = Rc::new(RefCell::new(None));
    let off_up: Rc<RefCell<Option<Box<dyn Fn()>>>> = Rc::new(RefCell::new(None));

    let mv = register_move(Rc::new(mv));
    *off_move.borrow_mut() = Some(Box::new(mv));

    let om = Rc::clone(&off_move);
    let ou = Rc::clone(&off_up);
    let up = register_up(Rc::new(move || {
        // 先摘注册再回调：`done` 里可能再触发一次布局/重绘。
        if let Some(off) = om.borrow().as_ref() {
            off();
        }
        if let Some(off) = ou.borrow().as_ref() {
            off();
        }
        done();
    }));
    *off_up.borrow_mut() = Some(Box::new(up));
}

/// 构建一根分割条。
pub fn gutter(value: Signal<f32>, spec: DragSpec, p: SizeProbe) -> Fc {
    let active = Signal::new(false);
    let hover = Signal::new(false);
    let last_press: Rc<Cell<Option<Instant>>> = Rc::new(Cell::new(None));

    let axis = spec.axis;
    let pointer_of = move |ev: &MouseEvent2| match axis {
        Axis::X => ev.global_x,
        Axis::Y => ev.global_y,
    };
    let probe_of = move |size: (f32, f32)| match axis {
        Axis::X => size.0,
        Axis::Y => size.1,
    };

    let on_down: Rc<dyn Fn(&mut MouseEvent2)> = Rc::new({
        let last_press = Rc::clone(&last_press);
        let spec = spec.clone();
        move |ev: &mut MouseEvent2| {
            let now = Instant::now();
            if let Some(prev) = last_press.get() {
                if now.duration_since(prev) < DOUBLE_CLICK {
                    value.set(spec.initial);
                    last_press.set(None);
                    return;
                }
            }
            last_press.set(Some(now));

            let start_pointer = pointer_of(ev);
            let start_value = value.get();
            let probe = Rc::clone(&p);
            let max_fn = Rc::clone(&spec.max);
            let (sign, smin) = (spec.sign, spec.min);
            active.set(true);
            drag_global(
                move |x, y| {
                    let pointer = match axis {
                        Axis::X => x,
                        Axis::Y => y,
                    };
                    // 位移 → 占比：除以**实测**的主轴尺寸（探针由容器 draw 写入，
                    // 不是硬编码常量），所以窗口缩放后拖动手感一致。
                    let dp = pointer - start_pointer;
                    let dim = probe_of(probe.get()).max(1.0);
                    let next = start_value + dp * sign / dim;
                    value.set(next.clamp(smin, max_fn()));
                },
                move || active.set(false),
            );
        }
    });

    // 交叉轴尺寸交给父布局的 `Stretch`，**不能**声明成 0：一旦声明，`width()`
    // 就直接返回它、压过 `Stretch`（`allow_size` 优先于 `default_size`），
    // 命中区会塌成一条零宽线。所以按轴分两支，只声明主轴那 6px。
    let paint: Rc<dyn Fn(&mut Scene, &Node2)> = Rc::new(move |s: &mut Scene, n: &Node2| {
        if n.width() <= 0.0 || n.height() <= 0.0 {
            return;
        }
        let on = active.get();
        let hot = on || hover.get();
        if on {
            s.fill_round_rect(
                Rect::new(0.0, 0.0, n.width(), n.height()),
                3.0,
                theme::ACCENT_SOFT,
            );
        } else if hover.get() {
            s.fill_round_rect(
                Rect::new(0.0, 0.0, n.width(), n.height()),
                3.0,
                theme::ROW_ACTIVE,
            );
        }
        // 三个小点常驻：不悬停也得看得出"这儿能拖"。颜色只在不悬停时压暗到
        // HANDLE——比底色亮一档，又不会在满屏面板里抢注意力。
        let color = if on {
            theme::ACCENT
        } else if hover.get() {
            theme::TEXT
        } else {
            theme::HANDLE
        };
        let (cx, cy) = (n.width() / 2.0, n.height() / 2.0);
        for k in -1..=1 {
            let off = k as f32 * 10.0;
            let (dx, dy) = match axis {
                Axis::X => (0.0, off),
                Axis::Y => (off, 0.0),
            };
            s.fill_round_rect(
                Rect::new(cx + dx - 2.0, cy + dy - 2.0, 4.0, 4.0),
                2.0,
                color,
            );
        }
        // 悬停/拖动时再补一条贯穿的 1px 轴线，看清"缝"在哪、拖到了哪
        if hot {
            let seam = if on { theme::ACCENT_DIM } else { theme::HANDLE };
            match axis {
                Axis::X => s.fill_rect(Rect::new(cx - 0.5, 0.0, 1.0, n.height()), seam),
                Axis::Y => s.fill_rect(Rect::new(0.0, cy - 0.5, n.width(), 1.0), seam),
            }
        }
    });

    if axis == Axis::X {
        // 竖条：宽 6，高由父的 Stretch 给
        let d = Rc::clone(&on_down);
        let paint = Rc::clone(&paint);
        node2! {
            debug_name: "gutter-x".to_string(),
            width: GUTTER,
            cursor: CursorType::Pointer,
            on_hover: move |entered: bool| hover.set(entered),
            mouse_down: move |e: &mut MouseEvent2| d(e),
            exts: vec![align(AlignItem::Stretch)],
            draw: move |s: &mut Scene, n: &Node2| paint(s, n),
        }
    } else {
        // 横条：高 6，宽由父的 Stretch 给
        node2! {
            debug_name: "gutter-y".to_string(),
            height: GUTTER,
            cursor: CursorType::Pointer,
            on_hover: move |entered: bool| hover.set(entered),
            mouse_down: move |e: &mut MouseEvent2| on_down(e),
            exts: vec![align(AlignItem::Stretch)],
            draw: move |s: &mut Scene, n: &Node2| paint(s, n),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wy_mve::canvas_render2::renderer::{dispatch_global_mouse_move, dispatch_global_mouse_up};

    /// 拖动起手后，move 必须来自**全局**广播：坐标远离分割条也照收。
    ///
    /// 这是"拖不动"的根因回归测试——旧实现用节点本地 `mouse_move`，
    /// 引擎只沿命中链派发（`tree/pointer.rs::move_`），指针一离开那 6px
    /// 就再也收不到消息。
    #[test]
    fn drag_keeps_receiving_after_leaving_the_strip() {
        let got: Rc<RefCell<Vec<(f32, f32)>>> = Rc::new(RefCell::new(Vec::new()));
        let ended = Rc::new(Cell::new(false));

        let g = Rc::clone(&got);
        let e = Rc::clone(&ended);
        drag_global(move |x, y| g.borrow_mut().push((x, y)), move || e.set(true));

        // 远在任何节点之外：本地命中派发到不了这儿，全局广播会。
        dispatch_global_mouse_move(3000.0, -1000.0);
        assert_eq!(got.borrow().len(), 1, "全局 move 必须送达拖拽回调");
        assert!(!ended.get(), "拖动中不应提前结束");

        dispatch_global_mouse_up();
        assert!(ended.get(), "松手要复位 active 这类界面状态");

        dispatch_global_mouse_move(10.0, 10.0);
        assert_eq!(got.borrow().len(), 1, "松手后必须摘除全局注册");
    }
}
