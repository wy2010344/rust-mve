//! 联系我们表单卡片：姓名/邮箱/留言输入 + 提交/重置 + 状态提示。

use wy_mve::{hbox, spacer, Node};
use wy_signal::{GetValue, SetValue};

use crate::state::App;
use crate::tokens::Theme;
use crate::widgets::{button, card, text_label, Btn};

use super::{input_field, CARD2_H, CARD2_W};

pub(super) fn form_card(theme: Theme, app: &App) -> Node {
    let (name, email, message) = (app.name, app.email, app.message);
    let (status, visits) = (app.status, app.visits);

    let submit = {
        let (name, email, status, visits) = (name, email, status, visits);
        button(
            theme,
            move || String::from("提交"),
            Btn::Primary,
            move || {
                if name.get().trim().is_empty() || email.get().trim().is_empty() {
                    status.set(String::from("✗ 姓名与邮箱必填"));
                } else if !email.get().contains('@') {
                    status.set(String::from("✗ 邮箱格式不正确"));
                } else {
                    status.set(format!("✓ 已提交：{} <{}>", name.get(), email.get()));
                    visits.set(visits.get() + 1);
                }
            },
        )
    };
    let reset = {
        let (name, email, message, status) = (name, email, message, status);
        button(
            theme,
            move || String::from("重置"),
            Btn::Ghost,
            move || {
                name.set(String::new());
                email.set(String::new());
                message.set(String::new());
                status.set(String::from("已重置"));
            },
        )
    };
    let status_text = status;

    card(
        theme,
        CARD2_W,
        CARD2_H,
        vec![
            text_label(
                move || String::from("联系我们"),
                17.0,
                theme,
                |t| t.foreground,
            ),
            spacer(0.0, 14.0),
            text_label(move || String::from("姓名"), 13.0, theme, |t| t.muted_fg),
            spacer(0.0, 4.0),
            input_field(theme, name, "输入姓名"),
            spacer(0.0, 10.0),
            text_label(move || String::from("邮箱"), 13.0, theme, |t| t.muted_fg),
            spacer(0.0, 4.0),
            input_field(theme, email, "you@example.com"),
            spacer(0.0, 10.0),
            text_label(move || String::from("留言"), 13.0, theme, |t| t.muted_fg),
            spacer(0.0, 4.0),
            input_field(theme, message, "想说的话..."),
            spacer(0.0, 16.0),
            hbox(vec![submit, spacer(8.0, 0.0), reset]),
            spacer(0.0, 12.0),
            text_label(move || status_text.get(), 13.0, theme, |t| t.muted_fg),
        ],
    )
}
