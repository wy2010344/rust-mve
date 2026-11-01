//! 父节点栈与整棵 Node2 树的组装入口（`build_root_node2` / `expand_node2`）。

use std::cell::RefCell;

use crate::canvas_render2::element::Node2Props;
use crate::{Fc, NodeContext};

use super::*;

// ============================================================================
// 父节点栈（TS `ctx.getParent()`）
// ============================================================================

thread_local! {
    /// 正在展开子节点的 `Node2` 栈：子组件构造时据此认父。
    static PARENT_STACK: RefCell<Vec<Node2>> = const { RefCell::new(Vec::new()) };
}

/// 当前正在展开子节点的父节点（TS `ctx.getParent()`）。
#[must_use]
pub fn current_parent() -> Option<Node2> {
    PARENT_STACK.with(|s| s.borrow().last().cloned())
}

/// 纯 Node2 模式下，把节点的子孙 `child_fcs` 递归展开成 Node2 活树。
///
/// 与 v1 引擎驱动 `to_node` 的 `arg_children_fn` 等价，但不产生 v1 `Node` IR：
/// 每个子 `Fc` 经 [`Node2::mount`] 走 `attach_to(parent)` 登记进父的 Node2 子节点，
/// 然后继续向下展开。产物是可直接 [`Node2::render_node2`] 的树。
///
/// 由 [`build_root_node2`] 调用；单独调用也可用于"先建父、再动态挂子"后补展开。
///
/// # 为什么用 `raw_children()` 而不是 `children()`
///
/// `children()` 会把 `hide()` 为真的子节点滤掉——那是**运行期**语义（不绘制、
/// 不命中、不进焦点链）。构建期若也按它递归，隐藏子树（对话框 / 下拉菜单在
/// 关闭状态）的子孙就永远不会被展开，等到 `open` 变 true 时里面是空的：一次
/// 性建树 + 运行期显隐的声明式浮层就此失效。所以展开必须走原始列表，把整棵树
/// 一次性建全。
pub fn expand_node2(node: &Node2) {
    let mut cx = NodeContext::new();
    cx.node2_parent = Some(node.clone());
    node.mount_children(&mut cx);
    for c in node.raw_children() {
        expand_node2(&c);
    }
}

/// 纯 Node2 组装入口（对标 `render_root`，但产出 `Node2` 活树、不走 v1 `to_node`）。
///
/// `root_arg` 是画布根（TS 画布根在原点 `(0,0)`、尺寸等于视口）；`page` 用
/// `cx.child(..)` 把声明式页面挂到根下。返回的根节点可直接交给
/// [`crate::canvas_render2::tree::geometry::hit_path`] / [`Node2::render_node2`]，
/// 或包进 `wy_engine::node2_app::Node2App` 跑起来。
pub fn build_root_node2(
    root_arg: Node2Props,
    page: impl FnOnce(&mut NodeContext) + 'static,
) -> Node2 {
    // 新树要重建：旧树登记的动态区域探针已经指向废弃节点。
    crate::canvas_render2::tree::engine_global::clear_region_probes();
    let root = Node2::empty().build(root_arg);
    let mut cx = NodeContext::new();
    cx.node2_parent = Some(root.clone());
    page(&mut cx);
    expand_node2(&root);
    root
}

/// 声明式画布根：[`build_root_node2`] 的 `Fc` 版——根节点本身就是 `node2!` 写的
/// 一棵普通声明式树，不再单独传一份 `Node2Props`。
///
/// ```ignore
/// Node2App::new(move |viewport| {
///     build_root_fc(node2! {
///         x: 0.0, y: 0.0,
///         width: viewport.get().0,
///         height: viewport.get().1,
///         layout: flex! { direction: Layout::Column, gap: 12.0 },
///         children: vec![header(), list(), toolbar()],
///     })
/// })
/// ```
///
/// # 为什么要绕一层"探针父"
///
/// `Fc` 是 `FnOnce(&mut NodeContext)` 的黑盒：它把造出的节点经 [`Node2::mount`]
/// 挂到上下文携带的父上，**不返回**句柄。所以这里给它一个一次性探针父，节点挂
/// 上来后立刻 [`Node2::detach`] 断开——此后它就是没有父的根（`parent().is_none()`，
/// 与 [`build_root_node2`] 的产物完全同构：布局、绝对坐标、命中测试都以它为原点）。
///
/// 根没有父级，`x`/`y` 无从推导（`default_position` 要问父布局），所以**根必须
/// 显式声明 `x: 0.0, y: 0.0`**——这一点两个入口一致。
pub fn build_root_fc(page: Fc) -> Node2 {
    // 新树要重建：旧树登记的动态区域探针已经指向废弃节点。
    crate::canvas_render2::tree::engine_global::clear_region_probes();
    let probe = Node2::empty().build(Node2Props::default());
    let mut cx = NodeContext::new();
    cx.node2_parent = Some(probe.clone());
    page.mount(&mut cx);
    let root = probe
        .raw_children()
        .into_iter()
        .next()
        .expect("根 Fc 必须造出一个节点（`node2! { .. }`）");
    root.detach();
    expand_node2(&root);
    root
}

