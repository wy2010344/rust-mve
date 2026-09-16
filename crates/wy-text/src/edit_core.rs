//! 核心编辑态：复刻 Kotlin `EditableTextNode` 的纯逻辑编辑内核。
//!
//! 设计要点：
//! - 所有索引使用**字符索引**（Unicode 标量序号），与 [`TextBuffer`] 一致；
//!   grapheme/word 边界内部做 `char ↔ byte` 桥接，对外接口统一为字符索引。
//! - 文本写入全部经由 [`TextBuffer::write_text`]，样式段自动对齐。
//! - 光标/选区由 `anchor` / `focus` 双端点表示（`None` = 未定位）。
//! - 显示层（占位、掩码圆点、tab 展开与索引映射）在此处提供，
//!   由组件层在绘制时调用，保证纯逻辑可测。

use crate::editing::TextBuffer;
use crate::TextSpan;
use unicode_segmentation::{GraphemeCursor, UnicodeSegmentation};

// ---------------------------------------------------------------------------
// 字符索引 ↔ 字节索引
// ---------------------------------------------------------------------------

fn char_to_byte(text: &str, ci: usize) -> usize {
    text.chars().take(ci).map(|c| c.len_utf8()).sum()
}

fn byte_to_char(text: &str, bi: usize) -> usize {
    let clamped = bi.min(text.len());
    text[..clamped].chars().count()
}

// ---------------------------------------------------------------------------
// 字素簇边界（字符索引域）
// ---------------------------------------------------------------------------

/// `ci` 所在簇的下一簇起点（结束偏移）；越界钳制到 text.length。
fn grapheme_next(text: &str, ci: usize) -> usize {
    let n = text.len();
    if n == 0 || ci >= text.chars().count() {
        return text.chars().count();
    }
    let bi = char_to_byte(text, ci);
    if bi >= n {
        return text.chars().count();
    }
    let mut cursor = GraphemeCursor::new(bi, n, true);
    match cursor.next_boundary(text, 0) {
        Ok(Some(b)) => byte_to_char(text, b),
        _ => text.chars().count(),
    }
}

/// `ci` 所在簇的前一簇起点；`ci == 0` 时返回 0。
fn grapheme_prev(text: &str, ci: usize) -> usize {
    let bi = char_to_byte(text, ci);
    if bi == 0 {
        return 0;
    }
    let mut cursor = GraphemeCursor::new(bi, text.len(), true);
    match cursor.prev_boundary(text, 0) {
        Ok(Some(b)) => byte_to_char(text, b),
        _ => 0,
    }
}

/// 簇的数量。
fn grapheme_count(text: &str) -> usize {
    text.grapheme_indices(true).count()
}

// ---------------------------------------------------------------------------
// 简单词边界（复刻 Kotlin `Words`）
// ---------------------------------------------------------------------------

fn is_word_char(text: &str, byte_pos: usize) -> bool {
    let ch = text[byte_pos.min(text.len())..]
        .chars()
        .next()
        .unwrap_or('\0');
    ch.is_alphanumeric()
}

/// 前一个词边界（字符索引域，复刻 `Words.prevBoundary`）。
fn word_prev(text: &str, ci: usize) -> usize {
    let n_chars = text.chars().count();
    let mut i = ci.min(n_chars);

    // 1) 跳过紧邻空白簇
    while i > 0 {
        let p = grapheme_prev(text, i);
        let pb = char_to_byte(text, p);
        if !text[pb..].chars().next().unwrap_or('\0').is_whitespace() {
            break;
        }
        i = p;
    }
    if i == 0 {
        return 0;
    }
    // 2a) 紧邻是词字符：连续吞词簇；2b) 否则退一簇
    let p = grapheme_prev(text, i);
    let pb = char_to_byte(text, p);
    if is_word_char(text, pb) {
        let mut j = i;
        while j > 0 {
            let q = grapheme_prev(text, j);
            let qb = char_to_byte(text, q);
            if !is_word_char(text, qb) {
                break;
            }
            j = q;
        }
        j
    } else {
        p
    }
}

/// 后一个词边界（字符索引域，复刻 `Words.nextBoundary`）。
fn word_next(text: &str, ci: usize) -> usize {
    let n_chars = text.chars().count();
    let mut i = ci.min(n_chars);

    // 1) 跳过紧邻空白簇
    while i < n_chars {
        let ib = char_to_byte(text, i);
        if !text[ib..].chars().next().unwrap_or('\0').is_whitespace() {
            break;
        }
        i = grapheme_next(text, i);
    }
    if i >= n_chars {
        return n_chars;
    }
    // 2a) 词字符连续吞；2b) 否则一簇
    let ib = char_to_byte(text, i);
    if is_word_char(text, ib) {
        let mut j = i;
        while j < n_chars {
            let jb = char_to_byte(text, j);
            if !is_word_char(text, jb) {
                break;
            }
            j = grapheme_next(text, j);
        }
        j
    } else {
        grapheme_next(text, i)
    }
}

// ---------------------------------------------------------------------------
// Tab 展开（复刻 Kotlin `TabRender`）
// ---------------------------------------------------------------------------

const SPACES_PER_TAB: usize = 4;

