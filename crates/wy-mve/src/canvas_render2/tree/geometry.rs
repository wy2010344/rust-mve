//! 尺寸 / 位置的全部**树算法**（TS `tree/geometry.ts`）。
//!
//! 与 TS 同文件、同分两半：
//! - **布局引擎那一半**（会向父级 / 子级递归索要尺寸位置）：
//!   [`default_size`] / [`default_position`] / [`size_from_parent`] /
//!   [`size_from_children`] / [`create_layout`] / [`layout_value`]，
//!   以及布局依赖追踪器 [`layout_dep`] 与成环错误 [`LayoutCycleError`]；
//! - **纯消息那一半**（只发 `children` / `acceptHit` / `x` / `y` / `padding`）：
//!   [`hitest`] / [`hit_path`] / [`absolute_position`] /
//!   [`clip_at_grad_parent_node`]。

use std::cell::RefCell;
use std::fmt;

use wy_layout::Layout as WyLayout;

use crate::canvas_render2::node2::{Axis, LayoutSize, Node2, StartEnd};
use crate::canvas_render2::tree::layout_direction::{layout_fun_of, LayoutInside2};

// ============================================================================
// 尺寸约束（TS `layoutSize0` / `layoutSize`）
// ============================================================================

/// TS `layoutSize0`：零尺寸、按内容区解释。
pub const LAYOUT_SIZE_0: LayoutSize = LayoutSize {
    value: 0.0,
    from_inside: true,
};

/// TS `layoutSize(value, fromInside)`。
#[must_use]
pub fn layout_size(value: f32, from_inside: bool) -> LayoutSize {
    LayoutSize { value, from_inside }
}

// ============================================================================
// 布局对象（TS `createLayout` / `layoutValue`）
// ============================================================================

/// 取某个轴上的布局对象（TS `createLayout(node, d)`）。
///
/// # 为什么要 memo
///
/// TS 的 `layoutX()` 是 `layoutXMemo ??= createLayout(this, 'x')`，memo 保证
/// **同一帧内同一 (节点, 轴) 只构造一次**——这不只是性能优化，是打破布局环的
/// 唯一手段：父布局的构造必然要读子项尺寸，而子项若"尺寸取父分配"
/// （`size_from_parent`）又会回头读父布局；没有 memo 就是无限递归。
///
/// 这里的失效口径是**每帧清空**（`Node2App` / v1 引擎在绘制前调
/// [`clear_layout_memo`]）。TS 用信号依赖追踪决定是否重算，Rust 侧尚未把
/// `layoutX/layoutY` 接进依赖系统；一帧内几何是确定的，每帧重算是正确且最省心
/// 的那个选择。
///
/// # 环闭合占位
///
/// 构造进行中若再次被请求（父 → 子 → 父），返回一条 [`AbsoluteLayout`]：它
/// `child_size` 报错 → 上层落 0，与 TS "memo 结果还是 `undefined` → `?? 0`"
/// 同效。这条路径只会出现在"子节点想问父级要尺寸、而父级正在测它"这一瞬，
/// 此时父级本来就不会用到它的自然尺寸（grow 按权重分、Stretch 按父 inner 分）。
#[must_use]
pub fn create_layout(node: &Node2, d: Axis) -> std::rc::Rc<dyn WyLayout> {
    // 探测模式：只记边，不缓存（TS `if (layoutDep.probeMode) return () => ...`）
    if with_layout_dep(|dep| dep.probe_mode) {
        return build_layout(node, d);
    }
    let key = (node.id(), d);
    if let Some(hit) = LAYOUT_MEMO.with(|m| m.borrow().get(&key).cloned()) {
        return hit;
    }
    // 先占位再构造：构造期重入拿到的是"空布局"，不是无限递归
    let placeholder: std::rc::Rc<dyn WyLayout> = std::rc::Rc::new(wy_layout::AbsoluteLayout::new());
    LAYOUT_MEMO.with(|m| {
        m.borrow_mut().insert(key, std::rc::Rc::clone(&placeholder));
    });
    let built = build_layout(node, d);
    LAYOUT_MEMO.with(|m| {
        m.borrow_mut().insert(key, std::rc::Rc::clone(&built));
    });
    built
}

/// 清空布局 memo。每帧绘制前调用（见 [`create_layout`] 的失效口径）。
pub fn clear_layout_memo() {
    LAYOUT_MEMO.with(|m| m.borrow_mut().clear());
}

// 布局 memo：`(节点 id, 轴) → 已构造的布局对象`。
// 单线程 UI，收在 `thread_local` 上；与 TS 的 `layoutXMemo` 字段一一对应。
thread_local! {
    static LAYOUT_MEMO: RefCell<std::collections::HashMap<(u64, Axis), std::rc::Rc<dyn WyLayout>>> =
        RefCell::new(std::collections::HashMap::new());
}

/// 真正构造布局对象（不做 memo，供 [`create_layout`] 与探测模式用）。
fn build_layout(node: &Node2, d: Axis) -> std::rc::Rc<dyn WyLayout> {
    let kids = node.layout_children();
    let ask = || inner_size(node, d);
    let inside = LayoutInside2 {
        children: &kids,
        inner_size: &ask,
    };
    layout_fun_of(node, d).create_layout(&inside)
}

