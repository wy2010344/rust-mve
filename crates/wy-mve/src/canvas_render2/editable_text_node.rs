//! 可编辑文本节点（对标 TS `canvasRender2/EditableTextNode.ts`，983 行）。
//!
//! 与 Kotlin `EditableTextNode` 不同，这里是**函数式工厂**：节点身份由 `Node2`
//! 创建，编辑器行为（光标 / 本地选区 / 键盘 / 剪贴板 / IME / 撤销重做）挂在节点上，
//! 同时实现 [`Selectable2`]（供 `SelectionManager2` 派生全局选区）与
//! [`EditableTextHandle`]（供引擎路由键盘 / IME / 剪贴板快捷键）。
//!
//! # 编辑态在 `EditCore`（`wy-text`），这里只管几何与呈现
//!
//! 文本、光标 / 选区、字素与词边界、撤销重做、IME 组合态、掩码与占位的显示映射
//! 全部由 [`EditCore`] 持有——它就是 Kotlin `EditableTextNode` 的纯逻辑层。
//! 本模块只保留组件该做的事：段落缓存与排版、光标矩形、垂直移动（依赖行几何，
//! 纯逻辑层给不出）、绘制、命中、闪烁、跨组件选区协调。
//!
//! **借用纪律**：`core` 装在 `RefCell` 里，任何 `borrow_mut()` 都必须在调用外部
//! 回调之前释放。`Signal::set` 会**同步** flush 观察者，而观察者（选区派生 memo）
//! 会回头读内核；那一刻还握着借用就是 `RefCell already mutably borrowed`。
//! `EditableTextController::edit` / `with_core` / `sync_params` 里的
//! `begin_batch()` 正是为此存在（见 `wy_signal::BatchGuard`）。
//!
//! # 索引空间（必要的管道，不是取巧）
//!
//! 内核索引一律是**字符（Unicode 标量）索引**，与 TS 的 UTF-16 索引在 BMP 文本上
//! 数值一致（仅 astral emoji 上有差异）。段落 [`TextParagraph`] 由 `wy_text` 提供，其 API
//! （`glyph_position_at_coordinate` / `rects_for_range` / `line_metrics` /
//! `word_boundary`）全部用**字节偏移**。因此在"段落 API 边界"统一做 字符↔字节
//! 转换：进入段落前 `char_to_byte(display_text, idx)`，拿回后 `byte_to_char`。
//!
//! `Selectable2` / `SelPair2` 的偏移空间与节点自身一致（字符索引），跨节点选区的
//! 派生只在同一节点内比较偏移（`SelectionManager2` 不涉及跨节点数值比较），故与
//! `rich_text` 的字节空间互不干扰。
//!
//! # Rust 机制映射（类型系统/运行时差异，不是打折）
//!
//! - **光标闪烁**：TS 用 `setInterval` 每 530ms 翻转 `cursorVisible` 信号。Rust
//!   无 `setInterval`，改为在 `draw` 内按墙钟时间计算可见性（[`CARET_BLINK_MS`]）；
//!   宿主按同一周期请求重绘（[`crate::canvas_render2::editable_text_node::CARET_BLINK_MS`]），
//!   否则光标会停在"灭"的相位上——看起来像"点了输入框却没有光标"。
//! - **IME**：TS 由 `requestFocus` 把节点登记为活跃编辑器、引擎经 `onComposing`
//!   上报；Rust 引擎经 `Node.ime_fn` 上报 [`ImeEvent`]，这里把 `ime_fn` / `ime_cursor_fn`
//!   桥接到 `on_composing` / `caret_local_rect`（落在 [`ImeExt`] 的两条回调上）。
//! - **销毁注销**：TS `ctx.addDestroy` 释放 CanvasKit 段落对象与 `blink` 定时器；
//!   Rust 段落是 `Rc<Layout>`（Drop 自动释放）、无定时器，编辑器句柄在 `build` 时
//!   `register_editor`（节点 id 单调递增不复用，注册表条目有界）。引擎 `Node` 暂无
//!   `on_destroy` 钩子，注销留待引擎补齐（见 #76）。
//! - **段落缓存**：TS `memo` → 这里 `ParagraphCache`（按"显示文本 + 样式 + 排版宽 +
//!   最大行数"失效重建），与 `rich_text` 同构。

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use wy_render::{Color, Point, Rect, Scene};
use wy_text::{
    build_paragraph, FontContext, RectStyle, TextAlign, TextDecoration, TextParagraph, TextRect,
    TextSpan, TextStyle,
};

use crate::canvas_render2::clipboard::{clipboard_get_text, clipboard_set_text};
use crate::canvas_render2::fc::{Fc, Ref2};
use crate::canvas_render2::focus::{
    self, EditableTextHandle, InputOverlayRect, KeyCode2, KeyEvent2,
};
use crate::canvas_render2::geometry::RectF;
use crate::canvas_render2::graphemes::Words;
use crate::canvas_render2::node2::{always, Axis, Get, ImeExt, LayoutSize, Node2};
use crate::canvas_render2::protocol::MouseEvent2;
use crate::canvas_render2::selection::{
    Rect2, SelPair2, SelPoint2, Selectable2, SelectableExt, SelectionManager2,
};
use crate::canvas_render2::text_defaults::{placeholder_color, text_color};
use crate::canvas_render2::text_metric::ink_shift_y_of;
use crate::canvas_render2::util::MAX_LAYOUT_WIDTH;
use crate::primitives::ImeEvent;
use crate::{rec, Node2Props};
use wy_signal::{create_signal, GetValue, SetValue, Signal};
use wy_text::EditCore;

thread_local! {
    /// 跨调用复用的字体上下文（Parley 需要可变引用）。
    static FC: RefCell<FontContext> = RefCell::new(FontContext::new());
}

/// 光标闪烁周期（毫秒），对齐 TS `BLINK_INTERVAL`。
///
/// 绘制只发生在 `draw` 里：没有人按这个周期请求重绘，光标就会停在上一次的相位上
/// ——正好落在"灭"相时，表现就是"点了输入框却没有光标"。宿主（引擎的
/// `Node2App`）用它排下一次唤醒，见 `WyApp::next_wake`。
pub const CARET_BLINK_MS: u64 = 530;

/// 光标宽度（像素）。
const CURSOR_WIDTH: f32 = 2.0;
/// 一页行数（`PageUp` / `PageDown`）。
const PAGE_LINES: i32 = 12;

/// 钳制到 `[lo, hi]`。
fn coerce(v: usize, lo: usize, hi: usize) -> usize {
    if v < lo {
        lo
    } else if v > hi {
        hi
    } else {
        v
    }
}

/// 字符索引 → 字节偏移（越界钳到 `[0, len]`）。
fn char_to_byte(text: &str, char_idx: usize) -> usize {
    let total = text.chars().count();
    let ci = char_idx.min(total);
    text.char_indices().nth(ci).map_or(text.len(), |(b, _)| b)
}

/// 字节偏移 → 字符索引（越界钳到 `[0, total]`）。
fn byte_to_char(text: &str, byte: usize) -> usize {
    let byte = byte.min(text.len());
    text[..byte].chars().count()
}

/// 段落缓存键（显示文本 + 样式 + 排版宽 + 最大行数）。
#[derive(Clone, PartialEq)]
struct ParaKey {
    text: String,
    font_size: f32,
    font_weight: u16,
    italic: bool,
    color: u32,
    letter_spacing: f32,
    word_spacing: f32,
    line_height: f32,
    max_width_bits: u64,
    max_lines: usize,
}

/// 段落缓存（对标 TS `memo(paragraph)`）。
#[derive(Default)]
struct ParagraphCache {
    key: Option<ParaKey>,
    para: Option<Rc<TextParagraph>>,
}

/// 光标局部矩形（节点坐标系，已含 padding 与单行水平滚动偏移）。
struct CaretLocalRect {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
}

// ============================================================================
// 编辑器节点工厂（对标 TS `editableTextNode = fc(function (ctx, arg) { … })`）
// ============================================================================

