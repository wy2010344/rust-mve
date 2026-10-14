//! shadcn 风格仪表盘示例：完整五脏俱全的声明式 UI。
//!
//! 运行：`cargo run -p wy-app --example shadcn`
//!
//! 复刻 shadcn/ui 的 zinc 调色板与卡片/按钮/徽章/表单语言，用 MVE 的
//! `rsx!` + 信号驱动。演示：
//! - 侧边栏导航（当前项高亮）
//! - 统计卡片（数值 + 环比徽章）
//! - 快捷操作按钮（primary/secondary/destructive/ghost 四变体，含 hover/pressed）
//! - 数据录入表单（shadcn 输入框配色 + 实时校验 + 提交）
//! - 待办列表（for_each keyed 增删）
//! - 主题切换（浅/深两套 zinc token，信号驱动全量重绘）
//!
//! 所有视觉都用 `Scene` 的 `fill_round_rect` / `stroke_round_rect` /
//! `draw_text` 自绘（Node 无样式字段，样式写在 `draw_fn` 里）。

use std::rc::Rc;

use wy_engine::mve_integration::MveApp;
use wy_mve::{rsx, spacer, text_field_opts, Node, NodeContext, TextFieldOpts};
use wy_render::{Color, Point, Rect, Scene};
use wy_signal::{GetValue, SetValue, Signal};

// ============================================================================
// shadcn zinc 设计 token（浅/深两套）
// ============================================================================

#[derive(Clone, Copy)]
struct Tokens {
    background: Color,
    foreground: Color,
    card: Color,
    muted: Color,
    muted_fg: Color,
    border: Color,
    input_bg: Color,
    primary: Color,
    primary_fg: Color,
    destructive: Color,
    destructive_fg: Color,
    accent: Color,
    ring: Color,
}

impl Tokens {
    const LIGHT: Self = Self {
        background: Color::rgb(250, 250, 250),
        foreground: Color::rgb(9, 9, 11),
        card: Color::WHITE,
        muted: Color::rgb(244, 244, 245),
        muted_fg: Color::rgb(113, 113, 122),
        border: Color::rgb(228, 228, 231),
        input_bg: Color::WHITE,
        primary: Color::rgb(24, 24, 27),
        primary_fg: Color::WHITE,
        destructive: Color::rgb(220, 38, 38),
        destructive_fg: Color::WHITE,
        accent: Color::rgb(244, 244, 245),
        ring: Color::rgb(24, 24, 27),
    };

    const DARK: Self = Self {
        background: Color::rgb(9, 9, 11),
        foreground: Color::rgb(250, 250, 250),
        card: Color::rgb(24, 24, 27),
        muted: Color::rgb(39, 39, 42),
        muted_fg: Color::rgb(161, 161, 170),
        border: Color::rgb(39, 39, 42),
        input_bg: Color::rgb(24, 24, 27),
        primary: Color::rgb(250, 250, 250),
        primary_fg: Color::rgb(24, 24, 27),
        destructive: Color::rgb(220, 38, 38),
        destructive_fg: Color::WHITE,
        accent: Color::rgb(39, 39, 42),
        ring: Color::rgb(250, 250, 250),
    };
}

// ============================================================================
// 样式组件：卡片 / 徽章 / 按钮 / 文本（Node 无样式字段，样式写在 draw_fn）
// ============================================================================

/// shadcn 卡片：底色 + 边框 + 圆角 8，内含子节点（Column 排布）。
fn card(t: Tokens, w: f32, h: f32, children: Vec<Node>) -> Node {
    let mut n = wy_mve::vbox(children);
    n.width = w;
    n.height = h;
    n.gap = 0.0;
    let (bg, border) = (t.card, t.border);
    n.draw_fn = Rc::new(move |scene| {
        if let Some(s) = scene.downcast_mut::<Scene>() {
            s.fill_round_rect(Rect::new(0.0, 0.0, w, h), 8.0, bg);
            s.stroke_round_rect(Rect::new(0.0, 0.0, w, h), 8.0, border, 1.0);
        }
    });
    n
}

