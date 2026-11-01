//! canvasRender2 的结构节点与顶层入口。
//!
//! 对标 TS：
//! - `ForEachNode.ts` / `ifNode` → [`for_each_node`] / [`if_node`]
//! - `Renderer.ts`（`renderCanvas`）→ [`render_canvas2`]
//!
//! 基础组件已按 TS 文件拆到独立模块，不再堆在本文件：
//! [`crate::canvas_render2::rect`] / [`crate::canvas_render2::rich_text`] /
//! [`crate::canvas_render2::wrapped_text`]。
//!
//! 顶层入口 [`render_canvas2`] 与 TS `renderCanvas` 同构：根节点走 `Node2.build`
//! （不再自建旧 `Node`），背景由 `draw` 画在根上，子节点经 `children` 展开。

use std::rc::Rc;

use crate::canvas_render2::node2::Node2;
use crate::{Fc, Node2Props, NodeContext};

// ============================================================================
// 条件分支 / 列表（对标 TS `ifNode` / `ForEachNode.ts`）
// ============================================================================

/// 条件分支（对标 TS `ifNode`）：`cond()` 为真渲染 `a`，否则渲染 `b`（可空）。
///
/// 返回 [`Fc`]（TS 的 `Fc`）而**不是**「还差 node」的形态：它自己不产生节点，
/// 只是把条件分支挂进父容器的 children 里（对应 TS `ifNode` 里那句
/// `ctx.renderForEach(...)`）。硬塞一个 node 属于凭空造值，谁也读不到。
#[must_use]
pub fn if_node(
    cond: impl Fn() -> bool + 'static,
    a: impl Fn(&mut NodeContext) + 'static,
    b: Option<impl Fn(&mut NodeContext) + 'static>,
) -> Fc {
    Fc::new(move |cx: &mut NodeContext| {
        cx.render_when(cond, move |v: bool, c: &mut NodeContext| {
            if v {
                a(c);
            } else if let Some(bf) = &b {
                bf(c);
            }
        });
    })
}

/// `forEachNode` 的等价物：keyed 增量列表（按 key 复用，新增/删除只动一项）。
///
/// 同 [`if_node`]：结构组件，返回 `Fc`，不产生节点。
#[must_use]
pub fn for_each_node<K, T>(
    each: impl Fn(&mut dyn FnMut(K, T)) + 'static,
    creater: impl Fn(K, Rc<std::cell::RefCell<T>>, &mut NodeContext) + 'static,
) -> Fc
where
    K: Eq + std::hash::Hash + Clone + 'static,
    T: Clone + PartialEq + 'static,
{
    Fc::new(move |cx: &mut NodeContext| {
        cx.render_for_each(each, creater);
    })
}

// ============================================================================
// 顶层入口（对标 TS `Renderer.ts` 的 `renderCanvas`）
// ============================================================================

/// 顶层入口（对标 TS `renderCanvas`）。
///
/// TS 原文是 `renderCanvas({ canvas, children, draw, padding..., ...rest })`：
/// 根节点 = `{...rest, width, height}`（画布尺寸），`children` 经
/// `renderRoot` + `buildChildren` 挂进根，**没有任何自创默认值**——
/// 布局/间距/背景全由调用方在 `rest` 里声明（背景就是 `rest.draw` 自己画）。
/// Rust 侧画布尺寸由引擎按视口分配给根，不需要 `width`/`height`。
///
/// 返回 [`Fc`](crate::Fc)，作为根 child 直接喂给 `Node2App`（不再有 `.of()`）。
#[must_use]
pub fn render_canvas2(rest: Node2Props, children: Vec<Fc>) -> Fc {
    // TS `{ ...rest, children }`：结构体更新语法就是它。
    let props = Node2Props { children, ..rest };
    Fc::new(move |cx| {
        let node = Node2::empty().build(props);
        node.mount(cx);
    })
}
