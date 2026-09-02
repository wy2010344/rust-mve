//! Node：UI 元素的具象类型。
//!
//! 对应 Kotlin 的 `Node` 类。用闭包代替继承。

use std::rc::Rc;

use crate::context::NodeContext;

type ArgChildrenFn = Rc<dyn Fn(&mut NodeContext)>;
type DrawFn = Rc<dyn Fn(&mut dyn std::any::Any)>;
type HitTestFn = Rc<dyn Fn(f32, f32) -> bool>;
type ClickFn = Rc<dyn Fn(&mut PointerEvent)>;
type KeyFn = Rc<dyn Fn(&mut KeyEvent) -> bool>;

/// Node：UI 元素的具象类型。
///
/// 通过闭包配置行为，不需要定义子类。
/// 所有闭包用 `Rc` 存储，支持廉价 Clone。
pub struct Node {
    pub(crate) arg_children_fn: ArgChildrenFn,
    pub(crate) draw_fn: DrawFn,
    pub(crate) hit_test_fn: HitTestFn,
    pub(crate) on_click_fn: Option<ClickFn>,
    pub(crate) on_down_fn: Option<ClickFn>,
    pub(crate) on_up_fn: Option<ClickFn>,
    pub(crate) key_fn: Option<KeyFn>,
    pub(crate) focusable: bool,
    pub(crate) hidden: bool,
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
        }
    }
}

impl PartialEq for Node {
    fn eq(&self, _other: &Self) -> bool {
        // 闭包不支持比较，Node 的相等性由调用方（Memo）保证
        false
    }
}

impl Default for Node {
    fn default() -> Self {
        Self::new()
    }
}

impl Node {
    /// 创建空节点。
    pub fn new() -> Self {
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
        }
    }

    /// 创建文本节点。
    pub fn text(_content: &str) -> Self {
        Self {
            draw_fn: Rc::new(move |_scene| {
                // 文本绘制由渲染层处理
            }),
            ..Self::new()
        }
    }

    /// 创建矩形节点。
    pub fn rect() -> Self {
        Self::new()
    }

    /// 设置子节点构建逻辑。
    ///
    /// 对应 Kotlin 的 `override fun argChildren()`。
    pub fn arg_children(mut self, f: impl Fn(&mut NodeContext) + 'static) -> Self {
        self.arg_children_fn = Rc::new(f);
        self
    }

    /// 设置绘制逻辑。
    pub fn draw(mut self, f: impl Fn(&mut dyn std::any::Any) + 'static) -> Self {
        self.draw_fn = Rc::new(f);
        self
    }

    /// 设置命中测试。
    pub fn hit_test(mut self, f: impl Fn(f32, f32) -> bool + 'static) -> Self {
        self.hit_test_fn = Rc::new(f);
        self
    }

    /// 设置点击事件。
    pub fn on_click(mut self, f: impl Fn(&mut PointerEvent) + 'static) -> Self {
        self.on_click_fn = Some(Rc::new(f));
        self
    }

    /// 设置按下事件。
    pub fn on_pointer_down(mut self, f: impl Fn(&mut PointerEvent) + 'static) -> Self {
        self.on_down_fn = Some(Rc::new(f));
        self
    }

    /// 设置释放事件。
    pub fn on_pointer_up(mut self, f: impl Fn(&mut PointerEvent) + 'static) -> Self {
        self.on_up_fn = Some(Rc::new(f));
        self
    }

    /// 设置按键事件。
    pub fn on_key(mut self, f: impl Fn(&mut KeyEvent) -> bool + 'static) -> Self {
        self.key_fn = Some(Rc::new(f));
        self
    }

    /// 设为可聚焦。
    pub fn focusable(mut self) -> Self {
        self.focusable = true;
        self
    }

    /// 设为隐藏。
    pub fn hide(mut self) -> Self {
        self.hidden = true;
        self
    }

    // --- 执行 ---

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
