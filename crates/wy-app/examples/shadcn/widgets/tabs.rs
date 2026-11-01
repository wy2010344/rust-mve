//! shadcn Tabs：分段选择器。受控：`active` 读信号高亮，点击写回信号。

use wy_mve::{flex, node2, rec, AlignItem, Fc, Layout, Node2, Scene};
use wy_render::Rect;
use wy_signal::{GetValue, SetValue, Signal};

use crate::tokens::Theme;

/// 单段高度。
const ITEM_H: f32 = 26.0;
/// 单段左右内边距（宽度测量与绘制共用）。
const PAD: f32 = 12.0;
/// 段文字字号。
const FONT: f32 = 12.0;

#[rec]
pub fn tabs(
    theme: Theme,
    /// 段文案列表。
    items: Vec<String>,
    /// 当前选中段下标（Signal 共享：绘制期读、点击写）。
    active: Signal<usize>,
) -> Fc {
    let slots: Vec<Fc> = items
        .into_iter()
        .enumerate()
        .map(|(i, item)| {
            node2! {
                height: ITEM_H,
                padding_inline_start: PAD,
                padding_inline_end: PAD,
                mouse_click: move |_e| active.set(i),
                // 段 = 容器 + 文本子节点：宽度由 `padding + 文本自然宽` 撑开，
                // 文字位置由 flex 居中，不再 `measure_text` + `draw_text`。
                layout: flex! {
                    direction: Layout::Row,
                    align_item: AlignItem::Center,
                    align_fix: true,
                },
                draw: move |s: &mut Scene, n: &Node2| {
                    let t = theme.tokens();
                    if active.get() == i {
                        let r = Rect::new(0.0, 0.0, n.width(), n.height());
                        s.fill_round_rect(r, 6.0, t.card);
                        s.stroke_round_rect(r, 6.0, t.border, 1.0);
                    }
                },
                children: vec![text_label! {
                    content: item.clone(),
                    font_size: FONT,
                    color: if active.get() == i {
                        theme.tokens().foreground
                    } else {
                        theme.tokens().muted_fg
                    },
                }],
            }
        })
        .collect();
    // **不能**写 `align_fix: true`：它在 Row 布局里管交叉轴（Y），含义是
    // 「高度取父分配」；而父（卡片列容器）分配高度时又要先问子节点的自然高
    // → 环闭合取 0，整条 tabs 高度塌成 0、与下面输入行重叠。
    node2! {
        padding: 3.0,
        layout: flex! { direction: Layout::Row, gap: 2.0 },
        children: slots,
    }
}