/// 可编辑文本节点（对标 TS `editableTextNode`）。
///
/// 与其它基础组件同构：`#[rec(extends = node2)]` 自动获得基类全部字段（`width` /
/// `height` / `padding` / `focusable` / `layout` / `exts` …）并注入 `rest: Node2Props`；
/// 本函数只声明编辑器自己的字段，节点仍由 `node2! { ..rest, .. }` 声明式组装——
/// 覆盖 `focusable` / `size` / `draw` / `handle_key` / `mouse_down`，追加两条 ext。
///
/// # 为什么 `text` 要写 `#[props(required)]`
///
/// `text` 是编辑器的逻辑文本真相源，没有合理缺省。但 `#[getter]` 无默认值时的
/// 默认形态是 `Option<Get<T>>`（「未声明」，`node2` 的 `width` / `height` / `x` /
/// `y` 依赖这一形态表达「交给布局自动」），所以要用字段级 `#[props(required)]`
/// 把它拉回必填——存储类型随之剥成 `Get<String>`，worker 里直接 `text()` 调用，
/// 不必层层 `unwrap`。
#[rec(extends = node2)]
// 22 个参数：`extends = node2` 不展开基类字段，自己的文本/排版/选区字段本来就多。
#[allow(clippy::too_many_arguments)]
#[must_use]
pub fn editable_text_node(
    /// 逻辑文本真相源（UTF-16/字符串）；编辑器写入经 `on_text_change` 回传。
    #[getter]
    #[props(required)]
    text: String,
    /// 文本变更回写（编辑器内部改动落点）。不传则编辑动作不回写外部状态。
    on_text_change: impl Fn(&str) + 'static,
    /// 占位文本（逻辑文本为空时显示，不参与编辑与选区）。
    #[getter]
    #[props(default = String::new())]
    placeholder: String,
    /// 密码模式：每个字素簇显示为一个圆点。
    #[getter]
    #[props(default = false)]
    obscure: bool,
    /// 单行输入（不折行，溢出水平跟随光标）。
    #[getter]
    #[props(default = false)]
    single_line: bool,
    /// 最大行数；不传则单行取 1、多行取 [`usize::MAX`]。**故意不给默认值**：
    /// 缺省依赖 `single_line`，只能留 `None` 由 worker 兜底（宏的 default 是
    /// 编译期表达式，取不到别的字段）。
    #[getter]
    max_lines: usize,
    /// 提交回调：单行回车 / 多行 Ctrl/Cmd+Enter 触发。
    on_submit: impl Fn() + 'static,
    /// 只读。
    #[getter]
    #[props(default = false)]
    read_only: bool,
    /// 是否消费 Tab 键插入制表符（仅多行生效）。
    #[getter]
    #[props(default = false)]
    tab_insert: bool,
    /// 撤销历史容量（动作式栈）。
    #[props(default = 100usize)]
    max_history_size: usize,
    /// 字体族（缺省 `None` = 系统默认）。
    #[getter]
    #[props(default = None)]
    font_family: Option<String>,
    /// 字号（缺省 16）。
    #[getter]
    #[props(default = 16.0f32)]
    font_size: f32,
    /// 字重（缺省 400）。
    #[getter]
    #[props(default = 400u16)]
    font_weight: u16,
    /// 斜体（缺省 false）。
    #[getter]
    #[props(default = false)]
    italic: bool,
    /// 文字颜色（缺省 [`text_color`]）。
    #[getter]
    #[props(default = text_color())]
    color: Color,
    /// 行高倍数（缺省 `Some(1.4)`，`None` = 用排版默认）。
    #[getter]
    #[props(default = Some(1.4f32))]
    line_height_multiplier: Option<f32>,
    /// 字间距（缺省 0）。
    #[getter]
    #[props(default = 0.0f32)]
    letter_spacing: f32,
    /// 词间距（缺省 0）。
    #[getter]
    #[props(default = 0.0f32)]
    word_spacing: f32,
    /// 选区颜色（缺省 `rgba(80,140,255,80)`）。
    #[getter]
    #[props(default = Color::rgba(80, 140, 255, 80))]
    selection_color: Color,
    /// 选区管理器（供 `hasSelection` / `selection_rect` / 吸收全局选区）。
    selection_manager: Option<Rc<SelectionManager2>>,
    /// 节点句柄槽（对齐 TS `arg.nodeRef`）：普通参数，默认空槽；
    /// 不传则调用方拿不到该节点。
    #[props(default = Ref2::new())]
    node_ref: Ref2<Node2>,
) -> Fc {
    // 显式尺寸取自基类字段（TS 里 `arg.width` 就是 node2 的 `width`）。留在
    // `rest` 里继续参与 node2 的布局，这里只 clone 一份 getter 供 `size()` 用。
    let has_explicit_width = rest.width.is_some();
    let explicit_width = rest.width.clone().unwrap_or_else(|| always(0.0));
    let has_explicit_height = rest.height.is_some();
    let explicit_height = rest.height.clone().unwrap_or_else(|| always(0.0));
    // `max_lines` 的缺省依赖 `single_line`，只能在两者都拿到后算。
    let max_lines = max_lines.unwrap_or_else(|| {
        let sl = Rc::clone(&single_line);
        Rc::new(move || if sl() { 1 } else { usize::MAX })
    });

    let ctrl = Rc::new(EditableTextController {
        // 节点还没造出来（`node2!` 才造）。控制器拿句柄槽，用的时候再取——
        // 所有读取都发生在布局期 / 绘制期 / 事件期，那时槽早已写好。
        node: node_ref.clone(),
        self_sel: RefCell::new(None),
        text: Rc::clone(&text),
        on_text_change,
        placeholder: Rc::clone(&placeholder),
        obscure: Rc::clone(&obscure),
        single_line: Rc::clone(&single_line),
        max_lines: Rc::clone(&max_lines),
        on_submit,
        read_only: Rc::clone(&read_only),
        tab_insert: Rc::clone(&tab_insert),
        font_family: Rc::clone(&font_family),
        font_size: Rc::clone(&font_size),
        font_weight: Rc::clone(&font_weight),
        italic: Rc::clone(&italic),
        color: Rc::clone(&color),
        line_height_multiplier: Rc::clone(&line_height_multiplier),
        letter_spacing: Rc::clone(&letter_spacing),
        word_spacing: Rc::clone(&word_spacing),
        selection_color: Rc::clone(&selection_color),
        has_explicit_width,
        explicit_width,
        has_explicit_height,
        explicit_height,
        selection_manager,
        caret_pair: create_signal(None),
        composing_text: create_signal(String::new()),
        composing_cursor_pos: create_signal(0usize),
        core: RefCell::new({
            let mut c = EditCore::with_text(text());
            c.single_line = single_line();
            c.max_history_size = max_history_size;
            c.set_placeholder(placeholder());
            c.set_obscure_text(obscure());
            c
        }),
        paragraph_cache: RefCell::new(ParagraphCache::default()),
        blink_anchor: Cell::new(None),
        was_focused: Cell::new(false),
    });
    ctrl.self_sel
        .replace(Some(Rc::clone(&ctrl) as Rc<dyn Selectable2>));

    let c_draw = Rc::clone(&ctrl);
    let c_size = Rc::clone(&ctrl);
    let c_key = Rc::clone(&ctrl);
    let c_down = Rc::clone(&ctrl);
    let c_ime = Rc::clone(&ctrl);
    let c_cur = Rc::clone(&ctrl);
    // IME 通道走 ext：`ime` / `ime_cursor` 不属于"节点长什么样"，也不该让
    // `node2` 的 props 为它开两个只有编辑器用得上的洞。
    let ime_ext = Rc::new(ImeExt {
        on_ime: Rc::new(move |e: &mut ImeEvent| -> bool {
            match e {
                ImeEvent::Preedit { text, cursor } => {
                    let pos = cursor.map_or(text.chars().count(), |(_, en)| byte_to_char(text, en));
                    c_ime.on_composing(text, pos);
                    true
                }
                ImeEvent::Commit(t) => {
                    c_ime.commit_composing_text(t);
                    true
                }
                ImeEvent::Enabled | ImeEvent::Disabled => false,
            }
        }),
        cursor: Rc::new(move || match c_cur.caret_local_rect() {
            Some(r) => (r.x, r.y, r.w, r.h),
            // 还没排版（空文本）时退到文本原点，候选框也落在字会开始出现的位置。
            None => (
                c_cur.node().padding_inline_start(),
                c_cur.text_origin_y(),
                0.0,
                0.0,
            ),
        }),
    });
    // 活跃编辑器不再走 id 注册表：把强类型句柄包成协议层标记挂进 `exts`，
    // 焦点落到本节点时由 `focus::active_editor()` 下转取用
    // （TS `requestFocus` → `node.findExt(EditorHandleExt)`）。
    let handle_ext = Rc::new(focus::editor_handle_ext(
        Rc::clone(&ctrl) as Rc<dyn EditableTextHandle>
    ));

    // 可选能力同样经 `exts` 里的 `SelectableExt` 标记声明（TS 同）：指针路由/
    // 选择管理器只认这一条消息，编辑器与富文本因此走完全相同的收集路径。
    let selectable_ext = Rc::new(SelectableExt(Rc::clone(&ctrl) as Rc<dyn Selectable2>));

    node2! {
        ..rest,
        node_ref: node_ref.clone(),
        // 编辑器**永远可聚焦**：不可聚焦就点不进去，也就永远收不到键盘 / IME
        // （`pointer::down` 只用 `focusable_leaf_of` 选焦点目标）。覆盖调用方传
        // 的值是有意的：只读 / 密码模式下编辑器仍需要焦点（选区、复制、光标），
        // "不可聚焦的编辑器"不是一种合法状态。
        focusable: true,
        size: move |d: Axis| -> LayoutSize { c_size.size(d) },
        draw: move |s: &mut Scene, n: &Node2| c_draw.draw(s, n),
        handle_key: move |e: &KeyEvent2| c_key.handle_key(e),
        mouse_down: move |ev: &mut MouseEvent2| c_down.mouse_down(ev.x, ev.y),
        exts_push: handle_ext,
        exts_push: ime_ext,
        exts_push: selectable_ext,
    }
}

impl EditableTextController {
    /// 本节点（句柄槽里的那一份；槽在 `node2!` 里立刻写好，之后不会变）。
    fn node(&self) -> Node2 {
        self.node.get()
    }
}

/// 文本变化回调（对应 TS `EditableTextNode.ts` 的 `arg.onChange`）。
///
/// 控制器只存一份、在编辑动作之后调用；`Rc<dyn Fn>` 而非 `Arc`，因为 UI 单线程。
type TextChangeHandler = Rc<dyn Fn(&str)>;

/// 可编辑文本节点控制器：实现 [`Selectable2`] 与 [`EditableTextHandle`]，
/// 并持有全部编辑器状态。节点本身由 [`Node2`] 承载，控制器挂在节点上。
struct EditableTextController {
    /// 本节点句柄槽。控制器先于节点构造（`node2!` 里才建节点），所以不直接存
    /// `Node2`，而是存槽、用时再取——所有读取都在节点成型之后。
    node: Ref2<Node2>,
    /// 自身作为 `Selectable2` 的 `Rc`（构造后回填，供 `cursor_sel_pair` 等使用）。
    self_sel: RefCell<Option<Rc<dyn Selectable2>>>,
    text: Get<String>,
    on_text_change: Option<TextChangeHandler>,
    placeholder: Get<String>,
    obscure: Get<bool>,
    single_line: Get<bool>,
    max_lines: Get<usize>,
    on_submit: Option<Rc<dyn Fn()>>,
    read_only: Get<bool>,
    tab_insert: Get<bool>,
    font_family: Get<Option<String>>,
    font_size: Get<f32>,
    font_weight: Get<u16>,
    italic: Get<bool>,
    color: Get<Color>,
    line_height_multiplier: Get<Option<f32>>,
    letter_spacing: Get<f32>,
    word_spacing: Get<f32>,
    selection_color: Get<Color>,
    has_explicit_width: bool,
    explicit_width: Get<f32>,
    has_explicit_height: bool,
    explicit_height: Get<f32>,
    selection_manager: Option<Rc<SelectionManager2>>,
    /// 唯一可置信信号源：本地光标 / 选区（原子写入，复刻 Kotlin `caretPair`）。
    ///
    /// 单一 `Option<(anchor, focus)>` 信号，保证 anchor 与 focus 永远同步更新、
    /// 不存在半新半旧的中间态。`localSelStart` / `cursor` / `cursorSelPair` /
    /// `composing*` 等所有派生量都**直接读它**（不再进 `RefCell<EditCore>` 去借用），
    /// 这样 SelectionManager 的派生 memo 订阅的是这个信号——正是之前把信号塞进
    /// `RefCell` 而丢失的响应式链条。
    caret_pair: Signal<Option<(usize, usize)>>,
    /// IME 组合串（信号承载，绘制依赖追踪）。
    composing_text: Signal<String>,
    /// 组合串内 IME 光标偏移（overlay 候选框定位参考）。
    composing_cursor_pos: Signal<usize>,
    /// 编辑内核 [`EditCore`]：文本缓冲 / undo / 掩码显示映射的**纯逻辑**（已无信号，
    /// 仅剩 `RefCell` 包裹的可变数据）。光标 / 选区 / 组合态的信号由上方控制器字段持有。
    ///
    /// 控制器自己只保留「几何与呈现」：段落缓存、光标矩形、垂直移动、绘制、
    /// 命中、闪烁。**借用纪律**：`RefCell` 的借用绝不跨外部回调——`write_text`
    /// 会调到宿主的 `on_text_change`，那时还握着借用就会重入 panic。
    core: RefCell<EditCore>,
    paragraph_cache: RefCell<ParagraphCache>,
    /// 光标闪烁相位锚点：聚焦 / 光标移动时重置成"刚亮"，让点击后立刻看得到光标。
    blink_anchor: Cell<Option<Instant>>,
    /// 上一帧是否聚焦（用于捕捉"刚获得焦点"这一瞬间）。
    was_focused: Cell<bool>,
}

// ============================================================================
// 段落构建
// ============================================================================

/// 构建排版（带兜底：空片段也返回一个可用的空排版）。
fn build_para(
    spans: &[TextSpan],
    max_width: Option<f32>,
    max_lines: usize,
    ellipsis: &str,
    align: TextAlign,
) -> TextParagraph {
    FC.with(|fc| {
        let mut borrow = fc.borrow_mut();
        build_paragraph(&mut borrow, spans, max_width, max_lines, ellipsis, align).unwrap_or_else(
            |_| {
                build_paragraph(
                    &mut borrow,
                    &[TextSpan {
                        text: String::new(),
                        style: TextStyle::normal().with_font_size(12.0).with_color(0),
                    }],
                    None,
                    usize::MAX,
                    "",
                    align,
                )
                .unwrap()
            },
        )
    })
}

// ============================================================================
// 控制器实现
// ============================================================================

impl EditableTextController {
    /// 自身作为 `Selectable2` 的 `Rc`（构造后回填）。
    fn self_sel(&self) -> Rc<dyn Selectable2> {
        self.self_sel
            .borrow()
            .clone()
            .expect("EditableTextController.self_sel 未设置")
    }

    /// 当前生效的选择管理器。
    ///
    /// 优先用调用方显式注入的那一份；没有就消费"当前渲染器装的管理器"
    /// （TS 从 `ctx.consume(selectionManagerContext)` 取，这里用线程局部等价）。
    /// **必须在用点解析、不能在构造期固定**：`Node2App` 先建树、后装管理器，
    /// 构造期取到的永远是 `None`——那会让编辑器选区既不吸收也不绘制，
    /// 表现就是"选中了但没有任何高亮"。
    fn manager(&self) -> Option<Rc<SelectionManager2>> {
        self.selection_manager
            .clone()
            .or_else(crate::canvas_render2::selection::current_selection_manager)
    }

    /// 逻辑文本字符长度。
    fn text_len(&self) -> usize {
        self.core.borrow().text_len()
    }

    // ---- 光标 / 选区（唯一真相源是控制器信号 `caret_pair`，派生量直接读它）----

    fn anchor_index(&self) -> Option<usize> {
        self.caret_pair.get().map(|(a, _)| a)
    }