fn logic_to_display(logic_pos: usize, text: &str) -> usize {
    let mut display = 0usize;
    for ch in text.chars().take(logic_pos) {
        display += if ch == '\t' { SPACES_PER_TAB } else { 1 };
    }
    display
}

fn display_to_logic(display_pos: usize, text: &str) -> usize {
    let mut disp = 0usize;
    let mut logic = 0usize;
    for ch in text.chars() {
        if disp >= display_pos {
            break;
        }
        disp += if ch == '\t' { SPACES_PER_TAB } else { 1 };
        logic += 1;
    }
    logic
}

// ---------------------------------------------------------------------------
// Undo/Redo（复刻 Kotlin `UndoRedo`）
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
struct TextState {
    text: String,
    cursor: usize,
}

trait TextEditAction: std::fmt::Debug {
    fn undo(&self, state: &TextState) -> TextState;
    fn redo(&self, state: &TextState) -> TextState;
}

#[derive(Debug)]
struct InsertTextAction {
    position: usize,
    inserted: String,
}

impl TextEditAction for InsertTextAction {
    fn undo(&self, state: &TextState) -> TextState {
        let new_text = remove_range(&state.text, self.position, self.position + self.inserted.len());
        TextState { text: new_text, cursor: self.position }
    }
    fn redo(&self, state: &TextState) -> TextState {
        let new_text = insert_at(&state.text, self.position, &self.inserted);
        TextState { text: new_text, cursor: self.position + self.inserted.len() }
    }
}

#[derive(Debug)]
struct DeleteTextAction {
    position: usize,
    deleted: String,
    is_backspace: bool,
}

impl TextEditAction for DeleteTextAction {
    fn undo(&self, state: &TextState) -> TextState {
        let new_text = insert_at(&state.text, self.position, &self.deleted);
        let cursor = if self.is_backspace {
            self.position
        } else {
            self.position + self.deleted.len()
        };
        TextState { text: new_text, cursor }
    }
    fn redo(&self, state: &TextState) -> TextState {
        let new_text = remove_range(&state.text, self.position, self.position + self.deleted.len());
        TextState { text: new_text, cursor: self.position }
    }
}

#[derive(Debug)]
struct ReplaceSelectionAction {
    position: usize,
    original_selected: String,
    replacement: String,
}

impl TextEditAction for ReplaceSelectionAction {
    fn undo(&self, state: &TextState) -> TextState {
        let head = &state.text[..slice_to_byte(&state.text, self.position)];
        let tail_start = slice_to_byte(&state.text, self.position + self.replacement.len());
        let tail = &state.text[tail_start.min(state.text.len())..];
        let mut new_text = String::with_capacity(head.len() + self.original_selected.len() + tail.len());
        new_text.push_str(head);
        new_text.push_str(&self.original_selected);
        new_text.push_str(tail);
        TextState { text: new_text, cursor: self.position }
    }
    fn redo(&self, state: &TextState) -> TextState {
        let head = &state.text[..slice_to_byte(&state.text, self.position)];
        let tail_start = slice_to_byte(&state.text, self.position + self.original_selected.len());
        let tail = &state.text[tail_start.min(state.text.len())..];
        let mut new_text = String::with_capacity(head.len() + self.replacement.len() + tail.len());
        new_text.push_str(head);
        new_text.push_str(&self.replacement);
        new_text.push_str(tail);
        TextState { text: new_text, cursor: self.position + self.replacement.len() }
    }
}

fn insert_at(text: &str, char_idx: usize, s: &str) -> String {
    let byte = char_to_byte(text, char_idx);
    let mut out = String::with_capacity(text.len() + s.len());
    out.push_str(&text[..byte]);
    out.push_str(s);
    out.push_str(&text[byte..]);
    out
}

fn remove_range(text: &str, start_char: usize, end_char: usize) -> String {
    let sb = char_to_byte(text, start_char);
    let eb = char_to_byte(text, end_char);
    let mut out = String::with_capacity(text.len() - (eb - sb));
    out.push_str(&text[..sb]);
    out.push_str(&text[eb..]);
    out
}

fn slice_to_byte(text: &str, char_idx: usize) -> usize {
    char_to_byte(text, char_idx)
}

fn slice_chars(text: &str, start: usize, end: usize) -> String {
    text.chars().skip(start).take(end - start).collect()
}

struct UndoRedo {
    max_history: usize,
    undo_stack: Vec<Box<dyn TextEditAction>>,
    redo_stack: Vec<Box<dyn TextEditAction>>,
}

impl UndoRedo {
    fn new(max_history: usize) -> Self {
        Self { max_history, undo_stack: Vec::new(), redo_stack: Vec::new() }
    }

    fn push(&mut self, action: Box<dyn TextEditAction>) {
        self.undo_stack.push(action);
        self.redo_stack.clear();
        if self.undo_stack.len() > self.max_history {
            self.undo_stack.remove(0);
        }
    }

    fn undo(&mut self, current: &TextState) -> Option<TextState> {
        let action = self.undo_stack.pop()?;
        let new_state = action.undo(current);
        self.redo_stack.push(action);
        Some(new_state)
    }

