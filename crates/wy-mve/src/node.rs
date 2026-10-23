//! Node：UI 元素的具象类型。
//!
//! 对应 Kotlin 的 `Node` 类。用闭包代替继承。
//! 声明式构造：像 JS 对象或 Kotlin 匿名类一样一次性构建，不可修改。
//!
//! # 构建一次模型
//!
//! 树的**结构**在 [`crate::context::render_root`] 时一次性构建：
//! - `arg_children_fn` 只执行一次，产出 `ChildSlot` 列表（静态节点 + 动态区域）
//! - 子节点缓存 `children` 共享（克隆共享同一缓存）
//! - 信号变化只重算受影响区域（`renderForEach` memo），不再整树重建

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use crate::context::NodeContext;
use wy_layout::{
    AlignItem, DirectionJustify, FlexChildConvert, FlexLayout, FlexObject, LayoutInsideObject,
};

type ArgChildrenFn = Rc<dyn Fn(&mut NodeContext)>;
type DrawFn = Rc<dyn Fn(&mut dyn std::any::Any)>;
type HitTestFn = Rc<dyn Fn(f32, f32) -> bool>;
type ClickFn = Rc<dyn Fn(&mut PointerEvent)>;
type HoverFn = Rc<dyn Fn(bool)>;
type KeyFn = Rc<dyn Fn(&mut KeyEvent) -> bool>;
/// IME 输入法事件处理（组合态文本上报）。
type ImeFn = Rc<dyn Fn(&mut ImeEvent) -> bool>;
/// IME 候选窗定位：光标盒 `(x, y, w, h)`（节点局部坐标）。
type ImeCursorFn = Rc<dyn Fn() -> (f32, f32, f32, f32)>;
/// 焦点获得/失去回调（引擎在焦点链变化时调用，供节点驱动聚焦视觉）。
type FocusFn = Rc<dyn Fn()>;
/// 测量节点自然尺寸（宽高像素）。文本等内容撑开型节点用它提供 intrinsic 尺寸，
/// 供容器布局消费（复刻 Kotlin `RichTextNode.argWidth/argHeight`）。
type MeasureFn = Rc<dyn Fn() -> (f32, f32)>;

/// 动态数值属性（复刻 Kotlin `open val get() = ...` 的 getter 语义）。
///
/// 在布局/绘制期（信号 track 上下文内）求值，读信号自动追踪依赖。
pub type DynNum = Rc<dyn Fn() -> f32>;
/// 动态布尔属性（对应 Kotlin `open val hide: Boolean get() = ...`）。
pub type DynBool = Rc<dyn Fn() -> bool>;

/// 上下文提供值的一条记录：`(类型 id, 值)`。
///
/// `ctx.provide(..)` 沿节点树向下传值，接收方用 `ctx.consume::<T>()` 取回。
type ProvidedEntry = (std::any::TypeId, Rc<dyn std::any::Any>);
/// 上下文提供值的快照（物化期沿树向下 seed）。
///
/// 声明为 `pub` 是为了让外部 crate 的 `Node { .., ..Node::default() }`
/// 结构体更新语法继续可用；业务代码不该读写它。
pub type ProvidedSnapshot = Rc<Vec<ProvidedEntry>>;

/// 节点是否参与事件命中（有 handler 才算可命中）。
pub fn has_handler(node: &Node) -> bool {
    node.on_click_fn.is_some()
        || node.on_down_fn.is_some()
        || node.on_up_fn.is_some()
        || node.on_hover_fn.is_some()
        || node.on_hover_move_fn.is_some()
        || node.on_wheel_fn.is_some()
}

