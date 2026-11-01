//! canvasRender2 的节点模型 [`Node2`]（逐字对标 TS `canvasRender2/Node.ts`）。
//!
//! # 只有一层节点
//!
//! 早期 v1 有个渲染中间表示 `Node`：把 flex 布局、绘制、命中、事件、焦点全部
//! 摊平成一组字段，`grow` / `align_items` / `justify` / `gap` 这类**布局配置**
//! 直接长在节点上，声明代码因此被迫记住一堆与"我要一个长什么样的节点"无关
//! 的字段。**那层已经删除**，仓库里只有 `Node2` 一种节点。
//!
//! TS 的 `Node2` 把职责分成两块：
//!
//! - **节点本体**（`Node2Props`）：位置、尺寸、内边距、绘制、事件、焦点、光标——
//!   而且几乎全是 `GetValue<T> = T | (() => T)`，布局期/命中期/悬停期**重新求值**，
//!   所以"只读时不可聚焦""拖动时改间距"这类状态变化立即生效、不必重建节点。
//! - **布局与伸展**：容器方向与对齐走 `layout: flex! { .. }`（一条 `LayoutDir2` 策略），子节点的伸展/对齐
//!   走 `exts: [grow(..), align(..), ignore_flex(..)]` —— 布局系统按需从
//!   `exts` 里 `find_ext` 取，节点本体零字段。
//!
//! # 求值语义（与 TS 一致）
//!
//! `width()` / `x()` / `padding_inline_start()` 等都是**方法**：调用时求值。
//! 显式给了读取式来源就用它；没给就回落到引擎在本帧绘制前写回的
//! [`NodeGeom`]（= TS 里由 `layout` 推导出来的那一维）。
//!
//! 也就是说，`draw` 回调里 `self.width()` 拿到的是**本帧最终生效值**，
//! 在 `width` 闭包里写 `node.width()` 则是递归（TS 同样栈溢出）——不做保护。

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use wy_render::Scene;

use crate::canvas_render2::focus::KeyEvent2;
use crate::canvas_render2::protocol::MouseEvent2;
use crate::{
    primitives::{CursorType, ImeEvent},
    rec, AlignItem, DirectionFixBetweenWhenOne, DirectionJustify, Fc, Layout, NodeGeom,
};

// ============================================================================
// 读取式值（TS `GetValue<T>`）
// ============================================================================

/// 读取式值：每次调用重新求值（TS `GetValue<T> = T | (() => T)` 的闭包那半）。
///
/// Rust 没有"值或函数"的联合类型，于是统一收敛成闭包：常量写 [`always`]，
/// 派生写 `|| signal.get()`。这样"静态"与"动态"就没有两套代码路径，
/// 也就没有"忘了用动态版"这种事。
pub type Get<T> = Rc<dyn Fn() -> T>;

/// 常量 → [`Get<T>`]（TS 里直接写字面量的那半）。
#[must_use]
pub fn always<T: Clone + 'static>(v: T) -> Get<T> {
    Rc::new(move || v.clone())
}

/// 闭包 → [`Get<T>`]（省掉 `Rc::new` 的包装）。
#[must_use]
pub fn get<T: 'static>(f: impl Fn() -> T + 'static) -> Get<T> {
    Rc::new(f)
}

/// 一维布局的轴（TS `PointKey`）：`X` = 主轴横向，`Y` = 主轴纵向。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Axis {
    /// 横向。
    X,
    /// 纵向。
    Y,
}

impl Axis {
    /// 取另一个轴。
    #[must_use]
    pub fn opposite(self) -> Self {
        match self {
            Self::X => Self::Y,
            Self::Y => Self::X,
        }
    }
}

/// 内边距的起止侧（TS `StartEnd`）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StartEnd {
    /// 起始侧（横向 = 左，纵向 = 上）。
    Start,
    /// 结束侧（横向 = 右，纵向 = 下）。
    End,
}

/// 声明尺寸（TS `LayoutSize`）：`value` + 它表示内容区还是外包。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LayoutSize {
    /// 尺寸值。
    pub value: f32,
    /// `true` = `value` 是**内容区**尺寸（外包需加两侧 padding）；
    /// `false` = `value` 就是外包尺寸。
    pub from_inside: bool,
}

impl LayoutSize {
    /// 外包尺寸。
    #[must_use]
    pub fn outer(value: f32) -> Self {
        Self {
            value,
            from_inside: false,
        }
    }

