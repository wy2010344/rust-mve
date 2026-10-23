//! 联系我们表单卡片：姓名/邮箱/留言输入 + 提交/重置 + 状态提示。

use wy_mve::{rsx, spacer, Node};
use wy_signal::{GetValue, SetValue};

use crate::state::App;
use crate::tokens::Theme;
use crate::widgets::{button, card, text_label, Btn};

use super::input_field;

pub(super) fn form_card(theme: Theme, app: &App) -> Node {
    let (name, email, message) = (app.name, app.email, app.message);
    let (status, visits) = (app.status, app.visits);

    rsx! {
        // 尺寸完全交给 flex：size (0,0) 作 flex-basis、grow 均分行宽，
        // align Stretch 与待办卡等高（行高填满窗口剩余高度）
        card {
            theme: theme,
            grow: 1.0,
            size: (0.0, 0.0),
            text_label { content: "联系我们", font_size: 17.0, color: move || theme.tokens().foreground }
            spacer(0.0, 14.0)
            text_label { content: "姓名", color: move || theme.tokens().muted_fg }
            spacer(0.0, 4.0)
            input_field(theme, name, "输入姓名", 0.0, "input-name")
            spacer(0.0, 10.0)
            text_label { content: "邮箱", color: move || theme.tokens().muted_fg }
            spacer(0.0, 4.0)
            input_field(theme, email, "you@example.com", 0.0, "input-email")
            spacer(0.0, 10.0)
            text_label { content: "留言", color: move || theme.tokens().muted_fg }
            spacer(0.0, 4.0)
            input_field(theme, message, "想说的话...", 0.0, "input-message")
            spacer(0.0, 16.0)
            hbox { gap: 8.0,
                button {
                    theme: theme,
                    label: "提交",
                    on_click: move || {
                        if name.get().trim().is_empty() || email.get().trim().is_empty() {
                            status.set(String::from("✗ 姓名与邮箱必填"));
                        } else if !email.get().contains('@') {
                            status.set(String::from("✗ 邮箱格式不正确"));
                        } else {
                            status.set(format!("已提交：{} <{}>", name.get(), email.get()));
                            visits.set(visits.get() + 1);
                        }
                    },
                }
                button {
                    theme: theme,
                    label: "重置",
                    variant: Btn::Ghost,
                    on_click: move || {
                        name.set(String::new());
                        email.set(String::new());
                        message.set(String::new());
                        status.set(String::from("已重置"));
                    },
                }
            }
            spacer(0.0, 12.0)
            text_label { content: move || status.get(), color: move || theme.tokens().muted_fg }
        }
    }
    .into_node()
}
