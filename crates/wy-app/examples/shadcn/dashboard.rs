//! UI 构建：页面组装、输入框工具、浮层（dialog / dropdown）。
//!
//! 树只构建一次；颜色经 [`Theme`] 在绘制期求值，主题切换自动重绘。
//! **响应式布局**：页面尺寸读窗口 `viewport`（与 TS 画布根同款：页面节点显式
//! 拿到画布宽高，不靠布局去猜），内部全部用 `grow` / `Stretch` 分配空间。
//!
//! 结构：页面（宽高 = 视口）→ 内容区（padding）→ 头部 / 统计行 / 表单与待办行
//! （grow 填满剩余高度）。
//!
//! # 浮层为什么单独返回
//!
//! `dialog` / `dropdown` 的尺寸取**父节点的 inner**（TS `innerSize(view, d)`）。
//! 如果把它们塞进页面节点，可视区就变成"页面内容区"（还要扣掉 `PAD`），遮罩
//! 盖不住整窗。所以它们必须直接挂在**画布根**下——[`overlays`] 返回 `Vec<Fc>`，
//! 由入口逐个 `cx.child(..)`，文档序最后 → 绘制在最上层、命中最先命中。

use std::cell::Cell;
use std::rc::Rc;

use wy_mve::canvas_render2::node2::Ext;
use wy_mve::canvas_render2::{dialog, dropdown, Placement, PlacementRect};
use wy_mve::{
    editable_text_node, flex, grow, node2, AlignItem, DirectionJustify, Fc, Layout, Node2, Scene,
};
use wy_render::Rect;
use wy_signal::{GetValue, SetValue, Signal};

use crate::form::form_card;
use crate::sidebar::sidebar;
use crate::state::App;
use crate::todos::todos_card;
use crate::tokens::Theme;
use crate::widgets::Btn;

/// 全局留白（页面四周）。
pub(super) const PAD: f32 = 24.0;
/// 侧栏宽度（固定值；高度随窗口拉伸）。
pub(super) const SIDEBAR_W: f32 = 160.0;

/// 文本输入框：外包一层容器负责「边框 + 抢宽度」，内层是编辑节点本身。
///
/// 为什么必须分两层：
/// - [`editable_text_node`] 的 `draw` 是**整段覆盖**的（TS `EditableTextNode`
///   末尾 `merge(p, ..)` 同样覆盖 `arg.draw`），传进去的背景/边框绘制会被丢掉；
/// - 它自己也声明了 `size`（= 内容宽），而 `declared` 优先于父级的 `Stretch`——
///   只靠交叉轴拉伸，输入框会缩成内容宽（视觉上"短一截"）。
///
/// 所以外层容器担两件事：画边框、抢父级的宽度份额（`exts` 由调用处给），并把
/// **自己的内容宽**回传给内层编辑器当 `width`。
///
/// 内边距刻意留在**编辑器自己**身上，不放在外层容器上：命中测试只认节点自己的
/// 矩形（`accept_hit`），padding 在外层时那 10px 属于"容器空白"，点上去命中链里
/// 没有可聚焦节点 → 焦点被清空，表现就是"点输入框边缘点不进去"。
pub(super) fn input_field(
    theme: Theme,
    value: Signal<String>,
    placeholder: &'static str,
    exts: Vec<Ext>,
) -> Fc {
    const PAD_X: f32 = 10.0;
    // 外层句柄：内层的 `width` 要读「外层内容宽」，所以先建外层、再挂内层。
    // `node2!` 在 build 后就写 ref，而 children 的 Fc 要等挂载才展开，读得到。
    let outer = wy_mve::Ref2::<Node2>::new();
    let inner_width = outer.clone();
    node2! {
        node_ref: outer,
        // 用占位文案当调试名：断言「这个框有没有撑开」时按它定位
        debug_name: placeholder.to_string(),
        height: 34.0,
        exts: exts,
        layout: flex! {
            direction: Layout::Row,
            align_item: AlignItem::Stretch,
            align_fix: true,
            // 主轴 `Start` 不是"靠左排"的意思，而是**让宽度改问父级分配**：
            // 默认的 `Grow` + 无 grow 子节点会让 `allowsSizeFromChildren` 为真，
            // 于是宽度退化成"内容宽"（内层编辑器自己声明了 size，此刻还是 0）。
            // 见 docs/07 第 4 条。
            direction_justify: DirectionJustify::Start,
        },
        draw: move |s: &mut Scene, n: &Node2| {
            let t = theme.tokens();
            let (w, h) = (n.width(), n.height());
            s.fill_round_rect(Rect::new(0.0, 0.0, w, h), 6.0, t.input_bg);
            s.stroke_round_rect(
                Rect::new(0.5, 0.5, (w - 1.0).max(0.0), (h - 1.0).max(0.0)),
                6.0,
                t.border,
                1.0,
            );
        },
        children: vec![
            editable_text_node! {
                text: value.get(),
                on_text_change: move |t: &str| value.set(t.to_string()),
                placeholder: placeholder.to_string(),
                single_line: true,
                height: 34.0,
                // 内边距归编辑器：整块矩形（含左右 10px）都属于它，点哪儿都能聚焦。
                padding_inline_start: PAD_X,
                padding_inline_end: PAD_X,
                // 编辑器宽度跟着外层内容区走（显式 `width` 才能盖过它自带的
                // `size`），否则会缩到"当前文本的宽度"。
                width: inner_width.get().width(),
                font_size: 13.0,
                color: theme.tokens().foreground,
            },
        ],
    }
}