    fn redo(&mut self, current: &TextState) -> Option<TextState> {
        let action = self.redo_stack.pop()?;
        let new_state = action.redo(current);
        self.undo_stack.push(action);
        if self.undo_stack.len() > self.max_history {
            self.undo_stack.remove(0);
        }
        Some(new_state)
    }

    fn clear(&mut self) {
        self.undo_stack.clear();
        self.redo_stack.clear();
    }
}

// ---------------------------------------------------------------------------
// EditCore
// ---------------------------------------------------------------------------

/// 核心编辑态（复刻 Kotlin `EditableTextNode` 的纯逻辑层）。
///
/// 所有光标/选区位置使用**字符索引**；写入由 [`TextBuffer::write_text`]
/// 保证样式段自动对齐。组件层只通过查询方法读取显示文本/索引映射。
pub struct EditCore {
    buffer: TextBuffer,

    // 本地光标/选区：caretPair, (anchor, focus)，None = 未定位
    anchor: Option<usize>,
    focus: Option<usize>,

    // 显示
    placeholder: String,
    obscure_text: bool,

    // IME 组合态
    composing_text: String,
    composing_cursor_pos: usize,
    composition_base: Option<(usize, String)>,

    // 垂直导航保持列
    preferred_x: f32,

    // 撤销栈
    undo: UndoRedo,

    // 行为标志
    pub single_line: bool,
    pub max_history_size: usize,
}

impl EditCore {
    // -----------------------------------------------------------------------
    // 构造
    // -----------------------------------------------------------------------

    pub fn new() -> Self {
        Self::with_text("")
    }

    pub fn with_text(text: impl Into<String>) -> Self {
        Self {
            buffer: TextBuffer::from_plain(text),
            anchor: None,
            focus: None,
            placeholder: String::new(),
            obscure_text: false,
            composing_text: String::new(),
            composing_cursor_pos: 0,
            composition_base: None,
            preferred_x: f32::NAN,
            undo: UndoRedo::new(100),
            single_line: false,
            max_history_size: 100,
        }
    }

    pub fn buffer(&self) -> &TextBuffer { &self.buffer }

    pub fn buffer_mut(&mut self) -> &mut TextBuffer { &mut self.buffer }

    // -----------------------------------------------------------------------
    // 文本查询
    // -----------------------------------------------------------------------

    pub fn text(&self) -> &str { self.buffer.text() }

    pub fn text_len(&self) -> usize { self.buffer.text().chars().count() }

    // -----------------------------------------------------------------------
    // 光标/选区查询
    // -----------------------------------------------------------------------

    pub fn anchor(&self) -> Option<usize> { self.anchor }

    pub fn focus(&self) -> Option<usize> { self.focus }

    /// 当前插入点位置（字符索引）；未定位返回 0。
    pub fn cursor(&self) -> usize {
        self.anchor
            .unwrap_or(0)
            .min(self.text_len())
    }

    pub fn has_selection(&self) -> bool {
        self.anchor.zip(self.focus).is_some_and(|(a, f)| a != f)
    }

    pub fn sel_start(&self) -> usize {
        self.anchor.zip(self.focus)
            .map_or(0, |(a, f)| a.min(f).min(self.text_len()))
    }

    pub fn sel_end(&self) -> usize {
        self.anchor.zip(self.focus)
            .map_or(0, |(a, f)| a.max(f).min(self.text_len()))
    }

    /// 扩选起点：已定位用 anchor，否则塌缩为光标。
    fn sel_anchor(&self) -> usize {
        self.anchor.unwrap_or_else(|| self.cursor())
    }

    // -----------------------------------------------------------------------
    // 光标设置
    // -----------------------------------------------------------------------

    pub fn set_cursor(&mut self, idx: usize) {
        let c = idx.min(self.text_len());
        self.anchor = Some(c);
        self.focus = Some(c);
    }

    pub fn extend_to(&mut self, pos: usize) {
        let p = pos.min(self.text_len());
        let a = self.sel_anchor();
        self.anchor = Some(a);
        self.focus = Some(p);
    }

    pub fn move_to(&mut self, pos: usize, extend: bool) {
        if extend {
            self.extend_to(pos);
        } else {
            self.set_cursor(pos);
        }
    }

    pub fn collapse_selection(&mut self) {
        if self.has_selection() {
            let a = self.anchor.unwrap_or(0);
            self.focus = Some(a);
        }
    }

    // -----------------------------------------------------------------------
    // 核心写入
    // -----------------------------------------------------------------------

    fn apply_text(&mut self, new_text: String) {
        self.buffer.write_text(new_text);
    }

    // -----------------------------------------------------------------------
    // 编辑操作
    // -----------------------------------------------------------------------

    pub fn insert_text(&mut self, inserted: &str) {
        let to_insert = if self.single_line {
            inserted.replace(['\n', '\r'], "")
        } else {
            inserted.to_string()
        };
        if to_insert.is_empty() { return; }
        if self.has_selection() {
            self.replace_sel(&to_insert);
            return;
        }
        let pos = self.cursor();
        self.undo.push(Box::new(InsertTextAction {
            position: pos,
            inserted: to_insert.clone(),
        }));
        self.apply_text(insert_at(self.text(), pos, &to_insert));
        self.set_cursor(pos + to_insert.chars().count());
    }

