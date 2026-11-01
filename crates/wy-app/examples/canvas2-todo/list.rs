//! 待办列表（对标 TS `canvas-render2/todo-list.ts`）。
//!
//! 关键点，与 TS 注释一一对应：
//!
//! - 条目由 [`for_each_node`] 管理（按 `todo.id` 增量复用/销毁），增删待办
//!   不会重建整个列表；
//! - 滚动条用 macOS 风格浮层式（[`overlay_scroll_bar`]）：**不占布局宽度**、
//!   滚动时浮现、停手自动淡出；滚轮/拖滑块滚动由 scroll 模块负责（内容裁剪见
//!   [`scroll_content`]）；
//! - 空态提示在无可见待办时占位显示（条件分支整体换掉，不占布局）。
//!
//! # 写法：树是声明出来的
//!
//! 卡片是 `rect_node! { .. }`，它的 `children` 里就一项 [`body`]——一份"滚动状态
//! 只造一次、再按条件挂两套子节点"的 `Fc`。分支用 [`if_node`] 产出的 `Fc` 直接
//! 进 `cx.child(..)`，没有 `.mount()` 链，也没有任何 `XxxArg::builder()`。
//!
//! # 容器必须在子构建期从 `ctx.parent()` 取
//!
//! `children` 是懒构建的（父节点造完才 `create` 子），所以"滚动容器 = 我爹"
//! 这件事只能在自己被构建的那一刻问上下文——TS 里那句
//! `createScroll(childCtx.getParent())` 的 Rust 落点就是 [`NodeContext::parent`]。

use std::cell::RefCell;

use wy_mve::canvas_render2::{create_scroll, for_each_node, grow, if_node, Axis, Scroll2};
// 宏在 crate 根（`#[macro_export]`），子模块里同名的是普通函数/模块。
use wy_mve::{
    flex, overlay_scroll_bar, rect_node, scroll_content, wrapped_text_node, AlignItem, Fc, Layout,
    NodeContext,
};

use crate::item::todo_item;
use crate::store::{palette, visible_todos, Todo};

thread_local! {
    /// 最近一次构建出来的滚动句柄。
    ///
    /// 只为**出图核验**服务：壳层（`main`）没有别的途径拿到列表内部的
    /// `Scroll2`（TS 同样不暴露），而"滚动之后长什么样"必须能被快照固定下来。
    /// 运行期没有任何代码读它。
    static LAST_SCROLL: RefCell<Option<Scroll2>> = const { RefCell::new(None) };
}

/// 取最近一次构建的滚动句柄（快照模式下用来把列表滚到指定位置）。
#[must_use]
pub fn last_scroll() -> Option<Scroll2> {
    LAST_SCROLL.with(|s| s.borrow().clone())
}

/// 待办列表（对标 TS `todoList`）。
///
/// 外层是固定 380×340 的圆角卡片，**自身就是滚动容器**：布局取 Row（滚动轴
/// 落在交叉轴 Y），`clip` 把溢出内容裁到 inner 区——三处正好对上 TS 的
/// `flex({direction:'x'}) + scrollContent 的 drawAtParent 裁剪 + acceptClip`。
#[must_use]
pub fn todo_list() -> Fc {
    rect_node! {
        background: Some(palette().card),
        border_color: Some(palette().border),
        border_width: 1.0,
        radius: 10.0,
        width: 380.0,
        height: 340.0,
        padding: 8.0,
        // 容器不写 `clip`：裁剪由内容节点的 `clip_to_parent` 自己声明
        // （TS `scrollContent` 的 `drawAtParent`），与 `todo-list.ts` 一致。
        layout: flex! {
            direction: Layout::Row,
            align_item: AlignItem::Stretch,
            align_fix: true,
        },
        children: vec![body()],
    }
}

/// 列表内容区（TS 里那个 `function (childCtx) { ... }` 子节点）。
///
/// 它自己**不产生节点**，只负责：取父容器 → 建滚动状态 → 按"有无可见待办"
/// 挂两套子节点。滚动状态在这里只造一次，分支重建不会把滚动位置抹回 0。
fn body() -> Fc {
    Fc::new(|cx: &mut NodeContext| {
        let container = cx
            .parent()
            .expect("列表内容必须挂在列表容器下（TS 同样依赖 getParent）");
        let scroll = create_scroll(&container, Axis::Y);
        LAST_SCROLL.with(|s| *s.borrow_mut() = Some(scroll.clone()));

        cx.child(if_node(
            || !visible_todos().is_empty(),
            move |c: &mut NodeContext| {
                // TS `listOf(scrollContent(..), overlayScrollBar(..))`：两者并列，
                // 浮层条排后面 → 画在上层、命中最先命中。
                c.child(content_list(scroll.clone()));
                c.child(overlay_scroll_bar! { scroll: scroll.clone() });
            },
            Some(move |c: &mut NodeContext| {
                c.child(empty_hint());
            }),
        ));
    })
}

/// 滚动内容：撑满剩余宽度、条目横向 stretch、条目间 gap 6。
fn content_list(scroll: Scroll2) -> Fc {
    scroll_content! {
        scroll: scroll,
        exts: vec![grow(1.0)],
        // alignFix 锁定 cross 为自身 inner；stretch 让条目横向撑满内容区
        layout: flex! {
            align_fix: true,
            align_item: AlignItem::Stretch,
            gap: 6.0,
        },
        children: vec![items()],
    }
}

/// 动态列表：`for_each_node` 按 key（`todo.id`）增量复用/销毁条目节点。
fn items() -> Fc {
    for_each_node::<u64, Todo>(
        |emit| {
            for t in visible_todos() {
                emit(t.id, t);
            }
        },
        |_key, each_time, cx| {
            // `each_time` 是这一项的句柄（`Rc<RefCell<Todo>>`），条目被复用时
            // 内容原地更新——与 TS 的 `eachTime.getValue()` 同义。
            cx.child(todo_item(each_time));
        },
    )
}

/// 空态提示：没有可见待办时显示。
fn empty_hint() -> Fc {
    wrapped_text_node! {
        auto_width: true,
        text: "暂无待办，点击下方按钮添加",
        font_size: 13.0,
        color: wy_mve::canvas_render2::placeholder_color(),
    }
}