/// 整页：尺寸 = 窗口视口，背景在 draw 期按自身几何画。
pub fn page(app: &App, viewport: Rc<Cell<(f32, f32)>>) -> Fc {
    let theme = Theme::new(app.theme_dark);
    let visits = app.visits;

    // 头部：标题（随主题切换文案）+ 版本徽章
    let header = node2! {
        layout: flex! { direction: Layout::Row, gap: 12.0, align_item: AlignItem::Center },
        children: vec![
            text_label! {
                content: if theme.dark.get() {
                    String::from("深色主题")
                } else {
                    String::from("浅色主题")
                },
                font_size: 20.0,
                color: theme.tokens().foreground,
            },
            badge! { theme: theme, text: "v0.1.0".to_string(), variant: 3u8 },
        ],
    };

    // 统计行：三张等宽卡（`grow(1.0)` + 父级 `Stretch` 等高）
    // 统计行**不写** `align_fix`：它是右侧列的主轴子项（无 grow），高度该由
    // 三张卡的内容撑开。写了反而会让 `height()` 回头问父分配，而父分配又要先
    // 量它的自然高 → 环闭合取 0，整行塌掉。
    let stats_row = node2! {
        layout: flex! { direction: Layout::Row, align_item: AlignItem::Stretch, gap: 16.0 },
        children: vec![
            stat_card! {
                theme: theme,
                title: String::from("总访问"),
                value: visits.get().to_string(),
                delta: String::from("+12%"),
                icon: wy_mve::Icon::Chart,
                exts: vec![grow(1.0)],
            },
            stat_card! {
                theme: theme,
                title: String::from("活跃用户"),
                value: String::from("42"),
                delta: String::from("+4%"),
                icon: wy_mve::Icon::User,
                exts: vec![grow(1.0)],
            },
            stat_card! {
                theme: theme,
                title: String::from("转化率"),
                value: String::from("7%"),
                delta: String::from("-1%"),
                icon: wy_mve::Icon::ArrowUp,
                exts: vec![grow(1.0)],
            },
        ],
    };

    // 表单/待办行：grow 填满剩余高度，`Stretch` 两卡等高
    let body_row = node2! {
        layout: flex! { direction: Layout::Row, align_item: AlignItem::Stretch, align_fix: true, gap: 16.0 },
        exts: vec![grow(1.0)],
        children: vec![form_card(theme, app), todos_card(theme, app)],
    };

    // 主体：侧栏（固定宽）+ 右侧内容（grow 吃掉剩余宽度）
    let main_row = node2! {
        layout: flex! { direction: Layout::Row, align_item: AlignItem::Stretch, align_fix: true, gap: 16.0 },
        exts: vec![grow(1.0)],
        children: vec![
            sidebar(theme, app),
            node2! {
                layout: flex! { direction: Layout::Column, align_item: AlignItem::Stretch, align_fix: true, gap: 16.0 },
                exts: vec![grow(1.0)],
                children: vec![stats_row, body_row],
            },
        ],
    };

    // 页面：宽高显式取视口（TS 画布根同款）；背景在 draw 里画
    let (vw, vh) = (viewport.clone(), viewport);
    node2! {
        debug_name: "page".to_string(),
        width: vw.get().0,
        height: vh.get().1,
        padding: PAD,
        layout: flex! {
            direction: Layout::Column,
            align_item: AlignItem::Stretch,
            align_fix: true,
            gap: 16.0,
        },
        draw: move |s: &mut Scene, n: &Node2| {
            let (w, h) = (n.width(), n.height());
            if w > 0.0 && h > 0.0 {
                s.fill_rect(Rect::new(0.0, 0.0, w, h), theme.tokens().background);
            }
        },
        children: vec![header, main_row],
    }
}

