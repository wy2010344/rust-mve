//! 跨组件选区管理器（对标 TS `canvasRender2/SelectionManager.ts`）。
//!
//! # 模型（与 TS 逐句一致）：纯派生，无反向写回
//!
//! "什么地方被选中了"完全由**初始原因**计算得出，本类型没有 `setSelection`
//! 推送入口：
//!
//! - **指针会话**：[`SelectionSource2::pointer_select`]（press/release 快照）+
//!   [`SelectionSource2::move_hit_test`] 推导两端。按住中焦点跟随移动；
//!   `release` 填入即定格（hover 自然失效）；press 落在可选文本之外 → 整个
//!   选区为空（浏览器语义：点击空白清除）；
//! - **键盘选择**：活跃编辑器的光标端点对直接派生（canvasRender2 暂无编辑器）；
//! - **程序化指令**（[`Self::select_all`] / [`Self::select`] / [`Self::clear`]）：
//!   物化成一个 `programmatic` 信号，任何更高优先级的来源出现即自动失效。
//!
//! 以前这里是"节点注册 `get`/`clear` 贡献者 + 拼接文本"：那份实现不可能支持
//! 跨节点连续拖选（每个节点只知道自己），且管理器持有的是副本而不是真相。
//! 现在对齐 TS：节点只提供坐标换算与文本查询（[`Selectable2`]），范围分配由
//! [`Self::range_of`] 按文档序纯派生。
//!
//! # 可选能力怎么挂到节点上（与 TS 同一套）
//!
//! TS 用 `exts` 里的 [`SelectableExt`] **标记**声明"我有这个能力"，外面一律经
//! `asSelectable2(node)` 问节点要——节点只回答 `findExt` 一条消息，不往节点结构体
//! 上加字段（强类型语言没法这么做）。Rust 照搬：
//!
//! - 组件把 `Rc<SelectableExt>` 塞进 `Node2Props::exts`；
//! - [`as_selectable2`] = `node.find_ext_rc::<SelectableExt>()`，与 TS 的
//!   `node.findExt(SelectableExt)?.impl` 逐字对应；
//! - 管理器**不持注册表**：可选集合每次从渲染树前序遍历派生，节点销毁（移出树）
//!   或隐藏即自动出局，不存在需要生命周期配对的注销调用。
//!
//! 以前这里是"节点创建时 `bind(id, sel)` + 内部 `Rc` 表 + 遍历时按 id 取回"。
//! 那份实现有两个真问题：注册发生在**构造期**而树是稍后挂载的（构造期绑的节点
//! 可能已经被替换），且管理器多了一份与树不同步的真相。改成 ext 标记后，
//! "谁在树上、谁可选"只有树这一份事实。

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use wy_signal::{create_memo, create_signal, GetValue, Memo, SetValue, Signal};

use crate::canvas_render2::geometry::RectF;
use crate::canvas_render2::node2::Node2;

/// 选中矩形（全局坐标，对标 TS `Rect2`）。
pub type Rect2 = RectF;

/// 可选文本能力：实现它的对象参与选区分配（对标 TS `Selectable2`）。
///
/// 节点是**纯数据面**：只提供坐标换算与文本查询，选区真相由
/// [`SelectionManager2`] 持有；节点按需读 [`SelectionManager2::range_of`]
/// 并绘制高亮。
pub trait Selectable2 {
    /// 选中区域（全局坐标），供 popover 定位；无选区返回 `None`。
    fn selection_rect(&self) -> Option<Rect2>;
    /// 文本总长度（越界钳制 / 全选）。
    fn text_length(&self) -> usize;
    /// 全局坐标 → 本节点文本偏移（跨节点拖拽时由目标节点换算）。
    fn position_for_point(&self, global_x: f64, global_y: f64) -> usize;
    /// 读取指定范围文本（`selected_text` 聚合拼接用）。
    fn text_in_range(&self, start: usize, end: usize) -> String;
    /// 包含 `offset` 的词边界（半开区间），供双击选词；无法分词返回 `None`。
    fn word_range_at(&self, offset: usize) -> Option<(usize, usize)> {
        let _ = offset;
        None
    }
    /// 包含 `offset` 的逻辑段落范围（半开，`'\n'` 分隔），供三击选段。
    fn paragraph_range_at(&self, offset: usize) -> Option<(usize, usize)> {
        let _ = offset;
        None
    }
}

/// 可选文本扩展：挂在节点 `exts` 上的标记载体（对标 TS `SelectableExt`）。
///
/// 节点不把 `position_for_point` / `text_in_range` 这些挂到自己身上（那会让节点
/// 结构体长出一堆可选字段），而是把能力对象塞进 `exts`，用本类型这个**标记**
/// 告诉外面"我有这个能力"。
///
/// ```ignore
/// let mut props = Node2Props::default();
/// props.exts.push(Rc::new(SelectableExt(sel)));
/// ```
pub struct SelectableExt(pub Rc<dyn Selectable2>);

