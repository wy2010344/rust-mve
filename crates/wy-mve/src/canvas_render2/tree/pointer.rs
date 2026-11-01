//! 指针交互状态机（TS `tree/pointer.ts`）。
//!
//! 职责：把"指针原始动作"翻译成"树上的语义"——更新 hover、派发指针消息、
//! 开/关选择会话、连击计数（双击选词 / 三击选段）、双击后的词级拖拽扩展、
//! 光标派生、按住键位掩码。
//!
//! 这一整块**没有任何宿主 API**：输入是 [`PointerInput2`]（x/y/button/shift/time/
//! device 几个纯字段），输出是对 [`PointerRouterDeps`] 里那组能力的调用。
//!
//! 与 [`crate::canvas_render2::tree::dispatch`] 的分工：那边只管"沿命中链按
//! 捕获→冒泡发消息"，这边管"什么时候该发、发给谁、附带什么状态"。

use std::cell::RefCell;
use std::rc::Rc;

use crate::canvas_render2::node2::Node2;
use crate::canvas_render2::protocol::{
    pointer_button_mask, MouseType2, PointerDevice, POINTER_BUTTON_LEFT, POINTER_BUTTON_RIGHT,
};
use crate::canvas_render2::selection::{
    expand_word_selection, HitestItem2, HitestResult2, PointerInput2, PointerSelect2, Selectable2,
};
use crate::canvas_render2::tree::dispatch::{cursor_of_path, MouseDispatchInit};
use crate::canvas_render2::tree::focus_walk::focusable_leaf_of;
use crate::canvas_render2::tree::geometry::NodeWithPosition2;
use crate::primitives::CursorType;

/// 连击判定窗口（毫秒）与位移容差（画布像素），对标 Kotlin `ClickTracker`。
pub const CLICK_INTERVAL: f64 = 400.0;
pub const CLICK_DISTANCE: f64 = 5.0;

/// 路由需要的外部能力（TS `PointerRouterDeps`）。
///
/// 全部是"函数"而不是对象字段访问 —— 状态机只**调用**它们，不关心背后是信号、
/// 变量还是别的东西。
pub trait PointerRouterDeps {
    /// 命中链（root→leaf，每节含相对自身坐标）；空数组表示未命中。
    fn hit(&self, x: f64, y: f64) -> Vec<NodeWithPosition2>;
    /// 该节点上的可选文本能力；非可选返回 `None`。
    fn selectable_of(&self, node: &Node2) -> Option<Rc<dyn Selectable2>>;
    /// 该可选对象当前是否在册（在树上、未隐藏、未被剪枝）。
    fn is_selectable(&self, sel: &Rc<dyn Selectable2>) -> bool;
    /// 聚焦到该节点（`None` = 清除焦点）。
    fn focus(&self, node: Option<&Node2>);
    /// 焦点是否落在编辑器上（是则宿主需要接管真实输入焦点）。
    fn has_active_editor(&self) -> bool;
    /// 编辑器接管后的钩子（宿主同步 IME 焦点）。
    fn on_editor_focus(&self);
    /// 派发指针消息（内部走 `dispatchMousePath`）；返回是否被 `stopPropagation` 中断。
    fn dispatch(&self, init: &MouseDispatchInit) -> bool;
    /// 按下 / 抬起 / 离开时同步按住态信号（供绘制层做按压反馈）。
    fn set_pressed(&self, node: Option<&Node2>);
    /// 当前指针会话。
    fn pointer_select(&self) -> Option<PointerSelect2>;
    /// 写入指针会话。
    fn set_pointer_select(&self, s: Option<PointerSelect2>);
    /// 按住中的实时命中快照。
    fn move_hit_test(&self) -> Option<HitestResult2>;
    /// 写入实时命中快照。
    fn set_move_hit_test(&self, hit: Option<HitestResult2>);
    /// 程序化选区（写进选择器）。
    fn select_range(&self, sel: &Rc<dyn Selectable2>, start: usize, end: usize);
    /// 光标派生结果交给宿主落到 UI 上。
    fn cursor(&self, t: CursorType);
}

