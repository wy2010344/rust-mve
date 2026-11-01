//! 头部 / 过滤栏 / 操作栏 / 工具行（对标 TS `canvas-render2/todo-toolbar.ts`）。
//!
//! 写法上**全部是声明式组件**，没有一处 `XxxArg::builder()`：
//!
//! | TS | 这里 |
//! |---|---|
//! | `function textButton({ label, primary, active, disabled, onClick })` | `#[rec] fn text_button(..)`：`label` / `active` / `disabled` 带 `#[getter]`，`primary` 带 `#[props(default)]` |
//! | `textButton({ .. })` | `text_button! { .. }` |
//! | `rectNode({ width, paddingInline, layout, children })` | `node2! { .. }`（TS 用 rectNode 但没给任何视觉属性，Rust 用真正的"无自绘容器"原语） |
//! | `buttons.map(b => textButton(..))` | `children: vec![text_button! { .. }, ..]`（三条各写一遍，与 TS 的 `buttonDefs` 一一对应） |
//!
//! 状态（禁用 / 激活 / 主题）一律**读取式**：`draw` 与 `spans` 都是绘制期才求值，
//! 信号一变重绘即换色，不用重建节点。

use std::rc::Rc;

use wy_mve::canvas_render2::RichTextSpan;
use wy_mve::canvas_render2::{grow, placeholder_color, text_color, MouseEvent2, Node2};
use wy_mve::{flex, node2, rec, wrapped_text_node, DirectionJustify, Fc, Layout};
use wy_render::{Color, Rect, Scene};
use wy_text::TextStyle;

use crate::store::{
    add_sample_todo, can_redo, can_undo, clear_done, filter, palette, redo_todo, set_filter, stats,
    theme, toggle_theme, undo_todo, visible_todos, Theme, TodoFilter,
};

/// 三块居中的工具栏共同的外壳（TS 里 `filterBar` / `actionBar` / `toolBar` 各自
/// 重复写了一遍同样的 `rectNode({ width:380, paddingInline:4, layout: flex({..}) })`，
/// 这里收成一处；`children` 槽与 TS 的 `children` 同序同义）。
///
/// 头部不走这里：TS 的 `todoHeader` 没有 `directionJustify: 'center'`（靠起点对齐），
/// 与这三块不同，所以它自己写 `node2!`。
///
/// 放在文件最前：`#[rec]` 生成的 `bar!` 是 `macro_rules!`，**文本序**要求先定义
/// 后使用，所以基础组件必须排在用它的函数之前（finance/ui2 里同样把
/// `widgets` 用 `#[macro_use]` 提到最前）。
#[rec]
pub fn bar(children: Vec<Fc>) -> Fc {
    node2! {
        width: 380.0,
        padding_inline_start: 4.0,
        padding_inline_end: 4.0,
        layout: flex! {
            direction: Layout::Row,
            gap: 8.0,
            direction_justify: DirectionJustify::Center,
        },
        children: children,
    }
}

