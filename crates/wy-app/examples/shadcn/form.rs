//! 联系我们表单卡片：姓名/邮箱/留言输入 + 提交/重置 + 状态提示。
//!
//! 与旧 `shadcn/dashboard/form.rs` 的差别只有形态：
//! - `rsx! { card { .. } }` → `card(theme, 0.0, vec![grow(1.0)], vec![..])`
//! - `spacer(0.0, N)` → `vgap(N)`（一个定高空节点，新体系没有 v1 `spacer`）
//! - `input_field` 内部换成 `editable_text_node`（见 `mod.rs`）

use wy_mve::{flex, grow, node2, AlignItem, Fc, Layout};
use wy_signal::{GetValue, SetValue};

use crate::dashboard::input_field;
use crate::state::App;
use crate::tokens::Theme;
use crate::widgets::{card, Btn};

/// 竖向间隔（替代旧 `spacer(0.0, h)`）。
fn vgap(h: f32) -> Fc {
    node2! { height: h }
}

pub(super) fn form_card(theme: Theme, app: &App) -> Fc {
    let (name, email, message) = (app.name, app.email, app.message);
    let (status, visits) = (app.status, app.visits);

    let buttons = node2! {
        layout: flex! { direction: Layout::Row, gap: 8.0, align_item: AlignItem::Center },
        children: vec![
            button! {
                theme: theme,
                label: "提交".to_string(),
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
            },
            button! {
                theme: theme,
                label: "重置".to_string(),
                variant: Btn::Ghost,
                on_click: move || {
                    name.set(String::new());
                    email.set(String::new());
                    message.set(String::new());
                    status.set(String::from("已重置"));
                },
            },
        ],
    };

    card(
        theme,
        0.0,
        vec![grow(1.0)],
        vec![
            text_label! {
                content: "联系我们".to_string(),
                font_size: 17.0,
                color: theme.tokens().foreground,
            },
            vgap(14.0),
            text_label! { content: "姓名".to_string(), color: theme.tokens().muted_fg },
            vgap(4.0),
            input_field(theme, name, "输入姓名", vec![]),
            vgap(10.0),
            text_label! { content: "邮箱".to_string(), color: theme.tokens().muted_fg },
            vgap(4.0),
            input_field(theme, email, "you@example.com", vec![]),
            vgap(10.0),
            text_label! { content: "留言".to_string(), color: theme.tokens().muted_fg },
            vgap(4.0),
            input_field(theme, message, "想说的话...", vec![]),
            vgap(16.0),
            buttons,
            vgap(12.0),
            text_label! { content: status.get(), color: theme.tokens().muted_fg },
        ],
    )
}