/// Node：UI 元素的具象类型。
///
/// 声明式构造，一次性构建完成，之后不可修改。
pub struct Node {
    /// 绘制逻辑。参数：`(scene, x, y, width, height)`。
    pub draw_fn: DrawFn,
    /// 子节点构建逻辑（**构建期执行一次**，产出 [`ChildSlot`] 列表）。
    pub arg_children_fn: ArgChildrenFn,
    /// 命中测试逻辑（本地坐标系，4 个角落）。
    pub hit_test_fn: HitTestFn,
    /// 点击事件。
    pub on_click_fn: Option<ClickFn>,
    /// 按下事件。
    pub on_down_fn: Option<ClickFn>,
    /// 释放事件。
    pub on_up_fn: Option<ClickFn>,
    /// 指针移动事件（**按下捕获期间**派发：拖选、拖拽）。
    ///
    /// 不按键的悬停移动走 [`Node::on_hover_move_fn`]，两者语义不同，
    /// 避免文本框这类"移动即扩选"的节点在悬停时被误触发。
    pub on_move_fn: Option<ClickFn>,
    /// 悬停移动事件（**未按下**时指针在节点上移动，携带局部坐标）。
    ///
    /// 引擎沿悬停链派发（叶→根）。用于十字线、悬停读数、tooltip——
    /// 这类"指针在哪"的信息如果不给节点，组件就只能自己读全局光标，
    /// 又退回手算坐标。
    pub on_hover_move_fn: Option<ClickFn>,
    /// 滚轮事件（`PointerEvent::wheel` 为带符号滚动量，正数向下/放大）。
    pub on_wheel_fn: Option<ClickFn>,
    /// 悬停变化（`true` = 指针进入，`false` = 离开）。
    ///
    /// 引擎在 `CursorMoved` 时比对命中链差异后派发，对应浏览器
    /// `pointerenter` / `pointerleave`（不冒泡，只发给链上真正声明了的节点）。
    pub on_hover_fn: Option<HoverFn>,
    /// 按键事件。
    pub key_fn: Option<KeyFn>,
    /// IME 输入法组合事件（组合文本 / 光标偏移；返回 true 表示已消费）。
    pub ime_fn: Option<ImeFn>,
    /// IME 候选窗定位：返回光标盒 `(x, y, w, h)`（节点局部坐标）。
    /// 引擎换算为窗口绝对坐标后喂给 `Window::set_ime_cursor_area`，
    /// 对齐 Kotlin `EditableTextNode.inputOverlay` 的候选窗跟随。
    pub ime_cursor_fn: Option<ImeCursorFn>,
    /// 获得焦点回调（焦点链叶子变为本节点时由引擎调用）。
    pub on_focus_fn: Option<FocusFn>,
    /// 失去焦点回调（焦点链叶子从本节点转走时由引擎调用）。
    pub on_blur_fn: Option<FocusFn>,
    /// 是否可聚焦。
    pub focusable: bool,
    /// 是否隐藏（不参与布局和命中测试）。
    pub hidden: bool,
    /// 跳过自身绘制（但子节点仍绘制）。
    pub skip_draw: bool,
    /// 是否裁剪子树（overflow hidden，滚动容器用）。
    ///
    /// 绘制期把自身 + 子节点限制在布局盒内（[`Scene::push_clip`]），
    /// 命中测试同样以布局盒为界——溢出内容既看不见也点不到。
    pub clip: bool,
    /// 绝对定位（Kotlin `notInLayout`）：不参与父容器 flex 排布与自然尺寸，
    /// 由 `x`/`y` 直接定位；绘制/命中仍按文档序（后声明在上）。
    /// 浮层、滚动条这类覆盖元素用它脱离布局流。
    pub not_in_layout: bool,
    /// 焦点顺序（显式 Tab 顺序，None = 按文档顺序）。
    pub focus_order: Option<i32>,
    /// 是否为焦点陷阱（模态窗口内焦点不外泄）。
    pub focus_trap: bool,
    /// 是否启用选择。
    pub selection_enabled: bool,
    /// 一维容器角色：渲染期对扁平化后的子节点做排布。
    pub layout: Option<Layout>,
    /// 布局位置（静态值；动态来源见 `x_fn`/`y_fn`，对应 Kotlin `argPosition`）。
    pub x: f32,
    pub y: f32,
    /// 布局尺寸（静态值；动态来源见 `width_fn`/`height_fn`，对应 Kotlin `argSize`）。
    pub width: f32,
    pub height: f32,
    /// 间距（复刻 Kotlin `FlexParam.gap`：容器属性而非布局配置，布局期读取）。
    pub gap: f32,
    /// 内边距（统一，后续可扩展为四方向）。
    pub padding: f32,
    /// 内边距-横向覆写（对齐 Kotlin 四向 `argPadding(direction, startEnd)`）。
    ///
    /// `None` = 用 `padding` 简写值；`Some(v)` = 左右都用 `v`。
    /// 生效规则：`padding` 四向同值是底，`pad_x`/`pad_y` 覆写对应方向。
    pub pad_x: Option<f32>,
    /// 内边距-纵向覆写。`None` = 用 `padding`；`Some(v)` = 上下都用 `v`。
    pub pad_y: Option<f32>,
    /// Flex grow（子节点在主轴上的伸展权重）。
    pub grow: f32,
    /// 交叉轴对齐（子节点自身对齐）。
    pub align_self: Option<AlignItem>,
    /// 容器主轴对齐（容器属性）。
    pub justify: DirectionJustify,
    /// 容器交叉轴对齐（容器属性）。
    pub align_items: AlignItem,
    /// 交叉轴尺寸由外部固定（复刻 Kotlin `StackObject.alignFix`，默认 false）。
    ///
    /// `false`：容器交叉轴自然尺寸由"非对齐子节点的最大尺寸"撑起
    /// （内容驱动，[`natural_size_of`] 的既有语义）。
    /// `true`：容器交叉轴**没有自身自然尺寸**——由父容器分配值决定
    /// （`layout_boxes` 的 `cross_avail`）。子节点有自然尺寸时两者行为
    /// 分叉，语义与 `AlignItem::Stretch`（"把我拉大到父尺寸"）不同。
    pub align_fix: bool,
    /// 动态位置 X（覆盖 `x`，对应 Kotlin `override fun argPosition(Direction.x)`）。
    pub x_fn: Option<DynNum>,
    /// 动态位置 Y（覆盖 `y`）。
    pub y_fn: Option<DynNum>,
    /// 动态宽度（覆盖 `width`，对应 Kotlin `override val argWidth`）。
    pub width_fn: Option<DynNum>,
    /// 动态高度（覆盖 `height`，对应 Kotlin `override val argHeight`）。
    pub height_fn: Option<DynNum>,
    /// 动态间距（覆盖 `gap`，对应 Kotlin `override val gap get() = ...`）。
    pub gap_fn: Option<DynNum>,
    /// 动态伸展权重（覆盖 `grow`）。
    ///
    /// 可拖动分栏需要它：拖动时改的是"这块占多少"，而不是某个像素坐标；
    /// 没有动态 grow 就只能退回给子节点算绝对高度。
    pub grow_fn: Option<DynNum>,
    /// 动态隐藏（覆盖 `hidden`，对应 Kotlin `override val hide get() = ...`）。
    pub hide_fn: Option<DynBool>,
    /// 整体尺寸来源（覆盖推导链，对应 Kotlin `override fun argSize`）。
    pub size_fn: Option<MeasureFn>,
    /// 自然尺寸测量：文本等内容型节点的 intrinsic 尺寸来源；
    /// `node_size` 优先取它，其次才用 `width`/`height`。
    pub measure_fn: Option<MeasureFn>,
    /// 从父上下文继承的 `provide` 快照（见 [`stamp_provided`]）。
    ///
    /// 只在物化期使用：本节点的子节点构建时用它 seed 上下文，
    /// 于是 `ctx.provide(..)` 的值能沿树下传给任意深度的 `ctx.consume`。
    ///
    /// 声明为 `pub` 是为了让外部 crate 的 `Node { .., ..Node::default() }`
    /// 结构体更新语法继续可用；业务代码不该读写它。
    #[doc(hidden)]
    pub provided: Option<ProvidedSnapshot>,
    /// 子节点缓存：构建期由 `arg_children_fn` 产出，之后复用。
    pub children: Rc<RefCell<Option<Vec<ChildSlot>>>>,
    /// 节点实例标识：克隆共享，用于依赖比对（复刻 Kotlin 引用相等语义）。
    pub identity: Rc<()>,
    /// 语义标签（可选）：给节点起个名字，便于测试与调试按名字定位。
    ///
    /// UI 里同类控件往往有很多个（多个输入框、多个按钮），按几何特征
    /// 反推"哪个是哪个"既脆弱又难读；给关键控件打个 `tag`，测试就能
    /// 精确断言"点到的是哪一个"。
    pub tag: Option<&'static str>,
    /// 声明为隐藏节点的记录（供调试）。
    pub _dummy: Cell<()>,
    /// 最近一次绘制分配的布局盒（引擎在绘制期写入；**局部尺寸**，不含父偏移）。
    ///
    /// 组件在**事件期/绘制期**需要知道"父容器实际分了我多大"（scroll 的滚动
    /// 上限、滑块几何，grow 填充类布局都靠它）时读它——布局盒每帧随 flex
    /// 重算，构建期根本不知道。绘制前读取返回 `ZERO`。
    ///
    /// **必须是 `Rc`**：`Node` 的克隆被放进树里（引擎遍历的是克隆），
    /// 组件自身也持有克隆。若用普通 `Cell`，克隆之间各写各的——
    /// 引擎写树里的那个、组件读自己手里的那个，**永远对不上**，
    /// `last_box` 机制等于失效（scroll 几何恒为 0、浮层翻转无从判断）。
    pub last_box: Rc<Cell<LayoutBox>>,
}

