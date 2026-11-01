//! 第二代声明式基础节点组件 [`node2()`]（对标 TS `canvasRender2` 的 `node2`）。
//!
//! TS 里 `node2` 是 `tfc(fn(ctx, node, arg: Node2Arg))`：把参数交给注入的 `node`，
//! 再把 `children` 摊进父上下文。这里的 props **就是**那份参数——[`Node2Props`]
//! 与 TS `Node2Arg` 逐字对应，中间**没有第二层 IR**：
//!
//! | TS `Node2Arg` | 本组件 props |
//! |---|---|
//! | `width?/height?: GetValue<number>` | `width/height: f32`（`#[getter]`，可写裸值或闭包） |
//! | `x?/y?(): number` | `x/y: f32`（`#[getter]`） |
//! | `size?/position?(s: PointKey)` | `size/position`（`impl Fn(Axis)` 保留，逐轴） |
//! | `paddingInlineStart?` … 四向 | `padding_inline_start` … 四向（`#[getter] f32`）+ `padding` 简写 |
//! | `layout?: LayoutDirection2` | `layout: LayoutDir2`（= `flex! { .. }`，省略即绝对定位） |
//! | `exts?: readonly any[]` | `exts: Vec<Ext>`（`grow` / `align` / `ignore_flex`） |
//! | `hide?/notInLayout?/skipDraw?/selectionEnabled?` | 同名（`#[getter] bool`，**带默认值**） |
//! | `focusable?/focusOrder?/focusTrap?` | 同名（`#[getter]`，读取式） |
//! | `cursor?: GetValue<CursorType>` | `cursor: CursorType`（`#[getter]`） |
//! | `draw?(ctx2, n)` | `draw: impl Fn(&mut Scene, &Node2)` |
//! | `drawAtParent?(ctx2)` | `draw_at_parent` |
//! | `acceptHit?/acceptClip?` | 同名 |
//! | `mouseClick?` … / `handleKey?` | `mouse_click` … / `handle_key`（收 [`KeyEvent2`]）|
//! | `children?: readonly Fc[]` | `children: Vec<Fc>` |
//!
//! # 只有一层：props 即节点参数
//!
//! `Node2::build` 直接吃这份 props（`Node2::empty().build(props)`），没有
//! "声明层 props → 引擎 IR" 的转接。[`Node2Props`] 里带默认值的开关（`hide` /
//! `focusable` / …）是 `Get<bool>` 而不是 `Option<Get<bool>>`：缺省已经是
//! `always(false)`，再包一层 `Option` 只会让"改一个字段"变成
//! `rest.hide = Some(always(true))` 这种过程式代码，还让"没写"与"写了 false"
//! 无法区分却又毫无意义。
//!
//! # `node2! { ..rest, .. }`：组件怎么写自己的差异
//!
//! `#[rec(extends = node2)]` 的组件拿到 `rest: Node2Props`，组件体只写覆盖项：
//!
//! ```ignore
//! node2! { ..rest, focusable: true, draw: .., exts_push: my_ext }
//! ```
//!
//! `..rest` 是 TS 的 `...rest`（宏把 props 展开成 builder 的起点），
//! `exts_push` 是 `Vec` 字段的追加 setter（`..rest` 带回调用方的扩展后再加自己那条）。
//!
//! # `extends` 要指向**最近的那一层基类**
//!
//! `extends = x` 表示「x 的参数我一个不重抄、全部继承」。所以选哪一层不是风格问题：
//! 在某个组件之上再加一层时，基类必须是**那个组件**，不是它脚下的 `node2`——
//! 否则中间那一层的字段（比如 `rich_text_node` 的 `no_wrap` / `node_ref`）就没有
//! 入口，只能靠手抄几个「透传」字段补，早晚漏。
//! 范例：`wrapped_text_node` 写 `#[rec(extends = rich_text_node)]`（TS
//! `WrappedTextNodeArg extends Omit<RichTextNodeArg,'spans'>` 的直译），它的
//! `!` 宏因此把 key 分三层转发（自有 → `RichTextNodeArg` → `Node2Props`）。
//!
//! 布局相关的 `grow` / `align` / `justify` / `align_items` / `gap` / `align_fix`
//! **不在节点本体上**——它们是布局系统的关注点：容器走 `layout: flex! { .. }`，
//! 子节点走 `exts: vec![grow(1.0)]`。这正是 TS 把 `Node2Arg` 与 `flex` / `grow`
//! 分开的做法。
//!
//! Rust 侧只多出两个**便捷字段**（语义等价、不改变模型）：
//! `clip`（= `drawAtParent` 裁剪 + `acceptClip` 门控的合体）与 `debug_name`（调试）。

