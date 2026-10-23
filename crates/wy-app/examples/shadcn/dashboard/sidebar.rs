//! 侧边栏：品牌（头像 + 标题）、导航按钮（带矢量图标）、设置区（深色模式开关）、
//! 对话框/下拉菜单触发器。
//!
//! 高度由主体行的 `align_items: Stretch` 拉伸到整窗高；底部按钮用
//! grow spacer 推到底部对齐。
//!
//! # 图标一律用矢量路径，不用字符
//!
//! 侧栏的导航图标、菜单的尖角等，早先是用 `"▾"`、`"▲"` 这类**字符**画的。
//! 字符能不能显示**取决于系统字体有没有该字形**——缺字形时渲染出来是
//! 空心方块（"豆腐块"），界面上就表现为"一堆方块"。
//! 这里改用 [`wy_mve::Icon`] 的自绘路径，与字体完全无关。

use wy_mve::{current_box, rsx, spacer, AlignItem, Icon, Node, PointerEvent, Slot};
use wy_render::{Point, Rect, Scene};
use wy_signal::{GetValue, SetValue};

use crate::state::App;
use crate::tokens::Theme;
use crate::widgets::{avatar, button, draw_icon_at, separator, text_label, toggle, Btn};

use super::SIDEBAR_W;

/// 导航项：文案 + 矢量图标。
const NAV: [(&str, Icon); 4] = [
    ("仪表盘", Icon::Chart),
    ("项目", Icon::Folder),
    ("设置", Icon::Gear),
    ("关于", Icon::User),
];

/// 导航项行高（固定，不随侧栏高度变化）。
const NAV_H: f32 = 34.0;

pub(super) fn sidebar(theme: Theme, app: &App) -> Node {
    let nav = app.theme_dark;
    let mut v: Vec<Slot> = Vec::new();

    // 品牌行：头像 + 标题
    v.push(
        rsx! {
            hbox {
                gap: 8.0,
                align_items: AlignItem::Center,
                avatar { theme: theme, label: "M" }
                text_label { content: "shadcn-mve", font_size: 16.0, color: move || theme.tokens().foreground }
            }
        }
        .into_node()
        .into(),
    );
    v.push(spacer(0.0, 12.0).into());
    v.push(rsx! { separator { theme: theme } }.into_node().into());
    v.push(spacer(0.0, 12.0).into());

    // 导航按钮：图标 + 文案（图标为矢量路径）
    for (name, icon) in NAV {
        let name = name.to_string();
        v.push(rsx! {
            node {
                width: SIDEBAR_W, height: NAV_H,
                draw: move |s: &mut Scene| {
                    let b = current_box();
                    let t = theme.tokens();
                    s.fill_round_rect(Rect::new(0.0, 0.0, b.width, b.height), 6.0, t.accent);
                    // 矢量图标（16px）
                    draw_icon_at(s, icon, 12.0, (NAV_H - 16.0) / 2.0, 16.0, t.muted_fg);
                    s.draw_text(Point::new(36.0, 9.0), &name, 14.0, t.muted_fg);
                },
            }
        });
    }

    // grow spacer：把设置区推到底部
    let mut push_down = spacer(0.0, 0.0);
    push_down.grow = 1.0;
    v.push(push_down.into());

    // 设置区：深色模式开关（受控 toggle，替代原 ghost 按钮）
    v.push(rsx! { separator { theme: theme } }.into_node().into());
    v.push(spacer(0.0, 12.0).into());
    v.push(
        rsx! {
            hbox {
                gap: 8.0,
                align_items: AlignItem::Center,
                text_label { content: "深色模式", color: move || theme.tokens().muted_fg }
                spacer(8.0, 0.0)
                toggle {
                    theme: theme,
                    checked: move || nav.get(),
                    on_change: move |v| nav.set(v),
                }
            }
        }
        .into_node()
        .into(),
    );
    v.push(spacer(0.0, 12.0).into());

    // 打开模态对话框
    let dialog_open = app.dialog_open;
    v.push(
        rsx! {
            button {
                theme: theme,
                width: SIDEBAR_W,
                label: "打开对话框",
                variant: Btn::Secondary,
                tag: "open-dialog",
                on_click: move || dialog_open.set(true),
            }
        }
        .into_node()
        .into(),
    );

    // 下拉菜单触发器：on_click_ev 捕获按钮窗口坐标作为面板锚点（下拉贴其下方）
    //
    // 面板位置由 `dropdown` 组件按可视区自动翻转——按钮在窗口底部时
    // 面板会向上弹，不会跑到屏幕外看不见。
    let menu_open = app.menu_open;
    let menu_anchor = app.menu_anchor;
    v.push(spacer(0.0, 4.0).into());
    let label = String::from("更多菜单");
    v.push(rsx! {
        node {
            width: SIDEBAR_W, height: NAV_H,
            tag: "more-menu",
            on_click_ev: move |ev: &mut PointerEvent| {
                // 按钮绝对坐标 = 事件窗口坐标 - 节点局部坐标
                menu_anchor.set((ev.root_x - ev.x, ev.root_y - ev.y));
                menu_open.set(true);
            },
            draw: move |s: &mut Scene| {
                let b = current_box();
                let t = theme.tokens();
                s.fill_round_rect(Rect::new(0.0, 0.0, b.width, b.height), 6.0, t.muted);
                s.draw_text(Point::new(12.0, 9.0), &label, 14.0, t.foreground);
                // 尖角（矢量，贴在文案右侧）
                let tw = wy_render::text_measure::measure_text(&label, 14.0).0;
                draw_icon_at(
                    s,
                    Icon::ChevronDown,
                    12.0 + tw + 8.0,
                    (NAV_H - 12.0) / 2.0,
                    12.0,
                    t.muted_fg,
                );
            },
        }
    });

    rsx! {
        vbox {
            gap: 4.0,
            align_items: AlignItem::Stretch,
            ..v
        }
    }
    .into_node()
}