    /// 内容区尺寸（外包 = value + padding start + padding end）。
    #[must_use]
    pub fn inside(value: f32) -> Self {
        Self {
            value,
            from_inside: true,
        }
    }
}

/// 单方向尺寸约束（TS `LayoutSizeDirection`）。
///
/// `direction` 指定被约束的轴；`value` 的内外语义由 `from_inside` 决定。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LayoutSizeDirection {
    /// 被约束的轴。
    pub direction: Axis,
    /// 尺寸值。
    pub value: f32,
    /// `true` = 内容区尺寸；`false` = 外包尺寸。
    pub from_inside: bool,
}

/// 构造单方向尺寸约束（TS `layoutSizeDirection`）。
#[must_use]
pub fn layout_size_direction(
    direction: Axis,
    value: f32,
    from_inside: bool,
) -> LayoutSizeDirection {
    LayoutSizeDirection {
        direction,
        value,
        from_inside,
    }
}

// ============================================================================
// 布局扩展（TS `exts: readonly any[]` + `grow` / `align` / `ignoreFlex`）
// ============================================================================

/// 布局扩展对象（TS `exts` 数组里的一项）。
///
/// 布局系统不认识具体类型，只按需 [`find_ext`]；这样"伸展权重""逐子节点对齐"
/// 这些**布局关注点**不必长在节点本体的字段上。
pub type Ext = Rc<dyn Any>;

/// 在 `exts` 里找指定类型的扩展（TS `findExt(n, c)` 的 `instanceof` 判定）。
#[must_use]
pub fn find_ext<T: Any>(exts: &[Ext]) -> Option<&T> {
    exts.iter().find_map(|e| e.downcast_ref::<T>())
}

/// 主轴伸展权重（TS `GrowChild`，由 TS `grow({argGrow})` 构造）。
///
/// 分轴：横向容器看 `grow_x`，纵向容器看 `grow_y` —— 对应 TS 的
/// `growX()` / `growY()`。
pub struct GrowChild {
    /// 横向权重来源。
    pub grow_x: Get<f32>,
    /// 纵向权重来源。
    pub grow_y: Get<f32>,
}

impl GrowChild {
    /// 取指定轴上的伸展权重（TS `GrowChild.growX/growY`）。
    #[must_use]
    pub fn grow(&self, d: Axis) -> f32 {
        match d {
            Axis::X => (self.grow_x)(),
            Axis::Y => (self.grow_y)(),
        }
    }
}

/// TS `grow({ argGrow: v })`：两轴同权重。
#[must_use]
pub fn grow(v: f32) -> Ext {
    Rc::new(GrowChild {
        grow_x: always(v),
        grow_y: always(v),
    })
}

/// TS `grow({ growX, growY })`：分轴权重。
#[must_use]
pub fn grow_xy(x: f32, y: f32) -> Ext {
    Rc::new(GrowChild {
        grow_x: always(x),
        grow_y: always(y),
    })
}

/// TS `grow({ argGrow: (d) => .. })`：按轴动态求值（拖动分栏常用）。
#[must_use]
pub fn grow_dyn(f: impl Fn(Axis) -> f32 + 'static) -> Ext {
    let f = Rc::new(f);
    let fx = Rc::clone(&f);
    let fy = Rc::clone(&f);
    Rc::new(GrowChild {
        grow_x: Rc::new(move || fx(Axis::X)),
        grow_y: Rc::new(move || fy(Axis::Y)),
    })
}

/// 逐子节点对齐（TS `AlignChild`，由 TS `align({alignX, alignY})` 构造）。
///
/// 覆盖容器的 `alignItem`：横向容器看 `align_y`（交叉轴是 y），反之看 `align_x`。
/// 交叉轴布局（[`FlexCross`]）在算位置前先问这个扩展，命中即用它的结果替代容器值；
/// 同时该子节点会**退出**容器"交叉轴取最大子节点"的那次计算（TS
/// `if (convert.align(it)) return;`，见 `wy_layout::StackLayout::new_lazy`）。
///
/// [`FlexCross`]: crate::canvas_render2::tree::layout_direction::FlexCross
pub struct AlignChild {
    /// 横向对齐来源。
    pub align_x: Get<Option<AlignItem>>,
    /// 纵向对齐来源。
    pub align_y: Get<Option<AlignItem>>,
}