/// 指定轴起/止侧的内边距（TS `getPadding(node, d, side)`）。
#[must_use]
pub fn get_padding(node: &Node2, d: Axis, side: StartEnd) -> f32 {
    node.padding(d, side)
}

/// 起始侧内边距（TS `paddingStart`）。
#[must_use]
pub fn padding_start(node: &Node2, d: Axis) -> f32 {
    get_padding(node, d, StartEnd::Start)
}

/// 结束侧内边距（TS `paddingEnd`）。
#[must_use]
pub fn padding_end(node: &Node2, d: Axis) -> f32 {
    get_padding(node, d, StartEnd::End)
}

/// 外包尺寸（TS `size(node, d)`）：顺带在探测模式下记一条 READ 边。
#[must_use]
pub fn size(node: &Node2, d: Axis) -> f32 {
    record_size_read(node, d);
    match d {
        Axis::X => node.width(),
        Axis::Y => node.height(),
    }
}

/// 内容区尺寸（TS `innerSize(node, d)`）。
#[must_use]
pub fn inner_size(node: &Node2, d: Axis) -> f32 {
    (size(node, d) - padding_start(node, d) - padding_end(node, d)).max(0.0)
}

/// 该轴上的布局对象（TS `layoutValue(node, d)` = `layoutX()` / `layoutY()`）。
#[must_use]
pub fn layout_value(node: &Node2, d: Axis) -> std::rc::Rc<dyn WyLayout> {
    match d {
        Axis::X => node.layout_x(),
        Axis::Y => node.layout_y(),
    }
}

/// 由子节点撑出的尺寸（TS `sizeFromChildren`）。
#[must_use]
pub fn size_from_children(node: &Node2, d: Axis) -> LayoutSize {
    LayoutSize {
        value: with_reader(node, || {
            layout_value(node, d).size_from_children().unwrap_or(0.0)
        }),
        from_inside: true,
    }
}

// ============================================================================
// 布局依赖追踪（TS `layoutDep` / `trackSize` / 探测模式）
// ============================================================================

/// 布局依赖成环错误（TS `LayoutCycleError`）。
///
/// TS 刻意不继承 `LayoutError`——`default_position` 会捕获 `LayoutError` 并降级
/// 成"退化为 padding start"，成环若被吞掉就永远看不见。
#[derive(Debug, Clone)]
pub struct LayoutCycleError {
    /// 完整依赖链与修法说明。
    pub message: String,
}

impl fmt::Display for LayoutCycleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for LayoutCycleError {}

/// 探测模式下记录的一条依赖边：`child` 在 `d` 方向上与 `parent` 互依
/// （TS `LayoutProbeEdge2`）。
#[derive(Clone)]
pub struct LayoutProbeEdge2 {
    /// 父级。
    pub parent: Node2,
    /// 子级。
    pub child: Node2,
    /// 互依的轴。
    pub d: Axis,
}

/// 布局依赖追踪器（TS `layoutDep`）。
///
/// TS 里它是模块级可变对象；Rust 没有模块级可变状态，收到 `thread_local` 上，
/// 经 [`with_layout_dep`] 访问——语义一致（单线程 UI，一处全局状态）。
#[derive(Default)]
pub struct LayoutDep {
    /// 诊断开关（置 false 可完全关掉依赖追踪）。
    pub enabled: bool,
    /// 当前正在求值的"向父级索要尺寸"边。
    pub stack: Vec<(u64, Axis)>,
    /// 探测模式开关（[`LayoutDep::probe_mode`] 为真时只记边、不递归）。
    pub probe_mode: bool,
    /// 正在测量的容器栈：栈顶即"谁在读子项尺寸"。
    pub readers: Vec<Node2>,
    /// ASK 边：子节点向父级索要尺寸。
    pub ask_edges: Vec<LayoutProbeEdge2>,
    /// READ 边：父级测量子项尺寸。
    pub read_edges: Vec<LayoutProbeEdge2>,
}

impl LayoutDep {
    /// 默认：开启追踪、非探测模式。
    #[must_use]
    pub fn new() -> Self {
        Self {
            enabled: true,
            ..Self::default()
        }
    }
}

thread_local! {
    static LAYOUT_DEP: RefCell<LayoutDep> = RefCell::new(LayoutDep::new());
}

/// 访问（可变）布局依赖追踪器。
pub fn with_layout_dep<R>(f: impl FnOnce(&mut LayoutDep) -> R) -> R {
    LAYOUT_DEP.with(|c| f(&mut c.borrow_mut()))
}

/// 取走并清空探测边（TS `takeProbeEdges`），返回 `(asks, reads)`。
#[must_use]
pub fn take_probe_edges() -> (Vec<LayoutProbeEdge2>, Vec<LayoutProbeEdge2>) {
    with_layout_dep(|d| {
        (
            std::mem::take(&mut d.ask_edges),
            std::mem::take(&mut d.read_edges),
        )
    })
}

