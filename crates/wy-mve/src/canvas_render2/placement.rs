//! 浮层相对锚点的放置（对标 TS `canvasCore/Placement.ts`、Kotlin `helper/Placement.kt`）。
//!
//! 纯几何：只吃矩形与方向，吐坐标，不碰节点、不碰信号。浮层组件
//! （[`crate::canvas_render2::popover`]）在 `position` 消息里调它，
//! 所以"空间不足自动翻转 + 贴边"这段算法只有一份。
//!
//! # 回退规则
//!
//! 判据只读锚 / 面板 / 视口几何，不含 `x`/`y` 自身迭代量 —— 两次调用幂等：
//! 1. 首选侧放不下（含 `gap`）且对侧放得下 → 翻转到对侧；
//! 2. 双侧都放不下 → 保持首选侧，再贴视口边 clamp；
//! 3. 对齐轴越界（End 端超出视口）→ 同样 clamp 贴边。

/// 放置方向（8 方向）。
///
/// 命名 = 主轴侧 + 对齐端：`BottomStart` 表示"锚点下方、起始边对齐"，
/// `RightEnd` 表示"锚点右侧、结束边对齐"。默认 `BottomStart`
/// 即 Dropdown / Popover 的传统行为。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Placement {
    /// 下方，左对齐。
    #[default]
    BottomStart,
    /// 下方，右对齐。
    BottomEnd,
    /// 上方，左对齐。
    TopStart,
    /// 上方，右对齐。
    TopEnd,
    /// 左侧，上对齐。
    LeftStart,
    /// 左侧，下对齐。
    LeftEnd,
    /// 右侧，上对齐。
    RightStart,
    /// 右侧，下对齐。
    RightEnd,
}

impl Placement {
    /// 主轴是否为纵向（上下）。
    #[must_use]
    pub fn is_vertical(self) -> bool {
        matches!(
            self,
            Placement::BottomStart | Placement::BottomEnd | Placement::TopStart | Placement::TopEnd
        )
    }

    /// 首选主轴侧是否为"正方向"（下 / 右）。
    #[must_use]
    pub fn preferred_first(self) -> bool {
        matches!(
            self,
            Placement::BottomStart
                | Placement::BottomEnd
                | Placement::RightStart
                | Placement::RightEnd
        )
    }

    /// 对齐端：`End` 对齐锚点结束边（右 / 下）。
    #[must_use]
    pub fn align_end(self) -> bool {
        matches!(
            self,
            Placement::BottomEnd | Placement::TopEnd | Placement::LeftEnd | Placement::RightEnd
        )
    }
}

/// 参与放置计算的矩形（左上角 + 宽高）。
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct PlacementRect {
    /// 左上角横坐标。
    pub x: f32,
    /// 左上角纵坐标。
    pub y: f32,
    /// 宽。
    pub width: f32,
    /// 高。
    pub height: f32,
}

impl PlacementRect {
    /// 构造矩形的简写。
    #[must_use]
    pub fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }
}

/// 参与放置计算的尺寸。
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct PlacementSize {
    /// 宽。
    pub width: f32,
    /// 高。
    pub height: f32,
}

impl PlacementSize {
    /// 构造尺寸的简写。
    #[must_use]
    pub fn new(width: f32, height: f32) -> Self {
        Self { width, height }
    }
}

/// 锚矩形 → 面板坐标（均为浮层根节点内的相对坐标），带空间不足自动回退。
///
/// # 参数
/// - `p`：放置方向；
/// - `anchor`：锚矩形（浮层根内相对坐标）——传按钮的绝对矩形即可，
///   面板会贴着它的某一条边展开；
/// - `panel`：面板尺寸；
/// - `viewport`：视口尺寸（浮层根节点的 outer 尺寸）；**`<= 0` 视为未测量**
///   （离屏 / 首帧），此时只出首选位：不翻转、不 clamp；
/// - `gap`：面板与锚点在主轴上的间距。
///
/// 返回值是**面板左上角**在浮层根坐标系里的位置。
#[must_use]
pub fn resolve_placement(
    p: Placement,
    anchor: PlacementRect,
    panel: PlacementSize,
    viewport: PlacementSize,
    gap: f32,
) -> (f32, f32) {
    let vertical = p.is_vertical();
    let first = p.preferred_first();
    let end = p.align_end();

    let (ax, ay, aw, ah) = (anchor.x, anchor.y, anchor.width, anchor.height);
    let (pw, ph) = (panel.width, panel.height);

    // 首选位（不依赖视口）
    let mut x = if vertical {
        if end {
            ax + aw - pw
        } else {
            ax
        }
    } else if first {
        ax + aw + gap
    } else {
        ax - pw - gap
    };
    let mut y = if vertical {
        if first {
            ay + ah + gap
        } else {
            ay - ph - gap
        }
    } else if end {
        ay + ah - ph
    } else {
        ay
    };

    // 视口无效（未测量）：直接返回首选位，保持静态定位行为
    if viewport.width <= 0.0 || viewport.height <= 0.0 {
        return (x, y);
    }

    // 首选侧放得下 → 首选；放不下且对侧放得下 → 翻转；双侧不足 → 保持首选（随后 clamp）
    if vertical {
        let fits_below = ay + ah + gap + ph <= viewport.height;
        let fits_above = ay - gap - ph >= 0.0;
        y = if preferred_side(first, fits_below, fits_above) {
            ay + ah + gap
        } else {
            ay - ph - gap
        };
    } else {
        let fits_right = ax + aw + gap + pw <= viewport.width;
        let fits_left = ax - gap - pw >= 0.0;
        x = if preferred_side(first, fits_right, fits_left) {
            ax + aw + gap
        } else {
            ax - pw - gap
        };
    }

    // 贴边 clamp（含对齐轴越界）
    x = clamp01(x, (viewport.width - pw).max(0.0));
    y = clamp01(y, (viewport.height - ph).max(0.0));
    (x, y)
}