/// 从节点的 `exts` 取可选能力（对标 TS `asSelectable2`）。
///
/// 与 TS 的差别只在"怎么认出这个扩展"：TS 是 `instanceof`（结构判定），
/// Rust 是 `find_ext_rc::<SelectableExt>`（类型标记）。两者都只问节点一条消息。
#[must_use]
pub fn as_selectable2(node: &Node2) -> Option<Rc<dyn Selectable2>> {
    node.find_ext_rc::<SelectableExt>().map(|e| Rc::clone(&e.0))
}

// 当前生效的选择管理器（TS 的 `selectionManagerContext` 的 Rust 落点）。
//
// TS 用 `ctx.provide/consume` 做**作用域内**的隐式传递；Rust 的 Node2 组件
// 构造期拿不到 `ctx`（`#[rec]` 组件只有参数），于是把"当前渲染器装的那个
// 管理器"放在线程局部——单线程 UI 下语义等价：`renderCanvas` 装了什么，
// 子树里的 `richTextNode` 就消费什么。不装也能用：组件退化为"没有高亮"。
thread_local! {
    static CURRENT_MANAGER: RefCell<Option<Rc<SelectionManager2>>> = RefCell::new(None);
}

/// 装上当前渲染器的选择管理器（渲染器启动时调用一次）。
pub fn set_current_selection_manager(manager: Option<Rc<SelectionManager2>>) {
    CURRENT_MANAGER.with(|c| *c.borrow_mut() = manager);
}

/// 取当前渲染器的选择管理器。
#[must_use]
pub fn current_selection_manager() -> Option<Rc<SelectionManager2>> {
    CURRENT_MANAGER.with(|c| c.borrow().clone())
}

/// 选区端点：某个可选节点内的偏移位置（对标 TS `SelPoint2`）。
pub struct SelPoint2 {
    /// 端点所在的可选节点（身份按注册对象判定）。
    pub node: Rc<dyn Selectable2>,
    /// 文本偏移。
    pub offset: usize,
}

impl SelPoint2 {
    /// 构造端点。
    #[must_use]
    pub fn new(node: Rc<dyn Selectable2>, offset: usize) -> Self {
        Self { node, offset }
    }
}

impl Clone for SelPoint2 {
    fn clone(&self) -> Self {
        Self {
            node: Rc::clone(&self.node),
            offset: self.offset,
        }
    }
}

/// 身份判定：端点"同一个可选节点"按注册对象指针比（TS 直接比对象引用）。
impl PartialEq for SelPoint2 {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.node, &other.node) && self.offset == other.offset
    }
}

impl Eq for SelPoint2 {}

impl std::fmt::Debug for SelPoint2 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SelPoint2")
            .field("offset", &self.offset)
            .finish_non_exhaustive()
    }
}

/// 一对端点：锚点与焦点（对标 TS `SelPair2`）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SelPair2 {
    /// 锚点（不动的那一端）。
    pub anchor: SelPoint2,
    /// 焦点（跟随指针/光标移动的那一端）。
    pub focus: SelPoint2,
}

impl SelPair2 {
    /// 构造端点对。
    #[must_use]
    pub fn new(anchor: SelPoint2, focus: SelPoint2) -> Self {
        Self { anchor, focus }
    }
}

/// 命中链上的一项（对标 TS `HitestResult2['chain'][number]`）。
#[derive(Clone)]
pub struct HitestItem2 {
    /// 命中的节点。
    pub node: Node2,
    /// 该节点局部坐标系下的命中点横坐标。
    pub x: f64,
    /// 该节点局部坐标系下的命中点纵坐标。
    pub y: f64,
}

/// 身份 + 坐标都相等才算同一项（`Node2` 的相等是节点身份）。
impl PartialEq for HitestItem2 {
    fn eq(&self, other: &Self) -> bool {
        self.node == other.node && self.x == other.x && self.y == other.y
    }
}

/// 一次指针按下的命中快照（对标 TS `HitestResult2`）。
///
/// `chain` 为 root → leaf；`x` / `y` 为**全局（canvas）坐标**。
#[derive(Clone, Default)]
pub struct HitestResult2 {
    /// root → leaf 的命中链。
    pub chain: Vec<HitestItem2>,
    /// 全局横坐标。
    pub x: f64,
    /// 全局纵坐标。
    pub y: f64,
    /// 命中时刻（毫秒）。
    pub time: f64,
}

impl PartialEq for HitestResult2 {
    fn eq(&self, other: &Self) -> bool {
        self.chain == other.chain
            && self.x == other.x
            && self.y == other.y
            && self.time == other.time
    }
}

/// 一次指针选择会话（对标 TS `PointerSelect2`）。
///
/// - `release` 为 `None` 表示仍在按住中：焦点端点跟随 `moveHitTest`；
/// - `release` 非 `None` 表示已松手定格：焦点冻结在释放位置；
/// - Shift + 按下时引擎复用上一会话的 `press`（锚点继承）。
#[derive(Clone, Default, PartialEq)]
pub struct PointerSelect2 {
    /// 按下快照。
    pub press: Option<HitestResult2>,
    /// 释放快照。
    pub release: Option<HitestResult2>,
}

