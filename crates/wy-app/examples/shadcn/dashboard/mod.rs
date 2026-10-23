//! UI 构建：顶层 `rsx!` 组装与输入框工具。
//!
//! 树只构建一次；颜色经 [`Theme`] 在绘制期求值，主题切换自动重绘。
//! **响应式布局**：根节点不声明尺寸（自动铺满窗口），内部全部用
//! `grow`/`Stretch` 分配空间——窗口缩放/最大化后布局自动跟随。
//!
//! 结构：根（铺满窗口）→ 内容区（padding + grow）→ 头部 / 统计行 /
//! 表单与待办行（grow 填满剩余高度）；对话框与下拉菜单以 `not_in_layout`
//! 浮层挂在根上（文档序最后 → 最上层，尺寸同样自动铺满）。
//!
//! 子模块按区块拆分：[`sidebar`]（侧栏）、[`stats`]（统计卡片）、
//! [`form`]（联系表单）、[`todos`]（待办清单）。

mod form;
mod sidebar;
mod stats;
mod todos;

use std::rc::Rc;

use wy_mve::{
    current_box, dialog, dropdown, rsx, text_field_opts, AlignItem, Node, TextFieldColors,
    TextFieldOpts,
};
use wy_render::{Point, Rect, Scene};
use wy_signal::{GetValue, SetValue, Signal};

use crate::state::App;
use crate::tokens::Theme;
use crate::widgets::{badge, button, text_label, Btn};

use form::form_card;
use sidebar::sidebar;
use stats::stat_card;
use todos::todos_card;

/// 全局留白（根内容区四周）。
pub(super) const PAD: f32 = 24.0;
/// 侧栏宽度（固定值；高度随窗口拉伸）。
pub(super) const SIDEBAR_W: f32 = 160.0;

