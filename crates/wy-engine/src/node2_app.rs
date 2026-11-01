//! Node2 原生应用入口（新模式，不走 v1 `to_node` 桥接）。
//!
//! 这是「旧模式」（`MveApp` + v1 `Node` 引擎）的替代运行时：一棵树就是
//! [`wy_mve::canvas_render2::node2::Node2`]，绘制走 [`Node2::render_node2`]，
//! 命中走 [`hit_path`]，指针走 [`tree::pointer`] 路由 + [`tree::dispatch`]，
//! 键盘走 [`tree::keyboard::route_key_event`]，IME 走 [`focus::active_editor`]，
//! resize 把视口写进一个共享 `Cell` 供根节点的 `width`/`height` 读取。
//!
//! 与 TS 对齐：画布根节点在原点 `(0,0)`、尺寸等于视口（对标 TS 画布给根
//! `width`/`height`）；子节点的 `x`/`y`/`width`/`height` 都是读取式、由 flex
//! 布局在绘制期惰性测量。

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};

use wy_mve::canvas_render2::animation::{take_due_frame, time_until_next_frame};
use wy_mve::canvas_render2::focus::{self, EditableTextHandle, KeyEvent2};
use wy_mve::canvas_render2::node2::Node2;
use wy_mve::canvas_render2::protocol::{
    EditorHandleExt, MouseType2, PointerDevice, POINTER_BUTTON_LEFT, POINTER_BUTTON_RIGHT,
};
use wy_mve::canvas_render2::renderer::{
    dispatch_global_mouse_down, dispatch_global_mouse_move, dispatch_global_mouse_up,
};
use wy_mve::canvas_render2::selection::{
    as_selectable2, EditorSelectionRoute, HitestResult2, PointerInput2, PointerSelect2, SelPair2,
    Selectable2, SelectionManager2, SelectionSource2,
};
use wy_mve::canvas_render2::tree::dispatch::{dispatch_mouse_path, MouseDispatchInit};
use wy_mve::canvas_render2::tree::engine_global;
use wy_mve::canvas_render2::tree::focus_walk::next_focus_node;
use wy_mve::canvas_render2::tree::geometry::hit_path;
use wy_mve::canvas_render2::tree::keyboard::{route_key_event, KeyRouterDeps, KeyTarget2};
use wy_mve::canvas_render2::tree::pointer::{
    create_pointer_router, PointerRouter2, PointerRouterDeps,
};
use wy_mve::canvas_render2::CARET_BLINK_MS;
use wy_mve::CursorType;
use wy_mve::{ImeEvent, Key as MveKey, KeyEvent as MveKeyEvent};
use wy_render::Scene;
use wy_signal::{create_signal, GetValue, SetValue, Signal};

use crate::event::KeyEvent as EngineKeyEvent;
use crate::ime_guard::ImeCommitGuard;
use crate::runner::WyApp;

/// 当前时间（毫秒），供指针连击窗口判定。
fn now_ms() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0.0, |d| d.as_millis() as f64)
}

/// 字节偏移 → 字符索引（越界钳到 `[0, total]`），复刻 `editable_text_node::byte_to_char`。
///
/// IME `Preedit` 的 `cursor` 是 `(start_byte, end_byte)`，而 [`EditableTextHandle::on_composing`]
/// 需要字符索引；这里把 `end` 字节换算成字符索引。
fn byte_to_char_idx(text: &str, byte: usize) -> usize {
    let byte = byte.min(text.len());
    text[..byte].chars().count()
}

/// 把引擎键码翻译成框架的语义键 [`wy_mve::Key`]。
///
/// 两套键位分开是有意的：引擎侧 [`crate::event::Key`] 贴着 winit 的物理键走，
/// 框架侧只关心"输入 / 导航"这类语义（`Char` / `ArrowUp` / `Enter` …）。
fn to_mve_key(k: &crate::event::Key) -> MveKey {
    use crate::event::Key as RK;
    match k {
        RK::Char(c) => MveKey::Char(*c),
        RK::Enter => MveKey::Enter,
        RK::Backspace => MveKey::Backspace,
        RK::Delete => MveKey::Delete,
        RK::ArrowUp => MveKey::ArrowUp,
        RK::ArrowDown => MveKey::ArrowDown,
        RK::ArrowLeft => MveKey::ArrowLeft,
        RK::ArrowRight => MveKey::ArrowRight,
        RK::Tab => MveKey::Tab,
        RK::Escape => MveKey::Escape,
        RK::Home => MveKey::Home,
        RK::End => MveKey::End,
        RK::PageUp => MveKey::PageUp,
        RK::PageDown => MveKey::PageDown,
    }
}

/// 活跃编辑器包成 `KeyTarget2`，供键盘路由优先派发（避免与焦点节点重复消费）。
struct EditorKeyTarget(Rc<dyn EditableTextHandle>);

impl KeyTarget2 for EditorKeyTarget {
    fn handle_key(&self, e: &KeyEvent2) -> bool {
        self.0.handle_key(e)
    }
    fn is_same_node(&self, node: &Node2) -> bool {
        node.find_ext_rc::<EditorHandleExt>()
            .and_then(|e| {
                Rc::clone(&e.0)
                    .downcast::<Rc<dyn EditableTextHandle>>()
                    .ok()
            })
            .is_some_and(|h| Rc::ptr_eq(&*h, &self.0))
    }
}