/// 记录一条 READ 边（父级测量子项尺寸）。只在探测模式下记账。
fn record_size_read(node: &Node2, d: Axis) {
    with_layout_dep(|dep| {
        if !dep.probe_mode {
            return;
        }
        if let Some(parent) = dep.readers.last().cloned() {
            dep.read_edges.push(LayoutProbeEdge2 {
                parent,
                child: node.clone(),
                d,
            });
        }
    });
}

/// 以 `node` 为"正在测量者"执行一段求值（期间子项的尺寸读取都记到它头上）。
///
/// TS 用 `try/finally` 保证出栈；Rust 这里没有作用域守卫，异常（panic）路径下
/// `readers` 会残留——与 TS 的差别仅在"布局 panic 之后"这一不可恢复路径。
fn with_reader<T>(node: &Node2, run: impl FnOnce() -> T) -> T {
    let probe = with_layout_dep(|d| d.probe_mode);
    if !probe {
        return run();
    }
    with_layout_dep(|d| d.readers.push(node.clone()));
    let out = run();
    with_layout_dep(|d| {
        d.readers.pop();
    });
    out
}

fn describe_size_edge(node: &Node2, d: Axis) -> String {
    let name = node.debug_name().unwrap_or_else(|| "节点".to_string());
    let dir = if d == Axis::X { "宽" } else { "高" };
    format!("{name}({dir})")
}

/// 追踪一条"向父级索要尺寸"的边：同一 `(节点, 轴)` 二次进入即成环。
///
/// TS 抛 `LayoutCycleError`；Rust 侧的处理见 [`size_from_parent`] 的文档：
/// 环闭合处**返回 0**（与 TS 里"memo 已存在但结果还没算出来"的取值一致），
/// 置 `WY_LAYOUT_CYCLE_PANIC=1` 时才 panic，并把依赖链与节点信息打出来。
fn track_size<T>(node: &Node2, d: Axis, run: impl FnOnce() -> T) -> Option<T> {
    let enabled = with_layout_dep(|d| d.enabled);
    if !enabled {
        return Some(run());
    }
    let id = node.id();
    let repeated = with_layout_dep(|dep| dep.stack.iter().any(|it| it.0 == id && it.1 == d));
    if repeated {
        if std::env::var_os("WY_LAYOUT_CYCLE_PANIC").is_some() {
            let chain = with_layout_dep(|dep| {
                dep.stack
                    .iter()
                    .map(|it| format!("{}({})", it.0, if it.1 == Axis::X { "宽" } else { "高" }))
                    .collect::<Vec<_>>()
                    .join(" → ")
            });
            panic!(
                "布局依赖成环：{} 的尺寸依赖父级，而父级正在测量它。\n依赖链：{} → {}\n\
                 修法（二选一）：\n  \
                 1) 让该节点有确定尺寸（不依赖父级）：返回 LayoutSize::outer(测得值)；\n  \
                 2) 让它成为 grow 子节点（grow(1.0)），由父级分配尺寸。",
                format!(
                    "{} [id={} parent={:?} layout={:?} kids={} exts={} decl={:?}]",
                    describe_size_edge(node, d),
                    node.id(),
                    node.parent().map(|p| p.id()),
                    node.layout().dir(),
                    node.children().len(),
                    node.exts().len(),
                    node.declared(d)
                ),
                chain,
                describe_size_edge(node, d)
            );
        }
        return None;
    }
    with_layout_dep(|dep| dep.stack.push((id, d)));
    let out = run();
    with_layout_dep(|dep| {
        dep.stack.pop();
    });
    Some(out)
}

// ============================================================================
// 默认尺寸 / 默认位置（TS `sizeFromParent` / `defaultSize` / `defaultPosition`）
// ============================================================================

/// 向父级索要尺寸 —— 全仓库唯一会让"子节点回读父级"发生的位置
/// （TS `sizeFromParent`）。
///
/// 探测模式下只记一条 ASK 边并返回占位尺寸，绝不回头问父级：否则环闭合处会
/// 直接爆栈，一次只能拿到第一个环。
///
/// # 成环时为什么返回 0 而不是崩
///
/// 父级正在测某个子节点、该子节点又回头问父级要尺寸——这在"父按子节点最大
/// 尺寸定交叉轴"的栈布局里是常态（例如一个没有内容、只带 `grow` 的占位子节点）。
/// TS 那边不会崩，是因为 `layoutX()` 先把布局对象 memo 住、结果字段此刻还是
/// `undefined`，读到就落成 `?? 0`。Rust 的布局是"构造即求值"，没有这个中间
/// 状态，于是把环闭合处取值直接定成 0——**与 TS 取到 `undefined ?? 0` 同效**。
/// 需要排查依赖链时置 `WY_LAYOUT_CYCLE_PANIC=1`，退化成带完整诊断信息的 panic。
#[must_use]
pub fn size_from_parent(node: &Node2, d: Axis) -> LayoutSize {
    let parent = node.parent().expect("未找到父节点");
    let probe = with_layout_dep(|dep| dep.probe_mode);
    if probe {
        with_layout_dep(|dep| {
            dep.ask_edges.push(LayoutProbeEdge2 {
                parent: parent.clone(),
                child: node.clone(),
                d,
            })
        });
        return LAYOUT_SIZE_0;
    }
    track_size(node, d, || LayoutSize {
        value: with_reader(&parent, || {
            layout_value(&parent, d)
                .child_size(node.layout_index())
                .unwrap_or(0.0)
        }),
        from_inside: false,
    })
    .unwrap_or(LAYOUT_SIZE_0)
}