impl Clone for Node {
    fn clone(&self) -> Self {
        Self {
            arg_children_fn: Rc::clone(&self.arg_children_fn),
            draw_fn: Rc::clone(&self.draw_fn),
            hit_test_fn: Rc::clone(&self.hit_test_fn),
            on_click_fn: self.on_click_fn.as_ref().map(Rc::clone),
            on_down_fn: self.on_down_fn.as_ref().map(Rc::clone),
            on_up_fn: self.on_up_fn.as_ref().map(Rc::clone),
            on_move_fn: self.on_move_fn.as_ref().map(Rc::clone),
            on_hover_move_fn: self.on_hover_move_fn.as_ref().map(Rc::clone),
            on_wheel_fn: self.on_wheel_fn.as_ref().map(Rc::clone),
            on_hover_fn: self.on_hover_fn.as_ref().map(Rc::clone),
            key_fn: self.key_fn.as_ref().map(Rc::clone),
            ime_fn: self.ime_fn.as_ref().map(Rc::clone),
            ime_cursor_fn: self.ime_cursor_fn.as_ref().map(Rc::clone),
            on_focus_fn: self.on_focus_fn.as_ref().map(Rc::clone),
            on_blur_fn: self.on_blur_fn.as_ref().map(Rc::clone),
            focusable: self.focusable,
            hidden: self.hidden,
            skip_draw: self.skip_draw,
            clip: self.clip,
            not_in_layout: self.not_in_layout,
            focus_order: self.focus_order,
            focus_trap: self.focus_trap,
            selection_enabled: self.selection_enabled,
            layout: self.layout,
            x: self.x,
            y: self.y,
            width: self.width,
            height: self.height,
            gap: self.gap,
            padding: self.padding,
            pad_x: self.pad_x,
            pad_y: self.pad_y,
            grow: self.grow,
            align_self: self.align_self,
            justify: self.justify,
            align_items: self.align_items,
            align_fix: self.align_fix,
            x_fn: self.x_fn.as_ref().map(Rc::clone),
            y_fn: self.y_fn.as_ref().map(Rc::clone),
            width_fn: self.width_fn.as_ref().map(Rc::clone),
            height_fn: self.height_fn.as_ref().map(Rc::clone),
            gap_fn: self.gap_fn.as_ref().map(Rc::clone),
            grow_fn: self.grow_fn.as_ref().map(Rc::clone),
            hide_fn: self.hide_fn.as_ref().map(Rc::clone),
            size_fn: self.size_fn.as_ref().map(Rc::clone),
            measure_fn: self.measure_fn.as_ref().map(Rc::clone),
            provided: self.provided.as_ref().map(Rc::clone),
            tag: self.tag,
            children: Rc::clone(&self.children),
            identity: Rc::clone(&self.identity),
            _dummy: Cell::new(()),
            last_box: Rc::clone(&self.last_box),
        }
    }
}

impl std::fmt::Debug for Node {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Node")
            .field("x", &self.x)
            .field("y", &self.y)
            .field("width", &self.width)
            .field("height", &self.height)
            .field("layout", &self.layout)
            .field("hidden", &self.hidden)
            .finish()
    }
}

impl PartialEq for Node {
    fn eq(&self, other: &Self) -> bool {
        // 复刻 Kotlin 引用相等：同一 Node 实例（克隆共享 identity）。
        // 复用相同 holder 的节点在重算后仍视为相等，memo 短路跳过重建。
        Rc::ptr_eq(&self.identity, &other.identity)
    }
}

impl Default for Node {
    fn default() -> Self {
        let identity = Rc::new(());
        Self {
            arg_children_fn: Rc::new(|_| {}),
            draw_fn: Rc::new(|_| {}),
            hit_test_fn: Rc::new(|_, _| true),
            on_click_fn: None,
            on_down_fn: None,
            on_up_fn: None,
            on_move_fn: None,
            on_hover_move_fn: None,
            on_wheel_fn: None,
            on_hover_fn: None,
            key_fn: None,
            ime_fn: None,
            ime_cursor_fn: None,
            on_focus_fn: None,
            on_blur_fn: None,
            focusable: false,
            hidden: false,
            skip_draw: false,
            clip: false,
            not_in_layout: false,
            focus_order: None,
            focus_trap: false,
            selection_enabled: true,
            layout: None,
            x: 0.0,
            y: 0.0,
            width: 0.0,
            height: 0.0,
            // gap 默认 0：对齐 Kotlin `FlexParam.gap` 与 wy-layout
            // `FlexObject::gap()` 的默认值（两层默认值曾不一致：Node 层 8.0
            // 实际生效，导致"忘了写 gap"的容器与上游行为不同）。
            // 需要间距的简写组件（row/column）在各自定义里显式设 8.0。
            gap: 0.0,
            padding: 0.0,
            pad_x: None,
            pad_y: None,
            grow: 0.0,
            align_self: None,
            justify: DirectionJustify::Grow,
            align_items: AlignItem::Center,
            align_fix: false,
            x_fn: None,
            y_fn: None,
            width_fn: None,
            height_fn: None,
            gap_fn: None,
            grow_fn: None,
            hide_fn: None,
            size_fn: None,
            measure_fn: None,
            provided: None,
            tag: None,
            children: Rc::new(RefCell::new(None)),
            identity,
            _dummy: Cell::new(()),
            last_box: Rc::new(Cell::new(LayoutBox::ZERO)),
        }
    }
}

// --- 执行方法（供内部调用） ---

impl Node {
    pub fn run_arg_children(&self, cx: &mut NodeContext) {
        (self.arg_children_fn)(cx);
    }

    pub fn run_draw(&self, scene: &mut dyn std::any::Any) {
        (self.draw_fn)(scene);
    }

    pub fn run_hit_test(&self, x: f32, y: f32) -> bool {
        (self.hit_test_fn)(x, y)
    }

    pub fn run_on_click(&self, event: &mut PointerEvent) {
        if let Some(f) = &self.on_click_fn {
            f(event);
        }
    }

    pub fn run_on_down(&self, event: &mut PointerEvent) {
        if let Some(f) = &self.on_down_fn {
            f(event);
        }
    }