/// 连击计数：同一叶子、窗口内、位移容差内则递增，否则重置为 1
/// （TS `createClickCounter`）。
///
/// 与 `renderer::create_click_counter` 的两点差别（后者是 v1 遗留，不动）：
/// - `time` 由**宿主注入**（`PointerInput2.time`），不用 `Instant::now()`——
///   TS 原文如此，便于测试与移植时替换时钟；
/// - `leaf` 为 `None` 时**照样**记录（`last.node = null`），下一次有叶子时
///   不会误判成"同一节点"。
#[derive(Default)]
struct ClickCounter {
    last: Option<ClickRecord>,
}

struct ClickRecord {
    node: Option<Node2>,
    x: f64,
    y: f64,
    time: f64,
    count: u32,
}

impl ClickCounter {
    fn record(&mut self, leaf: Option<&Node2>, x: f64, y: f64, time: f64) -> u32 {
        let prev = self.last.as_ref();
        let count = match prev {
            Some(p)
                if leaf.is_some_and(|l| Some(l) == p.node.as_ref())
                    && (x - p.x).abs() < CLICK_DISTANCE
                    && (y - p.y).abs() < CLICK_DISTANCE
                    && time - p.time < CLICK_INTERVAL =>
            {
                p.count + 1
            }
            _ => 1,
        };
        self.last = Some(ClickRecord {
            node: leaf.cloned(),
            x,
            y,
            time,
            count,
        });
        count
    }
}

/// 双击选词后的词级拖拽会话（同一节点内生效，松手即结束）。
struct WordDrag {
    sel: Rc<dyn Selectable2>,
    start: usize,
    end: usize,
}

/// 状态机的可变部分。
struct RouterState {
    word_drag: Option<WordDrag>,
    /// 当前按住的按键位掩码。
    ///
    /// 由状态机自己维护而不直接透传平台掩码：各平台位序不同（DOM / AWT 各一套），
    /// 而且"按下 / 抬起成对"只有引擎知道；节点侧只需认识一套编码。
    held_buttons: i32,
    /// 最近一次输入的设备类型（`click` / `wheel` 这类无设备参数的入口沿用）。
    last_device: PointerDevice,
    last_pointer_id: i32,
    click: ClickCounter,
}

impl Default for RouterState {
    fn default() -> Self {
        Self {
            word_drag: None,
            held_buttons: 0,
            last_device: PointerDevice::Mouse,
            last_pointer_id: 0,
            click: ClickCounter::default(),
        }
    }
}

/// 指针路由（TS `PointerRouter2`）。
pub struct PointerRouter2<D: PointerRouterDeps> {
    deps: Rc<D>,
    state: RefCell<RouterState>,
}

/// 造一个指针路由（TS `createPointerRouter`）。
#[must_use]
pub fn create_pointer_router<D: PointerRouterDeps>(deps: D) -> PointerRouter2<D> {
    PointerRouter2 {
        deps: Rc::new(deps),
        state: RefCell::new(RouterState::default()),
    }
}

impl<D: PointerRouterDeps> PointerRouter2<D> {
    /// 命中快照：命中链非空才算命中（TS `hitAt`）。
    fn hit_at(&self, x: f64, y: f64, time: f64) -> Option<HitestResult2> {
        let path = self.deps.hit(x, y);
        if path.is_empty() {
            return None;
        }
        Some(HitestResult2 {
            chain: path
                .iter()
                .map(|it| HitestItem2 {
                    node: it.node.clone(),
                    x: f64::from(it.x),
                    y: f64::from(it.y),
                })
                .collect(),
            x,
            y,
            time,
        })
    }

    fn leaf_node_of(&self, hit: Option<&HitestResult2>) -> Option<Node2> {
        hit.and_then(|h| h.chain.last().map(|it| it.node.clone()))
    }

    /// 命中链中最深的、在册的可选文本节点（TS `selectableLeaf`）。
    fn selectable_leaf(&self, hit: Option<&HitestResult2>) -> Option<Rc<dyn Selectable2>> {
        let hit = hit?;
        for it in hit.chain.iter().rev() {
            if let Some(sel) = self.deps.selectable_of(&it.node) {
                if self.deps.is_selectable(&sel) {
                    return Some(sel);
                }
            }
        }
        None
    }

