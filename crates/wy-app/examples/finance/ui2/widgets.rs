//! 基础组件：面板外壳、表头行、文本、小按钮、键值行、列单元格。
//!
//! 全部走 v2 原语：容器是 `node2!`、文本一律 `wrapped_text_node!` 子节点
//! （由布局撑尺寸，绝不 `Scene::draw_text` 手绘）、列表用 `for_each_node` +
//! `scroll_content!` 三段拼装。
//!
//! 视觉语言：8px 圆角面板、35px 标题栏、1px 边框、斑马纹行、表头用 FAINT 小字。

use std::rc::Rc;

use wy_mve::canvas_render2::create_scroll;
use wy_mve::canvas_render2::node2::{Axis, Ext};
use wy_mve::canvas_render2::protocol::MouseEvent2;
use wy_mve::{
    flex, grow, grow_dyn, node2, node_of, rec, scroll_bar, scroll_content, wrapped_text_node,
    AlignItem, CursorType, DirectionJustify, Fc, Layout, Node2, Node2Props, Ref2,
};
use wy_render::{Color, Rect, Scene};
use wy_signal::{GetValue, SetValue, Signal};

use crate::theme;

/// 面板标题栏高度。
pub const HEADER_H: f32 = theme::HEADER_H;
/// 面板内边距。
pub const PAD: f32 = theme::PAD;
/// 表头行高度。
pub const HEADER_ROW_H: f32 = 20.0;
/// 盘口 / 成交三列的列边界比例（与 [`col_header`] 的 `cells` 同一套）：
/// 价格左对齐、数量收在 60%、总额贴右。
pub const COLS_3: (f32, f32, f32) = (0.0, 0.60, 1.0);

/// 纯文本节点：字号/颜色绘制期求值，读信号自动追踪。
#[rec]
pub fn text_label(
    #[getter]
    #[props(required)]
    content: String,
    #[props(default = 13.0f32)] font_size: f32,
    #[getter]
    #[props(required)]
    color: Color,
    #[getter]
    #[props(default = false)]
    hide: bool,
) -> Fc {
    wrapped_text_node! {
        text: content(),
        font_size: font_size,
        color: color(),
        hide: hide(),
        auto_width: true,
    }
}

