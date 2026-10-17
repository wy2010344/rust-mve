//! 统计卡片行：标题 + 数值 + 变化徽章。

use wy_mve::{component, rsx, spacer};

use crate::tokens::Theme;
use crate::widgets::{badge, card, text_label};

use super::{STAT_H, STAT_W};

/// 统计卡片组件：标题 + 数值（信号驱动闭包）+ 变化徽章。
#[component]
pub(super) fn stat_card(
    theme: Theme,
    title: String,
    value: impl Fn() -> String,
    delta: String,
) -> Node {
    rsx! {
        card {
            theme: theme,
            w: STAT_W,
            h: STAT_H,
            text_label { content: move || title.clone(), theme: theme, color: |t| t.muted_fg }
            spacer(0.0, 6.0)
            hbox { gap: 6.0,
                text_label { content: move || value(), size: 22.0, theme: theme, color: |t| t.foreground }
                badge { theme: theme, text: move || delta.clone(), variant: || 1 }
            }
        }
    }
    .into_node()
}