    pub fn run_on_up(&self, event: &mut PointerEvent) {
        if let Some(f) = &self.on_up_fn {
            f(event);
        }
    }

    pub fn run_on_move(&self, event: &mut PointerEvent) {
        if let Some(f) = &self.on_move_fn {
            f(event);
        }
    }

    /// 触发滚轮回调（`event.wheel` 为带符号滚动量）。
    pub fn run_wheel(&self, event: &mut PointerEvent) {
        if let Some(f) = &self.on_wheel_fn {
            f(event);
        }
    }

    /// 触发悬停移动回调（未按下时的指针移动）。
    pub fn run_hover_move(&self, event: &mut PointerEvent) {
        if let Some(f) = &self.on_hover_move_fn {
            f(event);
        }
    }

    /// 触发悬停变化回调（`entered` = 指针进入）。
    pub fn run_hover(&self, entered: bool) {
        if let Some(f) = &self.on_hover_fn {
            f(entered);
        }
    }

    pub fn run_key(&self, event: &mut KeyEvent) -> bool {
        if let Some(f) = &self.key_fn {
            f(event)
        } else {
            false
        }
    }

    /// 向上传播 IME 组合事件；返回 `true` 表示已消费。
    pub fn run_ime(&self, event: &mut ImeEvent) -> bool {
        if let Some(f) = &self.ime_fn {
            f(event)
        } else {
            false
        }
    }

    /// 查询光标盒（节点局部坐标），供 IME 候选窗定位。
    pub fn run_ime_cursor_rect(&self) -> Option<(f32, f32, f32, f32)> {
        self.ime_cursor_fn.as_ref().map(|f| f())
    }

    /// 触发获得焦点回调（若存在）。
    pub fn run_focus(&self) {
        if let Some(f) = &self.on_focus_fn {
            f();
        }
    }

    /// 触发失去焦点回调（若存在）。
    pub fn run_blur(&self) {
        if let Some(f) = &self.on_blur_fn {
            f();
        }
    }

    pub fn is_focusable(&self) -> bool {
        self.focusable
    }

    /// 是否隐藏（动态 `hide_fn` 优先，回落静态 `hidden`；复刻 Kotlin `hide` getter）。
    pub fn is_hidden(&self) -> bool {
        if let Some(f) = &self.hide_fn {
            f()
        } else {
            self.hidden
        }
    }

    /// 布局间距（动态 `gap_fn` 优先，回落静态 `gap`；复刻 Kotlin `FlexParam.gap`）。
    pub fn gap(&self) -> f32 {
        if let Some(f) = &self.gap_fn {
            f()
        } else {
            self.gap
        }
    }

    /// 横向内边距生效值（`pad_x` 覆写 `padding`；复刻 Kotlin 四向 padding）。
    pub fn pad_x_eff(&self) -> f32 {
        self.pad_x.unwrap_or(self.padding).max(0.0)
    }

    /// 纵向内边距生效值（`pad_y` 覆写 `padding`）。
    pub fn pad_y_eff(&self) -> f32 {
        self.pad_y.unwrap_or(self.padding).max(0.0)
    }

    /// 伸展权重（动态 `grow_fn` 优先，回落静态 `grow`）。
    pub fn effective_grow(&self) -> f32 {
        if let Some(f) = &self.grow_fn {
            f()
        } else {
            self.grow
        }
    }

    /// 布局位置 X（动态 `x_fn` 优先，回落静态 `x`；复刻 Kotlin `argPosition(Direction.x)`）。
    pub fn pos_x(&self) -> f32 {
        if let Some(f) = &self.x_fn {
            f()
        } else {
            self.x
        }
    }

    /// 布局位置 Y（动态 `y_fn` 优先，回落静态 `y`）。
    pub fn pos_y(&self) -> f32 {
        if let Some(f) = &self.y_fn {
            f()
        } else {
            self.y
        }
    }

    /// 动态宽度（`width_fn` 覆盖 `width`；对应 Kotlin `argWidth`）。
    pub fn effective_width(&self) -> f32 {
        if let Some(f) = &self.width_fn {
            f()
        } else {
            self.width
        }
    }

    /// 动态高度（`height_fn` 覆盖 `height`；对应 Kotlin `argHeight`）。
    pub fn effective_height(&self) -> f32 {
        if let Some(f) = &self.height_fn {
            f()
        } else {
            self.height
        }
    }

    /// 最近一次绘制分配的布局盒（局部尺寸；绘制前为 `ZERO`）。
    ///
    /// 引擎在布局盒分配之后、绘制子树之前写入；事件回调（滚轮/点击）
    /// 与 `draw`/`hide_fn` 里读到的都是上一帧的实际分配值。
    pub fn last_box(&self) -> LayoutBox {
        self.last_box.get()
    }
}

/// 一维容器排布：渲染期对扁平化后的子节点计算偏移。
///
/// 对应 Kotlin `FlexParam`/`Direction` 的一维布局模型。
/// 间距由节点属性提供（`Node::gap`），布局算法本身无状态——
/// 复刻 Kotlin「gap 是容器属性、布局期向容器索取」的形态。
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum Layout {
    /// 主轴 = X，子节点依次右排。
    Row,
    /// 主轴 = Y，子节点依次下排。
    Column,
}

/// 子节点类型：静态节点或动态区域。
///
/// 对应 Kotlin `ValueOrGetList<Node>`。
/// `Node` 是句柄型结构（字段以 `Rc` 为主），不 `Box`（见 [`crate::fc::Slot`]）。
#[allow(clippy::large_enum_variant)]
pub enum ChildSlot {
    /// 静态节点（构建期生成，之后不变）。
    Node(Node),
    /// 动态区域：每次读取返回该区域当前的节点列表。
    /// 内部通常是 `renderForEach` 的 memo，只在该区域信号变化时重算。
    GetNodes(Rc<dyn Fn() -> Vec<ChildSlot>>),
}

impl Clone for ChildSlot {
    fn clone(&self) -> Self {
        match self {
            ChildSlot::Node(n) => ChildSlot::Node(n.clone()),
            ChildSlot::GetNodes(f) => ChildSlot::GetNodes(Rc::clone(f)),
        }
    }
}

/// 指针事件。
pub struct PointerEvent {
    pub x: f32,
    pub y: f32,
    pub root_x: f32,
    pub root_y: f32,
    /// 按下时 Shift 是否按住（shift+点击扩选，引擎在分发前填充）。
    pub shift: bool,
    /// 滚轮增量（正 = 向下/缩小，负 = 向上/放大）；非滚轮事件为 0。
    pub wheel: f32,
    pub stopped: bool,
}

