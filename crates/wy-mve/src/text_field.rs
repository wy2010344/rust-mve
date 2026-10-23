//! `TextField`：单行文本输入组件（复刻 Kotlin `EditableTextNode` + `TextField`）。
//!
//! 薄壳设计：编辑内核（光标 / 选区 / IME / 导航 / 删除 / 几何）全部由
//! [`wy_text::editable::EditableParagraph`]（Parley `PlainEditor`）承担，
//! 本组件只负责：
//! - **绘制**：背景 / 边框（聚焦高亮）/ 文本 / 占位符 / 选区高亮 / 光标 / IME 下划线
//! - **事件转发**：点击定位（`move_to_point`）、键盘导航与编辑、IME 组合态
//! - **值同步**：编辑后写回外部 `on_change`；外部 `value` 变化时重灌编辑器
//!
//! 声明式构造：内容由外部信号/闭包驱动。绘制在组件**本地坐标**。

use std::cell::RefCell;
use std::rc::Rc;

use wy_render::{Color, Point, Rect, Scene};
use wy_signal::{GetValue, SetValue, Signal};
use wy_text::EditableParagraph;

use crate::node::{ImeEvent, Key, KeyEvent, Node, PointerEvent};

/// 创建文本输入组件（默认配置）。
///
/// ```ignore
/// let text = Signal::new(String::new());
/// cx.child(text_field(
///     move || text.get(),
///     move |t| text.set(t),
/// ));
/// ```
pub fn text_field(
    value: impl Fn() -> String + 'static,
    on_change: impl Fn(String) + 'static,
) -> Node {
    text_field_opts(value, on_change, TextFieldOpts::default())
}

/// 文本输入配置。
#[derive(Clone)]
pub struct TextFieldOpts {
    /// 输入框宽度。
    pub width: f32,
    /// 输入框高度。
    pub height: f32,
    /// 字号。
    pub font_size: f32,
    /// 占位符。
    pub placeholder: String,
    /// 文本颜色。
    pub text_color: Color,
    /// 占位符颜色。
    pub placeholder_color: Color,
    /// 背景色。
    pub bg_color: Color,
    /// 常规边框色。
    pub border_color: Color,
    /// 聚焦边框色。
    pub focus_border_color: Color,
    /// 密码模式（掩码显示）。
    pub password: bool,
    /// 动态颜色覆盖：非空时在**绘制期**求值，优先于上方的静态颜色。
    ///
    /// 用于主题切换等场景——颜色随信号变化时，绘制期重新读取即触发重绘，
    /// 无需重建节点树（静态字段只在构建期读取一次）。
    pub colors_fn: Option<Rc<TextFieldColors>>,
}

/// 绘制期求值的动态颜色集合（主题等）。各字段在每帧绘制时调用。
#[derive(Clone)]
pub struct TextFieldColors {
    /// 文本颜色。
    pub text_color: Rc<dyn Fn() -> Color>,
    /// 占位符颜色。
    pub placeholder_color: Rc<dyn Fn() -> Color>,
    /// 背景色。
    pub bg_color: Rc<dyn Fn() -> Color>,
    /// 常规边框色。
    pub border_color: Rc<dyn Fn() -> Color>,
    /// 聚焦边框色。
    pub focus_border_color: Rc<dyn Fn() -> Color>,
}

impl Default for TextFieldOpts {
    fn default() -> Self {
        Self {
            width: 240.0,
            height: 34.0,
            font_size: 14.0,
            placeholder: String::new(),
            text_color: Color::BLACK,
            placeholder_color: Color::rgba(160, 160, 160, 255),
            bg_color: Color::WHITE,
            border_color: Color::rgba(180, 180, 180, 255),
            focus_border_color: Color::rgba(0, 120, 212, 255),
            password: false,
            colors_fn: None,
        }
    }
}

/// 组件共享运行时状态。
struct SharedState {
    /// 编辑内核（真源）。
    editable: EditableParagraph,
    /// 是否聚焦（信号承载：绘制依赖它，点击聚焦才会触发重绘）。
    focused: Signal<bool>,
    /// 状态修订号：Parley 内核（光标/选区/组合态）不是信号，本组件在每次
    /// 事件处理后递增它，供绘制 memo 感知内核状态变化（否则复用缓存 Scene）。
    state_rev: Signal<u64>,
    /// 上次已同步的外部文本（防回环与重复重灌）。
    last_synced: String,
    /// 多击计数（400ms / 5px 内同位置连击，Kotlin ClickTracker）。
    clicks: crate::text_util::ClickTracker,
    /// 双击拖选锚词（display 字节区间）：双击建立，on_move 词级扩展，on_up 清空。
    word_anchor: Option<(usize, usize)>,
    /// 撤销栈（parley PlainEditor 无 undo，快照式自建，深度对齐 EditCore=100）。
    undo_stack: Vec<Snapshot>,
    /// 重做栈（新编辑时清空）。
    redo_stack: Vec<Snapshot>,
}

/// 编辑快照（undo/redo 栈条目）。
#[derive(Clone)]
struct Snapshot {
    text: String,
    anchor: usize,
    cursor: usize,
}