impl AlignChild {
    /// 取指定轴上的对齐（`None` = 回落容器 `alignItem`）。
    #[must_use]
    pub fn align(&self, d: Axis) -> Option<AlignItem> {
        match d {
            Axis::X => (self.align_x)(),
            Axis::Y => (self.align_y)(),
        }
    }
}

/// TS `align({ argAlign: item })`：两轴同对齐。
#[must_use]
pub fn align(item: AlignItem) -> Ext {
    Rc::new(AlignChild {
        align_x: always(Some(item)),
        align_y: always(Some(item)),
    })
}

/// TS `align({ alignX, alignY })`：分轴对齐。
#[must_use]
pub fn align_xy(x: Option<AlignItem>, y: Option<AlignItem>) -> Ext {
    Rc::new(AlignChild {
        align_x: always(x),
        align_y: always(y),
    })
}

/// 不参与 flex 排布（TS `IgnoreFlexChild`，由 `ignore_flex({ignore})` 构造）。
///
/// 语义比"绝对定位"窄：只是不占 flex 主轴空间，其它方面仍是普通子节点。
pub struct IgnoreFlexChild {
    /// 是否忽略来源。
    pub ignore: Get<bool>,
}

/// TS `ignoreFlex({ ignore: true })`。
#[must_use]
pub fn ignore_flex(v: bool) -> Ext {
    Rc::new(IgnoreFlexChild { ignore: always(v) })
}

/// TS `ignoreFlex({ ignore: () => .. })`：读取式。
#[must_use]
pub fn ignore_flex_dyn(f: impl Fn() -> bool + 'static) -> Ext {
    Rc::new(IgnoreFlexChild { ignore: Rc::new(f) })
}

// ============================================================================
// 布局对象（TS `flex({...})` = `Node2Arg.layout`）
// ============================================================================

/// 容器布局策略接口（Kotlin `interface LayoutDirection` / TS `LayoutDirection2`）。
///
/// **这是一条策略接口，不是"二选一的联合"**。对齐 Kotlin：
///
/// ```kotlin
/// interface LayoutDirection {
///     val layoutX: LayoutFun<LayoutNode>
///     val layoutY: LayoutFun<LayoutNode>
/// }
/// ```
///
/// 实现方：
/// - [`Flex`] —— `flex! { .. }` 的产物（主轴走 flex、交叉轴走 stack）；
/// - [`AbsoluteLayoutDir`] —— 默认的绝对定位（[`absolute`]），子节点位置一律 0、
///   容器不替子节点分配尺寸。
///
/// 声明处用的句柄是 [`LayoutDir2`]（一条被共享的策略），缺省就地是绝对定位——
/// 既不藏进 `Option`，也不必用枚举把"以后还会有别的布局"提前钉死。
pub trait LayoutDir: std::fmt::Debug {
    /// 横向布局工厂（TS `layoutX()` / Kotlin `layoutX`）。
    fn layout_x(&self) -> Rc<dyn crate::canvas_render2::tree::layout_direction::LayoutFun2>;

    /// 纵向布局工厂（TS `layoutY()` / Kotlin `layoutY`）。
    fn layout_y(&self) -> Rc<dyn crate::canvas_render2::tree::layout_direction::LayoutFun2>;

    /// 主轴方向（TS `direction()`）；非 flex 布局给中性值 [`Layout::Column`]。
    ///
    /// 它只被 "grow 的轴向换算" 与 "自然尺寸沿轴累加" 用到，绝对布局本身不参与
    /// 父级 flex 的排布，取值不影响正确性。
    fn dir(&self) -> Layout {
        Layout::Column
    }

    /// 子节点间距（TS `gap()`）；非 flex 布局无间距。
    fn gap(&self) -> f32 {
        0.0
    }

    /// flex 配置（非 flex 布局返回 `None`）。
    ///
    /// v1 `Node` IR 桥接要用它把 `align_item` / `align_fix` / `justify` 搬过去；
    /// 那些参数只在 flex 上有意义，绝对定位一律取 v1 的中性缺省。
    fn flex(&self) -> Option<&Flex> {
        None
    }
}