    fn replace_sel(&mut self, replacement: &str) {
        let s = self.sel_start();
        let e = self.sel_end();
        if s == e {
            self.insert_text(replacement);
            return;
        }
        let original = slice_chars(self.text(), s, e);
        self.undo.push(Box::new(ReplaceSelectionAction {
            position: s,
            original_selected: original,
            replacement: replacement.to_string(),
        }));
        let mut new_text = String::with_capacity(self.text().len() - (e - s) + replacement.len());
        let sb = char_to_byte(self.text(), s);
        let eb = char_to_byte(self.text(), e);
        new_text.push_str(&self.text()[..sb]);
        new_text.push_str(replacement);
        new_text.push_str(&self.text()[eb..]);
        self.apply_text(new_text);
        self.set_cursor(s + replacement.chars().count());
    }

    pub fn backspace(&mut self) {
        if self.has_selection() {
            self.del_sel();
            return;
        }
        let pos = self.cursor();
        if pos == 0 { return; }
        let start = grapheme_prev(self.text(), pos);
        if start >= pos { return; }
        let deleted = slice_chars(self.text(), start, pos);
        self.undo.push(Box::new(DeleteTextAction {
            position: start,
            deleted,
            is_backspace: true,
        }));
        self.apply_text(remove_range(self.text(), start, pos));
        self.set_cursor(start);
    }

    pub fn delete(&mut self) {
        if self.has_selection() {
            self.del_sel();
            return;
        }
        let pos = self.cursor();
        if pos >= self.text_len() { return; }
        let end = grapheme_next(self.text(), pos);
        if end <= pos { return; }
        let deleted = slice_chars(self.text(), pos, end);
        self.undo.push(Box::new(DeleteTextAction {
            position: pos,
            deleted,
            is_backspace: false,
        }));
        self.apply_text(remove_range(self.text(), pos, end));
        self.set_cursor(pos);
    }

    fn del_sel(&mut self) {
        if !self.has_selection() { return; }
        let s = self.sel_start();
        let e = self.sel_end();
        let deleted = slice_chars(self.text(), s, e);
        self.undo.push(Box::new(DeleteTextAction {
            position: s,
            deleted,
            is_backspace: true,
        }));
        self.apply_text(remove_range(self.text(), s, e));
        self.set_cursor(s);
    }

    pub fn delete_word_backward(&mut self) {
        if self.has_selection() {
            self.del_sel();
            return;
        }
        let pos = self.cursor();
        if pos == 0 { return; }
        let start = word_prev(self.text(), pos);
        if start >= pos { return; }
        let deleted = slice_chars(self.text(), start, pos);
        self.undo.push(Box::new(DeleteTextAction {
            position: start,
            deleted,
            is_backspace: true,
        }));
        self.apply_text(remove_range(self.text(), start, pos));
        self.set_cursor(start);
        self.preferred_x = f32::NAN;
    }

    pub fn delete_word_forward(&mut self) {
        if self.has_selection() {
            self.del_sel();
            return;
        }
        let pos = self.cursor();
        if pos >= self.text_len() { return; }
        let end = word_next(self.text(), pos);
        if end <= pos { return; }
        let deleted = slice_chars(self.text(), pos, end);
        self.undo.push(Box::new(DeleteTextAction {
            position: pos,
            deleted,
            is_backspace: false,
        }));
        self.apply_text(remove_range(self.text(), pos, end));
        self.set_cursor(pos);
        self.preferred_x = f32::NAN;
    }

    // -----------------------------------------------------------------------
    // 导航（纯文本，无布局）
    // -----------------------------------------------------------------------

    pub fn move_left(&mut self) {
        if self.has_selection() {
            let s = self.sel_start();
            self.set_cursor(s);
            return;
        }
        let p = self.cursor();
        if p > 0 {
            self.set_cursor(grapheme_prev(self.text(), p));
        }
    }

    pub fn move_right(&mut self) {
        if self.has_selection() {
            let e = self.sel_end();
            self.set_cursor(e);
            return;
        }
        let p = self.cursor();
        if p < self.text_len() {
            self.set_cursor(grapheme_next(self.text(), p));
        }
    }

    pub fn select_left(&mut self) {
        let f = self.focus.unwrap_or(0).min(self.text_len());
        if f > 0 {
            let a = self.sel_anchor();
            self.anchor = Some(a);
            self.focus = Some(grapheme_prev(self.text(), f));
        }
    }

    pub fn select_right(&mut self) {
        let f = self.focus.unwrap_or(0).min(self.text_len());
        if f < self.text_len() {
            let a = self.sel_anchor();
            self.anchor = Some(a);
            self.focus = Some(grapheme_next(self.text(), f));
        }
    }

    pub fn select_all(&mut self) {
        self.anchor = Some(0);
        self.focus = Some(self.text_len());
    }

    pub fn select_range(&mut self, start: usize, end: usize) {
        self.anchor = Some(start.min(self.text_len()));
        self.focus = Some(end.min(self.text_len()));
    }

    pub fn move_prev_word(&mut self) {
        self.preferred_x = f32::NAN;
        self.set_cursor(word_prev(self.text(), self.cursor()));
    }