/// 宿主上报给指针路由的**纯数据**输入（TS `tree/interaction.ts` 的 `PointerInput2`）。
///
/// 有了这一层，指针状态机（连击 / 双击选词 / 三击选段 / 拖选会话）就不依赖
/// 任何宿主 API：宿主把它的指针事件填成这几个字段即可复用同一套规则。
///
/// 与 [`PointerSelect2`] / [`HitestResult2`] 同属"交互过程中的中间数据"，
/// TS 侧都在 `tree/interaction.ts`；Rust 侧与它们的落点保持一致（本文件）。
#[derive(Clone, Copy, Debug)]
pub struct PointerInput2 {
    /// 全局（画布）横坐标。
    pub x: f64,
    /// 全局（画布）纵坐标。
    pub y: f64,
    /// 触发本次事件的按键编号（**引擎编号**：1=左 / 2=中 / 3=右，0=无按键）。
    ///
    /// 宿主的原生编号各不相同（DOM 是 0=左 / 1=中 / 2=右，Android 是 1/2/3），
    /// **翻译由宿主适配器负责**——规则层只认引擎编号。
    pub button: i32,
    /// 按下时是否按住 Shift（锚点继承）。
    pub shift: bool,
    /// 事件时间戳（连击窗口判定）；由宿主注入，便于测试与移植时替换时钟。
    pub time: f64,
    /// 输入设备（鼠标 / 触摸 / 触控笔）。
    pub device: crate::canvas_render2::protocol::PointerDevice,
    /// 指针编号（多点触控区分；鼠标恒 0）。
    pub pointer_id: i32,
}

/// [`SelectionManager2`] 的派生源（对标 TS `SelectionSource2`）。
///
/// 由渲染器提供；测试可直接用信号模拟指针序列。
pub trait SelectionSource2 {
    /// 当前指针会话（`None` = 没有按下）。
    fn pointer_select(&self) -> Option<PointerSelect2>;
    /// 按住中的实时命中快照。
    fn move_hit_test(&self) -> Option<HitestResult2>;
    /// 活跃编辑器的光标端点对；canvasRender2 暂无编辑器，恒 `None`。
    fn active_editor_sel_pair(&self) -> Option<SelPair2> {
        None
    }
    /// 渲染树根（可选集合遍历用）。
    fn root_node(&self) -> Option<Node2>;
    /// 撤除指针会话（程序化指令落装前调用，保证生效）。
    fn clear_pointer_select(&self);
    /// 活跃编辑器（程序化选区的路由目标）；无则 `None`。
    ///
    /// 对标 TS `SelectionSource2.activeEditor()`。有这个入口，`select` /
    /// `selectAll` 才能像 TS 那样**在管理器内部**决定"写进编辑器本地光标"还是
    /// "物化成 programmatic"——而不是让渲染器在调用点按"有没有编辑器"猜。
    fn active_editor_route(&self) -> Option<Rc<dyn EditorSelectionRoute>> {
        None
    }
}

/// 程序化选区落到编辑器本地光标的最小契约（对标 TS `EditorSelectionRoute`）。
///
/// 和 TS 一样刻意只依赖 [`Selectable2`]，不反向依赖 `focus` 模块（避免循环
/// import 的 Rust 等价物：模块环依赖）。存在的理由是编辑器的选区真相在它
/// 自己的本地光标里：程序化指令不写进那里，就会被「塌缩光标」这条优先级
/// 压制，双击选词 / 三击选段永远不显示。
pub trait EditorSelectionRoute {
    /// 本路由对应的可选能力对象（`select` 用 `Rc::ptr_eq` 判"目标是不是编辑器"）。
    fn as_selectable(&self) -> Rc<dyn Selectable2>;
    /// 写入编辑器本地光标区间。
    fn select_range(&self, anchor_offset: usize, focus_offset: usize);
    /// 编辑器内全选。
    fn select_all(&self);
}

/// 双击后按住拖拽的词级扩展（纯函数，对标 TS `expandWordSelection`）。
///
/// 锚词 `[anchor_start, anchor_end)` 固定；目标偏移在锚词之外时，对应端吸附到
/// 目标处整词的边界；无分词能力时退化为字符粒度。
#[must_use]
pub fn expand_word_selection(
    anchor_start: usize,
    anchor_end: usize,
    target: usize,
    word_range_at: impl Fn(usize) -> Option<(usize, usize)>,
) -> (usize, usize) {
    if target < anchor_start {
        return (word_range_at(target).map_or(target, |w| w.0), anchor_end);
    }
    if target > anchor_end {
        return (anchor_start, word_range_at(target).map_or(target, |w| w.1));
    }
    (anchor_start, anchor_end)
}

/// 前序遍历收集可选节点：填充 `(文档序 id 列表, id → 可选能力)`。
///
/// **文档序 = 前序遍历序**，与 TS `collectSelectables` 一致。这里刻意不做任何
/// 排序：按 id 排序会把文档序打乱，"夹在锚点与焦点之间的节点整段选中"就会
/// 错落到别的节点上（id 是创建序，重建过的树与显示序并不一致）。
fn collect_selectables(
    node: &Node2,
    order: &mut Vec<u64>,
    out: &mut HashMap<u64, Rc<dyn Selectable2>>,
) {
    for child in node.children() {
        if !child.selection_enabled() {
            // 声明性退出选择：剪枝整棵子树（TS 同）。
            continue;
        }
        let id = child.id();
        // 可选节点内部仍可嵌套可选节点，所以命中后继续往下走。
        if let Some(sel) = as_selectable2(&child) {
            if !out.contains_key(&id) {
                order.push(id);
            }
            out.insert(id, sel);
        }
        collect_selectables(&child, order, out);
    }
}

