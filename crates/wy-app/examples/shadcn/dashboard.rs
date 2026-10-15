//! UI 构建：侧边栏、统计卡片、表单、待办列表，以及顶层 `rsx!` 组装。
//!
//! 树只构建一次；颜色经 [`Theme`] 在绘制期求值，主题切换自动重绘。
//! 布局适配窗口 800×600：侧栏 + 右侧内容（统计行 → 表单/待办并排）。

use std::rc::Rc;

use wy_mve::{rsx, spacer, text_field_opts, Node, TextFieldColors, TextFieldOpts};
use wy_render::{Point, Rect, Scene};
use wy_signal::{GetValue, SetValue, Signal};

use crate::state::{App, Todo};
use crate::tokens::Theme;
use crate::widgets::{badge, button, button_sized, card, text_label, Btn, CARD_PAD};

const NAV: [&str; 4] = ["仪表盘", "项目", "设置", "关于"];

// 窗口固定 800×600（见 wy-engine runner）；下列尺寸据此手工配平。
const WIN_W: f32 = 800.0;
const WIN_H: f32 = 600.0;
const PAD: f32 = 24.0;
const SIDEBAR_W: f32 = 160.0;
const GAP_MAIN: f32 = 20.0;
const CONTENT_W: f32 = WIN_W - PAD * 2.0 - SIDEBAR_W - GAP_MAIN; // 572
const STAT_W: f32 = 182.0;
const STAT_H: f32 = 92.0;
const STAT_GAP: f32 = (CONTENT_W - STAT_W * 3.0) / 2.0; // ≈13
const CARD2_W: f32 = (CONTENT_W - 16.0) / 2.0; // 278
const CARD2_H: f32 = 360.0;
const FIELD_W: f32 = CARD2_W - CARD_PAD * 2.0; // 246

fn sidebar(theme: Theme, app: &App) -> Node {
    let nav = app.theme_dark;
    let mut v: Vec<Node> = vec![
        text_label(
            move || String::from("▲ shadcn-mve"),
            17.0,
            theme,
            |t| t.foreground,
        ),
        spacer(0.0, 16.0),
    ];
    for name in NAV {
        let name = name.to_string();
        let mut item = wy_mve::button(
            {
                let name = name.clone();
                move || name.clone()
            },
            || {},
        );
        item.width = SIDEBAR_W;
        item.height = 34.0;
        item.hit_test_fn =
            Rc::new(move |x, y| (0.0..SIDEBAR_W).contains(&x) && (0.0..34.0).contains(&y));
        item.draw_fn = Rc::new(move |scene| {
            if let Some(s) = scene.downcast_mut::<Scene>() {
                let t = theme.tokens();
                s.fill_round_rect(Rect::new(0.0, 0.0, SIDEBAR_W, 34.0), 6.0, t.accent);
                s.draw_text(Point::new(12.0, 9.0), &name, 14.0, t.muted_fg);
            }
        });
        v.push(item);
    }
    v.push(spacer(0.0, 16.0));
    // 主题按钮：文案与配色都在绘制期读信号
    v.push(button(
        theme,
        move || {
            if nav.get() {
                String::from("切换到浅色")
            } else {
                String::from("切换到深色")
            }
        },
        Btn::Ghost,
        move || nav.set(!nav.get()),
    ));
    let mut n = wy_mve::vbox(v);
    n.gap = 4.0;
    n
}

fn stat_card(theme: Theme, title: &str, value: impl Fn() -> String + 'static, delta: &str) -> Node {
    let title = title.to_string();
    let delta = delta.to_string();
    card(
        theme,
        STAT_W,
        STAT_H,
        vec![
            text_label(move || title.clone(), 13.0, theme, |t| t.muted_fg),
            spacer(0.0, 6.0),
            wy_mve::hbox(vec![
                text_label(value, 22.0, theme, |t| t.foreground),
                spacer(6.0, 0.0),
                badge(theme, &delta, 1),
            ]),
        ],
    )
}

