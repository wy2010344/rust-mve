//! canvasRender2 的节点消息协议（TS `tree/protocol.ts` 的 1:1 Rust 复刻）。
//!
//! 这一层只有**类型与最外层数据载体**，没有任何树算法——算法在
//! [`mod@crate::canvas_render2::node2`]（测量 / 命中 / 装配）与未来会拆出的
//! `tree/*` 纯消息模块里，且全部只**向节点发消息**、不读对象结构。
//!
//! 移植契约（与 TS 同形）：
//! 1. 节点只回答消息：`node.width()` / `node.children()` / `node.hide()` 一律是
//!    「向节点发一条消息」，实现方是 `Node2` 的方法转调。
//! 2. 上层算法不得检查成员是否存在、不得按键取值。缺省行为由「兜底对象 + merge」
//!    提供，所以每条消息**永远答得出来**。
//!
//! 指针事件 [`MouseEvent2`] 是协议层事件（对标 Kotlin `PointerEvent`）：节点钩子
//! 收它，而不是宿主的原始事件。宿主（`crate::canvas_render2::node2::Node2::to_node`
//! 的桥接 / `wy-engine` 的派发）负责把宿主事件翻译成 [`MouseEvent2`] 再派发。

use std::cell::Cell;
use std::rc::Rc;

/// 输入设备类型（多点触控 / 触控笔场景用；DOM 侧固定 mouse）。
///
/// 与 TS `tree/protocol.ts` 的 `PointerDevice` 同形。
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum PointerDevice {
    /// 鼠标。
    Mouse,
    /// 触摸。
    Touch,
    /// 触控笔。
    Pen,
}

/// 指针事件类型（新增取值一律追加到末尾，避免序号漂移）。
///
/// 与 TS `MouseType2` 同形：`down` / `move` / `up` / `click` / `cancel` /
/// `wheel` / `dblclick` / `contextmenu`。
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum MouseType2 {
    /// 按下。
    Down,
    /// 移动。
    Move,
    /// 抬起。
    Up,
    /// 点击（按下 + 抬起）。
    Click,
    /// 取消（如触摸被系统中断）。
    Cancel,
    /// 滚轮。
    Wheel,
    /// 双击（不吞单击）。
    DoubleClick,
    /// 右键上下文菜单（右键不派发 down/click）。
    ContextMenu,
}

/// 鼠标按键编号（沿用 AWT `MouseEvent.getButton()`：1=左 2=中 3=右）。
pub const POINTER_BUTTON_LEFT: i32 = 1;
/// 鼠标中键编号。
pub const POINTER_BUTTON_MIDDLE: i32 = 2;
/// 鼠标右键编号。
pub const POINTER_BUTTON_RIGHT: i32 = 3;
/// 按住位掩码（沿用 AWT 位序：左=1 中=2 右=4）。
pub const POINTER_LEFT_DOWN_MASK: i32 = 1;
/// 中键按住位掩码。
pub const POINTER_MIDDLE_DOWN_MASK: i32 = 2;
/// 右键按住位掩码。
pub const POINTER_RIGHT_DOWN_MASK: i32 = 4;

/// 按键编号 → 按住位。
///
/// 未知编号返回 0：宁可少报一个键，也不要把不认识的键混进按住状态里，
/// 让调用方误判成"左键被按住"。与 TS `pointerButtonMask` 同形。
pub fn pointer_button_mask(button: i32) -> i32 {
    if button == POINTER_BUTTON_LEFT {
        POINTER_LEFT_DOWN_MASK
    } else if button == POINTER_BUTTON_MIDDLE {
        POINTER_MIDDLE_DOWN_MASK
    } else if button == POINTER_BUTTON_RIGHT {
        POINTER_RIGHT_DOWN_MASK
    } else {
        0
    }
}