/// 主轴落位裁决：是否落在"正方向"一侧（下 / 右）。
///
/// 三分支与 Kotlin 的 `when` 一一对应：首选能放就放首选；首选放不下但对侧
/// 能放就翻过去；两边都放不下就保持首选（随后 clamp 贴边）。
fn preferred_side(first: bool, fits_positive: bool, fits_negative: bool) -> bool {
    if first && fits_positive {
        return true;
    }
    if first && fits_negative {
        return false;
    }
    if !first && !fits_negative && fits_positive {
        return true;
    }
    first
}

/// `min(max(v, 0), max)`。
fn clamp01(v: f32, max: f32) -> f32 {
    v.max(0.0).min(max)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 800x600 视口、120x80 面板、gap 4、锚点 100x30 的夹具。
    fn viewport() -> PlacementSize {
        PlacementSize::new(800.0, 600.0)
    }

    fn panel() -> PlacementSize {
        PlacementSize::new(120.0, 80.0)
    }

    #[test]
    fn bottom_start_prefers_below_the_anchor() {
        let anchor = PlacementRect::new(60.0, 100.0, 100.0, 30.0);
        let (x, y) = resolve_placement(Placement::BottomStart, anchor, panel(), viewport(), 4.0);
        assert_eq!((x, y), (60.0, 134.0), "下方起始对齐 + gap");
    }

    #[test]
    fn bottom_start_flips_up_when_no_space_below() {
        // 锚点贴底：下方只剩 10px，面板 80 放不下 → 向上翻转
        let anchor = PlacementRect::new(60.0, 560.0, 100.0, 30.0);
        let (_, y) = resolve_placement(Placement::BottomStart, anchor, panel(), viewport(), 4.0);
        assert_eq!(y, 560.0 - 80.0 - 4.0, "应翻到锚点上方");
        assert!(y >= 0.0);
    }

    #[test]
    fn bottom_end_aligns_right_edge() {
        let anchor = PlacementRect::new(300.0, 100.0, 100.0, 30.0);
        let (x, _) = resolve_placement(Placement::BottomEnd, anchor, panel(), viewport(), 4.0);
        assert_eq!(x, 300.0 + 100.0 - 120.0, "End 端对齐锚点右缘");
    }

    #[test]
    fn top_start_flips_down_when_no_space_above() {
        // 锚点贴顶：上方放不下 → 翻到下方
        let anchor = PlacementRect::new(60.0, 10.0, 100.0, 30.0);
        let (_, y) = resolve_placement(Placement::TopStart, anchor, panel(), viewport(), 4.0);
        assert_eq!(y, 10.0 + 30.0 + 4.0, "上方不足应翻到下方");
    }

    #[test]
    fn right_start_prefers_right_side() {
        let anchor = PlacementRect::new(100.0, 200.0, 100.0, 30.0);
        let (x, y) = resolve_placement(Placement::RightStart, anchor, panel(), viewport(), 4.0);
        assert_eq!((x, y), (204.0, 200.0));
    }

    #[test]
    fn right_start_flips_left_when_no_space_right() {
        // 锚点贴右缘：右侧只有 20px → 翻到左侧
        let anchor = PlacementRect::new(700.0, 200.0, 90.0, 30.0);
        let (x, _) = resolve_placement(Placement::RightStart, anchor, panel(), viewport(), 4.0);
        assert_eq!(x, 700.0 - 120.0 - 4.0, "右侧不足应翻到左侧");
    }

    #[test]
    fn both_sides_too_small_keeps_preferred_then_clamps() {
        // 面板比视口还高：两侧都放不下 → 保持首选侧后 clamp，完整落在视口内
        let anchor = PlacementRect::new(60.0, 300.0, 100.0, 30.0);
        let tall = PlacementSize::new(120.0, 700.0);
        let (_, y) = resolve_placement(Placement::BottomStart, anchor, tall, viewport(), 4.0);
        assert!(y >= 0.0, "不应超出上缘");
        assert!(y <= 0.0 + 0.001, "面板比视口高 → clamp 到 0");
    }

    #[test]
    fn cross_axis_is_clamped_inside_viewport() {
        // 锚点贴右缘 + End 对齐：面板不能越出右边界
        let anchor = PlacementRect::new(760.0, 100.0, 30.0, 20.0);
        let (x, _) = resolve_placement(Placement::BottomEnd, anchor, panel(), viewport(), 4.0);
        assert!(x + 120.0 <= 800.0 + 0.001, "面板不应超出右缘，实际 x={x}");
    }

    #[test]
    fn unmeasured_viewport_returns_preferred_position_only() {
        let anchor = PlacementRect::new(60.0, 560.0, 100.0, 30.0);
        let (x, y) = resolve_placement(
            Placement::BottomStart,
            anchor,
            panel(),
            PlacementSize::new(0.0, 0.0),
            4.0,
        );
        assert_eq!((x, y), (60.0, 594.0), "未测量时只出首选位，不翻转");
    }

    #[test]
    fn placement_axes_helpers_match_the_eight_directions() {
        assert!(Placement::BottomStart.is_vertical());
        assert!(Placement::TopEnd.is_vertical());
        assert!(!Placement::LeftStart.is_vertical());
        assert!(Placement::RightEnd.preferred_first());
        assert!(!Placement::TopStart.preferred_first());
        assert!(Placement::TopEnd.align_end());
        assert!(Placement::LeftEnd.align_end());
        assert!(!Placement::RightStart.align_end());
    }
}