/// 选择器的派生源（对标 TS `Renderer.ts` 里 `createSelectionManager({..})` 的入参）。
///
/// **必须是信号**：管理器的 `ranges` 是 memo，只有读信号才会建立响应式依赖；
/// 用 `RefCell` 的话"按下 → 选区变化"永远不会被看见——这正是"跨组件选择仍然
/// 不行"的第二个根因。
struct Node2SelectionSource {
    pointer_select: Signal<Option<PointerSelect2>>,
    move_hit_test: Signal<Option<HitestResult2>>,
    root: Node2,
}

impl SelectionSource2 for Node2SelectionSource {
    fn pointer_select(&self) -> Option<PointerSelect2> {
        self.pointer_select.get()
    }
    fn move_hit_test(&self) -> Option<HitestResult2> {
        self.move_hit_test.get()
    }
    fn root_node(&self) -> Option<Node2> {
        Some(self.root.clone())
    }
    fn clear_pointer_select(&self) {
        self.pointer_select.set(None);
    }
    fn active_editor_sel_pair(&self) -> Option<SelPair2> {
        focus::active_editor().and_then(|e| e.cursor_sel_pair())
    }
    fn active_editor_route(&self) -> Option<Rc<dyn EditorSelectionRoute>> {
        focus::active_editor().map(|e| Rc::new(EditorRoute(e)) as Rc<dyn EditorSelectionRoute>)
    }
}

/// 把编辑器句柄接到 [`EditorSelectionRoute`] 上（TS 里两者结构兼容，Rust 需要显式桥）。
struct EditorRoute(Rc<dyn EditableTextHandle>);

impl EditorSelectionRoute for EditorRoute {
    fn as_selectable(&self) -> Rc<dyn Selectable2> {
        self.0.as_selectable()
    }
    fn select_range(&self, anchor_offset: usize, focus_offset: usize) {
        self.0.select_range(anchor_offset, focus_offset);
    }
    fn select_all(&self) {
        self.0.select_all();
    }
}

/// 指针路由的外部能力（[`PointerRouterDeps`]）。
///
/// 与 [`Node2App`] 共享根节点、`viewport`、`redraw` 回调，以及少量命中会话状态。
struct Node2Deps {
    root: Node2,
    redraw: Rc<RefCell<Option<Rc<dyn Fn()>>>>,
    pointer_select: Signal<Option<PointerSelect2>>,
    move_hit_test: Signal<Option<HitestResult2>>,
    cursor: Cell<CursorType>,
    /// 全局唯一的选择协调者（TS `Renderer.ts` 里的 `selectionManager`）。
    selection: Rc<SelectionManager2>,
}

impl PointerRouterDeps for Node2Deps {
    fn hit(
        &self,
        x: f64,
        y: f64,
    ) -> Vec<wy_mve::canvas_render2::tree::geometry::NodeWithPosition2> {
        hit_path(&self.root, x as f32, y as f32)
    }
    fn selectable_of(&self, node: &Node2) -> Option<Rc<dyn Selectable2>> {
        // 只问节点一条消息：`exts` 里有没有 `SelectableExt`（TS `asSelectable2`）。
        // 富文本与编辑器因此走同一条收集路径，不需要各写一份分支。
        as_selectable2(node)
    }
    fn is_selectable(&self, sel: &Rc<dyn Selectable2>) -> bool {
        self.selection.is_selectable_sel(sel)
    }
    fn focus(&self, node: Option<&Node2>) {
        match node {
            Some(n) => engine_global::request_focus(n),
            None => engine_global::clear_focus(),
        }
    }
    fn has_active_editor(&self) -> bool {
        engine_global::active_editor_ext().is_some()
    }
    fn on_editor_focus(&self) {
        // 先把 `Rc` clone 出来再回调：`if let Some(f) = x.borrow().clone()` 里的
        // 临时 `Ref` 在 edition 2021 下活到整个 `if let` 结束，回调里一旦再
        // `borrow_mut()` 同一格（重设 redraw 方案）就是 `RefCell already borrowed`。
        let redraw = self.redraw.borrow().clone();
        if let Some(f) = redraw {
            f();
        }
    }
    fn dispatch(&self, init: &MouseDispatchInit) -> bool {
        let path = hit_path(&self.root, init.global_x, init.global_y);
        // 命中链的捕获/冒泡顺序由 `dispatch_mouse_path` 负责（纯消息）；
        // **全局监听不受命中限制**，按类型在这里广播——拖拽会话（分割条 / 滚动条
        // 拇指）靠它持续收到 move/up，指针移出那几像素的命中区也不会断。
        // 位置与 TS `Renderer.ts` 的 `dispatch(init)` 逐字对应。
        let consumed = dispatch_mouse_path(&path, init);
        match init.type_ {
            MouseType2::Down => dispatch_global_mouse_down(init.global_x, init.global_y),
            MouseType2::Move => dispatch_global_mouse_move(init.global_x, init.global_y),
            MouseType2::Up | MouseType2::Cancel => dispatch_global_mouse_up(),
            _ => {}
        }
        consumed
    }
    fn set_pressed(&self, node: Option<&Node2>) {
        engine_global::set_pressed(node);
    }
    fn pointer_select(&self) -> Option<PointerSelect2> {
        self.pointer_select.get()
    }
    fn set_pointer_select(&self, s: Option<PointerSelect2>) {
        self.pointer_select.set(s);
    }
    fn move_hit_test(&self) -> Option<HitestResult2> {
        self.move_hit_test.get()
    }
    fn set_move_hit_test(&self, h: Option<HitestResult2>) {
        self.move_hit_test.set(h);
    }
    fn select_range(&self, sel: &Rc<dyn Selectable2>, start: usize, end: usize) {
        // 路由决策在管理器内部（TS `SelectionManager2.select` 同位置）：
        // 活跃编辑器在时写进它的本地光标，否则物化成 programmatic 选区。
        // 这里**不再**自己判"有没有编辑器"——那会把这个决定拆成两处，
        // 且"编辑器聚焦时点别的组件"会被错写进编辑器（跨组件选择失效）。
        self.selection.select_by_sel(sel, start, sel, end);
    }
    fn cursor(&self, t: CursorType) {
        self.cursor.set(t);
    }
}

