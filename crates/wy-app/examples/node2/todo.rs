//! Todo-list —— **纯 Node2 运行时**（`Node2App`）。
//!
//! 与旧 `todo2.rs` 除入口外，唯一的实质替换是把最后两处 v1 桥接换掉：
//!
//! | 用途 | 旧（v1） | 新（canvasRender2） |
//! |---|---|---|
//! | 输入框 | `text_field_opts(..)`（v1 `Node`） | [`editable_text_node`]（TS `EditableTextNode.ts`） |
//! | 文本 | `text_styled(..)`（v1 `Node`） | [`wrapped_text_node`]（TS `WrappedTextNode.ts`） |
//!
//! 换完之后整棵树**没有一个 v1 `Node`**：布局、绘制、命中、键盘、IME 全部
//! 由 `Node2` 自己承担——这正是"新模式"的定义。
//!
//! 运行：`cargo run -p wy-app --example node2-todo`
//! 出图：`cargo run -p wy-app --example node2-todo -- --snapshot docs/shot-node2-todo.png`

// `text_label` 必须排第一：`#[macro_use]` 只对其后的代码生效。
#[macro_use]
mod text_label;
mod theme;

// `#[rec]` 生成的 `!` 宏按 `$crate::XxxArg` 定位参数结构体，所以必须把
// 子模块里的 Arg 再导出到 crate 根（同 shadcn `main.rs` 的做法）。
pub use text_label::TextLabelArg;

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use theme::*;
use wy_engine::node2_app::Node2App;
use wy_engine::runner::run;
use wy_mve::canvas_render2::node2::{get, Node2Props};
use wy_mve::canvas_render2::{build_root_node2, create_scroll, for_each_node, if_node};
use wy_mve::editable_text_node;
use wy_mve::wrapped_text_node;
use wy_mve::{
    flex, grow, node2, node_of, rec, AlignItem, Axis, CursorType, DirectionJustify, Fc, Layout,
    LayoutSize, Node2, NodeContext, Ref2, Scene,
};
use wy_mve::{scroll_bar, scroll_content};
use wy_render::{Point, Rect};
use wy_signal::{GetValue, SetValue, Signal};

/// 行高（定高：滚动容器的前提，行不随面板伸缩）。
const ROW_H: f32 = 34.0;
/// 复选框边长。
const BOX: f32 = 17.0;

// ============================================================================
// 状态模型：结构信号 + 每项独立信号
// ============================================================================

/// 筛选维度：全部 / 进行中 / 已完成。
#[derive(Clone, Copy, PartialEq, Eq)]
enum Filter {
    All,
    Active,
    Done,
}

/// 每项待办的两个信号：文本 + 完成态。
type TodoItem = (Signal<String>, Signal<bool>);

/// 待办存储：`id -> (text, done)`，按需创建、之后复用同一对信号。
#[derive(Default, Clone)]
struct TodoStore(Rc<RefCell<HashMap<u64, TodoItem>>>);

