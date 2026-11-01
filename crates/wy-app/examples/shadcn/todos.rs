//! 待办清单卡片：筛选 tabs + 新待办输入 + checkbox 行（勾选/删除）+ 空态骨架。
//!
//! 列表放进滚动容器并 `grow(1.0)` 填满卡片剩余高度——条目多时视口内滚动，
//! 卡片高度始终与表单卡等高，底边对齐。
//!
//! # 信号模型（per-item，不是 React）
//!
//! - `App::todo_ids` 只管**结构**（有哪些项、什么顺序）；
//! - 每项的「完成」是**独立信号**（`App::todo_store`），勾选只写那一个；
//! - 行内文本/勾选态在**绘制期**读自己的信号 → 只有那一行重绘。
//!
//! # 迁移要点
//!
//! 旧 `for_each(emit, build)` → 新 `for_each_node(emit, build)`；旧 `scroll {..}`
//! 是 v1 组件，新体系要自己拼「容器 + 内容 + 滚动条」三段（见 [`scroll_view`]）。

use wy_mve::canvas_render2::node2::Node2Props;
use wy_mve::canvas_render2::{create_scroll, for_each_node};
use wy_mve::{
    flex, grow, grow_xy, node2, node_of, scroll_bar, scroll_content, AlignItem, Axis, Fc, Icon,
    Layout, Node2, Ref2,
};
use wy_signal::{GetValue, SetValue, Signal};

use crate::dashboard::input_field;
use crate::state::App;
use crate::tokens::Theme;
use crate::widgets::{card, separator, skeleton, tabs, Btn};

/// 单行高度（固定，不随容器高度变化）。
const ROW_H: f32 = 30.0;

/// 竖向间隔（替代旧 `spacer(0.0, h)`）。
fn vgap(h: f32) -> Fc {
    node2! { height: h }
}