/// Node2 原生应用：一棵 `Node2` + 指针/键盘路由，直接实现 [`WyApp`]。
///
/// 用法：
/// ```ignore
/// let app = Node2App::new(move |viewport| {
///     // viewport 是 `Rc<Cell<(f32,f32)>>`，根节点的 width/height 读它（getter，
///     // 每次布局现读——先 `let w = ..` 再塞进去会丢掉响应性）
///     build_root_fc(node2! {
///         x: 0.0,
///         y: 0.0,
///         width: viewport.get().0,
///         height: viewport.get().1,
///         layout: flex! { direction: Layout::Column },
///         children: vec![header(), list(), toolbar()],
///     })
/// });
/// wy_engine::runner::run(app)
/// ```
pub struct Node2App {
    root: Node2,
    viewport: Rc<Cell<(f32, f32)>>,
    router: PointerRouter2<Node2Deps>,
    redraw: Rc<RefCell<Option<Rc<dyn Fn()>>>>,
    /// 全局唯一的选中协调者（TS `Renderer.ts` 同位置）。
    selection: Rc<SelectionManager2>,
    /// 输入法组合进行中：键盘归输入法独占（`route_key_event` 会整条放弃）。
    composing: Cell<bool>,
    /// 提交守护窗口：丢掉选词那一枚按键的另一半（见 [`crate::ime_guard`]）。
    ime_guard: ImeCommitGuard,
    /// 下一次光标闪烁唤醒时刻（见 [`CARET_BLINK_MS`]）。
    blink_at: Cell<Option<Instant>>,
    shift: Cell<bool>,
    last_cursor: Cell<(f64, f64)>,
    title: String,
    size: (f32, f32),
}

impl Node2App {
    /// 用工厂闭包构建根节点。`viewport` 在 resize 时被更新，根节点的
    /// `width`/`height` 应读取它（见模块文档示例）。
    pub fn new(factory: impl FnOnce(Rc<Cell<(f32, f32)>>) -> Node2) -> Self {
        let viewport = Rc::new(Cell::new((800.0, 600.0)));
        let root = factory(viewport.clone());
        let redraw: Rc<RefCell<Option<Rc<dyn Fn()>>>> = Rc::new(RefCell::new(None));
        // 选择会话的两份原始事实（选区由 SelectionManager 纯派生，与 TS 同）。
        let pointer_select: Signal<Option<PointerSelect2>> = create_signal(None);
        let move_hit_test: Signal<Option<HitestResult2>> = create_signal(None);
        let selection = Rc::new(SelectionManager2::new(Rc::new(Node2SelectionSource {
            pointer_select,
            move_hit_test,
            root: root.clone(),
        })
            as Rc<dyn SelectionSource2>));
        // 组件构造期拿不到 `ctx`：把"当前渲染器装的管理器"放进线程局部，
        // 子树里的 `richTextNode` 就消费它（TS `selectionManagerContext` 的落点）。
        wy_mve::canvas_render2::selection::set_current_selection_manager(Some(Rc::clone(
            &selection,
        )));
        let deps = Node2Deps {
            root: root.clone(),
            redraw: redraw.clone(),
            pointer_select,
            move_hit_test,
            cursor: Cell::new(CursorType::Default),
            selection: Rc::clone(&selection),
        };
        let router = create_pointer_router(deps);
        Self {
            root,
            viewport,
            router,
            redraw,
            selection,
            composing: Cell::new(false),
            ime_guard: ImeCommitGuard::new(),
            blink_at: Cell::new(None),
            shift: Cell::new(false),
            last_cursor: Cell::new((0.0, 0.0)),
            title: "wy-ui · node2".to_string(),
            size: (800.0, 600.0),
        }
    }

    /// 全局选择器（对标 TS `renderCanvas` 返回值里的 `selectionManager`）。
    ///
    /// 宿主可以据此 `select_all()` / `selected_text()` / `selected_rect()`。
    #[must_use]
    pub fn selection_manager(&self) -> Rc<SelectionManager2> {
        Rc::clone(&self.selection)
    }

    /// 设置初始窗口尺寸（逻辑像素）。
    pub fn with_size(mut self, w: f32, h: f32) -> Self {
        self.size = (w, h);
        self.viewport.set((w, h));
        self
    }

    /// 设置窗口标题。
    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = title.into();
        self
    }

    /// 画布根节点（调试 / 测试用：检查挂载结果、算命中链）。
    #[must_use]
    pub fn root(&self) -> &Node2 {
        &self.root
    }

    fn redraw(&self) {
        // 同 `on_editor_focus`：clone 出 `Rc` 再调，别把 `Ref` 带进回调。
        let redraw = self.redraw.borrow().clone();
        if let Some(f) = redraw {
            f();
        }
    }
}