/// 面板外壳：圆角底 + 边框 + 标题栏，内容区自动占据剩余高度。
///
/// 标题、徽标、动作区是 `layout` 里的兄弟节点：`[标题(grow:1)] [徽标] [动作…]`。
/// 三者的左右位置由布局保证，不会出现"按钮压在徽标文字上"。
///
/// 调用处把 `exts: vec![grow_dyn(..)]` / `width` 等节点属性一并传进来即可
/// （`extends = node2` 转发 `rest`）。内容子节点经 `rest.children` 注入。
#[rec(extends = node2)]
pub fn panel(title: Rc<dyn Fn() -> String>, badge: Rc<dyn Fn() -> String>, actions: Vec<Fc>) -> Fc {
    // `rest.children` 要交给内容区，而 `..rest` 会整体消耗 `rest`。先把子节点
    // 原地挪出来（留一个空 Vec），`rest` 本身仍完好，之后照样能交给 `..rest`。
    // `Fc` 是 `FnOnce` 盒、没有 `Clone`，所以只能搬不能拷。
    let mut rest = rest;
    let content_children = std::mem::take(&mut rest.children);

    let title_c = Rc::clone(&title);
    let badge_c = Rc::clone(&badge);
    let badge_hide = Rc::clone(&badge);

    let title_cell = text_label! {
        content: title_c(),
        font_size: 12.5,
        color: theme::TEXT,
    };
    let badge_cell = text_label! {
        content: badge_c(),
        font_size: 9.5,
        color: theme::MUTED,
        hide: badge_hide().is_empty(),
    };

    let mut header_items: Vec<Fc> = vec![
        // 两个 wrapper 都必须**有 layout**：`node2!` 不给 `layout` 就是绝对定位，
        // 里面的文本会被钉在 (0,0) 且自身尺寸取不到（高 0），于是标题既不在
        // 垂直中线上、也撑不开 header 的行高。Row + Center 让它们自己量出高度。
        node2! {
            debug_name: "panel-title".to_string(),
            layout: flex! { direction: Layout::Row, align_item: AlignItem::Center },
            exts: vec![grow(1.0)],
            children: vec![title_cell],
        },
        node2! {
            layout: flex! { direction: Layout::Row, align_item: AlignItem::Center },
            children: vec![badge_cell],
        },
    ];
    header_items.extend(actions);

    let header = node2! {
        // 定高标题栏：`align_fix: true` 让标题/徽标/动作以**整条 36px 栏高**为
        // 居中基准，否则基准退化成最高子节点（普通小字 ~14、chip ~20），内容会
        // 贴顶、整体偏上 ~9px。参考 Kotlin `ButtonBase` / 任一固定高 Row 的写法。
        layout: flex! { direction: Layout::Row, align_item: AlignItem::Center, gap: 6.0, align_fix: true },
        height: HEADER_H,
        padding_inline_start: PAD,
        padding_inline_end: PAD,
        // 不能写 `skip_draw`：引擎的 `render_node2` 对 `skip_draw` 的子节点是
        // **整棵子树**一起跳过（`continue` 在递归之前），标题和徽标会一起消失。
        // 本节点本来就没给 `draw`，不写就等于不画自己。
        children: header_items,
    };
    let content = node2! {
        layout: flex! { direction: Layout::Column, align_item: AlignItem::Stretch, align_fix: true },
        exts: vec![grow(1.0)],
        children: content_children,
    };

    node2! {
        ..rest,
        layout: flex! { direction: Layout::Column, align_item: AlignItem::Stretch, align_fix: true },
        // 只画底/边框/标题栏分隔线；标题与徽标由上面的兄弟节点自己画
        draw: move |s: &mut Scene, n: &Node2| {
            theme::panel_bg(s, n.width(), n.height());
        },
        children: vec![header, content],
    }
}