pub(super) struct ParentScope;

impl ParentScope {
    pub(super) fn push(n: Node2) -> Self {
        PARENT_STACK.with(|s| s.borrow_mut().push(n));
        Self
    }
}

impl Drop for ParentScope {
    fn drop(&mut self) {
        PARENT_STACK.with(|s| {
            s.borrow_mut().pop();
        });
    }
}

// ============================================================================
// 几何谓词（TS `Node.ts` 的 `absoluteInInner` / `size`）
// ============================================================================

/// 节点尺寸（TS `size(node, d)` = `node.width()` / `node.height()`）。
///
/// **不是**"本帧布局分到的盒子"（那是 [`Node2::geom`]）：TS 的 `width()` /
/// `height()` 是读取式 getter，容器节点没显式声明尺寸时返回**子节点撑出来的
/// 自然尺寸**（`defaultSize` → `sizeFromChildren`）。
///
/// 滚动容器的 `contentSize` 靠的正是这个语义——它要的是"内容有多长"，
/// 而不是"视口有多高"（后者是布局盒，两者在滚动场景里恰好长度不同）。
///
/// 实现**必须走声明层数据**而不是 `to_node()`：`to_node()` 的子节点展开依赖
/// `child_fcs`，而它们在真正挂载时被消费掉（引擎侧 memo 一次），二次 `to_node()`
/// 展开不出子节点。TS 那边 `node.children` 是 `ctx.renderNode` 的**活树**，
/// `sizeFromChildren` 每次都读同一棵；这里对应的活数据就是 [`Node2::children`]，
/// 挂载时由 `Node2::mount` 登记维护。
#[must_use]
pub fn size(node: &Node2, d: Axis) -> f32 {
    {
        let dd = node.0.data.borrow();
        // 1) 组件显式给的 size 闭包（rich_text / wrapped_text 等）。
        if let Some(f) = &dd.size {
            let ls = f(d);
            // `from_inside`：外包尺寸 = value + 两端 padding（TS `toOuterSize`）。
            let v = if ls.from_inside {
                let (s, e) = match d {
                    Axis::X => (node.padding_inline_start(), node.padding_inline_end()),
                    Axis::Y => (node.padding_block_start(), node.padding_block_end()),
                };
                ls.value + s + e
            } else {
                ls.value
            };
            if v > 0.0 {
                return v;
            }
        }
        // 2) 显式宽/高（TS `node.width()` / `node.height()`）。
        let declared = match d {
            Axis::X => dd.width.as_ref().map(|g| g()),
            Axis::Y => dd.height.as_ref().map(|g| g()),
        };
        if let Some(v) = declared {
            if v > 0.0 {
                return v;
            }
        }
    }
    // 3) 容器：子节点自然尺寸沿轴累加——主轴相加 + gap×(n−1)，交叉轴取最大。
    //    对应 TS `layoutValue(node,d).sizeFromChildren()`（flex 的 `cache().length`
    //    同样把 gap 计入总长）。
    let layout = node.layout();
    let dir = layout.dir();
    let kids = node.layout_children();
    if kids.is_empty() {
        return 0.0;
    }
    if axis_of(dir) == d {
        let sum: f32 = kids.iter().map(|k| size(k, d)).sum();
        let gap = layout.gap().max(0.0);
        sum + gap * kids.len().saturating_sub(1) as f32
    } else {
        kids.iter().map(|k| size(k, d)).fold(0.0, f32::max)
    }
}

/// 画布绝对坐标 `(x, y)` 是否落在节点的 **inner 区**（内容区）里。
///
/// TS 原文：
/// ```ts
/// const innerX = absolutePosition(node, 'x') + node.paddingInlineStart();
/// const innerY = absolutePosition(node, 'y') + node.paddingBlockStart();
/// return innerX < x && x < innerX + innerSize(node, 'x')
///     && innerY < y && y < innerY + innerSize(node, 'y');
/// ```
/// 滚动容器用它判定"滚轮是不是该由我消费"——指针在视口外就不接管。
#[must_use]
pub fn absolute_in_inner(node: &Node2, x: f32, y: f32) -> bool {
    let g = node.node_ref();
    let (ix, iy) = g.abs_inner();
    let (iw, ih) = g.inner();
    ix < x && x < ix + iw && iy < y && y < iy + ih
}

// ============================================================================
// 挂载辅助：`Node2` → `Fc`
// ============================================================================

/// `Node2` → `Fc`：新体系组件工厂（`rect_node` / `rich_text_node` / `scroll` /
/// `editable_text_node` 等）在 `build` 之后用它把节点摊进 `children`。
/// 这是新体系内部转换（`Node2` → 新 `Fc`），不是旧一代互操作桥接。
impl From<Node2> for Fc {
    fn from(n: Node2) -> Self {
        Fc::new(move |cx| n.mount(cx))
    }
}
//
// 旧一代互操作桥接（`From<Node2> for Slot/ChildArg`）在 `EditableTextNode` /
// `Scroll` 复刻完成后已删除：新体系组件只产出 `Fc`，不再需要把 `Node2`
