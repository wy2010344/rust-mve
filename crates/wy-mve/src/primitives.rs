//! 两种写法共用的**纯数据类型**：光标、布局轴、鼠标键、键盘 / IME 事件。
//!
//! 这些不是"旧体系的遗留"，而是 `canvas_render2` 与宿主引擎之间的公共词汇：
//! - [`CursorType`]：节点声明光标，引擎拿去设 `canvas.style.cursor`；
//! - [`Layout`]：一维主轴方向，`Flex` / `LayoutDir2` 都用它；
//! - [`PointerButton`]：宿主指针事件的按键位（协议层 `MouseEvent2` 由它派生）；
//! - [`Key`] / [`KeyEvent`] / [`ImeEvent`]：宿主键盘 / 输入法事件。
//!
//! v1 的 `Node` 与它的一堆 `DynNum` / `DynBool` / `MeasureFn` 已经随旧声明层
//! 一起删除；本文件只剩"宿主与框架必须对齐的那几个枚举与结构体"。

/// 鼠标光标类型（对应 TS `canvasCore/Cursor` 的 `CursorType`）。
///
/// 枚举值即 CSS 关键字：宿主拿到后直接设 `canvas.style.cursor`。
/// 节点不指定时由引擎按「编辑器 → text / 可聚焦 → pointer」派生。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CursorType {
    /// 默认箭头。
    #[default]
    Default,
    /// 手型（可点击）。
    Pointer,
    /// 文本光标（可选中/可编辑）。
    Text,
}

/// 一维容器排布：渲染期对子节点计算偏移。
///
/// 对应 Kotlin `FlexParam`/`Direction` 的一维布局模型。
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum Layout {
    /// 主轴 = X，子节点依次右排。
    Row,
    /// 主轴 = Y，子节点依次下排。
    Column,
}

/// 鼠标按键（复刻 Kotlin `PointerEvent` 的 button 位；右键不派发
/// Down/Click，只派发 ContextMenu——避免误启文本拖选/滑块拖动）。
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub enum PointerButton {
    /// 左键（默认）。
    #[default]
    Left,
    /// 右键（按下触发 `on_context_menu`）。
    Right,
    /// 中键。
    Middle,
}

/// 键盘事件。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyEvent {
    /// 按下的键。
    pub key: Key,
    /// Ctrl 是否按住。
    pub ctrl: bool,
    /// Shift 是否按住。
    pub shift: bool,
    /// Alt 是否按住。
    pub alt: bool,
    /// Meta（Win / Cmd）是否按住。
    pub meta: bool,
}

/// 键位（宿主键码到框架语义键的映射结果）。
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Key {
    /// 可打印字符。
    Char(char),
    /// 回车。
    Enter,
    /// 退格。
    Backspace,
    /// 删除。
    Delete,
    /// 上方向键。
    ArrowUp,
    /// 下方向键。
    ArrowDown,
    /// 左方向键。
    ArrowLeft,
    /// 右方向键。
    ArrowRight,
    /// Tab。
    Tab,
    /// Esc。
    Escape,
    /// Home。
    Home,
    /// End。
    End,
    /// PageUp。
    PageUp,
    /// PageDown。
    PageDown,
}

/// IME 输入法事件（组合态文本）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ImeEvent {
    /// 输入法已激活（开始组合）。
    Enabled,
    /// 输入法已停用。
    Disabled,
    /// 组合文本更新。`cursor` 为组合带内光标区间 `(start, end)`（字节，
    /// 对齐 winit `Ime::Preedit` 与 Parley `set_compose`）；`None` 表示隐藏光标。
    Preedit {
        /// 组合态文本（尚未提交）。
        text: String,
        /// 组合带内光标区间（字节偏移）。
        cursor: Option<(usize, usize)>,
    },
    /// 组合确认文本。
    Commit(String),
}

/// 一个矩形盒（局部坐标 + 宽高）。
///
/// v1 里它是「父容器分给节点的布局结果」，还伴着一个 thread-local 的
/// `current_box()` 供 `draw` 读自身尺寸。**那套已经删除**：v2 的 `draw` 直接收
/// `&Node2`，`n.width()` / `n.height()` 就是最终生效几何，不必再借全局。
/// 这里只留这个纯值类型，给 [`NodeRef`](crate::NodeRef) 的 `box_()` / `abs_box()`
/// 当返回值。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LayoutBox {
    /// 左上角 x（局部坐标）。
    pub x: f32,
    /// 左上角 y（局部坐标）。
    pub y: f32,
    /// 宽。
    pub width: f32,
    /// 高。
    pub height: f32,
}

impl Default for LayoutBox {
    fn default() -> Self {
        Self::ZERO
    }
}

impl LayoutBox {
    /// 全零盒。
    pub const ZERO: Self = Self {
        x: 0.0,
        y: 0.0,
        width: 0.0,
        height: 0.0,
    };

    /// 构造一个盒。
    #[must_use]
    pub fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }
}