use wy_render::Scene;

use crate::canvas_render2::focus::KeyEvent2;
use crate::canvas_render2::node2::{absolute, Axis, Ext, LayoutDir2, LayoutSize, Node2};
use crate::canvas_render2::protocol::MouseEvent2;
use crate::primitives::CursorType;
use crate::{rec, Fc, NodeContext, Ref2};

/// 声明式基础节点：命名 props + `children`（`Fc` 列表），对标 TS `node2`。
///
/// 读取式字段（尺寸/内边距/开关/光标）均标 `#[getter]`：调用处写的是**表达式**
/// （`width: 80.0` / `width: abc.get()`），宏把它包成 `move || 表达式`，
/// 到布局期/绘制期才求值；没写就 `None`（引擎走自动兜底）。
///
/// ```ignore
/// node2! {
///     layout: flex! { direction: Layout::Row, gap: 8.0 },
///     padding: 12.0,
///     children: vec![
///         node2! { width: 80.0, exts: vec![grow(1.0)] },
///     ],
/// }
/// ```
#[rec(arg = Node2Props)]
// 节点参数本来就是"TS `Node2Arg` 的全部字段"：47 个是它的形状，不是写得乱。
#[allow(clippy::too_many_arguments)]
pub fn node2(
    /// 自己的句柄槽（TS `ref`）：**普通参数 + 默认值**。不传就内部新建一个，
    /// 调用方拿不到这个 node；要拿就显式传一个 `Ref2<Node2>` 进来。
    #[props(default = Ref2::new())]
    node_ref: Ref2<Node2>,
    children: Vec<Fc>,
    // ---- 尺寸 / 位置（读取式）----
    /// 外包宽（TS `width?: GetValue<number>`）。
    #[getter]
    width: f32,
    /// 外包高。
    #[getter]
    height: f32,
    /// 逐轴声明尺寸（低层覆盖，TS `size?(s: PointKey): LayoutSize`）。
    size: impl Fn(Axis) -> LayoutSize + 'static,
    /// 相对父的横向位置（TS `x?(): number`）。
    #[getter]
    x: f32,
    /// 相对父的纵向位置。
    #[getter]
    y: f32,
    /// 逐轴位置（低层覆盖，TS `position?(s: PointKey): number`）。
    position: impl Fn(Axis) -> f32 + 'static,
    // ---- 内边距（四向读取式）----
    /// 四向统一内边距（简写：四向各自未声明时用它）。
    #[getter]
    padding: f32,
    /// 起始横向内边距（左），TS `paddingInlineStart`。
    #[getter]
    padding_inline_start: f32,
    /// 结束横向内边距（右），TS `paddingInlineEnd`。
    #[getter]
    padding_inline_end: f32,
    /// 起始纵向内边距（上），TS `paddingBlockStart`。
    #[getter]
    padding_block_start: f32,
    /// 结束纵向内边距（下），TS `paddingBlockEnd`。
    #[getter]
    padding_block_end: f32,
    // ---- 布局（容器属性收在 Flex 里，子节点伸展/对齐在 exts 里）----
    /// 容器布局（TS `layout?: LayoutDirection2`）：`flex! { .. }` 或省略
    /// （= [`absolute`] 绝对定位）。收的是一条策略句柄，不必包 `Some(..)`。
    #[props(default = absolute())]
    layout: LayoutDir2,
    /// 布局扩展（TS `exts?: readonly any[]`）：`grow(..)` / `align(..)` / `ignore_flex(..)`。
    exts: Vec<Ext>,
    // ---- 读取式开关 ----
    /// 是否隐藏（不参与布局与命中）。
    #[getter]
    #[props(default = false)]
    hide: bool,
    /// 是否不参与父布局（绝对定位的浮层/滚动条）。
    #[getter]
    #[props(default = false)]
    not_in_layout: bool,
    /// 是否参与文本选择（默认 `true`）。
    #[getter]
    #[props(default = true)]
    selection_enabled: bool,
    /// 是否跳过自身绘制（子节点仍绘）。
    #[getter]
    #[props(default = false)]
    skip_draw: bool,
    // ---- 焦点（读取式）----
    /// 是否可被 Tab 焦点遍历拾取。
    #[getter]
    #[props(default = false)]
    focusable: bool,
    /// Tab 遍历显式顺序（`None` = 文档序）。声明成 `Option<i32>`，默认即 `None`，
    /// 只留一层 Option（对齐 Kotlin `val focusOrder: Int? = null`）。
    #[getter]
    focus_order: Option<i32>,
    /// 是否圈定焦点（模态层）。
    #[getter]
    #[props(default = false)]
    focus_trap: bool,
    /// 获得焦点。
    on_focus: impl Fn() + 'static,
    /// 失去焦点。
    on_blur: impl Fn() + 'static,
    /// 悬停变化（`true` = 指针进入）。Rust 侧扩展：TS 用 `cursor` 派生悬停视觉。
    on_hover: impl Fn(bool) + 'static,
    /// 键盘事件（返回 `true` = 已消费）。
    handle_key: impl Fn(&KeyEvent2) -> bool + 'static,
    // ---- 命中 / 光标 ----
    /// 命中判定（本地坐标，TS `acceptHit`）。
    accept_hit: impl Fn(f32, f32) -> bool + 'static,
    /// 子树命中门控（本地坐标，TS `acceptClip`）。
    accept_clip: impl Fn(f32, f32) -> bool + 'static,
    /// 鼠标光标（TS `cursor?: GetValue<CursorType>`）。
    #[getter]
    cursor: CursorType,
    // ---- 绘制 ----
    /// 自身绘制（TS `draw?(ctx2, n)`：第二个参数就是本节点的最终几何）。
    draw: impl Fn(&mut Scene, &Node2) + 'static,
    /// 父坐标系预绘制钩子（TS `drawAtParent`）：父节点 `translate` 之前调用，
    /// 用来做"按父 inner 区裁剪"这类事情。
    draw_at_parent: impl Fn(&mut Scene, &Node2) + 'static,
    // ---- 指针事件（捕获相位 = 根→叶，先于冒泡）----
    /// 点击（TS `mouseClick`）。
    mouse_click: impl Fn(&mut MouseEvent2) + 'static,
    /// 点击捕获相位。
    mouse_click_capture: impl Fn(&mut MouseEvent2) + 'static,
    /// 按下（TS `mouseDown`）。
    mouse_down: impl Fn(&mut MouseEvent2) + 'static,
    /// 按下捕获相位。
    mouse_down_capture: impl Fn(&mut MouseEvent2) + 'static,
    /// 抬起（TS `mouseUp`）。
    mouse_up: impl Fn(&mut MouseEvent2) + 'static,
    /// 抬起捕获相位。
    mouse_up_capture: impl Fn(&mut MouseEvent2) + 'static,
    /// 移动（TS `mouseMove`：按下期间的拖拽/拖选）。
    mouse_move: impl Fn(&mut MouseEvent2) + 'static,
    /// 移动捕获相位。
    mouse_move_capture: impl Fn(&mut MouseEvent2) + 'static,
    /// 滚轮。
    mouse_wheel: impl Fn(&mut MouseEvent2) + 'static,
    /// 滚轮捕获相位。
    mouse_wheel_capture: impl Fn(&mut MouseEvent2) + 'static,
    /// 双击（TS `mouseDoubleClick`）。
    mouse_double_click: impl Fn(&mut MouseEvent2) + 'static,
    /// 双击捕获相位（TS `mouseDoubleClickCapture`）。
    mouse_double_click_capture: impl Fn(&mut MouseEvent2) + 'static,
    /// 右键菜单（TS `mouseContextMenu`）。
    mouse_context_menu: impl Fn(&mut MouseEvent2) + 'static,
    /// 右键菜单捕获相位（TS `mouseContextMenuCapture`）。
    mouse_context_menu_capture: impl Fn(&mut MouseEvent2) + 'static,
    // ---- Rust 侧便捷字段（语义等价，TS 用 drawAtParent/acceptClip 表达）----
    /// 裁剪子树（自身 + 子节点限制在布局盒内）。
    #[props(default = false)]
    clip: bool,
    /// 把**自己的子树**裁到**父节点**的内盒（对齐 TS `ScrollContent.drawAtParent`）。
    ///
    /// 与 [`Self::clip`] 的分工：
    /// - `clip`：`本盒` 裁自己后代（容器说"我的内容不许出我的框"）；
    /// - `clip_to_parent`：`父盒` 裁我这一支（**子节点**说"我在我爹的框里就行"）。
    ///
    /// TS 只有后者，而且是由子节点自己在 `drawAtParent` 里做的：
    /// `ctx2.rect(parent 内盒); ctx2.clip();` 发生在父循环 `translate` 之前
    /// （`tree/assemble.ts`：`save → drawAtParent → translate → draw → restore`）。
    /// 所以 `scrollContent` 不需要容器写 `clip` 就能把溢出内容裁掉——Rust 侧
    /// 用这个字段一一对应，滚动容器不必再额外声明 `clip`。
    #[props(default = false)]
    clip_to_parent: bool,
    /// 诊断用可读名（TS `debugName()`）。
    #[getter]
    debug_name: String,
) -> Fc {
    // TS `assemble.ts` 的 node2 工厂：**自己造节点**（不再接收预造的空壳），
    // 挂进父上下文，最后一步才写 ref——与 TS 里 `ctx.addNode(node); ref.set(node);`
    // 同序，中间那段"节点已完整、可响应消息但 ref 仍未写"的窗口也一致。
    // 返回的就是普通 `Fc`：什么时候造节点、什么时候挂进父上下文、什么时候写 ref，
    // 全写在明面上——宏不做任何隐式注入。
    Fc::new(move |cx: &mut NodeContext| {
        // props 直接就是节点参数：中间的"声明层 → IR"转接层已经没有存在的必要
        // ——两者字段一一同名同型，多一层只会让 `extends = node2` 的组件以为
        // 自己该去改 IR（于是写出 `rest.focusable = Some(..)` 这种过程式代码）。
        let props = Node2Props {
            width,
            height,
            size,
            x,
            y,
            position,
            padding,
            padding_inline_start,
            padding_inline_end,
            padding_block_start,
            padding_block_end,
            layout,
            exts,
            hide,
            not_in_layout,
            selection_enabled,
            skip_draw,
            focusable,
            focus_order,
            focus_trap,
            on_focus,
            on_blur,
            on_hover,
            accept_hit,
            accept_clip,
            cursor,
            draw,
            draw_at_parent,
            handle_key,
            mouse_click,
            mouse_click_capture,
            mouse_down,
            mouse_down_capture,
            mouse_up,
            mouse_up_capture,
            mouse_move,
            mouse_move_capture,
            mouse_wheel,
            mouse_wheel_capture,
            mouse_double_click,
            mouse_double_click_capture,
            mouse_context_menu,
            mouse_context_menu_capture,
            clip,
            clip_to_parent,
            debug_name,
            node_ref: node_ref.clone(),
            children,
        };

        let node = Node2::empty().build(props);
        node_ref.set(node.clone());
        node.mount(cx);
    })
}