/// shadcn 徽章：小圆角标签。variant 0=default 1=secondary 2=destructive 3=outline。
fn badge(t: Tokens, text: &str, variant: u8) -> Node {
    let (bg, fg, border) = match variant {
        2 => (t.destructive, t.destructive_fg, None),
        3 => (Color::TRANSPARENT, t.foreground, Some(t.border)),
        1 => (t.muted, t.muted_fg, None),
        _ => (t.accent, t.foreground, None),
    };
    let text = text.to_string();
    let w = wy_render::text_measure::measure_text(&text, 12.0).0 + 16.0;
    let h = 20.0;
    let mut n = wy_mve::text({
        let text = text.clone();
        move || text.clone()
    });
    n.width = w;
    n.height = h;
    n.draw_fn = Rc::new(move |scene| {
        if let Some(s) = scene.downcast_mut::<Scene>() {
            s.fill_round_rect(Rect::new(0.0, 0.0, w, h), 4.0, bg);
            if let Some(b) = border {
                s.stroke_round_rect(Rect::new(0.0, 0.0, w, h), 4.0, b, 1.0);
            }
            s.draw_text(Point::new(8.0, 4.0), &text, 12.0, fg);
        }
    });
    n
}

#[derive(Clone, Copy, PartialEq)]
enum Btn {
    Primary,
    Secondary,
    Destructive,
    Ghost,
}

/// shadcn 按钮：四变体 + hover/pressed 反馈（Signal 驱动重绘）。
fn button(t: Tokens, label: &str, variant: Btn, on_click: impl Fn() + 'static) -> Node {
    let (w, h) = (96.0, 36.0);
    let label = label.to_string();
    let lw = wy_render::text_measure::measure_text(&label, 14.0).0;
    let pressed = Signal::new(false);
    let hover = Signal::new(false);
    let (base_bg, fg) = match variant {
        Btn::Primary => (t.primary, t.primary_fg),
        Btn::Secondary => (t.muted, t.foreground),
        Btn::Destructive => (t.destructive, t.destructive_fg),
        Btn::Ghost => (Color::TRANSPARENT, t.foreground),
    };
    let accent = t.accent;
    let has_border = variant == Btn::Secondary;
    let border = t.border;
    let p2 = pressed;
    let h2 = hover;

    let mut n = wy_mve::button(
        {
            let label = label.clone();
            move || label.clone()
        },
        on_click,
    );
    n.width = w;
    n.height = h;
    n.hit_test_fn = Rc::new(move |x, y| (0.0..w).contains(&x) && (0.0..h).contains(&y));
    n.draw_fn = Rc::new(move |scene| {
        if let Some(s) = scene.downcast_mut::<Scene>() {
            let mut bg = base_bg;
            if variant == Btn::Ghost && (h2.get() || p2.get()) {
                bg = accent;
            } else if h2.get() {
                bg = shade(bg, 0.92);
            }
            if p2.get() {
                bg = shade(bg, 0.85);
            }
            s.fill_round_rect(Rect::new(0.0, 0.0, w, h), 6.0, bg);
            if has_border {
                s.stroke_round_rect(Rect::new(0.0, 0.0, w, h), 6.0, border, 1.0);
            }
            let tx = (w - lw) / 2.0;
            s.draw_text(Point::new(tx, 10.0), &label, 14.0, fg);
        }
    });
    let pr = pressed;
    n.on_down_fn = Some(Rc::new(move |_| pr.set(true)));
    let pr2 = pressed;
    n.on_up_fn = Some(Rc::new(move |_| pr2.set(false)));
    n
}

/// 颜色缩放（hover/pressed 反馈）。
fn shade(c: Color, f: f32) -> Color {
    let s = |v: u8| ((v as f32) * f) as u8;
    Color::rgba(s(c.red()), s(c.green()), s(c.blue()), c.alpha())
}

/// shadcn 文本（自绘，可定字号/颜色）。
fn text_label(content: impl Fn() -> String + 'static, size: f32, color: Color) -> Node {
    let content = Rc::new(content);
    let mut n = wy_mve::text({
        let content = Rc::clone(&content);
        move || content()
    });
    n.draw_fn = Rc::new({
        let content = Rc::clone(&content);
        move |scene| {
            if let Some(s) = scene.downcast_mut::<Scene>() {
                let t = (content)();
                s.draw_text(Point::new(0.0, 0.0), &t, size, color);
            }
        }
    });
    n.measure_fn = Some(Rc::new(move || {
        wy_render::text_measure::measure_text(&(content)(), size)
    }));
    n
}

// ============================================================================
// 应用状态
// ============================================================================

#[derive(Clone, PartialEq)]
struct Todo {
    id: u32,
    text: String,
    done: bool,
}

struct App {
    theme_dark: Signal<bool>,
    name: Signal<String>,
    email: Signal<String>,
    message: Signal<String>,
    status: Signal<String>,
    todos: Signal<Vec<Todo>>,
    next_id: Signal<u32>,
    visits: Signal<u32>,
}