    pub fn move_next_word(&mut self) {
        self.preferred_x = f32::NAN;
        self.set_cursor(word_next(self.text(), self.cursor()));
    }

    pub fn select_prev_word(&mut self) {
        self.preferred_x = f32::NAN;
        let f = self.focus.unwrap_or(0).min(self.text_len());
        let a = self.sel_anchor();
        self.anchor = Some(a);
        self.focus = Some(word_prev(self.text(), f));
    }

    pub fn select_next_word(&mut self) {
        self.preferred_x = f32::NAN;
        let f = self.focus.unwrap_or(0).min(self.text_len());
        let a = self.sel_anchor();
        self.anchor = Some(a);
        self.focus = Some(word_next(self.text(), f));
    }

    // ---------- 行/文档级导航（需 layout 回调） ----------

    pub fn move_home(&mut self, line_range: Option<&dyn Fn(usize) -> Option<(usize, usize)>>) {
        self.preferred_x = f32::NAN;
        let lr = line_range.and_then(|f| f(self.cursor()));
        let target = lr.map_or(0, |(s, _)| s);
        self.set_cursor(target);
    }

    pub fn move_end(&mut self, line_range: Option<&dyn Fn(usize) -> Option<(usize, usize)>>) {
        self.preferred_x = f32::NAN;
        let lr = line_range.and_then(|f| f(self.cursor()));
        let target = lr.map_or(self.text_len(), |(_, e)| e);
        self.set_cursor(target);
    }

    pub fn select_home(&mut self, line_range: Option<&dyn Fn(usize) -> Option<(usize, usize)>>) {
        self.preferred_x = f32::NAN;
        let f = self.focus.unwrap_or(0).min(self.text_len());
        let lr = line_range.and_then(|func| func(f));
        let target = lr.map_or(0, |(s, _)| s);
        let a = self.sel_anchor();
        self.anchor = Some(a);
        self.focus = Some(target);
    }

    pub fn select_end(&mut self, line_range: Option<&dyn Fn(usize) -> Option<(usize, usize)>>) {
        self.preferred_x = f32::NAN;
        let f = self.focus.unwrap_or(0).min(self.text_len());
        let lr = line_range.and_then(|func| func(f));
        let target = lr.map_or(self.text_len(), |(_, e)| e);
        let a = self.sel_anchor();
        self.anchor = Some(a);
        self.focus = Some(target);
    }

    pub fn move_doc_start(&mut self) {
        self.preferred_x = f32::NAN;
        self.set_cursor(0);
    }

    pub fn move_doc_end(&mut self) {
        self.preferred_x = f32::NAN;
        self.set_cursor(self.text_len());
    }

    pub fn select_doc_start(&mut self) {
        self.preferred_x = f32::NAN;
        self.extend_to(0);
    }

    pub fn select_doc_end(&mut self) {
        self.preferred_x = f32::NAN;
        self.extend_to(self.text_len());
    }

    /// `move_to_position`：组件层计算好目标位置后统一定位（垂直导航/点击等）。
    pub fn move_to_position(&mut self, pos: usize, extend: bool) {
        self.move_to(pos, extend);
    }

    pub fn preferred_x(&self) -> f32 { self.preferred_x }

    pub fn set_preferred_x(&mut self, x: f32) { self.preferred_x = x; }

    // -----------------------------------------------------------------------
    // 撤销/重做
    // -----------------------------------------------------------------------

    pub fn can_undo(&self) -> bool { !self.undo.undo_stack.is_empty() }
    pub fn can_redo(&self) -> bool { !self.undo.redo_stack.is_empty() }

    pub fn undo(&mut self) {
        if self.in_composing() { return; }
        let current = TextState {
            text: self.text().to_string(),
            cursor: self.cursor(),
        };
        if let Some(state) = self.undo.undo(&current) {
            self.apply_text(state.text);
            self.set_cursor(state.cursor);
        }
    }

    pub fn redo(&mut self) {
        if self.in_composing() { return; }
        let current = TextState {
            text: self.text().to_string(),
            cursor: self.cursor(),
        };
        if let Some(state) = self.undo.redo(&current) {
            self.apply_text(state.text);
            self.set_cursor(state.cursor);
        }
    }

    pub fn clear_undo_history(&mut self) {
        self.undo.clear();
    }

    // -----------------------------------------------------------------------
    // IME 组合态（复刻 Kotlin `onComposing` / `endComposition`）
    // -----------------------------------------------------------------------

    pub fn in_composing(&self) -> bool {
        !self.composing_text.is_empty()
    }

    pub fn composing_text(&self) -> &str { &self.composing_text }

    pub fn composing_cursor_pos(&self) -> usize { self.composing_cursor_pos }

    /// 组合区间起点 = compositionBase.0 或当前光标；字符索引。
    pub fn composing_start(&self) -> usize {
        self.composition_base
            .as_ref()
            .map_or(self.cursor(), |(start, _)| *start)
            .min(self.text_len())
    }

    /// 组合区间长度 = composing_text 长度。
    pub fn composing_length(&self) -> usize {
        self.composing_text.chars().count()
    }