/// 默认尺寸逻辑（TS `defaultSize`）：布局允许由内容推导时取内容尺寸，
/// 否则取父布局分配的子尺寸。
///
/// # 为什么要先问工厂、再造布局
///
/// TS 的 `defaultSize` 是
/// `layoutValue(n,d).allowSizeFromChildren() ? sizeFromChildren() : sizeFromParent()`，
/// 之所以不自递归，是因为 TS 的 `layoutX()` **只建对象**、结果在各方法里惰性求值。
/// Rust 的 `FlexLayout` 是"构造即求值"：constructor 一进 grow 分支就要读容器
/// 可用尺寸 → `size(node,d)` → `node.width()` → 又回到这里。所以判定必须走
/// 工厂层的 [`LayoutFun2::allows_size_from_children`]（只看配置与子节点扩展），
/// **绝不能**为了判定就先 `layout_value`。
#[must_use]
pub fn default_size(node: &Node2, d: Axis) -> LayoutSize {
    if layout_fun_of(node, d).allows_size_from_children(node) {
        LayoutSize {
            value: with_reader(node, || {
                layout_value(node, d).size_from_children().unwrap_or(0.0)
            }),
            from_inside: true,
        }
    } else {
        size_from_parent(node, d)
    }
}

/// 默认位置逻辑（TS `defaultPosition`）：父布局分配的子位置 + 父的 padding start；
/// `LayoutError`（未参与布局）时退化为 padding start。
#[must_use]
pub fn default_position(node: &Node2, d: Axis) -> f32 {
    let parent = node.parent().expect("未找到父节点");
    let start = get_padding(&parent, d, StartEnd::Start);
    let placed = with_reader(&parent, || {
        layout_value(&parent, d).child_position(node.layout_index())
    });
    match placed {
        Ok(v) => v + start,
        // TS 只捕获 LayoutError；Rust 的 child_position 只在这一类失败时返 Err
        Err(_) => start,
    }
}

// ============================================================================
// 纯消息算法：命中 / 绝对坐标 / 祖父裁剪（TS `tree/geometry.ts` 后半段）
// ============================================================================

/// 全局（画布）坐标：沿父链累加（只发 `x()` / `y()` / `parent` 消息，TS `absolutePosition`）。
///
/// 与 `absolute_in_inner` 用 `geom.abs_inner()` 不同：这里走**父链消息**，
/// 不依赖布局写回的 `geom`——未布局的树也能求绝对坐标（命中测试在挂载后即可用）。
#[must_use]
pub fn absolute_position(node: &Node2, axis: Axis) -> f32 {
    match node.parent() {
        Some(p) => {
            let local = match axis {
                Axis::X => node.x(),
                Axis::Y => node.y(),
            };
            local + absolute_position(&p, axis)
        }
        // 根节点没有父布局可问：声明了位置就用声明的，否则是画布原点 0。
        // （`node.x()` 在这里会走 `default_position` 的 `expect("未找到父节点")`。）
        None => node.explicit_position(axis).unwrap_or(0.0),
    }
}

/// 全局横坐标（TS `absoluteX`）。
#[must_use]
pub fn absolute_x(node: &Node2) -> f32 {
    absolute_position(node, Axis::X)
}

/// 全局纵坐标（TS `absoluteY`）。
#[must_use]
pub fn absolute_y(node: &Node2) -> f32 {
    absolute_position(node, Axis::Y)
}

/// 命中链条目（TS `NodeHit2`）。
///
/// TS 侧 `NodeHit2` 是 `{ node, x, y }` 这么一个**结构类型**，`NodeWithPosition2`
/// 只是在它上面多挂一个 `next`；`selection::HitestItem2` 也是同形载体（坐标用 f64）。
/// TS 靠结构子类型让 `dispatchMousePath(path: readonly NodeHit2[])` 同时吃两者，
/// Rust 没有结构子类型，所以用一个只暴露节点的小 trait 接住它们——
/// 只问节点的那几个算法（`cursorOfPath` / `pathIncludes` / `focusableLeafOf`）
/// 就能两边通用。
pub trait NodeHit2 {
    /// 该节命中的节点。
    fn hit_node(&self) -> &Node2;
}

impl NodeHit2 for NodeWithPosition2 {
    fn hit_node(&self) -> &Node2 {
        &self.node
    }
}

impl NodeHit2 for crate::canvas_render2::selection::HitestItem2 {
    fn hit_node(&self) -> &Node2 {
        &self.node
    }
}