impl PointerEvent {
    pub fn new(x: f32, y: f32) -> Self {
        Self {
            x,
            y,
            root_x: x,
            root_y: y,
            shift: false,
            wheel: 0.0,
            stopped: false,
        }
    }

    pub fn stop_propagation(&mut self) {
        self.stopped = true;
    }
}

// --- 构建与展开（复刻 Kotlin `purifyList` / `Node.children` / Flex 一维布局） ---

/// 递归扁平化：静态节点追加，动态区域实时求值后递归展开。
///
/// 对应 Kotlin `com.wy.mve.purifyList`。
pub fn flatten(slots: &[ChildSlot], out: &mut Vec<Node>) {
    for slot in slots {
        match slot {
            ChildSlot::Node(n) => out.push(n.clone()),
            ChildSlot::GetNodes(f) => flatten(&f(), out),
        }
    }
}

/// 把 `provided` 快照盖到一批 slot 的静态节点上。
///
/// `Node` 之间的父子关系只在 slot 列表里体现，没有反向指针；而子节点是
/// **延迟**物化的（`materialize` 在绘制/命中时才跑），那时父上下文早已消失。
/// 所以在父上下文还活着的时候把快照盖到子节点上，子节点物化时就能用它
/// seed 自己的上下文——`provide` 因此能沿树向下传。
/// 单个 slot 版：供 [`NodeContext::push_slot`] 用（节点进入上下文的那一刻盖章）。
pub(crate) fn stamp_slot(slot: ChildSlot, provided: &crate::context::Provided) -> ChildSlot {
    if provided.is_empty() {
        return slot;
    }
    match slot {
        ChildSlot::Node(mut n) => {
            n.provided = Some(std::rc::Rc::new(provided.clone()));
            ChildSlot::Node(n)
        }
        other => other,
    }
}

/// 一次性构建节点子结构：执行 `arg_children_fn` 并缓存结果。
///
/// 复刻 Kotlin `Node.children` 的惰性 memo：只执行一次，之后每次复用。
/// 对静态 `Value` 子树递归构建；动态区域不在此展开（渲染期实时求值）。
///
/// 上下文继承链：本节点带入的 [`Node::provided`] → seed 本次构建的 `cx` →
/// 构建结束后把 `cx.provided` 盖到产出的子节点上，于是 `provide` 逐级下传。
pub fn materialize(node: &Node) {
    let cache = node.children.borrow();
    if cache.is_some() {
        return;
    }
    drop(cache);

    let mut cx = NodeContext::new();
    if let Some(inherited) = node.provided.as_ref() {
        cx.provided = (**inherited).clone();
    }
    node.run_arg_children(&mut cx);
    // 不再二次盖章：每个节点在 **push 进 cx 的那一刻** 就由 `push_slot` 盖上了
    // 当时的 provide 快照（那才是最准确的——它包含此前所有 provide，
    // 也只包含此前）。在这里重盖会覆盖掉子组件自己 provide 的那一层。
    *node.children.borrow_mut() = Some(cx.into_slots());

    let cache = node.children.borrow();
    if let Some(slots) = cache.as_ref() {
        for slot in slots.iter() {
            if let ChildSlot::Node(n) = slot {
                materialize(n);
            }
        }
    }
}

/// 当前节点的扁平化子节点列表（动态区域实时求值）。
pub fn children_nodes(node: &Node) -> Vec<Node> {
    materialize(node);
    let cache = node.children.borrow();
    let slots = cache.as_ref();
    let mut out = Vec::new();
    if let Some(slots) = slots {
        flatten(slots, &mut out);
    }
    out
}

/// 节点内容的尺寸：布局容器按实时子节点计算，普通节点用自身宽高。
///
/// 隐藏子节点（`is_hidden`）**不占布局空间**（对齐 Kotlin `hide` 的子节点
/// 不参与 flex 排布）；这样 `hide_fn` 可用于动态条件展示并触发重排。
///
/// 对应 Kotlin `FlexParam` 在布局期的尺寸派生。优先级（贴近 Kotlin
/// `RectNode.argSize` 的推导链）：
/// 1. `size_fn`（显式覆盖，对应 Kotlin `override fun argSize`）
/// 2. 布局容器：子节点累加撑起（对应 `sizeFromChildren`）
/// 3. `measure_fn`（内容测量，如文本）
/// 4. `width_fn`/`height_fn` 动态值
/// 5. 静态 `width`/`height`
pub fn node_size(node: &Node) -> (f32, f32) {
    // 显式声明的宽高优先于"子节点撑起"（Kotlin `argWidth`/`argHeight` 覆盖 `sizeFromChildren`）。
    // 没有这条，`vbox { width: 286.0 }` 这种"固定宽列"会被子节点撑成 0 宽。
    let declared = (node.effective_width(), node.effective_height());
    // 子节点撑起的自然尺寸（容器语义；叶子为 0）
    let natural = match node.layout {
        Some(layout) => {
            let (nw, nh) = natural_size_of(layout, node);
            // 容器没有子节点时退化为叶子语义：仍以 `measure` / 声明尺寸兜底。
            // 否则 `node {}` 这类「自绘叶子」一旦默认带 `layout`，就会因无子节点塌成 0 尺寸，
            // 既画不出也点不中。有子节点时才用容器自然尺寸撑开。
            if nw <= 0.0 && nh <= 0.0 {
                (0.0, 0.0)
            } else {
                (nw, nh)
            }
        }
        None => (0.0, 0.0),
    };
    // `measure_fn`（内容测量，如文本）——叶子节点的自然尺寸来源。
    let measured = node.measure_fn.as_ref().map(|m| m());

    if let Some(f) = &node.size_fn {
        let (sw, sh) = f();
        // **`size` 逐维覆盖**：某一维给 0 表示"这一维不设固定值"，
        // 回退到该维的自然/声明尺寸。
        //
        // 早先 `size` 是整体覆盖，于是 `size: (0.0, 0.0)`（本意是"宽度交给
        // flex 均分"）把**高度也清零**——卡片塌成 0 高、内容溢出、间隔错乱。
        // 逐维回退后，`size: (0.0, 0.0)` 表示"两维都不设固定值，都由
        // flex/内容决定"，正是等分卡片想要的语义。
        let w = if sw > 0.0 {
            sw
        } else if natural.0 > 0.0 {
            natural.0
        } else if let Some((mw, _)) = measured {
            mw
        } else {
            declared.0
        };
        let h = if sh > 0.0 {
            sh
        } else if natural.1 > 0.0 {
            natural.1
        } else if let Some((_, mh)) = measured {
            mh
        } else {
            declared.1
        };
        return (w, h);
    }

    if node.layout.is_some() {
        let (natural_w, natural_h) = natural;
        if natural_w <= 0.0 && natural_h <= 0.0 {
            if let Some((w, h)) = measured {
                return (w.max(declared.0), h.max(declared.1));
            }
            return declared;
        }
        return (
            if declared.0 > 0.0 {
                declared.0
            } else {
                natural_w
            },
            if declared.1 > 0.0 {
                declared.1
            } else {
                natural_h
            },
        );
    }
    if let Some((w, h)) = measured {
        (w.max(declared.0), h.max(declared.1))
    } else {
        declared
    }
}