    /// 组装一次派发的入参（TS `initOf`）。
    ///
    /// 坐标精度说明：TS 只有一种 `number`。Rust 侧指针会话这一层
    /// （[`PointerInput2`] / [`HitestResult2`]）用 `f64`，而协议层 [`MouseDispatchInit`]
    /// 跟着 [`crate::canvas_render2::protocol::MouseEvent2`] 用 `f32`——所以这里
    /// 是两侧唯一的精度收敛点。
    fn init_of(
        &self,
        type_: MouseType2,
        x: f64,
        y: f64,
        button: i32,
        wheel_delta_x: f64,
        wheel_delta_y: f64,
    ) -> MouseDispatchInit {
        let st = self.state.borrow();
        MouseDispatchInit {
            type_,
            global_x: x as f32,
            global_y: y as f32,
            device: st.last_device,
            pointer_id: st.last_pointer_id,
            button,
            buttons: st.held_buttons,
            wheel_delta_x: wheel_delta_x as f32,
            wheel_delta_y: wheel_delta_y as f32,
        }
    }

    /// 普通派发（无滚轮量，TS `fire`）。
    fn fire(&self, type_: MouseType2, x: f64, y: f64, button: i32) -> bool {
        let init = self.init_of(type_, x, y, button, 0.0, 0.0);
        self.deps.dispatch(&init)
    }

    /// 按下：返回 `(take_over, click_count)`。
    ///
    /// - `take_over`：宿主是否需要 `preventDefault`（焦点落到编辑器）；
    /// - `click_count`：本次按下构成的连击数（1=单击，2=双击，3=三击），
    ///   供宿主在按下后合成 `Click` / `DoubleClick`（TS 的 `mouseClick` 在**按下**
    ///   时就派发，不是抬起时——见 `MveApp::handle_event` 的 `dispatch_click_chain`）。
    pub fn down(&self, input: &PointerInput2) -> (bool, u32) {
        {
            let mut st = self.state.borrow_mut();
            st.last_device = input.device;
            st.last_pointer_id = input.pointer_id;
            st.held_buttons |= pointer_button_mask(input.button);
        }
        // 先刷新 hover 命中（任何按键都更新，含非主键——与 Kotlin mouseDown 顺序一致）
        let hit = self.hit_at(input.x, input.y, input.time);
        self.deps.set_move_hit_test(hit.clone());

        if input.button != POINTER_BUTTON_LEFT {
            // 非左键不派发 down：否则右键按下会顺带启动文本拖选、滑块拖动等
            // 左键专属行为，退化成"假左键"。右键只派发上下文菜单。
            if input.button == POINTER_BUTTON_RIGHT {
                self.fire(MouseType2::ContextMenu, input.x, input.y, input.button);
            }
            return (false, 0);
        }
        let leaf = self.leaf_node_of(hit.as_ref());
        // 焦点先于 down 与选择两步：
        // ① 节点的 mouseDown 能读到 isFocused()，编辑器由此接管光标；
        // ② 双击 / 三击要把选区写进**编辑器本地光标**，那要求 activeEditor 此刻已更新
        //    ——否则 SelectionManager.select 发现目标不是活跃编辑器，直接返回 false。
        //    （Kotlin 把 setFocused 放在 select 之后，这里刻意提前，是 TS 侧的修正）
        let focus_target = hit.as_ref().and_then(|h| focusable_leaf_of(&h.chain));
        self.deps.focus(focus_target.as_ref());
        let take_over = self.deps.has_active_editor();
        if take_over {
            self.deps.on_editor_focus();
        }
        self.deps.set_pressed(leaf.as_ref());

        let count = {
            let mut st = self.state.borrow_mut();
            st.word_drag = None;
            st.click.record(leaf.as_ref(), input.x, input.y, input.time)
        };

        if count >= 3 {
            // 三击选段：一次性物化整个逻辑段落（'\n' 分隔），不开拖拽会话
            if let Some(sel) = self.selectable_leaf(hit.as_ref()) {
                let off = sel.position_for_point(input.x, input.y);
                // 分段能力是能力对象自己的消息（不具备时兜底返回 None）
                let range = sel
                    .paragraph_range_at(off)
                    .unwrap_or_else(|| (0, sel.text_length()));
                if range.1 > range.0 {
                    self.deps.select_range(&sel, range.0, range.1);
                }
            }
            // down 必须在选区物化**之后**派发：编辑器在 mouseDown 里读 rangeOf(self)，
            // 看到非塌缩选区就返回（不抢光标），三击选段才不会被随后的 down 抹掉。
            self.fire(MouseType2::Down, input.x, input.y, input.button);
            return (take_over, count);
        }

        if count == 2 {
            // 双击选词：物化整词；词拖扩展由 move 驱动，松手即结束
            if let Some(sel) = self.selectable_leaf(hit.as_ref()) {
                let off = sel.position_for_point(input.x, input.y);
                match sel.word_range_at(off) {
                    Some(word) if word.1 > word.0 => {
                        self.deps.select_range(&sel, word.0, word.1);
                        self.state.borrow_mut().word_drag = Some(WordDrag {
                            sel: Rc::clone(&sel),
                            start: word.0,
                            end: word.1,
                        });
                    }
                    // 无分词能力时退化为选中整个节点文本
                    _ => self.deps.select_range(&sel, 0, sel.text_length()),
                }
            }
            self.fire(MouseType2::Down, input.x, input.y, input.button);
            return (take_over, count);
        }

        // 开启选择会话；Shift + 按下复用上一会话的 press（锚点继承，焦点重新跟随移动）
        //
        // 顺序上必须**先建会话再派发 down**（对齐 Kotlin Renderer.mouseDown）：
        // 否则编辑器在 mouseDown 里经 rangeOf(self) 读到的是上一次定格的旧选区。
        let prev = self.deps.pointer_select();
        let session = if input.shift {
            prev.map_or_else(
                || PointerSelect2 {
                    press: hit.clone(),
                    release: None,
                },
                |p| PointerSelect2 {
                    press: p.press,
                    release: None,
                },
            )
        } else {
            PointerSelect2 {
                press: hit.clone(),
                release: None,
            }
        };
        self.deps.set_pointer_select(Some(session));
        self.fire(MouseType2::Down, input.x, input.y, input.button);
        (take_over, count)
    }

