//! 布局方向（TS `canvasCore/FlexLayout.ts` 的 `createFlex`
//! + `tree/assemble.ts` 的 `absoluteLayoutDirection`）。
//!
//! # TS 原文（结构）
//!
//! ```ts
//! const main = { outerSize(n) { return size(n, direction()) },
//!                index(n) { return n.layoutIndex() },
//!                grow(n) { .. }, ignore(n) { .. },
//!                createLayout(o) { return new FlexLayout(arg, o, this) } };
//! const cross = { align(n) { .. }, outerSize(n) { return size(n, toOpposite(direction())) },
//!                ignore(n) { .. },
//!                createLayout(o) { return new StackLayout(arg, o, this) } };
//! return { layout(d) { return direction() == d ? main : cross } };
//! ```
//!
//! 即：**主轴走 flex、交叉轴走 stack**，`layout(d)` 按容器方向二选一。
//! 没有 `layout` 的节点默认绝对定位（[`AbsoluteLayoutFun`]）。

use std::rc::Rc;

use wy_layout::Layout as WyLayout;
use wy_layout::{
    AbsoluteLayout, AlignItem, DirectionFixBetweenWhenOne, DirectionJustify, FlexChildConvert,
    FlexLayout, FlexObject, StackChildConvert, StackLayout, StackObject,
};

use crate::canvas_render2::node2::{
    AbsoluteLayoutDir, AlignChild, Axis, Flex, GrowChild, IgnoreFlexChild, LayoutDir, Node2,
};

/// 惰性容器对象：TS `LayoutInsideObject<Node2>`。
///
/// TS 的两个成员都是 **getter**（`children()` / `innerSize()`），布局只在真正需要
/// 的分支里才去问——这一点是 `defaultSize` 能成立的前提（容器"由子节点撑尺寸"时
/// 绝不能回头问自己的尺寸，否则自递归），所以 Rust 侧**不能**退化成"先算好两个数
/// 再传进来"。可用空间保留成闭包，由 `wl::FlexLayout::new_lazy` /
/// `wl::StackLayout::new_lazy` 按分支求值。
pub struct LayoutInside2<'a> {
    /// 参与布局的子节点。
    pub children: &'a [Node2],
    /// 该轴上的可用空间（惰性求值）。
    pub inner_size: &'a dyn Fn() -> f32,
}

/// 布局工厂（TS `LayoutFun<Node2>`）：给定容器对象，产出该轴上的一维布局。
pub trait LayoutFun2 {
    /// 构造该轴上的布局对象（TS `createLayout(o)`）。
    fn create_layout(&self, inside: &LayoutInside2<'_>) -> Rc<dyn WyLayout>;

    /// 该轴上"尺寸由子节点撑起"是否成立（TS `layout.allowSizeFromChildren()`）。
    ///
    /// # 为什么不能问布局对象
    ///
    /// 构造 `FlexLayout` 时，只要存在 grow 子节点，就要立刻读容器的可用尺寸
    /// （`inside.innerSize()`），而那是 `size(node, d)` = `node.width()` ——又回到
    /// `defaultSize`。于是"先造布局、再问 allowSizeFromChildren"会自递归：
    ///
    /// ```text
    /// defaultSize(n, d)
    ///   → layoutValue(n, d)     构造 → innerSize(n, d) → n.width() → defaultSize(n, d)
    /// ```
    ///
    /// TS 之所以不会炸，是因为它的 `layoutX()` 只**建对象**、不算结果
    /// （结果在各方法里惰性算）；Rust 这边的 `FlexLayout` 是"构造即求值"，
    /// 所以这条判定必须绕开构造——它本身只取决于布局配置与子节点的 grow
    /// 扩展，与可用空间无关，完全可以在工厂层直接回答。
    fn allows_size_from_children(&self, node: &Node2) -> bool;
}

/// 默认方向：**绝对定位**（TS `absoluteLayoutDirection`）。
///
/// 没给 `layout` 的节点用它：子节点位置一律 0、容器不给子节点尺寸
/// （`child_size` 报错），于是 `defaultSize` 会走到 `sizeFromParent` 或节点自己的
/// 声明尺寸——正是"绝对定位容器"的语义。
pub struct AbsoluteLayoutFun;

impl LayoutFun2 for AbsoluteLayoutFun {
    fn create_layout(&self, _inside: &LayoutInside2<'_>) -> Rc<dyn WyLayout> {
        Rc::new(AbsoluteLayout)
    }