    /// 平台上报组合文本（复刻 `onComposing`）。
    pub fn on_composing(&mut self, text: &str, cursor_pos: usize) {
        if text.is_empty() {
            self.end_composition(true); // 空 preedit = 取消组合，还原原文
            return;
        }
        // absorbGlobalSelection：纯逻辑层无全局会话，未定位则从 0 开始
        if self.anchor.is_none() {
            self.anchor = Some(0);
            self.focus = Some(0);
        }
        let cur = self.text().to_string();
        let cur_len = cur.chars().count();
        let (start, win_len) = if let Some((ref base, ref _orig)) = self.composition_base {
            (*base, self.composing_text.chars().count())
        } else {
            let (s, e) = if self.has_selection() {
                (self.sel_start(), self.sel_end())
            } else {
                let c = self.cursor().min(cur_len);
                (c, c)
            };
            let orig = slice_chars(&cur, s, e);
            self.composition_base = Some((s, orig));
            (s, e - s)
        };
        let head_end = start.min(cur_len);
        let tail_start = (start + win_len).min(cur_len);
        let new_text = format!(
            "{}{}{}",
            slice_chars(&cur, 0, head_end),
            text,
            slice_chars(&cur, tail_start, cur_len),
        );
        self.apply_text(new_text);
        self.set_cursor(start + text.chars().count());
        self.composing_text = text.to_string();
        self.composing_cursor_pos = cursor_pos.min(text.chars().count());
    }

    /// 结束组合态。[restore]=true 还原被替换原文（取消语义）；false 仅清标记保留文本。
    pub fn end_composition(&mut self, restore: bool) {
        let base = self.composition_base.take();
        let win_len = self.composing_length();
        self.composing_text.clear();
        self.composing_cursor_pos = 0;
        self.preferred_x = f32::NAN;
        if !restore {
            return;
        }
        let (start, orig) = match base {
            Some((s, o)) => (s, o),
            None => return,
        };
        let cur = self.text().to_string();
        let cur_len = cur.chars().count();
        let tail_start = (start + win_len).min(cur_len);
        let new_text = format!(
            "{}{}{}",
            slice_chars(&cur, 0, start),
            orig,
            slice_chars(&cur, tail_start, cur_len),
        );
        if new_text != cur {
            self.apply_text(new_text);
        }
        self.set_cursor(start);
    }

    // -----------------------------------------------------------------------
    // 显示层：占位 / 掩码 / tab 展开
    // -----------------------------------------------------------------------

    pub fn showing_placeholder(&self) -> bool {
        self.text().is_empty() && !self.placeholder.is_empty()
    }

    pub fn placeholder(&self) -> &str { &self.placeholder }
    pub fn set_placeholder(&mut self, s: impl Into<String>) { self.placeholder = s.into(); }

    pub fn obscure_text(&self) -> bool { self.obscure_text }
    pub fn set_obscure_text(&mut self, v: bool) { self.obscure_text = v; }

    /// 显示文本：占位 / 掩码 / 普通（tab→4 空格）。
    pub fn display_text(&self) -> String {
        if self.showing_placeholder() {
            return self.placeholder.clone();
        }
        if self.obscure_text {
            let n = grapheme_count(self.text());
            return "•".repeat(n);
        }
        self.text().replace('\t', "    ")
    }

    /// 显示片段（掩码：逐簇圆点带段样式；占位：单片段；普通：buffer spans）。
    pub fn display_spans(&self, base_style: crate::TextStyle) -> Vec<TextSpan> {
        if self.showing_placeholder() {
            return vec![TextSpan::styled(self.placeholder.clone(), base_style)];
        }
        if self.obscure_text {
            return self.obscure_display_spans(base_style);
        }
        // 普通：复用 buffer.spans()，tab 已在 spans() 内替换
        let mut spans = self.buffer.spans();
        for span in &mut spans {
            if span.style == crate::TextStyle::default() {
                span.style = base_style.clone();
            }
        }
        spans
    }

    fn obscure_display_spans(&self, base_style: crate::TextStyle) -> Vec<TextSpan> {
        if self.text().is_empty() { return Vec::new(); }
        let mut out: Vec<TextSpan> = Vec::new();
        let mut ci = 0;
        while ci < self.text_len() {
            let st = self.buffer.style_at(ci).cloned().unwrap_or_else(|| base_style.clone());
            let last = out.last();
            if let Some(last_span) = last {
                if last_span.style == st {
                    if let Some(last_mut) = out.last_mut() {
                        last_mut.text.push('•');
                    }
                    ci = grapheme_next(self.text(), ci);
                    continue;
                }
            }
            out.push(TextSpan::styled("•".to_string(), st));
            ci = grapheme_next(self.text(), ci);
        }
        out
    }

    // -----------------------------------------------------------------------
    // 索引映射
    // -----------------------------------------------------------------------

    /// 显示索引 → 逻辑索引。
    pub fn display_to_logic_index(&self, display_pos: usize) -> usize {
        if self.showing_placeholder() {
            return 0;
        }
        if !self.obscure_text {
            return display_to_logic(display_pos, self.text());
        }
        // 掩码：第 k 个圆点 → 第 k 个字素簇起点（字符索引）
        let mut ci = 0;
        let mut n = 0;
        while ci < self.text_len() && n < display_pos {
            ci = grapheme_next(self.text(), ci);
            n += 1;
        }
        ci
    }