/// 布局容器由子节点撑起的自然尺寸（不含显式宽高覆盖）。
///
/// `align_fix`（复刻 Kotlin `StackObject.alignFix`）为 true 时，交叉轴
/// **没有自身自然尺寸**（返回 0）：容器交叉轴由父容器分配值决定，
/// 不取"非对齐子节点的最大尺寸"。主轴（内容顺排方向）不受影响。
fn natural_size_of(layout: Layout, node: &Node) -> (f32, f32) {
    let gap = node.gap();
    let (pad_x, pad_y) = (node.pad_x_eff(), node.pad_y_eff());
    let children = children_nodes(node);
    let mut main = 0.0f32;
    let mut cross = 0.0f32;
    let mut count = 0;
    for c in &children {
        // 隐藏与绝对定位（浮层/滚动条）都不占布局空间
        if c.is_hidden() || c.not_in_layout {
            continue;
        }
        let (cw, ch) = node_size(c);
        match layout {
            Layout::Row => {
                main += cw + gap;
                if !node.align_fix {
                    cross = cross.max(ch);
                }
            }
            Layout::Column => {
                main += ch + gap;
                if !node.align_fix {
                    cross = cross.max(cw);
                }
            }
        }
        count += 1;
    }
    let main = if count == 0 { 0.0 } else { main - gap };
    // 容器自身内边距必须算进自然尺寸（外尺寸 = 内容 + 上下/左右 padding）：
    // 否则「声明式自绘 + 可嵌套」的 `node {}`（如带 `padding` 的 `card`）在去掉
    // 固定高度后，自然尺寸不含 padding，背景比内容小、子节点溢出背景，且同行卡片
    // 因内容高度不同而出现高度不齐、视觉间隔不均。这里补全 padding，让嵌套容器
    // 的尺寸语义与 `layout_boxes`（父分配时先扣自身 padding 再给子节点）一致。
    match layout {
        Layout::Row => (main + pad_x * 2.0, cross + pad_y * 2.0),
        Layout::Column => (cross + pad_x * 2.0, main + pad_y * 2.0),
    }
}

/// 节点在父容器内的布局盒（相对父节点原点）。
///
/// 这是 flex 排布的**结果**：主轴位置由 [`layout_boxes`] 按 `grow`/`gap`/`justify`
/// 分配，交叉轴由 `align_self`/`align_items` 决定尺寸与偏移。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LayoutBox {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Default for LayoutBox {
    fn default() -> Self {
        Self::ZERO
    }
}

impl LayoutBox {
    /// 全零盒（不参与布局的子节点）。
    pub const ZERO: Self = Self {
        x: 0.0,
        y: 0.0,
        width: 0.0,
        height: 0.0,
    };

    pub fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }
}

thread_local! {
    /// 当前正在绘制的节点所分到的布局盒。
    ///
    /// 组件在 `draw` 里需要知道"我多大"才能排版自己的内容（画网格、算行高、
    /// 裁切）。没有它，组件只能回头读窗口尺寸自己手算坐标——那就退回绘制式写法了。
    static CURRENT_BOX: Cell<LayoutBox> = const { Cell::new(LayoutBox::ZERO) };
}

/// 当前绘制节点分到的布局盒（宽高即父容器分给它的尺寸）。
///
/// 只在 `draw` 期间有效；在布局期调用返回 `ZERO`。
pub fn current_box() -> LayoutBox {
    CURRENT_BOX.with(|c| c.get())
}

/// 在"当前盒"作用域内执行 `f`，返回后恢复外层盒（嵌套绘制用）。
pub fn with_box<R>(b: LayoutBox, f: impl FnOnce() -> R) -> R {
    let prev = CURRENT_BOX.with(|c| c.replace(b));
    let out = f();
    CURRENT_BOX.with(|c| c.set(prev));
    out
}

/// flex 配置：把 `Node` 列表适配成 `wy-layout` 的 [`FlexChildConvert`]。
struct FlexCfg<'a> {
    children: &'a [Node],
    main: &'a [f32],
    gap: f32,
    justify: DirectionJustify,
}

impl FlexChildConvert<usize> for FlexCfg<'_> {
    fn index(&self, child: &usize) -> usize {
        *child
    }
    fn grow(&self, child: &usize) -> f32 {
        self.children[*child].effective_grow()
    }
    fn outer_size(&self, child: &usize) -> f32 {
        self.main[*child]
    }
    fn ignore(&self, child: &usize) -> bool {
        let c = &self.children[*child];
        c.is_hidden() || c.not_in_layout
    }
}

impl FlexObject<usize> for FlexCfg<'_> {
    fn gap(&self) -> f32 {
        self.gap
    }
    fn direction_justify(&self) -> DirectionJustify {
        self.justify
    }
}