/// 布局策略句柄（TS 的 `layout?: LayoutDirection2` 传的就是这样一条句柄）。
///
/// 只是一层 `Rc<dyn LayoutDir>`：多个节点可以共用同一条策略，拷来拷去都是计数。
/// [`Default`] 就地是绝对定位——所以"不写 `layout:`"不是"没有布局"，而是
/// **一条具体的布局策略**。
#[derive(Clone)]
pub struct LayoutDir2(Rc<dyn LayoutDir>);

impl LayoutDir2 {
    /// 由任意 [`LayoutDir`] 实现造一条句柄。
    pub fn new(l: impl LayoutDir + 'static) -> Self {
        Self(Rc::new(l))
    }

    /// 绝对定位（TS `absoluteLayoutDirection`）。
    #[must_use]
    pub fn absolute() -> Self {
        Self(Rc::new(AbsoluteLayoutDir))
    }
}

impl Default for LayoutDir2 {
    fn default() -> Self {
        Self::absolute()
    }
}

impl std::ops::Deref for LayoutDir2 {
    type Target = dyn LayoutDir;

    fn deref(&self) -> &Self::Target {
        &*self.0
    }
}

impl std::fmt::Debug for LayoutDir2 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(&self.0, f)
    }
}

/// 绝对定位布局方向（TS `absoluteLayoutDirection` / Kotlin 匿名 `LayoutDirection`）。
///
/// 两个轴都用 `absoluteLayout()`：子节点落位 0、容器不分配尺寸。
#[derive(Clone, Copy, Debug, Default)]
pub struct AbsoluteLayoutDir;

/// 绝对定位布局（默认值，等价 TS `arg.layout ?? absoluteLayoutDirection`）。
#[must_use]
pub fn absolute() -> LayoutDir2 {
    LayoutDir2::absolute()
}

/// 弹性布局的配置项（TS `flex({...})` 的参数）。
///
/// 这是**容器**才有的属性：自己怎么排布子节点。子节点"想怎么被排"走 `exts`。
/// `direction` / `alignItem` / `alignFix` / `directionJustify` / `gap` 全部
/// 收在这里，正是 TS 把 flex 配置与节点本体分开的做法。
#[derive(Clone)]
pub struct Flex {
    /// 主轴方向（`Row` = x，`Column` = y），对应 TS `direction: PointKey`。
    pub direction: Get<Layout>,
    /// 交叉轴对齐（TS `alignItem`）。
    pub align_item: Get<AlignItem>,
    /// 交叉轴不取子节点最大尺寸，改由父分配（TS `alignFix`）。
    pub align_fix: Get<bool>,
    /// 主轴对齐（TS `directionJustify`）。
    pub direction_justify: Get<DirectionJustify>,
    /// 子节点间距（TS `gap`）。
    pub gap: Get<f32>,
    /// 反向排列（TS `reverse`）。
    pub reverse: Get<bool>,
    /// 仅一个子节点且 `Between` 对齐时，子节点的落位策略（TS `directionFixBetweenWhenOne`）。
    pub direction_fix_between_when_one: Get<DirectionFixBetweenWhenOne>,
}

impl Default for Flex {
    fn default() -> Self {
        Self {
            // TS `flex` 的 `direction ?? 'y'`：默认纵向。
            direction: always(Layout::Column),
            align_item: always(AlignItem::Center),
            align_fix: always(false),
            direction_justify: always(DirectionJustify::Grow),
            gap: always(0.0),
            reverse: always(false),
            direction_fix_between_when_one: always(DirectionFixBetweenWhenOne::Center),
        }
    }
}

impl std::fmt::Debug for Flex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Flex {:?} gap={}", (self.direction)(), (self.gap)())
    }
}

