//! 侧边栏：品牌（头像 + 标题）、导航项（矢量图标）、设置区（深色开关）、
//! 对话框/下拉菜单触发器。
//!
//! 高度由主体行的 `align_item: Stretch` 拉伸到整窗高；底部按钮用 grow 占位
//! 节点推到底部。
//!
//! # 迁移要点
//!
//! - 旧 `node { width: W, height: H, draw: |s| .. }` → `node2! { width: W, height: H, draw: |s, n| .. }`
//! - 旧 `on_click_ev` 里 `ev.root_x` → 新 `mouse_click` 里 `e.global_x`
//!   （语义相同：都是相对画布根的全局坐标，减去节点局部坐标得到节点原点）
//! - 旧的 `grow spacer` 是 `spacer(0,0)` 再手动 `grow = 1.0`；新体系直接用
//!   一个只带 `exts: vec![grow(1.0)]` 的空节点

use wy_mve::{flex, grow, node2, Fc, Icon, Layout, Node2, Scene};
use wy_render::Rect;
use wy_signal::{GetValue, SetValue};

use crate::dashboard::SIDEBAR_W;
use crate::state::App;
use crate::tokens::Theme;
use crate::widgets::{draw_icon_at, separator, Btn};

/// 导航项：文案 + 矢量图标。
const NAV: [(&str, Icon); 4] = [
    ("仪表盘", Icon::Chart),
    ("项目", Icon::Folder),
    ("设置", Icon::Gear),
    ("关于", Icon::User),
];

/// 导航项行高（固定，不随侧栏高度变化）。
const NAV_H: f32 = 34.0;

/// 竖向间隔（替代旧 `spacer(0.0, h)`）。
fn vgap(h: f32) -> Fc {
    node2! { height: h }
}

pub(super) fn sidebar(theme: Theme, app: &App) -> Fc {
    let nav = app.theme_dark;

    // 品牌行：头像 + 标题
    let mut v: Vec<Fc> = vec![
        node2! {
            layout: flex! { direction: Layout::Row, gap: 8.0 },
            children: vec![
                avatar! { theme: theme, label: "M".to_string() },
                text_label! {
                    content: "shadcn-mve".to_string(),
                    font_size: 16.0,
                    color: theme.tokens().foreground,
                },
            ],
        },
        vgap(12.0),
        separator(theme, false),
        vgap(12.0),
    ];

    // 导航按钮：图标节点 + 文本节点（图标是矢量路径），位置交给 flex
    for (name, icon) in NAV {
        v.push(node2! {
            width: SIDEBAR_W,
            height: NAV_H,
            padding_inline_start: 12.0,
            layout: flex! {
                direction: Layout::Row,
                align_item: wy_mve::AlignItem::Center,
                align_fix: true,
                gap: 8.0,
            },
            draw: move |s: &mut Scene, n: &Node2| {
                s.fill_round_rect(Rect::new(0.0, 0.0, n.width(), n.height()), 6.0, theme.tokens().accent);
            },
            children: vec![
                node2! {
                    width: 16.0,
                    height: 16.0,
                    draw: move |s: &mut Scene, _n: &Node2| {
                        draw_icon_at(s, icon, 0.0, 0.0, 16.0, theme.tokens().muted_fg);
                    },
                },
                text_label! {
                    content: name.to_string(),
                    font_size: 14.0,
                    color: theme.tokens().muted_fg,
                },
            ],
        });
    }

    // grow 占位：把设置区推到底部
    v.push(node2! { exts: vec![grow(1.0)] });

    // 设置区：深色模式开关
    v.push(separator(theme, false));
    v.push(vgap(12.0));
    v.push(node2! {
        layout: flex! { direction: Layout::Row, gap: 8.0 },
        children: vec![
            text_label! {
                content: "深色模式".to_string(),
                color: theme.tokens().muted_fg,
            },
            node2! { exts: vec![grow(1.0)] },
            toggle! {
                theme: theme,
                checked: nav.get(),
                on_change: move |v: bool| nav.set(v),
            },
        ],
    });
    v.push(vgap(12.0));

    // 打开模态对话框
    let dialog_open = app.dialog_open;
    v.push(button_sized! {
        theme: theme,
        label: "打开对话框".to_string(),
        variant: Btn::Secondary,
        w: SIDEBAR_W,
        h: 36.0,
        on_click: move || dialog_open.set(true),
    });

    // 下拉菜单触发器：点击时捕获按钮的窗口坐标作为面板锚点
    let menu_open = app.menu_open;
    let menu_anchor = app.menu_anchor;
    v.push(vgap(4.0));
    v.push(node2! {
        width: SIDEBAR_W,
        height: NAV_H,
        mouse_click: move |e| {
            // 节点原点 = 事件全局坐标 − 节点局部坐标；宽高即按钮自身声明的尺寸。
            // 存成矩形（x, y, w, h），下拉面板据此做 8 方向放置与自动翻转。
            menu_anchor.set((
                e.global_x - e.x,
                e.global_y - e.y,
                SIDEBAR_W,
                NAV_H,
            ));
            menu_open.set(true);
        },
        padding_inline_start: 12.0,
        layout: flex! {
            direction: Layout::Row,
            align_item: wy_mve::AlignItem::Center,
            align_fix: true,
            gap: 8.0,
        },
        draw: move |s: &mut Scene, n: &Node2| {
            s.fill_round_rect(Rect::new(0.0, 0.0, n.width(), n.height()), 6.0, theme.tokens().muted);
        },
        children: vec![
            text_label! {
                content: "更多菜单".to_string(),
                font_size: 14.0,
                color: theme.tokens().foreground,
            },
            // 尖角（矢量，紧跟文案）
            node2! {
                width: 12.0,
                height: 12.0,
                draw: move |s: &mut Scene, _n: &Node2| {
                    draw_icon_at(s, Icon::ChevronDown, 0.0, 0.0, 12.0, theme.tokens().muted_fg);
                },
            },
        ],
    });

    node2! {
        debug_name: "sidebar".to_string(),
        width: SIDEBAR_W,
        layout: flex! {
            direction: Layout::Column,
            align_item: wy_mve::AlignItem::Stretch,
            align_fix: true,
            gap: 4.0,
        },
        children: v,
    }
}