    fn allows_size_from_children(&self, _node: &Node2) -> bool {
        // TS `absoluteLayoutDirection` 的布局不给子节点尺寸，自己也不由子节点撑开。
        false
    }
}

/// 主轴工厂（TS `createFlex` 里的 `main`）：既是 `FlexChildConvert` 也是 `LayoutFun`。
pub struct FlexMain {
    flex: Flex,
}

impl FlexMain {
    /// 用容器布局配置构造主轴工厂。
    #[must_use]
    pub fn new(flex: Flex) -> Self {
        Self { flex }
    }
}

impl FlexChildConvert<Node2> for FlexMain {
    fn index(&self, child: &Node2) -> usize {
        child.layout_index()
    }

    fn grow(&self, child: &Node2) -> f32 {
        // TS：`direction() == 'x' ? grow.growX() : grow.growY()`
        child
            .find_ext_rc::<GrowChild>()
            .map_or(0.0, |g| g.grow(self.flex.dir_axis()))
    }

    fn outer_size(&self, child: &Node2) -> f32 {
        // TS：`size(n, direction())` —— 主轴上的**外包**尺寸
        super::geometry::size(child, self.flex.dir_axis())
    }

    fn ignore(&self, child: &Node2) -> bool {
        child
            .find_ext_rc::<IgnoreFlexChild>()
            .is_some_and(|it| (it.ignore)())
    }
}

impl FlexObject<Node2> for FlexMain {
    fn gap(&self) -> f32 {
        (self.flex.gap)()
    }

    fn direction_justify(&self) -> DirectionJustify {
        (self.flex.direction_justify)()
    }

    fn reverse(&self) -> bool {
        (self.flex.reverse)()
    }

    fn direction_fix_between_when_one(&self) -> DirectionFixBetweenWhenOne {
        (self.flex.direction_fix_between_when_one)()
    }
}

impl LayoutFun2 for FlexMain {
    fn create_layout(&self, inside: &LayoutInside2<'_>) -> Rc<dyn WyLayout> {
        Rc::new(FlexLayout::new_lazy(self, inside.children, || {
            (inside.inner_size)()
        }))
    }

    fn allows_size_from_children(&self, node: &Node2) -> bool {
        // TS `FlexLayout.allowSizeFromChildren()`：`justify === 'grow' && !hasGrowChildren`。
        // 两个条件都只看配置与子节点扩展，不需要可用空间——正是 `defaultSize`
        // 判定分支的前提（见 trait 文档）。
        (self.flex.direction_justify)() == DirectionJustify::Grow && !self.has_grow_children(node)
    }
}

impl FlexMain {
    /// 是否存在 grow 子节点（TS `hasGrowChildren`：忽略 `ignore` 的之后是否还有 `grow > 0`）。
    fn has_grow_children(&self, node: &Node2) -> bool {
        let axis = self.flex.dir_axis();
        node.layout_children().iter().any(|c| {
            !self.ignore(c)
                && c.find_ext_rc::<GrowChild>()
                    .is_some_and(|g| g.grow(axis) > 0.0)
        })
    }
}

/// 交叉轴工厂（TS `createFlex` 里的 `cross`）。
///
/// 逐子节点对齐走子节点的 [`AlignChild`] 扩展（TS `cross.align(n)` → `findExt(AlignChild)`），
/// 轴向按 TS 的写法取**交叉轴那一条**：横向容器读 `alignY`，纵向容器读 `alignX`。
/// 没有该扩展的子节点回落到容器级 `alignItem` / `alignFix`。
pub struct FlexCross {
    flex: Flex,
}

impl FlexCross {
    /// 用容器布局配置构造交叉轴工厂。
    #[must_use]
    pub fn new(flex: Flex) -> Self {
        Self { flex }
    }
}

impl StackChildConvert<Node2> for FlexCross {
    fn align(&self, child: &Node2) -> Option<&dyn wy_layout::Align> {
        let ac = child.find_ext_rc::<AlignChild>()?;
        ac.align(self.flex.dir_axis().opposite())
            .map(|item| item.as_align())
    }