/// SelectionManager：全局唯一的选中协调者（信号驱动，零命令式状态）。
pub struct SelectionManager2 {
    source: Rc<dyn SelectionSource2>,
    /// 全表范围分配（memo）：依赖指针会话 / 编辑器光标 / programmatic，
    /// 三者任一变化才重算。
    ranges: Memo<Vec<(u64, usize, usize)>>,
    /// 程序化指令的唯一落点（脉冲事件没有持续信号可供推导，物化成信号）。
    programmatic: Signal<Option<SelPair2>>,
}

impl SelectionManager2 {
    /// 新建管理器（`source` = TS `SelectionSource2`）。
    #[must_use]
    pub fn new(source: Rc<dyn SelectionSource2>) -> Self {
        let programmatic: Signal<Option<SelPair2>> = create_signal(None);
        // 派生链的入口 memo：`compute_ranges` 读取 source 的各信号与
        // `programmatic`，依赖在首次读取时自动收集（TS 的 memo 依赖同样靠读取建立）。
        let ranges = {
            let source = Rc::clone(&source);
            let programmatic = programmatic;
            create_memo(move || Self::compute_ranges(&*source, programmatic.get()))
        };
        Self {
            source,
            ranges,
            programmatic,
        }
    }

    /// 当前可选集合（文档序 id 列表 + id → 能力）。每次从树派生，无注册表。
    fn collect(&self) -> (Vec<u64>, HashMap<u64, Rc<dyn Selectable2>>) {
        let mut order = Vec::new();
        let mut out = HashMap::new();
        if let Some(root) = self.source.root_node() {
            collect_selectables(&root, &mut order, &mut out);
        }
        (order, out)
    }

    /// 当前可选集合（文档序）。供 `select_all` 之外的宿主逻辑使用。
    #[must_use]
    pub fn selectables(&self) -> Vec<Rc<dyn Selectable2>> {
        let (order, map) = self.collect();
        order
            .iter()
            .filter_map(|id| map.get(id).map(Rc::clone))
            .collect()
    }

    /// 对象当前是否在可选集合内（活性 + `selectionEnabled` 子树声明的统一判定）。
    #[must_use]
    pub fn is_selectable(&self, node: &Node2) -> bool {
        self.collect().1.contains_key(&node.id())
    }

    /// 能力对象当前是否在可选集合内（对标 TS `SelectionManager2.isSelectable(sel)`）。
    ///
    /// 指针路由手里只有能力对象（从命中链的节点 ext 取出来的），所以要有这一条。
    #[must_use]
    pub fn is_selectable_sel(&self, selectable: &Rc<dyn Selectable2>) -> bool {
        let (order, map) = self.collect();
        order
            .iter()
            .any(|id| map.get(id).is_some_and(|s| Rc::ptr_eq(s, selectable)))
    }

    /// 指定节点当前被分配的选中范围 `[start, end)`；`None` 表示未被选中。
    #[must_use]
    pub fn range_of(&self, selectable: &Rc<dyn Selectable2>) -> Option<(usize, usize)> {
        let (order, map) = self.collect();
        let id = order
            .iter()
            .find(|id| map.get(*id).is_some_and(|s| Rc::ptr_eq(s, selectable)))
            .copied()?;
        self.ranges
            .get()
            .into_iter()
            .find(|(n, _, _)| *n == id)
            .map(|(_, s, e)| (s, e))
    }

    /// 指定**节点**当前被分配的选中范围（`range_of` 的按节点入口）。
    ///
    /// 节点的 `Selectable2` 实现（如富文本的 `selection_rect`）手里有节点、
    /// 没有能力对象自己的 `Rc`，经这一条读范围即可，不必自建镜像。
    #[must_use]
    pub fn range_of_node(&self, node: &Node2) -> Option<(usize, usize)> {
        let (_, map) = self.collect();
        let id = node.id();
        if !map.contains_key(&id) {
            return None;
        }
        self.ranges
            .get()
            .into_iter()
            .find(|(n, _, _)| *n == id)
            .map(|(_, s, e)| (s, e))
    }

    /// 选中文本聚合：按文档序拼接各节点被分配片段（`null` → 空串）。
    #[must_use]
    pub fn selected_text(&self) -> Option<String> {
        let ranges = self.ranges.get();
        let (_, out) = self.collect();
        let mut text = String::new();
        for (id, s, e) in ranges {
            if let Some(sel) = out.get(&id) {
                text.push_str(&sel.text_in_range(s, e));
            }
        }
        if text.is_empty() {
            None
        } else {
            Some(text)
        }
    }

    /// 是否存在选中。
    #[must_use]
    pub fn has_selection(&self) -> bool {
        self.selected_text().is_some()
    }