    /// 移动。
    pub fn move_(&self, input: &PointerInput2) {
        {
            let mut st = self.state.borrow_mut();
            st.last_device = input.device;
            st.last_pointer_id = input.pointer_id;
        }
        let hit = self.hit_at(input.x, input.y, input.time);
        // 跨节点拖拽选择靠 moveHitTest 定位指针下的目标文本节点
        self.deps.set_move_hit_test(hit.clone());

        // 双击后的词级拖拽扩展：以锚词为基准按词粒度扩展（仅同节点内生效）
        let drag_on_hit = {
            let st = self.state.borrow();
            match (&st.word_drag, &hit) {
                (Some(drag), Some(h)) => h
                    .chain
                    .iter()
                    .any(|it| {
                        self.deps
                            .selectable_of(&it.node)
                            .is_some_and(|s| Rc::ptr_eq(&s, &drag.sel))
                    })
                    .then(|| (Rc::clone(&drag.sel), drag.start, drag.end)),
                _ => None,
            }
        };
        if let Some((sel, start, end)) = drag_on_hit {
            let off = sel.position_for_point(input.x, input.y);
            let range = expand_word_selection(start, end, off, |o| sel.word_range_at(o));
            if range.1 > range.0 {
                self.deps.select_range(&sel, range.0, range.1);
            }
        }

        // 光标随命中链派生（编辑器 → text，可聚焦 → pointer）
        self.deps
            .cursor(cursor_of_path(hit.as_ref().map(|h| h.chain.as_slice())));
        self.fire(MouseType2::Move, input.x, input.y, 0);
    }

