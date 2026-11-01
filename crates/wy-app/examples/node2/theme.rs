//! 四个 demo 共用的深色终端配色。
//!
//! 只放常量，不放逻辑——示例的看点是**声明式树**，不是主题系统。

#![allow(dead_code)]

use wy_mve::Color;

pub const BG: Color = Color::from_u32(0x0A0E14FF);
pub const PANEL: Color = Color::from_u32(0x11161FFF);
pub const PANEL2: Color = Color::from_u32(0x1C2430FF);
pub const ROW_ALT: Color = Color::from_u32(0x151B26FF);
pub const LINE: Color = Color::from_u32(0x232B3AFF);
pub const TEXT: Color = Color::from_u32(0xE6EDF3FF);
pub const MUTED: Color = Color::from_u32(0x9AA4B2FF);
pub const FAINT: Color = Color::from_u32(0x5A6472FF);
pub const ACCENT: Color = Color::from_u32(0x2F81F7FF);

/// 嵌套层级底色（abs 示例：每深一层亮一档，方便看清层叠关系）。
pub const L1: Color = Color::from_u32(0x1E2430FF);
pub const L2: Color = Color::from_u32(0x28303FFF);
pub const L3: Color = Color::from_u32(0x333D50FF);

/// 画布根节点：TS 的画布根在原点 `(0,0)`、尺寸等于视口。
///
/// 这一句每个 demo 都要写一遍（根没有父，`defaultPosition` 无从推导，
/// 所以 `x`/`y` 必须显式给 0）。
pub fn root_arg(viewport: std::rc::Rc<std::cell::Cell<(f32, f32)>>) -> wy_mve::Node2Props {
    use wy_mve::canvas_render2::node2::{always, get};
    let vp_w = viewport.clone();
    let vp_h = viewport;
    wy_mve::Node2Props {
        x: Some(always(0.0)),
        y: Some(always(0.0)),
        width: Some(get(move || vp_w.get().0)),
        height: Some(get(move || vp_h.get().1)),
        ..wy_mve::Node2Props::default()
    }
}
