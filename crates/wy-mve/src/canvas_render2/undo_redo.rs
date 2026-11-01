//! 文本编辑撤销/重做（对标 TS `canvasRender2/UndoRedo.ts`，实现抽自 `canvasCore/UndoRedo`）。
//!
//! 动作式栈：每次编辑 push 一个 [`TextAction`]；undo 取逆、redo 重放。
//! 索引空间为**字符（Unicode 标量）索引**（与 `Graphemes` / `Words` 一致）。

/// 字符索引 → 字节偏移。
fn char_to_byte(text: &str, char_idx: usize) -> usize {
    text.char_indices()
        .nth(char_idx)
        .map_or(text.len(), |(b, _)| b)
}

/// 文本快照（文本 + 光标位置）：undo/redo 的输入与输出。
///
/// 对标 TS `TextState`。
#[derive(Clone, Debug, PartialEq)]
pub struct TextState {
    /// 文本。
    pub text: String,
    /// 光标字符索引。
    pub cursor: usize,
}

/// 文本编辑动作（对标 TS `InsertTextAction` / `DeleteTextAction` / `ReplaceSelectionAction`）。
pub trait TextAction {
    /// 正向重放：基于当前状态返回新状态。
    fn redo(&self, s: &TextState) -> TextState;
    /// 反向撤销：基于当前状态返回新状态。
    fn undo(&self, s: &TextState) -> TextState;
}

/// 插入文本 `text`（位置 `pos`，字符索引）。
///
/// 对标 TS `InsertTextAction`。
pub struct InsertTextAction {
    /// 插入位置（字符索引）。
    pub pos: usize,
    /// 插入的文本。
    pub text: String,
}

impl TextAction for InsertTextAction {
    fn redo(&self, s: &TextState) -> TextState {
        let mut t = s.text.clone();
        let at = self.pos.min(t.chars().count());
        let byte = char_to_byte(&t, at);
        t.insert_str(byte, &self.text);
        TextState {
            text: t,
            cursor: at + self.text.chars().count(),
        }
    }

    fn undo(&self, s: &TextState) -> TextState {
        let mut t = s.text.clone();
        let at = self.pos.min(t.chars().count());
        let end_char = (at + self.text.chars().count()).min(t.chars().count());
        let start = char_to_byte(&t, at);
        let end = char_to_byte(&t, end_char);
        // 漂移保护：仅当该区间确实等于原插入文本时才移除。
        if t[start..end] == self.text {
            t.replace_range(start..end, "");
        }
        TextState {
            text: t,
            cursor: at,
        }
    }
}

/// 删除文本（位置 `start`，删除 `deleted`）。
///
/// 对标 TS `DeleteTextAction`；`is_forward` 决定撤销后光标落点。
pub struct DeleteTextAction {
    /// 删除起点（字符索引）。
    pub start: usize,
    /// 被删除的文本。
    pub deleted: String,
    /// 是否是前向删除（Delete 键）；后向（Backspace）为 false。
    pub is_forward: bool,
}

impl TextAction for DeleteTextAction {
    fn redo(&self, s: &TextState) -> TextState {
        let mut t = s.text.clone();
        let start = self.start.min(t.chars().count());
        let end_char = (start + self.deleted.chars().count()).min(t.chars().count());
        let bstart = char_to_byte(&t, start);
        let bend = char_to_byte(&t, end_char);
        if t[bstart..bend] == self.deleted {
            t.replace_range(bstart..bend, "");
        }
        TextState {
            text: t,
            cursor: start,
        }
    }

    fn undo(&self, s: &TextState) -> TextState {
        let mut t = s.text.clone();
        let start = self.start.min(t.chars().count());
        let bstart = char_to_byte(&t, start);
        t.insert_str(bstart, &self.deleted);
        let cursor = if self.is_forward {
            start + self.deleted.chars().count()
        } else {
            start
        };
        TextState { text: t, cursor }
    }
}

/// 替换选区（`start..start+deleted.len()` → `inserted`）。
///
/// 对标 TS `ReplaceSelectionAction`。
pub struct ReplaceSelectionAction {
    /// 选区起点（字符索引）。
    pub start: usize,
    /// 被替换掉的原文。
    pub deleted: String,
    /// 替换进去的新文。
    pub inserted: String,
}

impl TextAction for ReplaceSelectionAction {
    fn redo(&self, s: &TextState) -> TextState {
        let mut t = s.text.clone();
        let start = self.start.min(t.chars().count());
        let end_char = (start + self.deleted.chars().count()).min(t.chars().count());
        let bstart = char_to_byte(&t, start);
        let bend = char_to_byte(&t, end_char);
        if t[bstart..bend] == self.deleted {
            t.replace_range(bstart..bend, &self.inserted);
        }
        TextState {
            text: t,
            cursor: start + self.inserted.chars().count(),
        }
    }

