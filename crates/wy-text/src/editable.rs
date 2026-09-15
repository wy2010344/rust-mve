//! 可编辑文本段落：封装 Parley `PlainEditor`（纯文本单样式编辑内核）。
//!
//! 对应 Kotlin `EditableTextNode` 的**编辑内核**（光标/选区/IME/导航/删除），
//! 不含节点/绘制/样式，由 `wy-mve` 的 `TextField` 组件薄壳配合使用。
//!
//! # 能力（对齐 Kotlin `EditableTextNode`）
//! - 插入 / 退格 / 删除（按字素簇）
//! - 光标移动：左/右/词/行首尾（Home/End）/文档首尾 / Ctrl 词跳
//! - Shift 扩选 / Ctrl+A 全选 / 塌缩选区
//! - IME 组合态：`set_compose` / `finish_compose` / `clear_compose`
//! - 点击/拖动定位与扩选：`move_to_point` / `extend_selection_to_point`
//! - 选区/光标几何供绘制：`selection_geometry` / `cursor_geometry` / `ime_cursor_area`
//! - AccessKit 原生同步（`parley/accesskit` feature）
//!
//! Parley `PlainEditor` 内部持有 `String` buffer + `Layout`，是编辑状态的**唯一真源**；
//! 外部 signal 通过 [`EditableParagraph::snapshot`]（编辑后写回）与
//! [`EditableParagraph::replace_with`]（外部变化重灌）双向同步。

use std::cell::RefCell;

use parley::editing::{PlainEditor, PlainEditorDriver};
use parley::{LayoutContext, FontContext as ParleyFontContext};

/// 段落 `Brush` 类型（与 `wy-text` 排版一致）。
type Brush = [u8; 4];

// 全局编辑上下文（thread_local）：字体库 + 排版 scratch space。两个独立 RefCell
// 以便 `PlainEditor::driver` 同时借用率（同生命周期）不冲突地拆分。
thread_local! {
    static FONT_CX: RefCell<ParleyFontContext> = RefCell::new(ParleyFontContext::new());
    static LAYOUT_CX: RefCell<LayoutContext<Brush>> = RefCell::new(LayoutContext::new());
}

/// 编辑段落整体状态快照（供外部 signal 同步用）。
#[derive(Clone, Debug, PartialEq)]
pub struct EditSnapshot {
    /// 当前文本（不含 IME 组合文本）。
    pub text: String,
    /// 选区锚点（字节）。
    pub anchor: usize,
    /// 选区焦点 / 光标（字节）。
    pub cursor: usize,
}

/// 光标矩形（本地坐标，相对编辑器排版原点）。
pub type Rect4 = (f32, f32, f32, f32);

fn to_rect(bb: parley::BoundingBox) -> Rect4 {
    (bb.x0 as f32, bb.y0 as f32, bb.x1 as f32, bb.y1 as f32)
}

/// 可编辑段落：封装 Parley `PlainEditor`。
pub struct EditableParagraph {
    editor: PlainEditor<Brush>,
    /// 排版宽度（`None` = 不换行）。
    width: Option<f32>,
}

impl EditableParagraph {
    /// 创建空的可编辑段落。
    ///
    /// 字号在构造时定死（Parley 不支持建后改字号，只能 `set_scale` 整体缩放）。
    /// 需要不同字号请用不同实例。
    pub fn new(font_size: f32) -> Self {
        Self {
            editor: PlainEditor::new(font_size),
            width: None,
        }
    }

    /// 设置排版宽度（`None` = 不换行）。
    pub fn set_width(&mut self, width: Option<f32>) {
        self.width = width;
        self.editor.set_width(width);
    }

    /// 排版宽度。
    pub fn width(&self) -> Option<f32> {
        self.width
    }

    /// 当前字体大小（构造时的值）。
    pub fn font_size(&self) -> f32 {
        self.editor.get_font_size()
    }

    /// 当前文本（不含 IME 组合文本）。
    pub fn text(&self) -> String {
        self.editor.text().to_string()
    }

    /// 原始缓冲文本（**含** IME 组合文本）。
    pub fn raw_text(&self) -> &str {
        self.editor.raw_text()
    }

    /// 光标位置（字节，focus）。
    pub fn cursor(&self) -> usize {
        self.editor.raw_selection().focus().index()
    }

    /// 选区锚点（字节）。
    pub fn anchor(&self) -> usize {
        self.editor.raw_selection().anchor().index()
    }

    /// 排序后的选区 `(start, end)`；塌缩时 `start == end`。
    pub fn selection_range(&self) -> (usize, usize) {
        let range = self.editor.raw_selection().text_range();
        (range.start, range.end)
    }

    /// 是否正在 IME 组合。
    pub fn is_composing(&self) -> bool {
        self.editor.is_composing()
    }

    /// 组合文本（若在组合）。
    pub fn composing_text(&self) -> String {
        self.editor
            .raw_compose()
            .as_ref()
            .and_then(|r| self.editor.raw_text().get(r.clone()))
            .unwrap_or_default()
            .to_string()
    }

    /// 组合文本的字节区间（若在组合）。
    pub fn composing_range(&self) -> Option<std::ops::Range<usize>> {
        self.editor.raw_compose().clone()
    }

    /// 用外部文本**整体重灌**（外部 signal 变化时调用）。
    ///
    /// Parley 的 `set_text` 重置 buffer、选区塌缩到 0。
    pub fn replace_with(&mut self, text: &str) {
        self.editor.set_text(text);
    }

