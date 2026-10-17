//! UI 构建：顶层 `rsx!` 组装、共享尺寸常量与输入框工具。
//!
//! 树只构建一次；颜色经 [`Theme`] 在绘制期求值，主题切换自动重绘。
//! 布局适配窗口 800×600：侧栏 + 右侧内容（统计行 → 表单/待办并排）。
//!
//! 子模块按区块拆分：[`sidebar`]（侧栏）、[`stats`]（统计卡片）、
//! [`form`]（联系表单）、[`todos`]（待办清单）。

mod form;
mod sidebar;
mod stats;
mod todos;

use std::rc::Rc;

use wy_mve::{rsx, spacer, text_field_opts, Node, TextFieldColors, TextFieldOpts};
use wy_render::{Rect, Scene};
use wy_signal::{GetValue, SetValue, Signal};

use crate::state::App;
use crate::tokens::Theme;
use crate::widgets::{badge, text_label, CARD_PAD};

use form::form_card;
use sidebar::sidebar;
use stats::stat_card;
use todos::todos_card;

// 窗口固定 800×600（见 wy-engine runner）；下列尺寸据此手工配平。
pub(super) const WIN_W: f32 = 800.0;
pub(super) const WIN_H: f32 = 600.0;
pub(super) const PAD: f32 = 24.0;
pub(super) const SIDEBAR_W: f32 = 160.0;
pub(super) const GAP_MAIN: f32 = 20.0;
pub(super) const CONTENT_W: f32 = WIN_W - PAD * 2.0 - SIDEBAR_W - GAP_MAIN; // 572
pub(super) const STAT_W: f32 = 182.0;
pub(super) const STAT_H: f32 = 92.0;
pub(super) const STAT_GAP: f32 = (CONTENT_W - STAT_W * 3.0) / 2.0; // ≈13
pub(super) const CARD2_W: f32 = (CONTENT_W - 16.0) / 2.0; // 278
pub(super) const CARD2_H: f32 = 360.0;
pub(super) const FIELD_W: f32 = CARD2_W - CARD_PAD * 2.0; // 246

/// 文本输入框：宽度固定，占位符与配色在绘制期读信号。
pub(super) fn input_field(theme: Theme, value: Signal<String>, placeholder: &str) -> Node {
    text_field_opts(
        move || value.get(),
        move |v| value.set(v),
        TextFieldOpts {
            width: FIELD_W,
            height: 34.0,
            placeholder: placeholder.to_string(),
            colors_fn: Some(Rc::new(TextFieldColors {
                text_color: Rc::new(move || theme.tokens().foreground),
                placeholder_color: Rc::new(move || theme.tokens().muted_fg),
                bg_color: Rc::new(move || theme.tokens().input_bg),
                border_color: Rc::new(move || theme.tokens().border),
                focus_border_color: Rc::new(move || theme.tokens().ring),
            })),
            ..Default::default()
        },
    )
}

pub fn dashboard(app: &App) -> Node {
    let theme = Theme::new(app.theme_dark);
    let visits = app.visits;

    // 内容区（带外边距）：header → 统计行 → 表单/待办并排
    let content = rsx! {
        vbox_at(PAD, PAD) {
            gap: 0.0,

            hbox {
                gap: 12.0,
                text_label {
                    content: move || {
                        if theme.dark.get() {
                            String::from("深色主题")
                        } else {
                            String::from("浅色主题")
                        }
                    },
                    size: 20.0,
                    theme: theme,
                    color: |t| t.foreground,
                }
                badge { theme: theme, text: || String::from("v0.1.0"), variant: || 3 }
            }

            spacer(0.0, 20.0)

            hbox {
                gap: GAP_MAIN,
                sidebar(theme, app),
                vbox {
                    gap: 16.0,
                    hbox {
                        gap: STAT_GAP,
                        stat_card { theme: theme, title: String::from("总访问"), value: move || visits.get().to_string(), delta: String::from("+12%") }
                        stat_card { theme: theme, title: String::from("活跃用户"), value: move || String::from("42"), delta: String::from("+4%") }
                        stat_card { theme: theme, title: String::from("转化率"), value: move || String::from("7%"), delta: String::from("-1%") }
                    },
                    hbox {
                        gap: 16.0,
                        form_card(theme, app),
                        todos_card(theme, app),
                    }
                }
            }
        }
    };

    // 根节点：负责铺满窗口的背景，内容作为其子节点（内容自身带 PAD 偏移）。
    let mut root = wy_mve::vbox(vec![content.into_node()]);
    root.gap = 0.0;
    root.draw_fn = Rc::new(move |scene| {
        if let Some(s) = scene.downcast_mut::<Scene>() {
            s.fill_rect(Rect::new(0.0, 0.0, WIN_W, WIN_H), theme.tokens().background);
        }
    });
    root
}