    /// 选中区域的包围盒（供 popover 定位）：取所有选中片段的并集。
    #[must_use]
    pub fn selected_rect(&self) -> Option<Rect2> {
        let ranges = self.ranges.get();
        let (_, out) = self.collect();
        let mut result: Option<Rect2> = None;
        for (id, s, e) in ranges {
            if e <= s {
                continue;
            }
            let Some(sel) = out.get(&id) else { continue };
            let Some(rect) = sel.selection_rect() else {
                continue;
            };
            result = Some(match result {
                Some(prev) => RectF {
                    left: prev.left.min(rect.left),
                    top: prev.top.min(rect.top),
                    right: prev.right.max(rect.right),
                    bottom: prev.bottom.max(rect.bottom),
                },
                None => rect,
            });
        }
        result
    }

    // ---------- 程序化指令（脉冲事件的物化入口）----------

    /// 全选：从首个可选节点开头到末节点末尾；无可选节点时不做任何事。
    pub fn select_all(&self) {
        // 活跃编辑器存在时，全选写进编辑器本地光标（TS 同）。
        if let Some(ed) = self.source.active_editor_route() {
            ed.select_all();
            return;
        }
        let (order, out) = self.collect();
        let Some(first) = order.first().copied() else {
            return;
        };
        let Some(last) = order.last().copied() else {
            return;
        };
        let Some(a) = out.get(&first).map(Rc::clone) else {
            return;
        };
        let Some(b) = out.get(&last).map(Rc::clone) else {
            return;
        };
        // 新指令物化即撤除旧指针会话（含定格），否则冻结端点会压制本指令。
        let b_len = b.text_length();
        self.source.clear_pointer_select();
        self.programmatic.set(Some(SelPair2 {
            anchor: SelPoint2::new(a, 0),
            focus: SelPoint2::new(b, b_len),
        }));
    }

    /// 清除程序化选区（指针定格选区由下一次按下自动覆盖清除）。
    pub fn clear(&self) {
        self.programmatic.set(None);
    }

    /// 编程式设置任意选区区间；节点不在可选集合内时返回 `false`。
    pub fn select(
        &self,
        node: &Node2,
        anchor_offset: usize,
        focus_node: &Node2,
        focus_offset: usize,
    ) -> bool {
        let (_, out) = self.collect();
        let Some(a) = out.get(&node.id()).map(Rc::clone) else {
            return false;
        };
        let Some(b) = out.get(&focus_node.id()).map(Rc::clone) else {
            return false;
        };
        self.commit_select(&a, anchor_offset, &b, focus_offset)
    }

    /// 按**能力对象**设置选区（对标 TS `select(anchor, .., focus, ..)`）。
    ///
    /// 指针路由手里只有从节点 ext 取出的能力对象、没有 `Node2`，所以要有这一条；
    /// 双击选词 / 三击选段 / 拖选都走它。
    pub fn select_by_sel(
        &self,
        anchor: &Rc<dyn Selectable2>,
        anchor_offset: usize,
        focus: &Rc<dyn Selectable2>,
        focus_offset: usize,
    ) -> bool {
        if !self.is_selectable_sel(anchor) || !self.is_selectable_sel(focus) {
            return false;
        }
        self.commit_select(anchor, anchor_offset, focus, focus_offset)
    }

    /// 两个入口的共同落点：编辑器路由 → 钳制偏移 → 撤旧指针会话 → 物化 programmatic。
    fn commit_select(
        &self,
        anchor: &Rc<dyn Selectable2>,
        anchor_offset: usize,
        focus: &Rc<dyn Selectable2>,
        focus_offset: usize,
    ) -> bool {
        // 活跃编辑器存在时，程序化选区必须写进编辑器本地光标（TS 同）：
        // 否则会被「塌缩光标」（优先级 #4）压制 → 双击选词/三击选段永不显示。
        // 但**目标不是编辑器本身时一律拒绝**——否则"编辑器聚焦时点别人"
        // 会被错写进编辑器，跨组件选择就彻底失效了（TS 也是 `return false`）。
        if let Some(ed) = self.source.active_editor_route() {
            let own = ed.as_selectable();
            if !Rc::ptr_eq(&own, anchor) || !Rc::ptr_eq(&own, focus) {
                return false;
            }
            ed.select_range(
                anchor_offset.min(anchor.text_length()),
                focus_offset.min(focus.text_length()),
            );
            return true;
        }
        let anchor_offset = anchor_offset.min(anchor.text_length());
        let focus_offset = focus_offset.min(focus.text_length());
        // 新指令物化即撤除旧指针会话（含定格），否则冻结端点会压制本指令。
        self.source.clear_pointer_select();
        self.programmatic.set(Some(SelPair2 {
            anchor: SelPoint2::new(Rc::clone(anchor), anchor_offset),
            focus: SelPoint2::new(Rc::clone(focus), focus_offset),
        }));
        true
    }

    // ---------- 派生核（对应 TS 的三段 memo）----------

