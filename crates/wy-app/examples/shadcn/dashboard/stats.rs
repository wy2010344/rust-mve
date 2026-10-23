//! 统计卡片行：标题（带图标）+ 数值 + 变化徽章。
//!
//! # 行高固定
//!
//! 卡片高度随窗口变化（`grow` + `Stretch`），但**内部三行的行高固定**
//! ——标题行、数值行、间距都是常量。这样窗口拉高时卡片内部不会
//! 出现"行距忽疏忽密"的观感（正是用户说的"行高应该是固定的"）。
//! 内容整体顶部对齐，多出来的空间留在下方（而不是把行拉开）。

use wy_mve::{component, rsx, spacer, AlignItem, Icon};
use wy_render::{Point, Scene};

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

/// 统计卡片组件：标题（带矢量图标）+ 数值（信号驱动闭包）+ 变化徽章。
///
/// 尺寸交给父容器：调用处 `grow: 1.0` 均分行宽、`align_items: Stretch`
/// 等高——窗口缩放自动跟随；**内部行高固定**（见模块注释）。
#[component]
pub(super) fn stat_card(
    theme: Theme,
    title: String,
    #[getter] value: String,
    delta: String,
    icon: Icon,
) -> Node {
    rsx! {
        card {
            theme: theme,
            // 标题行：图标 + 文案（图标为矢量路径，不依赖字体字形）
            node {
                height: TITLE_H,
                align_items: AlignItem::Center,
                draw: move |s: &mut Scene| {
                    let t = theme.tokens();
                    draw_icon_at(s, icon, 0.0, (TITLE_H - ICON) / 2.0, ICON, t.muted_fg);
                    s.draw_text(Point::new(ICON + 6.0, 3.0), &title, 13.0, t.muted_fg);
                },
            }
            spacer(0.0, GAP)
            // 数值行：数值 + 徽章（行高固定）
            node {
                height: VALUE_H,
                align_items: AlignItem::Center,
                draw: move |s: &mut Scene| {
                    let t = theme.tokens();
                    s.draw_text(Point::new(0.0, 3.0), &value(), 22.0, t.foreground);
                    // 徽章贴在数值右侧
                    let vw = wy_render::text_measure::measure_text(&value(), 22.0).0;
                    let bw = wy_render::text_measure::measure_text(&delta, 12.0).0 + 16.0;
                    let x = vw + 6.0;
                    let y = (VALUE_H - 20.0) / 2.0;
                    s.fill_round_rect(
                        wy_render::Rect::new(x, y, bw, 20.0),
                        4.0,
                        t.muted,
                    );
                    s.draw_text(Point::new(x + 8.0, y + 4.0), &delta, 12.0, t.muted_fg);
                },
            }
        }
    }
    .into_node()
}