impl App {
    fn new() -> Self {
        Self {
            theme_dark: Signal::new(false),
            name: Signal::new(String::new()),
            email: Signal::new(String::new()),
            message: Signal::new(String::new()),
            status: Signal::new(String::from("填写表单后提交")),
            todos: Signal::new(vec![
                Todo {
                    id: 1,
                    text: String::from("设计系统 tokens"),
                    done: true,
                },
                Todo {
                    id: 2,
                    text: String::from("rsx 宏记录式参数"),
                    done: true,
                },
                Todo {
                    id: 3,
                    text: String::from("shadcn 仪表盘示例"),
                    done: false,
                },
            ]),
            next_id: Signal::new(4),
            visits: Signal::new(128),
        }
    }
}

// ============================================================================
// UI 构建
// ============================================================================

const NAV: [&str; 4] = ["仪表盘", "项目", "设置", "关于"];

fn sidebar(t: Tokens, app: &App) -> Node {
    let nav = app.theme_dark;
    let items: Vec<Node> = {
        let mut v: Vec<Node> = vec![
            text_label(move || String::from("▲ shadcn-mve"), 18.0, t.foreground),
            spacer(0.0, 20.0),
        ];
        for name in NAV {
            let name = name.to_string();
            let fg = t.muted_fg;
            let accent = t.accent;
            let mut item = wy_mve::button(
                {
                    let name = name.clone();
                    move || name.clone()
                },
                || {},
            );
            item.width = 180.0;
            item.height = 34.0;
            item.hit_test_fn =
                Rc::new(move |x, y| (0.0..180.0).contains(&x) && (0.0..34.0).contains(&y));
            item.draw_fn = Rc::new(move |scene| {
                if let Some(s) = scene.downcast_mut::<Scene>() {
                    s.fill_round_rect(Rect::new(0.0, 0.0, 180.0, 34.0), 6.0, accent);
                    s.draw_text(Point::new(12.0, 9.0), &name, 14.0, fg);
                }
            });
            v.push(item);
        }
        v.push(spacer(0.0, 20.0));
        let dark = nav;
        v.push(button(
            t,
            if dark.get() {
                "切换到浅色"
            } else {
                "切换到深色"
            },
            Btn::Ghost,
            move || nav.set(!dark.get()),
        ));
        v
    };
    let mut n = wy_mve::vbox(items);
    n.gap = 4.0;
    n
}

fn stat_card(t: Tokens, title: &str, value: &str, delta: &str) -> Node {
    let title = title.to_string();
    let value = value.to_string();
    let delta = delta.to_string();
    card(
        t,
        200.0,
        92.0,
        vec![
            text_label(move || title.clone(), 13.0, t.muted_fg),
            spacer(0.0, 6.0),
            wy_mve::hbox(vec![
                text_label(move || value.clone(), 24.0, t.foreground),
                spacer(8.0, 0.0),
                badge(t, &delta, 1),
            ]),
        ],
    )
}

fn input_field(t: Tokens, value: Signal<String>, placeholder: &str) -> Node {
    text_field_opts(
        move || value.get(),
        move |v| value.set(v),
        TextFieldOpts {
            width: 388.0,
            height: 36.0,
            placeholder: placeholder.to_string(),
            bg_color: t.input_bg,
            border_color: t.border,
            focus_border_color: t.ring,
            text_color: t.foreground,
            placeholder_color: t.muted_fg,
            ..Default::default()
        },
    )
}

fn form_card(t: Tokens, app: &App) -> Node {
    let (name, email, message) = (app.name, app.email, app.message);
    let (status, visits) = (app.status, app.visits);

    let submit = {
        let (name, email, status, visits) = (name, email, status, visits);
        button(t, "提交", Btn::Primary, move || {
            if name.get().trim().is_empty() || email.get().trim().is_empty() {
                status.set(String::from("✗ 姓名与邮箱必填"));
            } else if !email.get().contains('@') {
                status.set(String::from("✗ 邮箱格式不正确"));
            } else {
                status.set(format!("✓ 已提交：{} <{}>", name.get(), email.get()));
                visits.set(visits.get() + 1);
            }
        })
    };
    let reset = {
        let (name, email, message, status) = (name, email, message, status);
        button(t, "重置", Btn::Ghost, move || {
            name.set(String::new());
            email.set(String::new());
            message.set(String::new());
            status.set(String::from("已重置"));
        })
    };
    let status_text = status;

    card(
        t,
        420.0,
        372.0,
        vec![
            text_label(move || String::from("联系我们"), 18.0, t.foreground),
            spacer(0.0, 4.0),
            text_label(
                move || String::from("填写下面的表单，提交后状态会实时更新。"),
                13.0,
                t.muted_fg,
            ),
            spacer(0.0, 20.0),
            text_label(move || String::from("姓名"), 13.0, t.foreground),
            spacer(0.0, 6.0),
            input_field(t, name, "输入姓名"),
            spacer(0.0, 12.0),
            text_label(move || String::from("邮箱"), 13.0, t.foreground),
            spacer(0.0, 6.0),
            input_field(t, email, "you@example.com"),
            spacer(0.0, 12.0),
            text_label(move || String::from("留言"), 13.0, t.foreground),
            spacer(0.0, 6.0),
            input_field(t, message, "想说的话..."),
            spacer(0.0, 16.0),
            wy_mve::hbox(vec![submit, spacer(8.0, 0.0), reset]),
            spacer(0.0, 12.0),
            text_label(move || status_text.get(), 13.0, t.muted_fg),
        ],
    )
}