    fn outer_size(&self, child: &Node2) -> f32 {
        // TS：`size(n, toOpposite(direction()))`
        super::geometry::size(child, self.flex.dir_axis().opposite())
    }

    fn ignore(&self, child: &Node2) -> bool {
        child
            .find_ext_rc::<IgnoreFlexChild>()
            .is_some_and(|it| (it.ignore)())
    }
}

impl StackObject<Node2> for FlexCross {
    fn align_item(&self) -> AlignItem {
        (self.flex.align_item)()
    }

    fn align_fix(&self) -> bool {
        // TS `StackLayout.this.alignFix = arg.alignFix ?? false`——**只认 `alignFix`**。
        //
        // 别把 `alignItem == Stretch` 也算进来（那样 `allowSizeFromChildren()` 会跟着变
        // `false`，看起来"自动拉伸了"，实际是替调用方掩盖了一个配置错误）：TS 里
        // `alignItem: 'stretch'` 的子节点要想拿到父级分配的交叉轴尺寸，前提正是
        // `alignFix: true` 让父级交叉轴尺寸取自 `innerSize()`；否则 `size()`
        // 会退化成"子节点最大外包尺寸"，`childSize` 又叫回 `size()` → 父子互问，
        // TS 在 `sizeFromParent` 的提前判环处直接报 `LayoutCycleError`。
        (self.flex.align_fix)()
    }
}

impl LayoutFun2 for FlexCross {
    fn create_layout(&self, inside: &LayoutInside2<'_>) -> Rc<dyn WyLayout> {
        Rc::new(StackLayout::new_lazy(self, inside.children, || {
            (inside.inner_size)()
        }))
    }

    fn allows_size_from_children(&self, _node: &Node2) -> bool {
        // TS `StackLayout.allowSizeFromChildren()` = `!alignFix()`：alignFix 时交叉轴
        // 尺寸取自可用空间（= 父给的），否则取自子节点里最大的那个。
        !(self.flex.align_fix)()
    }
}

/// 取节点在轴 `d` 上的布局工厂（TS `node.layout(d)`，`arg.layout ?? absoluteLayoutDirection`）。
///
/// 一条语句就是 TS 那句三元：`layout` 本身就是策略句柄，按轴问它要工厂即可。
#[must_use]
pub fn layout_fun_of(node: &Node2, d: Axis) -> Rc<dyn LayoutFun2> {
    let layout = node.layout();
    match d {
        Axis::X => layout.layout_x(),
        Axis::Y => layout.layout_y(),
    }
}

// ============================================================================
// 布局策略的两个实现（Kotlin `LayoutDirection` 的 `Flex` / `absoluteLayoutDirection`）
// ============================================================================

impl LayoutDir for AbsoluteLayoutDir {
    fn layout_x(&self) -> Rc<dyn LayoutFun2> {
        Rc::new(AbsoluteLayoutFun)
    }

    fn layout_y(&self) -> Rc<dyn LayoutFun2> {
        Rc::new(AbsoluteLayoutFun)
    }
}

impl LayoutDir for Flex {
    fn layout_x(&self) -> Rc<dyn LayoutFun2> {
        self.fun_of_axis(Axis::X)
    }

    fn layout_y(&self) -> Rc<dyn LayoutFun2> {
        self.fun_of_axis(Axis::Y)
    }

    // `dir` / `gap` / `flex` 走 trait 默认以外的那三支：flex 有主轴、有间距、
    // 也就有 flex 配置可供 v1 IR 桥接读取。
    fn dir(&self) -> crate::primitives::Layout {
        Flex::dir(self)
    }

    fn gap(&self) -> f32 {
        (self.gap)()
    }

    fn flex(&self) -> Option<&Flex> {
        Some(self)
    }
}

impl Flex {
    /// TS `createFlex` 的 `direction() == d ? main : cross`。
    fn fun_of_axis(&self, d: Axis) -> Rc<dyn LayoutFun2> {
        if self.dir_axis() == d {
            Rc::new(FlexMain::new(self.clone()))
        } else {
            Rc::new(FlexCross::new(self.clone()))
        }
    }
}