    /// 命中链从深到浅找第一个仍在可选集合内的文本节点，并换算成文本偏移。
    fn point_from(
        hit: Option<&HitestResult2>,
        map: &HashMap<u64, Rc<dyn Selectable2>>,
    ) -> Option<SelPoint2> {
        let hit = hit?;
        for item in hit.chain.iter().rev() {
            let id = item.node.id();
            let Some(sel) = map.get(&id) else { continue };
            // 快照链可能引用已销毁/隐藏的节点，其布局坐标已失效——跳过。
            let offset = sel.position_for_point(hit.x, hit.y).min(sel.text_length());
            return Some(SelPoint2::new(Rc::clone(sel), offset));
        }
        None
    }

    /// 当前生效端点对（优先级从高到低，后者被前者自动压制）。
    fn current_pair(
        source: &dyn SelectionSource2,
        map: &HashMap<u64, Rc<dyn Selectable2>>,
        programmatic: Option<SelPair2>,
    ) -> Option<SelPair2> {
        let s = source.pointer_select();
        // #1 按住中：实时跟随 moveHit
        if let Some(s) = &s {
            if s.release.is_none() {
                return Self::pointer_pair(s, source, map);
            }
        }
        // #2 非塌缩光标：用户正在编辑器里显式选择（高于定格指针）
        let cursor_pair = source.active_editor_sel_pair();
        if let Some(p) = &cursor_pair {
            if p.anchor.offset != p.focus.offset {
                return cursor_pair;
            }
        }
        // #3 定格指针：拖选释放后的保留选区
        if let Some(s) = &s {
            return Self::pointer_pair(s, source, map);
        }
        // #4 塌缩光标：编辑器已聚焦但无选中（仅压制 #5）
        if cursor_pair.is_some() {
            return cursor_pair;
        }
        // #5 程序化
        programmatic
    }

    /// 指针会话推导端点对；press 落在可选文本之外返回 `None`（整个选区清空）。
    fn pointer_pair(
        s: &PointerSelect2,
        source: &dyn SelectionSource2,
        map: &HashMap<u64, Rc<dyn Selectable2>>,
    ) -> Option<SelPair2> {
        let anchor = Self::point_from(s.press.as_ref(), map)?;
        let focus = match s.release.as_ref() {
            Some(release) => Self::point_from(Some(release), map),
            None => Self::point_from(source.move_hit_test().as_ref(), map),
        };
        // 尚未移动时焦点退化为锚点（TS `focus: f ?? a`）。
        Some(SelPair2::new(
            anchor.clone(),
            focus.unwrap_or_else(|| anchor.clone()),
        ))
    }