fn todo_row(t: Tokens, todos: Signal<Vec<Todo>>, todo: Todo) -> Node {
    let id = todo.id;
    let text = todo.text.clone();
    let done = todo.done;
    let del = button(t, "删除", Btn::Destructive, move || {
        let mut l = todos.get();
        l.retain(|x| x.id != id);
        todos.set(l);
    });
    let fg = if done { t.muted_fg } else { t.foreground };
    wy_mve::hbox(vec![
        badge(
            t,
            if done { "完成" } else { "进行中" },
            if done { 1 } else { 0 },
        ),
        spacer(8.0, 0.0),
        text_label(move || text.clone(), 14.0, fg),
        spacer(0.0, 0.0),
        del,
    ])
}

fn todos_card(t: Tokens, app: &App) -> Node {
    let todos = app.todos;
    let next_id = app.next_id;
    let input = Signal::new(String::new());
    let add = {
        let (todos, next_id, input) = (todos, next_id, input);
        button(t, "添加", Btn::Secondary, move || {
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
        })
    };

    let list = todos.get();
    let mut rows: Vec<Node> = Vec::new();
    for todo in &list {
        rows.push(todo_row(t, todos, todo.clone()));
        rows.push(spacer(0.0, 4.0));
    }
    if rows.is_empty() {
        rows.push(text_label(
            move || String::from("暂无待办，添加一个吧"),
            13.0,
            t.muted_fg,
        ));
    }

    card(
        t,
        420.0,
        260.0,
        vec![
            text_label(move || String::from("待办清单"), 18.0, t.foreground),
            spacer(0.0, 12.0),
            input_field(t, input, "新的待办..."),
            spacer(0.0, 8.0),
            add,
            spacer(0.0, 12.0),
            wy_mve::vbox(rows),
        ],
    )
}

fn dashboard(app: &App) -> Node {
    let dark = app.theme_dark;
    let t = if dark.get() {
        Tokens::DARK
    } else {
        Tokens::LIGHT
    };
    let visits = app.visits;

    // rsx! 声明式组装：容器用标签 + 属性，子节点平铺；非字面量属性/文本
    // 自动展开成 draw 期闭包（读信号自动追踪）。
    let node = rsx! {
        vbox {
            gap: 0.0,
            width: 1000.0,
            height: 800.0,
            hide: false,

            hbox {
                gap: 12.0,
                text_label(
                    move || {
                        if dark.get() { String::from("深色主题") } else { String::from("浅色主题") }
                    },
                    20.0,
                    t.foreground,
                ),
                badge(t, "v0.1.0", 3),
            }

            spacer(0.0, 24.0)

            hbox {
                gap: 24.0,
                sidebar(t, app),
                vbox {
                    gap: 16.0,
                    hbox {
                        gap: 16.0,
                        stat_card(t, "总访问", &visits.get().to_string(), "+12%"),
                        stat_card(t, "活跃用户", "42", "+4%"),
                        stat_card(t, "转化率", "7%", "-1%"),
                    },
                    form_card(t, app),
                    todos_card(t, app),
                }
            }
        }
    };
    let mut node = node.into_node();
    node.draw_fn = Rc::new(move |scene| {
        if let Some(s) = scene.downcast_mut::<Scene>() {
            s.fill_rect(Rect::new(0.0, 0.0, 2000.0, 2000.0), t.background);
        }
    });
    node
}

// ============================================================================
// 入口
// ============================================================================

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let app = App::new();
    let app = Rc::new(app);

    let app_handle = Rc::clone(&app);
    let mve = MveApp::new(move |cx: &mut NodeContext| {
        let node = dashboard(&app_handle);
        cx.child(node);
    });

    wy_engine::runner::run(mve)
}