/// 表头行：列标题 + 底部细线。
///
/// `cells` 里每项是 `(标题, 右边界比例)`；比例 `0.0` 表示左对齐。
/// 行组件与表头用同一套比例，列才对得齐。
#[rec]
pub fn col_header(cells: Vec<(&'static str, f32)>) -> Fc {
    let mut prev = 0.0f32;
    let mut items: Vec<Fc> = Vec::new();
    // 直接消费整条 `cells`：迭代项是 `(&'static str, f32)` 两个 Copy 值，不像
    // `.iter()` 那样带着对 `cells` 的借用——文本 getter 要存成 `'static` 闭包，
    // 借用一旦进闭包就会逃逸出函数体。
    for (label, at) in cells {
        let text = text_label! {
            content: label.to_string(),
            font_size: 9.5,
            color: theme::FAINT,
        };
        if at <= 0.0 {
            prev = 0.0;
            items.push(node2! {
                layout: flex! { direction: Layout::Row, direction_justify: DirectionJustify::Start },
                children: vec![text],
            });
        } else {
            let frac = at - prev;
            prev = at;
            items.push(node2! {
                exts: vec![grow_dyn(move |d| if d == Axis::X { frac } else { 0.0 })],
                layout: flex! { direction: Layout::Row, direction_justify: DirectionJustify::End },
                children: vec![text],
            });
        }
    }
    node2! {
        debug_name: "col-header".to_string(),
        height: HEADER_ROW_H,
        // 外层**要有 layout**：`node2!` 不给 `layout` 就是绝对定位，里面那层
        // 内缩容器会被钉在 (0,0) 且量不到尺寸（宽高 0），列全部塌到同一点。
        // 这里是「外层画满整行宽的分隔线、内层负责 PAD 内的列排布」。
        layout: flex! { direction: Layout::Row, align_item: AlignItem::Stretch, align_fix: true },
        // 分隔线画满整行（贴面板边框），列内容再单独内缩 PAD——
        // 与 v1 `theme::header_row` 的 `1.0 .. w-1.0` + `PAD` 起排一致。
        draw: move |s: &mut Scene, n: &Node2| {
            s.fill_rect(
                Rect::new(1.0, n.height() - 1.0, (n.width() - 2.0).max(0.0), 1.0),
                theme::BORDER,
            );
        },
        children: vec![ node2! {
            exts: vec![grow(1.0)],
            padding_inline_start: PAD,
            padding_inline_end: PAD,
            layout: flex! { direction: Layout::Row, align_item: AlignItem::Center, align_fix: true, gap: 0.0 },
            children: items,
        } ],
    }
}

/// 一个列单元格：文本 / 字号 / 颜色，全部绘制期求值。
///
/// 列边界语义与 [`col_header`] 完全一致：`at` 是右边界占内容宽的比例
/// （`0.0` = 左对齐），所以表头与数据行永远对齐。
pub struct ColCell {
    /// 右边界比例（0.0 = 左对齐）。
    pub at: f32,
    /// 文本（getter）。
    pub text: Rc<dyn Fn() -> String>,
    /// 字号。
    pub size: f32,
    /// 颜色（getter）。
    pub color: Rc<dyn Fn() -> Color>,
}

impl ColCell {
    /// 左对齐列（价格 / 符号）。
    pub fn left(
        text: impl Fn() -> String + 'static,
        size: f32,
        color: impl Fn() -> Color + 'static,
    ) -> Self {
        Self {
            at: 0.0,
            text: Rc::new(text),
            size,
            color: Rc::new(color),
        }
    }

    /// 右对齐到「内容宽 × `at`」的列（数量 / 总额 / 时间）。
    pub fn right(
        at: f32,
        text: impl Fn() -> String + 'static,
        size: f32,
        color: impl Fn() -> Color + 'static,
    ) -> Self {
        Self {
            at,
            text: Rc::new(text),
            size,
            color: Rc::new(color),
        }
    }
}

/// 按列比例生成一行内的列节点（`theme::header_row` 的行版）。
///
/// 每列是一个 flex 子项：左列按内容宽、右列按 `at` 占比并把文本推到该段右缘。
/// 调用方把返回的 `Vec<Fc>` 直接作为行的 `children` 即可。
pub fn cols_row(cells: &[ColCell]) -> Vec<Fc> {
    let mut prev = 0.0f32;
    cells
        .iter()
        .map(|c| {
            // `c` 是对调用方数组的借用：只把里面的 `Rc` 克隆出来带进 getter，
            // 借用本身绝不能进——存成 `'static` 闭包会逃出本函数。
            let t = Rc::clone(&c.text);
            let tone = Rc::clone(&c.color);
            let (at, size) = (c.at, c.size);
            let text = wrapped_text_node! {
                text: t(),
                font_size: size,
                color: tone(),
                auto_width: true,
            };
            if at <= 0.0 {
                prev = 0.0;
                node2! {
                    layout: flex! { direction: Layout::Row, direction_justify: DirectionJustify::Start },
                    children: vec![text],
                }
            } else {
                let frac = at - prev;
                prev = at;
                node2! {
                    exts: vec![grow_dyn(move |d| if d == Axis::X { frac } else { 0.0 })],
                    layout: flex! { direction: Layout::Row, direction_justify: DirectionJustify::End },
                    children: vec![text],
                }
            }
        })
        .collect()
}

/// 标准列表行：定高 + 悬停/点击 + 选中高亮。
///
/// 行高**固定**（对齐参考实现）；`children` 是行内容（已按列排好的文本节点）。
/// 悬停底色与选中强调由内部两个信号驱动，绘制期求值。
pub fn list_row(
    h: f32,
    on_hover: impl Fn(bool) + 'static,
    on_click: impl Fn() + 'static,
    children: Vec<Fc>,
    tag: &'static str,
    highlight: impl Fn() -> bool + 'static,
) -> Fc {
    let hover = Signal::new(false);
    let hover_read = hover.clone();
    let hl = Rc::new(highlight);

    node2! {
        debug_name: tag.to_string(),
        height: h,
        on_hover: move |on| {
            hover.set(on);
            on_hover(on);
        },
        mouse_click: move |_e: &mut MouseEvent2| on_click(),
        layout: flex! { direction: Layout::Column, align_item: AlignItem::Stretch, align_fix: true, direction_justify: DirectionJustify::Center },
        draw: move |s: &mut Scene, n: &Node2| {
            let (w, h) = (n.width(), n.height());
            if hover_read.get() {
                s.fill_round_rect(Rect::new(2.0, 1.0, (w - 4.0).max(0.0), (h - 2.0).max(0.0)), 5.0, theme::ROW_ALT);
            } else if hl() {
                s.fill_round_rect(Rect::new(2.0, 1.0, (w - 4.0).max(0.0), (h - 2.0).max(0.0)), 5.0, theme::ACCENT_SOFT);
            }
        },
        children: {
            // 行内容统一内缩 PAD（列文字与表头对齐）。高亮/斑马纹仍按**整行宽**画，
            // 所以背景留在这一层、不跟着缩。
            vec![ node2! {
                exts: vec![grow(1.0)],
                padding_inline_start: PAD,
                padding_inline_end: PAD,
                layout: flex! {
                    direction: Layout::Column,
                    align_item: AlignItem::Stretch,
                    align_fix: true,
                    direction_justify: DirectionJustify::Center,
                },
                children: children,
            } ]
        },
    }
}

/// 竖向权重：`stacked` 里上下两格按同一个比例分高度（`a` / `1-a`）。
///
/// 面板自己不接 `exts`（`panel!` 的 `exts` 只能从调用处给），所以权重得由
/// 调用方算好传进去——比例是**信号**，拖动分割条时它变、布局跟着重排。
pub fn grow_y(split: Signal<f32>, top: bool) -> Vec<Ext> {
    vec![grow_dyn(move |d| {
        if d != Axis::Y {
            return 0.0;
        }
        let v = split.get();
        if top {
            v
        } else {
            (1.0 - v).max(0.0)
        }
    })]
}

/// 横向权重：三列按**占比**分宽度（左 `a` / 中 `1-a-b` / 右 `b`）。
///
/// flex 的 grow 分支给的是 `remaining * 权重 / 权重和`，不含子项自然尺寸，
/// 所以只要三列权重和为 1，宽度就精确等于「行宽 × 占比」——不需要任何像素常量。
pub fn grow_x(frac: impl Fn() -> f32 + 'static) -> Vec<Ext> {
    vec![grow_dyn(move |d| {
        if d != Axis::X {
            return 0.0;
        }
        frac().max(0.0)
    })]
}

/// 吃满主轴（FULL 视图里唯一那格）。
pub fn grow_all() -> Vec<Ext> {
    vec![grow(1.0)]
}

/// 弹性空白：吃掉主轴剩余空间。
pub fn flex_spacer() -> Fc {
    node2! { exts: vec![grow(1.0)], skip_draw: true }
}

/// 竖线分隔（顶栏分组之间用）。
pub fn vsep() -> Fc {
    node2! {
        width: 1.0,
        height: 16.0,
        draw: move |s: &mut Scene, n: &Node2| {
            s.fill_rect(Rect::new(0.0, 0.0, 1.0, n.height()), theme::BORDER);
        },
    }
}

/// 标签按钮：选中态 / 悬停态自己画，点击回调由调用方给。
pub fn tab_of(
    label: Rc<dyn Fn() -> String>,
    active: Rc<dyn Fn() -> bool>,
    on_pick: Rc<dyn Fn()>,
) -> Fc {
    const H: f32 = 21.0;
    let hover = Signal::new(false);
    let w = (theme::width(&label(), 10.0) + 16.0).max(28.0);

    // 每个 getter / 回调闭包各拿一份句柄：同一个 `Rc` 被两处捕获就得多给一份
    // （`active` 要给 draw 和 color，`label` 要给宽计算和文本）。
    let active_draw = Rc::clone(&active);
    let hover_draw = hover.clone();
    let label_text = Rc::clone(&label);
    let active_color = Rc::clone(&active);

    node2! {
        debug_name: "tab".to_string(),
        width: w,
        height: H,
        cursor: CursorType::Pointer,
        on_hover: move |on| hover.set(on),
        mouse_click: move |_e: &mut MouseEvent2| on_pick(),
        // 定高按钮必须 `align_fix: true`：交叉轴居中基准取**按钮自身高**（H），
        // 否则取"最高子节点"（文字行高 ~14），文字会贴顶、看着偏上 3~4px
        // （参考 Kotlin `ButtonBase`：`direction: Row + alignItem: center + alignFix: true`）。
        layout: flex! { direction: Layout::Row, align_item: AlignItem::Center, direction_justify: DirectionJustify::Center, align_fix: true },
        draw: move |s: &mut Scene, n: &Node2| {
            let on = active_draw();
            if on {
                s.fill_round_rect(Rect::new(0.0, 0.0, n.width(), H), 4.0, theme::ACCENT_SOFT);
                s.stroke_round_rect(Rect::new(0.5, 0.5, n.width() - 1.0, H - 1.0), 4.0, theme::ACCENT, 1.0);
            } else if hover_draw.get() {
                s.fill_round_rect(Rect::new(0.0, 0.0, n.width(), H), 4.0, theme::ROW_ALT);
            }
        },
        children: vec![text_label! {
            content: label_text(),
            font_size: 10.0,
            color: { let a = Rc::clone(&active_color); if a() { theme::TEXT } else { theme::MUTED } },
        }],
    }
}

/// 标签按钮（标题静态）。
pub fn tab_btn(
    label: &'static str,
    active: impl Fn() -> bool + 'static,
    on_pick: impl Fn() + 'static,
) -> Fc {
    tab_of(
        Rc::new(move || String::from(label)),
        Rc::new(active),
        Rc::new(on_pick),
    )
}

/// 方块按钮的共用外观：`(底色, 描边色)`。
///
/// 对外暴露是因为有文案随状态变化的按钮（深度面板的「FULL / CLOSE」）要
/// 自己构造节点，但必须和 [`chip_custom`] 长得一模一样。
pub fn chip_bg(hot: bool, active: bool) -> (Color, Color) {
    if active {
        (theme::ACCENT_SOFT, theme::ACCENT)
    } else if hot {
        (theme::ROW_ACTIVE, theme::HANDLE)
    } else {
        (theme::ROW_ALT, theme::BORDER)
    }
}

/// 通用方块按钮：底色 + 描边 + 居中文字（文字走 `text_label!` 子节点）。
///
/// `hide` 是**可选的显隐**（K 线的「LATEST」只在往右翻过之后才出现）：
/// 写死 `|| false` 就是常显。放在这里而不是外面再包一层，是因为包一层会多出
/// 一个节点、高度还要重新对齐——按钮本来就该能自己隐藏。
pub fn chip_custom(
    w: f32,
    base: Color,
    active: Rc<dyn Fn() -> bool>,
    on_click: Rc<dyn Fn()>,
    hide: Rc<dyn Fn() -> bool>,
    glyph: &'static str,
) -> Fc {
    const H: f32 = 20.0;
    let hover = Signal::new(false);
    // 同 `tab_of`：`active` / `hover` 要给 draw 和 color 两处，各备一份。
    let active_draw = Rc::clone(&active);
    let hover_draw = hover.clone();
    let active_color = Rc::clone(&active);
    let hover_color = hover.clone();

    node2! {
        width: w,
        height: H,
        hide: hide(),
        cursor: CursorType::Pointer,
        on_hover: move |on| hover.set(on),
        mouse_click: move |_e: &mut MouseEvent2| on_click(),
        // 定高按钮：`align_fix: true` 让交叉轴居中以按钮自身高为基准（同 `tab_of`）。
        layout: flex! { direction: Layout::Row, align_item: AlignItem::Center, direction_justify: DirectionJustify::Center, align_fix: true },
        draw: move |s: &mut Scene, n: &Node2| {
            let on = active_draw();
            let (bg, edge) = chip_bg(hover_draw.get(), on);
            s.fill_round_rect(Rect::new(0.0, 0.0, n.width(), H), 4.0, bg);
            s.stroke_round_rect(Rect::new(0.5, 0.5, n.width() - 1.0, H - 1.0), 4.0, edge, 1.0);
        },
        // getter 写裸表达式（宏自己包 `move ||`），不能再套一层闭包；
        // `active_color` / `hover_color` 是专供这里的句柄，直接捕获即可。
        children: vec![ text_label! {
            content: glyph.to_string(),
            font_size: 9.5,
            color: if active_color() || hover_color.get() { theme::TEXT } else { base },
        } ],
    }
}

/// 方块按钮（字符字形）。
pub fn chip(label: &'static str, w: f32, base: Color, on_click: impl Fn() + 'static) -> Fc {
    chip_custom(
        w,
        base,
        Rc::new(|| false),
        Rc::new(on_click),
        Rc::new(|| false),
        label,
    )
}

/// 同 [`chip`]，但带选中态（图表缩放档位、盘口模式）。
pub fn chip_on(
    label: &'static str,
    w: f32,
    base: Color,
    active: impl Fn() -> bool + 'static,
    on_click: impl Fn() + 'static,
) -> Fc {
    chip_custom(
        w,
        base,
        Rc::new(active),
        Rc::new(on_click),
        Rc::new(|| false),
        label,
    )
}

/// 小方按钮（图标字形）。
pub fn icon_btn(label: &'static str, w: f32, on_click: impl Fn() + 'static) -> Fc {
    chip(label, w, theme::MUTED, on_click)
}

/// 更多操作按钮（自绘三点，形状不是文字）：点击回调带事件。
///
/// 需要事件是因为下拉菜单的锚点必须是**触发按钮的窗口矩形**，只有事件期才拿得到
/// （`global_x - x` = 按钮原点；宽高是常量，直接带上）。
pub fn dots_btn_ev(
    tag: &'static str,
    w: f32,
    active: impl Fn() -> bool + 'static,
    on_click: impl Fn(&mut MouseEvent2) + 'static,
) -> Fc {
    const H: f32 = 20.0;
    let hover = Signal::new(false);
    // draw 里要读两次 hover，而 `on_hover` 已经把 `hover` 搬走了：先留一份。
    let hov = hover.clone();
    node2! {
        debug_name: tag.to_string(),
        width: w,
        height: H,
        cursor: CursorType::Pointer,
        on_hover: move |on| hover.set(on),
        mouse_click: move |e: &mut MouseEvent2| on_click(e),
        draw: move |s: &mut Scene, n: &Node2| {
            let on = active();
            let (bg, edge) = chip_bg(hov.get(), on);
            s.fill_round_rect(Rect::new(0.0, 0.0, n.width(), H), 4.0, bg);
            s.stroke_round_rect(Rect::new(0.5, 0.5, n.width() - 1.0, H - 1.0), 4.0, edge, 1.0);
            let color = if on || hov.get() { theme::TEXT } else { theme::MUTED };
            let (cx, cy) = (n.width() / 2.0, H / 2.0);
            for k in -1..=1 {
                s.fill_round_rect(Rect::new(cx + k as f32 * 4.5 - 1.0, cy - 1.0, 2.0, 2.0), 1.0, color);
            }
        },
    }
}

/// 小开关（盘口设置条用）：选中态描边高亮，文字走 `text_label!`。
pub fn toggle_chip(
    label: &'static str,
    active: impl Fn() -> bool + 'static,
    on_click: impl Fn() + 'static,
) -> Fc {
    const H: f32 = 17.0;
    let w = (theme::width(label, 9.0) + 14.0).max(34.0);
    let hover = Signal::new(false);
    let lbl = label;
    // draw 与 color 两处都要读 `active`：`impl Fn` 没法克隆，先包一层 `Rc` 再分份。
    let active: Rc<dyn Fn() -> bool> = Rc::new(active);
    let active_draw = Rc::clone(&active);
    let active_color = Rc::clone(&active);
    let hover_draw = hover.clone();

    node2! {
        width: w,
        height: H,
        cursor: CursorType::Pointer,
        on_hover: move |on| hover.set(on),
        mouse_click: move |_e: &mut MouseEvent2| on_click(),
        // 定高按钮：`align_fix: true` 让交叉轴居中以按钮自身高为基准（同 `tab_of`）。
        layout: flex! { direction: Layout::Row, align_item: AlignItem::Center, direction_justify: DirectionJustify::Center, align_fix: true },
        draw: move |s: &mut Scene, n: &Node2| {
            let on = active_draw();
            let bg = if on {
                theme::ACCENT_SOFT
            } else if hover_draw.get() {
                theme::ROW_ACTIVE
            } else {
                theme::ROW_ALT
            };
            s.fill_round_rect(Rect::new(0.0, 0.0, n.width(), H), 3.0, bg);
            s.stroke_round_rect(Rect::new(0.5, 0.5, n.width() - 1.0, H - 1.0), 3.0, if on { theme::ACCENT } else { theme::BORDER }, 1.0);
        },
        children: vec![ text_label! {
            content: lbl.to_string(),
            font_size: 9.0,
            color: if active_color() { theme::TEXT } else { theme::FAINT },
        } ],
    }
}

/// 一行"标签 —— 值"，值靠右。宽度由父容器 Stretch 给。
///
/// 行高定值（默认 26）：统计行进滚动容器，不再随面板 `grow` 均分。
#[rec]
pub fn kv_row(
    #[getter]
    #[props(required)]
    label: String,
    #[getter]
    #[props(required)]
    value: String,
    #[getter]
    #[props(required)]
    tone: Color,
    #[props(default = 26.0f32)] h: f32,
) -> Fc {
    let label_c = Rc::clone(&label);
    let value_c = Rc::clone(&value);
    let tone_c = Rc::clone(&tone);
    node2! {
        height: h,
        // 与 v1 一致：标签从 PAD 起排、数值贴 `w - PAD`，分隔线也只在内容宽内。
        padding_inline_start: PAD,
        padding_inline_end: PAD,
        layout: flex! { direction: Layout::Row, align_item: AlignItem::Center, align_fix: true },
        draw: move |s: &mut Scene, n: &Node2| {
            if n.height() >= 22.0 {
                s.fill_rect(
                    Rect::new(PAD, n.height() - 1.0, (n.width() - 2.0 * PAD).max(0.0), 1.0),
                    theme::GRID,
                );
            }
        },
        children: vec![
            text_label! { content: label_c(), font_size: 9.5, color: theme::FAINT },
            node2! { exts: vec![grow(1.0)], skip_draw: true },
            text_label! { content: value_c(), font_size: 10.5, color: tone_c() },
        ],
    }
}

/// 滚动视口：容器 + 内容 + 滚动条，三段都用 canvasRender2 原语拼。
///
/// 容器必须先有身份（`create_scroll(container, ..)` 拿着它）再挂内容，所以走
/// `node_of`：它把 `Ref2` 交给回调，回调用它建节点并写回槽。
pub fn scroll_view(pad: f32, children: Vec<Fc>) -> Fc {
    node_of(move |get: Ref2<Node2>| {
        let node = Node2::empty();
        let scroll = create_scroll(&node, Axis::Y);
        let content = scroll_content! {
            scroll: scroll.clone(),
            layout: flex! { align_item: AlignItem::Stretch, align_fix: true },
            exts: vec![grow(1.0)],
            padding_inline_start: pad,
            padding_inline_end: pad,
            padding_block_start: pad,
            padding_block_end: pad,
            children: children,
        };
        let bar = scroll_bar! { scroll: scroll.clone() };

        let node = node.build(Node2Props {
            layout: flex! {
                direction: Layout::Row,
                align_item: AlignItem::Stretch,
                align_fix: true,
            },
            exts: vec![grow(1.0)],
            clip: true,
            children: vec![content, bar],
            ..Node2Props::default()
        });
        get.set(node.clone());
        Fc::from(node)
    })
}
