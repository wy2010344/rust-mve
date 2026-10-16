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

use wy_mve::{children_nodes, has_handler, layout_offsets, render_root, Node, NodeContext};
use wy_render::{Point, Scene};

use crate::focus::{locate, tab_navigate, FocusManager, FocusPath};

/// 顶层节点遍历（origin 为节点坐标系原点在父坐标系中的位置）。
fn draw_nodes(nodes: &[Node], scene: &mut Scene) {
    for node in nodes {
        draw_node(node, (0.0, 0.0), scene);
    }
}

/// 递归绘制：自我绘制在最底层，子节点按文档序叠在其上（对应 Kotlin Node.draw）。
///
/// 位置/可见性/间距走节点的动态属性（`pos_x`/`pos_y`/`is_hidden`/`gap`），
/// 在 track 上下文内求值，读信号自动触发重绘。
fn draw_node(node: &Node, origin: (f32, f32), scene: &mut Scene) {
    if node.is_hidden() {
        return;
    }
    let pos = (origin.0 + node.pos_x(), origin.1 + node.pos_y());
    if pos != (0.0, 0.0) {
        scene.push_transform(Point::new(pos.0, pos.1));
    }
    if !node.skip_draw {
        node.run_draw(scene);
    }
    let children = children_nodes(node);
    match node.layout {
        Some(layout) => {
            let offsets = layout_offsets(layout, node.gap(), &children);
            for (child, ofs) in children.iter().zip(offsets.iter()) {
                draw_node(child, *ofs, scene);
            }
        }
        None => {
            for child in children.iter() {
                draw_node(child, (0.0, 0.0), scene);
            }
        }
    }
    if pos != (0.0, 0.0) {
        scene.pop_transform();
    }
}

/// 命中测试：子节点优先（后绘制的在上，倒序），对应 Kotlin `Node.hitTest`。
#[cfg(test)]
fn hit_test_nodes(nodes: &[Node], x: f32, y: f32) -> bool {
    nodes
        .iter()
        .any(|node| hit_test_node(node, (0.0, 0.0), x, y).is_some())
}

/// 返回命中节点的链（子→根，对应 Kotlin hitTest 的 [NodeWithPosition] 链）。
///
/// 每项为 `(Node, 绝对坐标)`，其中绝对坐标是该节点在根坐标系下的原点
/// （Kotlin `NodeWithPosition.position`），供分发时换算局部坐标。
fn hit_test_node(
    node: &Node,
    origin: (f32, f32),
    x: f32,
    y: f32,
) -> Option<Vec<(Node, (f32, f32))>> {
    if node.is_hidden() {
        return None;
    }
    let pos = (origin.0 + node.pos_x(), origin.1 + node.pos_y());
    let children = children_nodes(node);
    let offsets = match node.layout {
        Some(layout) => layout_offsets(layout, node.gap(), &children),
        None => vec![(0.0, 0.0); children.len()],
    };
    // 子节点优先：倒序（后绘制的在上面）
    for (child, ofs) in children.iter().zip(offsets.iter()).rev() {
        let child_origin = (pos.0 + ofs.0, pos.1 + ofs.1);
        if let Some(mut chain) = hit_test_node(child, child_origin, x, y) {
            chain.push((node.clone(), pos));
            return Some(chain);
        }
    }
    // 自身：必须有 handler 且命中才算
    if has_handler(node) && node.run_hit_test(x - pos.0, y - pos.1) {
        return Some(vec![(node.clone(), pos)]);
    }
    None
}

/// 沿命中链冒泡分发指针事件（叶→根，对应 Kotlin 冒泡阶段，先 onDown 后 onClick）。
///
/// 每节点用**局部坐标**（命中链携带的绝对位置换算），复刻 Kotlin
/// `dispatchClick`/`dispatchPointerEvent`：先 `runOnDown` 再 `runOnClick`，
/// 任一阶段 stopPropagation 即终止。
#[cfg(test)]
fn dispatch_click_nodes(nodes: &[Node], x: f32, y: f32, event: &mut wy_mve::PointerEvent) {
    if let Some(chain) = nodes
        .iter()
        .find_map(|node| hit_test_node(node, (0.0, 0.0), x, y))
    {
        dispatch_click_chain(&chain, event);
    }
}