impl TodoStore {
    fn of(&self, id: u64) -> TodoItem {
        *self
            .0
            .borrow_mut()
            .entry(id)
            .or_insert_with(|| (Signal::new(String::new()), Signal::new(false)))
    }
    fn text_of(&self, id: u64) -> Signal<String> {
        self.of(id).0
    }
    fn done_of(&self, id: u64) -> Signal<bool> {
        self.of(id).1
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let ids: Signal<Vec<u64>> = Signal::new(vec![1, 2, 3, 4]);
    let store = TodoStore::default();
    for (id, text, done) in [
        (1u64, "把组件契约写进 docs", true),
        (2, "验证纯 Node2 运行时", false),
        (3, "跑离屏快照对比", false),
        (4, "汇总 FINDING 清单", false),
    ] {
        let (t, d) = store.of(id);
        t.set(text.to_string());
        d.set(done);
    }

    let input = Signal::new(String::new());
    let next_id = Signal::new(5u64);
    let filter = Signal::new(Filter::All);

    let make = move || {
        Node2App::new(move |viewport| {
            let page_vp = viewport.clone();
            build_root_node2(theme::root_arg(viewport), move |cx| {
                cx.child(todo_app!(
                    ids,
                    store: store.clone(),
                    input,
                    next_id,
                    filter,
                    viewport: page_vp,
                ));
            })
        })
        .with_title("wy-ui · node2 · todo")
    };

    if let Some((path, w, h)) = wy_app::snapshot::parse_snapshot_args() {
        let app = make();
        wy_app::snapshot::save_snapshot(&app, &path, w, h).map_err(std::io::Error::other)?;
        return Ok(());
    }
    run(make())?;
    Ok(())
}

// ============================================================================
// 应用装配
// ============================================================================

/// 整页：输入行 + 筛选行 + 滚动列表 + 状态行。
#[rec]
fn todo_app(
    ids: Signal<Vec<u64>>,
    store: TodoStore,
    input: Signal<String>,
    next_id: Signal<u64>,
    filter: Signal<Filter>,
    viewport: Rc<Cell<(f32, f32)>>,
) -> Fc {
    // 页面铺满画布（同 counter：页面尺寸来自画布，不靠布局去猜）。
    let (vw, vh) = (viewport.clone(), viewport);
    let store_fe = store.clone();
    let filter_rows = filter;
    let store_rows = store.clone();

    // 列表：keyed 增量列表（`for_each_node` = TS `forEachNode`）。
    let rows = for_each_node(
        move |emit| {
            let f = filter_rows.get();
            for id in ids.get() {
                let is_done = store_rows.done_of(id).get();
                if visible_under(f, is_done) {
                    emit(id, ());
                }
            }
        },
        move |id, _v, cx| {
            cx.child(todo_row!(
                ids,
                store: store_fe.clone(),
                id,
            ));
        },
    );

    let list = node2! {
        exts: vec![grow(1.0)],
        layout: flex! { align_item: AlignItem::Stretch, align_fix: true },
        draw: move |s: &mut Scene, n: &Node2| {
            s.fill_round_rect(Rect::new(0.0, 0.0, n.width(), n.height()), 8.0, PANEL);
        },
        children: vec![
            scroll_view! { children: vec![rows] },
        ],
    };

    node2! {
        layout:
            flex! {
                direction: Layout::Column,
                align_item: AlignItem::Stretch,
                align_fix: true,
                gap: 10.0,
            },
        // 根随视口缩放：用 `size` 闭包读取 viewport Cell，布局期每次求值都重新读，
        // 窗口放大后面板会跟着重排（之前写 `width: vw.get().0` 裸值是建造期一次性
        // 求值，面板钉死在初始尺寸——"绘制区域不随窗口放大"的根因）。
        // `node2!` 的 `width/height` 只收裸 f32，无法传响应式闭包，所以用 `size` 闭包。
        size: move |d: Axis| -> LayoutSize {
            match d {
                Axis::X => LayoutSize::outer(vw.get().0),
                Axis::Y => LayoutSize::outer(vh.get().1),
            }
        },
        padding: 14.0,
        draw: move |s: &mut Scene, n: &Node2| {
            s.fill_rect(Rect::new(0.0, 0.0, n.width(), n.height()), BG);
        },
        children: vec![
            input_row! {
                store: store.clone(),
                input,
                ids,
                next_id,
            },
            filter_row! { filter, ids, store: store.clone() },
            list,
            status_row! { ids, store },
        ],
    }
}

/// 滚动视口：容器 + 内容 + 滚动条，三段都用 canvasRender2 的原语拼。
///
/// 容器必须**先有身份**（`create_scroll(container, ..)` 拿着它）再挂内容，
/// 所以走 `node_of`：它把 `Ref2` 交给回调，回调用它建节点并写回槽。
#[rec]
fn scroll_view(#[props(default = 6.0f32)] pad: f32, children: Vec<Fc>) -> Fc {
    node_of(move |get: Ref2<Node2>| {
        let node = Node2::empty();
        let scroll = create_scroll(&node, Axis::Y);
        let content = scroll_content! {
            scroll: scroll.clone(),
            // 内容：纵向排布 + 交叉轴撑满视口。
            layout: flex! { align_item: AlignItem::Stretch, align_fix: true },
            exts: vec![grow(1.0)],
            // 内边距放内容上（原因见 scroll 示例：容器的 `y` 被滚动位移写成显式
            // 值，容器 padding 会被这段位移吃掉）。
            padding_inline_start: pad,
            padding_inline_end: pad,
            padding_block_start: pad,
            padding_block_end: pad,
            children: children,
        };
        let bar = scroll_bar! {
            scroll: scroll.clone(),
        };

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

/// 输入行：文本框（grow 吃掉剩余宽）+ 「添加」按钮。
#[rec]
fn input_row(
    store: TodoStore,
    input: Signal<String>,
    ids: Signal<Vec<u64>>,
    next_id: Signal<u64>,
) -> Fc {
    node2! {
        layout:
            flex! {
                direction: Layout::Row,
                align_item: AlignItem::Center,
                gap: 8.0,
            },
        children: vec![
            text_field! { input },
            primary_btn! {
                text: "添加",
                on_click: move || add(&input, &ids, &next_id, &store),
            },
        ],
    }
}

/// 筛选行：三个分段按钮 + 右侧「清除已完成」。
#[rec]
fn filter_row(filter: Signal<Filter>, ids: Signal<Vec<u64>>, store: TodoStore) -> Fc {
    let f_all = filter;
    let f_active = filter;
    let f_done = filter;
    let f_clear = filter;
    let ids_clear = ids;
    let store_clear = store.clone();
    node2! {
        layout:
            flex! {
                direction: Layout::Row,
                align_item: AlignItem::Center,
                gap: 8.0,
            },
        children: vec![
            filter_btn! {
                filter,
                which: Filter::All,
                text: "全部",
                on_click: move || f_all.set(Filter::All),
            },
            filter_btn! {
                filter,
                which: Filter::Active,
                text: "进行中",
                on_click: move || f_active.set(Filter::Active),
            },
            filter_btn! {
                filter,
                which: Filter::Done,
                text: "已完成",
                on_click: move || f_done.set(Filter::Done),
            },
            // growing spacer：只给 exts（对应 TS `exts: [grow({argGrow: 1})]`）
            node2! { exts: vec![grow(1.0)] },
            primary_btn! {
                text: "清除已完成",
                on_click: move || {
                    let kept: Vec<u64> = ids_clear
                        .get()
                        .into_iter()
                        .filter(|id| !store_clear.done_of(*id).get())
                        .collect();
                    f_clear.set(Filter::All);
                    ids_clear.set(kept);
                },
            },
        ],
    }
}

/// 状态行：剩余计数。
#[rec]
fn status_row(ids: Signal<Vec<u64>>, store: TodoStore) -> Fc {
    let store_count = store.clone();
    node2! {
        layout:
            flex! {
                direction: Layout::Row,
                align_item: AlignItem::Center,
                gap: 8.0,
            },
        children: vec![
            count_text! {
                text: {
                    let left = ids
                        .get()
                        .iter()
                        .filter(|id| !store_count.done_of(**id).get())
                        .count();
                    format!("{} 项待完成", left)
                },
            },
        ],
    }
}

// ============================================================================
// 行组件
// ============================================================================

/// 一行 todo：复选框 + 文本 + 删除钮，定高，悬停高亮 + 底部分隔线。
#[rec]
fn todo_row(ids: Signal<Vec<u64>>, store: TodoStore, id: u64) -> Fc {
    let hover = Signal::new(false);
    let done = store.done_of(id);
    let store_t = store.clone();
    let store_d = store.clone();

    node2! {
        layout:
            flex! {
                direction: Layout::Row,
                align_item: AlignItem::Center,
                gap: 10.0,
                // 定高行：`align_fix` 让内容以**行高**为基准居中（否则基准是最高
                // 子项，整行文字/按钮都贴顶，就是"行未垂直居中"的来源）。
                align_fix: true,
            },
        padding_inline_start: 10.0,
        padding_inline_end: 10.0,
        height: ROW_H,
        on_hover: move |entered: bool| hover.set(entered),
        draw: move |s: &mut Scene, n: &Node2| {
            let (w, h) = (n.width(), n.height());
            if hover.get() {
                s.fill_rect(Rect::new(0.0, 0.0, w, h), ROW_ALT);
            }
            s.fill_rect(Rect::new(0.0, h - 1.0, w, 1.0), LINE);
        },
        children: vec![
            checkbox! {
                done: done.get(),
                on_toggle: {
                    let d = store.done_of(id);
                    move || d.set(!d.get())
                },
            },
            todo_text! {
                text: store_t.text_of(id).get(),
                done: store_d.done_of(id).get(),
            },
            node2! { exts: vec![grow(1.0)] },
            del_btn! {
                on_click: move || {
                    let kept: Vec<u64> = ids.get().into_iter().filter(|x| *x != id).collect();
                    ids.set(kept);
                },
            },
        ],
    }
}

/// 复选框：节点自绘（圆角框 + 对勾），`done` 走 getter。
#[rec]
fn checkbox(
    #[getter]
    #[props(required)]
    done: bool,
    #[props(required)] on_toggle: impl Fn() + 'static,
) -> Fc {
    node2! {
        width: BOX,
        height: BOX,
        cursor: CursorType::Pointer,
        mouse_click: move |_e| on_toggle(),
        draw: move |s: &mut Scene, _n: &Node2| {
            if done() {
                s.fill_round_rect(Rect::new(0.0, 0.0, BOX, BOX), 4.0, ACCENT);
                s.stroke_path(
                    [
                        Point::new(3.5, 9.0),
                        Point::new(7.0, 13.0),
                        Point::new(13.5, 4.0),
                    ],
                    1.8,
                    BG,
                );
            } else {
                s.stroke_round_rect(
                    Rect::new(0.75, 0.75, BOX - 1.5, BOX - 1.5),
                    4.0,
                    FAINT,
                    1.2,
                );
            }
        },
    }
}

/// 行文本：完成态灰掉 + 删除线。
///
/// 文本是**子节点**（`text_label!`），宽高由它自己撑；删除线由容器按子节点的
/// 实际宽度画——`draw` 运行时布局已完成，`n.children()` 读到的就是排好的几何。
#[rec]
fn todo_text(
    #[getter]
    #[props(required)]
    text: String,
    #[getter]
    #[props(required)]
    done: bool,
) -> Fc {
    let done_line = Rc::clone(&done);
    node2! {
        exts: vec![grow(1.0)],
        layout: flex! {
            direction: Layout::Row,
            align_item: AlignItem::Center,
            align_fix: true,
        },
        draw: move |s: &mut Scene, n: &Node2| {
            if !done_line() {
                return;
            }
            let Some(t) = n.children().first().cloned() else {
                return;
            };
            s.fill_rect(Rect::new(0.0, n.height() / 2.0, t.width(), 1.0), FAINT);
        },
        children: vec![text_label! {
            content: text(),
            font_size: 13.0,
            color: if done() { FAINT } else { TEXT },
        }],
    }
}

/// 计数文本：容器定高 18，文本由 flex 在交叉轴上居中。
#[rec]
fn count_text(
    #[getter]
    #[props(required)]
    text: String,
) -> Fc {
    node2! {
        height: 18.0,
        layout: flex! {
            direction: Layout::Row,
            align_item: AlignItem::Center,
            align_fix: true,
        },
        children: vec![text_label! { content: text(), font_size: 12.0, color: MUTED }],
    }
}

/// 删除钮：22×22 的 × 字热区（字形也是文本节点，两轴居中）。
#[rec]
fn del_btn(#[props(required)] on_click: impl Fn() + 'static) -> Fc {
    node2! {
        width: 22.0,
        height: 22.0,
        cursor: CursorType::Pointer,
        mouse_click: move |_e| on_click(),
        layout: flex! {
            direction: Layout::Row,
            align_item: AlignItem::Center,
            direction_justify: DirectionJustify::Center,
            align_fix: true,
        },
        children: vec![text_label! { content: "×".to_string(), font_size: 14.0, color: FAINT }],
    }
}

/// 筛选分段按钮：激活态高亮，文字用 `if_node` 声明式切换两个文本变体。
#[rec]
fn filter_btn(
    filter: Signal<Filter>,
    which: Filter,
    text: &'static str,
    #[props(required)] on_click: impl Fn() + 'static,
) -> Fc {
    let f_draw = filter;
    let f_text = filter;
    node2! {
        layout:
            flex! {
                direction: Layout::Row,
                align_item: AlignItem::Center,
            },
        padding_inline_start: 12.0,
        padding_inline_end: 12.0,
        padding_block_start: 5.0,
        padding_block_end: 5.0,
        cursor: CursorType::Pointer,
        mouse_click: move |_e| on_click(),
        draw: move |s: &mut Scene, n: &Node2| {
            let bg = if f_draw.get() == which { ACCENT } else { PANEL };
            s.fill_round_rect(Rect::new(0.0, 0.0, n.width(), n.height()), 6.0, bg);
        },
        children: vec![
            if_node(
                move || f_text.get() == which,
                move |cx: &mut NodeContext| cx.child(small_text! { text: text, color: BG }),
                Some(move |cx: &mut NodeContext| cx.child(small_text! { text: text, color: MUTED })),
            ),
        ],
    }
}

/// 主按钮：accent 底板 + 深色文字子节点（文字是内容 → 由它撑起自然尺寸）。
#[rec]
fn primary_btn(text: &'static str, #[props(required)] on_click: impl Fn() + 'static) -> Fc {
    node2! {
        layout:
            flex! {
                direction: Layout::Row,
                align_item: AlignItem::Center,
            },
        padding_inline_start: 14.0,
        padding_inline_end: 14.0,
        padding_block_start: 6.0,
        padding_block_end: 6.0,
        cursor: CursorType::Pointer,
        mouse_click: move |_e| on_click(),
        draw: move |s: &mut Scene, n: &Node2| {
            s.fill_round_rect(Rect::new(0.0, 0.0, n.width(), n.height()), 6.0, ACCENT);
        },
        children: vec![small_text! { text: text, color: BG }],
    }
}

/// 输入框：canvasRender2 自己的 `EditableTextNode`（TS `EditableTextNode.ts`）。
///
/// 它不是 v1 的 `text_field`：焦点、选区、光标、IME、撤销全在本体系内。
/// 声明风格与其它基础组件完全一致——`editable_text_node!` 宏收命名参数，基类
/// 字段（`height` / `padding_*` / `exts` / `draw` …）由 `#[rec(extends = node2)]`
/// 内联转发，调用处不再出现 `XxxArg { .. }` 与 `rest:`。
#[rec]
fn text_field(input: Signal<String>) -> Fc {
    let text = get(move || input.get());
    editable_text_node! {
        text: text(),
        on_text_change: move |t: &str| input.set(t.to_string()),
        placeholder: "要做点什么？".to_string(),
        single_line: true,
        height: 32.0,
        font_size: 13.0,
        color: TEXT,
        exts: vec![grow(1.0)],
        padding_inline_start: 10.0,
        padding_inline_end: 10.0,
        draw: |s: &mut Scene, n: &Node2| {
            let (w, h) = (n.width(), n.height());
            s.fill_round_rect(Rect::new(0.0, 0.0, w, h), 6.0, PANEL);
            s.stroke_round_rect(
                Rect::new(0.5, 0.5, (w - 1.0).max(0.0), (h - 1.0).max(0.0)),
                6.0,
                LINE,
                1.0,
            );
        },
    }
}

/// 12px 单行文本（`WrappedTextNode` 的薄封装，供按钮/分段控件内联使用）。
#[rec]
fn small_text(
    #[getter]
    #[props(default = String::new())]
    text: String,
    #[getter]
    #[props(default = TEXT)]
    color: wy_render::Color,
) -> Fc {
    wrapped_text_node! {
        text: text(),
        font_size: 12.5,
        color: color(),
    }
}

// ============================================================================
// 模型操作（纯信号，无 UI 知识）
// ============================================================================

/// 某待办在给定筛选下是否可见（纯函数，与 UI 解耦，便于单测）。
fn visible_under(f: Filter, done: bool) -> bool {
    match f {
        Filter::All => true,
        Filter::Active => !done,
        Filter::Done => done,
    }
}

fn add(input: &Signal<String>, ids: &Signal<Vec<u64>>, next_id: &Signal<u64>, store: &TodoStore) {
    let text = input.get().trim().to_string();
    if text.is_empty() {
        return;
    }
    let id = next_id.get();
    next_id.set(id + 1);
    let (t, d) = store.of(id);
    t.set(text);
    d.set(false);
    let mut v = ids.get();
    v.push(id);
    ids.set(v);
    input.set(String::new());
}

// ============================================================================
// 测试：模型操作是纯函数，直接单测
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (Signal<Vec<u64>>, TodoStore, Signal<String>, Signal<u64>) {
        let ids: Signal<Vec<u64>> = Signal::new(vec![1, 2, 3]);
        let store = TodoStore::default();
        for (id, text, done) in [(1u64, "a", true), (2, "b", false), (3, "c", false)] {
            let (t, d) = store.of(id);
            t.set(text.into());
            d.set(done);
        }
        (ids, store, Signal::new(String::new()), Signal::new(4))
    }

    #[test]
    fn add_appends_and_clears_input() {
        let (ids, store, input, next_id) = fixture();
        input.set("  新任务  ".into());
        add(&input, &ids, &next_id, &store);
        assert_eq!(ids.get(), vec![1, 2, 3, 4]);
        assert_eq!(store.text_of(4).get(), "新任务");
        assert!(!store.done_of(4).get());
        assert!(input.get().is_empty(), "添加后输入框应清空");
        assert_eq!(next_id.get(), 5);
    }

    #[test]
    fn add_ignores_blank() {
        let (ids, store, input, next_id) = fixture();
        input.set("   ".into());
        add(&input, &ids, &next_id, &store);
        assert_eq!(ids.get().len(), 3, "空白输入不应入列");
    }

    #[test]
    fn toggle_only_writes_that_item() {
        let (ids, store, _input, _next_id) = fixture();
        assert!(store.done_of(1).get());
        store.done_of(1).set(false);
        assert!(!store.done_of(1).get());
        assert!(!store.done_of(2).get(), "其它项不受影响");
        assert_eq!(ids.get(), vec![1, 2, 3], "勾选不动结构信号");
    }

    #[test]
    fn clear_done_keeps_open_items() {
        let (ids, store, _input, _next_id) = fixture();
        let kept: Vec<u64> = ids
            .get()
            .into_iter()
            .filter(|id| !store.done_of(*id).get())
            .collect();
        ids.set(kept);
        assert_eq!(ids.get(), vec![2, 3]);
    }

    #[test]
    fn filter_predicate_matches_expected_items() {
        assert!(visible_under(Filter::All, true));
        assert!(visible_under(Filter::All, false));
        assert!(!visible_under(Filter::Active, true));
        assert!(visible_under(Filter::Active, false));
        assert!(visible_under(Filter::Done, true));
        assert!(!visible_under(Filter::Done, false));
    }
}