    fn focus_index(&self) -> Option<usize> {
        self.caret_pair.get().map(|(_, f)| f)
    }

    /// 钳制后的光标（塌缩时返回 0 或文本末尾）。
    fn cursor(&self) -> usize {
        self.anchor_index().unwrap_or(0).min(self.text_len())
    }

    /// 垂直移动的视觉列（TS `preferredX`）。`None` = 还没定过。
    ///
    /// 核里用 `f32::NAN` 表示"未定"（Kotlin 同），这里翻译成 `Option`，
    /// 让控制器不必到处写 `is_nan()`。
    fn preferred_x(&self) -> Option<f32> {
        let x = self.core.borrow().preferred_x();
        (!x.is_nan()).then_some(x)
    }

    fn set_preferred_x(&self, v: Option<f32>) {
        self.with_core_mut(|c| c.set_preferred_x(v.unwrap_or(f32::NAN)));
    }

    fn local_sel_start(&self) -> usize {
        match self.caret_pair.get() {
            Some((a, f)) => a.min(f).min(self.text_len()),
            None => 0,
        }
    }

    fn local_sel_end(&self) -> usize {
        match self.caret_pair.get() {
            Some((a, f)) => a.max(f).min(self.text_len()),
            None => 0,
        }
    }

    /// 用钳制后的边界比较：文本被外部改短时残留的越界端点不应算"有选区"，
    /// 否则替换选区会回落到"按光标插入"形成相互递归（栈溢出）。
    fn local_has_sel(&self) -> bool {
        matches!(self.caret_pair.get(), Some((a, f)) if a != f)
    }

    /// **就地改内核的唯一入口**。任何 `borrow_mut()` 都必须走这里。
    ///
    /// 光标 / 选区 / IME 组合态的信号已提升到控制器字段（`caret_pair` 等），内核
    /// `EditCore` 不再持有信号，所以这里只负责把纯数据的 `borrow_mut` 收口——不再有
    /// "写信号触发同步 flush、观察者回头借 `core`" 的双借陷阱（旧版闪退的根因）。
    /// 信号在 [`Self::publish_sel`] 里、于 `core` 借用释放后统一发布。
    fn with_core_mut(&self, f: impl FnOnce(&mut EditCore)) {
        f(&mut self.core.borrow_mut());
    }

    /// 把核内纯字段态发布为控制器的响应式信号。
    ///
    /// 必须在 `core` 借用**释放后**调用：发布即 `Signal::set`，会同步 flush 观察者
    /// （如 SelectionManager 的派生 memo）。若 flush 发生在 `core.borrow_mut()` 仍持有
    /// 期间，观察者读 `core` 会 `RefCell already borrowed` 直接 panic；在借用释放后发布，
    /// 观察者读的是控制器信号字段（无需借 `core`），安全。
    fn publish_sel(&self) {
        let (a, f, ct, ccp) = {
            let c = self.core.borrow();
            (
                c.anchor(),
                c.focus(),
                c.composing_text(),
                c.composing_cursor_pos(),
            )
        };
        self.caret_pair.set(match (a, f) {
            (Some(a), Some(f)) => Some((a, f)),
            _ => None,
        });
        self.composing_text.set(ct);
        self.composing_cursor_pos.set(ccp);
    }

    fn set_cursor(&self, idx: usize) {
        self.sync_params();
        self.with_core_mut(|c| c.set_cursor(idx));
        self.publish_sel();
        // 光标位置变了：重新从"亮"开始闪，点击 / 方向键之后都不会有空窗。
        self.restart_blink();
    }

    fn extend_to(&self, new_pos: usize) {
        self.sync_params();
        self.with_core_mut(|c| c.extend_to(new_pos));
        self.publish_sel();
        // 光标位置变了：重新从"亮"开始闪，方向键 / 拖选之后都不会有空窗。
        self.restart_blink();
    }

    fn move_to(&self, new_pos: usize, extend: bool) {
        self.sync_params();
        self.with_core_mut(|c| c.move_to(new_pos, extend));
        self.publish_sel();
        // 光标位置变了：重新从"亮"开始闪，方向键 / 点击之后都不会有空窗。
        self.restart_blink();
    }

    // ---- 显示层（占位 / 掩码 / tab 展开）----

    /// 把外部真相源同步进内核。
    ///
    /// `text` / `placeholder` / `obscure` / `single_line` 都是**外部真相源**
    /// （调用方的信号），内核持有的是编辑期副本。任何"读内核"的入口先同步一次，
    /// 外部改动（程序化 `set` 文本、切主题换占位文案、切掩码）才立刻生效。
    ///
    /// 只把**外部真相源**（`text` / `placeholder` / `obscure` / `single_line` 这些
    /// 调用方信号）同步进内核副本。**绝不碰光标 / 选区**。
    ///
    /// 之前这里曾把控制器信号 `caret_pair` 回写进核（`set_pair` / `clear_pair`），
    /// 那是错误的双向同步：在 `edit` 里 `write_text` 触发同步 flush、而此刻
    /// `caret_pair` 还是旧值，回写会把刚被编辑动作更新的核内光标覆盖回旧值，
    /// 于是换行的光标停在上一个位置、上下键因光标落在空矩形退化成乱跳。
    /// 光标 / 选区是**单向**从核经 [`Self::publish_sel`] 流向 `caret_pair` 的，
    /// 核内 `anchor` / `focus` 由各编辑动作自己维护，无需在这里回灌。
    ///
    /// **只在值真的变了时才写**：无条件写会在绘制期做无谓的副本更新。
    /// 内核 `EditCore` 已无信号，本函数不再触发同步 flush（旧版依赖 `is_computing`
    /// 闸避免的断言已不存在——信号提升为控制器字段后即消除）。
    fn sync_params(&self) {
        self.with_core_mut(|c| {
            let t = (self.text)();
            if c.text() != t {
                c.replace_text(t);
            }
            let p = (self.placeholder)();
            if c.placeholder() != p {
                c.set_placeholder(p);
            }
            let o = (self.obscure)();
            if c.obscure_text() != o {
                c.set_obscure_text(o);
            }
            c.single_line = (self.single_line)();
        });
    }

    fn showing_placeholder(&self) -> bool {
        self.sync_params();
        self.core.borrow().showing_placeholder()
    }

    fn display_text(&self) -> String {
        self.sync_params();
        self.core.borrow().display_text()
    }

    /// 逻辑索引 → 显示（字符）索引。
    fn logic_to_display_index(&self, pos: usize) -> usize {
        self.sync_params();
        self.core.borrow().logic_to_display_index(pos)
    }

    /// 显示（字符）索引 → 逻辑索引。
    fn display_to_logic_index(&self, dpos: usize) -> usize {
        self.sync_params();
        self.core.borrow().display_to_logic_index(dpos)
    }

    /// 段落（显示文本排版）；`None` = 无文本。
    fn paragraph(&self) -> Option<Rc<TextParagraph>> {
        let dt = self.display_text();
        if dt.is_empty() {
            return None;
        }
        let is_ph = self.showing_placeholder();
        let style = TextStyle {
            font_family: (self.font_family)(),
            font_size: (self.font_size)(),
            font_weight: (self.font_weight)(),
            italic: (self.italic)(),
            color: if is_ph {
                placeholder_color().to_u32()
            } else {
                (self.color)().to_u32()
            },
            letter_spacing: (self.letter_spacing)(),
            word_spacing: (self.word_spacing)(),
            line_height_multiplier: (self.line_height_multiplier)(),
            decoration: TextDecoration::NONE,
        };
        let max_width = if (self.single_line)() {
            Some(MAX_LAYOUT_WIDTH)
        } else {
            let w = self.node().inner_size(Axis::X);
            if w > 0.0 {
                Some(w)
            } else {
                None
            }
        };
        let max_lines = (self.max_lines)();
        let lh = (self.line_height_multiplier)().unwrap_or(-1.0);
        let mw = max_width.unwrap_or(0.0);
        let key = ParaKey {
            text: dt.clone(),
            font_size: style.font_size,
            font_weight: style.font_weight,
            italic: style.italic,
            color: style.color,
            letter_spacing: style.letter_spacing,
            word_spacing: style.word_spacing,
            line_height: lh,
            max_width_bits: mw.to_bits() as u64,
            max_lines,
        };
        {
            let c = self.paragraph_cache.borrow();
            if c.key.as_ref() == Some(&key) {
                if let Some(p) = &c.para {
                    return Some(Rc::clone(p));
                }
            }
        }
        let span = TextSpan {
            text: dt.clone(),
            style,
        };
        let p = Rc::new(build_para(
            &[span],
            max_width,
            max_lines,
            "\u{2026}",
            TextAlign::Start,
        ));
        let mut c = self.paragraph_cache.borrow_mut();
        c.key = Some(key);
        c.para = Some(Rc::clone(&p));
        Some(p)
    }

    // ---- 几何辅助 ----

    /// 光标矩形（段落坐标系，已含 padding）；取光标处一格，文末取前一格。
    fn caret_rect(&self) -> Vec<TextRect> {
        let p = match self.paragraph() {
            Some(p) => p,
            None => return Vec::new(),
        };
        let dt = self.display_text();
        // 用 `cursor()`（缺省 0、钳到文末）而非 `focus_index().unwrap()`：
        // 编辑器刚聚焦、光标尚未落位时 `caret_pair` 为 `None`，仍应在位置 0
        // 画出光标（首帧可见性门禁要求），与 TS `cursor()` 的 `anchorIndex() >= 0 ? ... : 0`
        // 对齐；这也是此前内联副本的既有行为，不能因换成 `caret_rect()` 而丢。
        let fi = self.cursor();
        let dpos = self.logic_to_display_index(fi);
        let byte = char_to_byte(&dt, dpos);
        let end_byte = char_to_byte(&dt, (dpos + 1).min(dt.chars().count()));
        let list = p.rects_for_range(byte, end_byte, RectStyle::Tight);
        if !list.is_empty() {
            return list;
        }
        // 空行（含末尾空行）：此处没有任何字形簇可量，光标就停在**行首**，
        // 取该行行盒的零宽矩形。不能走下面的"前一格"回退——那会把光标画到
        // 上一行末尾，也会让上下键把行号判少一行。
        if let Some(li) = p.line_index_of_byte(byte) {
            if p.is_blank_line(li) {
                if let Some(l) = p.line_metrics().get(li) {
                    return vec![TextRect::new(0.0, l.top, 0.0, l.bottom)];
                }
            }
        }
        if dpos > 0 {
            let b2 = char_to_byte(&dt, dpos - 1);
            if let Some(prev) = p.rects_for_range(b2, byte, RectStyle::Tight).first() {
                // 文末插入点 = 前一字的**右缘**（而非左缘），保证光标画在最后一个
                // 字符之后；否则会画在最后一个字符之前，看起来"光标无法到末尾"。
                return vec![TextRect::new(prev.right, prev.top, prev.right, prev.bottom)];
            }
        }
        Vec::new()
    }

    /// 单行溢出时内容整体左移量（对齐 Kotlin RichTextNode.drawInContentBox）。
    fn compute_scroll_x(&self) -> f32 {
        if !(self.single_line)() {
            return 0.0;
        }
        let p = match self.paragraph() {
            Some(p) => p,
            None => return 0.0,
        };
        let box_w = self.node().inner_size(Axis::X).max(1.0);
        let caret_rects = self.caret_rect();
        let caret = match caret_rects.first() {
            Some(r) => r,
            None => return 0.0,
        };
        let left = caret.left;
        let right = caret.right;
        let mut off = if right > box_w { right - box_w } else { 0.0 };
        if left - off < 0.0 {
            off = left;
        }
        off.max(0.0).min((p.width() - box_w).max(0.0))
    }

    /// 单行墨迹垂直居中补偿（TS `inkShiftY` / Kotlin `inkShiftY`）。
    ///
    /// 单行编辑器的高度通常比行盒大，而行高留白全堆在字形**上方**，不补偿文字
    /// 就会贴在偏上位置（输入框里"文字贴顶"就是这个）。多行保持自然行高。
    fn ink_shift_y(&self) -> f32 {
        let p = self.paragraph();
        ink_shift_y_of(
            p.as_deref(),
            self.display_text().len(),
            self.node().inner_size(Axis::Y),
            (self.single_line)() || (self.max_lines)() == 1,
        )
    }

