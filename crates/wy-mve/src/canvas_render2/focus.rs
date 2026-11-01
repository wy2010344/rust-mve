//! 焦点真相源（对标 TS `canvasRender2/Focus.ts`）。
//!
//! TS 的 `Focus.ts` 在把状态下沉到 `tree/engineGlobal.ts` 之后只剩两件事：
//! 1. re-export 引擎全局（`focusedSignal` / `pressedSignal` / 修饰键 / 按键旁听）；
//! 2. 持有编辑器句柄的**强类型**（`EditableTextHandle`），并把
//!    `activeEditorSignal` 里的协议层标记 `EditorHandleExt` 下转成它
//!    （`getActiveEditor`）。
//!
//! Rust 侧一一对应：状态在 [`crate::canvas_render2::tree::engine_global`]，
//! 本文件 re-export 它并只保留下转与键盘 / IME 协议类型。
//!
//! 与 v1 的差别（**不再**为兼容而保留）：早期 Rust 侧把焦点存成节点 id
//! （`Signal<u64>`）、并用一张 `id → EditableTextHandle` 注册表来取活跃编辑器；
//! TS 存的是**节点本身**，活跃编辑器是 `node.findExt(EditorHandleExt)` 的结果。
//! 注册表方案会漏掉"未登记 id 的编辑器"，也会在节点销毁后残留条目
//! （`unregister_editor` 全仓库无人调用）——已按 TS 改为 ext 派生。

use std::rc::Rc;

use crate::{Key, KeyEvent};

use crate::canvas_render2::node2::Node2;
use crate::canvas_render2::protocol::EditorHandleExt;
use crate::canvas_render2::selection::{SelPair2, Selectable2};
use crate::canvas_render2::tree::engine_global;

pub use crate::canvas_render2::tree::engine_global::{
    active_editor_ext, apply_modifiers, clear_modifiers, is_pressed, modifiers, notify_key_press,
    pressed_node, register_key_press, set_pressed, Modifiers,
};

/// 当前持有焦点的节点 id（`0` = 无焦点）。
///
/// TS 没有这条——它是给无障碍树 / 引擎诊断用的派生值
/// （`focused_node().id`）。
#[must_use]
pub fn focused_id() -> u64 {
    engine_global::focused_node().map_or(0, |n| n.id())
}

/// 当前持有焦点的节点（TS `focusedSignal.get()`）。
#[must_use]
pub fn focused_node() -> Option<Node2> {
    engine_global::focused_node()
}

/// 指定节点是否持有焦点（TS `isFocused(node)`）。
#[must_use]
pub fn is_focused(node: &Node2) -> bool {
    engine_global::is_focused(node)
}

/// 请求把焦点给指定节点（TS `requestFocus(node)`）。
///
/// 若该节点挂了 [`EditorHandleExt`]，则同时设为活跃编辑器——与 TS
/// `activeEditorSignal.set(node.findExt(EditorHandleExt))` 一致。
pub fn request_focus(node: &Node2) {
    engine_global::request_focus(node);
}

/// 清空焦点（窗口失焦时调用，TS `clearFocus()`）。
pub fn clear_focus() {
    engine_global::clear_focus();
}

/// 当前活跃编辑器（键盘 / IME / 剪贴板快捷键的路由目标）。
///
/// 对标 TS `Focus.getActiveEditor()`：底层 `activeEditorSignal` 存的是协议层标记
/// （`impl` 未知），这里把它下转成强类型句柄。
#[must_use]
pub fn active_editor() -> Option<Rc<dyn EditableTextHandle>> {
    engine_global::active_editor_ext().and_then(|ext| {
        ext.0
            .downcast::<Rc<dyn EditableTextHandle>>()
            .ok()
            .map(|r| (*r).clone())
    })
}

/// 把编辑器句柄包成协议层标记（TS `new EditorHandleExt(handle)` 后塞进 `exts`）。
///
/// 编辑器节点在实现 [`Selectable2`] 的同时把本标记挂到 `exts` 上，焦点切换时
/// 引擎即可经 [`active_editor`] 取到强类型句柄。
#[must_use]
pub fn editor_handle_ext(handle: Rc<dyn EditableTextHandle>) -> EditorHandleExt {
    EditorHandleExt(Rc::new(handle) as Rc<dyn std::any::Any>)
}

/// 重置全局焦点状态（测试用）。
#[doc(hidden)]
pub fn reset() {
    engine_global::reset();
}

// ============================================================================
// 键盘事件 / 编辑器句柄（对标 TS `Focus.ts`）
// ============================================================================

/// 键位常量（对齐 TS `KeyCode2` 对象成员名；物理键码用 `KeyboardEvent.code`）。
// 故意保留 TS 的驼峰命名 `KeyCode2`，便于和真值源逐名对照。
#[allow(non_snake_case)]
pub mod KeyCode2 {
    /// 退格。
    pub const BACKSPACE: &str = "Backspace";
    /// 删除。
    pub const DELETE: &str = "Delete";
    /// 左箭头。
    pub const LEFT: &str = "ArrowLeft";
    /// 右箭头。
    pub const RIGHT: &str = "ArrowRight";
    /// 上箭头。
    pub const UP: &str = "ArrowUp";
    /// 下箭头。
    pub const DOWN: &str = "ArrowDown";
    /// Home。
    pub const HOME: &str = "Home";
    /// End。
    pub const END: &str = "End";
    /// Page Up。
    pub const PAGE_UP: &str = "PageUp";
    /// Page Down。
    pub const PAGE_DOWN: &str = "PageDown";
    /// 回车。
    pub const ENTER: &str = "Enter";
    /// Tab。
    pub const TAB: &str = "Tab";
    /// Escape。
    pub const ESCAPE: &str = "Escape";
}