impl WyApp for Node2App {
    fn draw(&self, scene: &mut Scene, width: f32, height: f32) {
        self.viewport.set((width, height));
        // 布局 memo 按帧失效：一帧内同一 (节点, 轴) 只构造一次布局对象，跨帧
        // 重算（信号/视口都可能变）。与 TS `layoutX()` 的 memo 同语义，见
        // `canvas_render2::tree::geometry::create_layout`。
        wy_mve::canvas_render2::tree::geometry::clear_layout_memo();
        // 动态区域（`for_each_node` / `if_node`）：脏了的先重算、把这一段的
        // 子节点就地换掉，再绘制。这一步必须在追踪上下文里（runner 已包好），
        // 区域内读到的信号才会连到重绘。
        engine_global::probe_regions();
        self.root.render_node2(scene);
    }

    fn setup(&mut self, request_redraw: Rc<dyn Fn()>) {
        self.redraw.borrow_mut().replace(request_redraw);
    }

    fn handle_event(&mut self, event: &WindowEvent) -> bool {
        match event {
            WindowEvent::ModifiersChanged(mods) => {
                let m = crate::winit_translate::translate_modifiers(&mods.state());
                let ctrl = m & 0x2 != 0;
                let shift = m & 0x1 != 0;
                let alt = m & 0x4 != 0;
                let meta = m & 0x8 != 0;
                engine_global::apply_modifiers(ctrl, shift, alt, meta);
                self.shift.set(shift);
                false
            }
            WindowEvent::CursorMoved { position, .. } => {
                let (x, y) = (position.x as f64, position.y as f64);
                self.last_cursor.set((x, y));
                self.router.move_(&PointerInput2 {
                    x,
                    y,
                    button: 0,
                    shift: self.shift.get(),
                    time: now_ms(),
                    device: PointerDevice::Mouse,
                    pointer_id: 0,
                });
                self.redraw();
                false
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let b = match button {
                    MouseButton::Left => POINTER_BUTTON_LEFT,
                    MouseButton::Right => POINTER_BUTTON_RIGHT,
                    _ => 0,
                };
                if b == 0 {
                    return false;
                }
                let (x, y) = self.last_cursor.get();
                let input = PointerInput2 {
                    x,
                    y,
                    button: b,
                    shift: self.shift.get(),
                    time: now_ms(),
                    device: PointerDevice::Mouse,
                    pointer_id: 0,
                };
                match state {
                    ElementState::Pressed => {
                        // TS 的 `mouseClick` 在**按下**时就派发（不是抬起时），
                        // 与 v1 `MveApp::handle_event` 的 `dispatch_click_chain` 一致。
                        // `down` 内部已发 Down + 焦点 + 选择会话，这里补发 Click，
                        // 连击数 >=2 再补发 DoubleClick（双击不吞单击）。
                        let (_, count) = self.router.down(&input);
                        self.router.click(x, y);
                        if count >= 2 {
                            self.router.double_click(x, y);
                        }
                    }
                    ElementState::Released => {
                        self.router.up(&input);
                    }
                }
                self.redraw();
                false
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let (dx, dy) = match delta {
                    MouseScrollDelta::LineDelta(_, y) => (0.0, *y as f64),
                    MouseScrollDelta::PixelDelta(p) => (p.x as f64, p.y as f64),
                };
                let (x, y) = self.last_cursor.get();
                self.router.wheel(x, y, dx, dy, Some(PointerDevice::Mouse));
                self.redraw();
                false
            }
            _ => false,
        }
    }

    fn on_resize(&mut self, width: f32, height: f32) {
        self.viewport.set((width, height));
        self.redraw();
    }

    fn handle_key_event(&mut self, event: &EngineKeyEvent) {
        let mve = MveKeyEvent {
            key: to_mve_key(&event.key),
            ctrl: event.ctrl,
            shift: event.shift,
            alt: event.alt,
            meta: event.meta,
        };
        // 输入法刚提交：紧随其后的字符键是**同一枚按键的另一半**
        // （Windows 上"空格选词"会在 Commit 之后补一条 `Character(" ")`），
        // 组合态此时已经结束，只能在应用层丢掉。
        if matches!(mve.key, MveKey::Char(_)) && self.ime_guard.swallow() {
            return;
        }
        let ke2 = KeyEvent2::from_mve(&mve);
        route_key_event(self, &ke2);
        self.redraw();
    }

    fn handle_ime_event(&mut self, event: &ImeEvent) {
        // 组合态**先**更新，且不依赖"有没有活跃编辑器"：编辑器句柄丢了还把
        // `composing` 卡在 true，键盘会永久失灵（Tab / 退格都没反应）。
        match event {
            ImeEvent::Preedit { text, .. } => {
                // winit 只在组合结束 / 取消时发空串 Preedit
                // （`WM_IME_COMPOSITION` 的 `Preedit(String::new(), None)`），
                // 组合中的每一帧都是非空组合串。
                self.composing.set(!text.is_empty());
            }
            ImeEvent::Commit(_) => {
                self.composing.set(false);
                self.ime_guard.arm();
            }
            ImeEvent::Disabled => self.composing.set(false),
            ImeEvent::Enabled => {}
        }
        if let Some(ed) = focus::active_editor() {
            match event {
                ImeEvent::Preedit { text, cursor } => {
                    let c =
                        cursor.map_or(text.chars().count(), |(_, en)| byte_to_char_idx(text, en));
                    ed.on_composing(text, c);
                }
                ImeEvent::Commit(t) => ed.commit_composing_text(t),
                ImeEvent::Enabled | ImeEvent::Disabled => {}
            }
            self.redraw();
        }
    }