    /// 文本原点 y（节点坐标系）：padding 起点 + 墨迹补偿。
    ///
    /// 绘制 / 选区 / 光标 / 命中全部经它，保证"看到的 = 点到的"（TS `textOriginY`）。
    fn text_origin_y(&self) -> f32 {
        self.node().padding_block_start() + self.ink_shift_y()
    }

    /// 光标局部矩形（节点坐标系，已含 padding 与单行水平滚动偏移）。
    fn caret_local_rect(&self) -> Option<CaretLocalRect> {
        let caret_rects = self.caret_rect();
        let r = caret_rects.first()?;
        Some(CaretLocalRect {
            x: r.left + self.node().padding_inline_start() - self.compute_scroll_x(),
            y: r.top + self.text_origin_y(),
            w: CURSOR_WIDTH.max(r.right - r.left),
            h: (self.font_size)().max(r.bottom - r.top),
        })
    }

    /// 光标矩形 + 光标插入点横坐标（供上下移动保持视觉列）。
    /// 光标矩形 + 光标插入点横坐标（供上下移动保持视觉列）。
    ///
    /// 与 TS `caretMetrics` 对齐：优先取光标**后一个字符**的矩形（其 `left` 即插入点）；
    /// 文末取不到时退回**前一个字符**的矩形，插入点取它的 `right` 缘（而非 `left`），
    /// 这样文末光标落在最后一个字符**之后**，否则会画在最后一个字符之前。
    ///
    /// 注意：`preferredX` 用插入点（TS `at.left` / `prev.right`），**不用矩形中心**——
    /// 中心点恰好落在字符间的临界点上，CanvasKit 取整方向会让视觉列左右抖动
    /// （见 TS `caret.ts` 注释）；Kotlin 虽然在 `moveUp` 里写的是 `r.left + r.width / 2f`，
    /// 但其 `cursorRect` 取的是 `pos+1`（后一字）或 `pos-1` 的矩形、`r.width` 在该语境下
    /// 通常即字符宽，等价于插入点；本端口沿用 TS 的插入点语义，保持与 `caretRectsOf` 一致。
    fn caret_metrics(&self) -> Option<(TextRect, f32)> {
        let p = self.paragraph()?;
        let dt = self.display_text();
        let fi = self.focus_index()?;
        let dpos = self.logic_to_display_index(fi);
        let byte = char_to_byte(&dt, dpos);
        let end_byte = char_to_byte(&dt, (dpos + 1).min(dt.chars().count()));
        if let Some(at) = p.rects_for_range(byte, end_byte, RectStyle::Tight).first() {
            return Some((*at, at.left));
        }
        if dpos > 0 {
            let b2 = char_to_byte(&dt, dpos - 1);
            if let Some(prev) = p.rects_for_range(b2, byte, RectStyle::Tight).first() {
                return Some((*prev, prev.right));
            }
        }
        None
    }

    /// 光标所在软行的索引（**按字节偏移**查，不用光标矩形去碰行盒）。
    ///
    /// 空行没有字形簇，光标矩形会退化到上一行末尾；拿它反推行号会少算一行，
    /// 于是"上键跳不到空行、下键从空行直接跳走"。偏移是先天确定的，不受
    /// 有没有字形影响——这也是 TS/Kotlin 用 `logicalIndex` 判定行号的原因。
    fn caret_line_index(&self) -> Option<usize> {
        let p = self.paragraph()?;
        let fi = self.focus_index()?;
        let dt = self.display_text();
        let dpos = self.logic_to_display_index(fi);
        p.line_index_of_byte(char_to_byte(&dt, dpos))
    }

    /// 第 `[index]` 软行：取行内横向点 + `preferredX` 求字素位置。
    ///
    /// `position_in_line` 把行范围当硬边界：空行返回行首。用全局坐标查找会
    /// 在空行上兜底到文末，正是"下键从空行直接跳到最后"的原因。
    fn move_to_line(&self, index: usize, extend: bool) {
        let p = match self.paragraph() {
            Some(p) => p,
            None => return,
        };
        let lines = p.line_metrics();
        if lines.is_empty() {
            return;
        }
        let i = index.min(lines.len() - 1);
        let dt = self.display_text();
        let px = self.preferred_x().unwrap_or(0.0);
        let byte = p.position_in_line(i, px);
        let dpos = byte_to_char(&dt, byte);
        let logic = self.display_to_logic_index(dpos);
        self.move_to(logic, extend);
    }

    /// 上下移动一行（保持 `preferredX` 视觉列）。
    fn vertical_move(&self, dir: i32, extend: bool) {
        let m = match self.caret_metrics() {
            Some(m) => m,
            None => {
                self.move_to(if dir < 0 { 0 } else { self.text_len() }, extend);
                return;
            }
        };
        if self.preferred_x().is_none() {
            self.set_preferred_x(Some(m.1));
        }
        let lines = match self.paragraph() {
            Some(p) => p.line_metrics(),
            None => return,
        };
        if lines.len() <= 1 {
            return;
        }
        let i = match self.caret_line_index() {
            Some(i) => i as i32,
            None => return,
        };
        let j = i + dir;
        if j < 0 || j >= lines.len() as i32 {
            return;
        }
        self.move_to_line(j as usize, extend);
    }

    fn move_up(&self) {
        self.vertical_move(-1, false);
    }

    fn move_down(&self) {
        self.vertical_move(1, false);
    }

    fn select_up(&self) {
        self.vertical_move(-1, true);
    }

    fn select_down(&self) {
        self.vertical_move(1, true);
    }

    /// 跳 `count` 行（PageUp/PageDown）。
    fn jump_lines(&self, count: i32, extend: bool) {
        let m = match self.caret_metrics() {
            Some(m) => m,
            None => {
                self.move_to(if count < 0 { 0 } else { self.text_len() }, extend);
                return;
            }
        };
        if self.preferred_x().is_none() {
            self.set_preferred_x(Some(m.1));
        }
        let lines = match self.paragraph() {
            Some(p) => p.line_metrics(),
            None => return,
        };
        if lines.is_empty() {
            return;
        }
        let i = match self.caret_line_index() {
            Some(i) => i as i32,
            None => return,
        };
        let j = (i + count).clamp(0, lines.len() as i32 - 1);
        if j == i {
            return;
        }
        self.move_to_line(j as usize, extend);
    }

    /// 第 `[pos]` 个字符所在软行范围 `[start, end)`（逻辑索引）。
    fn line_range_at(&self, pos: usize) -> Option<(usize, usize)> {
        let p = self.paragraph()?;
        if self.text_len() == 0 {
            return None;
        }
        let dt = self.display_text();
        let disp_pos = self.logic_to_display_index(pos);
        let lines = p.line_metrics();
        let m = lines
            .iter()
            .find(|l| {
                let s = byte_to_char(&dt, l.start_index);
                let e = byte_to_char(&dt, l.end_index);
                disp_pos >= s && disp_pos < e
            })
            .or_else(|| {
                if pos == self.text_len() {
                    lines.last()
                } else {
                    None
                }
            })?;
        let mut display_end = byte_to_char(&dt, m.end_index).min(dt.chars().count());
        if display_end > 0 {
            let ch = dt.chars().nth(display_end - 1);
            if ch == Some('\n') || ch == Some('\r') {
                display_end -= 1;
            }
        }
        let start = self.display_to_logic_index(byte_to_char(&dt, m.start_index));
        let end = self.display_to_logic_index(display_end);
        Some((start, end))
    }

    fn line_start(&self, pos: usize) -> usize {
        self.line_range_at(pos).map_or(0, |r| r.0)
    }

    fn line_end(&self, pos: usize) -> usize {
        self.line_range_at(pos).map_or(self.text_len(), |r| r.1)
    }

    // ---- 文本改写（经 undo/redo）----

    fn write_text(&self, new_value: &str) {
        if let Some(f) = &self.on_text_change {
            f(new_value);
        }
    }

    /// 在核上执行一次编辑动作；文本真的变了就回写外部。
    ///
    /// **借用只活在闭包内**：`write_text` 会调到宿主的 `on_text_change`，
    /// 那一刻还握着 `RefCell` 借用，重入就会 panic。返回 `true` = 文本变了。
    fn edit(&self, f: impl FnOnce(&mut EditCore)) -> bool {
        self.sync_params();
        // 借用只活在这一次 `with_core_mut` 里：`write_text` 调到宿主回调时
        // 借用已经释放，重入也不会撞 `RefCell already borrowed`。
        let changed = {
            let mut changed = None;
            self.with_core_mut(|c| {
                let before = c.text().to_string();
                f(c);
                changed = (c.text() != before).then(|| c.text().to_string());
            });
            changed
        };
        // 先把核内新光标 / 选区发布为控制器信号，**早于** `write_text`。
        //
        // `write_text` 会经 `on_text_change` 写外部 `text` 信号并触发**同步** flush；
        // 若此刻 `caret_pair` 还是旧值，订阅它的绘制 / 命中链在 flush 中读到的就是
        // 过期光标，下一次 `publish` 反而会把刚被编辑算好的新光标覆盖回旧值——
        // 表现为"插入换行后光标停在上一个位置、上下键因光标落在空矩形退化成乱跳"。
        // 先发布，保证任何 flush 期间 `caret_pair` 都已是新值（单一可信源原则：
        // 核 → `caret_pair` 单向流出）。
        self.publish_sel();
        match changed {
            Some(t) => {
                self.write_text(&t);
                true
            }
            None => false,
        }
    }

    /// 只动光标 / 选区的动作：不回写文本，但要让光标重新从"亮"开始闪。
    fn with_core(&self, f: impl FnOnce(&mut EditCore)) {
        self.sync_params();
        self.with_core_mut(f);
        // 光标 / 选区变了就发布到控制器信号（`caret_pair` 等）。
        self.publish_sel();
        self.restart_blink();
    }

    // ---- 文本改写（全部委托 `EditCore`，控制器不再自己拼字符串）----

    fn insert_text(&self, inserted: &str) {
        if (self.read_only)() {
            return;
        }
        self.edit(|c| c.insert_text(inserted));
        self.restart_blink();
    }

    fn backspace(&self) {
        self.edit(|c| c.backspace());
        self.restart_blink();
    }

    fn delete_forward(&self) {
        self.edit(|c| c.delete());
        self.restart_blink();
    }

    fn delete_word_backward(&self) {
        self.edit(|c| c.delete_word_backward());
        self.restart_blink();
    }

    fn delete_word_forward(&self) {
        self.edit(|c| c.delete_word_forward());
        self.restart_blink();
    }

    // ---- 光标 / 选区导航 ----

    fn move_left(&self) {
        self.with_core(|c| c.move_left());
    }

    fn move_right(&self) {
        self.with_core(|c| c.move_right());
    }

    fn move_prev_word(&self) {
        self.with_core(|c| c.move_prev_word());
    }

    fn move_next_word(&self) {
        self.with_core(|c| c.move_next_word());
    }

    fn select_left(&self) {
        self.with_core(|c| c.select_left());
    }

    fn select_right(&self) {
        self.with_core(|c| c.select_right());
    }

    // ---- 撤销 / 重做（组合态内的拦截在核里）----

    fn undo(&self) {
        self.edit(|c| c.undo());
        self.restart_blink();
    }

    fn redo(&self) {
        self.edit(|c| c.redo());
        self.restart_blink();
    }

    // ---- 选区吸收 / IME（逻辑全在核里，这里只做全局选区吸收与外部回写）----

    /// 若本地光标未定位，吸收全局（指针）选区为本地点对。
    ///
    /// 纯逻辑层的 `on_composing` 只看得见本地 `anchor`/`focus`，所以必须先把跨组件
    /// 选区写进核，组合窗口才会落在用户真正选中的那一段上。
    fn absorb_global_selection(&self) {
        if self.anchor_index().is_some() && self.focus_index().is_some() {
            return;
        }
        if let Some(m) = self.manager() {
            if let Some((a, b)) = m.range_of(&self.self_sel()) {
                self.with_core_mut(|c| c.select_range(a, b));
                // 跨组件选区已写进核，发布到控制器信号让派生量同步。
                self.publish_sel();
            }
        }
    }