fn input_field(theme: Theme, value: Signal<String>, placeholder: &str) -> Node {
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

fn form_card(theme: Theme, app: &App) -> Node {
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
            wy_mve::hbox(vec![submit, spacer(8.0, 0.0), reset]),
            spacer(0.0, 12.0),
            text_label(move || status_text.get(), 13.0, theme, |t| t.muted_fg),
        ],
    )
}

fn todo_row(theme: Theme, todos: Signal<Vec<Todo>>, todo: Todo) -> Node {
    let id = todo.id;
    let text = todo.text.clone();
    let done = todo.done;
    let del = button_sized(
        theme,
        move || String::from("删除"),
        Btn::Destructive,
        48.0,
        26.0,
        move || {
            let mut l = todos.get();
            l.retain(|x| x.id != id);
            todos.set(l);
        },
    );
    let fg: fn(&crate::tokens::Tokens) -> wy_render::Color = if done {
        |t| t.muted_fg
    } else {
        |t| t.foreground
    };
    wy_mve::hbox(vec![
        badge(
            theme,
            if done { "完成" } else { "进行中" },
            if done { 1 } else { 0 },
        ),
        spacer(8.0, 0.0),
        text_label(move || text.clone(), 14.0, theme, fg),
        del,
    ])
}

fn todos_card(theme: Theme, app: &App) -> Node {
    let todos = app.todos;
    let next_id = app.next_id;
    let input = Signal::new(String::new());
    let add = {
        let (todos, next_id, input) = (todos, next_id, input);
        button(
            theme,
            move || String::from("添加"),
            Btn::Secondary,
            move || {
                let text = input.get().trim().to_string();
                if !text.is_empty() {
                    let mut l = todos.get();
                    let id = next_id.get();
                    next_id.set(id + 1);
                    l.push(Todo {
                        id,
                        text,
                        done: false,
                    });
                    todos.set(l);
                    input.set(String::new());
                }
            },
        )
    };

    // 空列表提示：用一个「0/1 条」的 for_each 承载——空时才产出该项。
    // （不用 hide_fn：隐藏节点仍会占布局空间，见 node_size 未过滤 is_hidden）
    let empty_tip = wy_mve::for_each(
        move |emit| {
            if todos.get().is_empty() {
                emit(0u32, ());
            }
        },
        move |_k, _v, cx| {
            cx.child(text_label(
                move || String::from("暂无待办，添加一个吧"),
                13.0,
                theme,
                |t| t.muted_fg,
            ));
        },
    );

    card(
        theme,
        CARD2_W,
        CARD2_H,
        vec![
            text_label(
                move || String::from("待办清单"),
                17.0,
                theme,
                |t| t.foreground,
            ),
            spacer(0.0, 12.0),
            input_field(theme, input, "新的待办..."),
            spacer(0.0, 8.0),
            add,
            spacer(0.0, 12.0),
            // 列表用 `for_each`（keyed）：节点树只建一次，增删项只动对应 holder。
            wy_mve::vbox({
                let mut v: Vec<wy_mve::Slot> = Vec::new();
                v.push(wy_mve::for_each(
                    move |emit| {
                        for todo in todos.get() {
                            emit(todo.id, todo.clone());
                        }
                    },
                    move |_k, todo_rc, _cx| {
                        let todo = todo_rc.borrow().clone();
                        _cx.child(todo_row(theme, todos, todo));
                        _cx.child(spacer(0.0, 6.0));
                    },
                ));
                v.push(empty_tip);
                v
            }),
        ],
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
                text_label(
                    move || {
                        if theme.dark.get() {
                            String::from("深色主题")
                        } else {
                            String::from("浅色主题")
                        }
                    },
                    20.0,
                    theme,
                    |t| t.foreground,
                ),
                badge(theme, "v0.1.0", 3),
            }

            spacer(0.0, 20.0)

            hbox {
                gap: GAP_MAIN,
                sidebar(theme, app),
                vbox {
                    gap: 16.0,
                    hbox {
                        gap: STAT_GAP,
                        stat_card(theme, "总访问", move || visits.get().to_string(), "+12%"),
                        stat_card(theme, "活跃用户", move || "42".to_string(), "+4%"),
                        stat_card(theme, "转化率", move || "7%".to_string(), "-1%"),
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