/// 滚动视口：容器 + 内容 + 滚动条，三段都用 canvasRender2 的原语拼。
///
/// 容器必须**先有身份**（`create_scroll(container, ..)` 拿着它）再挂内容，
/// 所以走 `node_of`：它把 `Ref2` 交给回调，回调用它建节点并写回槽。
fn scroll_view(pad: f32, children: Vec<Fc>) -> Fc {
    node_of(move |get: Ref2<Node2>| {
        let node = Node2::empty();
        let scroll = create_scroll(&node, Axis::Y);
        let content = scroll_content! {
            scroll: scroll.clone(),
            // 内容：纵向排布 + 交叉轴撑满视口。
            layout: flex! { align_item: AlignItem::Stretch, align_fix: true },
            exts: vec![grow(1.0)],
            padding_inline_start: pad,
            padding_inline_end: pad,
            padding_block_start: pad,
            padding_block_end: pad,
            children: children,
        };
        let bar = scroll_bar! { scroll: scroll.clone() };

        // 容器：纵向滚动 → 布局取 Row（滚动轴落在交叉轴），`clip` 裁掉溢出。
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

/// 单条待办：checkbox + 文本 + 删除。文本与勾选态都在绘制期读该项自己的信号。
fn todo_row(
    theme: Theme,
    ids: Signal<Vec<u32>>,
    id: u32,
    done: Signal<bool>,
    text: Signal<String>,
) -> Fc {
    let store_done = done;
    node2! {
        height: ROW_H,
        // `align_fix: true` 是"行高是这 30，不是我最高子项的高"的唯一说法。
        // 缺了它，交叉轴 `StackLayout` 的基准会退化成**最高子节点**（这里是 24 高的
        // 删除按钮），于是 `Center` 把 checkbox / 文字排在这 24 里，相对 30 的行盒整体偏上
        // 3px——Kotlin 组件里凡是给了 `height` 的容器都配 `alignFix = true`（见
        // `components/.../Button.kt`），TS `StackLayout` 同样如此。
        layout: flex! {
            direction: Layout::Row,
            align_item: AlignItem::Center,
            align_fix: true,
            gap: 10.0,
        },
        children: vec![
            checkbox! {
                theme: theme,
                checked: done.get(),
                on_change: move |v: bool| store_done.set(v),
            },
            text_label! {
                content: text.get(),
                color: if done.get() {
                    theme.tokens().muted_fg
                } else {
                    theme.tokens().foreground
                },
            },
            // 删除按钮推到行尾
            node2! { exts: vec![grow(1.0)] },
            button_sized! {
                theme: theme,
                label: String::new(),
                variant: Btn::Ghost,
                w: 26.0,
                h: 24.0,
                icon: Icon::X,
                on_click: move || {
                    // 删除只改**结构**信号（ids），不动其它项的状态
                    let mut l = ids.get();
                    l.retain(|x| *x != id);
                    ids.set(l);
                },
            },
        ],
    }
}

pub(super) fn todos_card(theme: Theme, app: &App) -> Fc {
    let ids = app.todo_ids;
    let next_id = app.next_id;
    let filter = app.todo_filter;
    let input = app.todo_input;
    let store = app.todo_store.clone();

    // 计数：标题行右侧（信号驱动）
    let count_store = store.clone();
    let count_label = {
        move || {
            let l = ids.get();
            let done = l.iter().filter(|id| count_store.done_get(**id)).count();
            format!("共 {} 项 · 完成 {}", l.len(), done)
        }
    };

    // 空态：骨架占位 + 提示（列表非空时隐藏）。
    // 旧版是 `node.hide_fn = Some(Rc::new(..))`；新体系 `hide` 是第一类字段。
    let empty_tip = node2! {
        hide: !ids.get().is_empty(),
        layout: flex! {
            direction: Layout::Column,
            gap: 8.0,
            align_item: AlignItem::Stretch,
            align_fix: true,
        },
        children: vec![
            skeleton(theme, 14.0),
            skeleton(theme, 14.0),
            text_label! { content: "暂无待办，添加一个吧".to_string(), color: theme.tokens().muted_fg },
        ],
    };

    // 列表：keyed 增量列表（`for_each_node` = TS `forEachNode`）。
    //
    // **筛选在 emit 闭包内读取**：`filter.get()` 必须写在源闭包里，
    // 否则 memo 追踪不到它——改筛选不重算，表现为「筛选无效」。
    let rows = {
        let store_rows = store.clone();
        let store_fe = store.clone();
        for_each_node(
            move |emit| {
                let f = filter.get();
                for id in ids.get() {
                    let done = store_rows.done_of(id).get();
                    if match f {
                        1 => !done,
                        2 => done,
                        _ => true,
                    } {
                        emit(id, ());
                    }
                }
            },
            move |id, _v, cx| {
                let done = store_fe.done_of(id);
                let text = store_fe.text_of(id);
                cx.child(todo_row(theme, ids, id, done, text));
                cx.child(separator(theme, false));
            },
        )
    };

    let list = node2! {
        layout: flex! { direction: Layout::Column, align_item: AlignItem::Stretch, align_fix: true, gap: 0.0 },
        children: vec![rows, empty_tip],
    };

    // 输入行
    let add_row = {
        let store_add = store.clone();
        node2! {
            layout: flex! { direction: Layout::Row, align_item: AlignItem::Center, gap: 8.0 },
            children: vec![
                // 输入框**直接**当这一行的主轴子项，靠 `grow` 抢剩余宽度。
                // 别在外面再包一层带 `grow` 的容器：`editable_text_node` 自己
                // 声明了 `size`（= 内容宽），容器分到宽度也传不进去——
                // 内层没有 `grow` 也没有 `width`，`defaultSize` 又回头问父，
                // 环闭合取 0，整框连边框都画不出来（只剩旁边一个按钮）。
                input_field(theme, input, "新的待办...", vec![grow_xy(1.0, 0.0)]),
                button_sized! {
                    theme: theme,
                    label: "添加".to_string(),
                    variant: Btn::Secondary,
                    w: 56.0,
                    h: 34.0,
                    on_click: move || {
                        let text = input.get().trim().to_string();
                        if !text.is_empty() {
                            let id = next_id.get();
                            next_id.set(id + 1);
                            let done = store_add.done_of(id);
                            done.set(false);
                            store_add.text_of(id).set(text);
                            let mut l = ids.get();
                            l.push(id);
                            ids.set(l);
                            input.set(String::new());
                        }
                    },
                },
            ],
        }
    };

    // 标题行
    let title_row = node2! {
        layout: flex! { direction: Layout::Row, align_item: AlignItem::Center, gap: 8.0 },
        children: vec![
            text_label! {
                content: "待办清单".to_string(),
                font_size: 17.0,
                color: theme.tokens().foreground,
            },
            node2! { width: 8.0, height: 0.0 },
            text_label! { content: count_label(), color: theme.tokens().muted_fg },
        ],
    };

    // 尺寸完全交给 flex（同表单卡）：等宽等高
    card(
        theme,
        0.0,
        vec![grow(1.0)],
        vec![
            title_row,
            vgap(12.0),
            tabs(
                theme,
                vec![
                    String::from("全部"),
                    String::from("进行中"),
                    String::from("已完成"),
                ],
                filter,
            ),
            vgap(8.0),
            add_row,
            vgap(12.0),
            scroll_view(0.0, vec![list]),
        ],
    )
}