    fn on_composing(&self, t: &str, cursor_position: usize) {
        if (self.read_only)() {
            return;
        }
        self.absorb_global_selection();
        self.edit(|c| c.on_composing(t, cursor_position));
        self.restart_blink();
    }

    /// 提交组合串：清掉组合态，保留已上屏的窗口文本。
    fn commit_composition(&self) {
        self.edit(|c| c.commit_composition());
        self.restart_blink();
    }

    /// 结束组合：`restore = true` 还原被替换的原文（取消语义）。
    fn end_composition(&self, restore: bool) {
        self.edit(|c| c.end_composition(restore));
        self.restart_blink();
    }

    fn commit_composing_text(&self, final_text: &str) {
        if !final_text.is_empty() {
            self.on_composing(final_text, final_text.chars().count());
        }
        self.commit_composition();
    }

    /// 鼠标按下：落点定位光标并塌缩（已有分配选区时由拖拽接管）。
    fn mouse_down(&self, local_x: f32, local_y: f32) {
        self.set_preferred_x(None);
        if let Some(m) = self.manager() {
            if let Some((a, b)) = m.range_of(&self.self_sel()) {
                if b > a {
                    return;
                }
            }
        }
        let idx = self.position_for_point(local_x as f64, local_y as f64);
        self.set_cursor(idx);
    }

    /// 声明尺寸（对标 TS `size(d)`）。
    fn size(&self, d: Axis) -> LayoutSize {
        let p = self.paragraph();
        match d {
            Axis::X => {
                if self.has_explicit_width {
                    LayoutSize::outer((self.explicit_width)())
                } else {
                    LayoutSize::inside(p.as_ref().map_or(0.0, |p| p.width()))
                }
            }
            Axis::Y => {
                if self.has_explicit_height {
                    LayoutSize::outer((self.explicit_height)())
                } else {
                    LayoutSize::inside(
                        p.as_ref()
                            .map_or_else(|| (self.font_size)() * 1.4, |p| p.height()),
                    )
                }
            }
        }
    }

    /// 光标可见性：从 [`Self::blink_anchor`] 起每 [`CARET_BLINK_MS`] 翻转一次。
    ///
    /// 锚点在"获得焦点 / 光标移动"时重置，所以刚点进输入框的那一下必然是亮相；
    /// 之后靠宿主按同一周期请求重绘来翻转（等价于 TS 的 `repeatEvery`）。
    /// 没有锚点（从未聚焦过）时退回墙钟相位，保持与旧行为一致。
    fn cursor_visible(&self) -> bool {
        match self.blink_anchor.get() {
            Some(at) => Self::phase_visible(at, Instant::now()),
            // 没有锚点（从未聚焦过）：退回墙钟相位。
            None => {
                let ms = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map(|d| d.as_millis() as u64)
                    .unwrap_or(0);
                (ms / CARET_BLINK_MS).is_multiple_of(2)
            }
        }
    }

    /// 相位数学：从 `anchor` 起 `[0, 530)` 亮、`[530, 1060)` 灭，如此循环。
    fn phase_visible(anchor: Instant, now: Instant) -> bool {
        let ms = now.saturating_duration_since(anchor).as_millis() as u64;
        (ms / CARET_BLINK_MS).is_multiple_of(2)
    }

    /// 重置闪烁相位：下一次绘制必然画出光标（"刚点进去"不该有空窗）。
    fn restart_blink(&self) {
        self.blink_anchor.set(Some(Instant::now()));
    }

    /// 绘制（选区高亮 → 文本 → 光标 → 组合串）。
    fn draw(&self, s: &mut Scene, n: &Node2) {
        // 刚获得焦点（点击 / Tab 进来）的那一帧把闪烁相位归零：
        // 否则第一帧可能正好落在"灭"相，看上去像没聚焦。
        let focused_now = focus::is_focused(n);
        if focused_now != self.was_focused.get() {
            self.was_focused.set(focused_now);
            if focused_now {
                self.restart_blink();
            }
        }
        let p = self.paragraph();
        // 光标位置必须作为绘制依赖被**无条件读取一次**：`draw` 经 `range_of`
        // 读的是选区 memo，而塌缩光标（纯光标、无选区）下移动光标不会改变该
        // memo 的输出，于是绘制 RecordMemo 收不到任何信号变化、复用旧帧，光标
        // 停在上一位置甚至消失。这里直接读 `caret_pair`，让它成为绘制依赖——
        // 光标一动就触发重绘，光标跟上。详见 docs/08 §10「单一可置信信号源」。
        let _caret_tracked = self.cursor();
        let pad_x = n.padding_inline_start();
        let pad_y = n.padding_block_start();
        // 文本原点 y = padding 起点 + 单行墨迹补偿（TS `textOriginY`）。
        // 绘制、选区、光标、组合串必须共用它，否则"看到的字"和"点到的位置"会错开。
        let origin_y = pad_y + self.ink_shift_y();
        let scroll_x = self.compute_scroll_x();
        if (self.single_line)() {
            s.push_clip(Rect::new(
                pad_x,
                pad_y,
                n.inner_size(Axis::X).max(0.0),
                n.inner_size(Axis::Y).max(0.0),
            ));
        }
        s.push_transform(Point::new(-scroll_x, 0.0));

        let focused = focused_now;
        let range = self.manager().and_then(|m| m.range_of(&self.self_sel()));

        if let Some((rs, re)) = range {
            if re > rs {
                if let Some(p) = &p {
                    let dt = self.display_text();
                    let sb = char_to_byte(&dt, self.logic_to_display_index(rs));
                    let eb = char_to_byte(&dt, self.logic_to_display_index(re));
                    let color = (self.selection_color)();
                    for r in p.rects_for_range(sb, eb, RectStyle::Tight) {
                        s.fill_rect(
                            Rect::new(
                                r.left + pad_x,
                                r.top + origin_y,
                                r.right - r.left,
                                r.bottom - r.top,
                            ),
                            color,
                        );
                    }
                }
            }
        }

        if let Some(p) = &p {
            s.draw_paragraph(Point::new(pad_x, origin_y), p.shared_layout());
        }

        // 光标：无分配选区且聚焦且（闪烁）可见时绘制。
        //
        // **复用 `caret_rect()` 这一唯一几何源**——它已含末尾空行 / 行首的兜底
        // （parley 不给末尾换行处的字形矩形，要在「下一行行首」造一个零宽矩形）。
        // 此前这里另抄了一份内联 `rects_for_range`，漏掉了换行分支，于是换行后
        // 光标被画在上一行末尾、上下键在行边界退化。单一几何源杜绝这类分叉。
        if range.is_none() && focused && self.cursor_visible() {
            if let Some(r) = self.caret_rect().into_iter().next() {
                let col = text_color();
                s.fill_rect(
                    Rect::new(
                        r.left + pad_x,
                        r.top + origin_y,
                        CURSOR_WIDTH,
                        r.bottom - r.top,
                    ),
                    col,
                );
            }
        }

        // 组合串高亮 + 下划线。
        // 显示区间由核直接给（`composing_display_range`），与光标绘制共用同一套
        // 「逻辑索引 → 显示索引」换算，避免这里再手算一次起点+长度。
        let composing_range = self.core.borrow().composing_display_range();
        if let Some((cs, ce)) = composing_range {
            if let Some(p) = &p {
                let dt = self.display_text();
                let sb = char_to_byte(&dt, cs);
                let eb = char_to_byte(&dt, ce);
                let bg = Color::rgba(200, 200, 255, 50);
                let ul = Color::rgba(0, 0, 0, 140);
                for r in p.rects_for_range(sb, eb, RectStyle::Tight) {
                    s.fill_rect(
                        Rect::new(
                            r.left + pad_x,
                            r.top + origin_y,
                            r.right - r.left,
                            r.bottom - r.top,
                        ),
                        bg,
                    );
                    s.fill_rect(
                        Rect::new(
                            r.left + pad_x,
                            (r.bottom - 2.0).max(r.top) + origin_y,
                            r.right - r.left,
                            2.0,
                        ),
                        ul,
                    );
                }
            }
        }

        s.pop_transform();
        if (self.single_line)() {
            s.pop_clip();
        }
    }

    /// 键盘路由（返回 `true` = 已消费）。
    fn handle_key(&self, e: &KeyEvent2) -> bool {
        if (self.read_only)() {
            return false;
        }
        self.absorb_global_selection();
        let key = e.key.to_lowercase();
        if e.ctrl && !e.shift && key == "z" {
            self.undo();
            return true;
        }
        if (e.ctrl && key == "y") || (e.ctrl && e.shift && key == "z") {
            self.redo();
            return true;
        }
        match e.code.as_str() {
            KeyCode2::BACKSPACE => {
                if e.ctrl || e.alt {
                    self.delete_word_backward();
                } else {
                    self.backspace();
                }
                self.set_preferred_x(None);
                true
            }
            KeyCode2::DELETE => {
                if e.ctrl || e.alt {
                    self.delete_word_forward();
                } else {
                    self.delete_forward();
                }
                self.set_preferred_x(None);
                true
            }
            KeyCode2::LEFT => {
                if e.ctrl && e.shift {
                    self.with_core(|c| c.select_prev_word());
                } else if e.ctrl {
                    self.move_prev_word();
                } else if e.shift {
                    self.select_left();
                } else {
                    self.move_left();
                }
                self.set_preferred_x(None);
                true
            }
            KeyCode2::RIGHT => {
                if e.ctrl && e.shift {
                    self.with_core(|c| c.select_next_word());
                } else if e.ctrl {
                    self.move_next_word();
                } else if e.shift {
                    self.select_right();
                } else {
                    self.move_right();
                }
                self.set_preferred_x(None);
                true
            }
            KeyCode2::HOME => {
                if e.ctrl && e.shift {
                    self.extend_to(0);
                } else if e.ctrl {
                    self.set_cursor(0);
                } else if e.shift {
                    self.extend_to(self.line_start(self.focus_index().unwrap_or(0)));
                } else {
                    self.set_cursor(self.line_start(self.focus_index().unwrap_or(0)));
                }
                self.set_preferred_x(None);
                true
            }
            KeyCode2::END => {
                if e.ctrl && e.shift {
                    self.extend_to(self.text_len());
                } else if e.ctrl {
                    self.set_cursor(self.text_len());
                } else if e.shift {
                    self.extend_to(self.line_end(self.focus_index().unwrap_or(0)));
                } else {
                    self.set_cursor(self.line_end(self.focus_index().unwrap_or(0)));
                }
                self.set_preferred_x(None);
                true
            }
            KeyCode2::PAGE_UP => {
                self.jump_lines(-PAGE_LINES, e.shift);
                true
            }
            KeyCode2::PAGE_DOWN => {
                self.jump_lines(PAGE_LINES, e.shift);
                true
            }
            KeyCode2::UP => {
                if e.shift {
                    self.select_up();
                } else {
                    self.move_up();
                }
                true
            }
            KeyCode2::DOWN => {
                if e.shift {
                    self.select_down();
                } else {
                    self.move_down();
                }
                true
            }
            KeyCode2::ENTER => {
                // 单行：回车即提交；多行：Ctrl/Cmd+Enter 才提交，裸回车换行。
                if (self.single_line)() || e.ctrl || e.meta {
                    if let Some(f) = &self.on_submit {
                        f();
                    }
                } else {
                    self.insert_text("\n");
                }
                self.set_preferred_x(None);
                true
            }
            KeyCode2::TAB => {
                if (self.tab_insert)() && !(self.single_line)() {
                    self.insert_text("\t");
                    true
                } else {
                    false
                }
            }
            _ => {
                if e.ctrl || e.alt {
                    return false;
                }
                // 组合态下键盘由输入法独占，不再按可打印字符插入。
                if self.core.borrow().in_composing() {
                    return false;
                }
                // 只认单字符（与旧版 canvasRender 同口径）。
                let code = if e.key.chars().count() == 1 {
                    e.key.chars().next().unwrap() as u32
                } else {
                    0
                };
                if code < 0x20 || code == 0x7f {
                    return false;
                }
                self.set_preferred_x(None);
                self.end_composition(false);
                self.insert_text(&e.key);
                true
            }
        }
    }
}

