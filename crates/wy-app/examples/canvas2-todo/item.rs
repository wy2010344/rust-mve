//! 单条待办（对标 TS `canvas-render2/todo-item.ts`）。
//!
//! 结构：`rectNode`（横向 flex、显式高度 40）里放三样——
//!
//! 1. **复选框**：`Path2` 圆角方框，绘制回放与命中判定共用同一份路径
//!    （`contains` 是纯函数判定，命中区域跟随圆角而非外接矩形）；
//! 2. **文本**：`wrappedTextNode` + `grow(1)`，完成时灰色删除线；带链接时追加
//!    链接片段（点击由富文本命中后经 `UrlOpener` 打开，不触发本条的回调）；
//! 3. **删除按钮**：一个手绘的叉。
//!
//! 条目配色随主题（[`palette`] + `TextDefaults`）切换——`spans` 与 `draw` 都是
//! 读取式，切主题后重绘即变。
//!
//! # 写法
//!
//! 三个子节点都是 `node2! { .. }` 声明出来的（没有 `Node2Props::builder()` 链），
//! 外壳是 `rect_node! { .. }`——视觉属性（background / border / radius）与节点属性
//! （width / padding / layout / children）**写在同一个块里**，与 TS 那个
//! `rectNode({ background, borderColor, ..., children })` 调用一一对应。

use std::cell::RefCell;
use std::rc::Rc;

use wy_mve::canvas_render2::{
    append_rounded_rect, grow, placeholder_color, text_color, LayoutSize, MouseEvent2, Path2,
    RichTextSpan,
};
// `node2!` / `rect_node!` / `wrapped_text_node!` 是 `#[macro_export]` 的宏，落在
// **crate 根**，不在 `canvas_render2` 子模块里（子模块里同名的是函数/模块）。
use wy_mve::{flex, node2, rect_node, wrapped_text_node, Fc, Layout, Node2, Scene};
use wy_render::Color;
use wy_text::{TextDecoration, TextStyle};

use crate::store::{palette, remove_todo, toggle_todo, Todo};

/// 复选框底色（TS `0xff22c55e`）。
const GREEN: Color = Color::from_u32(0x22C55EFF);

/// 复选框路径：圆角方框录制一次，绘制回放与命中判定共用。
///
/// TS: `const box = createPath2(); appendRoundedRect(box, 0.75, 0.75, 16.5, 16.5, 4);`
fn checkbox_path() -> Path2 {
    let mut path = Path2::new();
    append_rounded_rect(&mut path, 0.75, 0.75, 16.5, 16.5, 4.0);
    path
}

/// 对勾折线（TS 里直接走 2D context 的三段 `lineTo`）。
fn check_path() -> Path2 {
    let mut path = Path2::new();
    path.move_to(4.5, 9.5);
    path.line_to(7.5, 12.5);
    path.line_to(13.5, 6.0);
    path
}

/// 一条待办（对标 TS `todoItem`）。
///
/// `todo` 是 `for_each_node` 交给我们的**每项句柄**（`Rc<RefCell<Todo>>`）：
/// 条目被 keyed 复用时内容原地更新，所以这里一律读最新值、不缓存副本。
pub fn todo_item(todo: Rc<RefCell<Todo>>) -> Fc {
    let box_path = Rc::new(checkbox_path());
    let tick = Rc::new(check_path());

    // ---- 复选框 ----
    let check_todo = Rc::clone(&todo);
    let hit_path = Rc::clone(&box_path);
    let paint_path = Rc::clone(&box_path);
    let check_tick = Rc::clone(&tick);
    let checkbox = node2! {
        size: |_d: wy_mve::canvas_render2::Axis| LayoutSize::outer(18.0),
        // 命中判定跟着圆角走（Path2 的 contains 是纯几何判定）
        accept_hit: move |x: f32, y: f32| hit_path.contains(x, y),
        draw: move |scene: &mut Scene, _n: &Node2| {
            let done = check_todo.borrow().done;
            if done {
                paint_path.fill_into(scene, GREEN);
            }
            let stroke = if done { GREEN } else { palette().checkbox_border };
            paint_path.stroke_into(scene, 1.5, stroke);
            if done {
                check_tick.stroke_into(scene, 2.0, Color::WHITE);
            }
        },
        mouse_click: {
            let t = Rc::clone(&todo);
            move |_e: &mut MouseEvent2| toggle_todo(t.borrow().id)
        },
    };

    // ---- 文本 ----
    let text_todo = Rc::clone(&todo);
    let text = wrapped_text_node! {
        exts: vec![grow(1.0)],
        // `spans` 继承自 `rich_text_node`（`#[getter]`）：这里写**值表达式**，
        // 宏自己包 `move ||` —— 与其它文本字段一致，不再手写闭包。
        spans: {
            let t = text_todo.borrow().clone();
            // 完成时灰色 + 删除线（TS `color: placeholder, decoration:{lineThrough}`）
            let mut main_style = TextStyle::normal()
                .with_font_size(13.0)
                .with_color(if t.done {
                    placeholder_color().to_u32()
                } else {
                    text_color().to_u32()
                });
            main_style.decoration = if t.done {
                TextDecoration::LINE_THROUGH
            } else {
                TextDecoration::NONE
            };
            let main = RichTextSpan {
                text: t.text.clone(),
                style: main_style,
                url: None,
            };
            let Some(link) = t.link.clone() else {
                return vec![main];
            };
            let mut link_style = TextStyle::normal()
                .with_font_size(13.0)
                .with_color(palette().link.to_u32());
            link_style.decoration = TextDecoration::UNDERLINE;
            vec![
                main,
                RichTextSpan {
                    text: " 打开链接".to_string(),
                    style: link_style,
                    url: Some(link),
                },
            ]
        },
    };

    // ---- 删除按钮 ----
    let delete = node2! {
        size: |_d: wy_mve::canvas_render2::Axis| LayoutSize::outer(18.0),
        draw: move |scene: &mut Scene, _n: &Node2| {
            let color = palette().checkbox_border;
            let mut cross = Path2::new();
            cross.move_to(5.0, 5.0);
            cross.line_to(13.0, 13.0);
            cross.move_to(13.0, 5.0);
            cross.line_to(5.0, 13.0);
            cross.stroke_into(scene, 1.5, color);
        },
        mouse_click: {
            let t = Rc::clone(&todo);
            move |_e: &mut MouseEvent2| remove_todo(t.borrow().id)
        },
    };

    // ---- 条目外壳 ----
    let shell_todo = Rc::clone(&todo);
    rect_node! {
        // 读取式：完成态换卡片底色（TS `background: () => palette().cardDone`）
        background: {
            let p = palette();
            Some(if shell_todo.borrow().done { p.card_done } else { p.card })
        },
        border_color: Some(palette().border),
        border_width: 1.0,
        radius: 6.0,
        height: 40.0,
        // 竖向 padding 让 stretch 后的文本行高与条目高度匹配，视觉居中
        padding_inline_start: 10.0,
        padding_inline_end: 10.0,
        padding_block_start: 11.0,
        padding_block_end: 11.0,
        layout: flex! {
            direction: Layout::Row,
            gap: 8.0,
        },
        children: vec![checkbox, text, delete],
    }
}
