//! MVE 集成：将 `wy-mve` 的 Node 树连接到渲染管线。
//!
//! # 构建一次模型（复刻 Kotlin `Renderer.signal.collect { draw() }`）
//!
//! - `MveApp::new(callback)` 用 [`render_root`] **一次性**构建树结构：
//!   callback 只执行一次，顶层快照 Slot 固化，动态列表封成 memo 区域。
//! - 每帧 `draw()` 读取 `Root.target`（memo）得到当前扁平节点列表，绘制期
//!   按容器的 `Layout` 角色实时计算偏移——**不再每帧重建 Node 树**。
//! - draw / 事件处理遍历节点时读取信号（被 `RedrawTracker` 自动追踪），
//!   信号变化只触发受影响区域重算 → 局部重绘，与 Kotlin 行为一致。

use std::cell::UnsafeCell;
use std::rc::Rc;
use std::time::Instant;

use wy_mve::{children_nodes, has_handler, render_root, Node, NodeContext};
use wy_render::{Point, Rect, Scene};
use wy_signal::{GetValue, SetValue, Signal};

use crate::focus::{locate, tab_navigate, FocusManager, FocusPath};

/// 双击判定参数（对齐 Kotlin `Renderer.DoubleClickDetector` 默认值）。
const DOUBLE_CLICK_INTERVAL_MS: u128 = 400;
/// 双击落点距离阈值（px）：超过则不算同一次双击。
const DOUBLE_CLICK_DIST: f32 = 5.0;

/// 顶层节点遍历（`avail` 为窗口逻辑尺寸，作为根容器的主轴可用空间）。
fn draw_nodes(nodes: &[Node], scene: &mut Scene, avail: (f32, f32)) {
    for node in nodes {
        draw_node(node, (0.0, 0.0), avail, scene);
    }
}

/// 递归绘制：自我绘制在最底层，子节点按文档序叠在其上（对应 Kotlin Node.draw）。
///
/// 位置/可见性/间距走节点的动态属性（`pos_x`/`pos_y`/`is_hidden`/`gap`），
/// 在 track 上下文内求值，读信号自动触发重绘。
///
/// `avail` 是父容器分给本节点的尺寸：布局容器据此做 flex 排布，
/// 同时作为 [`wy_mve::current_box`] 暴露给 `draw`（组件排版自身内容需要它）。
fn draw_node(node: &Node, origin: (f32, f32), avail: (f32, f32), scene: &mut Scene) {
    if node.is_hidden() {
        return;
    }
    // 记录本节点本帧分到的布局盒（局部尺寸）：事件回调（滚轮上限、滑块几何）
    // 与 hide_fn 里读 `last_box()` 都以此为准——flex 分配每帧变化，构建期不可知。
    node.last_box
        .set(wy_mve::LayoutBox::new(0.0, 0.0, avail.0, avail.1));
    let pos = (origin.0 + node.pos_x(), origin.1 + node.pos_y());
    if pos != (0.0, 0.0) {
        scene.push_transform(Point::new(pos.0, pos.1));
    }
    // 裁剪容器（scroll 等）：自身 + 子节点都限制在布局盒内（overflow hidden）。
    // 裁剪矩形用节点局部坐标——Scene 的变换栈会把后续图元（含 clip）一起平移。
    let clipped = node.clip && avail.0 > 0.0 && avail.1 > 0.0;
    if clipped {
        scene.push_clip(Rect::new(0.0, 0.0, avail.0, avail.1));
    }
    if !node.skip_draw {
        wy_mve::with_box(wy_mve::LayoutBox::new(0.0, 0.0, avail.0, avail.1), || {
            node.run_draw(scene)
        });
    }
    let children = children_nodes(node);
    if node.layout.is_some() {
        let boxes = wy_mve::layout_boxes(node, avail, &children);
        for (child, b) in children.iter().zip(boxes.iter()) {
            // 注意：`origin` 是**相对父节点**的偏移（父已 push_transform），
            // 所以这里直接传布局盒偏移，不能再叠加绝对坐标。
            draw_node(child, (b.x, b.y), (b.width, b.height), scene);
        }
    } else {
        for child in children.iter() {
            // 无布局容器（group）：子节点绝对定位，尺寸用自身自然尺寸
            let own = wy_mve::node_size(child);
            draw_node(child, (0.0, 0.0), own, scene);
        }
    }
    if clipped {
        scene.pop_clip();
    }
    if pos != (0.0, 0.0) {
        scene.pop_transform();
    }
}

/// 命中测试：子节点优先（后绘制的在上，倒序），对应 Kotlin `Node.hitTest`。
#[cfg(test)]
///
/// `avail` 必须与实际绘制时传给 [`draw_nodes`] 的视口一致，否则 flex 分配
/// 出来的尺寸不同，会出现"看得见但点不中"（测试里传 `(0.0, 0.0)` 会让
/// `grow` 的子节点分到 0 宽度）。
fn hit_test_nodes(nodes: &[Node], x: f32, y: f32, avail: (f32, f32)) -> bool {
    nodes
        .iter()
        .any(|node| hit_test_node(node, (0.0, 0.0), x, y, avail).is_some())
}

/// 返回命中节点的链（子→根，对应 Kotlin hitTest 的 [NodeWithPosition] 链）。
///
/// 每项为 `(Node, 绝对坐标)`，其中绝对坐标是该节点在根坐标系下的原点
/// （Kotlin `NodeWithPosition.position`），供分发时换算局部坐标。
///
/// 布局盒必须与 [`draw_node`] 完全一致（同一份 [`wy_mve::layout_boxes`]），
/// 否则会出现"看得见但点不中"。
fn hit_test_node(
    node: &Node,
    origin: (f32, f32),
    x: f32,
    y: f32,
    avail: (f32, f32),
) -> Option<Vec<(Node, (f32, f32))>> {
    if node.is_hidden() {
        return None;
    }
    let pos = (origin.0 + node.pos_x(), origin.1 + node.pos_y());
    let (lx, ly) = (x - pos.0, y - pos.1);
    // 裁剪容器：盒外内容不可见也不可点（对应 Kotlin ScrollContent.acceptClip）。
    // 溢出的子节点若不在此拦截，会在自身布局盒内被命中——点视口下方"看不见"
    // 的内容却能触发回调，正是滚动容器最容易出的事故。
    if node.clip && !in_layout_box(node, lx, ly, avail) {
        return None;
    }
    let children = children_nodes(node);
    let boxes = if node.layout.is_some() {
        wy_mve::layout_boxes(node, avail, &children)
    } else {
        vec![wy_mve::LayoutBox::ZERO; children.len()]
    };
    // 子节点优先：倒序（后绘制的在上面）
    for (child, b) in children.iter().zip(boxes.iter()).rev() {
        let child_origin = (pos.0 + b.x, pos.1 + b.y);
        let child_avail = if node.layout.is_some() {
            (b.width, b.height)
        } else {
            wy_mve::node_size(child)
        };
        if let Some(mut chain) = hit_test_node(child, child_origin, x, y, child_avail) {
            chain.push((node.clone(), pos));
            return Some(chain);
        }
    }
    // 自身：必须有 handler、点在自身的布局盒内，且通过自定义命中测试才算
    if has_handler(node) && in_layout_box(node, lx, ly, avail) && node.run_hit_test(lx, ly) {
        return Some(vec![(node.clone(), pos)]);
    }
    None
}

/// 点是否落在节点的布局盒内。
///
/// 没有这一层，默认 `hit_test_fn`（恒真）会让任意坐标"命中"远处节点——
/// 点顶栏却触发右侧盘口某一档，正是缺了边界判断。
///
/// 盒尺寸优先取布局分配值，回落节点自身声明；两者都拿不到（0）时**不可命中**，
/// 而不是放行：放行会让"宽度塌成 0"的坏布局静默变成"到处都能点"。
fn in_layout_box(node: &Node, x: f32, y: f32, avail: (f32, f32)) -> bool {
    let (w, h) = if avail.0 > 0.0 && avail.1 > 0.0 {
        avail
    } else {
        (node.effective_width(), node.effective_height())
    };
    if w <= 0.0 || h <= 0.0 {
        return false;
    }
    x >= 0.0 && y >= 0.0 && x <= w && y <= h
}

/// 沿命中链冒泡分发指针事件（叶→根，对应 Kotlin 冒泡阶段，先 onDown 后 onClick）。
///
/// 每节点用**局部坐标**（命中链携带的绝对位置换算），复刻 Kotlin
/// `dispatchClick`/`dispatchPointerEvent`：先 `runOnDown` 再 `runOnClick`，
/// 任一阶段 stopPropagation 即终止。
#[cfg(test)]
fn dispatch_click_nodes(
    nodes: &[Node],
    x: f32,
    y: f32,
    avail: (f32, f32),
    event: &mut wy_mve::PointerEvent,
) {
    if let Some(chain) = nodes
        .iter()
        .rev()
        .find_map(|node| hit_test_node(node, (0.0, 0.0), x, y, avail))
    {
        dispatch_click_chain(&chain, event);
    }
}

/// 沿给定命中链分发 down/click 两阶段 × 捕获/冒泡两相位（链为 叶→根）。
///
/// 复刻 Kotlin `dispatchPointer`：Kotlin 对 Down 与 Click 各做一次
/// dispatch（各自 capture+bubble）；本 port 无 `method` 字段，阶段由
/// **调用哪个 handler** 表达（runOnDown vs runOnClick），故四轮顺序为
/// down_capture（根→叶）→ click_capture（根→叶）→ down（叶→根）→
/// click（叶→根）。任一相位 `stopPropagation` 即止。
fn dispatch_click_chain(chain: &[(Node, (f32, f32))], event: &mut wy_mve::PointerEvent) {
    // 捕获相位：链逆序 = 根→叶（外层先于内层看到事件）
    for (node, pos) in chain.iter().rev() {
        event.x = event.root_x - pos.0;
        event.y = event.root_y - pos.1;
        node.run_on_down_capture(event);
        if event.stopped {
            return;
        }
    }
    for (node, pos) in chain.iter().rev() {
        event.x = event.root_x - pos.0;
        event.y = event.root_y - pos.1;
        node.run_on_click_capture(event);
        if event.stopped {
            return;
        }
    }
    // 冒泡相位：叶→根（内层先于外层）
    for (node, pos) in chain {
        event.x = event.root_x - pos.0;
        event.y = event.root_y - pos.1;
        node.run_on_down(event);
        if event.stopped {
            return;
        }
    }
    for (node, pos) in chain {
        event.x = event.root_x - pos.0;
        event.y = event.root_y - pos.1;
        node.run_on_click(event);
        if event.stopped {
            return;
        }
    }
}

/// 按 `identity` 在整棵树里定位节点（hover 派发用：命中链是嵌套的，不能只看顶层）。
fn find_identity(nodes: &[Node], id: &Rc<()>) -> Option<Node> {
    for node in nodes {
        if Rc::ptr_eq(&node.identity, id) {
            return Some(node.clone());
        }
        if let Some(found) = find_identity(&children_nodes(node), id) {
            return Some(found);
        }
    }
    None
}