/// 命中链的一节（结构化最小集）：`node` 是当前节点，`x` / `y` 是**相对该节点**的局部坐标。
///
/// 只声明成普通数据（不含消息），上层可以在不了解节点的前提下遍历命中链。
/// 对标 TS `NodeHit2` / `NodeWithPosition2`。
pub struct NodeWithPosition2 {
    pub node: Node2,
    pub x: f32,
    pub y: f32,
    /// 指向更深一节的节点（首节为根，末节为最深命中，`None` = 叶）。
    pub next: Option<Box<NodeWithPosition2>>,
}

impl NodeWithPosition2 {
    /// 展平为 `root → leaf` 数组（TS `hitPath` 的内层累加）。
    #[must_use]
    pub fn collect(&self) -> Vec<NodeWithPosition2> {
        let mut out = Vec::new();
        let mut cur = Some(self);
        while let Some(n) = cur {
            out.push(NodeWithPosition2 {
                node: n.node.clone(),
                x: n.x,
                y: n.y,
                next: None,
            });
            cur = n.next.as_deref();
        }
        out
    }
}

/// 命中测试：自叶向根深度优先，返回命中链（首节为根，末节为最深命中节点）。
///
/// 只发消息：`children()` / `acceptClip()` / `x()` / `y()` / `acceptHit()`。
/// 对标 TS `hitest`。
#[must_use]
pub fn hitest(node: &Node2, x: f32, y: f32) -> Option<NodeWithPosition2> {
    let local_x = x - node.x();
    let local_y = y - node.y();
    for child in node.children().iter().rev() {
        if child.accept_clip(local_x, local_y) {
            if let Some(found) = hitest(child, local_x, local_y) {
                return Some(NodeWithPosition2 {
                    node: node.clone(),
                    x: local_x,
                    y: local_y,
                    next: Some(Box::new(found)),
                });
            }
        }
    }
    if node.accept_hit(local_x, local_y) {
        return Some(NodeWithPosition2 {
            node: node.clone(),
            x: local_x,
            y: local_y,
            next: None,
        });
    }
    None
}

/// 命中链展平成数组（`root → leaf`，TS `hitPath`）。
///
/// 派发与光标派生都按这个顺序走，比链式 `next` 更好读，也便于 ooc 侧用数组表达。
#[must_use]
pub fn hit_path(root: &Node2, x: f32, y: f32) -> Vec<NodeWithPosition2> {
    match hitest(root, x, y) {
        Some(chain) => chain.collect(),
        None => Vec::new(),
    }
}