/// 通用圆角按钮（TS `textButton`）。
///
/// 五个参数与 TS 解构出来的字段一一对应，区别只在**求值时点**：
/// `#[getter]` 的字段在调用处写的是表达式本身（宏包成 `move || 表达式`），
/// 绘制 / 点击时才求值——所以 `disabled` 永远是最新值，不会出现"点了才更新灰态"。
///
/// `spans` 取色 + `draw` 画胶囊底：`draw` 先于文字执行（`rich_text_node` 里
/// 继承来的 draw 在段落绘制之前跑），底色不会盖住字。
#[rec]
pub fn text_button(
    /// 文案。主题切换时"切换浅色/深色"要跟着变，所以是读取式。
    #[getter]
    #[props(required)]
    label: String,
    /// 主按钮（整颗胶囊实心）。
    #[props(default = false)]
    primary: bool,
    /// 激活态（过滤栏的当前项）。
    #[getter]
    #[props(default = false)]
    active: bool,
    /// 禁用态（撤销 / 重做无路可走）。
    #[getter]
    #[props(default = false)]
    disabled: bool,
    /// 点击；禁用态下不触发（判定在点击那一刻现读）。
    on_click: impl Fn() + 'static,
) -> Fc {
    // 三个读取源分三份：`spans` / `draw` / `mouse_click` 各自独占自己的那份。
    let spans_label = label.clone();
    let spans_active = active.clone();
    let spans_disabled = disabled.clone();
    let draw_active = active;
    let draw_disabled = disabled.clone();
    let click_disabled = disabled;
    let on_click = on_click.unwrap_or_else(|| Rc::new(|| ()));

    wrapped_text_node! {
        auto_width: true,
        padding: 10.0,
        mouse_click: move |_e: &mut MouseEvent2| {
            if click_disabled() {
                return;
            }
            on_click();
        },
        spans: {
            let p = palette();
            let color = if spans_disabled() {
                p.btn_disabled_text
            } else if primary {
                Color::WHITE
            } else if spans_active() {
                p.btn_active_text
            } else {
                p.btn_text
            };
            vec![RichTextSpan {
                text: spans_label(),
                style: TextStyle::normal()
                    .with_font_size(13.0)
                    .with_color(color.to_u32()),
                url: None,
            }]
        },
        draw: move |scene: &mut Scene, n: &Node2| {
            let p = palette();
            let bg = if draw_disabled() {
                p.btn_disabled_bg
            } else if primary {
                // TS `0xff1d4ed8`（blue-700）
                Color::from_u32(0x1D4ED8FF)
            } else if draw_active() {
                p.btn_active_bg
            } else {
                p.btn_bg
            };
            let (w, h) = (n.width(), n.height());
            if w > 0.0 && h > 0.0 {
                scene.fill_round_rect(Rect::new(0.0, 0.0, w, h), h / 2.0, bg);
            }
        },
    }
}

/// 标题 + 统计（统计文本 grow 撑满、右对齐）。
#[must_use]
pub fn todo_header() -> Fc {
    node2! {
        width: 380.0,
        padding_inline_start: 4.0,
        padding_inline_end: 4.0,
        layout: flex! {
            direction: Layout::Row,
            gap: 8.0,
        },
        children: vec![
            wrapped_text_node! {
                auto_width: true,
                text: "待办清单",
                font_size: 20.0,
                font_weight: 700,
                color: text_color(),
            },
            wrapped_text_node! {
                exts: vec![grow(1.0)],
                font_size: 13.0,
                color: placeholder_color(),
                // TS `共 N 项 · 完成 M 项`：读取式，统计随信号变
                text: {
                    let (total, done) = stats();
                    format!("共 {total} 项 · 完成 {done} 项")
                },
            },
        ],
    }
}

/// 过滤切换：全部 / 未完成 / 已完成。
#[must_use]
pub fn filter_bar() -> Fc {
    bar! {
        children: vec![
            text_button! {
                label: "全部",
                active: filter() == TodoFilter::All,
                on_click: move || set_filter(TodoFilter::All),
            },
            text_button! {
                label: "未完成",
                active: filter() == TodoFilter::Active,
                on_click: move || set_filter(TodoFilter::Active),
            },
            text_button! {
                label: "已完成",
                active: filter() == TodoFilter::Done,
                on_click: move || set_filter(TodoFilter::Done),
            },
        ],
    }
}

/// 操作栏：添加一条 / 清除已完成 /（右侧）空态提示。
#[must_use]
pub fn action_bar() -> Fc {
    bar! {
        children: vec![
            text_button! {
                label: "+ 添加一条",
                primary: true,
                on_click: add_sample_todo,
            },
            text_button! {
                label: "清除已完成",
                on_click: clear_done,
            },
            // 底部提示：当前过滤下无数据时才出现文字
            wrapped_text_node! {
                exts: vec![grow(1.0)],
                font_size: 13.0,
                color: placeholder_color(),
                text: if visible_todos().is_empty() { "当前过滤下没有待办" } else { "" },
            },
        ],
    }
}

/// 工具行：撤销 / 重做 / 主题切换。
///
/// 撤销 / 重做消费 `store` 的快照历史（增删改均可退），`disabled` 现读
/// `can_undo` / `can_redo`，无路可走时置灰。
#[must_use]
pub fn tool_bar() -> Fc {
    bar! {
        children: vec![
            text_button! {
                label: "撤销",
                disabled: !can_undo(),
                on_click: undo_todo,
            },
            text_button! {
                label: "重做",
                disabled: !can_redo(),
                on_click: redo_todo,
            },
            text_button! {
                label: if theme() == Theme::Dark { "切换浅色" } else { "切换深色" },
                on_click: toggle_theme,
            },
        ],
    }
}
