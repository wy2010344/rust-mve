//! shadcn Separator：细分隔线。

use wy_mve::{node2, rec, Fc, Node2, Scene};
use wy_render::Rect;

use crate::tokens::Theme;

/// 分隔线。自己不声明宽度，靠父级 `align_item: Stretch` 拉伸。
#[rec]
pub fn separator(theme: Theme, #[props(default = false)] vertical: bool) -> Fc {
    // 横线：只声明高度，宽度交给父级 `align_item: Stretch` 拉伸。
    // **不能**写成 `width: 0.0` —— 显式 0 就是真的 0 宽，父级的拉伸不会覆盖
    // 一个已声明的尺寸（旧 v1 靠 `align_self: Stretch` 才能这么写）。
    if vertical {
        node2! {
            width: 1.0,
            draw: move |s: &mut Scene, n: &Node2| {
                s.fill_rect(
                    Rect::new(0.0, 0.0, n.width().max(1.0), n.height().max(1.0)),
                    theme.tokens().border,
                );
            },
        }
    } else {
        node2! {
            height: 1.0,
            draw: move |s: &mut Scene, n: &Node2| {
                s.fill_rect(
                    Rect::new(0.0, 0.0, n.width().max(1.0), n.height().max(1.0)),
                    theme.tokens().border,
                );
            },
        }
    }
}