/// 按容器角色 + 可用尺寸计算每个子节点的布局盒。
///
/// - 主轴：`wy_layout::FlexLayout` 分配（`grow` 吃剩余空间，`gap` 由容器给，
///   `justify` 决定无 grow 时的对齐）。
/// - 交叉轴：`align_self` 优先，其次容器的 `align_items`；`Stretch` 撑满交叉轴。
/// - `padding` 先从可用尺寸里扣掉，再加回到偏移上。
///
/// 可用尺寸为 0（未知）时退回"自然尺寸顺排"，与 [`layout_offsets`] 一致。
pub fn layout_boxes(node: &Node, avail: (f32, f32), children: &[Node]) -> Vec<LayoutBox> {
    let Some(layout) = node.layout else {
        return vec![LayoutBox::ZERO; children.len()];
    };
    if children.is_empty() {
        return Vec::new();
    }
    let (pad_x, pad_y) = (node.pad_x_eff(), node.pad_y_eff());
    let gap = node.gap();
    let inner_w = (avail.0 - pad_x * 2.0).max(0.0);
    let inner_h = (avail.1 - pad_y * 2.0).max(0.0);
    let (main_avail, cross_avail) = match layout {
        Layout::Row => (inner_w, inner_h),
        Layout::Column => (inner_h, inner_w),
    };

    // 自然尺寸（hidden / not_in_layout 视作 0，不占 flex 空间）
    let natural: Vec<(f32, f32)> = children
        .iter()
        .map(|c| {
            if c.is_hidden() || c.not_in_layout {
                (0.0, 0.0)
            } else {
                node_size(c)
            }
        })
        .collect();
    let main: Vec<f32> = natural
        .iter()
        .map(|&(w, h)| match layout {
            Layout::Row => w,
            Layout::Column => h,
        })
        .collect();
    let cross: Vec<f32> = natural
        .iter()
        .map(|&(w, h)| match layout {
            Layout::Row => h,
            Layout::Column => w,
        })
        .collect();

    // 主轴：走真正的 flex（grow/justify）分配
    let idx: Vec<usize> = (0..children.len()).collect();
    let cfg = FlexCfg {
        children,
        main: &main,
        gap,
        justify: node.justify,
    };
    let flex = if main_avail > 0.0 {
        Some(FlexLayout::new(
            &cfg,
            &LayoutInsideObject::new(&idx, main_avail),
        ))
    } else {
        None
    };

    let mut out = Vec::with_capacity(children.len());
    let mut cursor = 0.0f32;
    for (i, child) in children.iter().enumerate() {
        if child.is_hidden() {
            out.push(LayoutBox::ZERO);
            continue;
        }
        // 绝对定位子节点（浮层/滚动条）：不参与 flex，布局盒归零、
        // 位置统一交给绘制/命中的 `pos_x`/`pos_y` 叠加（与 `vbox_at` 同机制，
        // 避免偏移被 layout_boxes 和 draw_node 各加一遍）。
        //
        // 未声明任何尺寸（宽高均为 0）的绝对定位子节点**撑满父容器内区**：
        // dialog/dropdown 这类"铺满窗口的遮罩层"因此天然跟随窗口尺寸——
        // 声明了任一尺寸的（如滚动条滑块）仍用自身自然尺寸。
        if child.not_in_layout {
            let (dw, dh) = (child.effective_width(), child.effective_height());
            let (w, h) = if dw <= 0.0 && dh <= 0.0 {
                (inner_w, inner_h)
            } else {
                node_size(child)
            };
            out.push(LayoutBox::new(0.0, 0.0, w, h));
            continue;
        }
        // 主轴位置/尺寸：flex 结果优先，退化时按自然尺寸顺排
        let (main_pos, main_len) = match (&flex, main_avail > 0.0) {
            (Some(f), true) => {
                let r = f.result();
                let p = r.positions.get(i).copied().flatten();
                let l = r.child_lengths.get(i).copied().flatten();
                match (p, l) {
                    (Some(p), Some(l)) => (p, l),
                    // 该子节点被 flex 忽略（理论上不会，hidden 已提前处理）
                    _ => {
                        let o = (cursor, main[i]);
                        cursor += main[i] + gap;
                        o
                    }
                }
            }
            _ => {
                let o = (cursor, main[i]);
                cursor += main[i] + gap;
                o
            }
        };

        // 交叉轴：对齐策略。显式声明了交叉轴位置（`x`/`y` 或对应动态属性）的节点
        // 由自己定位，不参与自动对齐——对应 Kotlin 里 `argPosition` 覆盖 `align` 的行为。
        let explicit_cross = match layout {
            Layout::Row => child.y_fn.is_some() || child.y != 0.0,
            Layout::Column => child.x_fn.is_some() || child.x != 0.0,
        };
        let align = child.align_self.unwrap_or(node.align_items);
        let (cross_len, cross_pos) = if explicit_cross {
            (cross[i], 0.0)
        } else {
            match align {
                AlignItem::Stretch => {
                    let len = if cross_avail > 0.0 {
                        cross_avail
                    } else {
                        cross[i]
                    };
                    (len, 0.0)
                }
                AlignItem::Start => (cross[i], 0.0),
                AlignItem::Center => (cross[i], ((cross_avail - cross[i]) / 2.0).max(0.0)),
                AlignItem::End => (cross[i], (cross_avail - cross[i]).max(0.0)),
            }
        };

        out.push(match layout {
            Layout::Row => LayoutBox::new(pad_x + main_pos, pad_y + cross_pos, main_len, cross_len),
            Layout::Column => {
                LayoutBox::new(pad_x + cross_pos, pad_y + main_pos, cross_len, main_len)
            }
        });
    }
    out
}

/// 对扁平化子节点计算一维布局偏移（主轴依次排布，Cross 轴对齐到 0）。
///
/// `gap` 由调用方从节点读取（`Node::gap`，动态属性优先）。
///
/// 不知道容器尺寸时的退化实现，保留给不参与 flex 的调用方；
/// 有可用尺寸时请用 [`layout_boxes`]（支持 `grow`/`justify`/`align`）。
pub fn layout_offsets(layout: Layout, gap: f32, children: &[Node]) -> Vec<(f32, f32)> {
    match layout {
        Layout::Row => {
            let mut x = 0.0;
            children
                .iter()
                .map(|c| {
                    let o = (x, 0.0);
                    if !c.is_hidden() {
                        x += node_size(c).0 + gap;
                    }
                    o
                })
                .collect()
        }
        Layout::Column => {
            let mut y = 0.0;
            children
                .iter()
                .map(|c| {
                    let o = (0.0, y);
                    if !c.is_hidden() {
                        y += node_size(c).1 + gap;
                    }
                    o
                })
                .collect()
        }
    }
}