    /// 在一个编辑操作闭包内执行驱动操作（插入/删除/移动/选区/IME）。
    ///
    /// 闭包借用 `PlainEditorDriver`，结束后归还编辑状态。
    ///
    /// # Panics
    /// 若编辑期间嵌套调用 `with_driver`（同一线程二次借用字体上下文）。
    pub fn with_driver<R>(&mut self, f: impl FnOnce(PlainEditorDriver<'_, Brush>) -> R) -> R {
        FONT_CX.with(|fc| {
            LAYOUT_CX.with(|lc| {
                let mut fc = fc.borrow_mut();
                let mut lc = lc.borrow_mut();
                let drv = self.editor.driver(&mut fc, &mut lc);
                f(drv)
            })
        })
    }

    /// 编辑后同步外部：当前 `(text, anchor, cursor)` 快照。
    pub fn snapshot(&self) -> EditSnapshot {
        EditSnapshot {
            text: self.text(),
            anchor: self.anchor(),
            cursor: self.cursor(),
        }
    }

    /// 刷新排版（buffer 修改后强制造型；draw 前调用确保几何最新）。
    pub fn refresh_layout(&mut self) {
        FONT_CX.with(|fc| {
            LAYOUT_CX.with(|lc| {
                let mut fc = fc.borrow_mut();
                let mut lc = lc.borrow_mut();
                let mut drv = self.editor.driver(&mut fc, &mut lc);
                drv.refresh_layout();
            })
        });
    }

    /// 选区几何（本地坐标矩形列表，供高亮绘制）。
    pub fn selection_geometry(&self) -> Vec<Rect4> {
        self.editor
            .selection_geometry()
            .iter()
            .map(|(bb, _)| to_rect(*bb))
            .collect()
    }

    /// 光标矩形（本地坐标）；无光标（如组合态隐藏）返回 `None`。
    pub fn cursor_geometry(&self) -> Option<Rect4> {
        self.editor.cursor_geometry(1.0).map(to_rect)
    }

    /// IME 候选框排除矩形（相对编辑器左上）。
    pub fn ime_cursor_area(&self) -> Rect4 {
        to_rect(self.editor.ime_cursor_area())
    }

    /// 将本地坐标定位光标（点击命中）；`shift` 为真时扩选。
    pub fn move_to_point(&mut self, x: f32, y: f32, shift: bool) {
        if shift {
            self.with_driver(|mut drv| drv.extend_selection_to_point(x, y));
        } else {
            self.with_driver(|mut drv| drv.move_to_point(x, y));
        }
    }
}

impl Default for EditableParagraph {
    fn default() -> Self {
        Self::new(14.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_paragraph_is_empty() {
        let ed = EditableParagraph::new(14.0);
        assert_eq!(ed.text(), "");
        assert_eq!(ed.cursor(), 0);
        assert_eq!(ed.font_size(), 14.0);
    }

    #[test]
    fn replace_with_sets_text_and_resets_cursor() {
        let mut ed = EditableParagraph::new(14.0);
        ed.replace_with("hello");
        assert_eq!(ed.text(), "hello");
        assert_eq!(ed.cursor(), 0);
    }

    #[test]
    fn insert_at_cursor_via_driver() {
        let mut ed = EditableParagraph::new(14.0);
        ed.replace_with("ab");
        ed.with_driver(|mut drv| drv.move_to_byte(1));
        assert_eq!(ed.cursor(), 1);
        ed.with_driver(|mut drv| drv.insert_or_replace_selection("X"));
        assert_eq!(ed.text(), "aXb");
        assert_eq!(ed.cursor(), 2);
    }

    #[test]
    fn backspace_deletes_grapheme() {
        let mut ed = EditableParagraph::new(14.0);
        ed.replace_with("a😀b");
        // a(1) + 😀(4) = 5 字节处是 b 前
        ed.with_driver(|mut drv| drv.move_to_byte(5));
        ed.with_driver(|mut drv| drv.backdelete());
        assert_eq!(ed.text(), "ab");
    }

    #[test]
    fn snapshot_reports_text_and_cursor() {
        let mut ed = EditableParagraph::new(14.0);
        ed.replace_with("hello");
        ed.with_driver(|mut drv| drv.move_to_byte(3));
        let snap = ed.snapshot();
        assert_eq!(snap.text, "hello");
        assert_eq!(snap.cursor, 3);
        assert_eq!(snap.anchor, 3);
    }

    #[test]
    fn selection_geometry_available() {
        let mut ed = EditableParagraph::new(14.0);
        ed.replace_with("hello world");
        ed.with_driver(|mut drv| drv.select_byte_range(0, 5));
        let (a, b) = ed.selection_range();
        assert_eq!((a, b), (0, 5));
        let geo = ed.selection_geometry();
        assert!(!geo.is_empty());
    }

    #[test]
    fn composing_state_tracks_ime() {
        let mut ed = EditableParagraph::new(14.0);
        ed.replace_with("a");
        ed.with_driver(|mut drv| drv.move_to_byte(1));
        ed.with_driver(|mut drv| drv.set_compose("中", Some((3, 3))));
        assert!(ed.is_composing());
        assert_eq!(ed.composing_text(), "中");
        ed.with_driver(|mut drv| drv.finish_compose());
        assert!(!ed.is_composing());
        assert_eq!(ed.text(), "a中");
        // a(1) + 中(3) = 4
        assert_eq!(ed.cursor(), 4);
    }

    #[test]
    fn clear_compose_removes_preedit() {
        let mut ed = EditableParagraph::new(14.0);
        ed.replace_with("a");
        ed.with_driver(|mut drv| drv.move_to_byte(1));
        ed.with_driver(|mut drv| drv.set_compose("中", Some((2, 2))));
        ed.with_driver(|mut drv| drv.clear_compose());
        assert!(!ed.is_composing());
        assert_eq!(ed.text(), "a");
        assert_eq!(ed.cursor(), 1);
    }
}