impl Snapshot {
    fn capture(s: &SharedState) -> Self {
        Self {
            text: s.editable.text(),
            anchor: s.editable.anchor(),
            cursor: s.editable.cursor(),
        }
    }

    fn apply(&self, s: &mut SharedState) {
        s.editable.replace_with(&self.text);
        if self.anchor == self.cursor {
            s.editable
                .with_driver(|mut drv| drv.move_to_byte(self.cursor));
        } else {
            s.editable.select_range(self.anchor, self.cursor);
        }
    }
}

/// undo/redo 栈深度上限（对齐 Kotlin `maxHistorySize = 100`）。
const MAX_HISTORY: usize = 100;

impl SharedState {
    /// 标记内核状态已变化，使绘制 memo 失效并重录。
    fn mark_dirty(&mut self) {
        self.state_rev.set(self.state_rev.get().wrapping_add(1));
    }

    /// 文本变化后入栈（redo 清空；超深丢最旧）。
    fn record_history(&mut self, snap: Snapshot) {
        self.undo_stack.push(snap);
        if self.undo_stack.len() > MAX_HISTORY {
            self.undo_stack.remove(0);
        }
        self.redo_stack.clear();
    }

    /// 撤销：弹栈应用；IME 组合态禁用（防破坏组合会话）。
    fn undo(&mut self) {
        if self.editable.is_composing() {
            return;
        }
        if let Some(snap) = self.undo_stack.pop() {
            let cur = Snapshot::capture(self);
            self.redo_stack.push(cur);
            snap.apply(self);
        }
    }

    /// 重做。
    fn redo(&mut self) {
        if self.editable.is_composing() {
            return;
        }
        if let Some(snap) = self.redo_stack.pop() {
            let cur = Snapshot::capture(self);
            self.undo_stack.push(cur);
            snap.apply(self);
        }
    }
}

type Shared = Rc<RefCell<SharedState>>;

/// 文本绘制左内边距。
const PAD_X: f32 = 6.0;
/// 文本垂直居中（绘制为基线顶部位置）。
fn text_top(opts: &TextFieldOpts) -> f32 {
    (opts.height - opts.font_size) / 2.0
}

/// 掩码显示文本（真实文本 → 等长 `*`）。
fn mask(text: &str) -> String {
    "*".repeat(text.chars().count())
}