    /// 抬起。
    pub fn up(&self, input: &PointerInput2) {
        {
            let mut st = self.state.borrow_mut();
            st.last_device = input.device;
            st.last_pointer_id = input.pointer_id;
            st.held_buttons &= !pointer_button_mask(input.button);
            st.word_drag = None;
        }
        let hit = self.hit_at(input.x, input.y, input.time);
        let s = self.deps.pointer_select();
        // 松手落空（画布空白）时用最后一次有效 hover / 按下点兜底：
        // release 留 None 会被派生链当作"仍在按住中"，导致松手后不按键移动仍改选区
        let release = hit
            .clone()
            .or_else(|| self.deps.move_hit_test())
            .or_else(|| s.as_ref().and_then(|p| p.press.clone()));
        self.deps.set_move_hit_test(hit);
        self.deps.set_pressed(None);
        // 松手定格：填入 release 后 hover 不再影响选区（纯数据变化，无命令）
        if let Some(s) = s {
            self.deps.set_pointer_select(Some(PointerSelect2 {
                press: s.press,
                release,
            }));
        }
        self.fire(MouseType2::Up, input.x, input.y, input.button);
    }

    /// 指针离开画布。
    pub fn leave(&self) {
        // 按住拖出画布：把当前进度定格（选区保持连续），再清理 hover 位置
        if let Some(s) = self.deps.pointer_select() {
            if s.release.is_none() {
                self.deps.set_pointer_select(Some(PointerSelect2 {
                    release: self.deps.move_hit_test().or_else(|| s.press.clone()),
                    press: s.press,
                }));
            }
        }
        let mut st = self.state.borrow_mut();
        st.word_drag = None;
        // 按住状态无法再收到配对的 up（窗口外松手），一并复位防残留
        st.held_buttons = 0;
        drop(st);
        self.deps.set_pressed(None);
        self.deps.set_move_hit_test(None);
        self.deps.cursor(CursorType::Default);
    }

    /// 滚轮：返回 `true` 表示节点已消费（宿主不必再交给页面滚动）。
    pub fn wheel(
        &self,
        x: f64,
        y: f64,
        delta_x: f64,
        delta_y: f64,
        device: Option<PointerDevice>,
    ) -> bool {
        if let Some(d) = device {
            self.state.borrow_mut().last_device = d;
        }
        // 滚轮不改 hover 快照：它不改变"指针在哪"，只借命中链找接收者。
        let init = self.init_of(MouseType2::Wheel, x, y, 0, delta_x, delta_y);
        self.deps.dispatch(&init)
    }

    /// 指针被系统取消（触摸被打断等）：按抬起收尾，并清掉会话。
    pub fn cancel(&self, x: f64, y: f64, device: Option<PointerDevice>, time: Option<f64>) {
        if let Some(d) = device {
            self.state.borrow_mut().last_device = d;
        }
        let t = time.unwrap_or_else(now_ms);
        let hit = self.hit_at(x, y, t);
        self.deps.set_pressed(None);
        self.deps.set_pointer_select(None);
        self.deps.set_move_hit_test(hit);
        let mut st = self.state.borrow_mut();
        st.word_drag = None;
        st.held_buttons = 0;
        drop(st);
        self.fire(MouseType2::Cancel, x, y, 0);
    }

    /// 单击（宿主在确认是单击后调用）。
    pub fn click(&self, x: f64, y: f64) {
        self.fire(MouseType2::Click, x, y, 0);
    }

    /// 双击。
    pub fn double_click(&self, x: f64, y: f64) {
        self.fire(MouseType2::DoubleClick, x, y, 0);
    }
}