    fn ime_allowed(&self) -> Option<bool> {
        // 有活跃编辑器才允许 IME（与 TS 编辑器聚焦时启用输入法一致）。
        focus::active_editor_ext().is_some().then_some(true)
    }

    /// IME 候选窗光标盒：焦点节点的局部光标盒 + 节点绝对位置。
    ///
    /// 对齐 TS `EditableEditor.inputOverlay` / Kotlin `requestInputOverlay`：
    /// 候选框跟着光标走。`Node2` 的 `abs_x/abs_y` 是沿父链累加的绝对坐标，
    /// 所以不需要 v1 `MveApp::ime_cursor_area` 那套"逐级累加 layout_boxes"。
    ///
    /// 缺这段实现时 `WyApp` 的默认实现返回 `None`，winit 收不到
    /// `set_ime_cursor_area` → 候选框不跟随（"输入法提示框没有聚集"）。
    fn ime_cursor_area(&self) -> Option<(f32, f32, f32, f32)> {
        let node = engine_global::focused_node()?;
        let (x, y, w, h) = node.ime_cursor_rect()?;
        Some((node.abs_x() + x, node.abs_y() + y, w, h))
    }

    fn window_size(&self) -> (f32, f32) {
        self.size
    }

    fn window_title(&self) -> &str {
        &self.title
    }

    /// 光标闪烁：焦点在编辑器里时，按 [`CARET_BLINK_MS`] 排下一次唤醒。
    ///
    /// 编辑器只在 `draw` 里决定光标亮不亮（`(elapsed / 530) % 2 == 0`）。没有
    /// 定时重绘的话，画面会停在上一次的相位上——正好是"灭"就无法解释地看不见
    /// 光标（"点了输入框却没有光标"）。TS 侧靠 `repeatEvery` 驱动，这里靠事件
    /// 循环的定时唤醒，语义相同。
    ///
    /// 动画（浮层滚动条淡出等）走同一套：组件通过 `FrameTick` 登记"下一帧几点"，
    /// 这里与光标相位取较早者——TS 那边就是同一个 `requestAnimationFrame` 队列。
    fn next_wake(&self) -> Option<Instant> {
        let blink = if focus::active_editor_ext().is_some() {
            let now_ms = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |d| d.as_millis() as u64);
            // 醒在相位边界上：否则每次都在同一个相位里重绘，光标永远不翻转。
            let remain = CARET_BLINK_MS - now_ms % CARET_BLINK_MS;
            let at = Instant::now() + Duration::from_millis(remain);
            self.blink_at.set(Some(at));
            Some(at)
        } else {
            self.blink_at.set(None);
            None
        };
        let anim = time_until_next_frame().map(|d| Instant::now() + d);
        match (blink, anim) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }

    fn on_wake(&mut self) {
        // `next_wake` 每次都把截止时刻推到未来，所以这里只会在"到点"时触发一次；
        // 少了这层判断，每次 `about_to_wait` 都会重绘，变成忙循环。
        let blink_due = self.blink_at.get().is_some_and(|at| Instant::now() >= at);
        if blink_due {
            self.blink_at.set(None);
        }
        // 动画到点：摘牌并重绘一帧（`Fade` 读时会按墙钟算出新进度）。
        let frame_due = take_due_frame();
        if blink_due || frame_due {
            self.redraw();
        }
    }
}

