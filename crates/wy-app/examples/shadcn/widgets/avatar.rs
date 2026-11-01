//! shadcn Avatar：圆形头像（无图片资源，渲染首字母占位）。
//!
//! 首字母是**文本子节点**，位置由 flex 居中（`directionJustify: center` +
//! `alignItem: center` + `alignFix`），圆形底色在 `draw` 里画。

use wy_mve::{flex, node2, rec, AlignItem, DirectionJustify, Fc, Layout, Node2, Scene};
use wy_render::Rect;

use crate::tokens::Theme;

/// 占位文字字号。
const FONT: f32 = 13.0;

#[rec]
pub fn avatar(
    theme: Theme,
    #[getter]
    #[props(required)]
    label: String,
    #[props(default = 32.0f32)] size: f32,
) -> Fc {
    node2! {
        width: size,
        height: size,
        layout: flex! {
            direction: Layout::Row,
            align_item: AlignItem::Center,
            direction_justify: DirectionJustify::Center,
            align_fix: true,
        },
        draw: move |s: &mut Scene, n: &Node2| {
            let t = theme.tokens();
            let (w, h) = (n.width(), n.height());
            s.fill_round_rect(Rect::new(0.0, 0.0, w, h), w / 2.0, t.muted);
        },
        children: vec![text_label! {
            content: label(),
            font_size: FONT,
            color: theme.tokens().muted_fg,
        }],
    }
}