/// 键盘事件。
pub struct KeyEvent {
    pub key: Key,
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub meta: bool,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Key {
    Char(char),
    Enter,
    Backspace,
    Delete,
    ArrowUp,
    ArrowDown,
    ArrowLeft,
    ArrowRight,
    Tab,
    Escape,
    Home,
    End,
    PageUp,
    PageDown,
}

/// IME 输入法事件（组合态文本）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ImeEvent {
    /// 输入法已激活（开始组合）。
    Enabled,
    /// 输入法已停用。
    Disabled,
    /// 组合文本更新。[cursor] 为组合带内光标区间 `(start, end)`（字节，
    /// 对齐 winit `Ime::Preedit` 与 Parley `set_compose`）；`None` 表示隐藏光标。
    Preedit {
        text: String,
        cursor: Option<(usize, usize)>,
    },
    /// 组合确认文本。
    Commit(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn node_default_gap_is_zero() {
        // 对齐 Kotlin `FlexParam.gap` 与 wy-layout `FlexObject::gap()`
        // 的默认值（都曾/现为 0）。Node 层曾是 8.0，两层默认值不一致，
        // 且 Node 层的值实际生效——"忘了写 gap"的容器与上游行为不同。
        assert_eq!(Node::default().gap(), 0.0);
    }

    #[test]
    fn align_fix_container_has_no_cross_natural_size() {
        // 复刻 Kotlin `StackObject.alignFix` / wy-layout
        // `StackLayout::allow_size_from_children()` 为 false：
        // align_fix 容器交叉轴**没有自身自然尺寸**（不取子节点 max），
        // 主轴（顺排方向）仍由内容撑起。
        let leaf = |w: f32, h: f32| Node {
            width: w,
            height: h,
            ..Node::default()
        };
        let col = Node {
            layout: Some(Layout::Column),
            align_fix: true,
            arg_children_fn: Rc::new(move |cx| {
                cx.add_node(leaf(50.0, 20.0));
                cx.add_node(leaf(80.0, 30.0));
            }),
            ..Node::default()
        };
        let (w, h) = natural_size_of(Layout::Column, &col);
        // 主轴（高）= 20 + 30；交叉轴（宽）= 0（不由内容撑）
        assert_eq!(w, 0.0, "align_fix 容器交叉轴不应由内容撑");
        assert_eq!(h, 50.0, "主轴仍按内容累加");
    }

    #[test]
    fn no_align_fix_container_cross_size_from_children() {
        // 默认（align_fix=false）行为不变：交叉轴取非对齐子节点最大尺寸
        let col = Node {
            layout: Some(Layout::Column),
            arg_children_fn: Rc::new(move |cx| {
                cx.add_node(Node {
                    width: 50.0,
                    height: 20.0,
                    ..Node::default()
                });
                cx.add_node(Node {
                    width: 80.0,
                    height: 30.0,
                    ..Node::default()
                });
            }),
            ..Node::default()
        };
        let (w, h) = natural_size_of(Layout::Column, &col);
        assert_eq!(w, 80.0, "默认行为：交叉轴取子节点最大宽度");
        assert_eq!(h, 50.0);
    }

    #[test]
    fn align_fix_child_stretch_uses_parent_allocation() {
        // align_fix 容器 + Stretch 子节点：子节点交叉轴拿父分配内区
        // （与 wy-layout `align_fix` 时 `size_val = inside.inner_size` 一致）
        let child = Node {
            width: 50.0,
            height: 20.0,
            ..Node::default()
        };
        let col = Node {
            layout: Some(Layout::Column),
            align_fix: true,
            align_items: AlignItem::Stretch,
            arg_children_fn: Rc::new(move |cx| {
                cx.add_node(child.clone());
            }),
            ..Node::default()
        };
        let children = children_nodes(&col);
        let boxes = layout_boxes(&col, (200.0, 100.0), &children);
        assert_eq!(boxes.len(), 1);
        assert_eq!(boxes[0].width, 200.0, "Stretch 子节点交叉轴拿父分配内区");
    }

    #[test]
    fn pad_xy_overrides_padding_shorthand() {
        // 复刻 Kotlin 四向 padding：`padding` 是同值简写（底），
        // `pad_x`/`pad_y` 覆写对应方向。
        let n = Node {
            padding: 10.0,
            ..Node::default()
        };
        assert_eq!((n.pad_x_eff(), n.pad_y_eff()), (10.0, 10.0));
        let n = Node {
            padding: 10.0,
            pad_x: Some(4.0),
            ..Node::default()
        };
        assert_eq!(
            (n.pad_x_eff(), n.pad_y_eff()),
            (4.0, 10.0),
            "pad_x 覆写、pad_y 回落简写"
        );
        let n = Node {
            padding: 10.0,
            pad_y: Some(0.0),
            ..Node::default()
        };
        assert_eq!(
            (n.pad_x_eff(), n.pad_y_eff()),
            (10.0, 0.0),
            "pad_y: 0 是显式覆写而非缺省"
        );
    }

    #[test]
    fn pad_xy_splits_inner_area() {
        // 布局内区按方向分别扣：左右 4 / 上下 0（"左右 12、上下 0"的诉求）
        let child = Node {
            width: 10.0,
            height: 10.0,
            ..Node::default()
        };
        let col = Node {
            layout: Some(Layout::Column),
            padding: 12.0,
            pad_y: Some(0.0),
            align_items: AlignItem::Stretch,
            arg_children_fn: Rc::new(move |cx| {
                cx.add_node(child.clone());
            }),
            ..Node::default()
        };
        let children = children_nodes(&col);
        let boxes = layout_boxes(&col, (100.0, 40.0), &children);
        // 内区 = 100 - 2*12 = 76 宽；高不扣（pad_y=0）。
        // 注意 Column 的主轴是 Y：Stretch 只拉伸交叉轴（宽），
        // 主轴高度仍是 flex 自然尺寸（10）——不因 align 改变。
        assert_eq!(boxes[0].width, 76.0, "左右各扣 12，交叉轴 stretch 到内区");
        assert_eq!(boxes[0].height, 10.0, "主轴高度是 flex 自然尺寸，不吃满");
        assert_eq!(boxes[0].x, 12.0, "偏移加回左 padding");
        assert_eq!(boxes[0].y, 0.0, "偏移不加回纵向 padding");
    }

    #[test]
    fn last_box_is_shared_across_clones() {
        // 回归：`last_box` 若用普通 `Cell`，克隆之间各写各的——
        // 引擎写的是「树里那个克隆」，组件读的是「自己手里那个」，
        // 两者永远对不上，`last_box` 机制等于失效
        // （scroll 的滚动上限恒为 0、浮层无法判断可视区做翻转）。
        let n = Node::default();
        let clone = n.clone();

        n.last_box.set(LayoutBox::new(0.0, 0.0, 120.0, 80.0));

        assert_eq!(
            clone.last_box(),
            LayoutBox::new(0.0, 0.0, 120.0, 80.0),
            "克隆应看到同一份布局盒"
        );
        assert_eq!(n.last_box(), clone.last_box());
    }

    #[test]
    fn last_box_defaults_to_zero() {
        let n = Node::default();
        assert_eq!(n.last_box(), LayoutBox::ZERO);
    }
}