/// 节点是否被**祖父**的内容盒完全裁掉（`border` 为外扩容差，TS `clipAtGradParentNode`）。
///
/// 用于绘制期剪枝：滚动列表里已滚出视口的行即使不透明也完全不可见，跳过绘制能省
/// 大量绘制调用（对标 Kotlin `clipAtGradParentNode`）。
#[must_use]
pub fn clip_at_grad_parent_node(node: &Node2, border: f32) -> bool {
    let parent = match node.parent() {
        Some(p) => p,
        None => return false,
    };
    let grand = match parent.parent() {
        Some(g) => g,
        None => return false,
    };
    let nx = node.x() + parent.x() - border;
    let ny = node.y() + parent.y() - border;
    let nw = node.width() + border * 2.0;
    let nh = node.height() + border * 2.0;
    let cx = grand.padding_inline_start();
    let cy = grand.padding_block_start();
    let cw = grand.inner_size(Axis::X);
    let ch = grand.inner_size(Axis::Y);
    nx + nw <= cx || nx >= cx + cw || ny + nh <= cy || ny >= cy + ch
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canvas_render2::node2::{always, Flex, LayoutDir2, Node2Props};
    use crate::{Fc, Layout, NodeContext};
    use std::rc::Rc;

    /// 只给方向的 flex（测试里最常用的形态）。
    fn flex_dir(direction: Layout) -> LayoutDir2 {
        LayoutDir2::new(Flex {
            direction: always(direction),
            ..Flex::default()
        })
    }

    /// 方向 + 间距的 flex。
    fn flex_dir_gap(direction: Layout, gap: f32) -> LayoutDir2 {
        LayoutDir2::new(Flex {
            direction: always(direction),
            gap: always(gap),
            ..Flex::default()
        })
    }

    /// 给定外包宽高的叶子参数。
    fn leaf(w: f32, h: f32) -> Node2Props {
        Node2Props {
            width: Some(std::rc::Rc::new(move || w)),
            height: Some(std::rc::Rc::new(move || h)),
            ..Node2Props::default()
        }
    }

    /// 把参数当子节点挂到 `parent` 上（`mount` 要到引擎挂载才展开子节点，
    /// 单测无引擎，故直接走 `node2_parent` + `Fc::mount`）。
    fn attach_child(parent: &Node2, arg: Node2Props) {
        let mut cx = NodeContext::new();
        cx.node2_parent = Some(parent.clone());
        Fc::new(move |cx| {
            Node2::empty().build(arg).mount(cx);
        })
        .mount(&mut cx);
    }

    fn build_root(arg: Node2Props) -> Node2 {
        // 画布根在原点：未显式定位时补 x:0/y:0（TS 根节点必须定位，否则
        // `defaultPosition` 会因无父节点抛错；对齐 TS 画布根在 (0,0)）。
        // 已显式设了 x/y 的（如本文件的位置用例）保留原值。
        let mut a = arg;
        if a.x.is_none() {
            a.x = Some(Rc::new(|| 0.0));
        }
        if a.y.is_none() {
            a.y = Some(Rc::new(|| 0.0));
        }
        Node2::empty().build(a)
    }

    /// 没给 `layout` 的节点走绝对定位方向：位置一律 0、容器不给子节点尺寸。
    #[test]
    fn no_layout_uses_absolute_direction() {
        let root = build_root(leaf(100.0, 50.0));
        let lx = root.layout_x();
        assert!(
            !lx.allow_size_from_children(),
            "绝对布局不允许由子节点撑尺寸"
        );
        assert_eq!(
            lx.child_position(0),
            Ok(0.0),
            "TS absoluteLayoutObject.childPosition → 0"
        );
        assert!(
            lx.child_size(0).is_err(),
            "TS absoluteLayoutObject.childSize → 抛错"
        );
    }

    /// 纵向容器：主轴（y）顺排，尺寸由子节点累加。
    #[test]
    fn flex_column_places_children_along_main_axis() {
        let arg = Node2Props {
            layout: flex_dir(Layout::Column),
            ..Node2Props::default()
        };
        let root = build_root(arg);
        attach_child(&root, leaf(10.0, 20.0));
        attach_child(&root, leaf(30.0, 30.0));

        let ly = root.layout_y();
        assert!(
            ly.allow_size_from_children(),
            "Grow 且无 grow 子节点 → 允许由子节点撑尺寸"
        );
        assert_eq!(ly.size_from_children(), Ok(50.0), "20 + 30");
        assert_eq!(ly.child_position(0), Ok(0.0));
        assert_eq!(ly.child_position(1), Ok(20.0));
        assert_eq!(ly.child_size(1), Ok(30.0), "主轴尺寸 = 子节点自身外包高");
    }

    /// 横向容器 + gap：总长含 gap，`sizeFromChildren` 去掉末尾多余的那个 gap。
    #[test]
    fn flex_row_with_gap() {
        let arg = Node2Props {
            layout: flex_dir_gap(Layout::Row, 5.0),
            ..Node2Props::default()
        };
        let root = build_root(arg);
        attach_child(&root, leaf(10.0, 20.0));
        attach_child(&root, leaf(10.0, 20.0));

        let lx = root.layout_x();
        assert_eq!(lx.size_from_children(), Ok(25.0), "10 + 5 + 10");
        assert_eq!(lx.child_position(1), Ok(15.0));
    }

    /// 子节点索引 = 它在父节点 `layoutChildren()` 里的位置。
    #[test]
    fn layout_index_follows_layout_children_order() {
        let arg = Node2Props {
            layout: flex_dir(Layout::Column),
            ..Node2Props::default()
        };
        let root = build_root(arg);
        attach_child(&root, leaf(10.0, 20.0));
        attach_child(&root, leaf(10.0, 30.0));
        let kids = root.layout_children();
        assert_eq!(kids.len(), 2);
        assert_eq!(kids[0].layout_index(), 0);
        assert_eq!(kids[1].layout_index(), 1);
    }

    /// 叶子节点（自身是绝对布局）的默认尺寸 = 父布局分配给它的子尺寸。
    #[test]
    fn default_size_of_leaf_comes_from_parent() {
        let arg = Node2Props {
            layout: flex_dir(Layout::Column),
            ..Node2Props::default()
        };
        let root = build_root(arg);
        attach_child(&root, leaf(10.0, 20.0));
        attach_child(&root, leaf(30.0, 30.0));
        let kids = root.layout_children();

        let s0 = default_size(&kids[0], Axis::Y);
        assert!(!s0.from_inside, "来自父级分配的是外包尺寸");
        assert_eq!(s0.value, 20.0);
        let s1 = default_size(&kids[1], Axis::Y);
        assert_eq!(s1.value, 30.0);
    }

    /// 容器自身的默认尺寸：主轴取子节点累加，**且不会因为问自己尺寸而递归**
    /// （这条正是 `innerSize` 必须惰性求值的原因）。
    #[test]
    fn default_size_of_container_comes_from_children() {
        let arg = Node2Props {
            layout: flex_dir(Layout::Column),
            ..Node2Props::default()
        };
        let root = build_root(arg);
        attach_child(&root, leaf(10.0, 20.0));
        attach_child(&root, leaf(30.0, 30.0));

        let s = default_size(&root, Axis::Y);
        assert!(s.from_inside, "由内容推导的尺寸是内容区尺寸");
        assert_eq!(s.value, 50.0, "20 + 30");
        // 交叉轴（stack）取非对齐子节点的最大外包尺寸
        let sx = default_size(&root, Axis::X);
        assert_eq!(sx.value, 30.0, "max(10, 30)");
    }

    /// 默认位置 = 父布局分配的子位置 + 父的 padding start。
    #[test]
    fn default_position_adds_parent_padding_start() {
        let arg = Node2Props {
            layout: flex_dir(Layout::Column),
            padding_block_start: Some(std::rc::Rc::new(|| 7.0)),
            ..Node2Props::default()
        };
        let root = build_root(arg);
        attach_child(&root, leaf(10.0, 20.0));
        attach_child(&root, leaf(10.0, 30.0));
        let kids = root.layout_children();

        assert_eq!(default_position(&kids[0], Axis::Y), 7.0, "0 + padding 7");
        assert_eq!(default_position(&kids[1], Axis::Y), 27.0, "20 + padding 7");
    }
    // ---- 纯消息那一半：命中 / 绝对坐标 / 祖父裁剪 ----

    #[test]
    fn absolute_position_walks_parent_chain() {
        // 父 (10,20) 内嵌子 (30,40) → 子全局 (40,60)
        let mut parent_a = leaf(200.0, 200.0);
        parent_a.x = Some(Rc::new(|| 10.0));
        parent_a.y = Some(Rc::new(|| 20.0));
        let mut child_a = leaf(50.0, 50.0);
        child_a.x = Some(Rc::new(|| 30.0));
        child_a.y = Some(Rc::new(|| 40.0));
        let parent = build_root(parent_a);
        attach_child(&parent, child_a);
        let child = parent.children().into_iter().next().unwrap();

        assert_eq!(absolute_x(&child), 40.0);
        assert_eq!(absolute_y(&child), 60.0);
    }

    #[test]
    fn hitest_finds_deepest_and_chain_order() {
        let mut root_a = leaf(200.0, 200.0);
        root_a.x = Some(Rc::new(|| 0.0));
        root_a.y = Some(Rc::new(|| 0.0));
        let mut child_a = leaf(80.0, 80.0);
        child_a.x = Some(Rc::new(|| 10.0));
        child_a.y = Some(Rc::new(|| 10.0));
        let root = build_root(root_a);
        attach_child(&root, child_a);

        // 点 (30,30) 在 child(10..90) 内部 → 命中链 [root, child]
        let path = hit_path(&root, 30.0, 30.0);
        assert_eq!(path.len(), 2);
        assert_eq!(path[0].node.id(), root.id());
        assert_eq!(path[1].node.id(), root.children()[0].id());
        assert_eq!(path[1].x, 20.0); // 相对 child 的局部坐标
        assert_eq!(path[1].y, 20.0);

        // 点 (5,5) 不在 child(10..90) 内，只命中 root
        let path2 = hit_path(&root, 5.0, 5.0);
        assert_eq!(path2.len(), 1);
        assert_eq!(path2[0].node.id(), root.id());
    }

    #[test]
    fn clip_at_grad_parent_prunes_offscreen() {
        // grand(0,0,100,100) → parent(0,0) → child 在 (0,200) 滚出视口
        let mut grand_a = leaf(100.0, 100.0);
        grand_a.x = Some(Rc::new(|| 0.0));
        grand_a.y = Some(Rc::new(|| 0.0));
        let mut parent_a = leaf(100.0, 100.0);
        parent_a.x = Some(Rc::new(|| 0.0));
        parent_a.y = Some(Rc::new(|| 0.0));
        let mut child_a = leaf(20.0, 20.0);
        child_a.x = Some(Rc::new(|| 0.0));
        child_a.y = Some(Rc::new(|| 200.0));
        let grand = build_root(grand_a);
        attach_child(&grand, parent_a);
        let parent = grand.children()[0].clone();
        attach_child(&parent, child_a);
        let child = parent.children()[0].clone();

        assert!(clip_at_grad_parent_node(&child, 0.0));

        // child 移回视口内 (0,10) → 不被裁剪
        let mut grand_a2 = leaf(100.0, 100.0);
        grand_a2.x = Some(Rc::new(|| 0.0));
        grand_a2.y = Some(Rc::new(|| 0.0));
        let mut parent_a2 = leaf(100.0, 100.0);
        parent_a2.x = Some(Rc::new(|| 0.0));
        parent_a2.y = Some(Rc::new(|| 0.0));
        let mut child_a2 = leaf(20.0, 20.0);
        child_a2.x = Some(Rc::new(|| 0.0));
        child_a2.y = Some(Rc::new(|| 10.0));
        let grand2 = build_root(grand_a2);
        attach_child(&grand2, parent_a2);
        let parent2 = grand2.children()[0].clone();
        attach_child(&parent2, child_a2);
        let child2 = parent2.children()[0].clone();
        assert!(!clip_at_grad_parent_node(&child2, 0.0));
    }

    #[test]
    fn hide_child_is_invisible_to_tree_algorithms() {
        // 隐藏节点不在 `children()` 里，命中链也就走不进去（TS `hitest` 同样
        // 只读 `node.children()`）。
        let root = build_root(leaf(100.0, 100.0));
        let mut hidden = leaf(50.0, 50.0);
        hidden.hide = Rc::new(|| true);
        hidden.x = Some(Rc::new(|| 0.0));
        hidden.y = Some(Rc::new(|| 0.0));
        attach_child(&root, hidden);

        assert!(root.children().is_empty());
        // 命中只能落在 root 自己身上（子节点被 hide 挡在 `children()` 之外）
        let path = hit_path(&root, 10.0, 10.0);
        assert_eq!(path.len(), 1);
        assert_eq!(path[0].node.id(), root.id());
    }

    /// `alignFix` 决定交叉轴居中的基准：容器自身高，还是"最高子节点"。
    ///
    /// 对标 TS `wy-helper/src/layout/stack.ts` 与 Kotlin `StackLayout.kt`：
    /// ```ts
    /// this.size = memo(() => alignFix() ? inside.innerSize()
    ///                                   : max(children.map(outerSize)));
    /// // center: (size() - outerSize(child)) / 2
    /// ```
    /// 所以"给了交叉轴尺寸（`height`）却忘写 `alignFix`"不会报错，只会让
    /// 居中基准悄悄变成最高子节点 —— 行内元素整体偏上，肉眼很难归因。
    #[test]
    fn stack_align_fix_decides_centering_reference() {
        fn row(align_fix: bool) -> Node2 {
            let arg = Node2Props {
                width: Some(Rc::new(|| 100.0)),
                height: Some(Rc::new(|| 30.0)),
                layout: LayoutDir2::new(Flex {
                    direction: always(Layout::Row),
                    align_item: always(crate::AlignItem::Center),
                    align_fix: always(align_fix),
                    ..Flex::default()
                }),
                ..Node2Props::default()
            };
            let root = build_root(arg);
            attach_child(&root, leaf(18.0, 18.0));
            attach_child(&root, leaf(24.0, 24.0));
            root
        }

        // 有 alignFix：基准 = 容器高 30 → 18 高的子节点落在 (30-18)/2 = 6
        let fixed = row(true);
        assert_eq!(fixed.children()[0].y(), 6.0);

        // 无 alignFix：基准 = 最高子节点 24 → 落在 (24-18)/2 = 3
        let auto = row(false);
        assert_eq!(auto.children()[0].y(), 3.0);
    }

    /// 逐子节点对齐（TS `AlignChild` + `cross.align(n)`）覆盖容器 `alignItem`。
    ///
    /// 横向容器的交叉轴是 y，所以只认 `alignY`；只写 `alignX` 不生效（回落容器值）。
    /// 对标 TS `canvasCore/FlexLayout.ts`：
    /// ```ts
    /// align(n) {
    ///   const a = n.findExt(AlignChild);
    ///   if (a) return direction() == 'x' ? a.alignY() : a.alignX();
    /// }
    /// ```
    #[test]
    fn per_child_align_overrides_container_item() {
        use crate::canvas_render2::node2::{align, align_xy, Ext};

        fn row(children: Vec<(f32, f32, Vec<Ext>)>, align_item: crate::AlignItem) -> Node2 {
            let arg = Node2Props {
                width: Some(Rc::new(|| 100.0)),
                height: Some(Rc::new(|| 100.0)),
                layout: LayoutDir2::new(Flex {
                    direction: always(Layout::Row),
                    align_item: always(align_item),
                    // 交叉轴基准取容器自身高，排除"最高子节点"这一变量的干扰
                    align_fix: always(true),
                    ..Flex::default()
                }),
                ..Node2Props::default()
            };
            let root = build_root(arg);
            for (w, h, exts) in children {
                let mut a = leaf(w, h);
                a.exts = exts;
                attach_child(&root, a);
            }
            root
        }

        // 容器 Start：A 显式 End → 贴底；B 没有 ext → 贴顶
        let root = row(
            vec![
                (20.0, 20.0, vec![align(crate::AlignItem::End)]),
                (20.0, 20.0, vec![]),
            ],
            crate::AlignItem::Start,
        );
        assert_eq!(root.children()[0].y(), 80.0, "End: 100 - 20");
        assert_eq!(root.children()[1].y(), 0.0, "没有 ext 的子节点回落 Start");

        // 容器 Center：A 覆盖成 End，B 仍居中
        let root = row(
            vec![
                (20.0, 20.0, vec![align(crate::AlignItem::End)]),
                (20.0, 20.0, vec![]),
            ],
            crate::AlignItem::Center,
        );
        assert_eq!(root.children()[0].y(), 80.0, "逐子节点覆盖容器 Center");
        assert_eq!(root.children()[1].y(), 40.0, "Center: (100 - 20) / 2");

        // 只给 alignX 的 ext：横向容器交叉轴是 y → 不生效
        let root = row(
            vec![(
                20.0,
                20.0,
                vec![align_xy(Some(crate::AlignItem::End), None)],
            )],
            crate::AlignItem::Start,
        );
        assert_eq!(
            root.children()[0].y(),
            0.0,
            "横向容器读 alignY，alignX 不参与交叉轴"
        );
    }
}