    /// 全表分配（TS `ranges` memo）：按文档序把 `[anchor, focus]` 映射到各节点。
    fn compute_ranges(
        source: &dyn SelectionSource2,
        programmatic: Option<SelPair2>,
    ) -> Vec<(u64, usize, usize)> {
        let mut order = Vec::new();
        let mut map = HashMap::new();
        if let Some(root) = source.root_node() {
            collect_selectables(&root, &mut order, &mut map);
        }
        // `order` 已经是前序遍历序（= 文档序），**不排序**——见 `collect_selectables`。
        let Some(pair) = Self::current_pair(source, &map, programmatic) else {
            return Vec::new();
        };
        let ia = order.iter().position(|id| {
            map.get(id)
                .is_some_and(|s| Rc::ptr_eq(s, &pair.anchor.node))
        });
        let ifo = order
            .iter()
            .position(|id| map.get(id).is_some_and(|s| Rc::ptr_eq(s, &pair.focus.node)));
        let (Some(ia), Some(ifo)) = (ia, ifo) else {
            return Vec::new();
        };
        let forward = ia <= ifo;
        let first = if forward { &pair.anchor } else { &pair.focus };
        let second = if forward { &pair.focus } else { &pair.anchor };
        let (lo, hi) = (ia.min(ifo), ia.max(ifo));
        let mut out = Vec::new();
        for (idx, id) in order.iter().enumerate() {
            let sel = map.get(id).expect("order 与 map 同源");
            let range = if Rc::ptr_eq(sel, &first.node) && Rc::ptr_eq(sel, &second.node) {
                (
                    first.offset.min(second.offset),
                    first.offset.max(second.offset),
                )
            } else if Rc::ptr_eq(sel, &first.node) {
                (first.offset, sel.text_length())
            } else if Rc::ptr_eq(sel, &second.node) {
                (0, second.offset)
            } else if idx > lo && idx < hi {
                (0, sel.text_length())
            } else {
                continue;
            };
            if range.1 > range.0 {
                out.push((*id, range.0, range.1));
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canvas_render2::node2::Node2Props;

    /// 测试用可选节点：文本存在 `Rc<RefCell<String>>` 里，词边界按空格切。
    struct TestSelectable {
        text: Rc<RefCell<String>>,
    }

    impl Selectable2 for TestSelectable {
        fn selection_rect(&self) -> Option<Rect2> {
            Some(RectF::new(0.0, 0.0, 10.0, 10.0))
        }

        fn text_length(&self) -> usize {
            self.text.borrow().len()
        }

        fn position_for_point(&self, global_x: f64, _global_y: f64) -> usize {
            global_x.round().max(0.0) as usize
        }

        fn text_in_range(&self, start: usize, end: usize) -> String {
            let t = self.text.borrow();
            t.get(start..end.min(t.len()))
                .unwrap_or_default()
                .to_string()
        }

        fn word_range_at(&self, offset: usize) -> Option<(usize, usize)> {
            let t = self.text.borrow();
            let off = offset.min(t.len());
            let start = t[..off].rfind(' ').map_or(0, |i| i + 1);
            let end = t[off..].find(' ').map_or(t.len(), |i| off + i);
            if end > start {
                Some((start, end))
            } else {
                None
            }
        }
    }

    /// 派生源：**信号驱动**（与渲染器提供的真实源同构）。
    ///
    /// 用信号而不是 `Cell`，是因为管理器的派生链是 memo：只有读取信号才能建立
    /// 响应式依赖，`Cell` 的变化对 memo 不可见。
    struct TestSource {
        pointer: Signal<Option<Rc<PointerSelect2>>>,
        move_hit: Signal<Option<Rc<HitestResult2>>>,
        editor: Signal<Option<Rc<SelPair2>>>,
        root: Option<Node2>,
    }

    impl SelectionSource2 for TestSource {
        fn pointer_select(&self) -> Option<PointerSelect2> {
            self.pointer.get().as_ref().map(|p| PointerSelect2 {
                press: p.press.clone(),
                release: p.release.clone(),
            })
        }

        fn move_hit_test(&self) -> Option<HitestResult2> {
            self.move_hit.get().as_ref().map(|h| HitestResult2 {
                chain: h.chain.clone(),
                x: h.x,
                y: h.y,
                time: h.time,
            })
        }

        fn active_editor_sel_pair(&self) -> Option<SelPair2> {
            self.editor.get().as_ref().map(|p| SelPair2 {
                anchor: p.anchor.clone(),
                focus: p.focus.clone(),
            })
        }

        fn root_node(&self) -> Option<Node2> {
            self.root.clone()
        }

        fn clear_pointer_select(&self) {
            self.pointer.set(None);
        }
    }

    /// 造一棵 root → [a, b] 的树，并把 a / b 绑定为可选节点。
    fn setup(
        text_a: &str,
        text_b: &str,
    ) -> (
        Rc<TestSource>,
        Rc<SelectionManager2>,
        Rc<TestSelectable>,
        Rc<TestSelectable>,
        Node2,
        Node2,
    ) {
        let root = Node2::empty();
        let sa = Rc::new(TestSelectable {
            text: Rc::new(RefCell::new(text_a.to_string())),
        });
        let sb = Rc::new(TestSelectable {
            text: Rc::new(RefCell::new(text_b.to_string())),
        });
        // 可选能力经 `exts` 里的 `SelectableExt` 标记挂上（TS 同）——管理器不再
        // 持注册表，"谁可选"只看树上有没有这个标记。
        let a = Node2::empty().build(Node2Props {
            exts: vec![Rc::new(
                SelectableExt(Rc::clone(&sa) as Rc<dyn Selectable2>),
            )],
            ..Node2Props::default()
        });
        let b = Node2::empty().build(Node2Props {
            exts: vec![Rc::new(
                SelectableExt(Rc::clone(&sb) as Rc<dyn Selectable2>),
            )],
            ..Node2Props::default()
        });
        root.push_child(&a);
        root.push_child(&b);
        let source = Rc::new(TestSource {
            pointer: create_signal(None),
            move_hit: create_signal(None),
            editor: create_signal(None),
            root: Some(root.clone()),
        });
        let manager = Rc::new(SelectionManager2::new(
            Rc::clone(&source) as Rc<dyn SelectionSource2>
        ));
        (source, manager, sa, sb, a, b)
    }

    fn hit(node: &Node2, x: f64) -> HitestResult2 {
        HitestResult2 {
            chain: vec![HitestItem2 {
                node: node.clone(),
                x,
                y: 0.0,
            }],
            x,
            y: 0.0,
            time: 0.0,
        }
    }

    fn set_pointer(source: &TestSource, press: HitestResult2, release: Option<HitestResult2>) {
        source.pointer.set(Some(Rc::new(PointerSelect2 {
            press: Some(press),
            release,
        })));
    }

    /// 跨节点连续拖选：起点在 a 的第 2 字符、终点在 b 的第 3 字符。
    ///
    /// 起点节点取 `[2, len)`、终点节点取 `[0, 3)`——这正是 TS `ranges` 的
    /// "仅起点 / 仅终点"分支，也是旧"贡献者拼接"模型做不到的地方。
    #[test]
    fn drag_across_nodes_selects_full_middle() {
        let (source, manager, sa, sb, a, b) = setup("hello", "world");
        set_pointer(&source, hit(&a, 2.0), Some(hit(&b, 3.0)));

        assert_eq!(
            manager.range_of(&(Rc::clone(&sa) as Rc<dyn Selectable2>)),
            Some((2, 5)),
            "起点节点：从锚点到末尾"
        );
        assert_eq!(
            manager.range_of(&(Rc::clone(&sb) as Rc<dyn Selectable2>)),
            Some((0, 3)),
            "终点节点：从开头到焦点"
        );
        // 文档序聚合：a 的尾巴 + b 的头部
        assert_eq!(manager.selected_text(), Some("llowor".to_string()));
    }

    /// 单节点内拖选。
    #[test]
    fn drag_within_one_node() {
        let (source, manager, sa, _sb, a, _b) = setup("hello", "world");
        set_pointer(&source, hit(&a, 1.0), Some(hit(&a, 4.0)));
        assert_eq!(
            manager.range_of(&(Rc::clone(&sa) as Rc<dyn Selectable2>)),
            Some((1, 4))
        );
        assert_eq!(manager.selected_text(), Some("ell".to_string()));
    }

    /// 按住中：焦点跟随 moveHitTest（release 未填）。
    #[test]
    fn pressing_follows_move_hit() {
        let (source, manager, sa, _sb, a, _b) = setup("hello", "world");
        set_pointer(&source, hit(&a, 1.0), None);
        source.move_hit.set(Some(Rc::new(hit(&a, 3.0))));
        assert_eq!(
            manager.range_of(&(Rc::clone(&sa) as Rc<dyn Selectable2>)),
            Some((1, 3))
        );
        // 移动焦点 → 选区跟着变（同一会话内重新派生）
        source.move_hit.set(Some(Rc::new(hit(&a, 5.0))));
        assert_eq!(
            manager.range_of(&(Rc::clone(&sa) as Rc<dyn Selectable2>)),
            Some((1, 5))
        );
    }

    /// 已松手定格：hover（moveHitTest）不再影响选区。
    #[test]
    fn released_freezes_focus() {
        let (source, manager, sa, _sb, a, _b) = setup("hello", "world");
        set_pointer(&source, hit(&a, 1.0), Some(hit(&a, 3.0)));
        assert_eq!(
            manager.range_of(&(Rc::clone(&sa) as Rc<dyn Selectable2>)),
            Some((1, 3))
        );
        // 定格后再动指针：焦点冻结在释放位置
        source.move_hit.set(Some(Rc::new(hit(&a, 5.0))));
        assert_eq!(
            manager.range_of(&(Rc::clone(&sa) as Rc<dyn Selectable2>)),
            Some((1, 3))
        );
    }

    /// press 落在可选文本之外 → 整个选区为空（点击空白清除）。
    #[test]
    fn press_outside_selectable_clears_selection() {
        let (source, manager, sa, _sb, a, _b) = setup("hello", "world");
        let plain = Node2::empty();
        set_pointer(&source, hit(&plain, 0.0), Some(hit(&a, 3.0)));
        assert_eq!(
            manager.range_of(&(Rc::clone(&sa) as Rc<dyn Selectable2>)),
            None
        );
        assert!(!manager.has_selection());
    }

    /// 全选：从首个可选节点开头到末节点末尾，并撤除指针会话。
    #[test]
    fn select_all_covers_every_selectable() {
        let (source, manager, _sa, _sb, a, _b) = setup("hello", "world");
        set_pointer(&source, hit(&a, 1.0), Some(hit(&a, 2.0)));
        assert_eq!(manager.selected_text(), Some("e".to_string()));
        manager.select_all();
        assert_eq!(manager.selected_text(), Some("helloworld".to_string()));
    }

    /// 优先级：非塌缩编辑器光标 > 定格指针。
    #[test]
    fn expanded_editor_cursor_beats_released_pointer() {
        let (source, manager, sa, _sb, a, _b) = setup("hello", "world");
        set_pointer(&source, hit(&a, 1.0), Some(hit(&a, 2.0)));
        assert_eq!(manager.selected_text(), Some("e".to_string()));
        let sel = Rc::clone(&sa) as Rc<dyn Selectable2>;
        source.editor.set(Some(Rc::new(SelPair2 {
            anchor: SelPoint2::new(Rc::clone(&sel), 0),
            focus: SelPoint2::new(Rc::clone(&sel), 4),
        })));
        assert_eq!(manager.selected_text(), Some("hell".to_string()));
    }

    /// `selectionEnabled = false` 的子树整棵剪枝。
    #[test]
    fn selection_enabled_prunes_subtree() {
        let (_source, manager, sa, _sb, a, _b) = setup("hello", "world");
        let disabled = a.build(Node2Props {
            selection_enabled: Rc::new(|| false),
            ..Node2Props::default()
        });
        assert!(!manager.is_selectable(&disabled));
        assert_eq!(
            manager.range_of(&(Rc::clone(&sa) as Rc<dyn Selectable2>)),
            None
        );
    }

    /// 词级扩展（纯函数）。
    #[test]
    fn word_expansion_snaps_to_word_boundaries() {
        let s = TestSelectable {
            text: Rc::new(RefCell::new("hello brave world".to_string())),
        };
        // 锚词 brave = [6, 11)；目标落在 world 里 → 右端吸附到 world 末尾
        assert_eq!(
            expand_word_selection(6, 11, 14, |o| s.word_range_at(o)),
            (6, 17)
        );
        // 目标落在锚词内 → 保持锚词
        assert_eq!(
            expand_word_selection(6, 11, 8, |o| s.word_range_at(o)),
            (6, 11)
        );
        // 目标在锚词左侧 → 左端吸附到该词开头
        assert_eq!(
            expand_word_selection(6, 11, 2, |o| s.word_range_at(o)),
            (0, 11)
        );
    }
}