/// 弹性布局方向（TS `flex({...})`）：产出**一条** [`LayoutDir2`] 策略。
///
/// `#[rec]` 生成 `flex!` 宏，调用即"一次声明、不可再改"：
///
/// ```ignore
/// node2! { layout: flex! { direction: Layout::Row, gap: 8.0 }, .. }
/// ```
///
/// 返回值是 [`LayoutDir2`]——`layout:` 要的就是这条策略句柄（TS 的
/// `layout?: LayoutDirection2` 同理），不必再包 `Some(..)`。
///
/// 每个参数都在**字段上**写默认值（`#[props(default = ..)]`），省略即取默认；
/// 没有 `defaults(..)` 这种分组指令——默认值跟着字段走，读签名就能看全。
/// `#[getter]` 让每个字段写的是**表达式**（`gap: 8.0` / `gap: w.get() / 4.0`），
/// 布局期才求值，对应 TS 的 `GetValue<T>`。
#[rec]
pub fn flex(
    #[getter]
    #[props(default = Layout::Column)]
    direction: Layout,
    #[getter]
    #[props(default = AlignItem::Center)]
    align_item: AlignItem,
    #[getter]
    #[props(default = false)]
    align_fix: bool,
    #[getter]
    #[props(default = DirectionJustify::Grow)]
    direction_justify: DirectionJustify,
    #[getter]
    #[props(default = 0.0f32)]
    gap: f32,
    #[getter]
    #[props(default = false)]
    reverse: bool,
    #[getter]
    #[props(default = DirectionFixBetweenWhenOne::Center)]
    direction_fix_between_when_one: DirectionFixBetweenWhenOne,
) -> LayoutDir2 {
    LayoutDir2::new(Flex {
        direction,
        align_item,
        align_fix,
        direction_justify,
        gap,
        reverse,
        direction_fix_between_when_one,
    })
}

impl Flex {
    /// 主轴方向当前值。
    #[must_use]
    pub fn dir(&self) -> Layout {
        (self.direction)()
    }

    /// 主轴方向当前值对应的轴（TS `direction()` 的 `'x' | 'y'` → [`Axis`]）。
    #[must_use]
    pub fn dir_axis(&self) -> Axis {
        axis_of(self.dir())
    }
}

// ============================================================================
// Node2
// ============================================================================

/// 节点唯一标识（焦点信号、identity 比对用）。
type NodeId2 = u64;

thread_local! {
    static NEXT_ID: Cell<NodeId2> = const { Cell::new(1) };
}

fn next_id() -> NodeId2 {
    NEXT_ID.with(|c| {
        let v = c.get();
        c.set(v + 1);
        v
    })
}

/// 回调类型：绘制（拿到自己，便于读最终几何）。
pub type Draw2 = Rc<dyn Fn(&mut Scene, &Node2)>;
/// 回调类型：指针事件（协议级 [`MouseEvent2`]，不是宿主原始事件）。
pub type Mouse2 = Rc<dyn Fn(&mut MouseEvent2)>;
/// 回调类型：键盘（返回 `true` = 已消费）。
///
/// 收的是**协议级** [`KeyEvent2`]（TS `Node2.handleKey(e: KeyEvent2): boolean`）——
/// 节点只认协议事件，宿主 `KeyEvent` 到 [`KeyEvent2`] 的换算由引擎的
/// `KeyEvent2::from_mve` 负责。
pub type Key2 = Rc<dyn Fn(&KeyEvent2) -> bool>;

