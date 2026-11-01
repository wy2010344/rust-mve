//! shadcn 徽章：小圆角标签（`variant` 0=default 1=secondary 2=destructive 3=outline）。
//!
//! 结构：**容器 + 文本子节点**。宽度不是量出来的，而是
//! `paddingInlineStart/End + 文本节点自然宽` 由 flex 主轴 `sizeFromChildren` 撑出来
//! （Kotlin 侧同款：文本是子节点，容器不再自己测字再画）。
//! `align_fix: true` 让文本的垂直居中以徽章自身高度 `H` 为基准。

use std::rc::Rc;

use wy_mve::{flex, node2, rec, AlignItem, Fc, Layout, Node2, Scene};
use wy_render::{Color, Rect};

use crate::tokens::Theme;

/// 徽章高度。
const H: f32 = 20.0;
/// 徽章字号（文本节点与容器共用）。
const FONT: f32 = 12.0;
/// 左右内边距（各 8）。
const PAD_X: f32 = 8.0;

#[rec]
pub fn badge(
    theme: Theme,
    #[getter]
    #[props(required)]
    text: String,
    #[getter]
    #[props(default = 0u8)]
    variant: u8,
) -> Fc {
    // getter 是 `Rc<dyn Fn>`：`draw` 与文本子节点各要一份，先克隆。
    let variant_draw = Rc::clone(&variant);
    node2! {
        height: H,
        padding_inline_start: PAD_X,
        padding_inline_end: PAD_X,
        layout: flex! {
            direction: Layout::Row,
            align_item: AlignItem::Center,
            align_fix: true,
        },
        draw: move |s: &mut Scene, n: &Node2| {
            let t = theme.tokens();
            let (bg, _fg, border) = match variant_draw() {
                2 => (t.destructive, t.destructive_fg, None),
                3 => (Color::TRANSPARENT, t.foreground, Some(t.border)),
                1 => (t.muted, t.muted_fg, None),
                _ => (t.accent, t.foreground, None),
            };
            let r = Rect::new(0.0, 0.0, n.width(), n.height());
            s.fill_round_rect(r, 4.0, bg);
            if let Some(b) = border {
                s.stroke_round_rect(r, 4.0, b, 1.0);
            }
        },
        children: vec![text_label! {
            content: text(),
            font_size: FONT,
            color: match variant() {
                2 => theme.tokens().destructive_fg,
                1 => theme.tokens().muted_fg,
                _ => theme.tokens().foreground,
            },
        }],
    }
}