impl KeyRouterDeps for Node2App {
    fn composing(&self) -> bool {
        self.composing.get()
    }
    fn active_editor(&self) -> Option<Rc<dyn KeyTarget2>> {
        focus::active_editor().map(|h| Rc::new(EditorKeyTarget(h)) as Rc<dyn KeyTarget2>)
    }
    fn focused(&self) -> Option<Node2> {
        engine_global::focused_node()
    }
    fn select_all(&self) {
        // TS `Renderer.ts` 的 `keyDeps.selectAll`：有活跃编辑器就写进它，
        // 否则交给选择器全选——**能跨组件**（列表里 Ctrl+A 选中所有可选文本）。
        if let Some(e) = focus::active_editor() {
            e.select_all();
        } else {
            self.selection.select_all();
        }
    }
    fn copy(&self) {
        if let Some(e) = focus::active_editor() {
            e.copy();
        } else if let Some(t) = self.selection.selected_text() {
            wy_mve::canvas_render2::clipboard::clipboard_set_text(&t);
        }
    }
    fn cut(&self) {
        if let Some(e) = focus::active_editor() {
            e.cut();
        } else if let Some(t) = self.selection.selected_text() {
            // TS 同：非编辑器场景只复制，不删源文本（可选文本是只读的）。
            wy_mve::canvas_render2::clipboard::clipboard_set_text(&t);
        }
    }
    fn paste_pending(&self) {
        if let Some(e) = focus::active_editor() {
            e.paste();
        }
    }
    fn move_focus(&self, forward: bool) -> bool {
        let cur = engine_global::focused_node();
        if let Some(next) = next_focus_node(&self.root, cur.as_ref(), forward) {
            engine_global::request_focus(&next);
            self.redraw();
            true
        } else {
            false
        }
    }
    fn notify_unhandled(&self, event: &KeyEvent2) {
        engine_global::notify_key_press(event);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wy_mve::canvas_render2::build_root_node2;
    use wy_mve::canvas_render2::node2::Node2Props;
    use wy_mve::editable_text_node;
    use wy_signal::{GetValue, SetValue};

    /// 一棵只有单行编辑器的树（60 高，好让"文字被推到中间"看得出来）。
    fn app_with_editor() -> (Node2App, Node2) {
        let app = Node2App::new(|_vp| {
            build_root_node2(
                Node2Props {
                    // 根必须有显式的 x/y：缺了它 `default_position` 会去找"父节点"，
                    // 根没有父，直接 panic（真实应用里 `root_arg` 同样写 0,0）。
                    x: Some(wy_mve::canvas_render2::node2::always(0.0)),
                    y: Some(wy_mve::canvas_render2::node2::always(0.0)),
                    width: Some(wy_mve::canvas_render2::node2::always(400.0)),
                    height: Some(wy_mve::canvas_render2::node2::always(300.0)),
                    ..Node2Props::default()
                },
                |cx| {
                    cx.child(editable_text_node! {
                        text: "ab".to_string(),
                        on_text_change: |_t: &str| {},
                        single_line: true,
                        width: 200.0,
                        height: 60.0,
                    });
                },
            )
        });
        let editor = app
            .root()
            .children()
            .first()
            .cloned()
            .expect("编辑器应挂在根下");
        (app, editor)
    }

    /// 全局鼠标广播必须发生在路由的 `dispatch` 里（对标 TS `Renderer.ts`）。
    ///
    /// 分割条 / 滚动条拇指这类 6px 宽的可拖拽物，靠的就是"指针离开命中区之后
    /// 仍能收到 move/up"。广播一旦漏掉，它们在真实窗口里就完全拖不动——而节点
    /// 本地 `mouse_move` 是沿命中链派的，救不了。
    #[test]
    fn dispatch_broadcasts_global_mouse_even_without_hit() {
        use wy_mve::canvas_render2::node2::always;
        use wy_mve::canvas_render2::protocol::MouseType2;
        use wy_mve::canvas_render2::renderer::{register_move, register_up};
        use wy_mve::canvas_render2::tree::dispatch::mouse_dispatch_of;

        let root = Node2::empty().build(Node2Props {
            x: Some(always(0.0)),
            y: Some(always(0.0)),
            width: Some(always(100.0)),
            height: Some(always(100.0)),
            ..Node2Props::default()
        });
        let selection = Rc::new(SelectionManager2::new(Rc::new(Node2SelectionSource {
            pointer_select: create_signal(None),
            move_hit_test: create_signal(None),
            root: root.clone(),
        })
            as Rc<dyn SelectionSource2>));
        let deps = Node2Deps {
            root,
            redraw: Rc::new(RefCell::new(None)),
            pointer_select: create_signal(None),
            move_hit_test: create_signal(None),
            cursor: Cell::new(CursorType::Default),
            selection,
        };

        let seen = Rc::new(RefCell::new(Vec::<String>::new()));
        let s = Rc::clone(&seen);
        let off_move = register_move(Rc::new(move |x, y| {
            s.borrow_mut().push(format!("move:{x},{y}"));
        }));
        let s = Rc::clone(&seen);
        let off_up = register_up(Rc::new(move || s.borrow_mut().push("up".into())));

        // 坐标远在根之外：命中链为空，但全局广播与命中无关，照样要发出来。
        deps.dispatch(&mouse_dispatch_of(MouseType2::Move, 5000.0, 5000.0));
        deps.dispatch(&mouse_dispatch_of(MouseType2::Up, 5000.0, 5000.0));
        assert_eq!(
            seen.borrow().clone(),
            vec!["move:5000,5000".to_string(), "up".to_string()],
            "Move/Up 必须在 dispatch 里广播到全局注册表"
        );

        // 点击类消息不广播 move/up（TS 同样只认 down / move / up / cancel）。
        deps.dispatch(&mouse_dispatch_of(MouseType2::Click, 5000.0, 5000.0));
        assert_eq!(seen.borrow().len(), 2, "Click 不应触发全局 move/up");

        off_move();
        off_up();
    }

    /// IME 候选窗位置：焦点节点局部光标盒 + 绝对位置。
    ///
    /// 这条链路缺了 `ime_cursor_area` 覆盖时，`WyApp` 默认实现返回 `None`，
    /// winit 收不到 `set_ime_cursor_area` → 候选框不跟随光标（"提示框没聚集"）。
    #[test]
    fn ime_cursor_area_tracks_the_focused_editor() {
        let (app, editor) = app_with_editor();
        assert_eq!(app.ime_cursor_area(), None, "无焦点时不应有候选框位置");

        focus::request_focus(&editor);
        assert_eq!(app.ime_allowed(), Some(true), "编辑器聚焦后应允许 IME");

        // 定位光标，`caret_local_rect` 才有真实矩形
        focus::active_editor()
            .expect("聚焦后应取到编辑器句柄")
            .select_range(1, 1);

        let (x, y, w, h) = app.ime_cursor_area().expect("聚焦编辑器后应有候选框位置");
        let (lx, ly, lw, lh) = editor.ime_cursor_rect().expect("编辑器应带 IME 光标盒");
        assert_eq!(
            (x, y),
            (editor.abs_x() + lx, editor.abs_y() + ly),
            "全局 = 绝对原点 + 局部盒"
        );
        assert_eq!((w, h), (lw, lh));
        assert!(w > 0.0 && h > 0.0, "候选框尺寸应为正，实际 ({w}, {h})");
        // 60 高的单行框：候选框应被墨迹补偿推到中部，而不是贴在盒顶
        assert!(y > 10.0, "候选框应跟随到盒子中部，实际 y={y}");
        assert!(y + h <= 60.0, "候选框不应超出节点，实际 y={y} h={h}");
    }

    /// 同一棵单行编辑器，但文本真相源是一枚信号（编辑器把改写回写进来），
    /// 测试据此断言"到底写没写进正文"。
    fn app_with_capture() -> (Node2App, Node2, wy_signal::Signal<String>) {
        let value = wy_signal::create_signal(String::new());
        // `Signal` 是 `Copy`，直接按值捕获
        let text = value;
        let app = Node2App::new(|_vp| {
            build_root_node2(
                Node2Props {
                    x: Some(wy_mve::canvas_render2::node2::always(0.0)),
                    y: Some(wy_mve::canvas_render2::node2::always(0.0)),
                    width: Some(wy_mve::canvas_render2::node2::always(400.0)),
                    height: Some(wy_mve::canvas_render2::node2::always(300.0)),
                    ..Node2Props::default()
                },
                move |cx| {
                    cx.child(editable_text_node! {
                        text: value.get(),
                        on_text_change: move |t: &str| text.set(t.to_string()),
                        single_line: true,
                        width: 200.0,
                        height: 24.0,
                    });
                },
            )
        });
        let editor = app
            .root()
            .children()
            .first()
            .cloned()
            .expect("编辑器应挂在根下");
        focus::request_focus(&editor);
        (app, editor, value)
    }

    fn key(c: char) -> EngineKeyEvent {
        EngineKeyEvent::new(crate::event::Key::Char(c), true)
    }

    /// 组合串进行中：键盘归输入法独占，字母不能进正文。
    ///
    /// 平台侧的同类防护在 `winit_translate`（`text` 为空 = 被 IME 吃掉）；
    /// 这里守住的是"已经收到组合态"之后的按键。
    #[test]
    fn composing_blocks_plain_letters() {
        let (mut app, _editor, value) = app_with_capture();
        app.handle_ime_event(&ImeEvent::Preedit {
            text: "zhong".into(),
            cursor: None,
        });
        assert!(KeyRouterDeps::composing(&app), "收到组合串后应处于组合态");
        // 组合串会先"临时上屏"（编辑器负责画它并回写），这是对的
        assert_eq!(value.get(), "zhong");

        app.handle_key_event(&key('z'));
        assert_eq!(value.get(), "zhong", "组合中的字母不能写进正文");
    }

    /// 空串 `Preedit` = 平台在说"组合结束"。
    ///
    /// winit 在 `WM_IME_COMPOSITION` 里用 `Preedit(String::new(), None)` 表示组合
    /// 收尾；若仍把 `composing` 留在 true，键盘会永久失灵（退格、Tab 全都没反应）。
    #[test]
    fn empty_preedit_ends_composing() {
        let (mut app, _editor, _value) = app_with_capture();
        app.handle_ime_event(&ImeEvent::Preedit {
            text: "zhong".into(),
            cursor: None,
        });
        assert!(KeyRouterDeps::composing(&app));
        app.handle_ime_event(&ImeEvent::Preedit {
            text: String::new(),
            cursor: None,
        });
        assert!(!KeyRouterDeps::composing(&app), "空串 Preedit 应结束组合态");
    }

    /// 空格选词：`Commit` 之后补到的那枚空格不能进正文，但之后正常的空格照常输入。
    #[test]
    fn space_used_to_confirm_a_candidate_is_swallowed() {
        let (mut app, _editor, value) = app_with_capture();
        app.handle_ime_event(&ImeEvent::Preedit {
            text: "zhong".into(),
            cursor: None,
        });
        app.handle_ime_event(&ImeEvent::Commit("中".into()));
        assert_eq!(value.get(), "中", "提交的组合串应写进正文");
        assert!(!KeyRouterDeps::composing(&app), "提交后组合态结束");

        // 有的输入法会把"确认用的那一次空格"作为普通字符再上报一遍
        app.handle_key_event(&key(' '));
        assert_eq!(value.get(), "中", "选词的空格不能进正文");

        // 守护窗口（30ms）过后，用户真敲的空格必须照常输入
        std::thread::sleep(Duration::from_millis(40));
        app.handle_key_event(&key(' '));
        assert_eq!(value.get(), "中 ", "正常空格要能输入");
    }

    /// 聚焦编辑器时排闪烁唤醒；没有活跃编辑器就不排（不做无谓的后台唤醒）。
    #[test]
    fn caret_blink_schedules_wakes_only_while_editing() {
        let (app, editor, _value) = app_with_capture();
        let at = app.next_wake().expect("编辑器聚焦时应按 530ms 排唤醒");
        let wait = at.saturating_duration_since(Instant::now());
        assert!(
            wait <= Duration::from_millis(CARET_BLINK_MS),
            "唤醒间隔不应超过闪烁周期，实际 {wait:?}"
        );

        focus::clear_focus();
        assert_eq!(app.next_wake(), None, "没有编辑器时不应排唤醒");
        let _ = editor;
    }

    /// 编辑器自己也有选区高亮：`Node2App` 的线程局部管理器必须被它用上。
    ///
    /// 这条盯的是重构后的回归——编辑器此前只在**显式注入** `selection_manager`
    /// 时才绘制高亮，而经 `Node2App` 建树时没人能注入（管理器在 App 里），
    /// 于是"选中了，屏幕上一片安静"。管理器改为用点惰性解析后应当有底色。
    #[test]
    fn editor_selection_paints_its_highlight() {
        use wy_mve::Color;

        let (app, editor) = app_with_editor();
        focus::request_focus(&editor);
        focus::active_editor()
            .expect("聚焦后应取到编辑器句柄")
            .select_range(0, 2);

        let mut scene = Scene::new();
        app.draw(&mut scene, 400.0, 300.0);

        let mut fills = 0;
        for p in scene.iter() {
            if let wy_render::Primitive::Rect { color, .. } = p {
                // 编辑器默认底色（TS `editable/params.ts`：rgba(80,140,255,80)）
                if *color == Color::rgba(80, 140, 255, 80) {
                    fills += 1;
                }
            }
        }
        assert!(fills >= 1, "编辑器选区应画出高亮，实际 {fills} 块");
    }

    /// **跨组件选择**端到端：从 A 富文本拖到 B 富文本，两个节点都要画出高亮。
    ///
    /// 这条测试把真实链路整条压进去跑：`router.down/move/up` → `hit_path`
    /// → 节点 `exts` 里的 `Selectable2` → `SelectionManager2` 的信号派生
    /// → 富文本 `draw` 读回范围铺底色。此前"跨组件选择不行"的两个根因
    /// （节点自装拖选只能同节点内选；派生源用 `RefCell` 让 memo 看不见变化）
    /// 都在这条链路上，断任何一环这里都会红。
    #[test]
    fn dragging_across_two_rich_texts_paints_both_highlights() {
        use wy_mve::canvas_render2::build_root_fc;
        use wy_mve::canvas_render2::rich_text::RichTextSpan;
        use wy_mve::{flex, rect_node, rich_text_node, AlignItem, Color, Layout};

        // 两层结构：根 → 面板（x=20/y=15、padding 10、flex 列）→ 两块富文本。
        // 刻意多套一层容器：绝对坐标要沿父链累加（含父 padding），这正是
        // `position_for_point` 早先读恒 0 的 `NodeGeom` 时错得最离谱的地方。
        // 面板内：A 的绝对 y = 15 + 10 = 25（高 24），B = 25 + 24 + 6 = 55。
        //
        // 写法与产品代码一致：一棵 `node2!` / `rect_node!` 声明树交根，
        // 不写 `Node2Props::builder()` 链（也是给后来者的范例）。
        let root = build_root_fc(rect_node! {
            x: 0.0,
            y: 0.0,
            width: 400.0,
            height: 200.0,
            children: vec![rect_node! {
                x: 20.0,
                y: 15.0,
                width: 360.0,
                height: 150.0,
                padding: 10.0,
                layout: flex! {
                    direction: Layout::Column,
                    align_item: AlignItem::Start,
                    gap: 6.0,
                },
                children: vec![
                    rich_text_node! {
                        width: 240.0,
                        height: 24.0,
                        spans: vec![RichTextSpan::text("hello", Color::BLACK)],
                    },
                    rich_text_node! {
                        width: 240.0,
                        height: 24.0,
                        spans: vec![RichTextSpan::text("world", Color::BLACK)],
                    },
                ],
            }],
        });

        // 模板里 `pointer_select` 对应 `Option<PointerSelect2>`，不是引用。
        let pointer_select: Signal<Option<PointerSelect2>> = create_signal(None);
        let move_hit_test: Signal<Option<HitestResult2>> = create_signal(None);
        let selection = Rc::new(SelectionManager2::new(Rc::new(Node2SelectionSource {
            pointer_select,
            move_hit_test,
            root: root.clone(),
        })
            as Rc<dyn SelectionSource2>));
        let deps = Node2Deps {
            root: root.clone(),
            redraw: Rc::new(RefCell::new(None)),
            pointer_select,
            move_hit_test,
            cursor: Cell::new(CursorType::Default),
            selection: Rc::clone(&selection),
        };
        // 真实应用里这一步在 `Node2App::new` 里做（TS `selectionManagerContext`
        // 的落点）：组件构造期拿不到 `ctx`，靠线程局部"当前管理器"消费。
        wy_mve::canvas_render2::selection::set_current_selection_manager(Some(Rc::clone(
            &selection,
        )));
        let router = create_pointer_router(deps);

        let input = |x: f64, y: f64| PointerInput2 {
            x,
            y,
            button: POINTER_BUTTON_LEFT,
            shift: false,
            time: 0.0,
            device: PointerDevice::Mouse,
            pointer_id: 0,
        };

        // 拖选：A 的首字 → B 的首字（跨两个组件，坐标全用**画布全局**）。
        // x=38 = 面板 20 + padding 10 + 8（落在 "hello" 首字内）。
        router.down(&input(38.0, 35.0));
        router.move_(&input(38.0, 65.0));
        router.up(&input(38.0, 65.0));

        let text = selection
            .selected_text()
            .expect("拖过两个组件后应有选中文本");
        assert!(
            text.contains('o') && text.contains('w'),
            "选区应跨 A 的尾部和 B 的头部，实际 {text:?}"
        );

        // 画：两个节点都必须铺出选中底色（默认 `Color::rgba(100,100,200,60)`）。
        let mut scene = Scene::new();
        root.render_node2(&mut scene);
        let mut fills = 0;
        for p in scene.iter() {
            if let wy_render::Primitive::Rect { color, .. } = p {
                if *color == Color::rgba(100, 100, 200, 60) {
                    fills += 1;
                }
            }
        }
        assert!(fills >= 2, "两个组件都要有高亮，实际 {fills} 块");
    }
}