/// 创建带完整配置的文本输入组件。
pub fn text_field_opts(
    value: impl Fn() -> String + 'static,
    on_change: impl Fn(String) + 'static,
    opts: TextFieldOpts,
) -> Node {
    // 编辑器预灌当前外部文本（构建期一次）。
    let initial = value();
    let font_size = opts.font_size;
    let shared: Shared = Rc::new(RefCell::new(SharedState {
        editable: EditableParagraph::new(font_size),
        focused: Signal::new(false),
        state_rev: Signal::new(0),
        last_synced: initial.clone(),
        clicks: crate::text_util::ClickTracker::new(),
        word_anchor: None,
        undo_stack: Vec::new(),
        redo_stack: Vec::new(),
    }));
    {
        let mut s = shared.borrow_mut();
        s.editable.replace_with(&initial);
    }

    let value = Rc::new(value);
    let on_change = Rc::new(on_change);
    let opts = Rc::new(opts);

    // --- 外部文本 → 编辑器（draw 前同步；外部变化重灌，光标复位） ---
    let sync_fn = {
        let shared = Rc::clone(&shared);
        let value = Rc::clone(&value);
        Rc::new(move || {
            let mut s = shared.borrow_mut();
            let ext = value();
            if ext != s.last_synced {
                s.editable.replace_with(&ext);
                s.last_synced = ext;
                s.mark_dirty();
            }
        })
    };

    // --- 绘制 ---
    let draw_fn = {
        let shared = Rc::clone(&shared);
        let sync = Rc::clone(&sync_fn);
        let opts = Rc::clone(&opts);
        Rc::new(move |scene: &mut dyn std::any::Any| {
            let Some(scene) = scene.downcast_mut::<Scene>() else {
                return;
            };
            sync();
            let s = shared.borrow();
            // 追踪内核状态变化（Parley 光标/选区/组合态非信号，经 state_rev 桥接）
            let _state_rev = s.state_rev.get();
            // 动态颜色覆盖（主题等）：绘制期求值，优先于静态字段
            let (bg_color, border_color, focus_border_color, text_color, placeholder_color) =
                match &opts.colors_fn {
                    Some(c) => (
                        (c.bg_color)(),
                        (c.border_color)(),
                        (c.focus_border_color)(),
                        (c.text_color)(),
                        (c.placeholder_color)(),
                    ),
                    None => (
                        opts.bg_color,
                        opts.border_color,
                        opts.focus_border_color,
                        opts.text_color,
                        opts.placeholder_color,
                    ),
                };
            let border = if s.focused.get() {
                focus_border_color
            } else {
                border_color
            };
            // 实际尺寸：绘制期用引擎分配盒（被父容器 Stretch/grow 拉伸时跟随），
            // 未绘制过回落声明尺寸
            let b = crate::current_box();
            let (fw, fh) = (
                if b.width > 0.0 { b.width } else { opts.width },
                if b.height > 0.0 {
                    b.height
                } else {
                    opts.height
                },
            );
            scene.fill_rect(Rect::new(0.0, 0.0, fw, fh), bg_color);
            scene.stroke_round_rect(Rect::new(0.0, 0.0, fw, fh), 4.0, border, 1.0);

            let top = text_top(&opts);
            let text = s.editable.text();

            // 密码掩码：显示文本与几何都走掩码（同字素 count）。
            let (display, disp_len) = if opts.password {
                let m = mask(&text);
                let n = m.len();
                (m, n)
            } else {
                let n = text.len();
                (text, n)
            };

            // 文本或占位符
            let (shown, color) = if display.is_empty() && !opts.placeholder.is_empty() {
                (opts.placeholder.as_str(), placeholder_color)
            } else {
                (display.as_str(), text_color)
            };
            if !shown.is_empty() {
                scene.draw_text(Point::new(PAD_X, top), shown, opts.font_size, color);
            }

            // 选区高亮（字形行盒几何，对齐 Kotlin TIGHT rect）
            let anchor = s.editable.anchor().min(disp_len);
            let cursor = s.editable.cursor().min(disp_len);
            let (lo, hi) = (anchor.min(cursor), anchor.max(cursor));
            if lo < hi {
                let sb = wy_render::text_measure::caret_box(&display, opts.font_size, lo);
                let eb = wy_render::text_measure::caret_box(&display, opts.font_size, hi);
                scene.fill_rect(
                    Rect::new(PAD_X + sb.x, top + sb.top, eb.x - sb.x, sb.height),
                    Color::rgba(100, 100, 200, 60),
                );
            }

            // 光标（聚焦 + 无选区 + 未组合时显示，对齐 Kotlin !hasSel）
            if s.focused.get() && lo == hi && !s.editable.is_composing() {
                let cb = wy_render::text_measure::caret_box(&display, opts.font_size, cursor);
                scene.fill_rect(
                    Rect::new(PAD_X + cb.x, top + cb.top, 2.0, cb.height),
                    text_color,
                );
            }

            // IME 组合带：背景 + 底部 2px 下划线（对齐 Kotlin drawComposing）
            if let Some(range) = s.editable.composing_range() {
                let sb = wy_render::text_measure::caret_box(
                    &display,
                    opts.font_size,
                    range.start.min(disp_len),
                );
                let eb = wy_render::text_measure::caret_box(
                    &display,
                    opts.font_size,
                    range.end.min(disp_len),
                );
                let ax = PAD_X + sb.x;
                let bx = PAD_X + eb.x;
                let by = top + sb.top;
                let bh = sb.height;
                scene.fill_rect(
                    Rect::new(ax, by, (bx - ax).max(1.0), bh),
                    Color::rgba(200, 200, 255, 50),
                );
                scene.fill_rect(
                    Rect::new(ax, (by + bh - 2.0).max(by), (bx - ax).max(1.0), 2.0),
                    Color::rgba(0, 0, 0, 140),
                );
            }
        })
    };

    let hit_test_fn = {
        // 边界由引擎的 `in_layout_box`（实际分配盒）判定，这里恒真——
        // opts.width 是构建期声明值，被 Stretch 拉伸后不再等于实际宽度。
        Rc::new(move |_x: f32, _y: f32| true)
    };

    // --- 点击：聚焦 + 多击手势（单击定位 / 双击选词+拖词 / 三击选段） ---
    let on_down_fn = {
        let shared = Rc::clone(&shared);
        let opts = Rc::clone(&opts);
        Rc::new(move |event: &mut PointerEvent| {
            let mut s = shared.borrow_mut();
            s.focused.set(true);
            let count = s.clicks.record(event.x, event.y);
            match count {
                // 三击选段：单行字段段 = 全部文本（Kotlin paragraphRangeAt 段不含 \n）
                3.. => {
                    s.word_anchor = None;
                    let len = s.editable.text().len();
                    if len > 0 {
                        s.editable.select_range(0, len);
                    }
                }
                // 双击选词，锚定后可拖拽词级扩展
                2 => {
                    s.editable
                        .move_to_point(event.x - PAD_X, text_top(&opts), false);
                    let hit = s.editable.cursor();
                    let text = s.editable.text();
                    let (bs, be) =
                        crate::text_util::word_range_at(&text, hit).unwrap_or((0, text.len()));
                    s.word_anchor = Some((bs, be));
                    s.editable.select_range(bs, be);
                }
                // 单击定位（shift 扩选）
                _ => {
                    s.word_anchor = None;
                    s.editable
                        .move_to_point(event.x - PAD_X, text_top(&opts), event.shift);
                }
            }
            s.mark_dirty();
        })
    };

    // --- 拖选：双击锚词级扩展，否则光标扩展（选区跟随指针） ---
    let on_move_fn = {
        let shared = Rc::clone(&shared);
        let opts = Rc::clone(&opts);
        Rc::new(move |event: &mut PointerEvent| {
            let mut s = shared.borrow_mut();
            let extend = s.word_anchor.is_none();
            s.editable
                .move_to_point(event.x - PAD_X, text_top(&opts), extend);
            if let Some((a0, a1)) = s.word_anchor {
                let hit = s.editable.cursor();
                let text = s.editable.text();
                let (b0, b1) = crate::text_util::expand_word_selection(a0, a1, hit, |b| {
                    crate::text_util::word_range_at(&text, b)
                });
                s.editable.select_range(b0, b1);
            }
            s.mark_dirty();
        })
    };

    // --- 释放：结束双击拖词会话 ---
    let on_up_fn = {
        let shared = Rc::clone(&shared);
        Rc::new(move |_event: &mut PointerEvent| {
            let mut s = shared.borrow_mut();
            if s.word_anchor.take().is_some() {
                s.mark_dirty();
            }
        })
    };

    // --- 键盘导航/编辑 ---
    let key_fn = {
        let shared = Rc::clone(&shared);
        let value = Rc::clone(&value);
        let on_change = Rc::clone(&on_change);
        let opts = Rc::clone(&opts);
        Rc::new(move |event: &mut KeyEvent| -> bool {
            let mut s = shared.borrow_mut();
            let before = Snapshot::capture(&s);
            let handled = editable_handle_key(&mut s, &opts, event);
            if handled {
                // undo/redo 键本身不入栈；文本变化才记录
                let is_history = matches!(event.key, Key::Char(c) if matches!(c.to_ascii_lowercase(), 'z' | 'y'))
                    && (event.ctrl || event.meta);
                if !is_history && s.editable.text() != before.text {
                    s.record_history(before);
                }
                s.mark_dirty();
                push_change(&mut s, &*value, &*on_change);
            }
            handled
        })
    };

    // --- IME 组合态 ---
    let ime_fn = {
        let shared = Rc::clone(&shared);
        let value = Rc::clone(&value);
        let on_change = Rc::clone(&on_change);
        Rc::new(move |event: &mut ImeEvent| -> bool {
            let mut s = shared.borrow_mut();
            let before = Snapshot::capture(&s);
            let handled = editable_handle_ime(&mut s, event);
            if handled {
                // 仅 IME Commit 改正文才入栈（Preedit 组合带不算独立编辑）
                if matches!(event, ImeEvent::Commit(_)) && s.editable.text() != before.text {
                    s.record_history(before);
                }
                s.mark_dirty();
                push_change(&mut s, &*value, &*on_change);
            }
            handled
        })
    };

    // --- IME 候选窗定位：返回光标盒（节点局部坐标） ---
    let ime_cursor_fn = {
        let shared = Rc::clone(&shared);
        let opts = Rc::clone(&opts);
        Rc::new(move || {
            let s = shared.borrow();
            let text = s.editable.text();
            let (display, disp_len) = if opts.password {
                let m = mask(&text);
                let n = m.len();
                (m, n)
            } else {
                let n = text.len();
                (text, n)
            };
            let cursor = s.editable.cursor().min(disp_len);
            let cb = wy_render::text_measure::caret_box(&display, opts.font_size, cursor);
            (PAD_X + cb.x, text_top(&opts) + cb.top, 2.0, cb.height)
        })
    };

    // --- 聚焦视觉：引擎焦点变化 → 驱动内部 `focused` 信号（Tab 聚焦也高亮） ---
    // `Signal` 是 Copy，先取出句柄再 set，避免持有 RefCell 借用时触发重绘。
    let on_focus_fn = {
        let shared = Rc::clone(&shared);
        Rc::new(move || {
            let focused = shared.borrow().focused;
            focused.set(true);
        })
    };
    let on_blur_fn = {
        let shared = Rc::clone(&shared);
        Rc::new(move || {
            let focused = shared.borrow().focused;
            focused.set(false);
        })
    };

    Node {
        draw_fn,
        hit_test_fn,
        on_down_fn: Some(on_down_fn),
        on_move_fn: Some(on_move_fn),
        on_up_fn: Some(on_up_fn),
        key_fn: Some(key_fn),
        ime_fn: Some(ime_fn),
        ime_cursor_fn: Some(ime_cursor_fn),
        on_focus_fn: Some(on_focus_fn),
        on_blur_fn: Some(on_blur_fn),
        focusable: true,
        width: opts.width,
        height: opts.height,
        ..Node::default()
    }
}