/// 键盘事件（对标 TS `KeyEvent2`）。
///
/// `code` 用物理键位（`KeyCode2` 常量），`key` 为可打印字符（单字符）或键名
/// （控制键，长度 > 1，`handleKey` 的 `default` 分支据此拒绝录入）。
///
/// 引擎每次按键上报 `crate::KeyEvent`（只有 `key: Key` 枚举，无物理 `code`），
/// [`KeyEvent2::from_mve`] 据此补齐 `code` / `key`。
#[derive(Clone, Debug)]
pub struct KeyEvent2 {
    /// 实际字符（可打印键为单字符；控制键为空串或符号）。
    pub key: String,
    /// 物理键位（`KeyboardEvent.code`）。
    pub code: String,
    /// 是否按住 Ctrl。
    pub ctrl: bool,
    /// 是否按住 Shift。
    pub shift: bool,
    /// 是否按住 Alt。
    pub alt: bool,
    /// 是否按住 Meta（Win/Cmd）。
    pub meta: bool,
}

impl KeyEvent2 {
    /// 从引擎 [`KeyEvent`] 构造（补齐 `code` / `key`）。
    #[must_use]
    pub fn from_mve(e: &KeyEvent) -> Self {
        let code: &str = match e.key {
            Key::Enter => KeyCode2::ENTER,
            Key::Backspace => KeyCode2::BACKSPACE,
            Key::Delete => KeyCode2::DELETE,
            Key::ArrowUp => KeyCode2::UP,
            Key::ArrowDown => KeyCode2::DOWN,
            Key::ArrowLeft => KeyCode2::LEFT,
            Key::ArrowRight => KeyCode2::RIGHT,
            Key::Tab => KeyCode2::TAB,
            Key::Escape => KeyCode2::ESCAPE,
            Key::Home => KeyCode2::HOME,
            Key::End => KeyCode2::END,
            Key::PageUp => KeyCode2::PAGE_UP,
            Key::PageDown => KeyCode2::PAGE_DOWN,
            Key::Char(_) => "",
        };
        let key: String = match e.key {
            Key::Char(c) => c.to_string(),
            _ => code.to_string(),
        };
        Self {
            key,
            code: code.to_string(),
            ctrl: e.ctrl,
            shift: e.shift,
            alt: e.alt,
            meta: e.meta,
        }
    }
}

/// 原生输入控件（隐藏 textarea）的期望位置，用于把输入法候选框定位到光标处
/// （对标 TS `InputOverlayRect`）。
///
/// 坐标为画布内的 CSS 像素（不含 canvas 自身的 left/top）。
#[derive(Clone, Copy, Debug)]
pub struct InputOverlayRect {
    /// 横坐标。
    pub x: f32,
    /// 纵坐标。
    pub y: f32,
    /// 宽。
    pub w: f32,
    /// 高。
    pub h: f32,
    /// 字号。
    pub font_size: f32,
}

/// 活跃编辑器的统一句柄：焦点落在该节点上时，引擎的键盘 / IME / 剪贴板
/// 全局快捷键全部经此路由（对标 TS `EditableTextHandle`）。
///
/// 编辑器节点在实现 [`Selectable2`] 的同时实现本 trait，焦点切换时由
/// [`request_focus`] 自动登记 / 注销活跃编辑器。
pub trait EditableTextHandle {
    /// 本地光标端点对（活跃编辑器光标选区真相源）；未定位返回 `None`。
    fn cursor_sel_pair(&self) -> Option<SelPair2>;
    /// 是否存在非塌缩选区。
    fn has_selection(&self) -> bool;
    /// 剪切。
    fn cut(&self);
    /// 复制。
    fn copy(&self);
    /// 粘贴。
    fn paste(&self);
    /// 编辑器内全选（写入本地光标信号）。
    fn select_all(&self);
    /// 编程式设置选区内光标区间（双击选词 / 三击选段 / 程序化 select 的落点）。
    fn select_range(&self, anchor_offset: usize, focus_offset: usize);
    /// 塌缩外部显式选区（引擎在指针按下落点不在本编辑器时调用）。
    fn collapse_external_selection(&self);
    /// IME 预编辑串上报（空串 = 提交并结束组合）。
    fn on_composing(&self, text: &str, cursor: usize);
    /// 输入法组合结束：以最终串收尾并提交（空串表示放弃本次组合）。
    fn commit_composing_text(&self, final_text: &str);
    /// 原生输入控件的期望位置（光标处），供宿主定位 textarea 让候选框跟随光标。
    fn input_overlay(&self) -> Option<InputOverlayRect>;
    /// 键盘事件处理；返回 `true` 表示已消费。
    fn handle_key(&self, e: &KeyEvent2) -> bool;
    /// 返回自身作为 [`Selectable2`]（供 SelectionManager 派生范围与端点）。
    fn as_selectable(&self) -> Rc<dyn Selectable2>;
}