/// 文本输入框：宽度交给父容器分配（声明宽 0 + `grow` 参数决定主轴伸缩；
/// 交叉轴由卡片 `Stretch` 拉伸），占位符与配色在绘制期读信号。
pub(super) fn input_field(
    theme: Theme,
    value: Signal<String>,
    placeholder: &str,
    grow: f32,
    tag: &'static str,
) -> Node {
    let mut node = text_field_opts(
        move || value.get(),
        move |v| value.set(v),
        TextFieldOpts {
            width: 0.0,
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
    );
    node.grow = grow;
    node.tag = Some(tag);
    node
}

pub fn dashboard(app: &App) -> Node {
    let theme = Theme::new(app.theme_dark);
    let visits = app.visits;
    // Signal 是 Copy：先拷出句柄，闭包只捕获句柄（`app` 引用进不了 'static 闭包）
    let dialog_open = app.dialog_open;
    let menu_open = app.menu_open;
    let menu_anchor = app.menu_anchor;
    let status = app.status;

    rsx! {
        // 根：不声明尺寸 → 自动铺满窗口；align Stretch 让内容区横向撑满
        node {
            gap: 0.0,
            align_items: AlignItem::Stretch,
            draw: move |s: &mut Scene| {
                let b = current_box();
                s.fill_rect(Rect::new(0.0, 0.0, b.width, b.height), theme.tokens().background);
            },

            // 内容区：四周留白 + grow 纵向撑满窗口；align Stretch 让子项横向铺满
            node {
                padding: PAD,
                gap: 16.0,
                grow: 1.0,
                align_items: AlignItem::Stretch,

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
                        font_size: 20.0,
                        color: move || theme.tokens().foreground,
                    }
                    badge { theme: theme, text: "v0.1.0", variant: 3 }
                }

                // 主体：侧栏（固定宽）+ 右侧内容（grow 吃掉剩余宽度）；
                // 行 grow 纵向撑满剩余窗口高度，align Stretch 让子项等高
                hbox {
                    gap: 16.0,
                    grow: 1.0,
                    align_items: AlignItem::Stretch,

                    sidebar(theme, app),

                    vbox {
                        gap: 16.0,
                        grow: 1.0,
                        align_items: AlignItem::Stretch,

                        hbox {
                            gap: 16.0,
                            align_items: AlignItem::Stretch,
                            stat_card { theme: theme, grow: 1.0, size: (0.0, 0.0), icon: wy_mve::Icon::Chart, title: String::from("总访问"), value: move || visits.get().to_string(), delta: String::from("+12%") }
                            stat_card { theme: theme, grow: 1.0, size: (0.0, 0.0), icon: wy_mve::Icon::User, title: String::from("活跃用户"), value: move || String::from("42"), delta: String::from("+4%") }
                            stat_card { theme: theme, grow: 1.0, size: (0.0, 0.0), icon: wy_mve::Icon::ArrowUp, title: String::from("转化率"), value: move || String::from("7%"), delta: String::from("-1%") }
                        }

                        // 表单/待办行：grow 填满剩余高度，align Stretch 两卡等高——
                        // 底边对齐、间隔一致，不再"行高参差"。
                        // `size: (0, 0)`（flex-basis 0）：卡片尺寸完全交给 grow 均分，
                        // 不受内容自然宽度差影响。
                        hbox {
                            gap: 16.0,
                            grow: 1.0,
                            align_items: AlignItem::Stretch,
                            form_card(theme, app),
                            todos_card(theme, app),
                        }
                    }
                }
            }

            // ===== 浮层（not_in_layout 脱离布局流，文档序最后 → 最上层；
            //       未声明尺寸 → 自动撑满父容器/窗口，随窗口缩放跟随） =====

            // 模态对话框：遮罩 + 居中面板；Esc / 点遮罩关闭，点面板不关
            dialog {
                open: move || dialog_open.get(),
                panel_width: 360.0,
                on_close: move || dialog_open.set(false),
                surface: move || theme.tokens().card,
                border: move || theme.tokens().border,

                text_label { content: "删除确认", font_size: 17.0, color: move || theme.tokens().foreground }
                text_label { content: "确认要清空当前看板数据吗？此操作不可撤销。", color: move || theme.tokens().muted_fg }
                hbox {
                    gap: 8.0,
                    justify: wy_mve::DirectionJustify::End,
                    button {
                        theme: theme,
                        label: "取消",
                        variant: Btn::Ghost,
                        on_click: move || dialog_open.set(false),
                    }
                    button {
                        theme: theme,
                        label: "确认清空",
                        variant: Btn::Destructive,
                        on_click: move || {
                            status.set(String::from("已清空（演示）"));
                            dialog_open.set(false);
                        },
                    }
                }
            }

            // 下拉菜单：面板锚定"更多菜单"按钮下方（点击时捕获坐标）
            dropdown {
                open: move || menu_open.get(),
                anchor_x: move || menu_anchor.get().0,
                anchor_y: move || menu_anchor.get().1 + 38.0,
                panel_width: 150.0,
                on_close: move || menu_open.set(false),
                surface: move || theme.tokens().card,
                border: move || theme.tokens().border,

                menu_item(theme, app, "重命名看板")
                menu_item(theme, app, "导出数据")
                menu_item(theme, app, "删除看板")
            }
        }
    }
    .into_node()
}

/// 下拉菜单项：整行可点，点击后关闭菜单并回显到状态栏。
fn menu_item(theme: Theme, app: &App, label: &str) -> Node {
    let click_text = label.to_string();
    let draw_text = click_text.clone();
    let menu_open = app.menu_open;
    let status = app.status;
    rsx! {
        node {
            height: 30.0,
            on_click: move || {
                menu_open.set(false);
                status.set(format!("已选择：{click_text}"));
            },
            draw: move |s: &mut Scene| {
                let t = theme.tokens();
                let b = current_box();
                s.fill_round_rect(Rect::new(0.0, 0.0, b.width, b.height), 4.0, t.accent);
                s.draw_text(Point::new(8.0, 7.0), &draw_text, 13.0, t.foreground);
            },
        }
    }
    .into_node()
}