/// 编辑后把快照写回外部（仅在文本变化时调用 `on_change`）。
fn push_change(s: &mut SharedState, value: &dyn Fn() -> String, on_change: &dyn Fn(String)) {
    let snap = s.editable.snapshot();
    s.last_synced = snap.text.clone();
    if snap.text != value() {
        on_change(snap.text);
    }
}

/// 键盘事件 → 编辑内核操作；返回是否消费。
///
/// 纯逻辑，可单测。`Ctrl+Z`（撤销）留待二期（Kotlin 用 undoManager）。
fn editable_handle_key(s: &mut SharedState, opts: &TextFieldOpts, event: &KeyEvent) -> bool {
    let ctrl_or_meta = event.ctrl || event.meta;
    let shift = event.shift;

    // Ctrl/Meta 组合
    if ctrl_or_meta {
        // 需要能引用 editable 的 driver
        return handle_ctrl_shortcut(s, event);
    }

    let _ = opts;
    match event.key {
        // 控制字符（含 `'\0'`，如未映射键/IME 消费键的兜底）不入正文。
        Key::Char(ch) if ch.is_control() => false,
        Key::Char(ch) => {
            // winit 在 Shift+字母时给大写字母，直接插入即可
            s.editable.with_driver(|mut drv| {
                drv.insert_or_replace_selection(&ch.to_string());
            });
            true
        }
        Key::Backspace => {
            if shift {
                s.editable.with_driver(|mut drv| drv.backdelete_word());
            } else {
                s.editable.with_driver(|mut drv| drv.backdelete());
            }
            true
        }
        Key::Delete => {
            if shift {
                s.editable.with_driver(|mut drv| drv.delete_word());
            } else {
                s.editable.with_driver(|mut drv| drv.delete());
            }
            true
        }
        Key::ArrowLeft => {
            if shift {
                s.editable.with_driver(|mut drv| drv.select_left());
            } else {
                s.editable.with_driver(|mut drv| drv.move_left());
            }
            true
        }
        Key::ArrowRight => {
            if shift {
                s.editable.with_driver(|mut drv| drv.select_right());
            } else {
                s.editable.with_driver(|mut drv| drv.move_right());
            }
            true
        }
        Key::Home => {
            if shift {
                s.editable.with_driver(|mut drv| drv.select_to_line_start());
            } else {
                s.editable.with_driver(|mut drv| drv.move_to_line_start());
            }
            true
        }
        Key::End => {
            if shift {
                s.editable.with_driver(|mut drv| drv.select_to_line_end());
            } else {
                s.editable.with_driver(|mut drv| drv.move_to_line_end());
            }
            true
        }
        // 单行字段无页概念：消费但不动光标（对齐 Kotlin 单行 getGlyphPosition
        // y 越界 clamp 回本行 = 无变化）
        Key::PageUp | Key::PageDown => true,
        // 未处理的控制键（Enter / Tab / Escape）交给上层
        _ => false,
    }
}