/// `Node2` 的可变部分（构建期写入，之后只读）。
struct Node2Data {
    width: Option<Get<f32>>,
    height: Option<Get<f32>>,
    size: Option<Rc<dyn Fn(Axis) -> LayoutSize>>,
    x: Option<Get<f32>>,
    y: Option<Get<f32>>,
    position: Option<Rc<dyn Fn(Axis) -> f32>>,
    padding_inline_start: Option<Get<f32>>,
    padding_inline_end: Option<Get<f32>>,
    padding_block_start: Option<Get<f32>>,
    padding_block_end: Option<Get<f32>>,
    /// 容器布局策略：缺省就地是绝对定位（[`AbsoluteLayoutDir`]），或 `flex! { .. }`。
    layout: LayoutDir2,
    hide: Get<bool>,
    not_in_layout: Get<bool>,
    selection_enabled: Get<bool>,
    /// 跳过绘制——**连子树一起**（[`Node2::render_node2`] 在递归前 `continue`）。
    /// 带子节点的容器别用它"不画自己"，不给 `draw` 即可。
    skip_draw: Get<bool>,
    focusable: Get<bool>,
    focus_order: Get<Option<i32>>,
    focus_trap: Get<bool>,
    accept_hit: Option<Rc<dyn Fn(f32, f32) -> bool>>,
    accept_clip: Option<Rc<dyn Fn(f32, f32) -> bool>>,
    cursor: Option<Get<CursorType>>,
    draw: Option<Draw2>,
    draw_at_parent: Option<Draw2>,
    handle_key: Option<Key2>,
    /// IME 组合事件（Rust 引擎经 `ime_fn` 上报；TS 编辑器经 `onComposing` 消费）。
    ime: Option<Rc<dyn Fn(&mut ImeEvent) -> bool>>,
    /// IME 候选窗定位（返回节点局部坐标的光标盒 `x,y,w,h`）。
    ime_cursor: Option<Rc<dyn Fn() -> (f32, f32, f32, f32)>>,
    mouse_click: Option<Mouse2>,
    mouse_click_capture: Option<Mouse2>,
    mouse_down: Option<Mouse2>,
    mouse_down_capture: Option<Mouse2>,
    mouse_up: Option<Mouse2>,
    mouse_up_capture: Option<Mouse2>,
    mouse_move: Option<Mouse2>,
    mouse_move_capture: Option<Mouse2>,
    mouse_wheel: Option<Mouse2>,
    mouse_wheel_capture: Option<Mouse2>,
    mouse_double_click: Option<Mouse2>,
    mouse_double_click_capture: Option<Mouse2>,
    mouse_context_menu: Option<Mouse2>,
    mouse_context_menu_capture: Option<Mouse2>,
    on_focus: Option<Rc<dyn Fn()>>,
    on_blur: Option<Rc<dyn Fn()>>,
    /// 悬停变化（`true` = 指针进入）。
    ///
    /// **Rust 侧扩展**：TS `Node2Arg` 没有 hover 回调（悬停视觉靠 `cursor` 与
    /// 全局指针状态）；Rust 引擎的 hover 链已经是现成的，直接暴露出来，
    /// 省得业务里为了一个高亮去注册全局指针监听。
    on_hover: Option<Rc<dyn Fn(bool)>>,
    exts: Vec<Ext>,
    /// 裁剪子树（Rust 引擎的 `clip`；TS 用 `drawAtParent` + `acceptClip` 表达
    /// 同一件事，这里保留成便捷字段，语义等价于"画的时候把子树裁到本盒"）。
    clip: bool,
    /// 把本节点的子树裁到**父节点**内盒（TS `ScrollContent.drawAtParent`）。
    ///
    /// 渲染时由父循环在 `translate` **之前**压栈（父坐标系），画完本支再出栈——
    /// 与 TS `save → drawAtParent → translate → draw → restore` 同序。
    clip_to_parent: bool,
    /// 诊断用可读名（TS `debugName(): string | null`）：只在报错 / 审计信息里
    /// 出现，不参与任何求值。`None` = TS 的 `null`。
    debug_name: Option<Get<String>>,
    /// 子节点构造体（展开后转成 `children`）。
    child_fcs: Vec<Fc>,
}

impl Default for Node2Data {
    fn default() -> Self {
        Self {
            width: None,
            height: None,
            size: None,
            x: None,
            y: None,
            position: None,
            padding_inline_start: None,
            padding_inline_end: None,
            padding_block_start: None,
            padding_block_end: None,
            layout: absolute(),
            hide: always(false),
            not_in_layout: always(false),
            // TS `node.selectionEnabled = node.selectionEnabled || alawaysTrue`
            selection_enabled: always(true),
            skip_draw: always(false),
            focusable: always(false),
            focus_order: always(None),
            focus_trap: always(false),
            accept_hit: None,
            accept_clip: None,
            cursor: None,
            draw: None,
            draw_at_parent: None,
            handle_key: None,
            ime: None,
            ime_cursor: None,
            mouse_click: None,
            mouse_click_capture: None,
            mouse_down: None,
            mouse_down_capture: None,
            mouse_up: None,
            mouse_up_capture: None,
            mouse_move: None,
            mouse_move_capture: None,
            mouse_wheel: None,
            mouse_wheel_capture: None,
            mouse_double_click: None,
            mouse_double_click_capture: None,
            mouse_context_menu: None,
            mouse_context_menu_capture: None,
            on_focus: None,
            on_blur: None,
            on_hover: None,
            exts: Vec::new(),
            clip: false,
            clip_to_parent: false,
            debug_name: None,
            child_fcs: Vec::new(),
        }
    }
}

struct Node2Inner {
    id: NodeId2,
    geom: Rc<Cell<NodeGeom>>,
    data: RefCell<Node2Data>,
    children: RefCell<Vec<Node2>>,
    parent: RefCell<Option<Weak<Node2Inner>>>,
    /// 文档序索引（TS `_index`）：由**父节点**的 `children()` 在过滤完隐藏节点后
    /// 回填（TS 是 `purifyList` → `flatten` 过滤 hide、`after: collectIndex` 回填），
    /// `index()` 只负责读——与 TS 的写入时机完全一致。
    index: Cell<usize>,
    /// 布局序索引（TS `_layoutIndex`）：由**父节点**的 `layoutChildren()` 回填，
    /// `layoutIndex()` 只负责读——与 TS 的写入时机完全一致。
    layout_index: Cell<usize>,
}