/// 浮层：模态对话框 + 下拉菜单（直接挂在画布根下，见模块文档）。
pub fn overlays(app: &App) -> Vec<Fc> {
    let theme = Theme::new(app.theme_dark);

    let dialog_open = app.dialog_open;
    let status = app.status;
    let confirm_dialog = dialog(
        move || dialog_open.get(),
        360.0,
        move || dialog_open.set(false),
        move || theme.tokens().card,
        move || theme.tokens().border,
        vec![
            text_label! {
                content: "删除确认".to_string(),
                font_size: 17.0,
                color: theme.tokens().foreground,
            },
            text_label! {
                content: "确认要清空当前看板数据吗？此操作不可撤销。".to_string(),
                color: theme.tokens().muted_fg,
            },
            node2! {
                layout: flex! {
                    direction: Layout::Row,
                    gap: 8.0,
                    direction_justify: DirectionJustify::End,
                },
                children: vec![
                    button! {
                        theme: theme,
                        label: "取消".to_string(),
                        variant: Btn::Ghost,
                        on_click: move || dialog_open.set(false),
                    },
                    button! {
                        theme: theme,
                        label: "确认清空".to_string(),
                        variant: Btn::Destructive,
                        on_click: move || {
                            status.set(String::from("已清空（演示）"));
                            dialog_open.set(false);
                        },
                    },
                ],
            },
        ],
    );

    let menu_open = app.menu_open;
    let menu_anchor = app.menu_anchor;
    let menu_status = app.status;
    let more_menu = dropdown(
        move || menu_open.get(),
        move || {
            let (x, y, w, h) = menu_anchor.get();
            PlacementRect::new(x, y, w, h)
        },
        || Placement::BottomStart,
        6.0,
        150.0,
        move || menu_open.set(false),
        move || theme.tokens().card,
        move || theme.tokens().border,
        vec![
            menu_item(theme, menu_status, menu_open, "重命名看板"),
            menu_item(theme, menu_status, menu_open, "导出数据"),
            menu_item(theme, menu_status, menu_open, "删除看板"),
        ],
    );

    vec![confirm_dialog, more_menu]
}

/// 下拉菜单项：整行可点，点击后关闭菜单并回显到状态栏。
fn menu_item(
    theme: Theme,
    status: Signal<String>,
    menu_open: Signal<bool>,
    label: &'static str,
) -> Fc {
    node2! {
        height: 30.0,
        padding_inline_start: 8.0,
        layout: flex! {
            direction: Layout::Row,
            align_item: AlignItem::Center,
            align_fix: true,
        },
        mouse_click: move |_e| {
            menu_open.set(false);
            status.set(format!("已选择：{label}"));
        },
        draw: move |s: &mut Scene, n: &Node2| {
            let t = theme.tokens();
            s.fill_round_rect(Rect::new(0.0, 0.0, n.width(), n.height()), 4.0, t.accent);
        },
        children: vec![text_label! {
            content: label.to_string(),
            font_size: 13.0,
            color: theme.tokens().foreground,
        }],
    }
}
