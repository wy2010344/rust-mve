//! shadcn 卡片：底色 + 边框 + 圆角 8，内含 `children`（Column 排布，带内边距）。

use wy_mve::canvas_render2::node2::Ext;
use wy_mve::{flex, node2, rec, AlignItem, Fc, Layout, Node2, Scene};
use wy_render::Rect;

use crate::tokens::Theme;

/// 卡片内边距。
pub const CARD_PAD: f32 = 16.0;

/// 卡片容器。`exts` 由调用处给（`grow(1.0)` 之类），`gap` 是内容行间距。
#[rec]
pub fn card(
    theme: Theme,
    /// 子项间距。
    #[props(default = 0.0f32)]
    gap: f32,
    /// 布局扩展：`grow(1.0)` 等。
    exts: Vec<Ext>,
    children: Vec<Fc>,
) -> Fc {
    node2! {
        padding: CARD_PAD,
        // `align_fix: true`：交叉轴尺寸取「父给的」。没有它，列容器会被判定为
        // 「由内容撑开」，父级的 `grow` 分配落不到卡片上（见模块文档规则 2）。
        layout: flex! {
            direction: Layout::Column,
            align_item: AlignItem::Stretch,
            align_fix: true,
            gap: gap,
        },
        exts: exts,
        draw: move |s: &mut Scene, n: &Node2| {
            let t = theme.tokens();
            let (w, h) = (n.width(), n.height());
            s.fill_round_rect(Rect::new(0.0, 0.0, w, h), 8.0, t.card);
            s.stroke_round_rect(Rect::new(0.0, 0.0, w, h), 8.0, t.border, 1.0);
        },
        children: children,
    }
}