/// 沿捕获链分发指针移动（叶→根；拖动扩展选区）。
///
/// 双相位：capture（根→叶）先走，再冒泡（叶→根）——复刻 Kotlin
/// `onPointerMoveCapture` / `onPointerMove`。
fn dispatch_move_chain(chain: &[(Node, (f32, f32))], x: f32, y: f32, shift: bool) {
    let mut event = wy_mve::PointerEvent::new(x, y);
    event.shift = shift;
    for (node, pos) in chain.iter().rev() {
        event.x = x - pos.0;
        event.y = y - pos.1;
        node.run_on_move_capture(&mut event);
        if event.stopped {
            return;
        }
    }
    for (node, pos) in chain {
        event.x = x - pos.0;
        event.y = y - pos.1;
        node.run_on_move(&mut event);
        if event.stopped {
            return;
        }
    }
}

/// 沿捕获链分发指针释放（叶→根；结束拖动会话）。
///
/// 双相位：capture（根→叶）先走，再冒泡（叶→根）。
fn dispatch_up_chain(chain: &[(Node, (f32, f32))], x: f32, y: f32, shift: bool) {
    let mut event = wy_mve::PointerEvent::new(x, y);
    event.shift = shift;
    for (node, pos) in chain.iter().rev() {
        event.x = x - pos.0;
        event.y = y - pos.1;
        node.run_on_up_capture(&mut event);
        if event.stopped {
            return;
        }
    }
    for (node, pos) in chain {
        event.x = x - pos.0;
        event.y = y - pos.1;
        node.run_on_up(&mut event);
        if event.stopped {
            return;
        }
    }
}

/// 沿悬停链分发指针移动（叶→根；用于十字线/悬停读数）。
///
/// 与 [`dispatch_move_chain`] 的区别：那条是"按下期间"（拖选/拖拽），
/// 这条只在未按下时派发，且走 `run_hover_move`（语义独立的回调）。
///
/// 单相位：Kotlin 无 HoverCapture 对应物（其 Capture 变体只覆盖
/// Click/Down/Up/Move/Wheel/DoubleClick/ContextMenu），悬停移动不设
/// 捕获相位。
fn dispatch_hover_move_chain(chain: &[(Node, (f32, f32))], x: f32, y: f32) {
    let mut event = wy_mve::PointerEvent::new(x, y);
    for (node, pos) in chain {
        event.x = x - pos.0;
        event.y = y - pos.1;
        node.run_hover_move(&mut event);
        if event.stopped {
            return;
        }
    }
}

/// 沿命中链分发滚轮（叶→根；任一层 `stopPropagation` 即止）。
///
/// 与点击链同构：滚轮也应先给最深层（图表、盘口列表），不被消费时才冒泡给
/// 外层容器（整页滚动/缩放）。
fn dispatch_wheel_chain(chain: &[(Node, (f32, f32))], x: f32, y: f32, wheel: f32) -> bool {
    let mut event = wy_mve::PointerEvent::new(x, y);
    event.wheel = wheel;
    // 捕获相位（根→叶）：外层可先于图表/列表看到滚轮
    for (node, pos) in chain.iter().rev() {
        event.x = x - pos.0;
        event.y = y - pos.1;
        node.run_wheel_capture(&mut event);
        if event.stopped {
            return true;
        }
    }
    // 冒泡相位（叶→根）：最深层（图表、盘口列表）先消费
    for (node, pos) in chain {
        event.x = x - pos.0;
        event.y = y - pos.1;
        node.run_wheel(&mut event);
        if event.stopped {
            return true;
        }
    }
    false
}

/// 沿命中链分发双击（叶→根）。判定参数见 [`DOUBLE_CLICK_INTERVAL_MS`]。
///
/// 双相位：capture（根→叶）先走，再冒泡（叶→根）。
fn dispatch_double_click_chain(chain: &[(Node, (f32, f32))], event: &mut wy_mve::PointerEvent) {
    for (node, pos) in chain.iter().rev() {
        event.x = event.root_x - pos.0;
        event.y = event.root_y - pos.1;
        node.run_on_double_click_capture(event);
        if event.stopped {
            return;
        }
    }
    for (node, pos) in chain {
        event.x = event.root_x - pos.0;
        event.y = event.root_y - pos.1;
        node.run_on_double_click(event);
        if event.stopped {
            return;
        }
    }
}

/// 沿命中链分发右键菜单（叶→根；右键不走 Down/Click）。
///
/// 双相位：capture（根→叶）先走，再冒泡（叶→根）。
fn dispatch_context_menu_chain(chain: &[(Node, (f32, f32))], event: &mut wy_mve::PointerEvent) {
    for (node, pos) in chain.iter().rev() {
        event.x = event.root_x - pos.0;
        event.y = event.root_y - pos.1;
        node.run_on_context_menu_capture(event);
        if event.stopped {
            return;
        }
    }
    for (node, pos) in chain {
        event.x = event.root_x - pos.0;
        event.y = event.root_y - pos.1;
        node.run_on_context_menu(event);
        if event.stopped {
            return;
        }
    }
}

/// 焦点链未消费时的兜底广播：按文档序（根→叶）把按键派给树内声明了
/// `key_fn` 的节点，任一消费即止。
///
/// 对应 Kotlin `engineGlobal.registerKeyPress` 的「全局键」语义：浮层的
/// Esc 关闭不依赖焦点位置——即使焦点在空白处或还没点进浮层，Esc 也要生效。
fn dispatch_key_broadcast(nodes: &[Node], event: &mut wy_mve::KeyEvent) -> bool {
    for node in nodes {
        if node.run_key(event) {
            return true;
        }
        let children = children_nodes(node);
        if !children.is_empty() && dispatch_key_broadcast(&children, event) {
            return true;
        }
    }
    false
}

/// MVE 应用：连接 MVE Node 树与渲染引擎。
///
/// 树结构只构建一次；每帧 `draw()` 读取 `Root.target` memo 得到当前节点列表，
/// 信号变化自动触发重绘（`RedrawTracker`）。复刻 Kotlin Renderer 的管线模型。
pub struct MveApp {
    root: wy_mve::Root,
    cursor: UnsafeCell<(f32, f32)>,
    /// 焦点管理（identity 链定位，复刻 Kotlin Renderer）。
    focus: FocusManager,
    /// 按下捕获链（叶→根，携带绝对原点）：按下建立，move 沿链派发，松开清空。
    capture: Option<Vec<(Node, (f32, f32))>>,
    /// 上一次点击（时间 + 落点）：双击候选。判定复刻 Kotlin
    /// `DoubleClickDetector`（400ms / 5px，构成双击即消费候选）。
    last_click: Option<(Instant, f32, f32)>,
    /// 当前修饰键（winit ModifiersChanged 同步；按下时填入 PointerEvent.shift）。
    modifiers: winit::keyboard::ModifiersState,
    /// 当前视口逻辑尺寸：flex 根容器的主轴可用空间（`draw`/`on_resize` 更新）。
    ///
    /// **必须是信号**：绘制侧（`RedrawTracker`）用 `RecordMemo` 缓存整帧 Scene，
    /// 只在「被追踪的信号依赖」变化时重录。若视口只是普通 `Cell`，
    /// 窗口缩放后虽然 `request_redraw()` 被调用，绘制闭包仍会被短路、
    /// 复用旧 Scene——布局永远停在旧尺寸（"窗口一放大布局不跟随"的真因）。
    /// `draw` 里读取本信号即把它注册为绘制依赖，尺寸一变就重录重排。
    viewport: Signal<(f32, f32)>,
    /// 当前悬停链的节点身份（叶→根），用于 `CursorMoved` 时比对进出。
    hover: Vec<Rc<()>>,
}

impl MveApp {
    /// 记录一次点击，返回是否构成双击。
    ///
    /// 复刻 Kotlin `DoubleClickDetector.onClick`：间隔
    /// [`DOUBLE_CLICK_INTERVAL_MS`] 内 + 落点 [`DOUBLE_CLICK_DIST`] 内。
    /// 构成双击即**消费候选**（连点序列中双击只触发一次——三连击
    /// 只触发一次双击，与 Kotlin `pending = !isDouble` 一致）。
    fn note_click(&mut self, x: f32, y: f32) -> bool {
        let now = Instant::now();
        let is_double = match self.last_click {
            Some((t, lx, ly)) => {
                now.duration_since(t).as_millis() <= DOUBLE_CLICK_INTERVAL_MS
                    && (x - lx).abs() <= DOUBLE_CLICK_DIST
                    && (y - ly).abs() <= DOUBLE_CLICK_DIST
            }
            None => false,
        };
        self.last_click = if is_double { None } else { Some((now, x, y)) };
        is_double
    }

    /// 创建 MVE 应用：`callback` 构建根订阅树（只执行一次）。
    ///
    /// callback 内可用 `cx.child(...)` 添加顶层节点、`cx.render_for_each(...)`
    /// 添加动态列表区域；其中读取的信号会被 memo 自动追踪。
    pub fn new(callback: impl Fn(&mut NodeContext) + 'static) -> Self {
        let root = render_root(callback);
        Self {
            root,
            cursor: UnsafeCell::new((0.0, 0.0)),
            focus: FocusManager::default(),
            capture: None,
            last_click: None,
            modifiers: winit::keyboard::ModifiersState::empty(),
            viewport: Signal::new((0.0, 0.0)),
            hover: Vec::new(),
        }
    }

    /// 当前视口逻辑尺寸（最近一次 `draw`/`on_resize` 得到的值）。
    pub fn viewport(&self) -> (f32, f32) {
        self.viewport.get()
    }
    /// 根订阅树（节点列表 + 目标 memo）。
    ///
    /// 给宿主做**自检**用：拿到 `root.nodes()` 之后配合
    /// [`wy_mve::layout_boxes`] 就能在不开窗的情况下复核每个面板的实际盒，
    /// 相当于浏览器里的 `getBoundingClientRect()` 全量快照。
    pub fn root(&self) -> &wy_mve::Root {
        &self.root
    }

    /// 当前指针位置下的命中链（叶→根，携带绝对原点）；未命中为 `None`。
    fn hover_chain(&self, x: f32, y: f32) -> Option<Vec<(Node, (f32, f32))>> {
        let nodes = self.root.nodes();
        let avail = self.viewport.get();
        // 顶层**倒序**命中：后声明的在上（浮层/弹窗盖住主界面时优先命中浮层）
        nodes
            .iter()
            .rev()
            .find_map(|node| hit_test_node(node, (0.0, 0.0), x, y, avail))
    }

    /// 指针移动后重算悬停链，对进出节点派发 `on_hover`（不冒泡，只派给声明了的节点）。
    ///
    /// 对应浏览器 `pointerenter` / `pointerleave`：同一节点只在状态真正翻转时收到回调，
    /// 沿链移动（子→父）不会重复触发父节点。
    ///
    /// 返回本次的命中链，供调用方继续派发 [`dispatch_hover_move_chain`]
    /// （十字线这类"指针在哪"的读数需要每次移动都更新，不只是进出）。
    fn update_hover(&mut self, x: f32, y: f32) -> Option<Vec<(Node, (f32, f32))>> {
        let nodes = self.root.nodes();
        let chain = self.hover_chain(x, y);
        let next: Vec<Rc<()>> = chain
            .as_ref()
            .map(|chain| chain.iter().map(|(n, _)| Rc::clone(&n.identity)).collect())
            .unwrap_or_default();

        let same = next.len() == self.hover.len()
            && next
                .iter()
                .zip(self.hover.iter())
                .all(|(a, b)| Rc::ptr_eq(a, b));
        if same {
            return chain;
        }

        // 离开：旧链里不在新链的节点（叶→根顺序派发）
        for id in &self.hover {
            if !next.iter().any(|n| Rc::ptr_eq(n, id)) {
                if let Some(n) = find_identity(&nodes, id) {
                    n.run_hover(false);
                }
            }
        }
        // 进入：新链里不在旧链的节点（根→叶顺序更自然）
        for id in next.iter().rev() {
            if !self.hover.iter().any(|n| Rc::ptr_eq(n, id)) {
                if let Some(n) = find_identity(&nodes, id) {
                    n.run_hover(true);
                }
            }
        }
        self.hover = next;
        chain
    }