/// Ctrl 快捷键：A 全选 / C 复制 / X 剪切 / V 粘贴 / Home·End 文档首尾。
///
/// 对齐 Kotlin Renderer 全局拦截。单行字段：粘贴剥除换行；paste 空串不插，
/// 先终止组合态。
fn handle_ctrl_shortcut(s: &mut SharedState, event: &KeyEvent) -> bool {
    match event.key {
        // 单行字段 Ctrl+Home/End = 文档首尾（Kotlin moveDocStart/End）
        Key::Home => {
            s.editable.with_driver(|mut drv| drv.move_to_text_start());
            true
        }
        Key::End => {
            s.editable.with_driver(|mut drv| drv.move_to_text_end());
            true
        }
        Key::Char(c) => match c.to_ascii_lowercase() {
            'a' => {
                s.editable.select_all();
                true
            }
            'c' => {
                if let Some(sel) = s.editable.selected_text() {
                    crate::clipboard::clipboard_set(&sel);
                }
                true
            }
            'x' => {
                if let Some(sel) = s.editable.selected_text() {
                    crate::clipboard::clipboard_set(&sel);
                    s.editable.delete_selection();
                }
                true
            }
            'v' => {
                if let Some(text) = crate::clipboard::clipboard_get() {
                    // 单行字段粘贴剥除换行（对齐 EditCore::insert_text 的 single_line 过滤）
                    let text = text.replace(['\n', '\r'], "");
                    if !text.is_empty() {
                        s.editable.end_composing_if_any();
                        s.editable
                            .with_driver(|mut drv| drv.insert_or_replace_selection(&text));
                    }
                }
                true
            }
            // 撤销/重做（Kotlin: undo = ctrl&&!shift+'z'，redo = ctrl+'y' 或 ctrl&&shift+'z'）
            'z' if !event.shift => {
                s.undo();
                true
            }
            'y' | 'z' => {
                s.redo();
                true
            }
            _ => false,
        },
        _ => false,
    }
}

