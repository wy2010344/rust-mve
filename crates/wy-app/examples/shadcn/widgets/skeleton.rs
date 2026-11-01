//! shadcn Skeleton：加载占位灰块（高度给定，宽度由父撑满）。

use wy_mve::{flex, node2, rec, Fc, Layout, Node2, Scene};
use wy_render::Rect;

use crate::tokens::Theme;

#[rec]
pub fn skeleton(theme: Theme, #[props(default = 12.0f32)] h: f32) -> Fc {
    // `align_fix: true`：宽度取父给的（否则 Row 布局下会退成 0）。
    node2! {
        height: h,
        layout: flex! { direction: Layout::Row, align_fix: true },
        draw: move |s: &mut Scene, n: &Node2| {
            s.fill_round_rect(
                Rect::new(0.0, 0.0, n.width(), n.height()),
                4.0,
                theme.tokens().accent,
            );
        },
    }
}