/// 沿给定命中链分发 down/click 两阶段（链为 叶→根，携带绝对原点）。
fn dispatch_click_chain(chain: &[(Node, (f32, f32))], event: &mut wy_mve::PointerEvent) {
    // 链：叶→根（与调度一致）。每项 (node, 绝对位置)，换算局部坐标。
    for (node, pos) in chain {
        // Kotlin 区分 Down/Click 的方式是**调用哪个 handler**（runOnDown vs
        // runOnClick），不是事件上的字段——本 Rust port 的 `PointerEvent`
        // 无 `method` 字段（编译器 `available fields` 佐证）。两阶段循环本身
        // 就已表达阶段：第一轮 = Down，第二轮 = Click。
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

/// 沿捕获链分发指针移动（叶→根；拖动扩展选区）。
fn dispatch_move_chain(chain: &[(Node, (f32, f32))], x: f32, y: f32, shift: bool) {
    let mut event = wy_mve::PointerEvent::new(x, y);
    event.shift = shift;
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
fn dispatch_up_chain(chain: &[(Node, (f32, f32))], x: f32, y: f32, shift: bool) {
    let mut event = wy_mve::PointerEvent::new(x, y);
    event.shift = shift;
    for (node, pos) in chain {
        event.x = x - pos.0;
        event.y = y - pos.1;
        node.run_on_up(&mut event);
        if event.stopped {
            return;
        }
    }
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
    /// 当前修饰键（winit ModifiersChanged 同步；按下时填入 PointerEvent.shift）。
    modifiers: winit::keyboard::ModifiersState,
}

impl MveApp {
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
            modifiers: winit::keyboard::ModifiersState::empty(),
        }
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
    fn draw(&self, scene: &mut Scene, _width: f32, _height: f32) {
        // 等价于 Kotlin: signal.collect { didDraw().draw(canvas, 0f, 0f) }
        // Root.target 读取 → RedrawTracker 追踪 → 信号变化触发重绘
        let nodes = self.root.nodes();
        draw_nodes(&nodes, scene);
    }

    fn widget_tree(&mut self) -> Option<&mut wy_render::widget_tree::WidgetTree> {
        // 不使用 WidgetTree：直接遍历 Node 树绘制和处理事件
        None
    }

    fn on_resize(&mut self, _width: f32, _height: f32) {}

    fn handle_event(&mut self, event: &winit::event::WindowEvent) -> bool {
        use winit::event::{ElementState, MouseButton, WindowEvent};

        // SAFETY: handle_event 在 runner 的事件循环中调用，
        // runner 拥有 self.app 的独占访问权。
        let cursor = unsafe { &mut *self.cursor.get() };

        match event {
            WindowEvent::ModifiersChanged(mods) => {
                self.modifiers = mods.state();
                false
            }
            WindowEvent::CursorMoved { position, .. } => {
                *cursor = (position.x as f32, position.y as f32);
                // 按下捕获期间：沿链派发移动（编辑器拖选）
                if let Some(chain) = &self.capture {
                    let (x, y) = *cursor;
                    let shift = self.modifiers.shift_key();
                    dispatch_move_chain(chain, x, y, shift);
                }
                false
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } => {
                let (x, y) = *cursor;
                let nodes = self.root.nodes();
                let chain = nodes
                    .iter()
                    .find_map(|node| hit_test_node(node, (0.0, 0.0), x, y));
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
                mve_event.stopped
            }
            WindowEvent::MouseInput {
                state: ElementState::Released,
                button: MouseButton::Left,
                ..
            } => {
                let (x, y) = *cursor;
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
                self.focus.dispatch_key(&nodes, &mut mve_event);
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
        for i in 0..chain.len() {
            origin.0 += chain[i].pos_x();
            origin.1 += chain[i].pos_y();
            if let Some(child) = chain.get(i + 1) {
                let children = children_nodes(&chain[i]);
                let idx = children.iter().position(|c| c == child)?;
                let offsets = match chain[i].layout {
                    Some(layout) => layout_offsets(layout, chain[i].gap(), &children),
                    None => vec![(0.0, 0.0); children.len()],
                };
                let ofs = offsets.get(idx)?;
                origin.0 += ofs.0;
                origin.1 += ofs.1;
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
        let offsets = layout_offsets(layout, col.gap(), &children_nodes(&col));
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
        let chain = hit_test_node(col, (0.0, 0.0), click.0, click.1);
        assert!(chain.is_some());
        let chain = chain.unwrap();
        assert_eq!(chain.len(), 3); // +, row, column
        assert!(chain[0].0.on_click_fn.is_some(), "deepest is the + button");
        assert_eq!(chain[1].0.layout, Some(wy_mve::Layout::Row));
        assert_eq!(chain[1].0.gap(), 8.0);

        // 分发点击 → + 按钮 handler 执行并 stop
        let mut event = PointerEvent::new(click.0, click.1);
        dispatch_click_nodes(&nodes, click.0, click.1, &mut event);
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
        assert!(!hit_test_nodes(&nodes, 1000.0, 1000.0));
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
        let chain = hit_test_node(&nodes[0], (0.0, 0.0), 50.0, 16.0)
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
        let chain = hit_test_node(&nodes[0], (0.0, 0.0), 50.0, 50.0)
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
        let chain = hit_test_node(&nodes[0], (0.0, 0.0), 74.0, 40.0)
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
            inner.x = 16.0;
            inner.y = 16.0;
            let mut card = wy_mve::vbox(vec![inner]);
            card.size_fn = Some(Rc::new(|| (300.0, 100.0)));
            cx.child(wy_mve::vbox_at(24.0, 24.0, vec![card]));
        });

        let mut scene = Scene::new();
        app.draw(&mut scene, 400.0, 200.0);

        // 字段绝对原点 ≈ (24+16, 24+16) = (40,40)
        let nodes = app.root.nodes();
        let hit = hit_test_node(&nodes[0], (0.0, 0.0), 60.0, 50.0)
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
        let (btn_w, btn_h) = node_size(&children[0]);
        let click = (btn_w / 2.0, btn_h / 2.0);

        // 命中链最深节点应是按钮，而非下方文本框
        let chain = hit_test_node(col, (0.0, 0.0), click.0, click.1).expect("命中按钮");
        assert!(
            chain[0].0.on_click_fn.is_some(),
            "最深命中应是按钮，而非文本框"
        );

        let mut event = PointerEvent::new(click.0, click.1);
        dispatch_click_nodes(&nodes, click.0, click.1, &mut event);
        assert!(*clicked.borrow(), "按钮应被点击，而不是被下方文本框截获");
    }
}