    /// 当前焦点路径（叶子→根）；空 = 无焦点。
    pub fn focus_path(&self) -> &FocusPath {
        self.focus.path()
    }

    /// 设置焦点并把得失变化派发给节点（驱动聚焦视觉）。
    ///
    /// 比对旧/新焦点**叶子身份**（`Rc::ptr_eq`——`FocusPath` 的 `==` 是值比较，
    /// 恒真不可用）：变化时先对旧叶子 `run_blur`、再对新叶子 `run_focus`。
    fn apply_focus(&mut self, new_path: FocusPath) {
        let nodes = self.root.nodes();
        let old_leaf = locate(self.focus.path(), &nodes).and_then(|c| c.into_iter().next());
        let new_leaf = locate(&new_path, &nodes).and_then(|c| c.into_iter().next());
        let unchanged = match (&old_leaf, &new_leaf) {
            (Some(a), Some(b)) => Rc::ptr_eq(&a.identity, &b.identity),
            (None, None) => true,
            _ => false,
        };
        self.focus.set_focus(new_path);
        if unchanged {
            return;
        }
        if let Some(n) = old_leaf {
            n.run_blur();
        }
        if let Some(n) = new_leaf {
            n.run_focus();
        }
    }
}

impl crate::runner::WyApp for MveApp {
    fn draw(&self, scene: &mut Scene, width: f32, height: f32) {
        // 等价于 Kotlin: signal.collect { didDraw().draw(canvas, 0f, 0f) }
        // Root.target 读取 → RedrawTracker 追踪 → 信号变化触发重绘
        //
        // **视口尺寸必须作为「被读取的依赖」进入追踪上下文**：绘制侧用
        // `RecordMemo` 缓存整帧 Scene，只在依赖变化时重录。若这里只写不读
        // （或视口压根不是信号），窗口缩放后重绘请求会被短路、复用旧 Scene，
        // 布局就永远停在旧尺寸（"窗口一放大布局不跟随"的真因）。
        //
        // 写入放在 `on_resize`（事件期），绘制期**只读**——避免绘制闭包
        // 写信号触发自依赖重算。首帧若 `on_resize` 尚未跑过，用传入的
        // 宽高兜底（并写回，让随后的命中测试也能拿到正确的视口尺寸）。
        let tracked = self.viewport.get();
        let avail = if width > 0.0 && height > 0.0 && tracked != (width, height) {
            // 尺寸与记录的不一致（首帧或 resize 后未走 on_resize）：
            // 以本帧尺寸为准并写回——命中测试依赖它算布局盒。
            self.viewport.set((width, height));
            (width, height)
        } else if tracked == (0.0, 0.0) {
            self.viewport.set((width, height));
            (width, height)
        } else {
            tracked
        };
        let nodes = self.root.nodes();
        draw_nodes(&nodes, scene, avail);
    }

    fn widget_tree(&mut self) -> Option<&mut wy_render::widget_tree::WidgetTree> {
        // 不使用 WidgetTree：直接遍历 Node 树绘制和处理事件
        None
    }

    fn on_resize(&mut self, width: f32, height: f32) {
        if width > 0.0 && height > 0.0 {
            // 写视口信号：它已被绘制 memo 追踪为依赖，尺寸一变即触发
            // 重录 → flex 重排 → 布局跟随窗口。
            self.viewport.set((width, height));
        }
    }

    fn handle_event(&mut self, event: &winit::event::WindowEvent) -> bool {
        use winit::event::{ElementState, MouseButton, WindowEvent};

        // SAFETY: handle_event 在 runner 的事件循环中调用，
        // runner 拥有 self.app 的独占访问权。
        let _ = &self.cursor;

        match event {
            WindowEvent::ModifiersChanged(mods) => {
                self.modifiers = mods.state();
                false
            }
            WindowEvent::CursorMoved { position, .. } => {
                let (x, y) = (position.x as f32, position.y as f32);
                unsafe { *self.cursor.get() = (x, y) };
                // 悬停进出（组件自己的 hover 视觉，替代"全窗手算坐标"）
                let chain = self.update_hover(x, y);
                // 按下捕获期间：沿捕获链派发移动（编辑器拖选、分割条拖拽）
                if let Some(chain) = &self.capture {
                    let shift = self.modifiers.shift_key();
                    dispatch_move_chain(chain, x, y, shift);
                } else if let Some(chain) = &chain {
                    // 未按下：沿悬停链派发"悬停移动"（十字线、tooltip 读数）
                    dispatch_hover_move_chain(chain, x, y);
                }
                false
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let (x, y) = unsafe { *self.cursor.get() };
                let wheel = match delta {
                    winit::event::MouseScrollDelta::LineDelta(_, dy) => *dy,
                    winit::event::MouseScrollDelta::PixelDelta(pos) => pos.y as f32 / 50.0,
                };
                if wheel == 0.0 {
                    return false;
                }
                let Some(chain) = self.hover_chain(x, y) else {
                    return false;
                };
                dispatch_wheel_chain(&chain, x, y, wheel)
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } => {
                let (x, y) = unsafe { *self.cursor.get() };
                // 与上次点击候选落点过远 → 取消候选（复刻 Kotlin onPointerDown：
                // 用户转到别处，此前的单击不再可能与后续单击构成双击）
                if let Some((_, lx, ly)) = self.last_click {
                    if (x - lx).abs() > DOUBLE_CLICK_DIST || (y - ly).abs() > DOUBLE_CLICK_DIST {
                        self.last_click = None;
                    }
                }
                let nodes = self.root.nodes();
                let avail = self.viewport.get();
                // 顶层倒序（与 hover_chain 一致）：后声明的浮层优先命中
                let chain = nodes
                    .iter()
                    .rev()
                    .find_map(|node| hit_test_node(node, (0.0, 0.0), x, y, avail));
                let Some(chain) = chain else {
                    // 点击空白：清捕获（对齐浏览器语义）
                    self.capture = None;
                    return false;
                };
                // 存捕获链：move/up 沿链派发（即使移出节点也继续）
                self.capture = Some(chain.clone());
                let node_chain: Vec<Node> = chain.iter().map(|(n, _)| n.clone()).collect();
                let path = FocusManager::click_focus_path(&node_chain);
                self.apply_focus(path);
                let mut mve_event = wy_mve::PointerEvent::new(x, y);
                mve_event.shift = self.modifiers.shift_key();
                dispatch_click_chain(&chain, &mut mve_event);
                // 双击**不吞**单击：click 先派发，构成双击再补发
                // （复刻 Kotlin Renderer「先派发 Click，构成双击再补发
                // DoubleClick」的注释语义）
                if self.note_click(x, y) {
                    let mut dbl = wy_mve::PointerEvent::new(x, y);
                    dbl.shift = self.modifiers.shift_key();
                    dbl.button = wy_mve::PointerButton::Left;
                    dispatch_double_click_chain(&chain, &mut dbl);
                }
                mve_event.stopped
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Right,
                ..
            } => {
                // 右键只派发 ContextMenu（复刻 Kotlin：右键**不派发** Down/Click，
                // 避免误启文本拖选/滑块拖动）；也不建捕获、不动焦点。
                let (x, y) = unsafe { *self.cursor.get() };
                let nodes = self.root.nodes();
                let avail = self.viewport.get();
                let chain = nodes
                    .iter()
                    .rev()
                    .find_map(|node| hit_test_node(node, (0.0, 0.0), x, y, avail));
                let Some(chain) = chain else {
                    return false;
                };
                let mut ev = wy_mve::PointerEvent::new(x, y);
                ev.button = wy_mve::PointerButton::Right;
                dispatch_context_menu_chain(&chain, &mut ev);
                ev.stopped
            }
            WindowEvent::MouseInput {
                state: ElementState::Released,
                button: MouseButton::Left,
                ..
            } => {
                let (x, y) = unsafe { *self.cursor.get() };
                if let Some(chain) = self.capture.take() {
                    let shift = self.modifiers.shift_key();
                    dispatch_up_chain(&chain, x, y, shift);
                }
                false
            }
            _ => false,
        }
    }

    /// 处理键盘事件：Tab 触发焦点遍历，其余按键沿焦点链冒泡。
    fn handle_key_event(&mut self, event: &crate::event::KeyEvent) {
        let nodes = self.root.nodes();
        let forward = !event.shift;
        match event.key {
            crate::event::Key::Tab if event.pressed => {
                // Tab / Shift+Tab 焦点遍历
                let new_focus = tab_navigate(&nodes, self.focus.path(), forward, true);
                self.apply_focus(new_focus);
            }
            _ if event.pressed => {
                let mut mve_event = wy_mve::KeyEvent {
                    key: to_mve_key(&event.key),
                    ctrl: event.ctrl,
                    shift: event.shift,
                    alt: event.alt,
                    meta: event.meta,
                };
                // 先沿焦点链冒泡；未消费再全局广播（浮层 Esc 关闭等全局键）
                if !self.focus.dispatch_key(&nodes, &mut mve_event) {
                    dispatch_key_broadcast(&nodes, &mut mve_event);
                }
            }
            _ => {}
        }
    }

    /// 处理 IME 输入法组合事件（沿焦点链分发）。
    fn handle_ime_event(&mut self, event: &wy_mve::ImeEvent) {
        let nodes = self.root.nodes();
        let mut mve_event = event.clone();
        self.focus.dispatch_ime(&nodes, &mut mve_event);
    }

    /// IME 允许状态：焦点在可输入节点（含 `ime_fn`）→ 允许，否则禁止；
    /// 无焦点 → 不干预。
    fn ime_allowed(&self) -> Option<bool> {
        if self.focus.path().is_empty() {
            return None;
        }
        let nodes = self.root.nodes();
        locate(self.focus.path(), &nodes)
            .map(|chain| chain.first().is_some_and(|n| n.ime_fn.is_some()))
    }

    /// IME 候选窗光标盒：焦点节点的局部光标盒 + 焦点链布局偏移累加
    /// （对齐 Kotlin `EditableTextNode.inputOverlay` = 绝对原点 + padding + 字形盒）。
    fn ime_cursor_area(&self) -> Option<(f32, f32, f32, f32)> {
        let path = self.focus.path();
        if path.is_empty() {
            return None;
        }
        let nodes = self.root.nodes();
        // locate 返回 叶→根 链，反转成 根→叶 后逐级累加自身 pos 与父布局的子偏移。
        let mut chain = locate(path, &nodes)?;
        chain.reverse();
        let mut origin = (0.0f32, 0.0f32);
        let mut avail = self.viewport.get();
        for i in 0..chain.len() {
            origin.0 += chain[i].pos_x();
            origin.1 += chain[i].pos_y();
            if let Some(child) = chain.get(i + 1) {
                let children = children_nodes(&chain[i]);
                let idx = children.iter().position(|c| c == child)?;
                let boxes = if chain[i].layout.is_some() {
                    wy_mve::layout_boxes(&chain[i], avail, &children)
                } else {
                    vec![wy_mve::LayoutBox::ZERO; children.len()]
                };
                let b = boxes.get(idx)?;
                origin.0 += b.x;
                origin.1 += b.y;
                avail = if chain[i].layout.is_some() {
                    (b.width, b.height)
                } else {
                    wy_mve::node_size(child)
                };
            }
        }
        let (x, y, w, h) = chain.last()?.run_ime_cursor_rect()?;
        Some((origin.0 + x, origin.1 + y, w, h))
    }
}