    /// 逻辑索引 → 显示索引。
    pub fn logic_to_display_index(&self, logic_pos: usize) -> usize {
        if self.showing_placeholder() {
            return 0;
        }
        if !self.obscure_text {
            return logic_to_display(logic_pos, self.text());
        }
        // 掩码：逻辑 pos 落在第几个簇里 → 那个簇的序号
        let limit = logic_pos.min(self.text_len());
        let mut ci = 0;
        let mut n = 0;
        while ci < limit {
            let next = grapheme_next(self.text(), ci);
            if next > limit {
                break;
            }
            ci = next;
            n += 1;
        }
        n
    }

    /// 组合区间（显示索引域）：`None` 表示非组合态。
    pub fn composing_display_range(&self) -> Option<(usize, usize)> {
        if !self.in_composing() { return None; }
        let s = self.logic_to_display_index(self.composing_start());
        let e = self.logic_to_display_index(self.composing_start() + self.composing_length());
        Some((s, e))
    }
}

impl Default for EditCore {
    fn default() -> Self { Self::new() }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn new_core(text: &str) -> EditCore {
        let mut c = EditCore::with_text(text);
        if !text.is_empty() {
            c.set_cursor(text.chars().count());
        }
        c
    }

    #[test]
    fn new_empty() {
        let c = EditCore::new();
        assert_eq!(c.text(), "");
        assert_eq!(c.cursor(), 0);
        assert!(!c.has_selection());
    }

    #[test]
    fn insert_text_basic() {
        let mut c = new_core("");
        c.insert_text("abc");
        assert_eq!(c.text(), "abc");
        assert_eq!(c.cursor(), 3);
    }

    #[test]
    fn insert_at_cursor() {
        let mut c = new_core("ac");
        c.set_cursor(1);
        c.insert_text("b");
        assert_eq!(c.text(), "abc");
        assert_eq!(c.cursor(), 2);
    }

    #[test]
    fn backspace_ascii() {
        let mut c = new_core("abc");
        c.set_cursor(3);
        c.backspace();
        assert_eq!(c.text(), "ab");
        assert_eq!(c.cursor(), 2);
    }

    #[test]
    fn delete_ascii() {
        let mut c = new_core("abc");
        c.set_cursor(1);
        c.delete();
        assert_eq!(c.text(), "ac");
        assert_eq!(c.cursor(), 1);
    }

    #[test]
    fn backspace_removes_grapheme() {
        // e + combining acute = 1 字素簇，随后是 x
        let mut c = new_core("e\u{301}x");
        c.set_cursor(2);
        c.backspace();
        assert_eq!(c.text(), "x");
    }

    #[test]
    fn backspace_removes_emoji() {
        let mut c = new_core("a😀b");
        let emoji_len = "a😀".chars().count();
        c.set_cursor(emoji_len);
        c.backspace();
        assert_eq!(c.text(), "ab");
    }

    #[test]
    fn replace_selection() {
        let mut c = new_core("abcd");
        c.select_range(1, 3);
        assert!(c.has_selection());
        c.insert_text("XY");
        assert_eq!(c.text(), "aXYd");
        assert_eq!(c.cursor(), 3);
    }

    #[test]
    fn select_all_insert() {
        let mut c = new_core("hello");
        c.select_all();
        c.insert_text("world");
        assert_eq!(c.text(), "world");
    }

    #[test]
    fn undo_insert() {
        let mut c = new_core("ab");
        c.set_cursor(2);
        c.insert_text("c");
        assert_eq!(c.text(), "abc");
        c.undo();
        assert_eq!(c.text(), "ab");
        assert_eq!(c.cursor(), 2);
    }

    #[test]
    fn undo_delete() {
        let mut c = new_core("abc");
        c.set_cursor(2);
        c.backspace(); // 删 index 1('b') → "ac"，cursor=1
        assert_eq!(c.text(), "ac");
        assert_eq!(c.cursor(), 1);
        c.undo(); // 恢复 "abc"，is_backspace → cursor=1
        assert_eq!(c.text(), "abc");
        assert_eq!(c.cursor(), 1);
    }

    #[test]
    fn undo_replace_sel() {
        let mut c = new_core("abcd");
        c.select_range(1, 3);
        c.insert_text("X");
        assert_eq!(c.text(), "aXd");
        assert_eq!(c.cursor(), 2);
        c.undo(); // 恢复 "abcd"，cursor=1
        assert_eq!(c.text(), "abcd");
        assert_eq!(c.cursor(), 1);
    }

    #[test]
    fn redo_insert() {
        let mut c = new_core("");
        c.insert_text("hi");
        assert_eq!(c.text(), "hi");
        c.undo();
        assert_eq!(c.text(), "");
        c.redo();
        assert_eq!(c.text(), "hi");
        assert_eq!(c.cursor(), 2);
    }

    #[test]
    fn redo_overrides_future_on_new_edit() {
        let mut c = new_core("a");
        c.insert_text("b");
        c.undo();
        c.insert_text("c");
        c.redo(); // 无 effect
        assert_eq!(c.text(), "ac");
    }