/// 派发给节点的指针事件（局部坐标 + 全局坐标 + 按键状态 + 中断标记）。
///
/// 字段与 Kotlin 的 `PointerEvent` 对齐：`button` 是**触发本次事件**的键，
/// `buttons` 是**当前按住哪些键**的掩码（判断"是否正按住左键拖拽"用它）。
/// 修饰键（Ctrl/Shift…）不放事件里，走全局信号（见 `Focus.ts` 的 modifiers）。
///
/// 与 TS `MouseEvent2` 同形。ooc / Rust 侧同样构造一个只响应这几条消息的对象。
#[derive(Debug)]
pub struct MouseEvent2 {
    /// 事件类型。
    pub type_: MouseType2,
    /// 输入设备。
    pub device: PointerDevice,
    /// 指针编号（多点触控 / 多笔区分；鼠标恒 0）。
    pub pointer_id: i32,
    /// 相对当前节点的局部坐标。
    pub x: f32,
    /// 相对当前节点的局部坐标。
    pub y: f32,
    /// 相对画布（渲染根）的全局坐标。
    pub global_x: f32,
    /// 相对画布（渲染根）的全局坐标。
    pub global_y: f32,
    /// 触发本次事件的按键编号（**引擎编号**：1=左 / 2=中 / 3=右，0=无按键）。
    ///
    /// move / wheel / dblclick 等"不来自某个键"的事件为 0。
    /// 宿主的原生编号由宿主适配器翻译过来（DOM 的 0=左 → 引擎的 1=左）。
    pub button: i32,
    /// 当前按住的按键位掩码。
    pub buttons: i32,
    /// 滚轮水平位移（仅 wheel 事件非 0）。
    pub wheel_delta_x: f32,
    /// 滚轮垂直位移（仅 wheel 事件非 0）。
    pub wheel_delta_y: f32,
    /// 压感（鼠标恒 1）。
    pub pressure: f32,
    /// 是否已停止传播（中断命中链）。
    stopped_progression: Cell<bool>,
}

impl MouseEvent2 {
    /// 事件是否已被停止传播。
    pub fn stopped_progression(&self) -> bool {
        self.stopped_progression.get()
    }

    /// 停止事件传播（中断命中链）。
    pub fn stop_propagation(&self) {
        self.stopped_progression.set(true);
    }
}

/// 构造指针事件的入参（宿主侧填，缺省项由 [`create_mouse_event2`] 兜底）。
///
/// 与 TS `MouseEventInit2` 同形。
pub struct MouseEventInit2 {
    /// 事件类型（必填）。
    pub type_: MouseType2,
    /// 局部坐标。
    pub x: f32,
    /// 局部坐标。
    pub y: f32,
    /// 全局坐标。
    pub global_x: f32,
    /// 全局坐标。
    pub global_y: f32,
    /// 输入设备（缺省 mouse）。
    pub device: Option<PointerDevice>,
    /// 指针编号（缺省 0）。
    pub pointer_id: Option<i32>,
    /// 触发按键编号（缺省 0）。
    pub button: Option<i32>,
    /// 按住位掩码（缺省 0）。
    pub buttons: Option<i32>,
    /// 滚轮水平位移（缺省 0）。
    pub wheel_delta_x: Option<f32>,
    /// 滚轮垂直位移（缺省 0）。
    pub wheel_delta_y: Option<f32>,
    /// 压感（缺省 1）。
    pub pressure: Option<f32>,
}

/// 构造 [`MouseEvent2`]（缺省项兜底），与 TS `createMouseEvent2` 同形。
pub fn create_mouse_event2(init: MouseEventInit2) -> MouseEvent2 {
    MouseEvent2 {
        type_: init.type_,
        device: init.device.unwrap_or(PointerDevice::Mouse),
        pointer_id: init.pointer_id.unwrap_or(0),
        x: init.x,
        y: init.y,
        global_x: init.global_x,
        global_y: init.global_y,
        button: init.button.unwrap_or(0),
        buttons: init.buttons.unwrap_or(0),
        wheel_delta_x: init.wheel_delta_x.unwrap_or(0.0),
        wheel_delta_y: init.wheel_delta_y.unwrap_or(0.0),
        pressure: init.pressure.unwrap_or(1.0),
        stopped_progression: Cell::new(false),
    }
}

/// 编辑器句柄标记（挂在节点 `exts` 上的标记载体，与 `SelectableExt` 同机制）。
///
/// 节点结构体不承载 `cursor_sel_pair` / `handle_key` 等未知字段，统一经 `findExt`
/// 识别。放在协议层是因为「命中链上的节点是不是编辑器」是派发 / 光标派生
/// 要直问的问题——它只认这个标记类，不关心标记里 `impl` 具体是什么
/// （ooc 侧由内部 Symbol 承担，`impl` 因而留 `unknown`）。
///
/// 与 TS `EditorHandleExt` 同形。
pub struct EditorHandleExt(pub std::rc::Rc<dyn std::any::Any>);

impl Clone for EditorHandleExt {
    fn clone(&self) -> Self {
        Self(Rc::clone(&self.0))
    }
}

/// 比较按**引用同一性**（TS `activeEditorSignal` 是信号，换值才触发）。
impl PartialEq for EditorHandleExt {
    fn eq(&self, other: &Self) -> bool {
        std::rc::Rc::ptr_eq(&self.0, &other.0)
    }
}