/// wy-render `Key` → wy-mve `Key`（两者变体同构）。
fn to_mve_key(key: &crate::event::Key) -> wy_mve::Key {
    use crate::event::Key as RK;
    use wy_mve::Key as MK;
    match key {
        RK::Char(c) => MK::Char(*c),
        RK::Enter => MK::Enter,
        RK::Backspace => MK::Backspace,
        RK::Delete => MK::Delete,
        RK::ArrowUp => MK::ArrowUp,
        RK::ArrowDown => MK::ArrowDown,
        RK::ArrowLeft => MK::ArrowLeft,
        RK::ArrowRight => MK::ArrowRight,
        RK::Tab => MK::Tab,
        RK::Escape => MK::Escape,
        RK::Home => MK::Home,
        RK::End => MK::End,
        RK::PageUp => MK::PageUp,
        RK::PageDown => MK::PageDown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner::WyApp;
    use std::cell::RefCell;
    use std::rc::Rc;
    use wy_mve::{button, column, node_size, row, text, PointerEvent};
    use wy_signal::{GetValue, SetValue, Signal};

    /// 构建 counter 结构：column > (text + row(button-, button+))
    fn counter_app(count: Signal<i32>) -> MveApp {
        MveApp::new(move |cx| {
            let column_count = count;
            cx.child(column(move |cx| {
                let c = column_count;
                cx.child(text(move || format!("Count: {}", c.get())));

                let row_count = column_count;
                cx.child(row(move |cx| {
                    cx.child(button(move || "−".into(), {
                        let s = row_count;
                        move || s.set(s.get() - 1)
                    }));
                    cx.child(button(move || "+".into(), {
                        let s = row_count;
                        move || s.set(s.get() + 1)
                    }));
                }));
            }));
        })
    }

    #[test]
    fn draw_builds_tree_once() {
        let count = Signal::new(0);
        let app = counter_app(count);

        let mut scene = Scene::new();
        app.draw(&mut scene, 100.0, 100.0);

        // 信号变化后再次 draw：树只用 memo 重算，不重建
        count.set(10);
        app.draw(&mut scene, 100.0, 100.0);
    }

    #[test]
    fn node_equality_by_identity() {
        // 同一 Node 克隆相等；不同实例不等
        let n = Node::default();
        assert_eq!(n, n.clone());
        assert_ne!(n, Node::default());
    }

    #[test]
    fn press_capture_dispatches_move_then_up_once() {
        // 按下捕获：move/up 沿命中链派发；松开后捕获清空（后续 move 不派发）。
        let log: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
        let app = MveApp::new({
            let log = Rc::clone(&log);
            move |cx| {
                let l_down = Rc::clone(&log);
                let l_move = Rc::clone(&log);
                let l_up = Rc::clone(&log);
                cx.child(Node {
                    width: 100.0,
                    height: 100.0,
                    hit_test_fn: Rc::new(|x, y| {
                        (0.0..100.0).contains(&x) && (0.0..100.0).contains(&y)
                    }),
                    on_down_fn: Some(Rc::new(move |ev| {
                        l_down
                            .borrow_mut()
                            .push(format!("down:{:.0},{:.0}", ev.root_x, ev.root_y));
                    })),
                    on_move_fn: Some(Rc::new(move |ev| {
                        l_move.borrow_mut().push(format!("move:{:.0}", ev.root_x));
                    })),
                    on_up_fn: Some(Rc::new(move |_ev| {
                        l_up.borrow_mut().push("up".to_string());
                    })),
                    ..Node::default()
                });
            }
        });

        let mut scene = Scene::new();
        app.draw(&mut scene, 200.0, 200.0);

        let mut app = app;
        let cursor = |x: f64, y: f64| winit::event::WindowEvent::CursorMoved {
            device_id: winit::event::DeviceId::dummy(),
            position: winit::dpi::PhysicalPosition::new(x, y),
        };
        let mouse = |state: winit::event::ElementState| winit::event::WindowEvent::MouseInput {
            device_id: winit::event::DeviceId::dummy(),
            state,
            button: winit::event::MouseButton::Left,
        };

        app.handle_event(&cursor(50.0, 50.0)); // 只更新光标（未按下，无捕获）
        app.handle_event(&mouse(winit::event::ElementState::Pressed));
        app.handle_event(&cursor(70.0, 50.0)); // 捕获期间 → move 派发
        app.handle_event(&mouse(winit::event::ElementState::Released));
        app.handle_event(&cursor(90.0, 50.0)); // 已释放 → 不再派发

        assert_eq!(
            *log.borrow(),
            vec![
                "down:50,50".to_string(),
                "move:70".to_string(),
                "up".to_string()
            ]
        );
    }

    #[test]
    fn double_click_fires_once_per_pair() {
        // 双击**不吞**单击（复刻 Kotlin）：click 先派发，构成双击再补发
        // DoubleClick；构成双击即消费候选——三连击只触发一次双击。
        let log = Rc::new(RefCell::new(Vec::<String>::new()));
        let l_click = Rc::clone(&log);
        let l_dbl = Rc::clone(&log);
        let mut app = MveApp::new(move |cx| {
            let l_click = Rc::clone(&l_click);
            let l_dbl = Rc::clone(&l_dbl);
            cx.child(Node {
                width: 100.0,
                height: 100.0,
                hit_test_fn: Rc::new(|_, _| true),
                on_click_fn: Some(Rc::new(move |_ev| {
                    l_click.borrow_mut().push("click".to_string());
                })),
                on_double_click_fn: Some(Rc::new(move |_ev| {
                    l_dbl.borrow_mut().push("dbl".to_string());
                })),
                ..Node::default()
            });
        });
        let mut scene = Scene::new();
        app.draw(&mut scene, 200.0, 200.0);

        let cursor = |x: f64, y: f64| winit::event::WindowEvent::CursorMoved {
            device_id: winit::event::DeviceId::dummy(),
            position: winit::dpi::PhysicalPosition::new(x, y),
        };
        let mouse = |state: winit::event::ElementState| winit::event::WindowEvent::MouseInput {
            device_id: winit::event::DeviceId::dummy(),
            state,
            button: winit::event::MouseButton::Left,
        };
        let pressed = winit::event::ElementState::Pressed;
        let released = winit::event::ElementState::Released;

        app.handle_event(&cursor(50.0, 50.0));
        app.handle_event(&mouse(pressed)); // 第 1 次：click + 记候选
        app.handle_event(&mouse(released));
        app.handle_event(&mouse(pressed)); // 第 2 次（同点同时隔内）：click + dbl，候选消费
        app.handle_event(&mouse(released));
        app.handle_event(&mouse(pressed)); // 第 3 次：只 click，不再触发双击
        app.handle_event(&mouse(released));

        assert_eq!(
            *log.borrow(),
            vec![
                "click".to_string(),
                "click".to_string(),
                "dbl".to_string(),
                "click".to_string(),
            ]
        );
    }

    #[test]
    fn far_apart_clicks_do_not_form_double_click() {
        // 落点超过 5px 不构成双击（对齐 Kotlin distancePx）
        let log = Rc::new(RefCell::new(Vec::<String>::new()));
        let l_dbl = Rc::clone(&log);
        let mut app = MveApp::new(move |cx| {
            let l_dbl = Rc::clone(&l_dbl);
            cx.child(Node {
                width: 100.0,
                height: 100.0,
                hit_test_fn: Rc::new(|_, _| true),
                on_double_click_fn: Some(Rc::new(move |_ev| {
                    l_dbl.borrow_mut().push("dbl".to_string());
                })),
                ..Node::default()
            });
        });
        let mut scene = Scene::new();
        app.draw(&mut scene, 200.0, 200.0);

        let cursor = |x: f64, y: f64| winit::event::WindowEvent::CursorMoved {
            device_id: winit::event::DeviceId::dummy(),
            position: winit::dpi::PhysicalPosition::new(x, y),
        };
        let mouse = |state: winit::event::ElementState| winit::event::WindowEvent::MouseInput {
            device_id: winit::event::DeviceId::dummy(),
            state,
            button: winit::event::MouseButton::Left,
        };
        let pressed = winit::event::ElementState::Pressed;
        let released = winit::event::ElementState::Released;

        app.handle_event(&cursor(50.0, 50.0));
        app.handle_event(&mouse(pressed));
        app.handle_event(&mouse(released));
        app.handle_event(&cursor(90.0, 50.0)); // 远距离第二次点击
        app.handle_event(&mouse(pressed));
        app.handle_event(&mouse(released));

        assert!(log.borrow().is_empty(), "远距离连点不应触发双击");
    }

    #[test]
    fn right_click_fires_context_menu_not_click() {
        // 右键只派发 ContextMenu，**不派发** Down/Click（复刻 Kotlin：
        // 避免误启文本拖选/滑块拖动）；左键路径不受影响。
        let log = Rc::new(RefCell::new(Vec::<String>::new()));
        let l_click = Rc::clone(&log);
        let l_down = Rc::clone(&log);
        let l_menu = Rc::clone(&log);
        let mut app = MveApp::new(move |cx| {
            let l_click = Rc::clone(&l_click);
            let l_down = Rc::clone(&l_down);
            let l_menu = Rc::clone(&l_menu);
            cx.child(Node {
                width: 100.0,
                height: 100.0,
                hit_test_fn: Rc::new(|_, _| true),
                on_click_fn: Some(Rc::new(move |_ev| {
                    l_click.borrow_mut().push("click".to_string());
                })),
                on_down_fn: Some(Rc::new(move |_ev| {
                    l_down.borrow_mut().push("down".to_string());
                })),
                on_context_menu_fn: Some(Rc::new(move |_ev| {
                    l_menu.borrow_mut().push("menu".to_string());
                })),
                ..Node::default()
            });
        });
        let mut scene = Scene::new();
        app.draw(&mut scene, 200.0, 200.0);

        let cursor = |x: f64, y: f64| winit::event::WindowEvent::CursorMoved {
            device_id: winit::event::DeviceId::dummy(),
            position: winit::dpi::PhysicalPosition::new(x, y),
        };
        let mouse_btn = |state: winit::event::ElementState, button: winit::event::MouseButton| {
            winit::event::WindowEvent::MouseInput {
                device_id: winit::event::DeviceId::dummy(),
                state,
                button,
            }
        };
        let pressed = winit::event::ElementState::Pressed;
        let released = winit::event::ElementState::Released;

        app.handle_event(&cursor(50.0, 50.0));
        app.handle_event(&mouse_btn(pressed, winit::event::MouseButton::Right));
        assert_eq!(*log.borrow(), vec!["menu".to_string()], "右键只触发菜单");

        // 左键照常：down 先、click 后（capture 正常建立/释放）
        app.handle_event(&mouse_btn(pressed, winit::event::MouseButton::Left));
        app.handle_event(&mouse_btn(released, winit::event::MouseButton::Left));
        assert_eq!(
            *log.borrow(),
            vec!["menu".to_string(), "down".to_string(), "click".to_string()],
            "左键路径不受右键影响"
        );
    }

    /// 构造「外层（铺满+命中）> 内层（叶子+命中）」两层链，
    /// 外层挂捕获回调、内层挂冒泡回调，返回 (app, log)。
    fn two_level_capture_app(
        outer_log: &str,
        inner_log: &str,
        outer_stop: bool,
    ) -> (MveApp, Rc<RefCell<Vec<String>>>) {
        let log = Rc::new(RefCell::new(Vec::<String>::new()));
        let l_outer = Rc::clone(&log);
        let l_inner = Rc::clone(&log);
        let outer_tag = outer_log.to_string();
        let inner_tag = inner_log.to_string();
        let app = MveApp::new(move |cx| {
            let l_outer = Rc::clone(&l_outer);
            let l_inner = Rc::clone(&l_inner);
            let outer_tag = outer_tag.clone();
            let inner_tag = inner_tag.clone();
            cx.child(Node {
                width: 200.0,
                height: 200.0,
                hit_test_fn: Rc::new(|_, _| true),
                on_click_capture_fn: Some(Rc::new(move |ev| {
                    l_outer.borrow_mut().push(outer_tag.clone());
                    if outer_stop {
                        ev.stop_propagation();
                    }
                })),
                arg_children_fn: Rc::new(move |cx| {
                    let l_inner = Rc::clone(&l_inner);
                    let inner_tag = inner_tag.clone();
                    cx.child(Node {
                        width: 100.0,
                        height: 100.0,
                        hit_test_fn: Rc::new(|_, _| true),
                        on_click_fn: Some(Rc::new(move |_ev| {
                            l_inner.borrow_mut().push(inner_tag.clone());
                        })),
                        ..Node::default()
                    });
                }),
                ..Node::default()
            });
        });
        (app, log)
    }

    #[test]
    fn capture_phase_runs_before_bubble() {
        // 复刻 Kotlin dispatchPointer：捕获（根→叶）先于冒泡（叶→根）——
        // 外层的捕获必须先于内层的冒泡触发。
        let (mut app, log) = two_level_capture_app("outer_capture", "inner_bubble", false);
        let mut scene = Scene::new();
        app.draw(&mut scene, 200.0, 200.0);

        let mouse = |state: winit::event::ElementState| winit::event::WindowEvent::MouseInput {
            device_id: winit::event::DeviceId::dummy(),
            state,
            button: winit::event::MouseButton::Left,
        };
        let cursor = winit::event::WindowEvent::CursorMoved {
            device_id: winit::event::DeviceId::dummy(),
            position: winit::dpi::PhysicalPosition::new(50.0, 50.0),
        };
        app.handle_event(&cursor);
        app.handle_event(&mouse(winit::event::ElementState::Pressed));
        app.handle_event(&mouse(winit::event::ElementState::Released));

        assert_eq!(
            *log.borrow(),
            vec!["outer_capture".to_string(), "inner_bubble".to_string()],
            "捕获（外层）必须先于冒泡（内层）"
        );
    }

    #[test]
    fn capture_stop_propagation_blocks_bubble() {
        // 捕获相位 stopPropagation → 冒泡整段不再派发（复刻 Kotlin
        // dispatchPointer 的 `if (e.stoppedProgression) return`）。
        let (mut app, log) = two_level_capture_app("outer_capture", "inner_bubble", true);
        let mut scene = Scene::new();
        app.draw(&mut scene, 200.0, 200.0);

        let mouse = |state: winit::event::ElementState| winit::event::WindowEvent::MouseInput {
            device_id: winit::event::DeviceId::dummy(),
            state,
            button: winit::event::MouseButton::Left,
        };
        let cursor = winit::event::WindowEvent::CursorMoved {
            device_id: winit::event::DeviceId::dummy(),
            position: winit::dpi::PhysicalPosition::new(50.0, 50.0),
        };
        app.handle_event(&cursor);
        app.handle_event(&mouse(winit::event::ElementState::Pressed));
        app.handle_event(&mouse(winit::event::ElementState::Released));

        assert_eq!(
            *log.borrow(),
            vec!["outer_capture".to_string()],
            "捕获拦截后内层冒泡不应收到事件"
        );
    }

    #[test]
    fn wheel_capture_runs_before_bubble() {
        // 滚轮同样双相位：外层 wheel_capture 先于内层 wheel
        let log = Rc::new(RefCell::new(Vec::<String>::new()));
        let l_outer = Rc::clone(&log);
        let l_inner = Rc::clone(&log);
        let mut app = MveApp::new(move |cx| {
            let l_outer = Rc::clone(&l_outer);
            let l_inner = Rc::clone(&l_inner);
            cx.child(Node {
                width: 200.0,
                height: 200.0,
                hit_test_fn: Rc::new(|_, _| true),
                on_wheel_capture_fn: Some(Rc::new(move |_ev| {
                    l_outer.borrow_mut().push("outer_capture".to_string());
                })),
                arg_children_fn: Rc::new(move |cx| {
                    let l_inner = Rc::clone(&l_inner);
                    cx.child(Node {
                        width: 100.0,
                        height: 100.0,
                        hit_test_fn: Rc::new(|_, _| true),
                        on_wheel_fn: Some(Rc::new(move |_ev| {
                            l_inner.borrow_mut().push("inner_bubble".to_string());
                        })),
                        ..Node::default()
                    });
                }),
                ..Node::default()
            });
        });
        let mut scene = Scene::new();
        app.draw(&mut scene, 200.0, 200.0);

        let cursor = winit::event::WindowEvent::CursorMoved {
            device_id: winit::event::DeviceId::dummy(),
            position: winit::dpi::PhysicalPosition::new(50.0, 50.0),
        };
        let wheel = winit::event::WindowEvent::MouseWheel {
            device_id: winit::event::DeviceId::dummy(),
            delta: winit::event::MouseScrollDelta::LineDelta(0.0, 1.0),
            phase: winit::event::TouchPhase::Moved,
        };
        app.handle_event(&cursor);
        app.handle_event(&wheel);

        assert_eq!(
            *log.borrow(),
            vec!["outer_capture".to_string(), "inner_bubble".to_string()],
            "滚轮捕获先于冒泡"
        );
    }

    #[test]
    fn layout_container_sizes_children() {
        let col = column(|cx| {
            cx.child(Node {
                width: 50.0,
                height: 20.0,
                hit_test_fn: Rc::new(|_, _| false),
                ..Node::default()
            });
            cx.child(Node {
                width: 50.0,
                height: 30.0,
                hit_test_fn: Rc::new(|_, _| false),
                ..Node::default()
            });
        });
        let (w, h) = node_size(&col);
        assert_eq!(w, 50.0);
        assert_eq!(h, 58.0); // 20 + 8 gap + 30

        let layout = col.layout.unwrap();
        let offsets = wy_mve::layout_offsets(layout, col.gap(), &children_nodes(&col));
        assert_eq!(offsets[0], (0.0, 0.0));
        assert_eq!(offsets[1], (0.0, 28.0));
    }

    #[test]
    fn counter_click_dispatches_to_button() {
        let count = Signal::new(0);
        let app = counter_app(count);

        let mut scene = Scene::new();
        app.draw(&mut scene, 100.0, 100.0);

        // column > text + row(button-, button+)
        // 坐标由布局实时计算（文本高度来自 Parley 测量），不硬编码
        let nodes = app.root.nodes();
        assert_eq!(nodes.len(), 1); // column

        let col = &nodes[0];
        let children = children_nodes(col);
        assert_eq!(children.len(), 2); // text + row
        let (_, text_h) = node_size(&children[0]);
        let row_y = text_h + 8.0; // column gap=8

        let row = &children[1];
        let row_children = children_nodes(row);
        let (btn_w, _) = node_size(&row_children[0]);
        let plus_x = btn_w + 8.0 + btn_w / 2.0; // button- 宽 + gap + button+ 半宽

        let click = (plus_x, row_y + 16.0); // 按钮垂直中心
        let chain = hit_test_node(col, (0.0, 0.0), click.0, click.1, (100.0, 100.0));
        assert!(chain.is_some());
        let chain = chain.unwrap();
        assert_eq!(chain.len(), 3); // +, row, column
        assert!(chain[0].0.on_click_fn.is_some(), "deepest is the + button");
        assert_eq!(chain[1].0.layout, Some(wy_mve::Layout::Row));
        assert_eq!(chain[1].0.gap(), 8.0);

        // 分发点击 → + 按钮 handler 执行并 stop
        let mut event = PointerEvent::new(click.0, click.1);
        dispatch_click_nodes(&nodes, click.0, click.1, (100.0, 100.0), &mut event);
        assert!(event.stopped);
        assert_eq!(count.get(), 1);
    }

    #[test]
    fn hit_test_misses_outside_column() {
        let count = Signal::new(0);
        let app = counter_app(count);
        let _ = app;

        let mut scene = Scene::new();
        app.draw(&mut scene, 100.0, 100.0);

        let nodes = app.root.nodes();
        assert!(!hit_test_nodes(&nodes, 1000.0, 1000.0, (100.0, 100.0)));
    }

    #[test]
    fn text_reads_signal_at_draw() {
        let count = Signal::new(0);
        let count2 = count;
        let text_node = text(move || format!("Count: {}", count2.get()));
        assert!(text_node.on_click_fn.is_none());

        // draw 读信号在 tracker.draw 外不触发 memo；直接验证信号可读
        count.set(5);
        assert_eq!(count.get(), 5);
    }

    #[test]
    fn ime_allowed_after_click_focus() {
        let text = Signal::new(String::new());
        let t1 = text;
        let t2 = text;
        let mut app = MveApp::new(move |cx| {
            let v = t1;
            let s = t2;
            cx.child(wy_mve::text_field(move || v.get(), move |t| s.set(t)));
        });
        assert_eq!(app.ime_allowed(), None, "无焦点不干预");

        let mut scene = Scene::new();
        app.draw(&mut scene, 100.0, 100.0);

        // 点击输入框（默认 200x32 内点 (50,16)）→ 聚焦 → 允许 IME
        let nodes = app.root.nodes();
        let chain = hit_test_node(&nodes[0], (0.0, 0.0), 50.0, 16.0, (0.0, 0.0))
            .unwrap()
            .into_iter()
            .map(|(n, _)| n)
            .collect::<Vec<_>>();
        app.focus.focus_on_click(&chain);
        assert_eq!(app.ime_allowed(), Some(true), "聚焦输入框允许 IME");

        // 清除焦点 → 不干预
        app.focus.clear_focus();
        assert_eq!(app.ime_allowed(), None);
    }

    #[test]
    fn ime_cursor_area_follows_focus() {
        let mut app = MveApp::new(move |cx: &mut wy_mve::NodeContext| {
            cx.child(wy_mve::column(move |cx| {
                cx.child(wy_mve::text_field(String::new, move |_| {}));
                cx.child(wy_mve::text_field(String::new, move |_| {}));
            }));
        });
        assert_eq!(app.ime_cursor_area(), None, "无焦点无光标盒");

        let mut scene = Scene::new();
        app.draw(&mut scene, 200.0, 100.0);

        // 点击第二个输入框（默认 200x32，列 gap 8 → 第二个 y≈40）
        let nodes = app.root.nodes();
        let chain = hit_test_node(&nodes[0], (0.0, 0.0), 50.0, 50.0, (0.0, 0.0))
            .unwrap()
            .into_iter()
            .map(|(n, _)| n)
            .collect::<Vec<_>>();
        app.focus.focus_on_click(&chain);

        let (x, y, w, h) = app.ime_cursor_area().expect("聚焦后应有 IME 光标盒");
        assert!((0.0..200.0).contains(&x), "x={x}");
        // 焦点链偏移累加：第二行输入框的 y 必须超过第一行高度（32）
        assert!(y >= 40.0, "候选窗应跟随第二行光标，y={y}");
        assert!(w > 0.0 && h > 0.0, "w={w} h={h}");

        // 清除焦点 → 无光标盒
        app.focus.clear_focus();
        assert_eq!(app.ime_cursor_area(), None);
    }

    #[test]
    fn click_then_tab_between_nested_text_fields() {
        let a = Signal::new(String::new());
        let b = Signal::new(String::new());
        let (a1, a2) = (a, a);
        let (b1, b2) = (b, b);
        let mut app = MveApp::new(move |cx: &mut wy_mve::NodeContext| {
            cx.child(wy_mve::vbox(vec![wy_mve::vbox_at(
                24.0,
                24.0,
                vec![
                    wy_mve::text_field(move || a1.get(), move |t| a2.set(t)),
                    wy_mve::text_field(move || b1.get(), move |t| b2.set(t)),
                ],
            )]));
        });

        let mut scene = Scene::new();
        app.draw(&mut scene, 400.0, 200.0);

        // 点击第一个字段（偏移 24,24 内的点）
        let nodes = app.root.nodes();
        let chain = hit_test_node(&nodes[0], (0.0, 0.0), 74.0, 40.0, (0.0, 0.0))
            .expect("应命中文本字段")
            .into_iter()
            .map(|(n, _)| n)
            .collect::<Vec<_>>();
        app.focus.focus_on_click(&chain);
        assert_eq!(app.ime_allowed(), Some(true), "点击后允许 IME");

        let focusables = crate::focus::focusable_nodes(&nodes);
        assert_eq!(focusables.len(), 2, "应有两个可聚焦文本字段");
        let leaf = Rc::clone(app.focus.path().first().expect("应聚焦"));
        assert!(
            Rc::ptr_eq(&leaf, &focusables[0][0].identity),
            "点击后应聚焦第一个字段"
        );

        // Tab → 焦点应移到第二个字段
        app.handle_key_event(&crate::event::KeyEvent {
            key: crate::event::Key::Tab,
            pressed: true,
            ctrl: false,
            shift: false,
            alt: false,
            meta: false,
        });
        let leaf2 = Rc::clone(app.focus.path().first().expect("应聚焦"));
        assert!(
            Rc::ptr_eq(&leaf2, &focusables[1][0].identity),
            "Tab 应聚焦第二个字段"
        );

        // 输入字符 → 进入第二个字段
        app.handle_key_event(&crate::event::KeyEvent {
            key: crate::event::Key::Char('x'),
            pressed: true,
            ctrl: false,
            shift: false,
            alt: false,
            meta: false,
        });
        assert_eq!(b.get(), "x", "字符应进入 Tab 后的第二个字段");
    }

    #[test]
    fn click_input_inside_padded_card() {
        let s = Signal::new(String::new());
        let (s1, s2) = (s, s);
        let mut app = MveApp::new(move |cx: &mut wy_mve::NodeContext| {
            // 模拟 card(): vbox[size_fn] > inner(vbox x=16,y=16) > text_field
            let mut inner = wy_mve::vbox(vec![wy_mve::text_field(
                move || s1.get(),
                move |t| s2.set(t),
            )]);
            // 显式定位用 x_fn/y_fn（P1-4：判据是 fn 存在性，不是静态值非 0）
            inner.x_fn = Some(Rc::new(|| 16.0));
            inner.y_fn = Some(Rc::new(|| 16.0));
            let mut card = wy_mve::vbox(vec![inner]);
            card.size_fn = Some(Rc::new(|| (300.0, 100.0)));
            cx.child(wy_mve::vbox_at(24.0, 24.0, vec![card]));
        });

        let mut scene = Scene::new();
        app.draw(&mut scene, 400.0, 200.0);

        // 字段绝对原点 ≈ (24+16, 24+16) = (40,40)
        let nodes = app.root.nodes();
        let hit = hit_test_node(&nodes[0], (0.0, 0.0), 60.0, 50.0, (0.0, 0.0))
            .expect("应命中卡片内输入框")
            .into_iter()
            .map(|(n, _)| n)
            .collect::<Vec<_>>();
        app.focus.focus_on_click(&hit);
        assert_eq!(app.ime_allowed(), Some(true), "点击卡片内输入框应允许 IME");
    }

    /// 焦点变化派发 `on_blur`/`on_focus`：点击/ Tab 切换都会触发。
    #[test]
    fn focus_change_fires_on_focus_and_on_blur() {
        let log: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
        let app = MveApp::new({
            let log = Rc::clone(&log);
            move |cx: &mut wy_mve::NodeContext| {
                let make = |tag: &'static str, x: f32| {
                    let lf = Rc::clone(&log);
                    let lb = Rc::clone(&log);
                    Node {
                        x,
                        width: 50.0,
                        height: 32.0,
                        focusable: true,
                        hit_test_fn: Rc::new(|px, py| {
                            (0.0..50.0).contains(&px) && (0.0..32.0).contains(&py)
                        }),
                        // has_handler 需要事件回调才算可命中
                        on_click_fn: Some(Rc::new(|_| {})),
                        on_focus_fn: Some(Rc::new(move || {
                            lf.borrow_mut().push(format!("focus:{tag}"))
                        })),
                        on_blur_fn: Some(Rc::new(move || {
                            lb.borrow_mut().push(format!("blur:{tag}"))
                        })),
                        ..Node::default()
                    }
                };
                cx.child(make("1", 0.0));
                cx.child(make("2", 60.0));
            }
        });

        let mut app = app;
        let mut scene = Scene::new();
        app.draw(&mut scene, 200.0, 100.0);

        let cursor = |x: f64, y: f64| winit::event::WindowEvent::CursorMoved {
            device_id: winit::event::DeviceId::dummy(),
            position: winit::dpi::PhysicalPosition::new(x, y),
        };
        let press = || winit::event::WindowEvent::MouseInput {
            device_id: winit::event::DeviceId::dummy(),
            state: winit::event::ElementState::Pressed,
            button: winit::event::MouseButton::Left,
        };

        // 点击节点 1 → focus:1
        app.handle_event(&cursor(25.0, 16.0));
        app.handle_event(&press());
        assert_eq!(&*log.borrow(), &["focus:1"]);

        // Tab → 焦点移向节点 2 → blur:1, focus:2
        app.handle_key_event(&crate::event::KeyEvent {
            key: crate::event::Key::Tab,
            pressed: true,
            ctrl: false,
            shift: false,
            alt: false,
            meta: false,
        });
        assert_eq!(&*log.borrow(), &["focus:1", "blur:1", "focus:2"]);
    }

    #[test]
    fn render_for_each_builds_list_once() {
        use wy_mve::NodeContext;
        let items = Signal::new(vec![1, 2, 3]);
        let items2 = items;
        let app = MveApp::new(move |cx: &mut NodeContext| {
            let list = items2;
            cx.render_for_each(
                move |emit| {
                    let snapshot = list.get();
                    for v in snapshot {
                        emit(v, v);
                    }
                },
                |_k, value, atomic_cx| {
                    let v = *value.borrow();
                    let _ = v;
                    atomic_cx.child(text(move || format!("item {}", v)));
                },
            );
        });

        let mut scene = Scene::new();
        app.draw(&mut scene, 100.0, 100.0);
        let nodes = app.root.nodes();
        assert_eq!(nodes.len(), 3);

        // 增删列表 → 顶层节点数变化
        items.set(vec![1, 2]);
        app.draw(&mut scene, 100.0, 100.0);
        let nodes = app.root.nodes();
        assert_eq!(nodes.len(), 2);
    }

    // -----------------------------------------------------------------------
    // 回归：RedrawTracker + MveApp 完整路径 → 光标移动触发重绘
    // -----------------------------------------------------------------------

    /// 模拟 runner 的完整路径：RedrawTracker.draw(|scene| app.draw(scene, ...))
    /// 验证：方向键移动光标 → RecordMemo 重录 Scene。
    #[test]
    fn richtext_cursor_move_via_runner_path() {
        use crate::focus::FocusPath;
        use std::cell::RefCell;
        use std::rc::Rc;
        use wy_mve::RichTextOpts;

        let doc = Rc::new(RefCell::new(String::from("hello")));
        let d1 = Rc::clone(&doc);
        let d2 = Rc::clone(&doc);
        let (editor_node, core) = wy_mve::rich_editable_opts(
            move || d1.borrow().clone(),
            move |t| *d2.borrow_mut() = t,
            RichTextOpts::default(),
        );

        let mut app = MveApp::new(move |cx| {
            cx.child(editor_node.clone());
        });

        // 创建 RedrawTracker（模拟 runner）
        let tracker = crate::redraw_tracker::RedrawTracker::new(Rc::new(|| {}));

        // 第一帧：tracker.record → app.draw → draw_fn 注册信号依赖
        tracker.draw(|scene| {
            app.draw(scene, 400.0, 100.0);
        });
        assert_eq!(core.borrow().cursor(), 0);

        // 聚焦到编辑器（模拟 Tab 或点击聚焦）
        let nodes = app.root.nodes();
        let mut focus_path = FocusPath::new();
        // 找到编辑器节点的 identity
        for node in &nodes {
            if node.focusable {
                focus_path.push(Rc::clone(&node.identity));
                break;
            }
        }
        assert!(!focus_path.is_empty(), "应有 focusable 编辑器节点");
        app.focus.set_focus(focus_path);

        // 设置光标到位置 2（"he|llo"）
        core.borrow_mut().set_cursor(2);
        assert_eq!(core.borrow().cursor(), 2);

        // 第二帧：光标变了 → RecordMemo 应重录
        tracker.draw(|scene| {
            app.draw(scene, 400.0, 100.0);
        });

        // 模拟方向键：通过 handle_key_event 走 runner 完整路径
        let arrow_right = crate::event::KeyEvent {
            key: crate::event::Key::ArrowRight,
            pressed: true,
            ctrl: false,
            shift: false,
            alt: false,
            meta: false,
        };
        app.handle_key_event(&arrow_right);
        assert_eq!(
            core.borrow().cursor(),
            3,
            "handle_key_event 后光标应右移到 3"
        );

        // 第三帧：anchor/focus 信号变了 → RecordMemo 应重录
        tracker.draw(|scene| {
            app.draw(scene, 400.0, 100.0);
        });
        assert_eq!(core.borrow().cursor(), 3, "Scene 重录后光标仍在 3");

        // 连续多次移动：验证响应链不会断裂
        for expected in 4..=5 {
            let ev = crate::event::KeyEvent {
                key: crate::event::Key::ArrowRight,
                pressed: true,
                ctrl: false,
                shift: false,
                alt: false,
                meta: false,
            };
            app.handle_key_event(&ev);
            assert_eq!(
                core.borrow().cursor(),
                expected,
                "第 {} 次移动",
                expected - 3
            );
            tracker.draw(|scene| {
                app.draw(scene, 400.0, 100.0);
            });
        }

        // 左箭头
        let arrow_left = crate::event::KeyEvent {
            key: crate::event::Key::ArrowLeft,
            pressed: true,
            ctrl: false,
            shift: false,
            alt: false,
            meta: false,
        };
        app.handle_key_event(&arrow_left);
        assert_eq!(core.borrow().cursor(), 4, "左箭头后光标应回到 4");
        tracker.draw(|scene| {
            app.draw(scene, 400.0, 100.0);
        });

        // 输入字符
        let char_a = crate::event::KeyEvent {
            key: crate::event::Key::Char('x'),
            pressed: true,
            ctrl: false,
            shift: false,
            alt: false,
            meta: false,
        };
        app.handle_key_event(&char_a);
        assert_eq!(core.borrow().text(), "hellxo", "字符 x 应插入到位置 4");
        // 注意：insert_text 内部会 set_cursor(5)
        assert_eq!(core.borrow().cursor(), 5, "插入后光标在 5");
        tracker.draw(|scene| {
            app.draw(scene, 400.0, 100.0);
        });
    }

    #[test]
    fn nested_text_field_does_not_steal_clicks() {
        // 回归：column > [button, text_field]。文本框在逆序命中遍历中先被检查，
        // 若其 hit_test 不看边界（恒 true），会越界截获上方按钮的点击 →
        // 按钮失效、焦点错位。修复后点击按钮应命中按钮。
        let clicked = Rc::new(RefCell::new(false));
        let app = MveApp::new({
            let clicked = Rc::clone(&clicked);
            move |cx| {
                let c = Rc::clone(&clicked);
                cx.child(column(move |cx| {
                    cx.child(button(move || "ok".into(), {
                        let c = Rc::clone(&c);
                        move || *c.borrow_mut() = true
                    }));
                    cx.child(wy_mve::text_field(String::new, |_| {}));
                }));
            }
        });

        let mut scene = Scene::new();
        app.draw(&mut scene, 400.0, 400.0);

        let nodes = app.root.nodes();
        let col = &nodes[0];
        let children = children_nodes(col);
        // 按真实布局盒取按钮中心（column 默认交叉轴居中，不能假设 x=0 起排）
        let boxes = wy_mve::layout_boxes(col, (400.0, 400.0), &children);
        let btn_box = boxes[0];
        let click = (
            btn_box.x + btn_box.width / 2.0,
            btn_box.y + btn_box.height / 2.0,
        );
        assert!(btn_box.width > 0.0 && btn_box.height > 0.0, "按钮应有尺寸");

        // 命中链最深节点应是按钮，而非下方文本框
        let chain =
            hit_test_node(col, (0.0, 0.0), click.0, click.1, (400.0, 400.0)).expect("命中按钮");
        assert!(
            chain[0].0.on_click_fn.is_some(),
            "最深命中应是按钮，而非文本框"
        );

        let mut event = PointerEvent::new(click.0, click.1);
        dispatch_click_nodes(&nodes, click.0, click.1, (400.0, 400.0), &mut event);
        assert!(*clicked.borrow(), "按钮应被点击，而不是被下方文本框截获");
    }

    /// 命中必须落在节点的布局盒内：点左侧窄条不能误触右侧长条。
    ///
    /// 这条回归覆盖"点顶栏却触发右侧盘口"那类错位——根因是默认 `hit_test`
    /// 恒真且没有盒边界判断。
    #[test]
    fn hit_test_stays_inside_layout_box() {
        use std::cell::RefCell;

        let log: Rc<RefCell<Vec<&'static str>>> = Rc::new(RefCell::new(Vec::new()));

        let probe = |name: &'static str, log: Rc<RefCell<Vec<&'static str>>>| {
            let mut n = Node {
                height: 30.0,
                on_click_fn: Some(Rc::new(move |_| log.borrow_mut().push(name))),
                ..Node::default()
            };
            n.hit_test_fn = Rc::new(|_, _| true);
            n
        };

        let mut left = probe("left", Rc::clone(&log));
        left.width = 100.0;
        let mut right = probe("right", Rc::clone(&log));
        right.grow = 1.0;

        let row = Rc::new(std::cell::RefCell::new(wy_mve::hbox_gap(
            0.0,
            vec![left, right],
        )));
        let app = MveApp::new(move |cx: &mut wy_mve::NodeContext| {
            let row = row.borrow().clone();
            // 交叉轴拉伸：让 row 占满 400 宽，左边 100 + 右边 grow 300。
            // 默认 `align_items = Center` 会把 row 居中到 150..250，
            // 那样 x=50/x=300 都点空（测试初版就栽在这）。
            let mut wrap = wy_mve::vbox_at(0.0, 0.0, vec![row]);
            wrap.align_items = wy_mve::AlignItem::Stretch;
            cx.child(wrap);
        });

        let mut scene = Scene::new();
        app.draw(&mut scene, 400.0, 100.0);

        let nodes = app.root.nodes();
        let mut event = PointerEvent::new(50.0, 10.0);
        dispatch_click_nodes(&nodes, 50.0, 10.0, (400.0, 100.0), &mut event);
        assert_eq!(*log.borrow(), vec!["left"], "点 50 应命中左侧窄条");

        log.borrow_mut().clear();
        let mut event = PointerEvent::new(300.0, 10.0);
        dispatch_click_nodes(&nodes, 300.0, 10.0, (400.0, 100.0), &mut event);
        assert_eq!(
            *log.borrow(),
            vec!["right"],
            "点 300 应命中 grow 出来的右半区"
        );

        // 点在两列之外（y 超出高度）不应命中任何东西
        log.borrow_mut().clear();
        let mut event = PointerEvent::new(300.0, 80.0);
        dispatch_click_nodes(&nodes, 300.0, 80.0, (400.0, 100.0), &mut event);
        assert!(log.borrow().is_empty(), "盒外点击不应命中");
    }

    /// 悬停进出只派发一次，移出后收到 leave。
    #[test]
    fn hover_enter_and_leave_fire_once() {
        let seen: Rc<std::cell::RefCell<Vec<bool>>> = Rc::new(std::cell::RefCell::new(Vec::new()));
        let s = Rc::clone(&seen);
        let mut node = Node {
            width: 100.0,
            height: 40.0,
            ..Node::default()
        };
        node.on_hover_fn = Some(Rc::new(move |entered| s.borrow_mut().push(entered)));

        let node = Rc::new(std::cell::RefCell::new(node));
        let app = MveApp::new(move |cx: &mut wy_mve::NodeContext| {
            let node = node.borrow().clone();
            // `Start` 让节点贴住 x=0，占据 0..100；居中会落到 50..150，
            // 那样 x=150 正好压在右边界（`x <= w` 判命中），移不出去。
            let mut wrap = wy_mve::vbox_at(0.0, 0.0, vec![node]);
            wrap.align_items = wy_mve::AlignItem::Start;
            cx.child(wrap);
        });
        let mut scene = Scene::new();
        app.draw(&mut scene, 200.0, 100.0);

        let mut app = app;
        let cursor = |x: f64, y: f64| winit::event::WindowEvent::CursorMoved {
            device_id: winit::event::DeviceId::dummy(),
            position: winit::dpi::PhysicalPosition::new(x, y),
        };
        app.handle_event(&cursor(50.0, 20.0));
        app.handle_event(&cursor(52.0, 21.0)); // 仍在内部：不应重复触发
        app.handle_event(&cursor(150.0, 20.0)); // 移出（节点只占 0..100）

        assert_eq!(*seen.borrow(), vec![true, false], "进入一次、离开一次");
    }

    /// 滚轮沿命中链派发，链外不触发。
    #[test]
    fn wheel_dispatches_along_hit_chain() {
        let seen: Rc<RefCell<Vec<f32>>> = Rc::new(RefCell::new(Vec::new()));
        let s = Rc::clone(&seen);
        let mut node = Node {
            width: 100.0,
            height: 40.0,
            ..Node::default()
        };
        node.on_wheel_fn = Some(Rc::new(move |ev: &mut wy_mve::PointerEvent| {
            s.borrow_mut().push(ev.wheel);
            ev.stop_propagation();
        }));

        let node = Rc::new(RefCell::new(node));
        let app = MveApp::new(move |cx: &mut wy_mve::NodeContext| {
            let node = node.borrow().clone();
            let mut wrap = wy_mve::vbox_at(0.0, 0.0, vec![node]);
            wrap.align_items = wy_mve::AlignItem::Start;
            cx.child(wrap);
        });
        let mut scene = Scene::new();
        app.draw(&mut scene, 200.0, 100.0);

        // 先把光标挪进节点（滚轮用上一次光标位置命中）
        let mut app = app;
        let cursor = |x: f64, y: f64| winit::event::WindowEvent::CursorMoved {
            device_id: winit::event::DeviceId::dummy(),
            position: winit::dpi::PhysicalPosition::new(x, y),
        };
        let wheel = |dy: f64| winit::event::WindowEvent::MouseWheel {
            device_id: winit::event::DeviceId::dummy(),
            delta: winit::event::MouseScrollDelta::LineDelta(0.0, dy as f32),
            phase: winit::event::TouchPhase::Moved,
        };
        app.handle_event(&cursor(50.0, 20.0));
        assert!(app.handle_event(&wheel(2.0)), "节点消费了滚轮应返回 true");
        app.handle_event(&wheel(-1.0));

        assert_eq!(*seen.borrow(), vec![2.0, -1.0], "滚动量应原样送达");

        // 移到节点之外：滚轮不再命中
        app.handle_event(&cursor(180.0, 20.0));
        assert!(!app.handle_event(&wheel(3.0)));
        assert_eq!(seen.borrow().len(), 2, "链外滚轮不应触发");
    }

    /// 悬停移动携带局部坐标，且**不**触发"按下期"的 on_move（拖选用）。
    #[test]
    fn hover_move_carries_local_coords_and_keeps_drag_move_separate() {
        let moved: Rc<RefCell<Vec<(f32, f32)>>> = Rc::new(RefCell::new(Vec::new()));
        let dragged = Rc::new(RefCell::new(0u32));

        let m = Rc::clone(&moved);
        let d = Rc::clone(&dragged);
        let mut node = Node {
            width: 100.0,
            height: 40.0,
            ..Node::default()
        };
        node.on_hover_move_fn = Some(Rc::new(move |ev: &mut wy_mve::PointerEvent| {
            m.borrow_mut().push((ev.x, ev.y));
        }));
        node.on_move_fn = Some(Rc::new(move |_ev: &mut wy_mve::PointerEvent| {
            *d.borrow_mut() += 1;
        }));

        let node = Rc::new(RefCell::new(node));
        let app = MveApp::new(move |cx: &mut wy_mve::NodeContext| {
            let node = node.borrow().clone();
            let mut wrap = wy_mve::vbox_at(0.0, 0.0, vec![node]);
            wrap.align_items = wy_mve::AlignItem::Start;
            cx.child(wrap);
        });
        let mut scene = Scene::new();
        app.draw(&mut scene, 200.0, 100.0);

        let mut app = app;
        let cursor = |x: f64, y: f64| winit::event::WindowEvent::CursorMoved {
            device_id: winit::event::DeviceId::dummy(),
            position: winit::dpi::PhysicalPosition::new(x, y),
        };
        app.handle_event(&cursor(30.0, 10.0));
        app.handle_event(&cursor(40.0, 12.0));

        assert_eq!(*moved.borrow(), vec![(30.0, 10.0), (40.0, 12.0)]);
        assert_eq!(*dragged.borrow(), 0, "未按下时不应派发拖选用的 on_move");
    }

    /// clip 容器：溢出内容不可见也不可点（滚动容器裁剪语义）。
    ///
    /// 容器视口 200×100，内容列高 300，可点击按钮位于 y=240..280（视口外）。
    /// 没有 clip 守卫时，点 (50, 270) 会命中"看不见"的按钮——正是滚动容器
    /// 最典型的事故；有守卫时必须返回 None。
    #[test]
    fn clip_hides_overflow_from_hit_test() {
        let clicked = Rc::new(RefCell::new(0u32));
        let c = Rc::clone(&clicked);
        let button = Node {
            width: 180.0,
            height: 40.0,
            on_click_fn: Some(Rc::new(move |_ev: &mut wy_mve::PointerEvent| {
                *c.borrow_mut() += 1;
            })),
            ..Node::default()
        };
        let spacer = Node {
            height: 240.0,
            ..Node::default()
        };
        let content = Node {
            layout: Some(wy_mve::Layout::Column),
            arg_children_fn: Rc::new(move |cx: &mut wy_mve::NodeContext| {
                cx.add_node(spacer.clone());
                cx.add_node(button.clone());
            }),
            ..Node::default()
        };
        let wheel_seen = Rc::new(RefCell::new(0u32));
        let w = Rc::clone(&wheel_seen);
        let container = Node {
            layout: Some(wy_mve::Layout::Row),
            width: 200.0,
            height: 100.0,
            clip: true,
            on_wheel_fn: Some(Rc::new(move |_ev: &mut wy_mve::PointerEvent| {
                *w.borrow_mut() += 1;
            })),
            arg_children_fn: Rc::new(move |cx: &mut wy_mve::NodeContext| {
                cx.add_node(content.clone());
            }),
            ..Node::default()
        };
        let container = Rc::new(RefCell::new(container));
        let app = MveApp::new(move |cx: &mut wy_mve::NodeContext| {
            cx.child(container.borrow().clone());
        });
        let mut scene = Scene::new();
        app.draw(&mut scene, 200.0, 100.0);
        // 绘制期应有裁剪入栈
        assert!(scene
            .iter()
            .any(|p| matches!(p, wy_render::Primitive::ClipPush { .. })));

        let nodes = app.root().nodes();
        // 视口内：命中容器自身（滚轮 handler）
        assert!(
            nodes
                .iter()
                .any(|n| hit_test_node(n, (0.0, 0.0), 50.0, 50.0, (200.0, 100.0)).is_some()),
            "视口内可命中"
        );
        // 视口下方（内容溢出区）：不可命中——点不到看不见的按钮
        assert!(
            nodes
                .iter()
                .all(|n| hit_test_node(n, (0.0, 0.0), 50.0, 270.0, (200.0, 100.0)).is_none()),
            "溢出内容不应被命中"
        );
        assert_eq!(*clicked.borrow(), 0);
    }

    /// not_in_layout：不参与 flex 与自然尺寸，x/y 即布局盒。
    #[test]
    fn not_in_layout_skips_flex_and_uses_xy() {
        let normal = Node {
            width: 200.0,
            height: 40.0,
            ..Node::default()
        };
        let overlay = Node {
            not_in_layout: true,
            x: 10.0,
            y: 20.0,
            width: 100.0,
            height: 50.0,
            ..Node::default()
        };
        let parent = Node {
            layout: Some(wy_mve::Layout::Column),
            arg_children_fn: Rc::new(move |cx: &mut wy_mve::NodeContext| {
                cx.add_node(normal.clone());
                cx.add_node(overlay.clone());
            }),
            ..Node::default()
        };
        // 自然尺寸不含浮层：只有 normal 的 40
        let (pw, ph) = wy_mve::node_size(&parent);
        assert_eq!((pw, ph), (200.0, 40.0), "浮层不撑父容器尺寸");
        let children = wy_mve::children_nodes(&parent);
        let boxes = wy_mve::layout_boxes(&parent, (200.0, 300.0), &children);
        assert_eq!(boxes[0], wy_mve::LayoutBox::new(0.0, 0.0, 200.0, 40.0));
        // 盒归零：绝对位置由绘制/命中期的 pos_x/pos_y 统一叠加
        assert_eq!(boxes[1], wy_mve::LayoutBox::new(0.0, 0.0, 100.0, 50.0));
    }

    /// 焦点链未消费时按键全局广播：浮层 Esc 不依赖焦点位置。
    #[test]
    fn key_broadcast_reaches_unfocused_key_fn() {
        let esc_seen = Rc::new(RefCell::new(0u32));
        let e = Rc::clone(&esc_seen);
        let overlay = Node {
            width: 800.0,
            height: 600.0,
            key_fn: Some(Rc::new(move |ev: &mut wy_mve::KeyEvent| {
                if ev.key == wy_mve::Key::Escape {
                    *e.borrow_mut() += 1;
                    true
                } else {
                    false
                }
            })),
            ..Node::default()
        };
        let overlay = Rc::new(RefCell::new(overlay));
        let mut app = MveApp::new(move |cx: &mut wy_mve::NodeContext| {
            cx.child(overlay.borrow().clone());
        });
        let key = |k: crate::event::Key| crate::event::KeyEvent {
            key: k,
            pressed: true,
            ctrl: false,
            shift: false,
            alt: false,
            meta: false,
        };
        // 无焦点：Esc 仍到达浮层
        app.handle_key_event(&key(crate::event::Key::Escape));
        assert_eq!(*esc_seen.borrow(), 1, "无焦点也应广播到 key_fn");
        // 其它键不消费（浮层只认 Esc）
        app.handle_key_event(&key(crate::event::Key::Enter));
        assert_eq!(*esc_seen.borrow(), 1, "未声明的键不应触发");
    }

    #[test]
    fn clicking_text_field_then_typing_updates_value() {
        // 回归链路：点击文本框 → 焦点落到它 → 按键字符进入文本。
        // 三段任一断掉，用户看到的都是"点了输入框打不出字"。
        let value = Signal::new(String::new());
        let v = value;
        let mut app = MveApp::new(move |cx| {
            cx.child(wy_mve::rsx! {
                node {
                    width: 400.0,
                    height: 60.0,
                    align_items: wy_mve::AlignItem::Start,
                    text_field {
                        value: move || v.get(),
                        on_change: move |s: String| v.set(s),
                        width: 200.0,
                        height: 34.0,
                    }
                }
            });
        });

        app.on_resize(400.0, 200.0);
        let mut scene = Scene::new();
        app.draw(&mut scene, 400.0, 200.0);

        // 点击文本框（位于根节点内，坐标 (50, 15)）
        {
            use winit::event::{ElementState, MouseButton, WindowEvent};
            let cursor = WindowEvent::CursorMoved {
                device_id: winit::event::DeviceId::dummy(),
                position: winit::dpi::PhysicalPosition::new(50.0, 15.0),
            };
            let _ = app.handle_event(&cursor);
            let press = WindowEvent::MouseInput {
                device_id: winit::event::DeviceId::dummy(),
                state: ElementState::Pressed,
                button: MouseButton::Left,
            };
            let _ = app.handle_event(&press);
        }

        assert!(!app.focus_path().is_empty(), "点击文本框后焦点链不应为空");

        // 输入一个字符
        app.handle_key_event(&crate::event::KeyEvent {
            key: crate::event::Key::Char('a'),
            pressed: true,
            ctrl: false,
            shift: false,
            alt: false,
            meta: false,
        });

        assert_eq!(value.get(), String::from("a"), "按键后文本框内容应更新");
    }

    #[test]
    fn viewport_is_tracked_as_draw_dependency() {
        // 回归：「窗口一放大布局不跟随」的根因。
        //
        // 绘制侧用 `RecordMemo` 缓存整帧 Scene，只在**被追踪的信号依赖**变化时
        // 重录。视口尺寸若只是普通 `Cell`，就永远不是依赖 → resize 后重绘
        // 复用旧 Scene → 布局停在旧尺寸。这里断言：视口变化能被追踪到，
        // 即 `viewport` 必须是信号且被绘制读入。
        let mut app = MveApp::new(|cx| {
            cx.child(wy_mve::fc::fc__node(Vec::<wy_mve::Slot>::new()));
        });

        // 初始视口
        app.on_resize(800.0, 600.0);
        assert_eq!(app.viewport(), (800.0, 600.0));

        // 缩放后视口随之更新（命中测试与布局都依赖它）
        app.on_resize(1200.0, 800.0);
        assert_eq!(app.viewport(), (1200.0, 800.0));
    }

    #[test]
    fn draw_uses_tracked_viewport_size() {
        // 根节点铺满视口：把 `current_box()` 画成矩形，
        // 绘制结果的宽度即「绘制期看到的视口宽度」。
        // 若绘制期用的是过期尺寸（如上一帧的 800），这里会失败。
        let mut app = MveApp::new(|cx| {
            cx.child(wy_mve::rsx! {
                node {
                    draw: |s: &mut Scene| {
                        let b = wy_mve::current_box();
                        s.fill_rect(wy_render::Rect::new(0.0, 0.0, b.width, b.height), wy_render::Color::RED);
                    },
                }
            });
        });

        app.on_resize(800.0, 600.0);
        let mut scene = Scene::new();
        app.draw(&mut scene, 800.0, 600.0);

        // 视口变化 → 绘制期应读到新尺寸
        app.on_resize(1200.0, 800.0);
        let mut scene2 = Scene::new();
        app.draw(&mut scene2, 1200.0, 800.0);

        let mut width = 0.0f32;
        for p in scene2.iter() {
            if let wy_render::Primitive::Rect { rect, .. } = p {
                width = rect.width;
            }
        }
        assert!(
            (width - 1200.0).abs() < 0.5,
            "绘制期应看到新视口宽度 1200，实际 {width}"
        );
    }

    #[test]
    fn resize_invalidates_draw_cache_so_layout_follows() {
        // 完整复现 runner 的绘制管线：RedrawTracker 缓存整帧 Scene，
        // 只在**被追踪的信号依赖**变化时重录。
        //
        // 旧实现里视口是普通 `Cell`（不是信号），因此 resize 后
        // `request_redraw()` 触发的这一帧被 tracker 短路、复用旧 Scene——
        // 画面停在旧尺寸，布局完全不跟随窗口。这正是用户报的
        // 「窗口一放大布局不跟随」的真因（之前只补了 needs_redraw，
        // 没解决缓存失效）。
        let mut app = MveApp::new(|cx| {
            cx.child(wy_mve::rsx! {
                node {
                    draw: |s: &mut Scene| {
                        let b = wy_mve::current_box();
                        s.fill_rect(wy_render::Rect::new(0.0, 0.0, b.width, b.height), wy_render::Color::RED);
                    },
                }
            });
        });

        let tracker = crate::redraw_tracker::RedrawTracker::new(Rc::new(|| {}));

        // 第一帧：800x600
        app.on_resize(800.0, 600.0);
        tracker.draw(|scene| app.draw(scene, 800.0, 600.0));
        {
            let cached = tracker.scene();
            let mut w = 0.0f32;
            for p in cached.iter() {
                if let wy_render::Primitive::Rect { rect, .. } = p {
                    w = rect.width;
                }
            }
            assert!((w - 800.0).abs() < 0.5, "首帧宽度应为 800，实际 {w}");
        }

        // 窗口放大 → on_resize 写视口信号 → tracker 应判定依赖变化并重录
        app.on_resize(1200.0, 800.0);
        tracker.draw(|scene| app.draw(scene, 1200.0, 800.0));
        {
            let cached = tracker.scene();
            let mut w = 0.0f32;
            for p in cached.iter() {
                if let wy_render::Primitive::Rect { rect, .. } = p {
                    w = rect.width;
                }
            }
            assert!(
                (w - 1200.0).abs() < 0.5,
                "放大后缓存应重录，宽度应为 1200，实际 {w}"
            );
        }
    }
}