    fn undo(&self, s: &TextState) -> TextState {
        let mut t = s.text.clone();
        let start = self.start.min(t.chars().count());
        let end_char = (start + self.inserted.chars().count()).min(t.chars().count());
        let bstart = char_to_byte(&t, start);
        let bend = char_to_byte(&t, end_char);
        if t[bstart..bend] == self.inserted {
            t.replace_range(bstart..bend, &self.deleted);
        }
        TextState {
            text: t,
            cursor: start + self.deleted.chars().count(),
        }
    }
}

/// 撤销/重做栈（动作式，对标 TS `UndoRedo`）。
pub struct UndoRedo {
    max: usize,
    undo_stack: Vec<Box<dyn TextAction>>,
    redo_stack: Vec<Box<dyn TextAction>>,
}

impl UndoRedo {
    /// 创建栈（容量 `max`）。
    #[must_use]
    pub fn new(max: usize) -> Self {
        Self {
            max,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
        }
    }

    /// 压入一个动作：清空重做栈（标准行为）。
    pub fn push(&mut self, action: impl TextAction + 'static) {
        self.undo_stack.push(Box::new(action));
        if self.undo_stack.len() > self.max {
            self.undo_stack.remove(0);
        }
        self.redo_stack.clear();
    }

    /// 撤销：弹出最后一个动作，取其逆，并转入重做栈。
    #[must_use]
    pub fn undo(&mut self, state: &TextState) -> Option<TextState> {
        let action = self.undo_stack.pop()?;
        let new = action.undo(state);
        self.redo_stack.push(action);
        Some(new)
    }

    /// 重做：弹出重做栈动作，重放，并转回撤销栈。
    #[must_use]
    pub fn redo(&mut self, state: &TextState) -> Option<TextState> {
        let action = self.redo_stack.pop()?;
        let new = action.redo(state);
        self.undo_stack.push(action);
        Some(new)
    }

    /// 是否可撤销。
    #[must_use]
    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }

    /// 是否可重做。
    #[must_use]
    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_then_undo_redo() {
        let mut ur = UndoRedo::new(100);
        let mut s = TextState {
            text: "ab".into(),
            cursor: 2,
        };
        // 编辑器先提交插入：文本变为 "aXb"。
        s = InsertTextAction {
            pos: 1,
            text: "X".into(),
        }
        .redo(&s);
        assert_eq!(s.text, "aXb");
        assert_eq!(s.cursor, 2);
        ur.push(InsertTextAction {
            pos: 1,
            text: "X".into(),
        });
        s = ur.undo(&s).unwrap();
        assert_eq!(s.text, "ab");
        assert_eq!(s.cursor, 1);
        s = ur.redo(&s).unwrap();
        assert_eq!(s.text, "aXb");
        assert_eq!(s.cursor, 2);
    }

    #[test]
    fn delete_then_undo() {
        let mut ur = UndoRedo::new(100);
        let mut s = TextState {
            text: "abc".into(),
            cursor: 3,
        };
        // 编辑器先提交删除：删掉索引 1 的 'b'，文本变为 "ac"。
        s = DeleteTextAction {
            start: 1,
            deleted: "b".into(),
            is_forward: false,
        }
        .redo(&s);
        assert_eq!(s.text, "ac");
        assert_eq!(s.cursor, 1);
        ur.push(DeleteTextAction {
            start: 1,
            deleted: "b".into(),
            is_forward: false,
        });
        s = ur.undo(&s).unwrap();
        assert_eq!(s.text, "abc");
        assert_eq!(s.cursor, 1);
    }

    #[test]
    fn replace_then_undo() {
        let mut ur = UndoRedo::new(100);
        let mut s = TextState {
            text: "hello".into(),
            cursor: 5,
        };
        // 编辑器先提交替换：文本变为 "hi"。
        s = ReplaceSelectionAction {
            start: 0,
            deleted: "hello".into(),
            inserted: "hi".into(),
        }
        .redo(&s);
        assert_eq!(s.text, "hi");
        assert_eq!(s.cursor, 2);
        ur.push(ReplaceSelectionAction {
            start: 0,
            deleted: "hello".into(),
            inserted: "hi".into(),
        });
        s = ur.undo(&s).unwrap();
        assert_eq!(s.text, "hello");
        assert_eq!(s.cursor, 5);
    }
}