/// 当前时间（毫秒）。TS 的 `Date.now()`，只在 `cancel` 没给 `time` 时兜底用
/// —— 正常路径的时间戳由宿主经 [`PointerInput2::time`] 注入。
fn now_ms() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0.0, |d| d.as_millis() as f64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canvas_render2::geometry::RectF;
    use crate::canvas_render2::node2::{Node2, Node2Props};
    use crate::canvas_render2::protocol::{
        POINTER_BUTTON_LEFT, POINTER_LEFT_DOWN_MASK, POINTER_RIGHT_DOWN_MASK,
    };
    use crate::canvas_render2::tree::geometry::hit_path;
    use crate::{Fc, NodeContext};
    use std::collections::HashMap;

    // ---- 一个够用的可选文本实现 ----

    struct TextSel {
        text: String,
    }

    impl Selectable2 for TextSel {
        fn selection_rect(&self) -> Option<RectF> {
            None
        }
        fn text_length(&self) -> usize {
            self.text.chars().count()
        }
        fn position_for_point(&self, _gx: f64, _gy: f64) -> usize {
            0
        }
        fn text_in_range(&self, start: usize, end: usize) -> String {
            self.text.chars().skip(start).take(end - start).collect()
        }
        /// 按空格分词（"hello world" → [0,5) / [6,11)）
        fn word_range_at(&self, offset: usize) -> Option<(usize, usize)> {
            let words: Vec<(usize, usize)> = self
                .text
                .split_whitespace()
                .scan(0, |acc, w| {
                    let s = *acc;
                    *acc += w.chars().count() + 1;
                    Some((s, s + w.chars().count()))
                })
                .collect();
            words.into_iter().find(|(s, e)| offset >= *s && offset < *e)
        }
        fn paragraph_range_at(&self, _offset: usize) -> Option<(usize, usize)> {
            Some((0, self.text.chars().count()))
        }
    }

    // ---- 记录型 deps ----

    struct TestDeps {
        root: Node2,
        /// node id → 可选文本能力
        selectables: HashMap<u64, Rc<dyn Selectable2>>,
        log: RefCell<Vec<String>>,
        dispatched: RefCell<Vec<(MouseType2, i32)>>,
        pointer_select: RefCell<Option<PointerSelect2>>,
        move_hit: RefCell<Option<HitestResult2>>,
        ranges: RefCell<Vec<(usize, usize)>>,
        cursor: RefCell<Option<CursorType>>,
        editor: bool,
    }

    impl TestDeps {
        fn new(root: Node2) -> Self {
            Self {
                root,
                selectables: HashMap::new(),
                log: RefCell::new(Vec::new()),
                dispatched: RefCell::new(Vec::new()),
                pointer_select: RefCell::new(None),
                move_hit: RefCell::new(None),
                ranges: RefCell::new(Vec::new()),
                cursor: RefCell::new(None),
                editor: false,
            }
        }
    }

    impl PointerRouterDeps for TestDeps {
        fn hit(&self, x: f64, y: f64) -> Vec<NodeWithPosition2> {
            hit_path(&self.root, x as f32, y as f32)
        }
        fn selectable_of(&self, node: &Node2) -> Option<Rc<dyn Selectable2>> {
            self.selectables.get(&node.id()).cloned()
        }
        fn is_selectable(&self, _sel: &Rc<dyn Selectable2>) -> bool {
            true
        }
        fn focus(&self, node: Option<&Node2>) {
            self.log
                .borrow_mut()
                .push(format!("focus:{}", node.map_or(-1, |n| n.id() as i64)));
        }
        fn has_active_editor(&self) -> bool {
            self.editor
        }
        fn on_editor_focus(&self) {
            self.log.borrow_mut().push("editorFocus".into());
        }
        fn dispatch(&self, init: &MouseDispatchInit) -> bool {
            self.dispatched
                .borrow_mut()
                .push((init.type_, init.buttons));
            false
        }
        fn set_pressed(&self, node: Option<&Node2>) {
            self.log
                .borrow_mut()
                .push(format!("pressed:{}", node.map_or(-1, |n| n.id() as i64)));
        }
        fn pointer_select(&self) -> Option<PointerSelect2> {
            self.pointer_select.borrow().clone()
        }
        fn set_pointer_select(&self, s: Option<PointerSelect2>) {
            let tag = match &s {
                None => "null".to_string(),
                Some(p) => format!(
                    "press:{},release:{}",
                    p.press.as_ref().map_or(-1.0, |h| h.x),
                    p.release.as_ref().map_or(-1.0, |h| h.x)
                ),
            };
            self.log.borrow_mut().push(format!("select:{tag}"));
            *self.pointer_select.borrow_mut() = s;
        }
        fn move_hit_test(&self) -> Option<HitestResult2> {
            self.move_hit.borrow().clone()
        }
        fn set_move_hit_test(&self, hit: Option<HitestResult2>) {
            *self.move_hit.borrow_mut() = hit;
        }
        fn select_range(&self, _sel: &Rc<dyn Selectable2>, start: usize, end: usize) {
            self.ranges.borrow_mut().push((start, end));
        }
        fn cursor(&self, t: CursorType) {
            *self.cursor.borrow_mut() = Some(t);
        }
    }

    fn leaf(w: f32, h: f32) -> Node2Props {
        Node2Props {
            width: Some(Rc::new(move || w)),
            height: Some(Rc::new(move || h)),
            ..Node2Props::default()
        }
    }

    fn attach_child(parent: &Node2, arg: Node2Props) {
        let mut cx = NodeContext::new();
        cx.node2_parent = Some(parent.clone());
        Fc::new(move |cx| {
            Node2::empty().build(arg).mount(cx);
        })
        .mount(&mut cx);
    }

    fn input(x: f64, y: f64, button: i32, time: f64) -> PointerInput2 {
        PointerInput2 {
            x,
            y,
            button,
            shift: false,
            time,
            device: PointerDevice::Mouse,
            pointer_id: 0,
        }
    }

    /// 一棵 200×200 的根，里面一个可聚焦的 100×100 子节点（原点 (0,0)）。
    fn tree() -> (Node2, Node2) {
        let mut root_a = leaf(200.0, 200.0);
        root_a.x = Some(Rc::new(|| 0.0));
        root_a.y = Some(Rc::new(|| 0.0));
        let root = Node2::empty().build(root_a);
        let mut child = leaf(100.0, 100.0);
        child.x = Some(Rc::new(|| 0.0));
        child.y = Some(Rc::new(|| 0.0));
        child.focusable = Rc::new(|| true);
        attach_child(&root, child);
        let c = root.children()[0].clone();
        (root, c)
    }

    #[test]
    fn down_orders_focus_before_session_before_dispatch() {
        let (root, child) = tree();
        let deps = TestDeps::new(root.clone());
        let router = create_pointer_router(deps);
        let (r, count) = router.down(&input(10.0, 10.0, POINTER_BUTTON_LEFT, 0.0));
        assert!(!r);
        assert_eq!(count, 1, "首次按下是单击");
        let log = router.deps.log.borrow().clone();
        assert_eq!(
            log,
            vec![
                format!("focus:{}", child.id()),
                format!("pressed:{}", child.id()),
                // 会话的 press.x = 10，release 还没定格（-1）
                "select:press:10,release:-1".to_string(),
            ]
        );
        // down 在会话建立**之后**才派发
        assert_eq!(
            router.deps.dispatched.borrow().clone(),
            vec![(MouseType2::Down, POINTER_LEFT_DOWN_MASK)]
        );
    }

    #[test]
    fn non_left_button_never_dispatches_down() {
        let (root, _child) = tree();
        let router = create_pointer_router(TestDeps::new(root));
        // 右键：只派发 contextmenu，不派发 down
        router.down(&input(10.0, 10.0, POINTER_BUTTON_RIGHT, 0.0));
        assert_eq!(
            router.deps.dispatched.borrow().clone(),
            vec![(MouseType2::ContextMenu, POINTER_RIGHT_DOWN_MASK)]
        );
    }

    #[test]
    fn double_click_selects_word_and_triple_selects_paragraph() {
        let (root, child) = tree();
        let mut deps = TestDeps::new(root);
        deps.selectables.insert(
            child.id(),
            Rc::new(TextSel {
                text: "hello world".into(),
            }),
        );
        let router = create_pointer_router(deps);

        // 同一叶子、近距、窗口内 → 连击递增
        router.down(&input(10.0, 10.0, POINTER_BUTTON_LEFT, 0.0));
        router.up(&input(10.0, 10.0, POINTER_BUTTON_LEFT, 10.0));
        router.down(&input(11.0, 11.0, POINTER_BUTTON_LEFT, 100.0)); // count = 2 → 选词
        assert_eq!(
            router.deps.ranges.borrow().clone(),
            vec![(0, 5)],
            "双击选中整词 hello"
        );
        router.up(&input(11.0, 11.0, POINTER_BUTTON_LEFT, 110.0));
        router.down(&input(12.0, 12.0, POINTER_BUTTON_LEFT, 200.0)); // count = 3 → 选段
        assert_eq!(
            router.deps.ranges.borrow().clone(),
            vec![(0, 5), (0, 11)],
            "三击选中整个段落"
        );
    }

    #[test]
    fn click_counter_resets_on_move_or_other_node() {
        let (root, child) = tree();
        let mut deps = TestDeps::new(root.clone());
        deps.selectables.insert(
            child.id(),
            Rc::new(TextSel {
                text: "hello world".into(),
            }),
        );
        let router = create_pointer_router(deps);

        router.down(&input(10.0, 10.0, POINTER_BUTTON_LEFT, 0.0));
        router.up(&input(10.0, 10.0, POINTER_BUTTON_LEFT, 10.0));
        // 位移超过容差 → 重置为 1，不该触发选词
        router.down(&input(100.0, 100.0, POINTER_BUTTON_LEFT, 20.0));
        assert!(
            router.deps.ranges.borrow().is_empty(),
            "位移过远 → 连击重置，不进选词分支"
        );
        let _ = child;
    }

    #[test]
    fn up_freezes_release_and_clears_pressed() {
        let (root, _child) = tree();
        let router = create_pointer_router(TestDeps::new(root));
        router.down(&input(10.0, 10.0, POINTER_BUTTON_LEFT, 0.0));
        router.up(&input(20.0, 20.0, POINTER_BUTTON_LEFT, 10.0));
        let s = router.deps.pointer_select.borrow().clone().unwrap();
        assert!(
            s.release.is_some(),
            "松手必须填 release，否则派生链会当成仍在按住"
        );
        assert_eq!(s.release.unwrap().x, 20.0);
        // 抬起时按住掩码已把左键摘掉
        assert_eq!(
            router.deps.dispatched.borrow().last().unwrap().1,
            0,
            "up 派发时 buttons 已不含刚松开的左键"
        );
    }

    #[test]
    fn move_derives_cursor_and_does_not_touch_selection() {
        let (root, _child) = tree();
        let router = create_pointer_router(TestDeps::new(root));
        router.move_(&input(10.0, 10.0, 0, 0.0));
        // 命中链里有可聚焦节点 → pointer
        assert_eq!(*router.deps.cursor.borrow(), Some(CursorType::Pointer));
        assert_eq!(
            router.deps.dispatched.borrow().clone(),
            vec![(MouseType2::Move, 0)]
        );
    }

    #[test]
    fn leave_freezes_session_and_resets_held_buttons() {
        let (root, _child) = tree();
        let router = create_pointer_router(TestDeps::new(root));
        router.down(&input(10.0, 10.0, POINTER_BUTTON_LEFT, 0.0));
        router.leave();
        let s = router.deps.pointer_select.borrow().clone().unwrap();
        assert!(s.release.is_some(), "按住拖出画布要把进度定格");
        assert_eq!(router.state.borrow().held_buttons, 0, "窗口外松手收不到 up");
    }

    #[test]
    fn wheel_does_not_change_hover_snapshot() {
        let (root, _child) = tree();
        let router = create_pointer_router(TestDeps::new(root));
        router.move_(&input(10.0, 10.0, 0, 0.0));
        let before = router.deps.move_hit.borrow().as_ref().map(|h| h.x);
        let consumed = router.wheel(10.0, 10.0, 0.0, 30.0, None);
        assert!(!consumed, "宿主给 false 表示未被 stopPropagation 中断");
        let after = router.deps.move_hit.borrow().as_ref().map(|h| h.x);
        assert_eq!(before, after, "滚轮不改 hover 快照");
        assert_eq!(
            router.deps.dispatched.borrow().last().unwrap().0,
            MouseType2::Wheel
        );
    }

    #[test]
    fn cancel_drops_session_and_dispatches_cancel() {
        let (root, _child) = tree();
        let router = create_pointer_router(TestDeps::new(root));
        router.down(&input(10.0, 10.0, POINTER_BUTTON_LEFT, 0.0));
        router.cancel(10.0, 10.0, None, Some(50.0));
        assert!(
            router.deps.pointer_select.borrow().is_none(),
            "取消要清掉会话"
        );
        assert_eq!(
            router.deps.dispatched.borrow().last().unwrap().0,
            MouseType2::Cancel
        );
    }

    #[test]
    fn click_and_double_click_go_through_dispatch() {
        let (root, _child) = tree();
        let router = create_pointer_router(TestDeps::new(root));
        router.click(10.0, 10.0);
        router.double_click(10.0, 10.0);
        assert_eq!(
            router.deps.dispatched.borrow().clone(),
            vec![(MouseType2::Click, 0), (MouseType2::DoubleClick, 0)]
        );
    }
}
