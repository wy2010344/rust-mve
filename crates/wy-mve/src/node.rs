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

type ArgChildrenFn = Rc<dyn Fn(&mut NodeContext)>;
type DrawFn = Rc<dyn Fn(&mut dyn std::any::Any)>;
type HitTestFn = Rc<dyn Fn(f32, f32) -> bool>;
type ClickFn = Rc<dyn Fn(&mut PointerEvent)>;
type KeyFn = Rc<dyn Fn(&mut KeyEvent) -> bool>;
/// 测量节点自然尺寸（宽高像素）。文本等内容撑开型节点用它提供 intrinsic 尺寸，
/// 供容器布局消费（复刻 Kotlin `RichTextNode.argWidth/argHeight`）。
type MeasureFn = Rc<dyn Fn() -> (f32, f32)>;

/// 节点是否参与事件命中（有 handler 才算可命中）。
pub fn has_handler(node: &Node) -> bool {
    node.on_click_fn.is_some() || node.on_down_fn.is_some() || node.on_up_fn.is_some()
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
    /// 一维容器角色：渲染期对扁平化后的子节点做排布。
    pub layout: Option<Layout>,
    /// 布局尺寸（由布局系统或组件设置）。
    pub x: f32,
    pub y: f32,
    /// 布局尺寸（由布局系统或组件设置）。
    pub width: f32,
    pub height: f32,
    /// 自然尺寸测量：文本等内容型节点的 intrinsic 尺寸来源；
    /// `node_size` 优先取它，其次才用 `width`/`height`。
    pub measure_fn: Option<MeasureFn>,
    /// 子节点缓存：构建期由 `arg_children_fn` 产出，之后复用。
    pub children: Rc<RefCell<Option<Vec<ChildSlot>>>>,
    /// 节点实例标识：克隆共享，用于依赖比对（复刻 Kotlin 引用相等语义）。
    pub identity: Rc<()>,
    /// 声明为隐藏节点的记录（供调试）。
    pub _dummy: Cell<()>,
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
            layout: self.layout,
            x: self.x,
            y: self.y,
            width: self.width,
            height: self.height,
            measure_fn: self.measure_fn.as_ref().map(Rc::clone),
            children: Rc::clone(&self.children),
            identity: Rc::clone(&self.identity),
            _dummy: Cell::new(()),
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
            key_fn: None,
            focusable: false,
            hidden: false,
            skip_draw: false,
            focus_order: None,
            focus_trap: false,
            selection_enabled: true,
            layout: None,
            x: 0.0,
            y: 0.0,
            width: 0.0,
            height: 0.0,
            measure_fn: None,
            children: Rc::new(RefCell::new(None)),
            identity,
            _dummy: Cell::new(()),
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

/// 一维容器排布：渲染期对扁平化后的子节点计算偏移。
///
/// 对应 Kotlin `FlexParam`/`Direction` 的一维布局模型。
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum Layout {
    /// 主轴 = X，子节点依次右排。
    Row { gap: f32 },
    /// 主轴 = Y，子节点依次下排。
    Column { gap: f32 },
}

/// 子节点类型：静态节点或动态区域。
///
/// 对应 Kotlin `ValueOrGetList<Node>`。
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

/// 一次性构建节点子结构：执行 `arg_children_fn` 并缓存结果。
///
/// 复刻 Kotlin `Node.children` 的惰性 memo：只执行一次，之后每次复用。
/// 对静态 `Value` 子树递归构建；动态区域不在此展开（渲染期实时求值）。
pub fn materialize(node: &Node) {
    let cache = node.children.borrow();
    if cache.is_some() {
        return;
    }
    drop(cache);

    let mut cx = NodeContext::new();
    node.run_arg_children(&mut cx);
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
/// 对应 Kotlin `FlexParam` 在布局期的尺寸派生。
pub fn node_size(node: &Node) -> (f32, f32) {
    match node.layout {
        Some(Layout::Row { gap }) => {
            let children = children_nodes(node);
            let mut x = 0.0;
            let mut h = 0.0;
            for c in &children {
                let (cw, ch) = node_size(c);
                x += cw + gap;
                if ch > h {
                    h = ch;
                }
            }
            let w = if children.is_empty() { 0.0 } else { x - gap };
            (w, h)
        }
        Some(Layout::Column { gap }) => {
            let children = children_nodes(node);
            let mut y = 0.0;
            let mut w = 0.0;
            for c in &children {
                let (cw, ch) = node_size(c);
                y += ch + gap;
                if cw > w {
                    w = cw;
                }
            }
            let h = if children.is_empty() { 0.0 } else { y - gap };
            (w, h)
        }
        None => {
            if let Some(m) = &node.measure_fn {
                let (w, h) = m();
                (w.max(node.width), h.max(node.height))
            } else {
                (node.width, node.height)
            }
        }
    }
}

/// 对扁平化子节点计算一维布局偏移（主轴依次排布，Cross 轴对齐到 0）。
pub fn layout_offsets(layout: Layout, children: &[Node]) -> Vec<(f32, f32)> {
    match layout {
        Layout::Row { gap } => {
            let mut x = 0.0;
            children
                .iter()
                .map(|c| {
                    let o = (x, 0.0);
                    x += node_size(c).0 + gap;
                    o
                })
                .collect()
        }
        Layout::Column { gap } => {
            let mut y = 0.0;
            children
                .iter()
                .map(|c| {
                    let o = (0.0, y);
                    y += node_size(c).1 + gap;
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
}
