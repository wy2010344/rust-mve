//! 统计卡片行：标题 + 数值 + 变化徽章。

use wy_mve::{hbox, spacer, Node};

use crate::tokens::Theme;
use crate::widgets::{badge, card, text_label};

use super::{STAT_H, STAT_W};

pub(super) fn stat_card(
    theme: Theme,
    title: &str,
    value: impl Fn() -> String + 'static,
    delta: &str,
) -> Node {
    let title = title.to_string();
    let delta = delta.to_string();
    card(
        theme,
        STAT_W,
        STAT_H,
        vec![
            text_label(move || title.clone(), 13.0, theme, |t| t.muted_fg),
            spacer(0.0, 6.0),
            hbox(vec![
                text_label(value, 22.0, theme, |t| t.foreground),
                spacer(6.0, 0.0),
                badge(theme, &delta, 1),
            ]),
        ],
    )
}
