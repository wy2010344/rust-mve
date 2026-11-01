//! 显示层 tab 展开（对标 TS `canvasRender2/TabRender.ts`）。
//!
//! 段落（Parley / CanvasKit）对 `'\t'` 的宽度是未定义的，因此显示文本统一
//! 展开为空格。
//!
//! **索引换算不在这里**：逻辑索引（模型文本，含 `'\t'`）↔ 显示索引（展开后）
//! 是编辑器的职责，已随编辑态一起收进 `wy_text::EditCore`
//! （`logic_to_display_index` / `display_to_logic_index`）。这里只留下"文本怎么
//! 展开"这一件事，供富文本节点替换 tab 使用。
//!
//! 早先这里还有 `logic_to_display` / `display_to_logic` 两个函数，它们用
//! `text.len()`（**字节**数）当字符索引的上界去切 `chars[..limit]`，多字节
//! 文本会越界 panic；随统一编辑内核一并删除，不再有第二份换算。

/// 单个 tab 展开的等效空格数（固定 4，对标 Kotlin `SPACES_PER_TAB`）。
pub const SPACES_PER_TAB: usize = 4;

/// 显示层 tab 展开：逐字符替换。
#[must_use]
pub fn expand_tabs(text: &str) -> String {
    text.replace('\t', " ".repeat(SPACES_PER_TAB).as_str())
}
