//! shadcn Switch：滑块开关（`switch` 是 Rust 关键字，故名 `toggle`）。

use std::rc::Rc;

use wy_mve::{node2, rec, Fc, Node2, Scene};
use wy_render::Rect;

use crate::tokens::Theme;

/// 开关尺寸。
const W: f32 = 36.0;
/// 开关高度（= 滑块直径 + 4）。
const H: f32 = 20.0;

#[rec]
pub fn toggle(
    theme: Theme,
    #[getter]
    #[props(required)]
    checked: bool,
    #[props(required)] on_change: impl Fn(bool) + 'static,
) -> Fc {
    let click_checked = Rc::clone(&checked);
    node2! {
        width: W,
        height: H,
        mouse_click: move |_e| on_change(!click_checked()),
        draw: move |s: &mut Scene, _n: &Node2| {
            let t = theme.tokens();
            let on = checked();
            s.fill_round_rect(
                Rect::new(0.0, 0.0, W, H),
                H / 2.0,
                if on { t.primary } else { t.input_bg },
            );
            if !on {
                s.stroke_round_rect(Rect::new(0.5, 0.5, W - 1.0, H - 1.0), H / 2.0, t.border, 1.0);
            }
            // 滑块：开 = 靠右，关 = 靠左
            let thumb_x = if on { W - H + 2.0 } else { 2.0 };
            s.fill_round_rect(
                Rect::new(thumb_x, 2.0, H - 4.0, H - 4.0),
                (H - 4.0) / 2.0,
                t.card,
            );
        },
    }
}