// ============================================================================
// Selectable2（供 SelectionManager2 派生全局选区）
// ============================================================================

impl Selectable2 for EditableTextController {
    fn selection_rect(&self) -> Option<Rect2> {
        let m = self.manager()?;
        let (rs, re) = m.range_of(&self.self_sel())?;
        if re <= rs {
            return None;
        }
        let p = self.paragraph()?;
        let dt = self.display_text();
        let sb = char_to_byte(&dt, self.logic_to_display_index(rs));
        let eb = char_to_byte(&dt, self.logic_to_display_index(re));
        let rects = p.rects_for_range(sb, eb, RectStyle::Tight);
        if rects.is_empty() {
            return None;
        }
        let first = &rects[0];
        let last = &rects[rects.len() - 1];
        let x = self.node().abs_x() + self.node().padding_inline_start() - self.compute_scroll_x();
        let y = self.node().abs_y() + self.text_origin_y();
        Some(RectF::new(
            x + first.left.min(last.left),
            y + first.top.min(last.top),
            x + first.right.max(last.right),
            y + first.bottom.max(last.bottom),
        ))
    }

    fn text_length(&self) -> usize {
        self.text_len()
    }

    fn position_for_point(&self, local_x: f64, local_y: f64) -> usize {
        let p = match self.paragraph() {
            Some(p) => p,
            None => return 0,
        };
        let dt = self.display_text();
        let off = p.glyph_position_at_coordinate(
            local_x as f32 - self.node().padding_inline_start() + self.compute_scroll_x(),
            local_y as f32 - self.text_origin_y(),
        );
        let char_off = byte_to_char(&dt, off);
        coerce(self.display_to_logic_index(char_off), 0, self.text_len())
    }

    fn text_in_range(&self, start: usize, end: usize) -> String {
        let t = (self.text)();
        t.chars()
            .skip(start)
            .take(end.saturating_sub(start))
            .collect()
    }

    fn word_range_at(&self, offset: usize) -> Option<(usize, usize)> {
        Words::word_range_at(&(self.text)(), offset)
    }

    fn paragraph_range_at(&self, offset: usize) -> Option<(usize, usize)> {
        let t = (self.text)();
        if t.is_empty() {
            return None;
        }
        let d = coerce(offset, 0, t.chars().count());
        // 光标所在行的起点 = 光标之前**最后一个换行**之后的位置。
        let start = t
            .chars()
            .take(d)
            .enumerate()
            .fold(0usize, |acc, (i, c)| if c == '\n' { i + 1 } else { acc });
        let nl = t
            .chars()
            .skip(d)
            .position(|c| c == '\n')
            .map_or(t.chars().count(), |i| d + i);
        Some((start, nl))
    }
}

// ============================================================================
// EditableTextHandle（供引擎路由键盘 / IME / 剪贴板快捷键）
// ============================================================================

impl EditableTextHandle for EditableTextController {
    fn cursor_sel_pair(&self) -> Option<SelPair2> {
        if !focus::is_focused(&self.node()) {
            return None;
        }
        let a = self.anchor_index()?;
        let f = self.focus_index()?;
        let len = self.text_len();
        let sel = self.self_sel();
        Some(SelPair2::new(
            SelPoint2::new(Rc::clone(&sel), coerce(a, 0, len)),
            SelPoint2::new(sel, coerce(f, 0, len)),
        ))
    }

    fn caret_rect(&self) -> Vec<TextRect> {
        // 复用固有几何源，避免又抄一份（此前 `draw` 内联副本漏了换行分支）。
        EditableTextController::caret_rect(self)
    }

    fn has_selection(&self) -> bool {
        if let Some(m) = self.manager() {
            if let Some((s, e)) = m.range_of(&self.self_sel()) {
                return e > s;
            }
        }
        false
    }

    fn cut(&self) {
        if (self.read_only)() {
            return;
        }
        self.absorb_global_selection();
        if !self.local_has_sel() {
            return;
        }
        clipboard_set_text(&self.text_in_range(self.local_sel_start(), self.local_sel_end()));
        self.edit(|c| c.delete_selection());
    }

    fn copy(&self) {
        self.absorb_global_selection();
        if !self.local_has_sel() {
            return;
        }
        clipboard_set_text(&self.text_in_range(self.local_sel_start(), self.local_sel_end()));
    }

    fn paste(&self) {
        let t = clipboard_get_text();
        if t.is_empty() {
            return;
        }
        self.set_preferred_x(None);
        self.end_composition(false);
        self.insert_text(&t);
    }

    fn select_all(&self) {
        self.with_core(|c| c.select_all());
    }

    fn select_range(&self, anchor_offset: usize, focus_offset: usize) {
        self.set_preferred_x(None);
        self.with_core(|c| c.select_range(anchor_offset, focus_offset));
    }

    fn collapse_external_selection(&self) {
        self.with_core(|c| c.collapse_selection());
    }

    fn on_composing(&self, text: &str, cursor: usize) {
        self.on_composing(text, cursor);
    }

    fn commit_composing_text(&self, final_text: &str) {
        self.commit_composing_text(final_text);
    }

    fn input_overlay(&self) -> Option<InputOverlayRect> {
        let r = self.caret_local_rect();
        Some(InputOverlayRect {
            x: self.node().abs_x()
                + r.as_ref()
                    .map_or_else(|| self.node().padding_inline_start(), |r| r.x),
            y: self.node().abs_y() + r.as_ref().map_or_else(|| self.text_origin_y(), |r| r.y),
            w: r.as_ref().map_or(1.0, |r| r.w),
            h: r.as_ref().map_or_else(|| (self.font_size)(), |r| r.h),
            font_size: (self.font_size)(),
        })
    }

    fn handle_key(&self, e: &KeyEvent2) -> bool {
        self.handle_key(e)
    }

