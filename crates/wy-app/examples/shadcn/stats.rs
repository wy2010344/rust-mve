//! 统计卡片：标题（图标 + 文案）+ 数值 + 变化徽章。
//!
//! # 行高固定
//!
//! 卡片高度随窗口变化（`grow` + `Stretch`），但**内部两行的行高固定**
//! ——标题行、数值行、间距都是常量。这样窗口拉高时卡片内部不会出现
//! "行距忽疏忽密"，多余空间留在下方（内容顶部对齐）。
//!
//! # 文本是子节点，不是画笔
//!
//! 三处文案（标题 / 数值 / 变化）都走文本组件，位置由 flex 算（`Center` +
//! `align_fix`），宽度由文本自然宽撑开——不再 `measure_text` 之后把字
//! `draw_text` 摆在手算坐标上。变化徽章直接复用 `badge!`。

use wy_mve::canvas_render2::node2::Ext;
use wy_mve::{flex, node2, rec, AlignItem, Fc, Icon, Layout, Node2, Scene};

use crate::tokens::Theme;
use crate::widgets::{card, draw_icon_at};

/// 标题行高（固定）。
const TITLE_H: f32 = 20.0;
/// 标题与数值之间的间距（固定）。
const GAP: f32 = 6.0;
/// 数值行高（固定）。
const VALUE_H: f32 = 30.0;
/// 图标边长。
const ICON: f32 = 15.0;
/// 图标 / 数值与右侧内容之间的间距。
const ICON_GAP: f32 = 6.0;

/// 统计卡片：标题（带矢量图标）+ 数值（信号驱动 getter）+ 变化徽章。
///
/// 尺寸交给父容器：调用处 `exts: vec![grow(1.0)]` 均分行宽、父级 `Stretch`
/// 等高——窗口缩放自动跟随；**内部行高固定**（见模块注释）。
#[rec]
pub fn stat_card(
    theme: Theme,
    title: String,
    #[getter]
    #[props(required)]
    value: String,
    delta: String,
    icon: Icon,
    /// 布局扩展（`grow(1.0)`），等价旧版的 `grow: 1.0` 通用属性。
    exts: Vec<Ext>,
) -> Fc {
    // 标题行：图标节点 + 文本节点（图标是矢量路径，不依赖字体字形）
    let head = node2! {
        height: TITLE_H,
        layout: flex! {
            direction: Layout::Row,
            align_item: AlignItem::Center,
            align_fix: true,
            gap: ICON_GAP,
        },
        children: vec![
            node2! {
                width: ICON,
                height: ICON,
                draw: move |s: &mut Scene, _n: &Node2| {
                    draw_icon_at(s, icon, 0.0, 0.0, ICON, theme.tokens().muted_fg);
                },
            },
            text_label! {
                content: title.clone(),
                font_size: 13.0,
                color: theme.tokens().muted_fg,
            },
        ],
    };
    // 数值行：数值文本 + 变化徽章（徽章贴在数值右侧）
    let value_row = node2! {
        height: VALUE_H,
        layout: flex! {
            direction: Layout::Row,
            align_item: AlignItem::Center,
            align_fix: true,
            gap: ICON_GAP,
        },
        children: vec![
            text_label! {
                content: value(),
                font_size: 22.0,
                color: theme.tokens().foreground,
            },
            badge! { theme: theme, text: delta.clone(), variant: 1u8 },
        ],
    };

    card(theme, GAP, exts, vec![head, value_row])
}