    #[test]
    fn word_prev_next_basic() {
        // Kotlin Words.prevBoundary: 跳空白后连续吞词簇
        assert_eq!(word_prev("hello world", 6), 0); // 'w' 退到文档首
        assert_eq!(word_next("hello", 0), 5);       // hello 整词到末尾
        assert_eq!(word_prev("hello", 5), 0);       // 从末尾退到文档首
    }

    #[test]
    fn word_nav_cjk() {
        // CJK 被 is_alphanumeric 视为词字符，连续吞
        assert_eq!(word_next("你好世界", 0), 4); // '你' 是词字符，连续吞到文档尾
        assert_eq!(word_prev("你好世界", 2), 0); // 连续吞到文档首
    }

    #[test]
    fn move_prev_next_word_core() {
        let mut c = new_core("hello world foo");
        c.set_cursor(6);
        c.move_prev_word();
        // Kotlin: 跳空白(5) → 连续吞词簇 → 到文档首 0
        assert_eq!(c.cursor(), 0);
        c.move_next_word();
        assert_eq!(c.cursor(), 5);
        c.move_next_word();
        assert_eq!(c.cursor(), 11);
    }

    #[test]
    fn logic_display_roundtrip() {
        let s = "ab\tcd";
        assert_eq!(logic_to_display(0, s), 0);
        assert_eq!(logic_to_display(2, s), 2);
        assert_eq!(logic_to_display(3, s), 6); // tab = 4 spaces
        assert_eq!(display_to_logic(5, s), 3);
        assert_eq!(display_to_logic(2, s), 2);
    }

    #[test]
    fn obscure_display_text() {
        let mut c = new_core("abc");
        c.set_obscure_text(true);
        assert_eq!(c.display_text(), "•••");
    }

    #[test]
    fn obscure_display_spans_merge_adjacent_same_style() {
        let mut c = new_core("ab");
        c.set_obscure_text(true);
        let spans = c.display_spans(crate::TextStyle::default());
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].text, "••");
    }

    #[test]
    fn display_logic_index_roundtrip_normal() {
        let c = new_core("a\tb");
        assert_eq!(c.display_to_logic_index(0), 0);
        assert_eq!(c.display_to_logic_index(1), 1);
        assert_eq!(c.display_to_logic_index(4), 2);
        assert_eq!(c.logic_to_display_index(2), 5);
    }

    #[test]
    fn display_logic_index_roundtrip_obscure() {
        let mut c = new_core("abcd");
        c.set_obscure_text(true);
        assert_eq!(c.display_to_logic_index(2), 2);
        assert_eq!(c.logic_to_display_index(2), 2);
    }

    #[test]
    fn placeholder_shows() {
        let mut c = new_core("");
        c.set_placeholder("Type here");
        assert!(c.showing_placeholder());
        assert_eq!(c.display_text(), "Type here");
        let spans = c.display_spans(crate::TextStyle::default());
        assert_eq!(spans[0].text, "Type here");
    }

    #[test]
    fn composing_inserts_and_removes() {
        let mut c = new_core("");
        c.set_cursor(0);
        c.on_composing("ni", 1);
        assert!(c.in_composing());
        assert_eq!(c.text(), "ni");
        c.end_composition(false); // 确认提交
        assert_eq!(c.text(), "ni");
        assert!(!c.in_composing());
    }

    #[test]
    fn composing_cancel_restores() {
        let mut c = new_core("");
        c.set_cursor(0);
        c.on_composing("hao", 0);
        assert_eq!(c.text(), "hao");
        c.end_composition(true); // 取消还原
        assert_eq!(c.text(), "");
    }

    #[test]
    fn composing_replaces_selection() {
        let mut c = new_core("hello");
        c.select_all();
        c.on_composing("hi", 1);
        assert_eq!(c.text(), "hi");
        c.end_composition(false);
        assert_eq!(c.text(), "hi");
    }

    #[test]
    fn composing_update_window() {
        let mut c = new_core("");
        c.set_cursor(0);
        c.on_composing("n", 1);
        assert_eq!(c.text(), "n");
        c.on_composing("ni", 2);
        assert_eq!(c.text(), "ni");
        assert_eq!(c.composing_cursor_pos(), 2);
    }

    #[test]
    fn single_line_strips_newline() {
        let mut c = new_core("");
        c.single_line = true;
        c.insert_text("a\nb\r\nc");
        assert_eq!(c.text(), "abc");
    }

    #[test]
    fn doc_start_end() {
        let mut c = new_core("hello");
        c.move_doc_end();
        assert_eq!(c.cursor(), 5);
        c.move_doc_start();
        assert_eq!(c.cursor(), 0);
    }

    #[test]
    fn select_all_then_backspace() {
        let mut c = new_core("word");
        c.select_all();
        c.backspace();
        assert_eq!(c.text(), "");
        assert_eq!(c.cursor(), 0);
    }

    #[test]
    fn undo_count_respects_max() {
        let mut c = EditCore::new();
        c.max_history_size = 3;
        c.undo.max_history = 3;
        for i in 0..5 {
            c.set_cursor(c.text_len());
            c.insert_text(&i.to_string());
        }
        for _ in 0..3 {
            c.undo();
        }
        assert!(!c.can_undo());
    }
}