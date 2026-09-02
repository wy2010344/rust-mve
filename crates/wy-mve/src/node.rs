//! Node：UI 元素的具象类型。
//!
//! 对应 Kotlin 的 `Node` 类。用闭包代替继承。
//! 声明式构造：像 JS 对象或 Kotlin 匿名类一样一次性构建，不可修改。

use std::rc::Rc;

use crate::context::NodeContext;

type ArgChildrenFn = Rc<dyn Fn(&mut NodeContext)>;
type DrawFn = Rc<dyn Fn(&mut dyn std::any::Any)>;
type HitTestFn = Rc<dyn Fn(f32, f32) -> bool>;
type ClickFn = Rc<dyn Fn(&mut PointerEvent)>;
type KeyFn = Rc<dyn Fn(&mut KeyEvent) -> bool>;

/// Node：UI 元素的具象类型。
///
/// 声明式构造，一次性构建完成，之后不可修改。
///
/// ```ignore
/// // JS/Kotlin 风格的声明式构造
/// let node = Node {
///     draw_fn: Rc::new(|scene| { ... }),
///     on_click_fn: Some(Rc::new(|_| { ... })),
///     arg_children_fn: Rc::new(|cx| { ... }),
///     ..Node::default()
/// };
/// ```
pub struct Node {
    /// 绘制逻辑。
    pub draw_fn: DrawFn,
    /// 子节点构建逻辑。
    pub arg_children_fn: ArgChildrenFn,
    /// 命中测试逻辑。
    pub hit_test_fn: HitTestFn,
    /// 点击事件。
    pub on_click_fn: Option<ClickFn>,
    /// 按下事件。
    pub on_down_fn: Option<ClickFn>,
    /// 释放事件。
    pub on_up_fn: Option<ClickFn>,
    /// 按键事件。
    pub key_fn: Option<KeyFn>,
    /// 是否可聚焦。
    pub focusable: bool,
    /// 是否隐藏（不参与布局和命中测试）。
    pub hidden: bool,
    /// 跳过自身绘制（但子节点仍绘制）。
    pub skip_draw: bool,
    /// 焦点顺序（显式 Tab 顺序，None = 按文档顺序）。
    pub focus_order: Option<i32>,
    /// 是否为焦点陷阱（模态窗口内焦点不外泄）。
    pub focus_trap: bool,
    /// 是否启用选择。
    pub selection_enabled: bool,
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
            key_fn: self.key_fn.as_ref().map(Rc::clone),
            focusable: self.focusable,
            hidden: self.hidden,
            skip_draw: self.skip_draw,
            focus_order: self.focus_order,
            focus_trap: self.focus_trap,
            selection_enabled: self.selection_enabled,
        }
    }
}

impl PartialEq for Node {
    fn eq(&self, _other: &Self) -> bool {
        false
    }
}

impl Default for Node {
    fn default() -> Self {
        Self {
            arg_children_fn: Rc::new(|_| {}),
            draw_fn: Rc::new(|_| {}),
            hit_test_fn: Rc::new(|_, _| true),
            on_click_fn: None,
            on_down_fn: None,
            on_up_fn: None,
            key_fn: None,
            focusable: false,
            hidden: false,
            skip_draw: false,
            focus_order: None,
            focus_trap: false,
            selection_enabled: true,
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

    pub fn run_key(&self, event: &mut KeyEvent) -> bool {
        if let Some(f) = &self.key_fn {
            f(event)
        } else {
            false
        }
    }

    pub fn is_focusable(&self) -> bool {
        self.focusable
    }

    pub fn is_hidden(&self) -> bool {
        self.hidden
    }
}

/// 指针事件。
pub struct PointerEvent {
    pub x: f32,
    pub y: f32,
    pub root_x: f32,
    pub root_y: f32,
    pub stopped: bool,
}

impl PointerEvent {
    pub fn new(x: f32, y: f32) -> Self {
        Self {
            x,
            y,
            root_x: x,
            root_y: y,
            stopped: false,
        }
    }

    pub fn stop_propagation(&mut self) {
        self.stopped = true;
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
}