/// canvasRender2 的节点（TS `Node2`）。
///
/// `Clone` 是浅拷贝：所有副本共享同一个节点（`Rc`），可自由塞进
/// `draw` / 事件 / 布局闭包。
pub struct Node2(Rc<Node2Inner>);

impl Clone for Node2 {
    fn clone(&self) -> Self {
        Self(Rc::clone(&self.0))
    }
}

impl PartialEq for Node2 {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

impl std::fmt::Debug for Node2 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Node2")
            .field("id", &self.0.id)
            .field("debug_name", &self.debug_name())
            .field("width", &self.width())
            .field("height", &self.height())
            .finish()
    }
}

/// 节点的弱引用（TS `ctx.addDestroy` 的对偶）。
///
/// TS 的组件可以在构造期 `ctx.addDestroy(fn)`，节点销毁时把全局监听器摘干净。
/// Rust 侧组件的回调挂在**节点自己**的字段上（随节点一起消失），但
/// [`register_move`](crate::canvas_render2::renderer::register_move) 这类
/// **全局**注册表不在节点上：闭包一旦捕获 `Node2`（强引用）节点就永不销毁，
/// 于是注册表只增不减。
///
/// 所以长生命周期的全局回调一律捕获 [`Node2Weak`]：`upgrade()` 成功才干活，
/// 失败说明节点已经没了，回调就地注销自己。语义上等价于 TS 的 `addDestroy`，
/// 只是把"注销时机"从析构钩子换成"下次被调用时的存活检查"。
#[derive(Clone)]
pub struct Node2Weak(Weak<Node2Inner>);

impl Node2 {
    /// 取弱引用（不延长寿命）。
    #[must_use]
    pub fn downgrade(&self) -> Node2Weak {
        Node2Weak(Rc::downgrade(&self.0))
    }
}

impl Node2Weak {
    /// 节点仍存活时取回强引用。
    #[must_use]
    pub fn upgrade(&self) -> Option<Node2> {
        self.0.upgrade().map(Node2)
    }

    /// 节点是否已被销毁。
    #[must_use]
    pub fn is_dead(&self) -> bool {
        self.0.strong_count() == 0
    }
}

impl Default for Node2 {
    fn default() -> Self {
        Self::empty()
    }
}

// ============================================================================
// IME 扩展：编辑器节点的输入法通道
// ============================================================================

/// IME 组合事件扩展（TS 里编辑器节点自己持有 `onComposing`）。
///
/// 引擎的 IME 上报是**节点级**的（`ime_fn` / `ime_cursor_fn`），但"输入法怎么
/// 组合"不属于「节点长什么样」，也没有任何别的组件需要它。所以走 [`Ext`]：
/// 声明层零字段（`node2` / `node2!` 都不用为它开洞），[`Node2::build`] 时从
/// `exts` 里取出来落进引擎槽——与 `grow` / `align` / 编辑器句柄同一条路。
pub struct ImeExt {
    /// 组合事件（`Preedit` / `Commit`）。
    pub on_ime: Rc<dyn Fn(&mut ImeEvent) -> bool>,
    /// 候选窗定位（节点局部坐标的光标盒 `x, y, w, h`）。
    pub cursor: Rc<dyn Fn() -> (f32, f32, f32, f32)>,
}

// ============================================================================
// 子模块：`impl Node2` 按职责拆开（读取式 API / 构建 / 挂载）
// ============================================================================

mod build;
mod mount;
mod read;

// 树的组装入口与几何小工具原本就定义在 `impl Node2` 之外，路径保持不变。
// 节点参数（`node2` 组件的 props）在这里再导一次：`Node2::build` 与所有
// `extends = node2` 的组件都要它，放在节点模块入口最顺手。
pub use crate::canvas_render2::element::Node2Props;
pub use build::axis_of;
pub use mount::{
    absolute_in_inner, build_root_fc, build_root_node2, current_parent, expand_node2, size,
};

#[cfg(test)]
mod tests;