    fn as_selectable(&self) -> Rc<dyn Selectable2> {
        self.self_sel()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canvas_render2::build_root_node2;
    use crate::canvas_render2::node2::Node2Props;
    // 编辑态搬进 `EditCore` 之后，顶层不再依赖 `wy_signal`；测试要自建信号，
    // 在这里单独引入（比让生产代码挂着用不到的 import 干净）。
    use wy_signal::{create_signal, GetValue, SetValue, Signal};

    /// 建一个最小的编辑器节点（挂到空根下取出来）。
    fn editor_node() -> Node2 {
        let root = build_root_node2(Node2Props::default(), |cx| {
            cx.child(editable_text_node! {
                text: String::new(),
                on_text_change: |_t: &str| {},
                placeholder: "x".to_string(),
                single_line: true,
                width: 120.0,
                height: 24.0,
            });
        });
        root.children().first().expect("编辑器应挂在根下").clone()
    }

    // ---- IME 组合输入 ----
    //
    // 走的都是**引擎同一条路**：事件经节点上的 [`ImeExt`] 进来，光标 / 选区经
    // [`EditableTextHandle`]（`focus::active_editor()`）读，不碰控制器内部字段。
    //
    // 行为口径以 **Kotlin** `EditableComposingTest` 为准：预编辑串是"窗口替换"，
    // 空串上报只移除窗口。TS 只在"空串 = 提交（保留窗口文本）"这一条上不同，
    // 那一条在 winit 下会残留拼音串，故跟 Kotlin（见 `on_composing` 的注释）。

    /// 单编辑器环境（对标 Kotlin 测试里的 `Env`）。
    struct Env {
        node: Node2,
        value: Signal<String>,
    }

    impl Env {
        /// 挂一个**受控**编辑器：文本真相源是外部信号，写入经 `on_text_change` 回传。
        ///
        /// 根给显式尺寸：几何是读时惰性算的（TS 同），根没有尺寸时子节点量到 0，
        /// 就没法验证"文字在内容盒里居中"这类依赖 `inner_size` 的东西。
        fn new(initial: &str) -> Self {
            Self::with_height(initial, 24.0)
        }

        /// 同 [`Self::new`]，但指定节点高度（验证"文字被推到盒中间"要一个明显高于行盒的盒子）。
        fn with_height(initial: &str, height: f32) -> Self {
            let value = create_signal(initial.to_string());
            let root = build_root_node2(
                Node2Props {
                    width: Some(always(400.0)),
                    height: Some(always(300.0)),
                    ..Node2Props::default()
                },
                move |cx| {
                    cx.child(editable_text_node! {
                        text: value.get(),
                        on_text_change: move |t: &str| value.set(t.to_string()),
                        single_line: true,
                        width: 200.0,
                        height: height,
                    });
                },
            );
            let node = root.children().first().expect("编辑器应挂在根下").clone();
            Self { node, value }
        }

        fn text(&self) -> String {
            self.value.get()
        }

        /// 编辑器句柄：先把焦点给本节点，再走引擎的 `active_editor()` 下转。
        fn handle(&self) -> Rc<dyn EditableTextHandle> {
            focus::request_focus(&self.node);
            focus::active_editor().expect("编辑器带 EditorHandleExt，聚焦后应取到句柄")
        }

        /// 光标（塌缩时即插入点）。
        fn caret(&self) -> usize {
            self.handle()
                .cursor_sel_pair()
                .map_or(0, |p| p.focus.offset)
        }

        fn select(&self, anchor: usize, focus: usize) {
            self.handle().select_range(anchor, focus);
        }

        /// 引擎路径：`ImeExt.on_ime`。
        fn ime(&self, ev: &mut ImeEvent) -> bool {
            let ext = self
                .node
                .find_ext_rc::<ImeExt>()
                .expect("编辑器必须带 ImeExt");
            (ext.on_ime)(ev)
        }

        fn preedit(&self, text: &str) -> bool {
            self.ime(&mut ImeEvent::Preedit {
                text: text.to_string(),
                cursor: Some((text.len(), text.len())),
            })
        }

        /// 平台上报"组合结束"（winit Windows 的 `Ime::Preedit("", None)`）。
        fn composing_end(&self) -> bool {
            self.ime(&mut ImeEvent::Preedit {
                text: String::new(),
                cursor: None,
            })
        }

        fn commit(&self, text: &str) -> bool {
            self.ime(&mut ImeEvent::Commit(text.to_string()))
        }
    }

    #[test]
    fn editor_is_always_focusable() {
        // `pointer::down` 只把焦点给「命中链里最近的可聚焦节点」。
        // 开关在 props 里是 `Get<bool>`（默认 `always(false)`），组件体用
        // `focusable: true` 无条件覆盖；一旦丢了这一行就是默认 false →
        // 点不进去 → 永远打不了字。
        assert!(editor_node().focusable(), "编辑器必须可聚焦");
    }

    /// 回归：**第一次**组合就 panic（`RefCell already borrowed`）。
    ///
    /// `on_composing` 曾在 `if let` 的条件里直接 `borrow()` 快照，edition 2021
    /// 下那个临时 `Ref` 活到整个 `if let`（含 `else` 分支）结束，于是 `else` 里
    /// 存 `composition_base` 的 `borrow_mut()` 撞上自己——一打拼音就崩。
    #[test]
    fn ime_first_preedit_does_not_panic() {
        let env = Env::new("");
        assert!(env.preedit("zhong"), "组合事件应被消费");
        assert_eq!(
            env.text(),
            "zhong",
            "组合串立刻进正文（与 TS / Kotlin 一致）"
        );
    }

    /// 多帧演进：后一帧整体替换前一帧的窗口；空串上报只移除窗口。
    ///
    /// 对标 Kotlin `composingFramesReplaceWindowAndCommitRemovesIt`。
    #[test]
    fn composing_frames_replace_window_and_empty_preedit_removes_it() {
        let env = Env::new("abXY");
        env.select(2, 2); // 光标落到 X 前（Kotlin `moveCursorTo(2)`）

        env.preedit("pin");
        assert_eq!(env.text(), "abpinXY", "首帧在光标处插入预编辑串");
        assert_eq!(env.caret(), 5, "光标跟随到预编辑尾");

        env.preedit("ping");
        assert_eq!(env.text(), "abpingXY", "第二帧整体替换第一帧窗口，不追加");

        env.composing_end();
        assert_eq!(env.text(), "abXY", "空串上报移除窗口、还原原文本");
        assert_eq!(env.caret(), 2, "光标回到基座插入点");

        env.preedit("品");
        assert_eq!(env.text(), "ab品XY", "提交后从基座重新开窗");
    }

    /// 组合起点有本地选区：窗口替换选区；移除窗口后选区内容不回来
    /// （"取消还原"是 `end_composition(restore = true)` 的显式语义，TS 与 Kotlin
    /// 里同样没有事件接进去，只有测试直接调用）。
    ///
    /// 对标 Kotlin `composingOverSelectionReplacesAndCancelRestores` 的前一半。
    #[test]
    fn composing_over_selection_replaces_it() {
        let env = Env::new("abXY");
        env.select(2, 4); // 选中 "XY"

        env.preedit("x");
        assert_eq!(env.text(), "abx", "预编辑替换被选中的 XY");
        assert_eq!(env.caret(), 3);

        env.composing_end();
        assert_eq!(
            env.text(),
            "ab",
            "移除窗口即撤掉预编辑串（已确认文本随后插入）"
        );
        assert_eq!(env.caret(), 2, "光标回到基座插入点");

        env.commit("选");
        assert_eq!(env.text(), "ab选", "提交文本落在基座上");
    }

    /// winit Windows 真实序列：`Preedit("")` 清窗后再 `Commit(text)`。
    ///
    /// TS 的 `commitComposition()` 在这条路上会把拼音串留在正文里（得到
    /// `nihao你好`），Kotlin 的 `endComposition(false)` 才是对的。
    #[test]
    fn winit_clear_then_commit_writes_final_text() {
        let env = Env::new("");
        env.preedit("nihao");
        assert_eq!(env.text(), "nihao");

        env.composing_end();
        assert_eq!(env.text(), "", "清窗把组合串撤掉");

        env.commit("你好");
        assert_eq!(env.text(), "你好", "已确认文本经常规提交路径写入");
        assert_eq!(env.caret(), 2);
    }

    /// 组合进行中 undo / redo 必须被抑制（对标 Kotlin `undoRedoSuppressedDuringComposing`）。
    #[test]
    fn undo_redo_suppressed_during_composing() {
        let env = Env::new("ab");
        env.select(2, 2);
        env.preedit("pin");
        assert_eq!(env.text(), "abpin");

        let handle = env.handle();
        let ctrl_z = KeyEvent2 {
            key: "z".to_string(),
            code: "KeyZ".to_string(),
            ctrl: true,
            shift: false,
            alt: false,
            meta: false,
        };
        assert!(handle.handle_key(&ctrl_z), "组合中 Ctrl+Z 仍算已消费");
        assert!(env.text().contains("pin"), "组合中 undo 不得改写正文");
    }

    // ---- 垂直居中 / IME 候选框几何 ----

    /// 单行编辑器：文字（以及跟着它的光标、IME 候选框）要被墨迹补偿推到内容盒中线。
    ///
    /// 不补偿时文字贴在盒顶——`height: 34` 的输入框里字形只占最上面 12px，
    /// 这就是"输入框文字没上下居中"的根因（TS `inkShiftYOf` 注释同款）。
    /// 这里用 60 高的盒子，补偿前后差距（≈5 vs ≈24）一目了然。
    #[test]
    fn single_line_text_is_vertically_centered() {
        let env = Env::with_height("abc", 60.0);
        env.select(1, 1); // 定位光标，`caret_local_rect` 才有值
        let (_, y, _, _) = env.node.ime_cursor_rect().expect("编辑器必须带 IME 光标盒");
        let inner = env.node.inner_size(Axis::Y);
        assert_eq!(inner, 60.0, "根给了尺寸，编辑器高度应算出来");
        assert!(
            y > 15.0 && y < 32.0,
            "文本原点应被推到盒子中部（约 (60-行高)/2 ≈ 24），实际 y={y}"
        );
        assert!(y <= inner, "文本原点不应跑出内容盒，实际 y={y}");
    }

    /// 空文本时退到文本原点：候选框不能跑到节点外。
    #[test]
    fn ime_cursor_rect_on_empty_editor_stays_inside() {
        let env = Env::new("");
        let (x, y, _, _) = env.node.ime_cursor_rect().expect("编辑器必须带 IME 光标盒");
        assert!(
            x >= 0.0 && y >= 0.0,
            "空文本时光标盒落在原点，实际 ({x}, {y})"
        );
        assert!(y <= 24.0, "空文本时光标盒不应超出节点，实际 y={y}");
    }

    /// 画一帧，数出光标矩形（宽 = [`CURSOR_WIDTH`] 的填充矩形）。
    fn caret_rects(scene: &Scene) -> usize {
        scene
            .iter()
            .filter(|p| {
                matches!(p, wy_render::Primitive::Rect { rect, .. }
                    if (rect.width - CURSOR_WIDTH).abs() < 0.01)
            })
            .count()
    }

    /// 聚焦后的**第一帧**必须画得出光标。
    ///
    /// 可见性只按墙钟 `(ms / 530) % 2` 算，若相位锚点不随聚焦归零，点进去的那一帧
    /// 有 50% 概率落在"灭"相 —— 而且没有后续重绘，就成了"点了输入框却没有光标"。
    #[test]
    fn caret_is_drawn_on_the_first_frame_after_focus() {
        let env = Env::new("ab");
        let mut idle = Scene::new();
        env.node.render_node2(&mut idle);
        assert_eq!(caret_rects(&idle), 0, "未聚焦时不应画光标");

        focus::request_focus(&env.node);
        let mut focused = Scene::new();
        env.node.render_node2(&mut focused);
        assert_eq!(caret_rects(&focused), 1, "聚焦后的第一帧必须画出光标");
    }

    /// 单行回车 = 提交，不插入换行；多行回车 = 换行，Ctrl/Cmd+Enter 才提交。
    ///
    /// 这一条是"回车加入列表"的全部机制：面板那边只是给了 `on_submit`。
    /// 单行分支与"多行 + Ctrl"分支合并成一个条件，但**语义是两条**——
    /// 单行里按不按 Ctrl 都提交（不存在"单行换行"这种状态）。
    #[test]
    fn enter_submits_single_line_and_ctrl_enter_submits_multi_line() {
        use std::cell::Cell;

        let submits = Rc::new(Cell::new(0u32));
        let make = |single_line: bool| {
            let submits = Rc::clone(&submits);
            let value = create_signal("ab".to_string());
            let root = build_root_node2(
                Node2Props {
                    width: Some(always(400.0)),
                    height: Some(always(300.0)),
                    ..Node2Props::default()
                },
                move |cx| {
                    cx.child(editable_text_node! {
                        text: value.get(),
                        on_text_change: move |t: &str| value.set(t.to_string()),
                        single_line: single_line,
                        on_submit: move || submits.set(submits.get() + 1),
                        width: 200.0,
                        height: 60.0,
                    });
                },
            );
            let node = root.children().first().expect("编辑器应挂在根下").clone();
            (node, value)
        };
        let key = |code: &str, ctrl: bool| KeyEvent2 {
            key: String::new(),
            code: code.to_string(),
            ctrl,
            shift: false,
            alt: false,
            meta: false,
        };

        // 单行：回车提交、正文不变（不插 "\n"）。
        let (single, single_text) = make(true);
        assert!(
            single.handle_key(&key(KeyCode2::ENTER, false)),
            "回车应被消费"
        );
        assert_eq!(submits.get(), 1, "单行回车必须触发 on_submit");
        assert_eq!(single_text.get(), "ab", "单行不插入换行");

        // 多行：裸回车换行、不提交。
        let (multi, multi_text) = make(false);
        // 光标默认在 0，先挪到末尾，"插在末尾"才读得直白。
        focus::request_focus(&multi);
        focus::active_editor()
            .expect("编辑器聚焦后应取到句柄")
            .select_range(2, 2);
        assert!(
            multi.handle_key(&key(KeyCode2::ENTER, false)),
            "回车应被消费"
        );
        assert_eq!(submits.get(), 1, "裸回车在多行里不该提交");
        assert_eq!(multi_text.get(), "ab\n", "多行裸回车插入换行");

        // 多行 + Ctrl：提交，不换行。
        assert!(
            multi.handle_key(&key(KeyCode2::ENTER, true)),
            "Ctrl+回车应被消费"
        );
        assert_eq!(submits.get(), 2, "Ctrl+回车应触发 on_submit");
        assert_eq!(multi_text.get(), "ab\n", "Ctrl+回车不插入换行");
    }

    /// 闪烁相位：亮 530ms、灭 530ms，循环。
    #[test]
    fn caret_phase_flips_every_period() {
        use std::time::Duration;
        let t0 = Instant::now();
        let at = |ms: u64| t0 + Duration::from_millis(ms);
        assert!(
            EditableTextController::phase_visible(t0, t0),
            "0ms 应是亮相"
        );
        assert!(
            EditableTextController::phase_visible(t0, at(CARET_BLINK_MS - 1)),
            "第一个周期末尾仍是亮相"
        );
        assert!(
            !EditableTextController::phase_visible(t0, at(CARET_BLINK_MS + 1)),
            "跨过周期应翻成灭相"
        );
        assert!(
            EditableTextController::phase_visible(t0, at(CARET_BLINK_MS * 2 + 1)),
            "两个周期后回到亮相"
        );
    }

    /// 多行编辑器（`single_line: false`，足够高），供行内移动类测试共用。
    fn multiline_node(initial: &str) -> Node2 {
        let value = create_signal(initial.to_string());
        let root = build_root_node2(
            Node2Props {
                width: Some(always(400.0)),
                height: Some(always(300.0)),
                ..Node2Props::default()
            },
            move |cx| {
                cx.child(editable_text_node! {
                    text: value.get(),
                    on_text_change: move |t: &str| value.set(t.to_string()),
                    single_line: false,
                    width: 200.0,
                    height: 120.0,
                });
            },
        );
        root.children().first().expect("编辑器应挂在根下").clone()
    }

    /// 空行是**落脚点**，不是跳过点：上下键必须逐行推进。
    ///
    /// 回归门：`move_to_line` 早先按坐标反查位置，空行没有字形簇 →
    /// `Cluster::from_point` 返回 `None` → 兜底"文末"。于是
    /// 「从 b 按上不动（跳不过空行）、从 a 按下直接跳到文末」，正是用户报的现象。
    ///
    /// 文本 `"a\n\nb"`：行 0 = `a`(0)、行 1 = 空行(2)、行 2 = `b`(3)。
    #[test]
    fn vertical_moves_stop_on_blank_lines() {
        let node = multiline_node("a\n\nb");
        focus::request_focus(&node);
        let h = focus::active_editor().expect("聚焦后应取到编辑器句柄");
        let caret = || h.cursor_sel_pair().map_or(0, |p| p.focus.offset);
        let key = |code: &str| KeyEvent2 {
            key: String::new(),
            code: code.to_string(),
            ctrl: false,
            shift: false,
            alt: false,
            meta: false,
        };

        h.select_range(0, 0);
        assert_eq!(caret(), 0, "起点在第一行行首");

        assert!(node.handle_key(&key(KeyCode2::DOWN)));
        assert_eq!(caret(), 2, "下键应停在空行上，不能跳到文末");
        assert!(node.handle_key(&key(KeyCode2::DOWN)));
        assert_eq!(caret(), 3, "从空行再按下应到 b 的开头");
        assert!(node.handle_key(&key(KeyCode2::UP)));
        assert_eq!(caret(), 2, "上键应回到空行，不能原地不动");
        assert!(node.handle_key(&key(KeyCode2::UP)));
        assert_eq!(caret(), 0, "再按上应回到第一行");
    }

    // ---- 普通按键路径（输入 / 退格 / 撤销 / 重做 / 选区替换）----
    //
    // 这些是"用户每天都在按的键"，却一条测试都没有：IME、空行、闪烁都有覆盖，
    // 反而最基础的那条路没人看着。编辑器从自带实现切到 `EditCore` 之后，
    // 这里正是"看起来能打字、其实某一步歪了"最容易漏掉的地方。

    fn ch(c: char) -> KeyEvent2 {
        KeyEvent2 {
            key: c.to_string(),
            code: String::new(),
            ctrl: false,
            shift: false,
            alt: false,
            meta: false,
        }
    }

    fn ctrl(c: char) -> KeyEvent2 {
        KeyEvent2 {
            key: c.to_string(),
            code: String::new(),
            ctrl: true,
            shift: false,
            alt: false,
            meta: false,
        }
    }

    fn code(code: &str) -> KeyEvent2 {
        KeyEvent2 {
            key: String::new(),
            code: code.to_string(),
            ctrl: false,
            shift: false,
            alt: false,
            meta: false,
        }
    }

    /// 打字 → 退格 → 撤销 → 重做，文本与光标都要对得上。
    #[test]
    fn typing_backspace_undo_redo_round_trip() {
        let env = Env::new("");
        env.select(0, 0);

        assert!(env.node.handle_key(&ch('a')));
        assert!(env.node.handle_key(&ch('b')));
        assert_eq!(env.text(), "ab");
        assert_eq!(env.caret(), 2, "打完字光标应在末尾");

        assert!(env.node.handle_key(&code(KeyCode2::BACKSPACE)));
        assert_eq!(env.text(), "a");
        assert_eq!(env.caret(), 1);

        // 撤销：一次按键一条历史（与旧实现同粒度），所以逐字回退
        assert!(env.node.handle_key(&ctrl('z')));
        assert_eq!(env.text(), "ab", "撤销退回上一步文本");
        assert!(env.node.handle_key(&ctrl('z')));
        assert_eq!(env.text(), "a");

        // 重做：Ctrl+Y 与 Ctrl+Shift+Z 都要能走
        assert!(env.node.handle_key(&ctrl('y')));
        assert_eq!(env.text(), "ab", "重做恢复被撤销的那次插入");
    }

    /// 中文退格：删一整个字符，不是删一个字节。
    #[test]
    fn backspace_deletes_whole_cjk_char() {
        let env = Env::new("你好");
        env.select(2, 2);
        assert!(env.node.handle_key(&code(KeyCode2::BACKSPACE)));
        assert_eq!(env.text(), "你");
        assert_eq!(env.caret(), 1);
    }

    /// 选区替换：有选区时打字即替换，撤销后原文回来（含中文）。
    #[test]
    fn typing_over_selection_replaces_and_undo_restores() {
        let env = Env::new("abc");
        env.select(0, 2);
        assert!(env.node.handle_key(&ch('x')));
        assert_eq!(env.text(), "xc");
        assert!(env.node.handle_key(&ctrl('z')));
        assert_eq!(env.text(), "abc", "撤销应还原被替换的两个字符");
    }

    /// 中间插入中文后撤销：两边字符不能被吃掉（字节索引 / 字符索引之分）。
    #[test]
    fn undo_after_mid_text_cjk_insert_keeps_surroundings() {
        let env = Env::new("abcd");
        env.select(1, 1);
        assert!(env.node.handle_key(&ch('你')));
        assert_eq!(env.text(), "a你bcd");
        assert!(env.node.handle_key(&ctrl('z')));
        assert_eq!(env.text(), "abcd");
    }

    /// 末尾空行（文本以 `\n` 结尾）同样可停、可离开。
    ///
    /// 文本 `"a\n"`：行 0 = `a`(0)，行 1 = 空行（= 文末，offset 2）。
    #[test]
    fn trailing_blank_line_is_reachable_and_escapable() {
        let node = multiline_node("a\n");
        focus::request_focus(&node);
        let h = focus::active_editor().expect("聚焦后应取到编辑器句柄");
        let caret = || h.cursor_sel_pair().map_or(0, |p| p.focus.offset);
        let key = |code: &str| KeyEvent2 {
            key: String::new(),
            code: code.to_string(),
            ctrl: false,
            shift: false,
            alt: false,
            meta: false,
        };

        h.select_range(0, 0);
        assert!(node.handle_key(&key(KeyCode2::DOWN)));
        assert_eq!(caret(), 2, "下键应停在末尾空行");
        // 已在最后一行：再按不动，也不能漂到别处
        assert!(node.handle_key(&key(KeyCode2::DOWN)));
        assert_eq!(caret(), 2, "最后一行再按下不应移动");
        assert!(node.handle_key(&key(KeyCode2::UP)));
        assert_eq!(caret(), 0, "上键能离开末尾空行");
    }

    /// 回归：换行后光标必须前进到新行，不能停留在上一行末尾。
    ///
    /// 曾经的 bug：`sync_params` 把控制器信号 `caret_pair` **回写**进核
    /// （`set_pair` / `clear_pair`），而 `edit` 在 `write_text` 之后才 `publish_sel`。
    /// `write_text` 触发同步 flush、刷新布局，期间 `display_text → sync_params` 用
    /// **还是旧值**的 `caret_pair` 把核里刚被编辑动作算好的新光标覆盖回旧值，
    /// 于是插入 `\n` 后光标停在上一行、上下键因光标落在空矩形退化成乱跳。
    /// 修复后光标是单向从核经 `publish_sel` 流向 `caret_pair`，不再回灌，换行即前进。
    #[test]
    fn enter_advances_caret_to_new_line() {
        let node = multiline_node("hello");
        focus::request_focus(&node);
        let h = focus::active_editor().expect("聚焦后应取到编辑器句柄");
        let caret = || h.cursor_sel_pair().map_or(0, |p| p.focus.offset);
        let key = |code: &str| KeyEvent2 {
            key: String::new(),
            code: code.to_string(),
            ctrl: false,
            shift: false,
            alt: false,
            meta: false,
        };

        h.select_range(5, 5); // 光标在 "hello" 末尾
        assert_eq!(caret(), 5);

        // 多行裸回车 = 换行：光标应前进到换行符之后（第 6 个字符 = 新行起点）。
        assert!(node.handle_key(&key(KeyCode2::ENTER)));
        assert_eq!(caret(), 6, "换行后光标必须前进到新行，不能停在上一行");

        // 再按一次回车：连续换行光标继续前进（中间空行也是落脚点）。
        assert!(node.handle_key(&key(KeyCode2::ENTER)));
        assert_eq!(caret(), 7, "连续换行光标应继续前进");

        // 换行后上下键应基于正确的新行光标工作，而非退化成跳到文首/文末。
        assert!(node.handle_key(&key(KeyCode2::UP)));
        assert_eq!(caret(), 6, "上键回到上一行行首（preferredX 列），不应漂走");
    }

    /// 回归：换行后光标**矩形**必须落到下一行（top 从 ≈0 抬升到行高）。
    ///
    /// 之前 `draw` 内联了一份 `rects_for_range` 副本、漏掉末尾空行兜底，于是
    /// 逻辑光标虽前进到新行（offset 正确），视觉矩形却被画在上一行末尾。这条测试
    /// 直接断言 `caret_rect()` 的 `top` 在换行后明显下移，抓住"画在上一行"的视觉 bug。
    #[test]
    fn newline_caret_rect_moves_to_next_line() {
        let node = multiline_node("abc");
        focus::request_focus(&node);
        let h = focus::active_editor().expect("聚焦后应取到编辑器句柄");
        let caret = || h.cursor_sel_pair().map_or(0, |p| p.focus.offset);
        let rect_top = || h.caret_rect().into_iter().next().map_or(-1.0, |r| r.top);
        let key = |code: &str| KeyEvent2 {
            key: String::new(),
            code: code.to_string(),
            ctrl: false,
            shift: false,
            alt: false,
            meta: false,
        };

        h.select_range(3, 3); // 光标在 "abc" 末尾（第 0 行）
        assert_eq!(caret(), 3);
        assert!(
            rect_top() < 1.0,
            "换行前光标矩形应在第 0 行（top ≈ 0），实际 top = {}",
            rect_top()
        );

        // 回车换行：逻辑光标前进到新行起点。
        assert!(node.handle_key(&key(KeyCode2::ENTER)));
        assert_eq!(caret(), 4, "换行后逻辑光标前进到新行起点");
        let top_after = rect_top();
        assert!(
            top_after > 1.0,
            "换行后光标矩形必须落到下一行（top 抬升到行高），实际 top = {}",
            top_after
        );
    }

    /// 回归：末行极短（单字符）时，**文末**上键不能甩到行首。
    ///
    /// 文本 `"ab\nc"`：行 0 = `ab`、行 1 = `c`（极短）。`caret_metrics` 文末回退取
    /// 前一字符矩形时，插入点必须是它的**右缘**（`prev.right`）——若误用左缘
    /// （`prev.left`），末行极短时左缘退化为 0，上键被甩到上一行行首（offset 0）。
    ///
    /// 同时锁定 TS 规范行为：光标在末字**之前**（即行 1 的列 0）时，视觉列本来就是 0
    /// （TS `caretMetrics` 取 `at.left` = 行首），上键落上一行列 0 是正确的，不是 bug。
    #[test]
    fn up_from_short_last_line_keeps_column() {
        let node = multiline_node("ab\nc");
        focus::request_focus(&node);
        let h = focus::active_editor().expect("聚焦后应取到编辑器句柄");
        let caret = || h.cursor_sel_pair().map_or(0, |p| p.focus.offset);
        let key = |code: &str| KeyEvent2 {
            key: String::new(),
            code: code.to_string(),
            ctrl: false,
            shift: false,
            alt: false,
            meta: false,
        };

        // 文末（最后一个字符之后）：插入点 = 'c' 右缘（非零），上键不能落行首。
        h.select_range(4, 4);
        assert_eq!(caret(), 4);
        assert!(node.handle_key(&key(KeyCode2::UP)));
        let after = caret();
        assert!(
            after > 0,
            "文末上键不能甩到行首（offset 0），实际 offset = {after}"
        );
        assert!(
            after <= 2,
            "文末上键应落在上一行（行 0 共 2 字符），实际 offset = {after}"
        );

        // 末字之前（行 1 列 0）：TS 规范行为——上键落上一行列 0。
        h.select_range(3, 3);
        assert_eq!(caret(), 3);
        assert!(node.handle_key(&key(KeyCode2::UP)));
        assert_eq!(
            caret(),
            0,
            "光标在行首（列 0）时上键应落上一行列 0（TS 规范行为）"
        );
    }

    /// 回归（IME 后 preferredX 重置）：TS 在 `commitComposition` / `endComposition`
    /// 里都重置 `preferredX = NaN`（editor.ts:551/562）。Rust 对应重置在
    /// `EditCore` 的组合 / 提交路径内部完成——此测试锁住"IME 提交后上键必须用
    /// 当前光标插入点，不得沿用历史 preferredX"这一行为。
    #[test]
    fn ime_commit_resets_preferred_x() {
        let node = multiline_node("abcdefgh\nxyz");
        focus::request_focus(&node);
        let h = focus::active_editor().unwrap();
        let caret = || h.cursor_sel_pair().map_or(0, |p| p.focus.offset);
        let key = |code: &str| KeyEvent2 {
            key: String::new(),
            code: code.to_string(),
            ctrl: false,
            shift: false,
            alt: false,
            meta: false,
        };

        // 光标在行 0 行首（x=0），按一次下键 → preferred_x 被记成 Some(0.0)。
        h.select_range(0, 0);
        assert!(node.handle_key(&key(KeyCode2::DOWN)));
        assert_eq!(caret(), 9, "从行首下键应落在下一行（行 1 = 'xyz'）行首");

        // IME 提交打字（真实中文输入路径）：TS 在 commit 时重置 preferredX。
        h.commit_composing_text("测试");

        // 此时上键必须用**当前光标**的插入点，而不是残留的 0。
        assert!(node.handle_key(&key(KeyCode2::UP)));
        let after = caret();
        assert!(
            after > 0,
            "IME 提交后上键不能沿用残留 preferredX=0 落到行首，实际 offset = {after}"
        );
        assert!(after <= 8, "上键应落在上一行内，实际 offset = {after}");
    }
}
