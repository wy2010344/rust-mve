//! shadcn Checkbox：勾选框。受控，勾用矢量路径（不依赖字体字形）。

use std::rc::Rc;

use wy_mve::{node2, rec, Fc, Icon, Node2, Scene};
use wy_render::Rect;

use crate::tokens::Theme;
use crate::widgets::icon::draw_icon_at;

/// 边长。
const S: f32 = 18.0;

#[rec]
pub fn checkbox(
    theme: Theme,
    #[getter]
    #[props(required)]
    checked: bool,
    #[props(required)] on_change: impl Fn(bool) + 'static,
) -> Fc {
    let click_checked = Rc::clone(&checked);
    node2! {
        width: S,
        height: S,
        mouse_click: move |_e| on_change(!click_checked()),
        draw: move |s: &mut Scene, _n: &Node2| {
            let t = theme.tokens();
            let r = Rect::new(0.5, 0.5, S - 1.0, S - 1.0);
            if checked() {
                s.fill_round_rect(r, 4.0, t.primary);
                draw_icon_at(s, Icon::Check, 3.0, 3.0, S - 6.0, t.primary_fg);
            } else {
                s.fill_round_rect(r, 4.0, t.input_bg);
                s.stroke_round_rect(r, 4.0, t.border, 1.0);
            }
        },
    }
}