/// IME 组合事件 → 编辑内核。
fn editable_handle_ime(s: &mut SharedState, event: &ImeEvent) -> bool {
    match event {
        ImeEvent::Enabled => true,
        ImeEvent::Disabled => {
            s.editable.with_driver(|mut drv| drv.finish_compose());
            true
        }
        ImeEvent::Preedit { text, cursor } => {
            if text.is_empty() {
                s.editable.with_driver(|mut drv| drv.clear_compose());
            } else {
                // cursor 即组合带内区间 (start,end)，透传给 parley set_compose
                let cur = cursor.filter(|(_, e)| *e <= text.len());
                s.editable.with_driver(|mut drv| drv.set_compose(text, cur));
            }
            true
        }
        ImeEvent::Commit(text) => {
            // 各平台提交序列不同：
            // - 组合态仍在（Preedit 后直接 Commit）：结束组合即可（缓冲已含组合文本）；
            //   提交文本与组合文本不一致时，以平台提交文本为准（丢弃组合带重插）。
            // - 组合已被空 Preedit 清除（macOS/Windows 常见 `Preedit("") → Commit(text)`）：
            //   组合缓冲已空，必须把提交文本显式写入正文，否则提交丢失。
            if s.editable.is_composing() {
                if s.editable.composing_text() == *text {
                    s.editable.with_driver(|mut drv| drv.finish_compose());
                } else {
                    s.editable.with_driver(|mut drv| drv.clear_compose());
                    if !text.is_empty() {
                        s.editable
                            .with_driver(|mut drv| drv.insert_or_replace_selection(text.as_str()));
                    }
                }
            } else if !text.is_empty() {
                s.editable
                    .with_driver(|mut drv| drv.insert_or_replace_selection(text.as_str()));
            }
            true
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    // --- 纯逻辑：editable_handle_key / editable_handle_ime ---

    fn shared(text: &str, font_size: f32) -> SharedState {
        let mut e = EditableParagraph::new(font_size);
        e.replace_with(text);
        SharedState {
            editable: e,
            focused: Signal::new(true),
            state_rev: Signal::new(0),
            last_synced: text.to_string(),
            clicks: crate::text_util::ClickTracker::new(),
            word_anchor: None,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
        }
    }

    fn key(k: Key, ctrl: bool, shift: bool) -> KeyEvent {
        KeyEvent {
            key: k,
            ctrl,
            shift,
            alt: false,
            meta: false,
        }
    }

    /// 文本框暴露焦点回调：引擎焦点变化据此驱动 `focused`（Tab 聚焦也高亮）。
    #[test]
    fn text_field_wires_focus_callbacks() {
        let v = Signal::new(String::new());
        let value = v;
        let node = text_field(move || value.get(), |_| {});
        assert!(node.on_focus_fn.is_some(), "应有 on_focus_fn");
        assert!(node.on_blur_fn.is_some(), "应有 on_blur_fn");
        // 直接触发不应 panic（内部 set 信号）
        node.run_focus();
        node.run_blur();
    }

    #[test]
    fn char_insert_in_the_middle() {
        let mut s = shared("hi", 14.0);
        s.editable.with_driver(|mut drv| drv.move_to_byte(1));
        let ev = key(Key::Char('x'), false, false);
        assert!(editable_handle_key(&mut s, &default_opts(), &ev));
        assert_eq!(s.editable.text(), "hxi");
        assert_eq!(s.editable.cursor(), 2);
    }

    #[test]
    fn shift_letter_inserts_uppercase() {
        // winit 在 Shift+字母时给大写字母；此前 `!shift` 守卫把它丢弃
        let mut s = shared("", 14.0);
        let ev = key(Key::Char('A'), false, true);
        assert!(editable_handle_key(&mut s, &default_opts(), &ev));
        assert_eq!(s.editable.text(), "A");
    }

    /// 控制字符（如 `'\0'`，来自未映射键 / Windows IME 消费键的兜底翻译）
    /// 不得写入正文——否则会渲染成方块。
    #[test]
    fn control_char_is_not_inserted() {
        let mut s = shared("ab", 14.0);
        s.editable.with_driver(|mut drv| drv.move_to_byte(2));
        let ev = key(Key::Char('\0'), false, false);
        assert!(!editable_handle_key(&mut s, &default_opts(), &ev));
        assert_eq!(s.editable.text(), "ab");
        let ev = key(Key::Char('\t'), false, false);
        assert!(!editable_handle_key(&mut s, &default_opts(), &ev));
        assert_eq!(s.editable.text(), "ab");
    }

    #[test]
    fn backspace_deletes_grapheme() {
        let mut s = shared("a😀b", 14.0);
        // a(1) + 😀(4) = 5 字节处是 b 前
        s.editable.with_driver(|mut drv| drv.move_to_byte(5));
        let ev = key(Key::Backspace, false, false);
        assert!(editable_handle_key(&mut s, &default_opts(), &ev));
        assert_eq!(s.editable.text(), "ab");
    }

    #[test]
    fn left_arrow_moves_and_collapses() {
        let mut s = shared("hello", 14.0);
        s.editable
            .with_driver(|mut drv| drv.select_byte_range(2, 5));
        let ev = key(Key::ArrowLeft, false, false);
        assert!(editable_handle_key(&mut s, &default_opts(), &ev));
        // move_left 在选区非塌缩时塌缩到锚点
        assert!(s.editable.selection_range() == (2, 2));
        let ev = key(Key::ArrowLeft, false, false);
        assert!(editable_handle_key(&mut s, &default_opts(), &ev));
        assert_eq!(s.editable.cursor(), 1);
    }

    #[test]
    fn shift_arrow_extends_selection() {
        let mut s = shared("hello", 14.0);
        let ev = key(Key::ArrowRight, false, true);
        assert!(editable_handle_key(&mut s, &default_opts(), &ev));
        assert_eq!(s.editable.selection_range(), (0, 1));
    }

    #[test]
    fn home_end_navigation() {
        let mut s = shared("hello", 14.0);
        s.editable.with_driver(|mut drv| drv.move_to_byte(3));
        let ev = key(Key::Home, false, false);
        assert!(editable_handle_key(&mut s, &default_opts(), &ev));
        assert_eq!(s.editable.cursor(), 0);
        let ev = key(Key::End, false, false);
        assert!(editable_handle_key(&mut s, &default_opts(), &ev));
        assert_eq!(s.editable.cursor(), 5);
    }

    #[test]
    fn enter_tab_escape_not_consumed() {
        let mut s = shared("hi", 14.0);
        for k in [Key::Enter, Key::Tab, Key::Escape] {
            let ev = key(k, false, false);
            assert!(
                !editable_handle_key(&mut s, &default_opts(), &ev),
                "{k:?} 不应消费"
            );
        }
    }

    #[test]
    fn page_up_down_consumed_as_noop() {
        // 单行字段无页概念：消费但不动光标
        let mut s = shared("hi", 14.0);
        s.editable.with_driver(|mut drv| drv.move_to_byte(1));
        for k in [Key::PageUp, Key::PageDown] {
            let ev = key(k, false, false);
            assert!(
                editable_handle_key(&mut s, &default_opts(), &ev),
                "{k:?} 应消费"
            );
            assert_eq!(s.editable.cursor(), 1, "{k:?} 光标不动");
        }
    }

    #[test]
    fn ime_preedit_and_commit() {
        let mut s = shared("a", 14.0);
        s.editable.with_driver(|mut drv| drv.move_to_byte(1));
        let ev = ImeEvent::Preedit {
            text: "中".into(),
            cursor: Some((3, 3)),
        };
        assert!(editable_handle_ime(&mut s, &ev));
        assert!(s.editable.is_composing());
        assert_eq!(s.editable.raw_text(), "a中"); // 组合文本含在 raw_text
        let ev = ImeEvent::Commit("中".into());
        assert!(editable_handle_ime(&mut s, &ev));
        assert!(!s.editable.is_composing());
        assert_eq!(s.editable.text(), "a中");
        // a(1) + 中(3) = 4 字节，光标在组合带末尾
        assert_eq!(s.editable.cursor(), 4);
    }

    /// macOS/Windows 常见提交序列：`Preedit(文本) → Preedit("") → Commit(文本)`。
    /// 空 preedit 会清除组合带，提交文本必须显式写入正文（否则提交丢失）。
    #[test]
    fn ime_commit_after_empty_preedit_inserts_text() {
        let mut s = shared("a", 14.0);
        s.editable.with_driver(|mut drv| drv.move_to_byte(1));
        let ev = ImeEvent::Preedit {
            text: "中".into(),
            cursor: Some((3, 3)),
        };
        assert!(editable_handle_ime(&mut s, &ev));
        let ev = ImeEvent::Preedit {
            text: String::new(),
            cursor: None,
        };
        assert!(editable_handle_ime(&mut s, &ev));
        assert_eq!(s.editable.text(), "a", "空 preedit 清除组合带");
        let ev = ImeEvent::Commit("中".into());
        assert!(editable_handle_ime(&mut s, &ev));
        assert_eq!(s.editable.text(), "a中", "提交文本写入正文");
        // a(1) + 中(3) = 4 字节
        assert_eq!(s.editable.cursor(), 4);
    }

    /// 组合态仍在但提交文本与组合文本不一致：丢弃组合带，以提交文本为准。
    #[test]
    fn ime_commit_replaces_mismatched_preedit() {
        let mut s = shared("", 14.0);
        let ev = ImeEvent::Preedit {
            text: "nihao".into(),
            cursor: None,
        };
        assert!(editable_handle_ime(&mut s, &ev));
        assert_eq!(s.editable.raw_text(), "nihao");
        let ev = ImeEvent::Commit("你好".into());
        assert!(editable_handle_ime(&mut s, &ev));
        assert_eq!(s.editable.text(), "你好", "提交文本替换组合带");
        assert!(!s.editable.is_composing());
    }

    #[test]
    fn ime_clear_removes_preedit() {
        let mut s = shared("a", 14.0);
        s.editable.with_driver(|mut drv| drv.move_to_byte(1));
        let ev = ImeEvent::Preedit {
            text: "中".into(),
            cursor: None,
        };
        assert!(editable_handle_ime(&mut s, &ev));
        assert!(s.editable.is_composing());
        let ev = ImeEvent::Preedit {
            text: String::new(),
            cursor: None,
        };
        assert!(editable_handle_ime(&mut s, &ev));
        assert!(!s.editable.is_composing());
        assert_eq!(s.editable.text(), "a");
    }

    #[test]
    fn push_change_writes_only_on_diff() {
        let cell = Rc::new(RefCell::new(String::from("hi")));
        let mut s = shared("hi", 14.0);
        let cell_for_value = Rc::clone(&cell);
        let value = move || cell_for_value.borrow().clone();
        let count = Rc::new(std::cell::Cell::new(0));
        let count_for_cb = Rc::clone(&count);
        let cell_for_cb = Rc::clone(&cell);
        let on_change: Rc<dyn Fn(String)> = Rc::new(move |t: String| {
            count_for_cb.set(count_for_cb.get() + 1);
            *cell_for_cb.borrow_mut() = t;
        });
        // 文本一致：不触发 on_change
        push_change(&mut s, &value, &*on_change);
        assert_eq!(count.get(), 0);
        // 编辑后触发
        s.editable.with_driver(|mut drv| drv.move_to_byte(2));
        s.editable
            .with_driver(|mut drv| drv.insert_or_replace_selection("!"));
        push_change(&mut s, &value, &*on_change);
        assert_eq!(count.get(), 1);
        assert_eq!(cell.borrow().as_str(), "hi!");
    }

    #[test]
    fn mask_preserves_char_count() {
        assert_eq!(mask("héllo"), "*****");
        assert_eq!(mask(""), "");
        assert_eq!(mask("a😀b"), "***");
    }

    #[test]
    fn ctrl_z_undo_inserts() {
        let mut s = shared("", 14.0);
        let ev = key(Key::Char('a'), false, false);
        assert!(editable_handle_key(&mut s, &default_opts(), &ev));
        assert_eq!(s.editable.text(), "a");
        // 模拟 key_fn 的历史记录（纯函数层不入栈，组件层负责）
        s.record_history(Snapshot {
            text: String::new(),
            anchor: 0,
            cursor: 0,
        });
        let ev = key(Key::Char('z'), true, false); // Ctrl+Z
        assert!(editable_handle_key(&mut s, &default_opts(), &ev));
        assert_eq!(s.editable.text(), "", "Ctrl+Z 撤销插入");
        assert_eq!(s.editable.cursor(), 0);
    }

    #[test]
    fn ctrl_y_redo_after_undo() {
        let mut s = shared("", 14.0);
        s.record_history(Snapshot {
            text: String::new(),
            anchor: 0,
            cursor: 0,
        });
        s.editable.with_driver(|mut drv| drv.move_to_byte(0));
        s.editable
            .with_driver(|mut drv| drv.insert_or_replace_selection("x"));
        let ev = key(Key::Char('z'), true, false);
        assert!(editable_handle_key(&mut s, &default_opts(), &ev));
        assert_eq!(s.editable.text(), "");
        let ev = key(Key::Char('y'), true, false); // Ctrl+Y
        assert!(editable_handle_key(&mut s, &default_opts(), &ev));
        assert_eq!(s.editable.text(), "x", "Ctrl+Y 重做");
    }

    #[test]
    fn ctrl_shift_z_redo() {
        let mut s = shared("", 14.0);
        s.record_history(Snapshot {
            text: String::new(),
            anchor: 0,
            cursor: 0,
        });
        s.editable.with_driver(|mut drv| drv.move_to_byte(0));
        s.editable
            .with_driver(|mut drv| drv.insert_or_replace_selection("x"));
        let ev = key(Key::Char('z'), true, false);
        assert!(editable_handle_key(&mut s, &default_opts(), &ev));
        assert_eq!(s.editable.text(), "");
        let ev = key(Key::Char('z'), true, true); // Ctrl+Shift+Z
        assert!(editable_handle_key(&mut s, &default_opts(), &ev));
        assert_eq!(s.editable.text(), "x", "Ctrl+Shift+Z 重做");
    }

    #[test]
    fn undo_redo_restores_selection() {
        let mut s = shared("abc", 14.0);
        s.editable
            .with_driver(|mut drv| drv.select_byte_range(0, 3));
        s.record_history(Snapshot {
            text: "abc".into(),
            anchor: 0,
            cursor: 3,
        });
        s.editable.delete_selection();
        s.editable
            .with_driver(|mut drv| drv.insert_or_replace_selection("x"));
        assert_eq!(s.editable.text(), "x");
        let ev = key(Key::Char('z'), true, false);
        assert!(editable_handle_key(&mut s, &default_opts(), &ev));
        assert_eq!(s.editable.text(), "abc");
        assert_eq!(s.editable.selection_range(), (0, 3), "undo 恢复选区");
    }

    #[test]
    fn undo_composition_is_noop() {
        let mut s = shared("a", 14.0);
        s.record_history(Snapshot {
            text: "a".into(),
            anchor: 1,
            cursor: 1,
        });
        s.editable.with_driver(|mut drv| drv.move_to_byte(1));
        // 组合态中禁用 undo（防破坏 IME 会话）
        let ev = ImeEvent::Preedit {
            text: "中".into(),
            cursor: Some((3, 3)),
        };
        assert!(editable_handle_ime(&mut s, &ev));
        assert!(s.editable.is_composing());
        let ev = key(Key::Char('z'), true, false);
        assert!(editable_handle_key(&mut s, &default_opts(), &ev));
        // 组合态 undo 不生效：已提交文本不变，组合态仍在
        assert!(s.editable.is_composing(), "组合态仍活跃");
        assert_eq!(s.editable.text(), "a", "已提交文本不变");
    }

    fn default_opts() -> TextFieldOpts {
        TextFieldOpts::default()
    }

    // --- 命中测试边界（对齐 Kotlin LayoutNode.acceptHit） ---

    #[test]
    fn hit_test_is_shape_only_bounds_by_engine() {
        // 命中形状恒真：**边界由引擎的 `in_layout_box`（实际分配盒）判定**——
        // 输入框会被父容器 Stretch/grow 拉伸，构建期声明的 `opts.width` 不再
        // 等于实际宽度，组件自判边界会"看得见点不中"或"越界抢点击"。
        let n = text_field(String::new, |_| {});
        assert!(n.run_hit_test(10.0, 10.0), "形状恒真，边界交给引擎");
        assert!(n.run_hit_test(9999.0, 9999.0), "越界与否由引擎布局盒判定");
        assert!(n.run_hit_test(-1.0, -1.0), "同上");
    }
}
